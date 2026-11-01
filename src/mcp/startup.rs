//! Lazy, visible application startup for a validated MCP tool call.
use super::{transport::Client, transport::error};
use crate::{config, platform};
use serde_json::Value;
use std::{
    ffi::{OsStr, OsString},
    io,
    os::windows::{ffi::OsStrExt, ffi::OsStringExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(12);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

fn unavailable(message: &str) -> Value {
    error("BACKEND_UNAVAILABLE", message, true)
}

/// Only an absent pipe permits considering launch. Access denial, capacity,
/// peer verification and protocol failures must never start a replacement.
fn pipe_absent(error: &io::Error) -> bool {
    error.raw_os_error() == Some(winapi::shared::winerror::ERROR_FILE_NOT_FOUND as i32)
}

pub(crate) async fn connect(cancelled: impl Fn() -> bool) -> Result<Client, Value> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut launch_guard = None;
    let mut launcher: Option<Child> = None;
    let mut launched = false;
    let mut saw_running = false;
    loop {
        // Once launched, keep the coordinator until readiness or the deadline,
        // even if this request is cancelled. Do not create duplicate windows.
        if cancelled() && !launched {
            return Err(error(
                "CANCELLED",
                "Request cancelled before application startup",
                false,
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(unavailable(if launched {
                "Windows Terminal was launched, but WinProc TUI did not become ready within 12 seconds. Check its window, startup selection, Settings and integrity level; retry after it is ready."
            } else {
                "WinProc TUI is running or another bridge is starting it, but MCP is unavailable. Check its window, Disable MCP connections, listener status and integrity level."
            }));
        }
        match timeout(remaining.min(CONNECT_TIMEOUT), Client::connect()).await {
            Ok(Ok(client)) => return Ok(client),
            Ok(Err(error)) if pipe_absent(&error) => {}
            _ => {
                return Err(unavailable(if launched {
                    "Windows Terminal was launched, but its application connection failed. Check the TUI window, connection capacity, integrity level and listener compatibility."
                } else {
                    "Cannot connect to WinProc TUI. Check connection capacity, logon/integrity level and listener compatibility. No replacement application was started."
                }));
            }
        }
        if let Some(child) = launcher.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => {
                    return Err(unavailable(
                        "Windows Terminal rejected the launch. Check Windows Terminal installation and profile settings, or start WinProc TUI manually.",
                    ));
                }
                Ok(Some(_)) => launcher = None,
                Ok(None) => {}
                Err(_) => {
                    return Err(unavailable(
                        "Cannot observe Windows Terminal startup. Check its window before retrying.",
                    ));
                }
            }
        }
        let running = platform::single_instance_running().map_err(|_| unavailable(
            "Cannot determine whether WinProc TUI is running. Check the Windows session, integrity level and any Terminal window before retrying."
        ))?;
        saw_running |= running;
        if !launched && !saw_running {
            if launch_guard.is_none() {
                launch_guard = platform::acquire_mcp_launch_guard().map_err(|_| {
                    unavailable(
                        "Cannot coordinate WinProc TUI startup; no application was started.",
                    )
                })?;
                // Recheck pipe and application presence after acquiring the gate.
                if launch_guard.is_some() {
                    continue;
                }
            } else {
                if cancelled() {
                    return Err(error(
                        "CANCELLED",
                        "Request cancelled before application startup",
                        false,
                    ));
                }
                let executable = std::env::current_exe()
                    .map_err(|_| unavailable("Cannot locate the WinProc TUI executable."))?;
                let paths =
                    config::resolve_config_paths_from_executable(&executable).map_err(|_| {
                        unavailable("Cannot resolve the WinProc TUI configuration location.")
                    })?;
                if saved_connections_disabled(&paths).map_err(|_| unavailable(
                    "Cannot read the WinProc TUI configuration safely. Start the application manually and check its configuration."
                ))? {
                    return Err(unavailable(
                        "MCP connections are disabled in the saved configuration. Start WinProc TUI manually and uncheck Disable MCP connections in Settings."
                    ));
                }
                // Keep the absolute launch spelling after resolving configuration.
                // Launching the canonical target would bypass normal migration of
                // a legacy configuration beside a command link/symlink.

                let terminal = terminal_path().ok_or_else(|| unavailable(
                    "Windows Terminal is unavailable. Install or enable its wt.exe app execution alias, or start WinProc TUI manually."
                ))?;
                launcher = Some(terminal_command(&terminal, &executable).spawn().map_err(|_| unavailable(
                    "Cannot launch Windows Terminal. Install or enable its wt.exe app execution alias, or start WinProc TUI manually."
                ))?);
                launched = true;
            }
        }
        sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now()))).await;
    }
}

