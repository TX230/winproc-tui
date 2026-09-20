# Tracking and Live History

This document defines how `winproc-tui` represents investigation state, tracking intent, process identity, Investigation Profiles, and bounded Live history. Metric meanings remain in [metrics.md](metrics.md), Graph ownership remains in [graph-workspace.md](graph-workspace.md), and Recording scope remains in [recording-and-log-view.md](recording-and-log-view.md).

## Concepts

| Concept | Meaning |
|---|---|
| Tracking List entry | A case-insensitive process name expressing what the user wants to retain or record. |
| `ProcessIdentity` | One process lifetime identified by PID, process name, and start time. |
| Current Investigation | The mutable working Tracking List and optional active-profile binding used by the current Live session. |
| Working Tracking List | The list of process names currently tracked by the Current Investigation. It is not an independently named object. |
| Investigation Profile | A named reusable Tracking List. It is changed only by explicit profile actions. |
| Tracked-only | A global display preference; it is not inferred from whether the working Tracking List is empty. |
| Ghost Row | The newest exited process identity retained for a tracked process name so its last sampled values and history remain inspectable. |

Tracking List edits are available only in Live. Recording and Log view reject direct edits and pending list changes before they can modify the working list or prune Live history. Tracked-only remains a display preference available from every main panel in all three activities; modal dialogs and text editing retain their own input handling.

Tracking intent uses process names because PIDs change across restarts and one process name can have several live instances. Histories, selections, Process Info targets, and process Graphs use full `ProcessIdentity` values so a reused PID or restarted process never inherits another lifetime's samples.

System history is independent from Tracking Lists. MEM, GPU, System Activity, and aggregate CPU histories are retained without adding a process name to the Tracking List.

## Current Investigation and Profiles

The Current Investigation owns only the working Tracking List and its optional active-profile binding. The last working Tracking List is automatically written after a successful run.

Changing a process's tracked state edits the working Tracking List in the Current Investigation. It marks an active profile as modified but never overwrites that named profile. A saved profile contains only its profile name and case-insensitive tracked process names. It changes only through explicit Save, Save As, or Delete actions.

Opening a profile is available only in Live and replaces only the working Tracking List and active-profile binding. It does not change Tracked-only, Processes Flat/Tree mode, visible columns and order, process sort, Graphs, Graph layout or time span, Samples or Delta visibility, Y-axis mode, or the Recording interval. It can remove process names whose older retained samples are no longer needed. When that operation would discard history beyond general Live retention, the application asks for confirmation before pruning it. Profile deletion remains available in Recording and Log view because it does not change the active investigation.

The active profile is the explicit target for Save. A saved profile becomes active only when it is selected at startup, opened in Live, or created with Save As during the current run. `Resume last` and `Start empty` begin with no active profile, so Save follows the Save As flow until a profile is explicitly opened or created. The header shows that binding and a non-color modified marker; Log view has no Current Investigation binding.

Profiles express tracking intent with case-insensitive process names. They never store a PID, start time, `ProcessIdentity`, Graphs, current selection, text filter, retained history, A/B points, or app settings.

Theme, mouse enablement, Tracked-only, Processes Flat/Tree mode, visible process columns and order, process sort, process column widths, preferred Processes panel height, Graph layout and time span, Samples and Delta visibility, Y-axis lower-bound mode, and the default Recording interval are app settings stored once in `winproc-tui.toml`. Filter input, selections, retained samples, runtime process identities, and Graphs remain session-local and are not restored.

## Startup

Startup mode can `Resume last`, `Ask at startup`, or `Start empty`. The chooser contains `Previous tracking list`, `Empty tracking list`, and every saved Investigation Profile. The two built-in choices are virtual and are never persisted or bound as named profiles. The startup setting is available from the Settings menu.

Startup applies the selected Tracking List before the first sample so tracked-history retention applies from the first capture. App settings are loaded independently. The Graph workspace starts empty each time the app starts; users add Graphs explicitly during the run.

Startup-setting and explicit profile changes persist immediately. Other changes to the Current Investigation and app settings are written after a successful interactive run; filter input is never persisted. Existing broad Investigation Profiles are normalized to a profile name and tracked process names. When global fields for app settings are absent, settings from the legacy last Current Investigation are promoted to those fields; legacy app settings and Graph templates stored in individual profiles are discarded. Legacy named Tracking Lists are migrated once into Investigation Profiles. A colliding profile name is preserved by adding a ` (Tracking List)` suffix, with a numeric suffix when needed; subsequent writes omit the legacy formats.

## Processes Flat and Tree Views

The Processes table supports a persisted `Flat` / `Tree` view preference. Flat view keeps the ordinary globally sorted list. Tree view shows parent-child relationships as process trees using live processes from the currently displayed snapshot. The sampling worker captures each process's parent PID as part of the normal snapshot, so tree construction does not add work to the UI thread.

A parent edge is accepted only when exactly one live row in that same snapshot has the reported parent PID. The edge targets that row's full `ProcessIdentity`; missing, inaccessible, ambiguous, and self-referential parents become roots. Cycles are broken without recursion, and their members become roots. Parentage is never inferred from an earlier snapshot or from retained history, and it is not added to recording schemas. Log view therefore remains Flat even when Tree is the saved Live preference.

