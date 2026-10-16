# Resource workload scenarios

`tools/resource-workloads/` is a standalone Windows 11 x64 developer tool for metric validation and screenshot preparation (Issue #160). It is not part of the application, runtime zip, or crates.io package. It never edits application configuration or starts Recording automatically.

## Build and run

Requirements: PowerShell 7, .NET 10 SDK/runtime, and Visual Studio C++ Build Tools with the Windows SDK. No third-party NuGet packages are required. The Direct3D 11 helper uses hardware only and statically links the MSVC CRT.

From this checkout:

```powershell
./tools/resource-workloads/build.ps1

# One full-length scenario, quarter-scale resource sizes.
./tools/resource-workloads/run.ps1 -Mode verify -Workload memory -Scale 0.25

# All verification scenarios, sequentially (about 32 minutes).
./tools/resource-workloads/run.ps1 -Mode verify

# Six concurrent processes, ten minutes, half-scale resource sizes.
./tools/resource-workloads/run.ps1 -Mode screenshot -Background

# Short mechanical checks, including hardware GPU tests.
./tools/resource-workloads/test.ps1
./tools/resource-workloads/test-comparison.ps1
```

Build outputs are under `target/resource-workloads/`. All `demo-*.exe` apphosts use the same managed assembly and runtime configuration; keep the publish directory together. **All workloads run inside .NET**, including native-memory, CPU and GPU modes. Their managed runtime, logging allocations and driver overhead are part of the baseline. These programs cannot validate native-process `.NET --` behavior.

The launcher prints process names, PIDs, evidence directory and a cooperative stop command. `processes.json` records the names and PIDs. Outputs go to a new `logs/workloads/<run>/` directory. `-Output` must name a new directory; existing runs are not overwritten. Screenshot mode runs concurrently; verification runs sequentially. `-Workload` selects a subset.

A foreground launcher requests cleanup on interruption. A background run returns immediately; create its printed `stop.request` file to stop it. Individual executables also support Ctrl+C. There is no forced termination. Every child has its own duration limit, including when its launcher exits. Background launch success is not completion success: inspect every `end` record.

## Bounds and timing

- `-Scale`: 0.0625 through 1; scales memory, object counts, I/O and managed allocation rates. Counts round down with a minimum of one; GPU payload rounds up to a multiple of 4 MiB. CPU duty and worker count are independent.
- `-Seconds`: 1 through 3,600 scenario seconds; default 120 for verification and 600 for screenshots.
- `-TimeScale`: 0.025 through 1; multiplies duration and phase timing, not resource sizes or transfer rates. Default 1. Shortened runs validate mechanics only, not TUI or EventPipe sampling accuracy.
- `-Adapter`: zero-based DXGI hardware adapter index, excluding software adapters. The `adapter` record gives the name and LUID. Match the LUID rather than assuming the TUI ordinal is identical. An unavailable adapter fails, without WARP fallback.
- `-NaturalGc`: disables deliberate collections. Launcher presets otherwise request non-compacting full GC at managed retention transitions. Direct executable invocation defaults to natural GC; pass `--force-gc true` for controlled retention tests.
- CPU defaults to one worker at 50% / 100% duty. Direct invocation accepts `--cpu-workers N`, bounded to `min(16, logical CPU count)`.

At scale 1, native memory holds at most 768 MiB, managed retention holds 576 MiB of payload plus overhead, and GPU texture payload reaches 512 MiB. Each I/O process uses one fixed 64 MiB scratch file. These are per-process limits; concurrent runs add their costs. Select a lower scale on constrained hosts. The suite does not deliberately exhaust system resources.

## Verification scenarios

Except native memory, the schedule is 0–15 s baseline, 15–45 s low, 45–75 s high, 75–105 s released, and 105–120 s settled. GPU devices/runtime infrastructure persist until exit: released payload does not imply zero process usage.

Values below are at scale 1:

| Workload | Low | High | Evidence |
|---|---|---|---|
| `cpu` | One worker, 50% duty | Same worker, 100% duty | CPU-time delta and iterations; expected total-capacity percentage is approximately `100 * workers * duty_fraction / logical_cpus`. |
| `threads` | 8 waiting threads | 32 | Successful worker count and OS thread count. |
| `handles` | 200 event handles | 1,000 | Successful event count and OS handle count. |
| `files` | 4 held files | 16 | Actual filenames for live Files and file-user inspection. |
| `io-read` | 8 MiB/s target | 32 MiB/s target | Actual cumulative read bytes; fixed 64 MiB circular file. |
| `io-write` | 8 MiB/s target | 32 MiB/s target | Actual cumulative write bytes; fixed 64 MiB circular file. |
| `dotnet-short` | 8 MiB/s in short-lived 16 KiB arrays | 32 MiB/s | Actual payload allocation, runtime allocation and GC counts. |
| `dotnet-gen2` | 64 MiB in retained 16 KiB arrays | 192 MiB | Retained payload and after-GC generation sizes. Natural GC does not guarantee immediate promotion. |
| `dotnet-loh` | 64 MiB in 1 MiB arrays | 256 MiB | Retained payload and after-GC LOH size. |
| `dotnet-poh` | 32 MiB in pinned 1 MiB arrays | 128 MiB | Pinned array allocation and after-GC POH size. |
| `dotnet-frag` | 64 MiB in 1 MiB arrays | Grow to 256 MiB, drop alternate arrays, retain 128 MiB | Non-compacting GC when enabled; fragmentation is runtime-dependent. |
| `gpu` | 128 MiB texture payload | 512 MiB | Initialized RGBA8 textures, one refresh pass per second; payload and DXGI local/nonlocal usage are separate. |
| `gpu-active` | 128 MiB, one pass per tick | Same payload, four passes per tick | At most about ten ticks/s; completed passes, not guaranteed GPU saturation. |
| `user` | 10 hidden windows | 50 | Successful window count and OS USER count. |
| `gdi` | 100 brushes | 500 | Successful brush count and OS GDI count. |

`dotnet` combines Gen2/LOH/POH retention with short-lived allocation. `objects` combines USER and GDI. Both run independently or in the screenshot preset.

Native `memory` uses: 0–15 s baseline; 15–35 s commit 256 MiB without touching every page; 35–55 s write one byte per page; 55–75 s add/touch 512 MiB; 75–95 s release the additional 512 MiB; 95–120 s release the remainder.

Private Bytes and Working Set measure different things. Resident bytes need not equal committed payload. Cached process I/O need not match physical Disk R/W. File preparation writes 64 MiB before workload traffic and is logged separately. Evidence logging also contributes process I/O. Requested throughput is not a physical-disk oracle.

## Screenshot preset

The default processes are `demo-cpu.exe`, `demo-memory.exe`, `demo-dotnet.exe`, `demo-gpu.exe`, `demo-io-write.exe`, and `demo-objects.exe`. Use `-Workload` to include threads, handles, held files or read I/O.

After 15 seconds of baseline, activity follows low/high/high/low levels in ten-second steps. Retained resources reach their high plateau at 45 seconds and stay there until shutdown. `capture` begins at 135 seconds, after two minutes of active history. CPU, allocation, GPU refresh and I/O continue cycling during capture. Memory stays steady after warmup; use verification mode for staircase/release screenshots.

1. Register executable names in Tracking before the run, or during the initial baseline. Select columns and start Recording at `1s`.
2. Launch the screenshot preset and check evidence for failures.
3. Wait for `capture`, then select Graphs such as CPU%, PrivBytes, .NET Gen2/LOH/POH, GPU D/S and I/O.
4. Capture the live terminal. DISPLAY PAUSED can freeze the display while collection and Recording continue.
5. Explicitly stop Recording and open the log to capture Log view. Live Files is a separate screenshot; file lists are not recorded.
6. Create `stop.request` or wait for expiry. Check every `end.success` and `cleanup.success`.

These are real resource changes, not fake displayed values. Scheduler, driver and GC differences prevent guaranteed pixel-identical numbers. UMA hardware need not show a Dedicated increase. Network traffic, Encode/Decode and deliberate system paging pressure are outside this suite.

## Evidence and Recording alignment

Workload JSONL schema 1 is independent of recording schema 3. Records include `start`, `phase`, `sample`, optional `adapter`/`setup`/`gc-request`, `error`, `cleanup` and `end`. Every record has UTC time, monotonic elapsed time, PID and workload. Start metadata includes process start time, options, runtime and CPU count.

`actual` contains successful resource counts, retained payload and cumulative work. `observed` contains process CPU time, memory, thread/handle/USER/GDI counts, runtime allocation totals and after-GC generation sizes. Divide counter differences by actual elapsed time for rates. Tool overhead means these are not independent measurements. Generation sizes and heap totals describe the last GC; payload excludes object headers.

GPU batches are bounded and waited on with a two-second timeout. Completed passes increase only after success. Device/allocation/query failures produce error evidence and nonzero exit. Ordinary stop/failure releases native pointers, windows, GDI objects, streams and GPU state. Only a unique, owned scratch directory within the output root is removed; evidence remains.

```powershell
./tools/resource-workloads/compare-recording.ps1 `
    -Evidence logs/workloads/<run>/dotnet/<evidence>.jsonl `
    -Recording <recording.jsonl> `
    -Output logs/workloads/<run>/dotnet-comparison.jsonl
```

The helper requires one schema-v3 session at `1s`, matches PID plus start time (one-second rounding tolerance), reads metric positions from the committed schema, and pairs the latest earlier workload sample within 1.5 seconds. Missing metrics stay null; stale evidence gives an unpaired row. Output is never overwritten; failure can leave partial diagnostic output.

Pairs are review evidence, not automatic accuracy verdicts. Compare stable phases away from transitions. USER/GDI has a slower TUI refresh cadence, and EventPipe needs startup time. Aggregated recordings require window-aware comparison and are rejected.

## Validation boundaries

`test.ps1` runs 16 scenarios at one-eighth scale and 40x shortened time, checks phase/resource/work evidence and cleanup, tests invalid GPU selection, and stops screenshot retention cooperatively. `-SkipGpu` explicitly omits hardware cases. `test-comparison.ps1` uses synthetic fixtures for mapping, missing values, stale evidence and PID-reuse rejection.

These checks validate the generator and comparison mechanics, not end-to-end TUI sampling accuracy or visual quality. For those, run at normal speed, inspect the application, and retain both logs and screenshots. Separate no-monitoring, collection and terminal-display conditions when measuring observer overhead.

References: [pinned arrays](https://learn.microsoft.com/en-us/dotnet/api/system.gc.allocatearray), [D3D11 textures](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11device-createtexture2d), [DXGI memory information](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgi1_4-idxgiadapter3-queryvideomemoryinfo).
