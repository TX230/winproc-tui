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

The Process table can select the 24 columns included in `MetricColumn::ALL`. Fresh configurations select CPU, private bytes, private working set, threads, handles, GPU usage and dedicated memory, and read/write I/O rates. The remaining columns, including .NET metrics, remain explicitly selectable; saved column selections are retained.
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
| `.NET Heap` | `dotnet_heap_bytes` | Managed heap size. | .NET 9/10 built-in metric `dotnet.gc.last_collection.heap.size`, summed across gen0, gen1, gen2, LOH, and POH; .NET 8 `System.Runtime` EventCounter `gc-heap-size`; legacy PDH `\.NET CLR Memory(*)\# Bytes in all Heaps` fallback | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Gen0` | `dotnet_gc_gen0_heap_bytes` | Generation 0 heap size after the last GC. | .NET 9/10 `dotnet.gc.last_collection.heap.size`, `gen0` tag; .NET 8 `System.Runtime` EventCounter `gen-0-size`; unavailable on .NET Framework because its similarly named PDH counter is an allocation threshold rather than the same measurement | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Gen1` | `dotnet_gc_gen1_heap_bytes` | Generation 1 heap size after the last GC. | .NET 9/10 `dotnet.gc.last_collection.heap.size`, `gen1` tag; .NET 8 `System.Runtime` EventCounter `gen-1-size`; .NET Framework PDH `\.NET CLR Memory(*)\Gen 1 heap size` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Gen2` | `dotnet_gc_gen2_heap_bytes` | Generation 2 heap size after the last GC. | .NET 9/10 `dotnet.gc.last_collection.heap.size`, `gen2` tag; .NET 8 `System.Runtime` EventCounter `gen-2-size`; .NET Framework PDH `\.NET CLR Memory(*)\Gen 2 heap size` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET LOH` | `dotnet_gc_loh_bytes` | Large object heap size after the last GC. | .NET 9/10 `dotnet.gc.last_collection.heap.size`, `loh` tag; .NET 8 `System.Runtime` EventCounter `loh-size`; .NET Framework PDH `\.NET CLR Memory(*)\Large Object Heap size` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET POH` | `dotnet_gc_poh_bytes` | Pinned object heap size after the last GC. | .NET 9/10 `dotnet.gc.last_collection.heap.size`, `poh` tag; .NET 8 `System.Runtime` EventCounter `poh-size`; unavailable on .NET Framework, which has no POH generation | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Commit` | `dotnet_gc_committed_bytes` | Memory committed by the GC. | .NET 9/10 built-in metric `dotnet.gc.last_collection.memory.committed_size`; .NET 8 `System.Runtime` EventCounter `gc-committed` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Frag` | `dotnet_gc_fragmentation_bytes` | Managed heap fragmentation after the last GC. | .NET 9/10 built-in metric `dotnet.gc.last_collection.heap.fragmentation.size`, summed across gen0, gen1, gen2, LOH, and POH; .NET 8 `System.Runtime` EventCounter `gc-fragmentation`, converted from percent using `gc-heap-size` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `.NET Alloc/s` | `dotnet_allocation_bytes_per_sec` | Managed allocation rate over the collection interval. | .NET 9/10 built-in metric `dotnet.gc.heap.total_allocated`; .NET 8 `System.Runtime` EventCounter `alloc-rate` | Adaptive decimal bytes per second in Processes; exact bytes/s in detail/copy/log |
| `GPU D` | `gpu_dedicated_bytes` | Dedicated VRAM used by the process. | PDH `\GPU Process Memory(pid_*)\Local Usage` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `GPU S` | `gpu_shared_bytes` | Shared system memory used by the process for GPU resources. | PDH `\GPU Process Memory(pid_*)\Non Local Usage` | Adaptive decimal byte unit in Processes; exact bytes in detail/copy/log |
| `IO Read/s` | `io_read_bytes_per_sec` | Process read I/O throughput, including file, network, and device I/O. | PDH `IO Read Bytes/sec` | Whole-number decimal `KB/s` in Processes; Graph, Samples, and copy use the adaptive rate format described below |
| `IO Write/s` | `io_write_bytes_per_sec` | Process write I/O throughput, including file, network, and device I/O. | PDH `IO Write Bytes/sec` | Whole-number decimal `KB/s` in Processes; Graph, Samples, and copy use the adaptive rate format described below |
| `Full Path` | `path` | Executable path. Used to distinguish same-name processes from different build or working directories. | `sysinfo::Process::exe()` | Path text, shortened from the start when the cell is narrow |

When the `Full Path` column is selected in the Process table, filtering matches both process name and executable path.
When it is not selected, filtering matches process name only.
Compact byte formatting is used in the Processes table and for Graph Y-axis tick labels. Sorting and Graph data continue to use the raw numeric values.

Modern .NET metrics are collected for every detected live process identity; its process name does not need to be in the Tracking List to display current values. Non-tracked processes keep only ordinary short Live history, while Recording remains limited to the session's fixed Tracking List scope.

.NET 9/10 uses the `System.Runtime` meter, .NET 8 uses `System.Runtime` EventCounters, and .NET Framework uses the legacy PDH query for its supported subset. Unsupported, inaccessible, stale, or incomplete values remain unavailable (`--`) instead of being estimated from a different runtime source. .NET 8 byte conversions are rounded, and its fragmentation byte value is derived from the reported heap size and fragmentation percentage.

Diagnostics-pipe detection, complete-interval rules, freshness, retry, and shutdown behavior are defined in [.NET Runtime Metrics Collection](dotnet-metrics-collection.md).

The `.NET` column preset selects `.NET Gen2`, `.NET LOH`, and `.NET POH` by default because those heaps are the highest-value first view for long-lived, large, and pinned object growth. Gen0 and Gen1 remain selectable, graphable, copyable, and recordable.

`WS Shrbl` is derived only when both PDH counters exist in the same sample and `Working Set >= Working Set - Private`. An invalid negative difference is unavailable (`--`); it is never clamped to zero. The collector does not call `QueryWorkingSet`, and the previous `WS Shrd` metric is not collected or exposed.

## MEM and GPU Panels

MEM metrics and per-adapter GPU metrics retain 7,200 one-second samples and do not depend on the Tracking List. Available GPU adapters use one-based titles from `GPU 1/N` through `GPU N/N`.

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

`Usage`, `Threads`, and `Processes` are Graph sources. `Freq(P/E)` and the per-core values are display-only and are not recorded.

| Display | Log field | Description | Primary source | Format |
|---|---|---|---|---|
| `Usage` | `cpu_percent`, `cpu_user_percent`, `cpu_kernel_percent` | Total processor utilization with its user-mode (`U`) and privileged/kernel-mode (`K`) components. | PDH `\Processor Information(_Total)\% Processor Time`, `% User Time`, and `% Privileged Time`; total falls back to the `sysinfo` CPU refresh | `nn% (U nn%, K nn%)`; unavailable components use `--` |
| `Freq(P/E)` | Not recorded | Average current clock for logical CPUs classified as performance or efficiency cores. It changes with power management and load. | PDH `\Processor Information(*)\Processor Frequency` multiplied by `\Processor Information(*)\% Processor Performance`, plus Windows processor `EfficiencyClass` | `P MHz / E MHz`; the slash and E value are omitted when no E core is classified |
| `[Per-core Usage (P/E)]` dialog | Not recorded | Utilization for every logical CPU, with `P`, `E`, or `-` when classification is unavailable. | `sysinfo` CPU usage and `GetLogicalProcessorInformationEx(RelationProcessorCore)` | Compact grid of `CPU n (P/E/-) nn%`, in ascending index order across each row |
| `Threads` | `thread_count` | System thread count. | `GetPerformanceInfo` (`ThreadCount`) | Integer; graphable |
| `Processes` | `process_count` | System process count. | `GetPerformanceInfo` (`ProcessCount`); fallback is the collected process count | Integer; graphable and recorded |

If P/E classification is unavailable or all logical CPUs report the same `EfficiencyClass`, `Freq(P/E)` uses the ordinary current-clock summary without an E segment and dialog entries use `-`.
The per-core dialog uses the available width to place several CPUs on each row. It reduces the column count on narrower screens and scrolls by grid row when the entries exceed the visible height.
`Usage`, `Threads`, and `Processes` are retained in `SystemHistory` and can be graphed. Total, user, and kernel utilization plus the process count are stored in recording frames; older schema-v2 logs without the new user/kernel fields show `--` for those two components. Per-logical-CPU values and frequency are not recorded, so the per-core dialog reports that state in Log view.

## NW/DISK Activity

`NW/DISK` displays compact network and disk activity counters.
These values are sampled once per screen update and are stored in recording frames so Log view can show the recorded values.

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `Net Rx` | `network_received_bytes_per_sec` | Total receive throughput across network interfaces. | PDH `\Network Interface(*)\Bytes Received/sec`, excluding `_Total` and summing instances | Whole-number `Mbps`, value right-aligned to at least 4 characters |
| `Net Tx` | `network_sent_bytes_per_sec` | Total send throughput across network interfaces. | PDH `\Network Interface(*)\Bytes Sent/sec`, excluding `_Total` and summing instances | Whole-number `Mbps`, value right-aligned to at least 4 characters |
| `Disk R` | `disk_read_bytes_per_sec` | Total disk read throughput. | PDH `\PhysicalDisk(_Total)\Disk Read Bytes/sec` | Whole-number `MB/s`, value right-aligned to at least 4 characters |
| `Disk W` | `disk_write_bytes_per_sec` | Total disk write throughput. | PDH `\PhysicalDisk(_Total)\Disk Write Bytes/sec` | Whole-number `MB/s`, value right-aligned to at least 4 characters |
| `Disk Q` | `disk_queue_length` | Current total physical disk queue length. | PDH `\PhysicalDisk(_Total)\Current Disk Queue Length` | Whole number, right-aligned to at least 4 characters |

Unavailable values are displayed as `--` and omitted from recording frames.

## System Info

System Info fields describe the current host rather than a historical sample. Field collection and lifecycle are defined in [Process Investigation](process-investigation.md).

| Display name | Description | Primary source |
|---|---|---|
| `winproc-tui` | Running package version. | Cargo package metadata compiled into the executable |
| `Windows` | Windows product/version name. | Windows CurrentVersion registry data through `sysinfo` |
| `Build` | Windows build number. | Windows CurrentVersion registry data through `sysinfo` |
| `Architecture` | Native system architecture such as `x64`. | `GetSystemInfo` through `sysinfo` |
| `CPU` | CPU name and basic clock. | `sysinfo` / registry |
| `Cores` | Topology summary such as P-cores and E-cores. | `GetLogicalProcessorInformationEx` |
| `Cache` | CPU cache summary. | `GetLogicalProcessorInformationEx` |
| `Physical memory` | Installed physical-memory capacity. | `GetPerformanceInfo` (`PhysicalTotal`) |
| `Commit limit` | Current system commit limit. | PDH `\Memory\Commit Limit` |
| `GPU n` | Adapter name plus dedicated and shared capacities. Each hardware adapter has its own row. | DXGI adapter description |
| `Disk x` | Free and total capacity for each logical drive. | `GetLogicalDrives`, `GetDiskFreeSpaceExW` |

Rendering and clipboard output consume the same ordered field list. Clipboard output includes every field as plain `Label: value` text even when a small terminal clips later rows.

## Process Info

Process Info exposes Image and Metrics fields for one fixed process identity. Collection ownership, asynchronous target safety, and dialog lifecycle are defined in [Process Investigation](process-investigation.md).

The `Image` tab displays these values:

| Display name | Description |
|---|---|
| `Process` | Process name and PID. |
| `User` | User that owns the process. |
| `Architecture` | `x64` or `x86` when available. |
| `.NET version` | Loaded runtime derived from `coreclr.dll` for .NET Core / .NET 5+ or `clr.dll` for .NET Framework. Native processes show `--`. |
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

The `Metrics` tab always lists the 23 numeric selectable process metrics in `MetricColumn::ALL` order, independently of the current Processes preset. `Full Path` is excluded. Unlike the compact Processes column headers, the tab uses descriptive row names:

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
| `.NET Gen0` | `.NET Gen 0 Heap` |
| `.NET Gen1` | `.NET Gen 1 Heap` |
| `.NET Gen2` | `.NET Gen 2 Heap` |
| `.NET LOH` | `.NET Large Object Heap` |
| `.NET POH` | `.NET Pinned Object Heap` |
| `.NET Commit` | `.NET GC Committed` |
| `.NET Frag` | `.NET GC Fragmentation` |
| `.NET Alloc/s` | `.NET Allocation Rate` |
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

## Process Priority

Process Info Scheduling reads the CPU scheduling priority class with [GetPriorityClass](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getpriorityclass). Explicit changes use [SetPriorityClass](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass) and require `PROCESS_SET_INFORMATION`; query-only access can still display the current class. The collector retains a handle and verifies process creation time with `GetProcessTimes`.

The displayed class is an on-demand value, separate from CPU utilization and per-thread priority. Idle, Below normal, Normal, Above normal, and High are selectable. Realtime and unknown native values are display-only. Priority values, editor drafts, and restoration points never enter samples, recordings, exports, configuration, or Investigation Profiles. The interaction and lifetime rules belong to [Process Investigation](process-investigation.md#scheduling).

CPU affinity uses [GetProcessAffinityMask](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getprocessaffinitymask) and [SetProcessAffinityMask](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask) on that retained handle, after `GetActiveProcessorGroupCount` confirms a single group. Each checklist bit identifies a group 0 logical CPU allowed by the system mask. Checkbox marks reflect the last verified process mask; masks and restoration values remain session-internal rather than being shown as routine summaries. Existing validated processor topology supplies P/E classification when known. These on-demand values never enter sampling, recording schemas, exports, configuration, or profiles. CPU% remains measured against total system logical CPU capacity, independent of the process affinity mask.


## Open Files

The Process Info `Files` tab displays one entry per named disk-file handle of the fixed live process. Repeated paths remain separate entries, sorted by path and original process handle value.
This is a supporting investigation tool after an increase in `Hndl` has been found, not a metric that is sampled continuously.

The visible Files tab refreshes automatically when collection is inexpensive; slow or failed captures suspend automatic refresh until a successful manual retry. The displayed duration measures the worker's complete request, including identity checks and file attributes, rather than disk I/O latency. The interval is idle time after completion, not a fixed sampling cadence. Scheduling rules belong to [Process Investigation](process-investigation.md#open-files-and-dlls); neither the captures nor their timings enter Recording.

Sources are `NtQuerySystemInformation(SystemExtendedHandleInformation)`, `DuplicateHandle`, `GetFileType(FILE_TYPE_DISK)`, and `GetFinalPathNameByHandleW`.
On the same duplicated handle, `NtQueryInformationFile` queries `FileAccessInformation` and `FileModeInformation` independently. This avoids reopening the file or using access rights from an earlier handle-table entry that may have been reused. Each query failure leaves that field `--`; details show its NTSTATUS or incomplete-reply diagnostic. Duplication uses `DUPLICATE_SAME_ACCESS` and never changes the target's opening mode.
The app displays what can be collected with normal user permissions. Permission failures and handles that cannot be duplicated are treated as uncollected counts or `<access denied>`.
Running as administrator may reveal more handles, but administrator privileges are not a prerequisite.

The table shows the original handle value, file name, directory, and these independent attributes:

| Field | Meaning |
|---|---|
| Cached | `N` when `FILE_NO_INTERMEDIATE_BUFFERING` is set (the native mode corresponding to `CreateFile`'s `FILE_FLAG_NO_BUFFERING`); otherwise `Y`. This is the handle's caching configuration, not a cache-hit measurement. |
| Async | `N` when `FILE_SYNCHRONOUS_IO_ALERT` or `FILE_SYNCHRONOUS_IO_NONALERT` is set; otherwise `Y` for an asynchronous-capable handle, corresponding to `FILE_FLAG_OVERLAPPED`. This describes the opening mode, not how individual requests completed. |
| Access | The granted `FILE_READ_DATA`, `FILE_WRITE_DATA`, and `FILE_APPEND_DATA` bits, shown together as `R`, `W`, and `A` in that order (for example, `RW`, `WA`, or `RWA`). `-` means none of these bits is granted; metadata and other rights can still exist. These are granted rights, not the original `dwDesiredAccess` argument. |
| W-Thru | `Y` when `FILE_WRITE_THROUGH` is set (corresponding to `FILE_FLAG_WRITE_THROUGH`); otherwise `N`. It is independent of caching. |

Unavailable attributes use `--`, distinct from `N` and from an Access value of `-`.

Details include the full access mask and mode flags in hexadecimal. The mode flags are only the subset exposed by [FILE_MODE_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_mode_information); the query contract is documented under [NtQueryInformationFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryinformationfile). Share mode, the complete original `CreateFile` options, actual I/O completion behavior, and device-cache state are not reconstructed. Random access is not inferred from the absence of a sequential flag.

Native tests cover local NTFS disk files with cached/non-cached, write-through, overlapped, read/write/append, metadata-only, and zero-access handles in a separate process. SMB, ReFS, FAT/exFAT, cloud placeholders, and third-party filesystem/filter drivers are not validated by those tests. These are best-effort queries; unsupported or failed queries remain unavailable. No universal filesystem compatibility is claimed.

It does not show a true file-open timestamp because the stable file metadata timestamps available through Windows are file timestamps, not the time when the target process opened that handle.

When copying to the clipboard, use raw text without a header.
Each TSV row contains the full path, original handle in hexadecimal, Cached, Async, Access, W-Thru, raw access mask, and raw mode flags, in that order. Attribute values use the same compact notation as the table. Query failures retain their diagnostic in the raw-field column. The list copies all filtered handle rows; details copy only the selected handle. These attributes are not recorded and do not alter recording schemas.

## File User Search

Find processes by file captures open disk-file handles on an explicit search. It obtains `SystemExtendedHandleInformation` once per scan, duplicates handles from held source process lifetimes, checks `GetFileType(FILE_TYPE_DISK)`, and resolves paths with `GetFinalPathNameByHandleW`. This is a capture interval, not an atomic filesystem snapshot. Multiple handles from one process lifetime to the same returned path produce one result with a handle count. Native creation times verify process ownership and subsequent navigation.

Matching is case-insensitive text matching with Unicode lowercase conversion and `/` normalized to `\`. Extended `\\?\` and `\\?\UNC\` path prefixes are normalized to drive and UNC paths. Filename mode searches only the last path component; path mode searches the whole returned path. Exact mode requires an absolute drive or UNC path and additionally normalizes redundant separators and lexical `.`/`..` components without traversing above a drive or share root. It does not resolve symlinks, hard links, short filenames, device-path aliases, or filesystem-specific case-sensitive identity. Query input is limited to 4,096 UTF-16 units.

Coverage counts use explicit units: processes in the table, inspected processes, access-denied processes, exited processes, otherwise unavailable/unverified processes; total handles, attempted handle inspections, unreadable handles, unnamed disk-file handles, skipped handles belonging to unavailable processes, and handles not reached. Unreadable handles may include non-file objects whose type could not be determined. Successful inspection is limited to named disk-file handles and excludes memory-mapped-only use. Partial zero results are not evidence that a file is unused.

Limits are 30 seconds overall, 5 seconds without progress, 500,000 attempted handle inspections, 5,000 result rows or 8 MiB of retained result strings/row overhead, a 256 MiB system-table buffer, 1 MiB per protocol message, and 512 MiB of helper committed memory. Process termination confirmation waits at most one second; delayed kernel cleanup remains explicit. Limits and cancellation preserve matches whose process lifetime was verified after inspection; an interrupted process may have additional unreported matches.

Clipboard output is one raw TSV row with full matched path, PID, process name, and handle count, without a header. Search data is neither a sampled metric nor recorded/exported data. Native tests exercise duplicate handles and long Unicode paths on local NTFS. UNC normalization is tested as text; SMB, cloud placeholders, other filesystems, and stalled filesystem drivers are not covered by that native fixture. Cancellation tests use deliberately stalled helpers rather than claiming compatibility with every driver.

## Network Endpoints

The global Network browser and Process Info `Network` tab show on-demand endpoint reports, separate from throughput metrics. Sources are `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)` and `GetExtendedUdpTable(UDP_TABLE_OWNER_PID)` for both `AF_INET` and `AF_INET6`.

| Field | Meaning and format |
|---|---|
| Protocol | `TCP4`, `TCP6`, `UDP4`, or `UDP6`. |
| Local | Numeric bound address and decimal port, such as `127.0.0.1:8080` or `[fe80::1%7]:5353`. Wildcard binds retain `0.0.0.0` or `::`; IPv6 interface scope is retained when present. |
| Remote | Numeric TCP peer address and port. TCP listeners and UDP rows display `--`. UDP tables do not expose a peer or TCP-style listening state. |
| State | Windows `netstat` TCP labels, including `LISTENING`, `ESTABLISHED`, `CLOSE_WAIT`, and `TIME_WAIT`. UDP has no TCP connection state and leaves this field empty, including in details and tab-separated clipboard output. Unrecognized TCP state codes display `UNKNOWN`. |
| PID | Owning PID reported by the table, retained even if the process name cannot be verified. |
| Process | Name resolved for a verified process lifetime, otherwise `--`. |

Windows network-order ports are converted before display. Columns size to the filtered endpoint values and available space, with numeric endpoints taking priority over long executable names when space is limited. The global browser expands on wider terminals. Narrow tables omit the remote column; the detail view and clipboard retain complete IPv6 addresses, scopes, peers, and names. Filtering searches the full protocol, local/remote endpoint, state, PID, and process name, case-insensitively. Clipboard output is one raw tab-separated row in that order, without a header. TCP state labels are identical in the table, detail view, filter, and clipboard output.

Capture metadata includes start/end times, successful-table count, and per-table failures. Missing tables are not interpreted as zero endpoints. An unverified name does not mean that the endpoint lacks an owning process. These reports and metadata are not sampled, recorded, or exported. Lifecycle and navigation rules are defined in [Process Investigation](process-investigation.md).

## Loaded DLLs

The Process Info `DLLs` tab displays a point-in-time snapshot of DLL modules loaded by the fixed live process target. It is not a sampled metric and is not recorded. The list excludes the main executable and non-DLL modules, removes duplicate paths case-insensitively, and sorts by DLL name then directory.

The table exposes `DLL`, `Company`, `Product Version`, `File Version`, `Modified`, and `Directory`. Version-resource or file-metadata failures remain per-file values such as `<not available>` or `<missing>` and do not remove that DLL from the list. Narrow layouts prioritize DLL name, file version, modified time, and directory; the selected-row detail retains every full value.

The filter searches every displayed field. Clipboard output contains only the selected DLL's full path. Collection and Log-view lifecycle are defined in [Process Investigation](process-investigation.md).

## Process Environment

The Process Info `Environment` tab displays a best-effort snapshot of the fixed live target's remote environment block. Environment values are not metrics and never enter normal sampling, recording, export, or Log view.

Entries are split at the first `=`. Windows per-drive entries such as `=C:=C:\work` keep `=C:` as the name by using the second separator. Empty values are valid; rows without a separator are counted and skipped. Names are sorted case-insensitively. The filter searches both names and values, and clipboard output contains only the selected `NAME=value` without a header. Status and error text never includes an environment value.

Log view displays `This information is not included in recording logs.` and does not open a process or read remote memory. Recording and export schemas do not include Environment results.

## Meaning of CPU%

`CPU%` means "what percentage of total logical CPU capacity the target process is using."

PDH `\Process(*)\% Processor Time` can sum values across multiple logical CPUs. Therefore, the value is read with `PDH_FMT_NOCAP100`, divided by the logical CPU count, and then clamped to `0.0..=100.0`.

Examples:

- On a 16-logical-CPU machine, a process fully using 1 logical CPU is about `6.25%`.
- On a 16-logical-CPU machine, a process fully using all logical CPUs is about `100%`.

## Sampling Frequency

The base screen update interval is fixed at 1 second and is not configurable. A Recording session may aggregate those one-second samples into `1s`, `2s`, `5s`, or `10s` output frames; this does not change Live collection or Live history.

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
Live keeps at most two ordinary identity histories per case-insensitive process name, and each retained identity keeps its full sample capacity from the table above. Current identities normally occupy those newest-generation slots; if more than two same-name processes are concurrently live, all remain available. Identities referenced by DISPLAY PAUSED, Graphs, or Process Info are also protected and can temporarily exceed the ordinary limit. Recording and loaded Log-view histories are not subject to this Live generation limit.

## Display Formats

| Kind | Display |
|---|---|
| Byte-based process metric | Processes and the Process Info Metrics section use adaptive decimal `B` / `KB` / `MB` / `GB` / `TB` / `PB` / `EB` values with one decimal above bytes. Graph Y-axis ticks use the same compact units and may add precision to keep nearby tick labels distinct. Samples, cursor labels, Graph A/B details and deltas, clipboard output, and recording logs retain exact byte integers. |
| Count metric | Graph Y-axis ticks, Samples, cursor labels, A/B details and deltas, clipboard output, and recording logs use integers. |
| .NET byte rate | Processes and Process Info use adaptive decimal bytes with `/s`; Samples, cursor labels, A/B details, clipboard output, and recording logs retain the exact bytes-per-second value. |
| System memory / VRAM | MB. |
| GPU name / capacity | `name / N GB VRAM`. |
| Disk summary | Aggregated on one line, such as `C: used/total GB`. |
| Process I/O in Processes | Whole-number decimal `KB/s`. |
| Other process I/O displays | Whole-number `Kbps` below 1 Mbps; otherwise whole-number `Mbps`. |
| CPU% | 1 decimal place. |
| GPU% | 1 decimal place. |
| Missing value | `--`. |

`GB`, `MB`, `KB/s`, `Kbps`, and `Mbps` are rounded using a base of 1,000.
Graph card titles keep process metric names compact and qualify system metrics with their source panel, such as `MEM In use`, `GPU Usage`, `CPU Threads`, and `NW/DISK Net Rx`. System and GPU cards omit the redundant `SYSTEM` target and do not include the full GPU adapter name. Units remain visible in metric-specific values and Y-axis ticks rather than as separate title metadata.
Percent, throughput, and disk queue-length Y-axis ticks retain their metric-specific formats.
The `B-A` value in each Graph card title uses the same metric-specific format as the A/B comparison. It is `--` unless both points are set and that Graph has values at both exact captured times.

### A/B range statistics

When both A and B are set, range statistics use the inclusive chronological interval from the earlier timestamp through the later timestamp. Reversing A and B therefore leaves the range statistics unchanged while preserving the signed endpoint `B-A` result. Only available raw stored values for the active Graph contribute; unavailable metrics, missing processes or adapters, and absent Log-view frames are excluded rather than treated as zero or replaced with nearby values.

`Min` and `Max` report the exact stored timestamp that produced each value. When multiple stored samples tie, the earliest captured timestamp in the interval is selected. `Avg` is the arithmetic mean of available values, formatted with the active Graph's metric format, and `Samples` is the available-value count. When Live, paused, or Log-view state provides its complete frame-time sequence, `Samples` also shows the expected frame count and `Missing` is `expected - available`.

Live and paused-display statistics begin directly with `Min`; raw stored values are already the default for these views. Log-view statistics describe the stored Recording frames and prefix that line with the session interval, such as `Range (10s avg) Min`. A longer Recording interval is not expanded into reconstructed one-second values, and a partial final aggregation window remains one stored frame.

### Graph display smoothing

Each Graph independently displays either its raw stored samples or `MA5`. Raw is the default and retains the existing plotted representation. `MA5` is a trailing simple moving average of exactly five contiguous available stored values, including the current value. The first four values in a retained sample series or after any gap produce no MA5 point. An unavailable current metric, a missing frame, a process absence, or an adapter absence clears the window, so the line never connects or averages across that gap. Process Graphs remain keyed by full process identity, and GPU Graphs remain keyed by adapter LUID, so process restarts, PID reuse, and adapter changes cannot share an MA5 window.

MA5 is a display-only derivation calculated in one bounded pass over the retained samples of each visible Graph. Its values determine the plotted line and visible Y-axis bounds. Raw histories are never rewritten. Samples rows, Max, A/B points, `B-A`, range statistics, clipboard output, Recording aggregation, and stored log data continue to use raw exact values. The Samples MA5 summary follows the same complete-window and gap rules.

The compact per-card `[MA]` label means MA5. A cursor value attached to the smoothed line is prefixed with `MA5`. In Log view, the effective window is disclosed as `MA5 (5×Ns avg)`, where `N` is the recording frame interval. Thus `1s`, `2s`, `5s`, and `10s` logs use five already-recorded frames and show 5-, 10-, 25-, or 50-second effective windows; the application does not reconstruct one-second data from aggregated logs.

## Metrics in Recording Logs

Recording logs are JSON Lines. The current writer outputs schema version 3 and the reader loads schema versions 2 and 3.

The current schema-v3 writer shape is also available as a machine-readable [JSON Schema for each record line](schemas/recording-v3-line.schema.json). Cross-record ordering and identity rules remain normative in this document.

At recording start, the writer copies the working Tracking List into the recording session. That fixed session copy supplies process matching until recording stops and is written once in the schema-v3 session record. The working list cannot be edited through the UI during recording.

Recording uses an aggregation interval of `1s`, `2s`, `5s`, or `10s`; `1s` is the default. The writer collects that many one-second snapshots and writes one representative frame. Numeric values are arithmetic means of the values available in the window. Missing values and an absent process do not contribute zero. Process values are aggregated independently by `(PID, name, start_time)`, and GPU values are aggregated independently by adapter LUID. Floating-point metrics remain floating point; averaged byte and count metrics are rounded to the nearest integer. The representative frame uses the final snapshot timestamp in the window.

Aggregation deliberately smooths short spikes. `1s` preserves the finest recorded detail, while longer intervals reduce file size, selected-log loading work, and Log-view memory for long-term trends. Stopping, quitting, or reaching the 24-hour limit flushes a partial final window before the end record so short or non-aligned recordings remain loadable.

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
| `interval` | number | Recording aggregation interval in seconds: `1`, `2`, `5`, or `10`. |
| `tracked` | string array | Fixed Tracking List copied at recording start. |
| `columns` | string array | Process metric columns displayed at recording start. |
| `sort` | two-element string array | Sort column and `asc` or `desc`. |
| `system` | object | Optional `cpu`, `cpu_mhz`, `topology`, and `cache` metadata. |

A process definition is `[process_id, pid, name, start_time, path]`. `process_id` is a monotonically assigned session-local integer. `start_time` and `path` may be `null`. A definition with the same ID may be emitted again before a later frame if its path becomes available or changes.

Process identity is not based on the tracked process name alone. The writer registers the sampled `(PID, name, start_time)` identity and assigns each identity a separate `process_id`. Therefore:

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
| `f64_values` | CPU%, GPU%, reserved, reserved, reserved |
| `u64_values` | private bytes, working set, working-set private, working-set shareable, threads, handles, USER objects, GDI objects, GPU dedicated, GPU shared, .NET heap, I/O read bytes/s, I/O write bytes/s, .NET GC committed, .NET GC fragmentation, .NET allocation bytes/s, .NET Gen0 heap, .NET Gen1 heap, .NET Gen2 heap, .NET LOH, .NET POH |

Fixed-order missing positions are `null`; they remain unavailable in Log view and are not treated as zero. Process `f64_values` positions 2 through 4 are retained as `null` reservations for the removed .NET GC-rate metrics, and legacy values in those positions are ignored when an older schema-v3 log is loaded. Integer byte and count values remain exact JSON integers. For aggregated frames, those integers are the rounded arithmetic means described above. The schema-v3 reader accepts older shorter process arrays and pads the newly appended positions as unavailable.

An end payload is `[ended_at_ms, reason]`. The current writer uses `stopped` for an explicit stop or quit and `duration_limit` for the automatic 24-hour stop. A missing end record is valid after interruption; the last complete frame remains loadable.

Example with one process definition and one compact frame:

```json
{"s":{"v":3,"id":"20260504143012","app":"1.0.0","host":"PC","start":1777872612000,"interval":1,"tracked":["app.exe"],"columns":["PrivBytes"],"sort":["Process","asc"],"system":{"cpu":"Example CPU"}}}
{"p":[0,1234,"app.exe",1700000000,"C:\\work\\app.exe"]}
{"f":[1777872612000,[[1234567890,34359738368,12000000000,null,null,null,2345678901,68719476736,null,null,12,4,214,3812,37,29,8,10000000,20000000,30000000,40000000],1.5,[]],[[0,[12.5,null,null,null,null],[123456789,98765432,90000000,8589932,42,512,21,35,null,null,null,1048576,524288,null,null,null,null,null,null,null,null]]]]}
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
| `interval_seconds` | number | Legacy interval metadata. Version 2 logs written by winproc-tui contain `1`. |
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
| `processes` | object array | Live processes matching the fixed session Tracking List. This can be empty when no live process matches the session's Tracking List. |

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

## References

- [About Windows Performance Counters](https://learn.microsoft.com/en-us/windows/win32/perfctrs/about-performance-counters)
- [Collecting Performance Data with PDH](https://learn.microsoft.com/en-us/windows/win32/perfctrs/collecting-performance-data)
- [`PERFORMANCE_INFORMATION` structure](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-performance_information)
- [`GetGuiResources`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguiresources)
- [`GetLogicalProcessorInformationEx`](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getlogicalprocessorinformationex)
- [.NET runtime metrics](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/built-in-metrics-runtime)
- [Well-known .NET EventCounters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters)

Exact byte values in Graph cursor labels, comparison captions, and Samples have explicit `B` or `B/s` unit context. Samples labels distinguish the retained-history maximum, the MA5 at the selected time, and statistics over the A-B interval. Compact axis units do not change those exact values or raw clipboard/recording formats.
