# 0008: MCP Graph actions and CSV export ownership

- Status: Accepted
- Date: 2026-11-03
- Supersedes: [ADR 0007](0007-mcp-agent-delivery-defaults.md), retaining its delivery defaults
- Related: [Issue #206](https://github.com/TX230/winproc-tui/issues/206)
- Contracts: [MCP integration](../mcp-integration-specification.md#graph-workspace-and-csv-export), [Graph workspace](../graph-workspace.md)

## Context and constraints

The TUI already owns up to 16 identity-bound Graphs and snapshots displayed histories for background CSV writing. MCP clients need to add explicit metrics and export to a specified directory without extra mechanical calls. An independently opened MCP log is different from the log displayed by the TUI. Human changes, process reuse, connection loss and an uncertain response must not retarget or repeat effects.

## Decision

Retain ADR 0007's normal bounded completion, first-page delivery, one-pass cleanup, explicit retention and raw-mode escape hatch. Existing tracking, profile, Recording and collection actions retain their required operation IDs and revisions.

Add Graph discovery and append-only batch actions to the same application-owned operation ledger. CSV export may append explicit sources in the same action. Preserve existing sources, IDs, modes, active selection, focus, visibility and shared time/A-B state; select the first Graph only when the workspace was empty. Validate the complete batch and capacity before changing entries. Treat failed destination/range validation or worker startup as unaccepted work and restore entries; accepted write failure retains the requested additions and any partial file.

Only the two new additive Graph/export actions permit omitted operation IDs and revisions. Generate a missing ID before dispatch and return it in outcomes or uncertain-dispatch errors. An explicit ID with identical arguments supports retries; never replay an unknown outcome automatically. An omitted revision permits applying against the current workspace at acceptance, rather than silently replacing it. A supplied revision rejects concurrent edits. Displayed-process references also bind the full identity and revision, independently of that optional guard. Live references may target retained displayed Live history; Log view requires displayed-Graph references. New log loads invalidate earlier displayed references even when recorded timestamps coincide.

Snapshot using the existing CSV implementation on application acceptance and write on an independent worker. The application owns the receiver and completion after disconnect or local MCP disablement; a pending unaccepted action remains cancellable. Keep the shared mutation slot reserved until writing finishes, while getters, sampling, Recording and human interaction continue. The ledger retains the path, row count or failure and the accepted workspace/source metadata. Application exit ends this in-memory ownership; this is not a durable export queue.

## Alternatives

- Export MCP metric reads in a second serializer: risks different raw values, gaps, ranges or CSV ordering.
- Infer process targets from PID/current selection: permits retargeting and ambiguous Log/paused sources.
- Require discovery and generated fields before every system export: adds avoidable model decisions and calls.
- Generate IDs for all existing actions or weaken their guards: changes unrelated authorization contracts.
- Cancel or roll back an accepted writer on disconnect: risks partial output and undoing later human changes.

## Consequences

A system-metric add-and-export can complete in one ordinary MCP call, with bounded continuation if needed. Process selection remains explicit. Graph changes are visible session state, not saved profiles or Tracking List edits. File creation follows Windows permissions, creates missing directories and never overwrites existing files. MCP and TUI exports share exact raw serialization, including paused and recorded data. This adds controlled file output; Recording-directory configuration remains unchanged.
