# winproc-tui Metrics Specification

This document describes the metrics handled by `winproc-tui`, including display names, data sources, and display formats.
In the current implementation, unavailable values are displayed as `--` in the UI. Schema-v3 fixed-order arrays encode unavailable positions as `null`; schema-v2 object fields were normally omitted when unavailable.

## Sampling Freshness

Live sampling is requested once per second. The header derives freshness from the `captured_at` time of the latest successfully applied live `Snapshot`.

- Less than 3 seconds old: no freshness text is shown.
- 3 seconds old or older: `STALE Ns`, where `N` is the whole-second age.
- A successful live sample immediately removes the stale warning.
- Log view does not display sampling freshness because it reads a saved log instead of live sampling.

`DISPLAY PAUSED` freezes the displayed snapshot only. The current live snapshot, process/system histories, sampling freshness, and an active recording continue to update.

## Process Table Columns

The Process table can select the 16 columns included in `MetricColumn::ALL`. All 16 are selected when no saved column selection exists.
Most columns are numeric metrics that can be sorted, graphed, sampled, and recorded.
`Full Path` is a text column for process identification; it can be displayed, sorted, copied, filtered, and recorded, but it is not a Graph metric.

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `CPU%` | `cpu_percent` | CPU usage for the target process, shown as a percentage of total logical CPU capacity. | PDH `\Process(*)\% Processor Time` | `%` with 1 decimal place |
| `PrivBytes` | `private_bytes` | Committed memory owned by the process. This corresponds to Windows Commit size. | PDH `Private Bytes`; fallback is `sysinfo::virtual_memory()` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `WS` | `workset_bytes` | Working Set currently resident in physical memory. | PDH `Working Set`; fallback is `sysinfo::memory()` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `WS Priv` | `workset_private_bytes` | Private part of the Working Set that is not shared with other processes. | PDH `Working Set - Private` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `WS Shrbl` | `workset_shareable_bytes` | Working Set bytes that can potentially be shared. This is not the amount currently shared with another process. | Same-sample PDH `Working Set - Working Set - Private` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `Thrd` | `thread_count` | Thread count. Used to spot unexpected growth. | ToolHelp process snapshot | Integer |
| `Hndl` | `handle_count` | Handle count. Used to spot leaked files, synchronization objects, and similar resources. | PDH `Handle Count`; fallback is `GetProcessHandleCount` | Integer |
| `USER` | `user_object_count` | Count of USER objects such as windows, menus, cursors, and icons. | `GetGuiResources(GR_USEROBJECTS)` | Integer |
| `GDI` | `gdi_object_count` | Count of GDI objects such as bitmaps, brushes, pens, and fonts. | `GetGuiResources(GR_GDIOBJECTS)` | Integer |
| `GPU%` | `gpu_percent` | Sum of the process's GPU engine utilization values, clamped to 100%. | PDH `\GPU Engine(pid_*)\Utilization Percentage` | `%` with 1 decimal place |
| `.NET Heap` | `dotnet_heap_bytes` | Total .NET CLR managed heap size. | PDH `\.NET CLR Memory(*)\# Bytes in all Heaps` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `GPU D` | `gpu_dedicated_bytes` | Dedicated VRAM used by the process. | PDH `\GPU Process Memory(pid_*)\Local Usage` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `GPU S` | `gpu_shared_bytes` | Shared system memory used by the process for GPU resources. | PDH `\GPU Process Memory(pid_*)\Non Local Usage` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `IO Read/s` | `io_read_bytes_per_sec` | Process read I/O throughput, including file, network, and device I/O. | PDH `IO Read Bytes/sec` | Whole-number decimal `KB/s` in Processes; Graph, Samples, and copy use the adaptive rate format described below |
| `IO Write/s` | `io_write_bytes_per_sec` | Process write I/O throughput, including file, network, and device I/O. | PDH `IO Write Bytes/sec` | Whole-number decimal `KB/s` in Processes; Graph, Samples, and copy use the adaptive rate format described below |
| `Full Path` | `path` | Executable path. Used to distinguish same-name processes from different build or working directories. | `sysinfo::Process::exe()` | Path text, shortened from the start when the cell is narrow |

When the `Full Path` column is selected in the Process table, `Ctrl+F` filtering matches both process name and executable path.
When it is not selected, filtering matches process name only.
Compact byte formatting is used in the Processes table and for Graph Y-axis tick labels. Sorting and Graph data continue to use the raw numeric values.

`WS Shrbl` is derived only when both PDH counters exist in the same sample and `Working Set >= Working Set - Private`. An invalid negative difference is unavailable (`--`); it is never clamped to zero. The collector does not call `QueryWorkingSet`, and the previous `WS Shrd` metric is not collected or exposed.

