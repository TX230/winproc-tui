# Process Investigation

This document defines the ownership and lifecycle of System Info and Process Info investigations. Field meanings and display formats remain in [metrics.md](metrics.md); .NET runtime sampling internals remain in [.NET Runtime Metrics Collection](dotnet-metrics-collection.md).

## System Info

System Info describes the current host rather than a historical sample. Windows product metadata is captured once during startup, while memory, GPU, disk, and CPU capacity values come from the latest Live `Snapshot`.

Display pause and Log view do not replace those host-capacity fields with paused or recorded values. Opening System Info performs no new collection. One ordered field model supplies both rendered rows and complete clipboard output, so terminal clipping never truncates copied data.

## Fixed Process Target

Opening Process Info creates a `ProcessInfoDialogTarget` containing the selected `ProcessIdentity`, opening `ProcessRow`, and lifecycle. Every tab and worker request uses that fixed target instead of consulting the current Processes selection again.

The active tab is retained between ordinary opens. A direct investigation action can select a specific tab for the new dialog session. Opening a new session clears session-local filters, while tab switches and explicit refreshes preserve them.

All live collectors verify that the process still has the expected identity. A PID that exits or is reused must never deliver information to the open dialog.

## Tab Collection Boundaries

| Tab | Data source and lifecycle |
|---|---|
| Metrics | Uses the fixed process identity and its Live, paused, or loaded history. |
| Image | Starts background static-process collection after the target is stable; recorded row data is available as a fallback. |
| Files | Explicitly enumerates open disk files on an independent worker. |
| DLLs | Takes an explicit module and file-metadata snapshot on its own worker. |
| Environment | Reads the live target's remote environment block on an independent worker and clears values when the dialog closes. |

Image, Files, DLL, and Environment collection never runs as part of ordinary sampling. Each request carries the dialog generation; refreshable tabs also carry request IDs. Results from a closed, reopened, or superseded dialog are rejected even if they refer to the same PID.

Image collection may inspect loaded `coreclr.dll` or `clr.dll` to report the active .NET runtime version. This does not add module enumeration to normal sampling or Recording.

## Open Files and DLLs

Open Files lists disk files currently open by the fixed live process. It is not a general handle browser for pipes, sockets, registry keys, synchronization objects, or every Windows handle type.

DLL collection is an explicit point-in-time Toolhelp snapshot. File metadata failures remain per-row unavailable values rather than failing the whole list. Files and DLL filters search complete displayed paths, and explicit refresh must not queue redundant work for the same dialog session.

Both collectors run outside the UI and sampling threads. Process identity is checked before results are accepted.

## Environment

Environment is a best-effort Windows 11 x64 investigation action. The worker handles native x64 and WOW64 pointer widths, validates remote-memory regions, enforces a 4 MiB limit, and requires valid terminated UTF-16 data.

Environment values may contain passwords, tokens, or other secrets. They remain in dialog-owned memory, are cleared when Process Info closes, and never enter status text, error text, Recording, exported data, or Log view.

## Log View and A/B Data

Log view never starts live Image, Files, DLL, or Environment workers. Metrics and recorded Image fields use loaded data when present; dynamic tabs show that their data was not recorded.

Process Info comparisons resolve A, B, and displayed-current values by exact `ProcessIdentity` and exact `captured_at`. Nearby samples, the latest Ghost Row value, and samples from a reused PID are not substituted. A delta is calculated only when both exact values exist.

## Input and Layout Boundaries

The dialog's tab, content, detail, and scrollbar hit regions are derived from the same responsive layout used for drawing. Clicking outside the modal neither dismisses it nor operates underlying panels.

Passive tabs keep navigation on their content without creating a false focus stop. Interactive tabs separate tab selection from selectable or filterable content. Exact keys and footer guidance remain owned by the in-app Help, dialog implementation, and rendering tests.

## Invariants

- Every tab uses the process identity fixed when the dialog opened.
- Asynchronous results must match both target identity and dialog generation.
- Blocking process-specific collection never runs on the UI or sampling thread.
- Log view never starts live process-investigation workers.
- Environment values never leave dialog-owned state or appear in diagnostic text.
- Process comparisons never substitute a nearby time or different identity.
- Drawing and hit testing use the same dialog geometry.
