# winproc-tui Metrics Specification

This document describes the metrics handled by `winproc-tui`, including display names, data sources, and display formats.
In the current implementation, unavailable values are displayed as `--` in the UI and are omitted from recording logs rather than written as `null`.

## Process Metrics

The Process table can select the 14 metrics included in `MetricColumn::ALL`.

| Display name | Log field | Description | Primary source | Display format |
|---|---|---|---|---|
| `CPU%` | `cpu_percent` | CPU usage for the target process, shown as a percentage of total logical CPU capacity. | PDH `\Process(*)\% Processor Time` | `%` with 1 decimal place |
| `Private` | `private_bytes` | Committed memory owned by the process. This corresponds to Windows Commit size. | PDH `Private Bytes`; fallback is `sysinfo::virtual_memory()` | Byte integer with thousands separators |
| `WS` | `workset_bytes` | Working Set currently resident in physical memory. | PDH `Working Set`; fallback is `sysinfo::memory()` | Byte integer with thousands separators |
| `WS Priv` | `workset_private_bytes` | Private part of the Working Set that is not shared with other processes. | PDH `Working Set - Private` | Byte integer with thousands separators |
| `Thrd` | `thread_count` | Thread count. Used to spot unexpected growth. | ToolHelp process snapshot | Integer |
| `Hndl` | `handle_count` | Handle count. Used to spot leaked files, synchronization objects, and similar resources. | PDH `Handle Count`; fallback is `GetProcessHandleCount` | Integer |
| `USER` | `user_object_count` | Count of USER objects such as windows, menus, cursors, and icons. | `GetGuiResources(GR_USEROBJECTS)` | Integer |
| `GDI` | `gdi_object_count` | Count of GDI objects such as bitmaps, brushes, pens, and fonts. | `GetGuiResources(GR_GDIOBJECTS)` | Integer |
| `GPU%` | `gpu_percent` | Per-process GPU engine utilization. | PDH `\GPU Engine(pid_*)\Utilization Percentage` | `%` with 1 decimal place |
| `.NET Heap` | `dotnet_heap_bytes` | Total .NET CLR managed heap size. | PDH `\.NET CLR Memory(*)\# Bytes in all Heaps` | Byte integer with thousands separators |
| `GPU D` | `gpu_dedicated_bytes` | Dedicated VRAM used by the process. | PDH `\GPU Process Memory(pid_*)\Local Usage` | Byte integer with thousands separators |
| `GPU S` | `gpu_shared_bytes` | Shared system memory used by the process for GPU resources. | PDH `\GPU Process Memory(pid_*)\Non Local Usage` | Byte integer with thousands separators |
| `IO Read/s` | `io_read_bytes_per_sec` | Process read I/O throughput, including file, network, and device I/O. | PDH `IO Read Bytes/sec` | `Mbps` |
| `IO Write/s` | `io_write_bytes_per_sec` | Process write I/O throughput, including file, network, and device I/O. | PDH `IO Write Bytes/sec` | `Mbps` |

## Process Metrics Defined Internally Only

| Display name | Log field | Description | Current behavior |
|---|---|---|---|
| `WS Shrbl` | `workset_shareable_bytes` | Shareable pages in the Working Set. | Normally not collected because `collect_ws_share=false`. |
| `WS Shrd` | `workset_shared_bytes` | Shareable pages that are actually shared. | Normally not collected because `collect_ws_share=false`. |

These metrics use the heavy `QueryWorkingSet` collection path, so they are currently excluded from normal monitoring.

## System / RAM / VRAM Metrics

The `RAM/VRAM` panel and system details use four metrics.

| Display name | Description | Primary source | Display format |
|---|---|---|---|
| `Physical Memory` | Physical memory used by the OS and total installed physical memory. | `sysinfo` and PDH `Available Bytes` | `used / total MB` |
| `Committed` | OS-wide committed bytes and commit limit. | PDH `Committed Bytes`, `Commit Limit` | `used / total MB` |
| `GPU Dedicated` | Dedicated GPU memory usage and capacity. | GPU PDH counters, DXGI adapter description | `used / total MB` |
| `GPU Shared` | Shared GPU memory usage and capacity. | GPU PDH counters, DXGI adapter description | `used / total MB` |

`RAM/VRAM` metrics always retain 7,200 samples and do not have a Tracked List display.
When the `RAM/VRAM` panel has focus, `1` / `2` / `3` / `4` assign the selected metric to the corresponding Graph slot.

## System Info

The `System Info` panel is not part of metric history. It displays supporting information about the current environment.

| Display name | Description | Primary source |
|---|---|---|
| `CPU` | CPU name and clock. | Registry, `sysinfo` |
| `Cores` | Topology summary such as P-cores and E-cores. | `GetLogicalProcessorInformationEx` |
| `Cache` | CPU cache summary. | `GetLogicalProcessorInformationEx` |
| `GPU` | GPU name and VRAM capacity. | DXGI |
| `Disk` | Used / total capacity for each disk. | `sysinfo` disk APIs |

## Process Info

Pressing `i` switches from `System Info` to `Process Info`, which displays supporting information for the selected process.
Process Info is collected on a worker thread after the selected row has been stable for 200 ms.
While collection is pending or running, the UI keeps the most recently displayed Process Info. If no information has been displayed yet, it shows `--`.

