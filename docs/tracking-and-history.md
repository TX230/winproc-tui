# Tracking and Live History

This document defines how `winproc-tui` represents tracking intent, process identity, named Tracking Lists, and bounded Live history. Metric meanings remain in [metrics.md](metrics.md), Graph ownership remains in [graph-workspace.md](graph-workspace.md), and Recording scope remains in [recording-and-log-view.md](recording-and-log-view.md).

## Concepts

| Concept | Meaning |
|---|---|
| Tracking List entry | A case-insensitive process name expressing what the user wants to retain or record. |
| `ProcessIdentity` | One process lifetime identified by PID, name, and start time. |
| Working Tracking List | The mutable set used by the current Live session. |
| Saved named Tracking List | A persistent definition changed only by explicit list-management actions. |
| Tracked-only | An independent display filter; it is not inferred from whether the working list is empty. |
| Ghost Row | The newest exited identity retained for a tracked name so its final values and history remain inspectable. |

Tracking intent uses process names because PIDs change across restarts and one name can have several live instances. Histories, selections, Process Info targets, and process Graphs use full `ProcessIdentity` values so a reused PID or restarted process never inherits another lifetime's samples.

System history is independent from Tracking Lists. MEM, GPU, System Activity, and aggregate CPU histories are retained without registering a process name.

## Working and Saved Lists

Changing a process's tracked state edits only the working Tracking List. Saved named definitions change only through explicit Save, Save As, Rename, or Delete actions.

The Tracking Lists dialog includes a virtual `Empty (default)` entry. It represents an empty working list, is active only when no saved definition is active, and is never persisted, renamed, deleted, or overwritten. Loading it or a saved definition replaces the working list without changing Tracked-only.

Loading a definition can remove names whose older retained samples are no longer needed. When that operation would discard history beyond general Live retention, the application asks for confirmation before pruning it.

## Startup

Startup mode can resume the previous working list, start empty, or open a chooser containing the previous working list, the virtual empty entry, and saved definitions.

The startup choice is resolved before the first sample. This ensures tracked-history retention applies from the first capture. Canceling the chooser exits before initial sampling and restores the terminal.

Tracking List startup changes and explicit saved-list actions persist immediately. Other session settings are written after a successful interactive run; filter input is never persisted.

## Live History Retention

Tracked process identities retain 7,200 samples, approximately two hours at the fixed one-second Live interval. General non-tracked identities retain 120 samples, approximately two minutes. System history retains 7,200 samples.

Capacity alone is insufficient because frequent process restarts could leave many small identity maps. After every Live snapshot, pruning retains:

- identities sampled within general Live retention;
- current processes;
- Live and Ghost Row identities visible in a paused display;
- the newest exited identity for each tracked name;
- identities referenced by process Graphs;
- the fixed target of an open Process Info dialog.

Older exited or restarted identities are removed from both sample and peak maps using one retained-identity set. Loaded logs are reconstructed from recorded frames and do not use these Live-history capacities.

## Recording Boundary

Starting a Recording copies the working Tracking List into session-owned scope. Later display filtering does not alter that scope, and the working list cannot be edited until Recording ends. See [Recording and Log View](recording-and-log-view.md) for lifecycle rules.

## Invariants

- A tracked name, a currently matching process, and one process identity are distinct concepts.
- PID reuse and process restart must never merge histories.
- Tracked-only must remain independent from the contents of the working Tracking List.
- Working-list changes must not overwrite saved definitions implicitly.
- The virtual empty entry must never be persisted as a named definition.
- History pruning must remove samples and peaks together.
- A paused Ghost Row, registered process Graph, or open Process Info target must remain inspectable even when its identity would otherwise age out.
