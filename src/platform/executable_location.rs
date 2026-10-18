use crate::model::ProcessIdentity;
use std::path::PathBuf;
struct ProcessHandle(winapi::shared::ntdef::HANDLE);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a successfully opened process handle.
        unsafe {
            winapi::um::handleapi::CloseHandle(self.0);
        }
    }
}
fn native_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
impl ProcessHandle {
    fn verify(&self, expected: u64) -> Result<(), String> {
        use winapi::shared::minwindef::FILETIME;
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        // SAFETY: owned query handle and four distinct initialized output slots.
        if unsafe {
            winapi::um::processthreadsapi::GetProcessTimes(
                self.0,
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        } == 0
        {
            return Err(native_error("GetProcessTimes"));
        }
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        if ticks(times[1]) != 0 {
            return Err("Process exited".into());
        }
        if (ticks(times[0]) / 10_000_000).checked_sub(11_644_473_600) != Some(expected) {
            return Err("Process identity changed".into());
        }
        Ok(())
    }
}
fn resolve(identity: &ProcessIdentity) -> Result<(ProcessHandle, PathBuf), String> {
    use std::os::windows::ffi::OsStringExt;
    let start = identity
        .start_time
        .ok_or("Process creation time unavailable; identity cannot be verified")?;
    // SAFETY: PID and query-only access flags; no pointers supplied.
    let handle = unsafe {
        winapi::um::processthreadsapi::OpenProcess(
            winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            identity.pid,
        )
    };
    if handle.is_null() {
        return Err(native_error("OpenProcess (target exited or inaccessible)"));
    }
    let process = ProcessHandle(handle);
    process.verify(start)?;
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    // SAFETY: owned handle and writable buffer with its actual capacity.
    if unsafe {
        winapi::um::winbase::QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length)
    } == 0
    {
        return Err(native_error("QueryFullProcessImageNameW"));
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&identity.name))
    {
        return Err("Process identity changed (executable name mismatch)".into());
    }
    if !std::fs::metadata(&path)
        .map_err(|e| format!("Executable path unavailable: {e}"))?
        .is_file()
    {
        return Err("Executable path is not a file".into());
    }
    process.verify(start)?;
    Ok((process, path))
}
pub(crate) fn reveal(identity: &ProcessIdentity) -> Result<String, String> {
    let (_held_process, path) = resolve(identity)?;
    // Reveal only: never invoke the executable or its default association.
    crate::platform::shell::launch(&path, crate::platform::shell::PathAction::Reveal)?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_location_resolves_self_and_rejects_unknown_or_changed_identity() {
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(std::process::id());
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let process = system.process(pid).unwrap();
        let mut identity = ProcessIdentity {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        let (_, path) = resolve(&identity).unwrap();
        assert_eq!(path, std::env::current_exe().unwrap());
        identity.start_time = None;
        assert!(
            resolve(&identity)
                .err()
                .unwrap()
                .contains("creation time unavailable")
        );
        identity.start_time = Some(process.start_time() + 1);
        assert!(
            resolve(&identity)
                .err()
                .unwrap()
                .contains("identity changed")
        );
        identity.start_time = Some(process.start_time());
        identity.name = "unrelated.exe".into();
        assert!(resolve(&identity).err().unwrap().contains("name mismatch"));
    }
    #[test]
    fn executable_location_rejects_exited_target_without_shell_launch() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "exit", "0"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let identity = ProcessIdentity {
            pid: child.id(),
            name: "cmd.exe".into(),
            start_time: Some(1),
        };
        assert!(reveal(&identity).is_err());
    }
}
