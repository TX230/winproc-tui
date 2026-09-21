# Graph Workspace

This document defines the state model and invariants of the ordered Graph Workspace, its shared Samples inspector, and A/B comparison. Metric values and display formats remain in [metrics.md](metrics.md); process-history ownership remains in [tracking-and-history.md](tracking-and-history.md).

## Graph Identity and Sources

The workspace stores an ordered `Vec<GraphEntry>` with a limit of 16. Each entry contains a run-unique, monotonically increasing `GraphId`, one `GraphSlot` source, and its display mode. `Raw` is the default display mode; `MA5` is an explicit per-Graph alternative.

A source identifies one graphable system metric or one process metric. Process sources retain a full `ProcessIdentity`, not a visible row or PID alone. GPU sources retain the adapter LUID so changing the currently displayed adapter does not retarget an existing Graph.

The collection has no holes or duplicate sources. A non-empty collection always has one `active_graph_id` that resolves to an entry, and an ID is never reused during the run.

Graphs remain available only for the current session. They are not written to `winproc-tui.toml`, and the Graph workspace starts empty each time the app starts. Opening or saving an Investigation Profile does not add, remove, reorder, or otherwise change the current Graphs.

## Persistence Boundary

Graph source, order, active ID, Raw/MA5 mode, A/B points, selected sample time, fit-all state, visible-time offset, live-follow position, and Graph scroll position remain session-local. Process Graphs require a full runtime `ProcessIdentity`, so a tracked process name in an Investigation Profile is never treated as enough information to recreate one.

Graph layout, fixed time span, Samples and Delta visibility, and Y-axis lower-bound mode are app settings stored once in `winproc-tui.toml`. They apply to the current workspace but do not belong to an Investigation Profile.

## Shared and Graph-Specific State

| Shared by the workspace | Owned by each Graph |
|---|---|
| Visible absolute time range | Source and metric |
| Live-follow state | Sample availability |
| Selected sample time | Y-axis scale |
| A and B timestamps | Rendered values and B-A result |
| Y-axis lower-bound mode | Process or GPU identity |
| Log-view frame interval | Raw or MA5 display mode |

The shared right edge comes from the latest sample across all Graphs in the workspace. Each series is plotted against that reference rather than against its own latest sample.

`Fit all` spans the earliest first sample through the latest last sample across the whole workspace. Changing the active Graph therefore cannot change the fitted time range.

## Adding, Removing, and Ordering Graphs

Adding or removing a source changes only that source's entry. Removing an entry does not renumber or reuse Graph IDs.

Direct moves update the ordered collection immediately while keeping the moved entry active. The reorder dialog edits a draft ID sequence and replaces the order only when applied; canceling discards the draft. Both paths preserve Graph identity, active selection, visible time range, selected time, live-follow state, and A/B timestamps.

The active Graph determines which series the shared Samples inspector displays. Changing the active Graph aligns Samples to the shared selected time without manufacturing a value when that series has no sample there.

Changing a Graph's display mode affects only that entry. Reordering and resize preserve the mode with its `GraphId`; removing the entry removes its mode. The active Graph, selected time, visible range, live-follow state, and A/B timestamps do not change when a mode button is used, including when the button belongs to an inactive card.

## Time Selection and A/B Comparison

Navigation may select a nearby useful timestamp, but a Graph displays a value only when that series has a sample at the exact selected `captured_at`. Another Graph's nearby sample must never be presented as synchronized data.

When selection moves outside the visible range, the range shifts only far enough to reveal it. Moving between samples already inside the range does not move the window.

Explicit keyboard panning works from both Graph and Samples focus and translates the shared time window without changing its fixed span, selected sample time, active Graph, or A/B timestamps. It follows the same history bounds and latest-follow behavior as other time-panning actions. Graph page navigation changes the time span; Samples page navigation moves the sample selection without changing the span.

A/B timestamps are shared, while values remain Graph-specific. A Graph reports `B-A` only when it has values at both exact timestamps; otherwise it displays an unavailable value. Process Info applies the same-time rule to the fixed process identity described in [Process Investigation](process-investigation.md).

When both timestamps are set, the active Samples inspector also describes the inclusive chronological interval between them. Reversing A and B does not reverse or otherwise change the interval statistics; the signed endpoint `B-A` remains unchanged. Summary labels distinguish the retained-history maximum, selected-time MA5, and A-B interval statistics. The statistics are recalculated independently from the active Graph's raw stored samples and appear only when that Graph has at least one available value in the interval. Min and max retain the exact stored timestamp that produced the value, with the earliest timestamp selected for a tie. The mean and available count exclude missing metrics and absent process or adapter samples. When the display state owns a complete frame-time sequence, it also reports expected frames and how many of those frames lack an available value for the active Graph.

Changes to summary height or terminal geometry keep the selected Samples row visible by adjusting the shared viewport offset to the new row capacity. Rendering and mouse selection use that same offset.

