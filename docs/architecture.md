# winproc-tui Architecture

`winproc-tui` is a Windows 11 x64-only TUI process investigation tool.
This document summarizes the overall codebase structure, responsibility boundaries, data flow, and major types.
The implementation uses Rust 2024 edition, with [ratatui](https://github.com/ratatui/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm) for the TUI runtime.

## 1. Overview

`winproc-tui` consists of three primary layers.

1. **Sampling layer (`samplers`)**: Collects system and process measurements as `Snapshot` values using Windows APIs such as PDH, psapi, tlhelp32, DXGI, and `sysinfo`. It runs on a dedicated worker thread.
2. **Model layer (`model`)**: Provides snapshot structs, metric column definitions, sorting, and history buffers (`ProcessHistory` / `SystemHistory`). This is a pure data layer that does not depend on the UI or samplers.
3. **Application layer (`app`)**: Owns state (`App`), event handling for keys and mouse input, tracked-list management, A/B comparison, recording/replay, clipboard support, and navigation. `run_tui` drives the main loop.

Additional layers:

- **UI layer (`ui`)**: ratatui drawing, screen-area helpers, theme definitions, and panel/modal implementations.
- **Configuration layer (`config` / `cli`)**: `winproc-tui.toml` input/output and CLI parsing.
- **Platform layer (`platform`)**: Windows wide-string (UTF-16) conversion helpers.

Dependencies flow in one direction: `ui` -> `app` -> (`model`, `samplers`, `config`, `ui::widgets`) -> `platform`.
`model` does not depend on any of the other layers.

```text
                +--------------------+
                |       main.rs      |
                | (terminal & loop)  |
                +---------+----------+
                          |
                  +-------v-------+
                  |     app       |  state / actions / export /
                  |  (App, loop)  |  navigation / clipboard
                  +---+-------+---+
                      |       |
              +-------v-+   +-v-------+
              |   ui    |   | model   |
              | (draw)  |   | (data)  |
              +---------+   +----+----+
                               ^
                               |
                        +------+------+
                        |  samplers   |  PDH / Win32 / DXGI / sysinfo
                        |  (worker)   |
                        +-------------+
```

## 2. Directory Structure

```text
src/
  main.rs              startup/shutdown, terminal setup, tests
  cli.rs               minimal clap-based CLI options
  config.rs            winproc-tui.toml reading/writing and RuntimeConfig construction
  platform.rs          Win32 wide-string conversion utilities
  app/
    mod.rs             run_tui main loop
    state.rs           App struct, enums, snapshot application
    actions.rs         on_key / on_mouse event dispatch
    navigation.rs      process-table selection movement
    path_completion.rs reusable directory path completion for modal text inputs
    clipboard.rs       Ctrl+C copy behavior
    export.rs          recording (JSON Lines) and replay
  model/
    mod.rs             public symbol definitions
    snapshot.rs        Snapshot
    process.rs         ProcessRow / ProcessExtraMetrics / ProcessInfo
    system.rs          DiskUsageSample / GpuUsageSample / SystemCounterSample
    columns.rs         MetricColumn / SortColumn / SortDirection / ColumnPreset
    history.rs         ProcessHistory / SystemHistory / ProcessSample
  samplers/
    mod.rs             SamplingWorker / SamplingRuntime / SamplingOptions
    counters.rs        PDH counter abstraction (System/Process)
    pdh.rs             low-level PDH wrapper
    cpu.rs             CPU summary collection
    memory.rs          memory-counter mapping
    disk.rs            disk-capacity collection
    gpu.rs             DXGI / GPU PDH counters
    process.rs         per-process extra metrics (HANDLE count, GDI/USER, WS breakdown)
    process_info.rs    detail collection for the Process Info dialog
    open_files.rs      disk file handle listing for the selected process
  ui/
    mod.rs             full draw composition
    layout.rs          screen-area calculation
    theme.rs           Dark / Light themes
    header.rs / footer.rs
    system_panel.rs    top RAM/VRAM / NW/DISK / CPUs panels and System Info dialog
    process_info_dialog.rs
    cpu_panel.rs       compact live CPU pressure panel
    process_table.rs   process list (columns, sort, tracked display)
    details_panel.rs   Graph + Samples + A/B
    column_picker.rs   column-selection modal
    help.rs            help modal
    open_files.rs      open-files modal
    quit_confirm.rs    quit confirmation
    recording_dialog.rs recording path input / overwrite confirmation / warning
    tracked_remove_confirm.rs tracked removal confirmation (history discard)
    format.rs          number and byte formatting
    widgets/           shared widgets (block, scrollable_modal)
docs/                  architecture, metrics, release workflow
```

## 3. Startup Sequence

`main` performs the following steps (`src/main.rs`):

1. Parse only `--help` / `--version` through `Cli::parse()`.
2. Install a Windows console control handler. `Ctrl+C`, `Ctrl+Break`, terminal close, logoff, and shutdown set a process-wide termination flag. Terminal close, logoff, and shutdown events also wait briefly for the main TUI loop and worker cleanup to finish within the Windows console cleanup window.
3. Resolve `winproc-tui.toml` next to the executable through `resolve_config_path()`.
4. Read TOML through `load_config()`; on failure, warn and continue with defaults. Convert it to `RuntimeConfig` through `build_runtime_config()`.
5. Construct `App::new(runtime)`. The constructor creates `SamplingRuntime::new(...)`, performs the first `collect()` **synchronously once**, initializes history from that result, and then starts a separate `SamplingWorker::spawn(...)` thread.
6. Enter raw mode + AlternateScreen through `setup_terminal(mouse)`, enabling mouse capture if needed.
7. Call `run_tui(&mut terminal, &mut app)`, then restore the terminal reliably through `restore_terminal(...)`.
8. Write TOML back through `write_app_config()` only if `run_tui` succeeds, so a failure does not destroy user settings.

## 4. Runtime Control Flow (`app::run_tui`)

`run_tui` in `src/app/mod.rs` is the single entry point for the main loop after terminal setup.

The main loop is a single-threaded event loop driven by `Instant`:

1. `app.poll_sample_results()` receives arrived snapshots from `SamplingWorker` and applies them to state through `apply_sample_result`.
2. If `dirty` is set, `sync_layout_state` recalculates panel page sizes from the screen size, then `ui::draw(frame, app)` redraws.
3. Until the next tick, the loop waits for terminal input with `event::poll(wait)`, where `wait` is capped at 50 ms and shortened for in-flight worker polling. This wakes promptly for key repeats while still checking Windows console control requests regularly.
4. Keys are delegated to `App::on_key`; mouse events are delegated to `App::on_mouse`; `Resize` sets `dirty`.
5. When `tick_interval()` has elapsed, currently fixed at 1 second, the loop issues `app.request_sample()` and updates `last_tick`.

Key points:

- Drawing is **dirty-driven**. `terminal.draw` runs only when state has changed.
- While display pause is active, background sample results continue updating live history, but they do not mark the UI dirty unless a visible warning or recording error needs to be shown.
- Sampling is **non-blocking**. The UI only sends requests; responses are read later with `try_recv`.
- Interactive quits set `should_quit` through key handling, with the quit confirmation modal in between.
- If the Windows console control handler has observed `Ctrl+C`, `Ctrl+Break`, terminal close, logoff, or shutdown, the loop confirms quit internally without opening the modal, so recording is stopped and flushed before `App` drops its workers. For terminal close, logoff, and shutdown, the handler thread waits up to 4.5 seconds for this cleanup path to complete, then lets Windows continue its default close handling if the main loop did not finish.

## 5. Sampling Subsystem (`samplers`)

### 5.1 Worker Structure

`SamplingWorker` communicates with the UI thread through two MPSC channels.

- `request_tx: Sender<SampleRequest>`: sends `Sample` or `Stop`.
- `result_rx: Receiver<CollectSnapshotResult>`: receives snapshots and warning messages.

The worker thread owns `SamplingRuntime` and runs a `recv()` loop. Each `SampleRequest::Sample` triggers `runtime.collect()`.
On `Drop`, the worker sends `Stop` and then joins the thread.

`SamplingRuntime` owns:

- `sysinfo::System` for process enumeration, memory summary, and CPU list.
- `SystemCounterSampler` for system counters through PDH.
- `ProcessCounterSampler` for process counters through PDH.
- Caches for "heavy" metrics: `cached_slow_process_extras`, GPU summary, and GPU capacity.
- `sample_index`, which refreshes heavy metrics every `SLOW_SAMPLE_INTERVAL = 5` samples and reuses cached values on other ticks.

`SamplingOptions` can enable or disable WS share analysis, GPU collection, and GUI resource collection.
These options are currently fixed in `build_runtime_config` rather than user-configurable.

### 5.2 Collection Contents (`collect_snapshot`)

1. Refresh `sysinfo` memory, processes, and CPU data.
2. Sample process PDH counters and build PID-keyed `ProcessExtraMetrics` through `collect_process_extras`: CPU%, Private, WS metrics, handle count, GDI/USER, .NET heap, I/O, and GPU contribution when needed.
3. Collect GPU summary/capacity only on slow ticks through `collect_gpu_summary_usage` / `collect_gpu_capacity`; reuse caches otherwise.
4. Collect CPU summary, per-logical-CPU utilization, P/E core classification when available, disk capacity, and system activity counters.
5. Iterate `system.processes()` to build `ProcessRow` values, provisionally sorting them by WS -> Private -> name in descending order. Final sorting is handled by UI-side `sort_process_rows`.
6. Map system counters into `Snapshot` fields through `map_memory_counters`, and return warning strings for PDH failures and similar conditions.

The return value is `CollectSnapshotResult { snapshot, warning }`.

### 5.3 Collection Modules

- `pdh.rs`: Wrappers for `PdhOpenQueryW` / `PdhAddEnglishCounterW` / `PdhCollectQueryData` / `PdhGetFormattedCounterArrayW`, plus helpers for reading `Vec<(instance, value)>` from English counter names.
- `counters.rs`: High-level wrappers for system counters and `Process(*)` instances. `SystemCounterSampler` reads Memory / PhysicalDisk throughput and queue length / Network counters. `ProcessCounterSampler` reads CPU%, Private Bytes, WS metrics, I/O Bytes/sec, and related process counters.
- `process.rs`: Uses `OpenProcess` + `QueryWorkingSet` to calculate WS Private/Shareable/Shared, `GetGuiResources` for USER/GDI counts, and `Process32FirstW/NextW` to fill PID and parent PID data.
- `process_info.rs`: Collects executable path, SID/user, command line, file attributes, and version information for the Process Info dialog.
- `open_files.rs`: Enumerates the system handle table through `NtQuerySystemInformation(SystemExtendedHandleInformation)`, opens the selected process with `PROCESS_DUP_HANDLE`, duplicates handles, and reconstructs open file paths through `FILE_TYPE_DISK` and `GetFinalPathNameByHandleW`. Collection happens only on explicit user action. Access-denied and duplicate failures are treated as best-effort uncollected counts.
- `gpu.rs`: Uses DXGI for physical adapter capacity, PDH `GPU Engine` / `GPU Process Memory` for per-process usage, and `is_filtered_dxgi_adapter` to exclude remote/software adapters.
- `memory.rs`: Maps PDH values into `Snapshot` fields and applies needed fallbacks.
- `cpu.rs` / `disk.rs`: CPU topology/cache strings, compact CPU panel samples, current-frequency aggregation, and logical-drive capacity data.

## 6. Data Model (`model`)

### 6.1 Snapshot

`Snapshot` is the aggregate value for one tick. It contains `captured_at` (`DateTime<Local>`), memory totals/usage/commit values, dedicated/shared GPU values, CPU name/topology/cache, compact CPU panel samples, GPU name, disk list, system activity values for network/disk throughput and disk queue length, and `Vec<ProcessRow>`.

`ProcessRow` is the smallest unit used for sorting and table drawing.
It stores the PID, process name, optional executable path, CPU%, Private, WS total / WS Priv / WS Shareable / WS Shared, thread count, handle count, USER/GDI, GPU%, dedicated/shared GPU, .NET heap, and I/O Read/Write as `Option` values. Unavailable items are `None`.

`ProcessInfo` contains static information for the Process Info dialog.
Each field is wrapped in `InfoValue` (value / Missing / AccessDenied / Exited / NotAvailable / FileMissing), allowing the UI to color values by state.

### 6.2 Columns and Sorting

- `MetricColumn`: enum for displayable and sortable Process table columns such as Private, WS Priv, Handle, GPU Dedicated, and Full Path. Provides `label()` / `is_selectable()` / `raw_value(&ProcessRow)` / `compare_values`. `Full Path` is selectable and sortable but is not graphable.
- `SortColumn`: one of `Pid`, `ProcessName`, or `Metric(MetricColumn)`. String parsing is used when restoring TOML.
- `ColumnPreset`: presets such as Default / Memory / Resources / .NET / GPU / IO / Custom. `effective_columns()` returns the initial display order.
- `sort_process_rows`: Takes `SortSpec { column, direction }` and stably sorts `ProcessRow` arrays while pushing missing values to the end. Process-name comparisons are case-insensitive for both direct Process sorting and tie-breakers.

### 6.3 History

There are two history types.

- `ProcessHistory`: A map keyed by `ProcessIdentity { pid, name, start_time }` whose values are `VecDeque<ProcessSample>`. Capacity is asymmetric: **tracked processes use `TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY = 7,200`** (about 2 hours at a 1-second tick), while **non-tracked processes use `GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY = 120`** (about 2 minutes). On each `record_snapshot`, processes present in the latest snapshot append a sample; disappeared tracked processes are retained; disappeared non-tracked processes are garbage-collected.
- `SystemHistory`: Keeps 7,200 `(captured_at, value)` entries per `SystemMetric` (CpuAverage / PhysicalMemory / Committed / GpuDedicated / GpuShared / network throughput / disk throughput / disk queue length). It is used by CPU, RAM/VRAM, and System Activity graphs and A/B comparison.

`ProcessSample` is a subset of `ProcessRow`: timestamp plus per-metric `Option<u64|f64>` values.

## 7. Application State (`app::state::App`)

`App` is a single large struct that holds all state shared by UI drawing and event handling. Main groups:

- **Runtime/sampler**: `runtime: RuntimeConfig`, `sampling_worker: SamplingWorker`, `sampling_in_progress: bool`, `snapshot: Snapshot`, `previous_snapshot: Option<Snapshot>` for live process-table change highlighting.
- **Process table**: `process_table_state: TableState`, `process_page_size`, `selected_process_identity`, multi-selection anchor / live identity set, `selected_process_column_index`, short navigation order hold, `process_columns`, `column_preset`, `sort: SortSpec`, `filter_text` / `filter_draft` / `filter_editing`.
- **Visible-row cache**: `visible_process_entries: Vec<VisibleProcessEntry>` (`Live(index)` or `Ghost(identity)`) plus exited tracked rows in `exited_tracked_rows`. `rebuild_visible_process_cache` filters by text filter, tracked state, and exited state. The text filter matches process names, and also matches executable paths when the `Full Path` column is selected.
- **Tracking**: `watch_list`, `normalized_watch_names`, `watch_enabled`, `last_tracked_live_identities`.
- **Details / Graph**: `show_details`, graph slots for Process metrics or system metrics, graph span/offset/Y-min lock, sample-list selection/offset, scrollbar drag state, and `details_live` for auto-scroll.
- **A/B comparison**: `ab_comparison: Option<AbComparison>`. It is keyed by scope (process or system metric) plus metric, and is cleared when the target changes.
- **Modals**: Flags and scroll/selection state for Help, column picker, log list, log directory input and validation, open files, Settings, quit confirmation, recording path input, overwrite confirmation, no-tracked warning, tracked removal confirmation, process-kill confirmation, and warning dialogs. `has_modal_focus()` returns whether any modal is open.
- **Recording/replay**: `recording_session: Option<RecordingSession>`, `recording_last_dir`, `recording_spinner_index`, `playback_path`, `playback_display`, and log-list/load workers. `activity()` returns `Live` / `Recording` / `Playback`.
- **Theme/status**: `theme_index`, `status` footer text.

`App::new` takes one initial snapshot, normalizes tracking filters, finalizes column configuration, initializes sorting, builds the visible-row cache, clamps table state, and then returns `App`.

## 8. Event Handling

### 8.1 Key Input (`app::actions::on_key`)

Modals are handled first, in priority order:

1. Quit confirmation -> overwrite confirmation -> no-tracked warning -> tracked removal confirmation -> column picker / Help / recording path input dialog.
2. While filter editing is active (`filter_editing == true`), normal navigation is disabled. Only text editing and confirm/cancel are accepted.
3. Otherwise, keys are interpreted based on the current `FocusedPanel` value: System / Cpu / Processes / DetailsGraph / DetailsSamples.

Representative operations:

- `KeyEventKind::Press` and `Repeat` are handled, while `Release` is ignored so held editing keys such as Backspace use terminal key repeat without double-processing key-up events.
- `Tab` / `Shift+Tab`: move focus with `FocusedPanel::next/previous(show_details)`.
- `Left` / `Right` (Processes): select a process-table column.
- `Shift+Up` / `Shift+Down` (Processes): extend live-row multi-selection from the anchor to the cursor row.
- `Ctrl+Up` / `Ctrl+Down` (Processes): move the process-table cursor without changing the multi-selection.
- `Ctrl+Space` (Processes): add/remove the current live row from the multi-selection.
- `Shift+Left` / `Shift+Right` (Processes): move the selected metric column left / right in the custom process-table column order.
- `Enter` (Processes): open the Process Info dialog for the selected row.
- `Enter` (System): report the selected RAM/VRAM metric in status.
- `1` / `2` / `3` / `4` (System): assign the selected RAM/VRAM metric to the matching Graph slot.
- `Enter` (NW/DISK): report the selected activity metric in status.
- `1` / `2` / `3` / `4` (NW/DISK): assign the selected network/disk activity metric to the matching Graph slot.
- `1` / `2` / `3` / `4` (Cpu): assign `CPU Usage` to the matching Graph slot.
- `Space` (Processes): add/remove the selected process name in `watch_list`.
- `t`: toggle `watch_enabled`.
- `Delete`: for live rows, open process-kill confirmation and run `taskkill /f /im` per selected image name after confirmation; for Ghost Rows, delete an exited tracked row through the history-discard confirmation dialog.
- `s` / `c`: toggle sorting / open the column picker.
- `i`: open the System Info dialog.
- `Ctrl+C`: copy the selected row text for RAM/VRAM, NW/DISK, CPUs, Processes, or Samples to the clipboard.
- `f` (Processes): open the open-files list for the selected live process.
- `Ctrl+U` (Open files modal): refresh the open-files list for the selected live process if no previous open-files request is still running.
- `Ctrl+O`: open the Settings dialog.
- `Ctrl+P`: pause/resume display updates.
- `Ctrl+R`: start/stop recording.
- `Ctrl+L`: open the log list; rejected while Recording because Replay cannot start during recording.
- `Esc`: normally opens quit confirmation; during Playback it returns to Live.
- `F2`: switch theme.
- `?`: open Help.

### 8.2 Mouse Input

`on_mouse(mouse, screen_area)` uses helpers in `ui::layout` to hit-test screen regions and handles:

- Panel clicks for focus movement / row selection.
- Scrollbar dragging for Help, column picker, and Samples.
- Graph clicks for sample selection and Y-axis toggle.
- Graph right-button drag and Ctrl+left-button drag for visible range panning.
- Wheels for table, graph, and modal scrolling.
- Ctrl+wheel is forwarded as the Windows Terminal zoom shortcut through `SendInput`, so it changes terminal font scale instead of scrolling inside the TUI.

Mouse capture is enabled only when `runtime.mouse` is true, and can be disabled through settings.

## 9. UI Drawing (`ui`)

### 9.1 Layout

`ui::draw` uses `screen_layout(area)` to split the screen vertically into header / body / footer.
The body is split into a fixed-height top area (`SYSTEM_PANEL_HEIGHT`) for `RAM/VRAM`, `NW/DISK`, and `CPUs`, plus a lower area for the rest.
When `show_details` is true, the lower area is split again into a 13-row process table and a Details panel.

Each modal (Help, column picker, recording dialog, quit confirmation, and others) is drawn as a top-level overlay over `area`.

### 9.2 Major Panels

- `system_panel`: Draws the top row as `RAM/VRAM`, `NW/DISK`, and `CPUs`. RAM/VRAM rows highlight the selected metric and Graph slot state. NW/DISK rows show System Activity metrics and can take focus, select a metric, and assign Graph slots with the same row/number behavior as RAM/VRAM. The System Info view is drawn as a top-level modal dialog when requested.
- `cpu_panel`: Draws the compact `CPUs` panel in the top row. It shows average CPU usage, current P/E clock averages in integer MHz when PDH reports them, and a `Per-core Usage` row where P/E groups are labeled on the same line as their colored per-logical-CPU glyphs. The panel can take focus; `CPU Usage` uses `SystemHistory`, can be assigned to Graph slots, and reserves the first two content cells for the Graph slot number.
- `process_table`: Uses `App::visible_process_row_window(offset, rows)` to build a ratatui `Table`. Ghost Rows (exited tracked processes) use a different color and appear after live rows. The cursor row and live multi-selected rows use separate highlights. Headers show `↑` / `↓` for the sorted column. Live metric cells compare their present raw value against `previous_snapshot` and use the success color for increases and the danger color for decreases. The `Full Path` column is rendered as left-aligned text, takes extra table width when visible, and is shortened from the start when needed. While the user is actively moving the process cursor, sample refreshes briefly preserve the current row order to avoid periodic cursor jumps from metric-driven resorting.
- `details_panel`: Upper Graph (`Chart` widget) plus lower Samples table. The Graph shows history for the current `details_target` / `details_metric` across `graph_time_span_seconds`. It supports Y-min lock to 0, A/B markers, selected cursor, and offset movement with Ctrl+Arrow or mouse drag. A/B marker letters are drawn on the Graph X-axis line, separate from the selected cursor value label. Ctrl+Arrow pans by about one eighth of the current time span; mouse drag clamps the offset so at least one sample from the active series remains in the visible range. Fit-all mode is not cleared by drag attempts. Live graph scrolling stops only when the Graph visible range is moved away from the live edge by Ctrl+Arrow or graph mouse drag; moving the cursor or setting an A/B point does not freeze the Graph visible range. While stopped, sampling updates preserve the absolute visible time window instead of sliding it forward.
- `recording_dialog`: Save-path edit dialog, overwrite confirmation, and no-tracked warning.
- `process_info_dialog`: Displays static details for the selected process through a modal opened from the Processes panel.
- `open_files`: Displays disk file handles for the selected process grouped by path in a compact count / file / directory table. Permission failures and unavailable handles stay as diagnostic lines in the UI and are not part of continuous sampling. The file-name filter is modal state kept across Open files modal openings during the application session.

### 9.3 Shared Widgets

- `widgets::block`: Theme-aware block construction.
- `widgets::scrollable_modal`: Shared scroll state (`ScrollableModalState`) and page-size helpers for Help and column picker.

`ui::format` centralizes byte, integer, and MB/s formatting.
`theme` manages `Dark` / `Light` color definitions and theme index switching.

## 10. Configuration I/O (`config`)

- Input: Deserialize `winproc-tui.toml` with `serde` into `AppConfig` (`general` / `process_table` / `recording` / `[[tracked]]`). On failure, continue with defaults without deleting user settings.
- `build_runtime_config` resolves string column names and preset names into enums, builds `SamplingOptions`, and returns `RuntimeConfig`. `interval_seconds` is read, but currently only the fixed 1-second runtime interval is used.
- Output: Immediately before exit, `write_app_config(&path, &app)` writes back the current theme, column order, sort, `tracked_only`, last recording directory, and tracked list. Filter input is not persisted.

## 11. Recording (`app::export`)

The `Ctrl+R` start flow is:

1. `toggle_recording` checks `activity()` to decide start / stop / reject during Playback.
2. If the configured Tracked List is empty, set `show_recording_no_tracked_warning` and stop.
3. Open the path-input modal with the default path: previous `recording_last_dir` or cwd plus `winproc-tui-YYYYMMDDhhmmss.log`. The modal uses `app::path_completion` for `Tab` directory completion.
4. On confirmation, if the file already exists, go through `RecordingOverwriteSelection` for overwrite confirmation.
5. Allocate the file + `BufWriter`, write the `record_type: "session"` header JSON, then append one JSON Lines `record_type: "frame"` line on each sample application.
6. On another `Ctrl+R`, or when `should_quit` is confirmed, append `record_type: "end"` if possible, flush, close, and update `recording_last_dir` for persistence.

Recording may start when the Tracked List has configured names but none of those names currently match a live process.
In that case, frames still record system metrics and write an empty `processes` array until matching processes appear.

See `docs/metrics.md` for the log value specification.

### 11.1 Activity Transition Policy

The transition rules for `Live` / `Recording` / `Playback` are agent-facing invariants in `AGENTS.md`.
Implementation-wise, `App::activity()` gives `recording_session` priority over `playback_path`.

`Recording` and `Playback` are mutually exclusive.
`Ctrl+R` does not start recording during Playback, and `Ctrl+L` does not open the Log list during Recording.
Additionally, if the log-list loading worker completes after Recording has started, `apply_loaded_log` still rejects the transition to Playback.
This double check prevents `REC` and `PLAY` from mixing when UI operations and worker completion timing overlap.

### 11.2 Log List / Replay

`Ctrl+L` opens the log list.
The log list scans `*.log` files in `recording_last_dir` if present, otherwise in the current directory, using a background worker.
The active search directory is kept in `log_list_dir`; pressing `d` in the log list opens a directory input dialog, and applying a valid directory updates `log_list_dir` and starts a new scan without changing `recording_last_dir`.
The directory input dialog uses `app::path_completion` for `Tab` completion, replacing a unique directory match and cycling through multiple directory matches on repeated `Tab` presses.
Refreshing with `r` scans the active `log_list_dir` rather than recomputing the default directory.
Log summaries read only the first session record and the last non-empty record, so listing a directory does not parse every frame in every log.
This fast path derives schema, start time, and duration; full frame parsing is deferred until the selected log is opened.
Only logs identified as `schema_version: 2` are displayed; old schemas are not displayed.
Broken v2 logs are shown as errors in the list and do not crash the UI thread.

Opening a log sets `playback_path` and `playback_display`.
`playback_display` contains the loaded `Snapshot` / `ProcessHistory` / `SystemHistory`, and the existing Processes / Graph / Samples UI reads replay data through `display_*` accessors.
During Replay, live sampling results are not reflected in the display, and `Ctrl+R` reports a rejection status instead of starting recording.
During Recording, both `Ctrl+L` and `apply_loaded_log` reject transitions to Replay.
During Playback, `Esc` returns to Live. Closing the Log list while in Playback also returns to Live.

## 12. Tracking (Watch) and Ghost Rows

- `watch_list` contains only **process names normalized to lowercase**, not PIDs. All processes with the same name are tracked.
- When a tracked process existed in the previous snapshot but not in the current one, `record_exited_tracked_rows` saves the last observed `ProcessRow` in `exited_tracked_rows`, and the table keeps showing it as `VisibleProcessEntry::Ghost(identity)`.
- History is also retained per `ProcessIdentity`, so values remain available in Details after exit.
- `Delete` on a live row opens the process-kill confirmation dialog. After confirmation, `taskkill /f /im` runs once for each selected image name. `Delete` on a Ghost Row still discards that exited tracked row and its history.

## 13. Tests

Many `#[cfg(test)]` unit tests live at the end of `src/main.rs` and inside individual modules.
The codebase supports unit testing for both the TUI and the asynchronous sampler using `SamplingWorker::test_pair()`, `TestBackend` drawing tests, and `App` construction without `SamplingRuntime` by injecting mock channels through `SamplingWorker::test_pair`.

## 14. Known Constraints and Non-Goals

- **Windows 11 x64 only**. `platform.rs` and `samplers/` depend on Windows APIs through `winapi`; builds on other OSes are not expected.
- The sampling interval is internally fixed at 1 second. `interval_seconds` is saved in TOML but is not applied.
- Metrics that require administrator privileges, such as WS breakdown for protected processes, may be unavailable. In that case, the UI displays states such as `InfoValue::AccessDenied`.
- This is not a long-term time-series database. Tracked history is capped at 7,200 samples, equal to 2 hours at a 1-second interval.

## 15. Extension Guidelines

Typical steps for adding a new measurement:

1. Add an `Option<u64|f64>` field to `model::ProcessRow` or `model::system::*`.
2. Collect the real value in `samplers/process.rs` or `samplers/<source>.rs`, then route it through `ProcessExtraMetrics` and `collect_snapshot` into `ProcessRow`.
3. Add a variant to `model::columns::MetricColumn`, and update `label`, `raw_value`, `compare_values`, `is_selectable`, and `from_str`. Update `ColumnPreset::effective_columns` if needed.
4. Add the corresponding field to `model::history::ProcessSample` and `record_snapshot`.
5. Add `app::state::DetailsMetric`, update `From<MetricColumn>`, and add unit-handling code to the graph drawing branch in `details_panel.rs`.
6. Add UI formatting in `ui::format` or process-table cell rendering.
7. For config compatibility, consider adding old-name aliases to `MetricColumn::FromStr`.

When adding a new modal or focus mode, update the `FocusedPanel` transitions, `has_modal_focus` checks, `on_key` / `on_mouse` dispatch tables, `ui::draw` overlay calls, and `sync_layout_state` page-size calculations.
