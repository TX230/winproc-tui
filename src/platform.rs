use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FALSE, TRUE, WORD},
        ntdef::HANDLE,
        winerror::ERROR_ALREADY_EXISTS,
    },
    um::{
        consoleapi::SetConsoleCtrlHandler,
        errhandlingapi::{GetLastError, SetLastError},
        handleapi::CloseHandle,
        synchapi::CreateMutexW,
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

pub(crate) mod shell;

static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);
const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\TX230.winproc-tui.SingleInstance";
const CONSOLE_CLOSE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(4_500);
const CONSOLE_CLOSE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONTROL_NOT_HANDLED: BOOL = 0;

pub(crate) struct SingleInstanceGuard {
    handle: HANDLE,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the guard is created only for a non-null mutex handle returned by
        // `CreateMutexW` and is its sole owner, so this closes the handle exactly once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
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
        Ok(Some(SingleInstanceGuard { handle }))
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
    use super::acquire_named_mutex;

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