Roots and each sibling group follow the current sort column and direction, then each subtree is displayed in parent-first order. Expand/collapse state is session-local and keyed by full `ProcessIdentity`, so a reused PID does not inherit an earlier process lifetime's state. Leaves, the synthetic Tracked Total row, and Ghost Rows have no disclosure control.

In Tree view, a text filter keeps direct live matches and the ancestor paths needed to locate them. Those ancestors are muted context rows, matching paths are temporarily revealed, and match counts and jump navigation count only direct matches. Expand/collapse is temporarily unavailable while a text filter is active, its disclosure glyphs are muted, and the existing session-local collapsed state resumes unchanged when the filter is cleared. Tracked-only is applied before tree construction: untracked ancestors are not reintroduced, and combining it with a text filter searches only the tracked subset. Ghost Rows remain top-level entries after the live process trees and follow the same text filter. Tracked Total remains outside the hierarchy.

Selection follows full process identity across sampling refreshes and sibling reordering. Collapsing a subtree whose descendant has focus moves focus to the collapsed parent, and hidden descendants are removed from multi-selection. Display pause builds the tree from the frozen snapshot; Recording continues to use the current live snapshot and allows Tree view.

## Live History Retention

Tracked process identities retain 7,200 samples, approximately two hours at the fixed one-second Live interval. General non-tracked identities retain 120 samples, approximately two minutes. System history retains 7,200 samples.

Capacity alone is insufficient because frequent process restarts could leave many small identity maps. After every Live snapshot, pruning retains:

- the two newest ordinary identities for each case-insensitive process name, selected from identities sampled within general Live retention and retained exits for tracked process names;
- every current process identity, including all concurrently live same-name instances;
- Live and Ghost Row identities visible in a paused display;
- identities referenced by process Graphs;
- the fixed target of an open Process Info dialog.

The two-generation limit removes an entire older `ProcessIdentity`; it does not shorten the sample series of either retained generation. Current, paused-display, Graph, and Process Info protections can temporarily retain more than two identities for a process name. For a tracked process name, up to two exited identities may remain in internal Live state, while the Processes table continues to show only the newest one as its Ghost Row.

Older exited or restarted identities are removed from both sample and peak maps using one retained-identity set. Recording writes every matching generation to the session log independently of this Live pruning. Loaded logs are reconstructed from recorded frames and do not use Live sample or generation capacities.

## Recording Boundary

Starting a Recording copies the working Tracking List into session-owned scope. Later display filtering does not alter that scope, and the working list cannot be edited until Recording ends. See [Recording and Log View](recording-and-log-view.md) for lifecycle rules.

## Invariants

- A tracked process name, a currently matching process, and one process identity are distinct concepts.
- PID reuse and process restart must never merge histories.
- Tracked-only must remain independent from the contents of the working Tracking List.
- Current Investigation changes must not overwrite a named profile implicitly.
- Opening a profile must pass the retained-history confirmation boundary before replacing the working Tracking List.
- `Previous tracking list` and `Empty tracking list` must never be persisted as named profiles.
- A profile change must never change app settings or current Graphs.
- Tracking intent must be applied before the initial sample.
- History pruning must remove samples and peaks together.
- Ordinary Live history must retain at most two complete generations per case-insensitive process name.
- Concurrently live identities and explicit paused-display, Graph, or Process Info references must remain inspectable even when they exceed the ordinary generation limit.
- A paused Ghost Row, process Graph in the workspace, or open Process Info target must remain inspectable even when its identity would otherwise age out.

Opening a profile also confirms before discarding unsaved changes to the working Tracking List, including reopening the active profile. The same confirmation describes any retained-history removal; cancellation preserves the list, binding, and histories. Equivalent case-insensitive sets of process names do not prompt.

Tracking guidance identifies process names as its scope, including same-name processes started later. Profile and startup guidance distinguish the working tracking list, saved profiles, app settings, and Graphs that start empty on each run. Profile counts describe process names rather than live instances. The empty profile browser provides a direct Save As route for the current tracking list.

### Keyboard focus and process selection

Keyboard focus and process selection are independent. In Processes, Process Info and Files use the focused process, and clipboard copy uses the focused row. Kill targets selected processes when any are selected, otherwise the focused process, and always requires confirmation of fixed process identities. Ordinary row navigation clears process selection; focus-only navigation preserves it. The focused cell has a distinct highlight within selected rows. A marker before each selected PID and a persistent count distinguish selection without relying on color. Filtering, collapse and refresh remove hidden or exited processes from the selection under the existing identity rules.

In Tracked-only mode, select-all selects eligible process identities in the current list, including rows outside the viewport, without changing keyboard focus or scroll position. Text filtering and Tree collapse bound this list; hidden rows, Ghost Rows, and Tracked Total are excluded. Repeating select-all leaves the eligible set selected. Text input and modal dialogs retain their own key handling.

Tree indentation is bounded to preserve readable process names. A compressed-depth marker retains the indication of deeper ancestry; disclosure geometry uses the same bound as rendering. Process Info remains the full-name inspection route. Parentage, filtering and collapse semantics do not depend on the displayed indentation.

Process context menus capture the full process identity and displayed row when opened, and retain that identity while visible. Reordering and PID reuse cannot retarget their actions. Double-clicking a process name or PID opens Process Info; tracking by process name is an explicit context action and retains Live-only and retained-history confirmation rules.
