# winproc-tui Architecture

`winproc-tui` is a Windows 11 x64-only process monitoring TUI built with Rust 2024, ratatui, crossterm, Windows APIs, PDH, DXGI, and sysinfo.

This document is the entry point for system-wide responsibility boundaries, runtime data flow, and cross-cutting design decisions. Feature-specific state and invariants are owned by the related design documents:

- [Tracking and Live History](tracking-and-history.md): Current Investigation, profiles, tracking intent, process identity, Ghost Rows, and retention.
- [Graph Workspace](graph-workspace.md): Graph identity, shared time state, Samples, A/B comparison, and responsive layout.
- [Process Investigation](process-investigation.md): System Info, Process Info, Files, DLLs, Environment, Network endpoints, and asynchronous target safety.
- [Recording and Log View](recording-and-log-view.md): activity transitions, session ownership, failure handling, and log loading.
- [Metrics](metrics.md): metric meanings, data sources, display formats, aggregation, and recording schemas.
- [.NET Runtime Metrics Collection](dotnet-metrics-collection.md): diagnostics IPC, EventPipe parsing, and runtime-specific collection details.

Product positioning, installation, and first-use workflows belong in the [README](../README.md) and [Japanese README](../README.ja.md). Complete controls belong in the in-app [Help](../src/ui/help.rs), contextual footers, implementation, and tests.

Maintainer release, Scoop, and Windows Package Manager procedures live in the [Release Workflow](release-workflow.md). The machine-readable schema for each current schema-v3 JSON Lines record lives under [`schemas/`](schemas/README.md).

## 1. Runtime Overview

`App` and the single-threaded `run_tui` event loop coordinate the application. Sampling and other potentially slow Windows operations run outside the UI thread and return typed results asynchronously.

```mermaid
flowchart LR
    Input["Keyboard / Mouse"] --> App["App / run_tui<br/>state and actions"]
    Config["CLI / winproc-tui.toml"] --> App

    App -->|SampleRequest| Worker["SamplingWorker"]
    Worker --> Runtime["SamplingRuntime"]
    Runtime --> Windows["PDH / Win32 / DXGI / sysinfo / .NET diagnostics IPC"]
    Windows --> Runtime
    Runtime -->|Snapshot and warning| Worker
    Worker -->|CollectSnapshotResult| App

    App --> Model["Model values owned by App<br/>Snapshot / Histories"]
    App --> UI["UI<br/>ratatui rendering"]
    Model --> UI
    UI --> Terminal["Windows terminal"]
    App -->|explicit save / successful exit| Config
```

This is a runtime data-flow diagram, not a strict Rust dependency graph. `ui` reads application state for rendering, while `app` also consumes geometry helpers from `ui::layout` so drawing and mouse hit testing use the same rectangles.

## 2. Component Boundaries

| Component | Responsibility |
|---|---|
| `main`, `cli`, `config`, `platform`, `terminal` | Process startup, single-instance enforcement, console control handling, terminal setup, frame output and restoration, CLI parsing, persistence, and small Windows helpers. |
| `app` | Main loop, application state, actions, navigation, recording, log loading, clipboard operations, and worker coordination. |
| `model` | UI-independent snapshots, process and system values, identities, column and sorting definitions, and history containers. |
| `samplers` | Collection through sysinfo, PDH, Win32, DXGI, .NET diagnostics IPC, and process-specific helpers; owns the sampling worker and runtime boundary. |
| `ui` | ratatui composition, panels and modals, formatting, themes, and shared screen geometry. |

`model` is the data layer and does not depend on `ui` or `samplers`. Samplers produce model values but never mutate `App` or widgets directly. `App` owns the active model state and coordinates all transitions.

## 3. Cross-Cutting Design Decisions

### 3.1 Keep the UI thread responsive

Windows counter, handle, module, file-metadata, remote-memory, and log operations can block or take variable time. Sampling, Process Info collection, log-directory scans, and full log loading therefore run on dedicated workers or bounded session threads.

Requests and results cross thread boundaries through typed channels or bounded latest-value caches. `App` allows only one sample request in flight, so a slow collection delays the next result instead of creating an unbounded queue.