## MEM and GPU Panels

The first system resource region contains a two-column `MEM` panel and a per-adapter `GPU` panel. Both retain 7,200 one-second samples and do not depend on the Tracking List. Wide layouts show `MEM | GPU | NW/DISK | CPU`; narrow layouts show the selected `MEM` or `GPU` resource view while preserving `NW/DISK` and `CPU` when space permits.

`Tab` / `Shift+Tab` treat `MEM` and `GPU` as separate stops in the normal panel focus cycle. Press `m` or `g` while either resource panel has focus to jump directly to `MEM` or `GPU`. For MEM, `Left` / `Right` move between columns and `Up` / `Down` select a row within the current column. For GPU, `Left` / `Right` change the adapter and `Up` / `Down` select a row. `Space` or double-click adds or removes the selected row in the Graph Workspace.
All nine MEM rows are visible at once under the title `MEM`. Available GPU adapters use one-based titles from `GPU 1/N` through `GPU N/N`.

### MEM left column: Overview

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `In use` | `physical_memory_bytes` | Installed physical memory minus physical pages available to the system. | `GetPerformanceInfo` (`PhysicalTotal`, `PhysicalAvailable`) | `used / total MB` |
| `Modified` | `modified_memory_bytes` | Modified physical pages waiting to be written to disk before they can be repurposed. | PDH `\Memory\Modified Page List Bytes` | MB |
| `Standby` | `standby_memory_bytes` | Sum of standby reserve, normal-priority, and core cache lists. | PDH `Standby Cache Reserve/Normal Priority/Core Bytes` | MB |
| `Free + Zeroed` | `free_zeroed_memory_bytes` | Free and zeroed page lists. | PDH `\Memory\Free & Zero Page List Bytes` | MB |
| `Commit charge` | `committed_bytes`, `commit_limit_bytes` | OS-wide commit charge and commit limit. | PDH `Committed Bytes`, `Commit Limit` | `used / limit MB` |

`Available` is not a panel or Graph source because it overlaps the reusable standby, free, and zeroed page states already shown. `available_memory_bytes` remains in new recordings for schema compatibility and is still used as the fallback source for the `In use` calculation when `GetPerformanceInfo` is unavailable.
The MEM panel omits parenthetical capacity percentages for `In use` and `Commit charge`; the absolute used and limit values remain visible and graphable.

### MEM right column: Pressure

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `Paged Pool` | `paged_pool_bytes` | Pageable kernel pool allocation. | `GetPerformanceInfo` (`KernelPaged`) | MB |
| `Nonpaged Pool` | `nonpaged_pool_bytes` | Nonpageable kernel pool allocation. | `GetPerformanceInfo` (`KernelNonpaged`) | MB |
| `Pages In/s` | `pages_input_per_sec` | Pages read from disk to resolve hard page faults. | PDH `\Memory\Pages Input/sec` | Integer pages/s |
| `Pages Out/s` | `pages_output_per_sec` | Pages written to disk so physical pages can be repurposed. | PDH `\Memory\Pages Output/sec` | Integer pages/s |
`Threads` is displayed in the CPU panel instead of MEM. `Pages Out/s` remains the final pressure row because it directly indicates memory pressure that causes page writeback.

### GPU per adapter

The title is `GPU n/N`, using one-based page numbers for available adapters (`GPU 1/2`, then `GPU 2/2`). When no hardware adapter is available, it is `GPU 0/0`. Adapter identity is the WDDM/DXGI LUID; values from different adapters are never summed. Only adapters in the current DXGI hardware catalog are shown. PDH-only LUIDs, including the WARP software adapter, never create GPU panel entries. `Dedicated` and `Shared` capacity come from that adapter's DXGI description. They are capacity values, not WDDM budget values.

| Display name | GPU adapter log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `Usage` | `utilization_percent` | Busiest physical engine on the adapter. | PDH `\GPU Engine(pid_*)\Utilization Percentage` | Busiest-engine percent |
| `Encode` | `encode_average_percent`, `encode_max_percent`, `encode_engine_count` | VideoEncode utilization across exposed physical encode engines. | Same GPU Engine counter, `engtype_VideoEncode` | `average% max maximum% NE` |
| `Decode` | `decode_average_percent`, `decode_max_percent`, `decode_engine_count` | VideoDecode utilization across exposed physical decode engines. | Same GPU Engine counter, `engtype_VideoDecode` | `average% max maximum% NE` |
| `Dedicated` | `dedicated_bytes`, `dedicated_total_bytes` | Dedicated video-memory usage and capacity for this adapter. | PDH `\GPU Adapter Memory(*)\Dedicated Usage`, DXGI | `used / total MB` |
| `Shared` | `shared_bytes`, `shared_total_bytes` | Shared system-memory usage and capacity for this adapter. | PDH `\GPU Adapter Memory(*)\Shared Usage`, DXGI | `used / total MB` |