| Display name | Description |
|---|---|
| `Process` | Process name and PID. |
| `Parent` | Parent process information. |
| `Started` | Start time and uptime. |
| `Executable` | Executable path. |
| `Command` | Command line. |
| `File` | Modified time, file size, and product version. |

Unavailable values are displayed as one of `<access denied>`, `<exited>`, `<not available>`, `<missing>`, or `--`.

## Open Files

`f` (with the Processes panel focused) displays disk file handles for the selected live process, grouped by path.
This is a supporting investigation tool after an increase in `Hndl` has been found, not a metric that is sampled continuously.
While the Open files modal is open, `Ctrl+U` refreshes the list on demand without queuing another request if a collection is already running.

Sources are `NtQuerySystemInformation(SystemExtendedHandleInformation)`, `DuplicateHandle`, `GetFileType(FILE_TYPE_DISK)`, and `GetFinalPathNameByHandleW`.
The app displays what can be collected with normal user permissions. Permission failures and handles that cannot be duplicated are treated as uncollected counts or `<access denied>`.
Running as administrator may reveal more handles, but administrator privileges are not a prerequisite.

The display table shows handle count, file name, and directory.
It does not show a true file-open timestamp because the stable file metadata timestamps available through Windows are file timestamps, not the time when the target process opened that handle.

When copying to the clipboard, use raw text without a header.
Usually this is only the path. If the same path has multiple handles, copy `path<TAB>count`.

## Meaning of CPU%

`CPU%` means "what percentage of total logical CPU capacity the target process is using."

PDH `\Process(*)\% Processor Time` can sum values across multiple logical CPUs. Therefore, the value is read with `PDH_FMT_NOCAP100`, divided by the logical CPU count, and then clamped to `0.0..=100.0`.

Examples:

- On a 16-logical-CPU machine, a process fully using 1 logical CPU is about `6.25%`.
- On a 16-logical-CPU machine, a process fully using all logical CPUs is about `100%`.

## Sampling Frequency

The base screen update interval is fixed at 1 second. `interval_seconds` exists in the config file, but the runtime is fixed to 1 second.

Heavy metrics are not collected every second.

| Kind | Frequency | Target |
|---|---:|---|
| Normal sample | Every 1 second | `sysinfo`, PDH process counters, thread count, handle count |
| Slow sample | Every 5 seconds | GUI resources, GPU usage/capacity, WS share metrics |

Slow-sample values are cached until the next slow sample.

## History Retention

| Target | Retained samples | Notes |
|---|---:|---|
| General process | 120 | About 2 minutes. |
| Tracked process | 7,200 | About 2 hours. |
| System metrics | 7,200 | Used for `RAM/VRAM` details. |

Process history identity consists of PID, process name, and start time.
When start time is available, it is included in the identity to avoid mixing history after PID reuse.

## Display Formats

| Kind | Display |
|---|---|
| Byte-based process metric | Byte integer with thousands separators. |
| System memory / VRAM | MB. |
| GPU name / capacity | `name / N GB VRAM`. |
| Disk summary | Aggregated on one line, such as `C: used/total GB`. |
| I/O speed | `Mbps`. |
| CPU% | 1 decimal place. |
| GPU% | 1 decimal place. |
| Missing value | `--`. |

`GB`, `MB`, and `Mbps` are rounded using a base of 1,000.

## Metrics in Recording Logs

Recording logs are JSON Lines. The current writer outputs `schema_version: 2`.
`record_type: "session"` stores session information, `record_type: "frame"` stores one sample, and `record_type: "end"` stores end information.
The reader currently loads only `schema_version: 2`.
Compatibility with older schemas is deferred until v1.0.0 or later.

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
| `interval_seconds` | number | Currently `1`. |
| `tracked_names` | string array | Tracked List at session start. |
| `columns` | string array | Process metric columns currently displayed. |
| `sort` | object | Sort column / direction. |
| `system` | object | Supporting information such as CPU / GPU names. |

Frame record fields:

| Field | Type | Description |
|---|---|---|
| `schema_version` | number | `2`. |
| `record_type` | string | `frame`. |
| `session_id` | string | Same ID as the session record. |
| `captured_at` | string | RFC 3339 timestamp. |
| `tracked_names` | string array | Tracked List at frame creation time. |
| `system_metrics` | object | RAM/VRAM metrics that have history. |
| `processes` | object array | Live processes matching the Tracked List. |

Process object fields:

| Field | Type | Description |
|---|---|---|
| `pid` | number | PID. |
| `name` | string | Process name. |
| `start_time` | number | Present only when available. |
| `metrics` | object | Only metrics that were collected. |

A `frame` record outputs processes matching the Tracked List and RAM/VRAM system metrics.

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
    "committed_bytes": 2345678901,
    "commit_limit_bytes": 68719476736
  },
  "processes": [
    {
      "pid": 1234,
      "name": "app.exe",
      "start_time": 1700000000,
      "metrics": {
        "private_bytes": 123456789,
        "workset_private_bytes": 98765432
      }
    }
  ]
}
```

`metrics` contains only values that were collected. Values that could not be collected are omitted rather than written as `null`.
For compatibility, the reader also accepts `null` as a missing value.
Missing values are displayed as `--` in the UI and are not treated as 0 in Graph.
