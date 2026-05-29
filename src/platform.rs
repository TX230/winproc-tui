use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use winapi::{
    shared::minwindef::{BOOL, DWORD, TRUE, WORD},
    um::{
        consoleapi::SetConsoleCtrlHandler,
        wincon::{
            CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT,
            CTRL_SHUTDOWN_EVENT,
        },
        winuser::{
            GetAsyncKeyState, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
            VK_CONTROL, VK_OEM_MINUS, VK_OEM_PLUS,
        },
    },
};

static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);
const CONSOLE_CLOSE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(4_500);
const CONSOLE_CLOSE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONTROL_NOT_HANDLED: BOOL = 0;

pub(crate) fn install_console_control_handler() -> std::io::Result<()> {
    let ok = unsafe { SetConsoleCtrlHandler(Some(console_control_handler), TRUE) };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn termination_requested() -> bool {
    TERMINATION_REQUESTED.load(Ordering::SeqCst)
}

pub(crate) fn mark_shutdown_complete() {
    SHUTDOWN_COMPLETE.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn console_control_handler(control_type: DWORD) -> BOOL {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            TERMINATION_REQUESTED.store(true, Ordering::SeqCst);
            TRUE
        }
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
            TERMINATION_REQUESTED.store(true, Ordering::SeqCst);
            wait_for_shutdown_complete(CONSOLE_CLOSE_CLEANUP_TIMEOUT);
            CONTROL_NOT_HANDLED
        }
        _ => CONTROL_NOT_HANDLED,
    }
}

fn wait_for_shutdown_complete(timeout: Duration) {
    let started_at = Instant::now();
    while !SHUTDOWN_COMPLETE.load(Ordering::SeqCst) && started_at.elapsed() < timeout {
        std::thread::sleep(CONSOLE_CLOSE_POLL_INTERVAL);
    }
}

pub(crate) fn wide_slice_to_string(value: &[u16]) -> String {
    let len = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

pub(crate) fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub(crate) fn send_terminal_zoom_shortcut(zoom_in: bool) -> std::io::Result<()> {
    let key = if zoom_in {
        VK_OEM_PLUS as WORD
    } else {
        VK_OEM_MINUS as WORD
    };
    let inputs = if control_key_is_down() {
        vec![keyboard_input(key, 0), keyboard_input(key, KEYEVENTF_KEYUP)]
    } else {
        vec![
            keyboard_input(VK_CONTROL as WORD, 0),
            keyboard_input(key, 0),
            keyboard_input(key, KEYEVENTF_KEYUP),
            keyboard_input(VK_CONTROL as WORD, KEYEVENTF_KEYUP),
        ]
    };

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr().cast_mut(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn control_key_is_down() -> bool {
    unsafe { GetAsyncKeyState(VK_CONTROL) < 0 }
}

fn keyboard_input(vk: WORD, flags: u32) -> INPUT {
    let mut input = unsafe { std::mem::zeroed::<INPUT>() };
    input.type_ = INPUT_KEYBOARD;
    unsafe {
        *input.u.ki_mut() = KEYBDINPUT {
            wVk: vk,
            wScan: 0,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
    }
    input
}
