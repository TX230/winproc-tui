# winproc-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows 11 x64](https://img.shields.io/badge/Platform-Windows%2011%20x64-0078D6?logo=windows&logoColor=white)](#requirements)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)

Languages: [English](README.md) | [Japanese](README.ja.md)

`winproc-tui` is a **process investigation tool for Windows 11, built for developers**.
It launches quickly from the terminal and lets you observe **current values** and **changes over time** for memory, handles, GUI resources, GPU memory, I/O, and other process metrics — all in a single screen. With up to four Graphs, A/B comparison, and JSON Lines recording with Playback, it is well suited for grasping the resource behavior of an application you are developing on the spot. Rather than competing on coverage with tools like Process Explorer or System Informer, `winproc-tui` is laser-focused on **quickly answering "what resources the app I am running right now uses, when, and how much"**.

![winproc-tui main screen showing the process list, multiple Graphs, Samples, and A/B comparison](assets/screenshots/main-screen.png)

## Features

- **Monitoring**: Shows RAM / VRAM, a compact CPU panel with average and per-logical-CPU load, and key per-process metrics in a table. Sorting, column selection, filtering, and jump search help you narrow down the target.
- **Graphing**: Lays out selected metrics in up to four Graph / Samples slots so you can review time-series movement and individual sample values. General process history keeps about 120 seconds, while tracked-process, RAM / VRAM, and CPU average history keep about 7,200 seconds.
- **Tracking (Tracked List)**: Registers process names of interest and can show only tracked rows. Their last collected values remain visible after the processes exit.
- **Recording and Playback**: Saves tracked processes, RAM / VRAM, and CPU average as JSON Lines logs and replays them later in the same Processes / Graph / Samples / A/B view layout.
- **A/B comparison**: Marks any two points as A and B, then shows the value difference and elapsed time between them.
- **Open files**: Lists the files a selected live process has open.
- **Interaction support**: `Ctrl+C` copies the selected row to the clipboard, `F2` switches themes, and mouse-based row selection and scrollbars are supported.

## When This Helps

- You want to track an application's resource usage over time and watch for memory leaks.
- You want to quantify how much a specific operation leaks (the difference between two points).
- You want to detect missed file closes and see which files a process currently has open.
- You want to **record a background service over a long period** and review the area around an incident with Playback.
- You want to compare resource usage before and after a refactor.

## Requirements

- OS: Windows 11 x64

This project is Windows-only. Linux, macOS, and other platforms are not supported.

## Use a Prebuilt Binary