The GPU panel keeps `%` on `Usage`, `Encode`, and `Decode` because utilization is itself a percentage. It omits the additional parenthetical capacity percentages from `Dedicated` and `Shared`.

GPU Engine instance names are parsed for PID, LUID, physical-engine index, engine index, and engine type. PID instances that refer to the same physical engine are summed, then clamped to `0..100`. The adapter-wide `Usage` value is the maximum physical-engine value. For Encode and Decode, the main row and Graph value are the average across exposed engines; `max` is the busiest engine, and `NE` is the number of exposed WDDM engines. `NE` is neither a simultaneous session limit nor a count of physical codec circuits. Workloads may also run on `3D` or `Compute`, so codec rows are classification-specific rather than a complete statement about every media workload.

## CPU Panel

The `CPU` panel is the rightmost compact system-pressure display in the top panel row, after the MEM/GPU resource region and `NW/DISK`. Its width depends on the fixed summary rows rather than the number of logical CPUs.

`Up` / `Down` moves through `Usage`, `Threads`, `Processes`, and the bottom `[Per-core Usage (P/E)]` button; `Home` / `End` jumps to the first or last item. `Space` or double-click adds or removes a selected metric in the Graph Workspace. `Enter` opens the dialog when the Per-core button has focus; clicking the button opens it directly. `Enter` or `Esc` closes the dialog. The button uses the shared focus-surface background and bold text while hovered.

Like registered values in `PROCESSES`, MEM, GPU, and `NW/DISK`, a registered `Usage`, `Threads`, or `Processes` value is green and the active Graph value is bold. `Freq(P/E)` is display-only. Source panels do not show Graph slot ordinals or reserve width for them.

| Display | Log field | Description | Primary source | Format |
|---|---|---|---|---|
| `Usage` | `cpu_percent`, `cpu_user_percent`, `cpu_kernel_percent` | Total processor utilization with its user-mode (`U`) and privileged/kernel-mode (`K`) components. | PDH `\Processor Information(_Total)\% Processor Time`, `% User Time`, and `% Privileged Time`; total falls back to the `sysinfo` CPU refresh | `nn% (U nn%, K nn%)`; unavailable components use `--` |
| `Freq(P/E)` | Not recorded | Average current clock for logical CPUs classified as performance or efficiency cores. It changes with power management and load. | PDH `\Processor Information(*)\Processor Frequency` multiplied by `\Processor Information(*)\% Processor Performance`, plus Windows processor `EfficiencyClass` | `P MHz / E MHz`; the slash and E value are omitted when no E core is classified |
| `[Per-core Usage (P/E)]` dialog | Not recorded | Utilization for every logical CPU, with `P`, `E`, or `-` when classification is unavailable. | `sysinfo` CPU usage and `GetLogicalProcessorInformationEx(RelationProcessorCore)` | One row per logical CPU as `CPU n (P/E/-) nn%` |
| `Threads` | `thread_count` | System thread count. | `GetPerformanceInfo` (`ThreadCount`) | Integer; graphable |
| `Processes` | `process_count` | System process count. | `GetPerformanceInfo` (`ProcessCount`); fallback is the collected process count | Integer; graphable and recorded |

If P/E classification is unavailable or all logical CPUs report the same `EfficiencyClass`, `Freq(P/E)` uses the ordinary current-clock summary without an E segment and dialog rows use `-`.
`Usage`, `Threads`, and `Processes` are retained in `SystemHistory` and can be graphed. Total, user, and kernel utilization plus the process count are stored in recording frames; older schema-v2 logs without the new user/kernel fields show `--` for those two components. Per-logical-CPU values and frequency are not recorded, so the per-core dialog reports that state in Log view.

## NW/DISK Activity