// Mirror configuration selection without migrating, rewriting, or logging any
// user paths/content. A running application's runtime setting remains authoritative.
fn saved_connections_disabled(paths: &config::ConfigPaths) -> anyhow::Result<bool> {
    let path = if paths.active.try_exists()? {
        Some(paths.active.as_path())
    } else {
        match paths.legacy.as_deref() {
            Some(path) if path.try_exists()? => Some(path),
            _ => None,
        }
    };
    let Some(path) = path else { return Ok(false) };
    let raw = std::fs::read_to_string(path)?;
    let config: config::AppConfig = toml::from_str(&raw)?;
    Ok(config.general.mcp_connections_disabled)
}

fn terminal_path() -> Option<PathBuf> {
    // Resolve the Windows app execution alias explicitly, never a wt.exe in the
    // client's working directory or next to an investigated executable.
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("Microsoft/WindowsApps/wt.exe"))
}

// WT splits commands on semicolons even inside an argv element. Its parser
// removes this backslash escape before passing the literal path to CreateProcess.
fn terminal_argument(value: &OsStr) -> OsString {
    let mut result = Vec::new();
    for character in value.encode_wide() {
        if character == b';' as u16 {
            result.push(b'\\' as u16);
        }
        result.push(character);
    }
    OsString::from_wide(&result)
}

fn terminal_command(terminal: &Path, executable: &Path) -> Command {
    let directory = executable
        .parent()
        .expect("absolute executable has a parent");
    let mut command = Command::new(terminal);
    // A visible terminal needs the normal user environment, not the MCP
    // client's NO_COLOR / TERM=dumb settings or inherited terminal session.
    command.args([
        "-w",
        "new",
        "new-tab",
        "--title",
        "WinProc TUI",
        "--suppressApplicationTitle",
        "--reloadEnvironment",
        "-d",
    ]);
    command.arg(terminal_argument(directory.as_os_str()));
    command
        .arg("--")
        .arg(terminal_argument(executable.as_os_str()));
    command.current_dir(directory);
    // The TUI receives its own terminal. Never inherit the bridge's MCP streams.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_request_never_attempts_connection_or_launch() {
        let result = connect(|| true).await;
        assert!(matches!(result, Err(value) if value["error"]["code"] == "CANCELLED"));
    }

    #[test]
    fn only_missing_pipe_allows_launch() {
        for code in [5, 231, 109] {
            assert!(!pipe_absent(&io::Error::from_raw_os_error(code)));
        }
        assert!(!pipe_absent(&io::Error::new(
            io::ErrorKind::InvalidData,
            "version"
        )));
        assert!(pipe_absent(&io::Error::from_raw_os_error(2)));
    }

    #[test]
    fn terminal_arguments_preserve_paths_and_force_a_new_window() {
        let executable = Path::new(r"C:\Tools & 日本語\; monitor\winproc-tui.exe");
        let command = terminal_command(Path::new(r"C:\WindowsApps\wt.exe"), executable);
        let args: Vec<_> = command.get_args().map(OsStr::to_os_string).collect();
        assert_eq!(&args[..3], ["-w", "new", "new-tab"]);
        assert_eq!(args[6], "--reloadEnvironment");
        assert_eq!(args[8], r"C:\Tools & 日本語\\; monitor");
        assert_eq!(args[9], "--");
        assert_eq!(args[10], r"C:\Tools & 日本語\\; monitor\winproc-tui.exe");
        assert_eq!(command.get_current_dir(), executable.parent());
    }

    #[test]
    fn saved_preference_respects_active_and_legacy_without_writes() {
        let root = std::env::temp_dir().join(format!("winproc-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let paths = config::ConfigPaths {
            active: root.join("active.toml"),
            legacy: Some(root.join("legacy.toml")),
        };
        assert!(!saved_connections_disabled(&paths).unwrap());
        let legacy = paths.legacy.as_ref().unwrap();
        std::fs::write(legacy, "[general]\nmcp_connections_disabled = true\n").unwrap();
        assert!(saved_connections_disabled(&paths).unwrap());
        assert!(!paths.active.exists());
        std::fs::write(&paths.active, "[general]\nmouse = true\n").unwrap();
        assert!(!saved_connections_disabled(&paths).unwrap());
        std::fs::write(&paths.active, "invalid = [").unwrap();
        assert!(saved_connections_disabled(&paths).is_err());
        assert_eq!(
            std::fs::read_to_string(legacy).unwrap(),
            "[general]\nmcp_connections_disabled = true\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
