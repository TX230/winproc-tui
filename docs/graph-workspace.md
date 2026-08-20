# Graph Workspace

This document defines the state model and invariants of the ordered Graph Workspace, its shared Samples inspector, and A/B comparison. Metric values and display formats remain in [metrics.md](metrics.md); process-history ownership remains in [tracking-and-history.md](tracking-and-history.md).

## Graph Identity and Sources

The workspace stores an ordered `Vec<GraphEntry>` with a limit of 16. Each entry contains a run-unique, monotonically increasing `GraphId` and one `GraphSlot` source.

A source identifies one graphable system metric or one process metric. Process sources retain a full `ProcessIdentity`, not a visible row or PID alone. GPU sources retain the adapter LUID so changing the currently displayed adapter does not retarget an existing Graph.

The collection has no holes or duplicate sources. A non-empty collection always has one `active_graph_id` that resolves to an entry, and an ID is never reused during the run.

Graph registrations, IDs, and scroll position are session state and are not restored across launches. Layout and explicit Samples and Delta visibility preferences are saved settings.

## Shared and Graph-Specific State

| Shared by the workspace | Owned by each Graph |
|---|---|
| Visible absolute time range | Source and metric |
| Live-follow state | Sample availability |
| Selected sample time | Y-axis scale |
| A and B timestamps | Rendered values and B-A result |
| Y-axis lower-bound mode | Process or GPU identity |

The shared right edge comes from the latest sample across all registered Graphs. Each series is plotted against that reference rather than against its own latest sample.

`Fit all` spans the earliest first sample through the latest last sample across the whole workspace. Changing the active Graph therefore cannot change the fitted time range.

## Registration and Ordering

Adding or removing a source changes only that source's entry. Removing an entry does not renumber or reuse Graph IDs.

Direct moves update the ordered collection immediately while keeping the moved entry active. The reorder dialog edits a draft ID sequence and replaces the order only when applied; canceling discards the draft. Both paths preserve Graph identity, active selection, visible time range, selected time, live-follow state, and A/B timestamps.

The active Graph determines which series the shared Samples inspector displays. Changing the active Graph aligns Samples to the shared selected time without manufacturing a value when that series has no sample there.

## Time Selection and A/B Comparison

Navigation may select a nearby useful timestamp, but a Graph displays a value only when that series has a sample at the exact selected `captured_at`. Another Graph's nearby sample must never be presented as synchronized data.

When selection moves outside the visible range, the range shifts only far enough to reveal it. Moving between samples already inside the range does not move the window.

A/B timestamps are shared, while values remain Graph-specific. A Graph reports `B-A` only when it has values at both exact timestamps; otherwise it displays an unavailable value. Process Info applies the same-time rule to the fixed process identity described in [Process Investigation](process-investigation.md).

## Layout and Resize

`GraphSlotLayout` supports Auto and explicit one-, two-, or three-column row-major grids. Auto chooses as many columns as fit while preserving the minimum readable card width. Explicit layouts fall back to fewer effective columns when the terminal is too narrow, and a single Graph uses the full width.

Cards scroll by layout row. Selection changes scroll position only enough to keep the active card visible. The Samples inspector is placed beside Graphs when width permits, below them when height permits, and otherwise collapses temporarily. Temporary collapse is distinct from the saved visibility preference so resizing can restore the inspector.

Graph assignment is independent from terminal geometry and workspace visibility. Resize preserves entries, order, active ID, selected time, A/B timestamps, and live-follow state while recalculating effective columns, Samples placement, and row scroll. If a readable plot cannot fit, the active card retains its identity and remove action and shows a resize message.

Drawing and mouse hit testing consume one `GraphWorkspaceLayout` result for shared controls, the viewport, visible cards, plot regions, remove actions, the scrollbar, and Samples. Only visible cards perform series-rendering work; the active Samples series can still be resolved while its card is outside the viewport.

## Invariants

- A non-empty workspace contains at most 16 unique sources and one valid active ID.
- Graph IDs are run-unique and never reused.
- Reordering preserves identity and all shared comparison state.
- Resize and visibility changes never discard Graph registrations.
- Every Graph remains reachable through row scrolling.
- Shared time state never substitutes another series' nearby sample.
- Drawing and hit testing use the same computed geometry.
- Multi-column Graphs and the active Samples inspector can coexist when their minimum sizes fit.
