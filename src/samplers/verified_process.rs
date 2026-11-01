//! Held query-only process identity for independent investigation sessions.
use crate::model::ProcessIdentity;
use std::{mem::zeroed, path::Path};
use winapi::{
    shared::{minwindef::FILETIME, ntdef::HANDLE},
    um::{
        handleapi::CloseHandle,
        processthreadsapi::{GetProcessTimes, OpenProcess},
        winbase::QueryFullProcessImageNameW,
        winnt::PROCESS_QUERY_LIMITED_INFORMATION,
    },
};
pub(crate) struct VerifiedProcess {
    handle: HANDLE,
    created: u64,
}
impl Drop for VerifiedProcess {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns the successful OpenProcess handle.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
fn times(handle: HANDLE) -> Result<u64, &'static str> {
    // SAFETY: distinct initialized FILETIME outputs and a caller-owned live handle.
    let mut values: [FILETIME; 4] = unsafe { zeroed() };
    if unsafe {
        GetProcessTimes(
            handle,
            &mut values[0],
            &mut values[1],
            &mut values[2],
            &mut values[3],
        )
    } == 0
    {
        return Err("IDENTITY_UNVERIFIABLE");
    }
    if values[1].dwHighDateTime != 0 || values[1].dwLowDateTime != 0 {
        return Err("PROCESS_EXITED");
    }
    Ok((u64::from(values[0].dwHighDateTime) << 32) | u64::from(values[0].dwLowDateTime))
}
impl VerifiedProcess {
    pub(crate) fn open(identity: &ProcessIdentity) -> Result<Self, &'static str> {
        let expected = identity.start_time.ok_or("IDENTITY_UNVERIFIABLE")?;
        // SAFETY: scalar arguments; ownership is acquired immediately on success.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, identity.pid) };
        if handle.is_null() {
            return Err("ACCESS_DENIED");
        }
        let mut process = Self { handle, created: 0 };
        process.created = times(handle)?;
        if (process.created / 10_000_000).checked_sub(11_644_473_600) != Some(expected) {
            return Err("IDENTITY_CHANGED");
        }
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        // SAFETY: owned query handle and correctly sized output buffer.
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut len) } == 0 {
            return Err("IDENTITY_UNVERIFIABLE");
        }
        if !Path::new(&String::from_utf16_lossy(&path[..len as usize]))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&identity.name))
        {
            return Err("IDENTITY_CHANGED");
        }
        Ok(process)
    }
    pub(crate) fn verify(&self) -> Result<(), &'static str> {
        if times(self.handle)? == self.created {
            Ok(())
        } else {
            Err("IDENTITY_CHANGED")
        }
    }
}