Release binaries are available from [GitHub Releases](https://github.com/TX230/winproc-tui/releases).
Download the zip, extract it to any folder, and run `winproc-tui.exe`.
No additional runtime or installer is required.

## Build From Source

If you want to try in-development code, you can build from source.

### 1. Install the Rust Toolchain

On Windows, [rustup](https://rustup.rs/) is recommended.
Building requires Rust 1.95.0 or later, the Rust 2024 edition, and the MSVC linker (the C++ toolchain from Build Tools for Visual Studio 2026).

Using winget:

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet --wait --norestart"
```

Verify the installation:

```powershell
rustup --version
rustc --version
cargo --version
```

### 2. Build and Run

```powershell
git clone https://github.com/TX230/winproc-tui.git
cd winproc-tui
cargo build --release
```

The executable is generated at `target\release\winproc-tui.exe`.
After building, launch it in either of the following ways:

```powershell
cargo run --release
# or run the built binary directly
.\target\release\winproc-tui.exe
```

### 3. Install as a Command (Optional)

Running `cargo install --path .` installs `winproc-tui.exe` into your per-user cargo bin directory (by default `%USERPROFILE%\.cargo\bin`).
That directory is on your PATH, so afterwards you can launch the tool from anywhere by simply typing `winproc-tui`.

```powershell
cargo install --path .
winproc-tui
```

## Command-Line Options

There are currently only two startup options.


| Option          | Description   |
| --------------- | ------------- |
| `-h, --help`    | Show help.    |
| `-V, --version` | Show version. |


## Controls

Only the main keys are listed in this README.
**Press** `?` **while running to view the full key bindings in the Help dialog.**

Some single-letter keys such as `f` map to different actions depending on which panel is focused. The tables below are grouped by panel, and the active panel is also shown in the footer.

### General


| Key                 | Action                                                              |
| ------------------- | ------------------------------------------------------------------- |
| `?`                 | Show / hide Help.                                                   |
| `q` / `Esc`         | Open the quit confirmation (returns to live display during Playback). |
| `Tab` / `Shift+Tab` | Move focus.                                                         |
| `Ctrl+C`            | Copy the selected row text from the focused panel.                  |
| `Ctrl+L`            | Open the log list.                                                  |
| `Ctrl+R`            | Start / stop recording.                                             |
| `Ctrl+P`            | Pause / resume screen updates.                                      |
| `Ctrl+O`            | Open the Settings dialog.                                           |
| `Ctrl+Wheel`        | Change the Windows Terminal zoom level.                             |
| `F2`                | Switch theme.                                                       |


### Process Controls


| Key                 | Action                                                                                |
| ------------------- | ------------------------------------------------------------------------------------- |
| `Ctrl+F`            | Filter the process list by name, or by executable path when the `Full Path` column is selected. |
| `Ctrl+I` / `Ctrl+J` | Process-name incremental search.                                                      |
| `1` – `4`           | Show the selected process, RAM / VRAM, or CPU Avg metric in Graph#1 – Graph#4 (press the same number again to clear). |
| `0`                 | Clear all Graphs and close the Graph panel.                                           |
| `s`                 | Sort by the selected column (press again to switch ascending / descending).           |
| `c`                 | Open the column picker.                                                               |
| `Shift+Up/Down`     | Select a continuous range of live process rows.                                       |
| `Ctrl+Up/Down`      | Move the cursor without changing the multi-selection.                                 |
| `Ctrl+Space`        | Add or remove the current live process row from the multi-selection.                  |
| `Shift+Left/Right`  | Move the selected metric column left or right.                                        |
| `Space`             | Add or remove the selected process name from the Tracked List.                        |
| `Delete`            | Confirm, then kill the selected live process rows with `taskkill /f /im`.             |
| `t`                 | Toggle whether only tracked processes are shown.                                      |
| `f`                 | Open the Open files list for the selected live process.                               |
| `g`                 | Open or close all configured Graphs at once.                                          |


### Graph and A/B Comparison


| Key                        | Action                                                                              |
| -------------------------- | ----------------------------------------------------------------------------------- |
| `Left` / `Right`           | Move the selected sample.                                                           |
| `Ctrl+Left` / `Ctrl+Right` | Pan the visible range.                                                              |
| Right drag / `Ctrl`+left drag | Pan the visible range with the mouse.                                            |
| `PageUp` / `PageDown`      | Change the visible time span.                                                       |
| `f`                        | Switch to a time span that fits all samples.                                        |
| `z`                        | Toggle the Y-axis lower bound between fixed at 0 and following the visible minimum. |
| `a` / `b`                  | Mark the selected sample as point A or point B.                                     |
| `Shift+A` / `Shift+B`      | Jump to point A or point B.                                                         |
| `x`                        | Clear the A/B comparison.                                                           |


When multiple Graphs are shown, the visible time span and A/B points are shared across slots, while the Y-axis scale is independent per Graph.
If there is not enough display area, the message `Not enough display area.` is shown and the Graph is not added.

## Recording and Playback

Press `Ctrl+R` to start or stop recording.
Recording targets processes that match the Tracked List and saves logs as JSON Lines (with the `.log` extension).
When recording starts, a save-path input dialog opens, and `Tab` completes directory names there.
Playback cannot start during recording, and recording cannot start during Playback.

Press `Ctrl+L` to open the log list.
The list shows `*.log` files from the previous recording directory if available, otherwise from the current directory.
The `Dir` row shows the directory currently being searched, and `d` lets you choose another directory.
Press `Enter` on a selected log to switch to the `PLAY` display and inspect the saved session through Processes / Graph / Samples / A/B comparison.
Playback is a viewer for saved sessions, not real-time replay. Press `Esc` to return to the live display.

The recording log format and the meaning of each field are described in [docs/metrics.md](docs/metrics.md).

## Configuration File

The configuration file is `winproc-tui.toml`, placed next to the executable.
If the file does not exist, defaults are used.
On exit, the current theme, process-table columns, sort, Tracked Only state, and tracked list are saved.
Filter input state is not carried over to the next launch.

Example:

```toml
[general]
mouse = true
theme = "Dark"

[process_table]
preset = "Default"
columns = ["Private", "WS Priv"]
sort_by = "WS Priv"
sort_order = "desc"
tracked_only = false

[[tracked]]
name = "app.exe"
```

The sampling interval is fixed to 1 second in the current version and is not user-configurable.

## Developer Docs

- [docs/metrics.md](docs/metrics.md): Metrics, data sources, and display formats.
- [docs/architecture.md](docs/architecture.md): Architecture, responsibility boundaries, and data flow.

## Non-Goals

`winproc-tui` does not aim to be:

- A full replacement for Process Explorer or System Informer.
- A tool that assumes administrator privileges for detailed collection.

It is a tool for quickly observing process changes during short development and verification sessions.

## Bug Reports and Feature Requests

Please report bugs and request features via GitHub Issues.
Templates are provided for both bug reports and feature requests.

This is a personal project. Unsolicited pull requests from external contributors are not accepted; use Issues for feedback and feature requests instead.

Issues may be written in either English or Japanese. The user-facing README is maintained in both languages, while detailed specification documents under `docs/` are kept in English only.

## License

MIT License. See [LICENSE](LICENSE) for details.
