use std::{
    ffi::OsStr,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle},
    },
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FALSE, TRUE, WORD},
        winerror::{ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND},
    },
    um::{
        consoleapi::SetConsoleCtrlHandler,
        errhandlingapi::{GetLastError, SetLastError},
        handleapi::CloseHandle,
        synchapi::{CreateMutexW, OpenMutexW},
        wincon::{
            CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT,
            CTRL_SHUTDOWN_EVENT,
        },
        winnt::SYNCHRONIZE,
        winuser::{
            GetAsyncKeyState, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
            VK_CONTROL, VK_OEM_MINUS, VK_OEM_PLUS,
        },
    },
};

pub(crate) mod executable_location;
pub(crate) mod shell;

static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);
const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\TX230.winproc-tui.SingleInstance";
const CONSOLE_CLOSE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(4_500);
const CONSOLE_CLOSE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONTROL_NOT_HANDLED: BOOL = 0;

pub(crate) struct SingleInstanceGuard {
    _handle: OwnedHandle,
}

pub(crate) fn single_instance_running() -> std::io::Result<bool> {
    named_mutex_exists(SINGLE_INSTANCE_MUTEX_NAME)
}

fn named_mutex_exists(name: &str) -> std::io::Result<bool> {
    let name = to_wide(name);
    // SAFETY: name is NUL terminated; no mutex ownership or wait is requested.
    let handle = unsafe { OpenMutexW(SYNCHRONIZE, FALSE, name.as_ptr()) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    // SAFETY: OpenMutexW returned an owned non-null handle; close it exactly once.
    drop(unsafe { OwnedHandle::from_raw_handle(handle.cast()) });
    Ok(true)
}

pub(crate) fn acquire_mcp_launch_guard() -> std::io::Result<Option<SingleInstanceGuard>> {
    acquire_named_mutex("Local\\TX230.winproc-tui.McpLaunch")
}

pub(crate) fn acquire_single_instance() -> std::io::Result<Option<SingleInstanceGuard>> {
    acquire_named_mutex(SINGLE_INSTANCE_MUTEX_NAME)
}

fn acquire_named_mutex(name: &str) -> std::io::Result<Option<SingleInstanceGuard>> {
    let name = to_wide(name);
    // SAFETY: this clears the calling thread's last-error slot immediately before the mutex call
    // so `ERROR_ALREADY_EXISTS` can be distinguished from a newly created mutex.
    unsafe {
        SetLastError(0);
    }
    // SAFETY: `name` is a live NUL-terminated UTF-16 buffer, the security-attributes pointer is
    // intentionally null, and the returned handle is checked before it is wrapped or closed.
    let handle = unsafe { CreateMutexW(ptr::null_mut(), FALSE, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }

    // SAFETY: this reads the calling thread's last-error slot immediately after `CreateMutexW`,
    // with no intervening Win32 call that could overwrite it.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: `handle` was validated above and has not been transferred; the duplicate case
        // does not construct a guard, so this is its only close.
        unsafe {
            CloseHandle(handle);
        }
        Ok(None)
    } else {
        // SAFETY: CreateMutexW returned an owned non-null handle, transferred once.
        Ok(Some(SingleInstanceGuard {
            _handle: unsafe { OwnedHandle::from_raw_handle(handle.cast()) },
        }))
    }
}

pub(crate) fn install_console_control_handler() -> std::io::Result<()> {
    // SAFETY: the callback uses the documented system ABI, has static lifetime, and touches only
    // thread-safe atomics plus bounded sleeping when Windows invokes it on a control thread.
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

// SAFETY contract: Windows supplies a `DWORD` control code and may invoke this system-ABI
// callback concurrently. It must not access borrowed state; it uses only process-lifetime atomics.
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

    // SAFETY: `inputs` is a live contiguous array of initialized `INPUT` values, its count fits
    // `u32`, and the element size exactly matches the Windows `INPUT` definition.
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
    // SAFETY: `VK_CONTROL` is a valid virtual-key code and the call has no pointer arguments.
    unsafe { GetAsyncKeyState(VK_CONTROL) < 0 }
}

fn keyboard_input(vk: WORD, flags: u32) -> INPUT {
    // SAFETY: all-zero is a valid initial representation for the C `INPUT` union; its active
    // variant is selected below by setting `type_` before the value is returned to Windows.
    let mut input = unsafe { std::mem::zeroed::<INPUT>() };
    input.type_ = INPUT_KEYBOARD;
    // SAFETY: `type_ == INPUT_KEYBOARD` selects the keyboard union member, and the assigned
    // `KEYBDINPUT` value is fully initialized.
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

#[cfg(test)]
mod tests {
    use super::{acquire_named_mutex, named_mutex_exists};

    #[test]
    fn probing_a_mutex_does_not_create_it_or_keep_it_alive() {
        let name = format!(
            "Local\\TX230.winproc-tui.Test.Probe.{}",
            uuid::Uuid::new_v4()
        );
        assert!(!named_mutex_exists(&name).unwrap());
        let guard = acquire_named_mutex(&name).unwrap().unwrap();
        assert!(named_mutex_exists(&name).unwrap());
        drop(guard);
        assert!(!named_mutex_exists(&name).unwrap());
    }

    #[test]
    fn named_mutex_rejects_a_duplicate_and_allows_reacquisition_after_drop() {
        let name = format!(
            "Local\\TX230.winproc-tui.Test.SingleInstance.{}",
            std::process::id()
        );
        let first = acquire_named_mutex(&name)
            .expect("first mutex acquisition should succeed")
            .expect("first mutex acquisition should own the guard");

        assert!(
            acquire_named_mutex(&name)
                .expect("duplicate mutex check should succeed")
                .is_none()
        );

        drop(first);

        assert!(
            acquire_named_mutex(&name)
                .expect("mutex should be acquirable after the first guard drops")
                .is_some()
        );
    }
}