The middle of the top panel shows `NW/DISK`, a compact System Activity view for network and disk counters.
Pressing `i` opens `System Info` as a dialog instead of replacing this panel.
These values are sampled once per screen update and are stored in recording frames so Log view can show the recorded values.
When the `NW/DISK` panel has focus, `Up` / `Down` select a metric and `Space` or double-click adds or removes it in the Graph Workspace, matching the MEM/GPU behavior.

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `Net Rx` | `network_received_bytes_per_sec` | Total receive throughput across network interfaces. | PDH `\Network Interface(*)\Bytes Received/sec`, excluding `_Total` and summing instances | Whole-number `Mbps`, value right-aligned to at least 4 characters |
| `Net Tx` | `network_sent_bytes_per_sec` | Total send throughput across network interfaces. | PDH `\Network Interface(*)\Bytes Sent/sec`, excluding `_Total` and summing instances | Whole-number `Mbps`, value right-aligned to at least 4 characters |
| `Disk R` | `disk_read_bytes_per_sec` | Total disk read throughput. | PDH `\PhysicalDisk(_Total)\Disk Read Bytes/sec` | Whole-number `MB/s`, value right-aligned to at least 4 characters |
| `Disk W` | `disk_write_bytes_per_sec` | Total disk write throughput. | PDH `\PhysicalDisk(_Total)\Disk Write Bytes/sec` | Whole-number `MB/s`, value right-aligned to at least 4 characters |
| `Disk Q` | `disk_queue_length` | Current total physical disk queue length. | PDH `\PhysicalDisk(_Total)\Current Disk Queue Length` | Whole number, right-aligned to at least 4 characters |

Unavailable values are displayed as `--` and omitted from recording frames.

## System Info

The `System Info` dialog is not part of metric history. It displays static supporting information about the current environment; live System Activity and CPU summaries stay in the top panels.

| Display name | Description | Primary source |
|---|---|---|
| `CPU` | CPU name and basic clock. | `sysinfo` / registry |
| `Cores` | Topology summary such as P-cores and E-cores. | `GetLogicalProcessorInformationEx` |
| `Cache` | CPU cache summary. | `GetLogicalProcessorInformationEx` |
| `GPU` | GPU name and VRAM capacity. | DXGI |
| `Disk` | Used / total capacity for each disk. | `sysinfo` disk APIs |

## Process Info

Pressing `Enter` on the Processes panel opens a responsive, tabbed `Process Info` dialog for the selected process. The dialog keeps one fixed `ProcessIdentity` across its tabs. It has a maximum outer size and shrinks independently in width and height to fit the terminal's available body area.

Live static Process Info is collected on a worker thread after the dialog target has been stable for 200 ms. The dialog immediately uses the selected `ProcessRow` as a fallback for recorded fields, so its metric history does not wait for static collection.

The `Image` tab displays these values:

| Display name | Description |
|---|---|
| `Process` | Process name and PID. |
| `User` | User that owns the process. |
| `Architecture` | `x64` or `x86` when available. |
| `Parent` | Parent process information. |
| `Started` | Start time and uptime. |
| `Executable` | Executable path. |
| `Command line` | Command line. |
| `Company` | Executable `CompanyName` version resource. |
| `Product` | Executable `ProductName` version resource. |
| `Product version` | Executable `ProductVersion` version resource. |
| `File version` | Executable `FileVersion` version resource. |
| `Modified` | Executable file modification time. |
| `Size` | Executable file size. |

Unavailable values are displayed as one of `<access denied>`, `<exited>`, `<not available>`, `<missing>`, or `--`.

The `Metrics` tab always lists the 15 numeric selectable process metrics in `MetricColumn::ALL` order, independently of the current Processes preset. `Full Path` is excluded. Unlike the compact Processes column headers, the tab uses descriptive row names:

| Processes column | Metrics row |
|---|---|
| `CPU%` | `CPU Usage` |
| `PrivBytes` | `Private Bytes` |
| `WS` | `Working Set` |
| `WS Priv` | `Working Set - Private` |
| `WS Shrbl` | `Working Set - Shareable` |
| `Thrd` | `Threads` |
| `Hndl` | `Handles` |
| `USER` | `USER Objects` |
| `GDI` | `GDI Objects` |
| `GPU%` | `GPU Usage` |
| `.NET Heap` | `.NET Heap` |
| `GPU D` | `GPU Dedicated Memory` |
| `GPU S` | `GPU Shared Memory` |
| `IO Read/s` | `I/O Read Throughput` |
| `IO Write/s` | `I/O Write Throughput` |

The comparison uses the app-wide A/B timestamps set in Graph or Samples:

| A | B | Displayed value | Delta |
|---|---|---|---|
| Not set | Not set | Current displayed Snapshot | Hidden |
| Set | Not set | Current displayed Snapshot | Current minus A |
| Set | Set | B | B minus A |
| Not set | Set | Current displayed Snapshot | Hidden |

For each point, the process identity (PID, name, and start time) and `captured_at` must match exactly. Nearby samples, the latest sample of an exited ghost row, and samples from a reused PID are not substituted. A missing point or metric is displayed as `--`, and a delta is calculated only when both values exist.

Metrics use compact decimal byte units and adaptive process I/O `Kbps` / `Mbps` conversion. Counts use thousands separators. Every calculated delta, including zero, has an explicit sign and is enclosed in parentheses by the dialog.