Worker results carry enough identity, generation, or request information to reject stale results after selection, dialog, process-lifetime, or activity changes.

The global Network browser and Process Info Network tab share an independent Network worker with a bounded request queue. Endpoint reports belong to their dialog sessions, outside `Snapshot`, histories, Recording, and exports. Capturing IP Helper tables and verifying process owners for navigation never runs on the UI or sampling thread.

Find processes by file has a separate controller and an isolated helper process for system-wide disk-file handle inspection. The helper shares handle-table, duplication, and path-resolution primitives with Files; it never enters the normal startup, configuration, single-instance, or terminal lifecycle. Bounded protocol messages, a latest-result slot, cancellation, deadlines, and job ownership bound scan work and cleanup. The browser owns queries and results independently of sampling and Recording.

Scheduling has an independent worker for priority reads and explicit confirmed changes. A session retains one verified process handle, uses a bounded request queue, and checks dialog generation before applying queued work. Its reports and restoration point belong to Process Info, outside sampling, histories, Recording, and configuration.

### 3.2 Keep state ownership centralized

`App` owns Live, paused, Recording, Log-list, and Log-view state. Display accessors select the appropriate snapshot and history without asking widgets to maintain activity-specific copies.

Long-lived tracking intent and per-process identity remain separate. The Current Investigation owns the working Tracking List, and named Investigation Profiles store reusable tracking lists only. App settings are stored independently. Graph sources, Recording scope, and Process Info targets preserve runtime identity where required.

### 3.3 Treat Windows data as best effort

Access restrictions, process exit, unsupported hardware, and counter failures produce unavailable values or warnings instead of failing the whole sample. Missing values remain explicit and are never replaced with plausible measurements. Formatting and recording omission rules are defined in [metrics.md](metrics.md).

### 3.4 Redraw only when visible state changes

`run_tui` is dirty-driven. It draws after input, resize, an applicable worker result, or another visible state transition rather than continuously between events.

Startup and the main UI share buffered terminal output and a synchronized-update boundary around each complete draw, including cursor updates. Supporting terminals present the completed frame together, so full-screen changes such as modal background dimming do not expose intermediate rows. Terminals that ignore synchronized updates still receive buffered output. A failed draw attempts to end the synchronized update before returning its error; terminal restoration also sends an end command.

Display pause freezes only the visible state. Sampling, histories, freshness, and Recording continue in the background. Log view owns separate loaded state and does not support display pause.

### 3.5 Preserve recoverable session data

Configuration is stored beside the real executable after resolving command links and filesystem aliases. If an older launcher-adjacent configuration exists and the real executable has no configuration yet, startup moves the existing file to the real executable directory before loading it. Configuration content is replaced only after a successful interactive run, while startup-setting and explicit Investigation Profile operations persist immediately. Recording uses appendable JSON Lines and preserves partial files after interruption or failure. Detailed persistence and lifecycle rules are defined by the relevant feature documents.

## 4. Runtime Flow

### 4.1 Startup and shutdown

1. `main` parses the CLI and acquires a Windows session-local named mutex. A second instance exits before terminal setup or configuration access.
2. The first instance installs the console control handler, resolves the real executable and its adjacent configuration, migrates a launcher-adjacent configuration when required, and enters raw mode and the alternate screen.
3. Investigation startup state is resolved before the first sample so the selected Tracking List applies to the initial capture. App settings load independently. `App::new` then performs one synchronous initial collection with an empty Graph workspace.
4. `SamplingWorker` handles subsequent samples while `run_tui` uses the same terminal session.
5. After the loop returns, `main` restores the terminal and saves session configuration only when the run succeeded.

Interactive quit enters application cleanup. If Recording is active, the writer is finalized before exit. Console close, logoff, shutdown, `Ctrl+C`, and `Ctrl+Break` request the same cleanup path; close-class events wait for a bounded period. Dropping `SamplingWorker` sends `Stop` and joins its thread.

### 4.2 Main-loop cycle

Each `run_tui` iteration:

1. Applies completed sample, investigation, and log-worker results that still match current state.
2. Recalculates layout state and draws only when dirty.
3. Polls terminal input with a bounded wait so worker and termination results remain responsive.
4. Dispatches input to `App`; resize invalidates layout.
5. Requests the next sample when due, unless one is already in flight or Log view is active.

Applying a Live sample updates the aggregate `Snapshot`, process and system histories, exited-process state, visible-row caches when needed, and an active Recording accumulator. A warning may accompany an otherwise usable snapshot.

### 4.3 Sampling cycle

`SamplingRuntime::collect` refreshes sysinfo, samples system and per-process PDH counters, applies Win32 and DXGI values, and returns one `CollectSnapshotResult { snapshot, warning }`.

GPU Engine, process GPU memory, and adapter memory share a persistent query. Adapter identity and capacity are periodically rechecked so topology changes can replace cached static data. Slow per-process extras are sampled less frequently and reused between refreshes; exact intervals and values remain in [metrics.md](metrics.md).

.NET 8/9/10 sessions run independently per live `ProcessIdentity` and publish only complete recent intervals. They never update `App` directly and do not run in Log view. Protocol and fallback behavior are documented in [.NET Runtime Metrics Collection](dotnet-metrics-collection.md).

The collection boundary deliberately produces one aggregate `Snapshot`. Explicit process investigations remain outside normal sampling as described in [Process Investigation](process-investigation.md).

## 5. State Ownership

`App` owns these high-level state groups:

- sampling progress, current Live data, freshness, and warnings;
- process-table selection, filtering, sorting, columns, and visible-row caches;
- Current Investigation, Investigation Profiles containing tracking lists, tracking intent, histories, and exited rows;
- ordered Graphs and shared comparison state;
- modal and asynchronous investigation sessions;
- display pause, Recording, Log list, and Log view;
- runtime settings, theme, and transient feedback.

`Snapshot` is the aggregate value for one capture time. It contains optional system and process measurements so unavailability can be represented without fabricating a value. `ProcessHistory` is keyed by full process identity, while `SystemHistory` owns system Graph sources. Detailed retention and Graph-source rules are defined in the related design documents.

## 6. UI Boundary

Directional main-screen focus follows effective panel rectangles, skips hidden panels, and stops at screen edges. Neighbors sharing the travel axis take priority, followed by edge distance and center alignment. Tab cycling remains available. Changing workspace focus preserves the active Graph and all panel selections.

Modal input has priority over underlying panels, and non-modal actions depend on the current focus state. Text editing and confirmation flows consume their own input instead of falling through to screen navigation.

The header separates category commands from the right-aligned activity and profile/log status. Session is the leftmost category and owns recording, log transitions, and Quit. Profile, View, Tools, and Settings open independent dropdowns; Help opens the current task's help directly. Tools groups Network endpoints and Find processes by file in Live and Recording. Direct keyboard routes switch those workspaces and return to Processes independently of activity. Hidden destinations remain reachable through overflow navigation and their keyboard routes. Status text never moves the left controls.

An otherwise-unhandled main-screen Esc opens Session. Horizontal header navigation follows the available headings, including Help. Focusing Help does not open a dropdown; explicit activation opens contextual help, while cancellation restores the workspace. Category access keys and mouse headings open only that category; another heading switches directly, and the current heading or an outside click dismisses it. Dialogs and text editing retain input priority. Menu actions reuse existing application transitions and revalidate the activity before activation. Checkbox and appearance choices apply in place. Closing a menu preserves the underlying workspace and its focus. Sampling, freshness, histories, and Recording continue while it is visible; Recording failures and automatic activity transitions dismiss it before presenting the higher-priority state. Network and file-search workspaces retain session state when hidden, while true dialogs own an overlay above the selected surface.

Drawing and hit testing derive regions from shared layout helpers. Semantic interaction state stores identities or sources rather than screen coordinates, so scroll, resize, and filtering cannot retarget an action accidentally.

Visible shortcut groups register their complete rendered geometry and equivalent key event in a transient input map. Each overlay replaces the map for the layer underneath; input consumes it until the next draw, and a changed terminal size invalidates it. Pointer hover changes only emphasis, while an explicit click uses the same command handling and confirmation rules as the keyboard. Ordinary menus dismiss on their trigger or an outside click.