Display smoothing does not change time selection or comparison data. Samples rows, Max, A/B values, `B-A`, range statistics, and clipboard output always use raw stored values. A cursor value attached to an MA5 line is labeled `MA5`; in Log view it also discloses the loaded frame interval.

## Layout and Resize

`GraphSlotLayout` supports Auto and explicit one-, two-, or three-column row-major grids. Auto chooses as many columns as fit while preserving the minimum readable card width. Explicit layouts fall back to fewer effective columns when the terminal is too narrow, and a single Graph uses the full width.

Time-axis labels adapt to the available card width without overlapping. Endpoint labels take priority over intermediate labels; this does not change the plotted time range or cursor and A/B positions.

Cards scroll by layout row. Selection changes scroll position only enough to keep the active card visible. The Samples inspector is placed beside Graphs when width permits, below them when height permits, and otherwise collapses temporarily. Temporary collapse is distinct from the saved visibility preference so resizing can restore the inspector.

The Graphs in the workspace are independent from terminal geometry and workspace visibility. Resize preserves entries, order, active ID, selected time, A/B timestamps, and live-follow state while recalculating effective columns, Samples placement, and row scroll. If a readable plot cannot fit, the active card retains its identity and remove action and shows a resize message.

The vertical split above the Graph Workspace uses either `Auto` or a saved preferred `PROCESSES` table-body capacity. `Auto` allocates about two fifths of the lower workspace body capacity to Processes, while reserving a readable Graph card and workspace controls. Sparse results remain content-sized. A manual preference uses the saved capacity, but the effective height still shrinks to the number of rendered process rows and always leaves the Graph Workspace its minimum readable height. Content or terminal-size clamps do not overwrite the preference, so later growth or a larger terminal restores it. Hiding Graphs gives `PROCESSES` the full lower area; showing them again reapplies the saved split across Live, Recording, and Log view. The `[process_table]` configuration stores this as `body_rows = "auto"` or a positive integer.

`h` increases the preferred body capacity, `Shift+H` decreases it, and `Alt+H` returns to `Auto` while Processes, a Graph, or Samples has focus. A mouse drag that begins on the shared bottom border of `PROCESSES` updates the same preference in whole rows. The shared layout result owns that border's drawing and hit-test rectangle. Modal input, hidden Graphs, and terminal resize end an active drag without persisting pointer coordinates.

Drawing and mouse hit testing consume one `GraphWorkspaceLayout` result for shared controls, the viewport, visible cards, plot regions, per-card `[RAW]` / `[MA]` mode actions, remove actions, the scrollbar, and Samples. The mode action is immediately before `[x]`; both remain separate when a title is truncated. Clicking it targets the owning stable `GraphId` without activating that card. Only visible cards perform series-rendering work; the active Samples series can still be resolved while its card is outside the viewport.

## Invariants

- A non-empty workspace contains at most 16 unique sources and one valid active ID.
- Graphs and their runtime identities are never persisted or reconstructed from process names.
- Graph IDs are run-unique and never reused.
- Reordering preserves identity and all shared comparison state.
- Raw or MA5 mode remains owned by its Graph entry across reordering and resize.
- Resize and visibility changes never remove Graphs from the workspace.
- Process/Graph split clamps never overwrite the saved `Auto` or preferred body-row setting.
- Every Graph remains reachable through row scrolling.
- Shared time state never substitutes another series' nearby sample.
- Drawing and hit testing use the same computed geometry.
- Multi-column Graphs and the active Samples inspector can coexist when their minimum sizes fit.

Process Graph and Samples captions retain the target PID when names must be shortened. Exact B-A values occupy a separate caption so target labels cannot truncate their digits. Layout controls distinguish effective column count from a constrained explicit preference.

Graph and Samples show their local history/follow state independently of the activity header. Manual history inspection offers a return to the latest sample; paused and recorded latest samples are labeled by their source. Returning to latest or reaching the latest pan boundary uses that same source label; it never implies that display pause has ended or that recorded samples are live.

MEM, GPU, NW/DISK, and CPU remain separate across terminal widths. When the full row does not fit, MEM omits its secondary memory-pressure column before the panels shrink. The row fills the available width, and drawing, hit testing, and keyboard metric selection use the visible layout. Resizing a selected hidden MEM column returns selection to the corresponding overview row; Graphs remain intact.

Directional panel navigation treats GRAPHS as one workspace and Samples as a separate visible panel. Entering or leaving GRAPHS preserves its active card; card selection and shared-time panning remain independent operations.

The complete GRAPHS frame indicates workspace keyboard focus using the same focus treatment as other main panels. Inner card frames identify the active Graph independently. Samples stays outside that frame. Card, plot, control, and scrollbar geometry reserves the workspace border cells and is shared by drawing and mouse input.

Context menus capture the clicked Graph ID and sample timestamp without changing shared time, A/B, follow state, or fit-all state until an action is chosen. Show graph on a Processes metric adds or reveals its fixed source and focuses it; removal is a separate action. A right-button drag pans time, while a press/release without movement opens the menu. Graph and Samples actions share the same workspace state.