In `DISPLAY PAUSED`, both the current Snapshot and history come from the paused display state. In Log view, Current is the final recorded Snapshot, metric history comes from the loaded recording, and no live Process Info worker request is made. Static fields that are absent from the recording are displayed as `--`.

## Open Files

`f` (with the Processes panel focused) opens Process Info on its `Files` tab and displays disk file handles for the selected live process, grouped by path. Switching to `Files` from another Process Info tab lazily starts the same collection for the dialog's fixed target.
This is a supporting investigation tool after an increase in `Hndl` has been found, not a metric that is sampled continuously.
While the `Files` tab is active, `Ctrl+U` refreshes the list on demand without queuing another request if a collection is already running.

Sources are `NtQuerySystemInformation(SystemExtendedHandleInformation)`, `DuplicateHandle`, `GetFileType(FILE_TYPE_DISK)`, and `GetFinalPathNameByHandleW`.
The app displays what can be collected with normal user permissions. Permission failures and handles that cannot be duplicated are treated as uncollected counts or `<access denied>`.
Running as administrator may reveal more handles, but administrator privileges are not a prerequisite.

The display table shows handle count, file name, and directory.
It does not show a true file-open timestamp because the stable file metadata timestamps available through Windows are file timestamps, not the time when the target process opened that handle.

When copying to the clipboard, use raw text without a header.
Usually this is only the path. If the same path has multiple handles, copy `path<TAB>count`.

## Loaded DLLs

The Process Info `DLLs` tab explicitly takes a point-in-time snapshot of DLL modules loaded by the fixed live process target. It is not part of normal sampling, recording, or Log view. The collector runs on its own worker so Toolhelp enumeration and file metadata reads do not block input, drawing, Open Files, or the sampling worker.

The collector combines native and WOW64 Toolhelp module snapshots, excludes the main executable and non-DLL modules, removes duplicate paths case-insensitively, and sorts by DLL name then directory. `ERROR_BAD_LENGTH` snapshot failures are retried up to three times. Process identity is checked before and after collection so a result from an exited process or reused PID is rejected.

The table exposes `DLL`, `Company`, `Product Version`, `File Version`, `Modified`, and `Directory`. Version-resource or file-metadata failures remain per-file values such as `<not available>` or `<missing>` and do not remove that DLL from the list. Narrow layouts prioritize DLL name, file version, modified time, and directory; the selected-row detail retains every full value.

The filter searches every displayed field. `Ctrl+U` takes a new snapshot without queuing a duplicate request, and the previous successful snapshot remains visible while refreshing or after a refresh error. `Ctrl+C` copies only the selected DLL's full path. Log view displays `Not recorded in Log view.` and never starts the DLL worker.

## Process Environment

The Process Info `Environment` tab is an explicit, best-effort Windows 11 x64 investigation action. It reads the fixed live target's remote environment block only when the tab is first activated or `Ctrl+U` requests a refresh. It is not part of normal sampling, recording, or Log view, and the in-memory result is cleared when Process Info closes because values may contain passwords, tokens, or other secrets.

The independent Environment worker distinguishes native x64 and WOW64 targets, queries the appropriate PEB, follows pointer-width-specific `ProcessParameters` and environment pointers, and reads UTF-16LE memory with `ReadProcessMemory`. Collection is limited to 4 MiB and requires a double-null terminator. Null or overflowing pointers, unreadable or partial regions, odd byte counts, invalid UTF-16, unsupported architecture, access denial, process exit, and identity change become typed unavailable states rather than raw addresses or OS status values.

Entries are split at the first `=`. Windows per-drive entries such as `=C:=C:\work` keep `=C:` as the name by using the second separator. Empty values are valid; rows without a separator are counted and skipped. Names are sorted case-insensitively. The filter searches both names and values, and `Ctrl+C` copies only the selected `NAME=value` without a header. Status and error text never includes an environment value.

Log view displays `Not recorded in Log view.` and does not open a process or read remote memory. Recording and export schemas do not include Environment results.

## Meaning of CPU%

`CPU%` means "what percentage of total logical CPU capacity the target process is using."

PDH `\Process(*)\% Processor Time` can sum values across multiple logical CPUs. Therefore, the value is read with `PDH_FMT_NOCAP100`, divided by the logical CPU count, and then clamped to `0.0..=100.0`.

Examples:

- On a 16-logical-CPU machine, a process fully using 1 logical CPU is about `6.25%`.
- On a 16-logical-CPU machine, a process fully using all logical CPUs is about `100%`.

## Sampling Frequency

The base screen update interval is fixed at 1 second and is not configurable.