The UI module renders state and exposes geometry; it does not collect metrics or own histories. Exact keys, colors, emphasis, widths, marker shapes, focus order, and drawing positions remain in implementation and rendering tests.

## 7. Invariants and Tests

Cross-cutting invariants are:

- sampling and other expensive Windows work never block the UI thread;
- only one instance reaches terminal or configuration setup in a Windows session;
- `model` remains independent from UI and sampler implementation;
- unavailable data stays explicit;
- display pause does not pause sampling, histories, freshness, or Recording;
- asynchronous results are applied only to the state and identity that requested them;
- drawing and hit testing use the same geometry;
- terminal restoration and Recording cleanup remain part of normal and console-triggered exit paths.

Feature-specific invariants live in their respective design documents rather than being repeated here.

Unit tests live beside modules and in `src/main.rs`. `SamplingWorker::test_pair` supports asynchronous state tests without a real collector. ratatui `TestBackend` and buffer assertions cover layout, styling, and interaction-sensitive rendering.

## 8. Documentation Ownership

When behavior changes, update its canonical owner:

| Change | Canonical documentation |
|---|---|
| Product positioning, installation, first-use workflow | README and Japanese README |
| Complete controls and contextual actions | In-app Help, Footer, dialog guidance, implementation, and tests |
| Metric meaning, source, format, aggregation, recording field | [metrics.md](metrics.md) |
| Current Investigation, profiles, tracking intent, identity, history retention | [tracking-and-history.md](tracking-and-history.md) |
| Graph, Samples, A/B, and workspace layout state | [graph-workspace.md](graph-workspace.md) |
| System Info and Process Info collection lifecycle | [process-investigation.md](process-investigation.md) |
| Recording, log loading, and Log-view lifecycle | [recording-and-log-view.md](recording-and-log-view.md) |
| Cross-component responsibility or runtime flow | This document |
| Schema-v3 record shape | [`schemas/recording-v3-line.schema.json`](schemas/recording-v3-line.schema.json) and [metrics.md](metrics.md) |
| Release, Scoop, and Windows Package Manager publication | [release-workflow.md](release-workflow.md) |
| Agent workflow and regression rules | [AGENTS.md](../AGENTS.md) |

### Action feedback

The existing footer boundary displays transient action results without adding layout height. Normal successful samples do not replace them. Action feedback expires independently of sampling, including during display pause and Log view; routine focus feedback has a shorter lifetime. Switching the working surface or entering/leaving an overlay clears unchanged feedback from its previous context. New feedback from an explicit action inside an overlay uses the same bounded footer lifetime. Dialog instructions stay in their own content and shortcut guidance. Errors still use their persistent dialogs where required. Dialogs also explain activity restrictions at the point of action and omit unavailable action shortcuts. Clipboard feedback describes the operation, never the copied Environment value.

### Help overlay

Help is an independent input layer above every dialog, including text editing and confirmations. Opening and closing it preserves the underlying dialog state and target. It owns keyboard and mouse input while visible. Help initially navigates to the section for the active panel or dialog, including Process Info and its Network and Scheduling tabs. A compact section picker and adjacent-section navigation share the complete scrollable reference; selecting a section changes only Help state. Closing the picker returns to the same Help position, and closing Help restores the caller without changing focus, target, selection, or text drafts. Help lays out complete rows for the available width; drawing, paging, section destinations, and scrollbar geometry use the same wrapped content.

### Appearance

The four accent preferences share readable dark surfaces. Settings shows the active preference and applies an explicit choice immediately; navigation or hover alone never changes it. A separate high-contrast option strengthens neutral text, framing, and guides without replacing the accent preference. Both are saved as app settings and apply to all profiles.

Border colors frame regions; inactive but relevant state uses readable text colors. Focus uses a border and emphasis, the focused cell and selected process rows use separate surfaces, tracking retains its marker, metric values added to Graphs retain the Graph caption, and activity, warnings, and errors retain text labels. These cues supplement color. Higher-contrast dark presentation keeps the same geometry; light and terminal-adaptive palettes remain deferred because the application owns dark surfaces throughout the screen and would need a separately validated palette.
