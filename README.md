# winproc-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows 11 x64](https://img.shields.io/badge/Platform-Windows%2011%20x64-0078D6?logo=windows&logoColor=white)](#requirements)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)

Language: [English](README.md) | [日本語](README.ja.md)

`winproc-tui` is a keyboard-first process monitor for Windows 11. It shows how an application's memory, handles, GUI resources, GPU memory, I/O, and other metrics change over time, directly in the terminal.

Select the metrics that matter, keep up to 16 Graphs in one workspace, compare exact points with A/B markers, and record sessions for later inspection. The focus is fast, repeatable investigation of selected processes during development and testing—not broad system inspection.

![The winproc-tui main screen showing system and process metrics, the Graph Workspace, Samples, and an A/B comparison](assets/screenshots/main-screen.png)

_Private memory growth shown alongside system activity, with A and B marking the interval to compare._

## Install

Official Windows binaries are published only on [TX230/winproc-tui Releases](https://github.com/TX230/winproc-tui/releases). WinGet and the [TX230 Scoop Bucket](https://github.com/TX230/scoop-bucket) install those same binaries. Copies, mirrors, and modified repositories are not official builds.

### WinGet

```powershell
winget install --id TX230.winproc-tui -e
winproc-tui
```

Update or uninstall with:

```powershell
winget upgrade --id TX230.winproc-tui -e
winget uninstall --id TX230.winproc-tui -e
```

### Scoop

```powershell
scoop bucket add tx230 https://github.com/TX230/scoop-bucket
scoop install tx230/winproc-tui
winproc-tui
```

Refresh the registered buckets, then update the app:

```powershell
scoop update
scoop update winproc-tui
```

Uninstall with `scoop uninstall winproc-tui`. A normal uninstall preserves saved settings; use `scoop uninstall --purge winproc-tui` to remove them as well.

## Quick Start

### See a Process Change Over Time

1. Select a process in `PROCESSES`.
2. Use `Left` / `Right` to select a metric column such as `PrivBytes`.
3. Press `Space` or double-click the metric cell to add a Graph. Repeat with other metrics to compare up to 16 Graphs in the workspace.

System MEM, GPU, CPU, and network/disk metrics can be graphed directly from their panels; they do not require a Tracking List entry.

![Graph Workspace showing 12 metrics in a three-column layout](assets/screenshots/main-screen-12slots.png)

_A three-column Graph Workspace comparing 12 metrics while preserving the process and system overview._

### Compare Two Points

Move focus to a Graph or Samples and choose a sample with `Left` / `Right`. Press `a` at the start point and `b` at the end point to see the value difference and elapsed time. Press `x` to clear the comparison.

### Track and Record Processes

1. Select a Process or PID cell and press `Space`, double-click it, or press `t` to add that process name to the working Tracking List.
2. Use `Ctrl+T` when you want to save or load named Tracking Lists.
3. Press `Ctrl+R`, choose a log path and a `1s`, `2s`, `5s`, or `10s` recording interval, then start recording.
4. Press `Ctrl+R` again and confirm with `y` to stop. `Enter`, `Esc`, or `n` continues recording.
5. Press `Ctrl+L` to reopen a saved log for inspection.

Recording requires at least one process name in the Tracking List, but no matching process needs to be running yet. The names are captured when Recording starts and remain fixed for that session. `Shift+T` can still switch between All processes and Tracked-only because it changes only the display.

Use `Tab` / `Shift+Tab` to move between panels and the arrow keys to select rows, columns, and samples. Press `F1` or `?` at any time for the complete controls; the footer shows the main actions available in the current context.

## Capabilities

- **Live monitoring**: Shows system memory pressure, per-adapter GPU load and memory, network and disk activity, CPU activity, and detailed per-process metrics.
- **Graphs and A/B comparison**: Keeps up to 16 metrics in an ordered workspace with synchronized Samples, then compares any two exact sample times.
- **Tracking Lists**: Saves named sets of process names, supports a Tracked-only view, and retains the latest values after a tracked process exits.
- **.NET metrics**: Automatically detects live .NET 8/9/10 processes and shows managed-runtime metrics, with selected heap metrics for .NET Framework 4.8.
- **Process Info**: Brings metrics, image and runtime details, open files, DLLs, and environment variables together for the selected process.
- **Recording and Log view**: Records system metrics and matching processes as JSON Lines, then reopens them in the same Processes, Graph, Samples, and A/B views.

Layouts, visible columns, sorting, and Tracking Lists are restored on the next launch. Filter input is not saved.

## When to Use It

`winproc-tui` is a good fit when you need to:

- Find out whether a process's memory or handle usage keeps growing.
- Measure resource usage before and after a specific operation or code change.
- See which files and DLLs a process is using and which runtime it loaded.
- Record a background service and return to the period around a later incident.

Use Windows Performance Monitor (PerfMon) for arbitrary counters, remote monitoring, and managed Data Collector Sets. Use Process Explorer or System Informer for broad system inspection. Choose `winproc-tui` when the task is to follow selected processes and compare their recent behavior quickly and repeatedly.

## Recording and Log View

Recording captures the names in the working Tracking List when the session starts. If none currently match a live process, the log still records system memory, per-adapter GPU, aggregate CPU, and network/disk activity; only the process list remains empty.

Live collection and Live history remain at one-second resolution. Longer recording intervals average available samples, reducing file size and Log-view loading work while smoothing short spikes. A recording lasts for at most 24 hours and returns to Live automatically at the limit.

Log view does not replay frames. It shows the final process snapshot and lets you inspect recorded history through Graphs, Samples, Process Info, and A/B comparison. Recording and Log view cannot be active at the same time.

See [docs/metrics.md](docs/metrics.md) for metric definitions, aggregation behavior, and the recording schema.

## Requirements

- Windows 11 x64 only; Linux and macOS are not supported.

Administrator privileges are not required for normal monitoring. Protected processes may prevent access to some Process Info data or open files; unavailable values are shown as `--`.

Only one `winproc-tui` instance can run in the same Windows session. A second instance exits without changing the active terminal or saved session settings.

## Build From Source

Building requires Rust 1.95.0 or later, the Rust 2024 edition toolchain, and the MSVC linker from Build Tools for Visual Studio 2026. Install Rust with [rustup](https://rustup.rs/), then build the project:

```powershell
git clone https://github.com/TX230/winproc-tui.git
cd winproc-tui
cargo build --release
.\target\release\winproc-tui.exe
```

Rust developers can optionally install the current checkout as a command:

```powershell
cargo install --path .
```

## More Information

- Press `F1` or `?` in the app for the complete keyboard and mouse controls.
- Run `winproc-tui --help` for command-line options.
- See [docs/metrics.md](docs/metrics.md) for metrics, data sources, display formats, and recording logs.
- See [docs/architecture.md](docs/architecture.md) for the system overview and links to focused design documents.

## Bug Reports and Feature Requests

Use [GitHub Issues](https://github.com/TX230/winproc-tui/issues) for bug reports and feature requests. Issues may be written in English or Japanese.

Report suspected security vulnerabilities privately as described in [SECURITY.md](SECURITY.md), not in a public Issue.

This is a personal project and does not accept unsolicited pull requests from external contributors. Opening or discussing an Issue does not authorize a pull request. See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution policy.

## License

MIT License. See [LICENSE](LICENSE).