| Kind | Frequency | Target |
|---|---:|---|
| Normal sample | Every 1 second | `sysinfo`, system/process PDH counters, `GetPerformanceInfo`, thread count, handle count, `WS Shrbl`, system GPU/Encode/Decode, per-adapter GPU memory usage, per-process GPU/GPU D/GPU S |
| Slow sample | Every 5 seconds | Per-process USER and GDI object counts |
| Startup / topology check | Startup, then every 5 seconds | DXGI adapter identity, name, and Dedicated/Shared capacity; cached unless the configuration changes |

GPU collection uses one persistent PDH query containing GPU Engine, GPU Process Memory, and GPU Adapter Memory counters. It does not open and close per-counter queries on every sample. Only GUI-resource values are cached between slow samples.

The one-second GPU-memory cadence was validated with a local Windows benchmark using 38 Local Usage and 38 Non Local Usage instances over 500 samples in three runs. Reopening both PDH queries for every sample took approximately `0.055-0.056 ms` wall time and `0.062 ms` CPU time per sample. A persistent two-counter query took approximately `0.030-0.033 ms` wall time and `0.031 ms` CPU time per sample. These numbers exclude Rust instance-name parsing and map construction, so they support the persistent-query design but are not an end-to-end sampling-cost guarantee.

## History Retention

| Target | Retained samples | Notes |
|---|---:|---|
| General process | 120 | About 2 minutes. |
| Tracked process | 7,200 | About 2 hours. |
| System metrics | 7,200 | Used for MEM, per-adapter GPU, System Activity, CPU Usage, system Threads, and system Processes graphs. |

Process history identity consists of PID, process name, and start time.
When start time is available, it is included in the identity to avoid mixing history after PID reuse.

## Display Formats

| Kind | Display |
|---|---|
| Byte-based process metric | Processes and the Process Info Metrics section use adaptive decimal `B` / `KB` / `MB` / `GB` / `TB` / `PB` / `EB` values with one decimal above bytes. Graph Y-axis ticks use the same compact units and may add precision to keep nearby tick labels distinct. Samples, cursor labels, Graph A/B details and deltas, clipboard output, and recording logs retain exact byte integers. |
| Count metric | Graph Y-axis ticks, Samples, cursor labels, A/B details and deltas, clipboard output, and recording logs use integers. |
| System memory / VRAM | MB. |
| GPU name / capacity | `name / N GB VRAM`. |
| Disk summary | Aggregated on one line, such as `C: used/total GB`. |
| Process I/O in Processes | Whole-number decimal `KB/s`. |
| Other process I/O displays | Whole-number `Kbps` below 1 Mbps; otherwise whole-number `Mbps`. |
| CPU% | 1 decimal place. |
| GPU% | 1 decimal place. |
| Missing value | `--`. |

`GB`, `MB`, `KB/s`, `Kbps`, and `Mbps` are rounded using a base of 1,000.
Graph card titles append concise unit metadata such as `[B]`, `[count]`, `[Kbps/Mbps]`, `[Mbps]`, or `[MB/s]`. A unit already present in the metric name is not repeated, so `CPU%` and `GPU%` remain unchanged while `CPU Usage` is shown as `CPU Usage [%]`.
Percent, throughput, and disk queue-length Y-axis ticks retain their metric-specific formats.
The `B-A` value in each Graph card title uses the same metric-specific format as the A/B comparison. It is `--` unless both points are set and that Graph has values at both exact captured times.

## Metrics in Recording Logs

Recording logs are JSON Lines. The current writer outputs schema version 3 and the reader loads schema versions 2 and 3.

At recording start, the writer copies the working Tracking List into the recording session. That fixed session copy supplies process matching until recording stops and is written once in the schema-v3 session record. The working list cannot be edited through the UI during recording.

A recording lasts for at most 24 hours. Reaching the limit writes the clean end record, flushes and closes the log, and returns the application to Live. The elapsed-time check uses a monotonic clock, while timestamps stored in the log remain local wall-clock values.

### Schema version 3

Version 3 uses an externally tagged record with one short key per line. The mandatory first record is `s`; subsequent `p`, `g`, `f`, and `e` records inherit its schema and session identity.

| Key | Payload | Description |
|---|---|---|
| `s` | object | Session metadata and `v: 3`. |
| `p` | array | Process definition written before the first frame that references it. |
| `g` | array | GPU adapter definition written before the first frame that references it. |
| `f` | array | One captured frame. |
| `e` | array | Optional clean end marker. |

The session payload uses these fields:

| Field | Type | Description |
|---|---|---|
| `v` | number | `3`. |
| `id` | string | Start time formatted as `YYYYMMDDhhmmss`. |
| `app` | string | winproc-tui package version. |
| `host` | string | `COMPUTERNAME` or `HOSTNAME`. |
| `start` | integer | Session start as Unix milliseconds. |
| `interval` | number | Fixed sampling interval metadata. Currently `1`. |
| `tracked` | string array | Fixed Tracking List copied at recording start. |
| `columns` | string array | Process metric columns displayed at recording start. |
| `sort` | two-element string array | Sort column and `asc` or `desc`. |
| `system` | object | Optional `cpu`, `cpu_mhz`, `topology`, and `cache` metadata. |

A process definition is `[process_id, pid, name, start_time, path]`. `process_id` is a monotonically assigned session-local integer. `start_time` and `path` may be `null`. A definition with the same ID may be emitted again before a later frame if its path becomes available or changes.

Process identity is not based on the tracked name alone. The writer registers the sampled `(PID, name, start_time)` identity and assigns each identity a separate `process_id`. Therefore:

- a matching process that starts after recording begins receives a definition immediately before its first frame;
- concurrent processes with the same name but different PIDs receive different IDs and histories;
- PID reuse is separated when `start_time` is available.

A GPU definition is `[adapter_id, luid_high, luid_low, name]`. `adapter_id` is also session-local and `name` may be `null`.

A frame payload is `[captured_at_ms, system_metrics, process_samples]`. `captured_at_ms` is Unix milliseconds. `system_metrics` is `[u64_values, disk_queue_length, gpu_samples]`; `process_samples` may be empty while system metrics continue to be recorded.

`system_metrics.u64_values` uses this fixed order:

| Index | Metric |
|---:|---|
| 0 | `physical_memory_bytes` |
| 1 | `total_memory_bytes` |
| 2 | `available_memory_bytes` |
| 3 | `modified_memory_bytes` |
| 4 | `standby_memory_bytes` |
| 5 | `free_zeroed_memory_bytes` |
| 6 | `committed_bytes` |
| 7 | `commit_limit_bytes` |
| 8 | `paged_pool_bytes` |
| 9 | `nonpaged_pool_bytes` |
| 10 | `pages_input_per_sec` |
| 11 | `pages_output_per_sec` |
| 12 | `process_count` |
| 13 | `thread_count` |
| 14 | `cpu_percent` |
| 15 | `cpu_user_percent` |
| 16 | `cpu_kernel_percent` |
| 17 | `disk_read_bytes_per_sec` |
| 18 | `disk_write_bytes_per_sec` |
| 19 | `network_received_bytes_per_sec` |
| 20 | `network_sent_bytes_per_sec` |

A GPU sample is `[adapter_id, f64_values, u64_values]`.

| Array | Index order |
|---|---|
| `f64_values` | utilization, encode average, encode maximum, decode average, decode maximum |
| `u64_values` | encode engine count, decode engine count, dedicated used, dedicated total, shared used, shared total |

A process sample is `[process_id, f64_values, u64_values]`.

| Array | Index order |
|---|---|
| `f64_values` | CPU%, GPU% |
| `u64_values` | private bytes, working set, working-set private, working-set shareable, threads, handles, USER objects, GDI objects, GPU dedicated, GPU shared, .NET heap, I/O read bytes/s, I/O write bytes/s |

Fixed-order missing positions are `null`; they remain unavailable in Log view and are not treated as zero. Integer byte and count values remain exact JSON integers.

An end payload is `[ended_at_ms, reason]`. The current writer uses `stopped` for an explicit stop or quit and `duration_limit` for the automatic 24-hour stop. A missing end record is valid after interruption; the last complete frame remains loadable.

Example with one process definition and one compact frame:

```json
{"s":{"v":3,"id":"20260504143012","app":"1.0.0","host":"PC","start":1777872612000,"interval":1,"tracked":["app.exe"],"columns":["PrivBytes"],"sort":["Process","asc"],"system":{"cpu":"Example CPU"}}}
{"p":[0,1234,"app.exe",1700000000,"C:\\work\\app.exe"]}
{"f":[1777872612000,[[1234567890,34359738368,12000000000,null,null,null,2345678901,68719476736,null,null,12,4,214,3812,37,29,8,10000000,20000000,30000000,40000000],1.5,[]],[[0,[12.5,null],[123456789,98765432,90000000,8589932,42,512,21,35,null,null,null,1048576,524288]]]]}
{"e":[1777872613000,"stopped"]}
```

### Schema version 2 compatibility

Version 2 remains readable. Its object-based layout is retained here for interpreting existing recordings; the current writer no longer emits it. The version 2 reader remains tolerant of logs whose frame-level `tracked_names` changed during a session.

Record types:

| `record_type` | Description |
|---|---|
| `session` | First record. Contains session metadata. |
| `frame` | Contains values for one sample. |
| `end` | End record appended at stop time if possible. |

Session record fields:

| Field | Type | Description |
|---|---|---|
| `schema_version` | number | `2`. |
| `record_type` | string | `session`. |
| `session_id` | string | Start time as `YYYYMMDDhhmmss`. |
| `winproc_tui_version` | string | Package version. |
| `host` | string | `COMPUTERNAME` or `HOSTNAME`. |
| `started_at` | string | RFC 3339 timestamp. |
| `interval_seconds` | number | Fixed sampling interval metadata. Currently `1`. This is a recording-log field, not a user setting. |
| `tracked_names` | string array | Fixed Tracking List copied into the recording session at start. |
| `columns` | string array | Process metric columns currently displayed. |
| `sort` | object | Sort column / direction. |
| `system` | object | Supporting information such as CPU / GPU names. |

`system.gpu_adapters` is an array keyed by `luid_high` and `luid_low`. It stores per-adapter names and capacities when available.

Frame record fields:

| Field | Type | Description |
|---|---|---|
| `schema_version` | number | `2`. |
| `record_type` | string | `frame`. |
| `session_id` | string | Same ID as the session record. |
| `captured_at` | string | RFC 3339 timestamp. |
| `tracked_names` | string array | Session Tracking List; fixed-scope schema-v2 writers repeated the same list in every frame. |
| `system_metrics` | object | System metrics recorded with the frame, including MEM, per-adapter GPU, CPU average, and System Activity values. |
| `processes` | object array | Live processes matching the fixed session Tracking List. This can be empty when the configured tracked names have no live match. |

Process object fields:

| Field | Type | Description |
|---|---|---|
| `pid` | number | PID. |
| `name` | string | Process name. |
| `path` | string | Present only when the executable path is available. |
| `start_time` | number | Present only when available. |
| `metrics` | object | Only metrics that were collected. |

A `frame` record outputs system metrics and the live processes matching the fixed session Tracking List.
System metrics are recorded even when no live process currently matches that list.
System Activity fields are optional for compatibility with older logs and with systems where a PDH counter is unavailable.
Every optional MEM, GPU, and process field is omitted when unavailable. Later schema-v2 recordings store GPU values in `system_metrics.gpu_adapters`; the reader also accepts the older aggregate GPU fields but never combines adapters when reading the per-adapter form.

```json
{
  "schema_version": 2,
  "record_type": "frame",
  "session_id": "20260504143012",
  "captured_at": "2026-05-04T14:30:12+09:00",
  "tracked_names": ["app.exe"],
  "system_metrics": {
    "physical_memory_bytes": 1234567890,
    "total_memory_bytes": 34359738368,
    "available_memory_bytes": 12000000000,
    "modified_memory_bytes": 750000000,
    "standby_memory_bytes": 4000000000,
    "free_zeroed_memory_bytes": 1000000000,
    "committed_bytes": 2345678901,
    "commit_limit_bytes": 68719476736,
    "paged_pool_bytes": 450000000,
    "nonpaged_pool_bytes": 320000000,
    "pages_input_per_sec": 12,
    "pages_output_per_sec": 4,
    "process_count": 214,
    "thread_count": 3812,
    "gpu_adapters": [
      {
        "luid_high": 0,
        "luid_low": 12345,
        "name": "Example GPU",
        "utilization_percent": 74.0,
        "encode_average_percent": 60.0,
        "encode_max_percent": 100.0,
        "encode_engine_count": 2,
        "decode_average_percent": 18.0,
        "decode_max_percent": 31.0,
        "decode_engine_count": 2,
        "dedicated_bytes": 2147483648,
        "dedicated_total_bytes": 8589934592,
        "shared_bytes": 536870912,
        "shared_total_bytes": 17179869184
      }
    ],
    "cpu_percent": 37,
    "cpu_user_percent": 29,
    "cpu_kernel_percent": 8,
    "disk_read_bytes_per_sec": 10000000,
    "disk_write_bytes_per_sec": 20000000,
    "disk_queue_length": 1.5,
    "network_received_bytes_per_sec": 30000000,
    "network_sent_bytes_per_sec": 40000000
  },
  "processes": [
    {
      "pid": 1234,
      "name": "app.exe",
      "path": "C:\\work\\app\\target\\release\\app.exe",
      "start_time": 1700000000,
      "metrics": {
        "private_bytes": 123456789,
        "workset_private_bytes": 98765432,
        "workset_shareable_bytes": 12582912
      }
    }
  ]
}
```

In schema version 2, `metrics` contains only values that were collected. Values that could not be collected were normally omitted; the reader also accepts `null` as a missing value.
Missing values are displayed as `--` in the UI and are not treated as 0 in Graph.
