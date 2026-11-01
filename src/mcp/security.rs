//! Windows identities for the local MCP endpoint. No network discovery is used.
use std::{ffi::OsStr, io, mem, os::windows::ffi::OsStrExt, ptr};
use winapi::{
    shared::{ntdef::HANDLE, sddl::ConvertSidToStringSidW},
    um::{
        handleapi::CloseHandle,
        processthreadsapi::{GetCurrentProcess, OpenProcess, OpenProcessToken},
        securitybaseapi::{GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation},
        winbase::LocalFree,
        winnt::{
            PROCESS_QUERY_LIMITED_INFORMATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
            TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel, TokenLogonSid,
        },
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) logon_sid: String,
    pub(crate) integrity: u32,
}

fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut bytes = 0;
    // SAFETY: first call only obtains the required buffer length.
    unsafe {
        GetTokenInformation(token, class, ptr::null_mut(), 0, &mut bytes);
    }
    if bytes == 0 || bytes > 1024 * 1024 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(mem::size_of::<usize>())];
    // SAFETY: aligned buffer owns at least bytes writable bytes; token stays open.
    if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), bytes, &mut bytes) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

fn process_identity(process: HANDLE) -> io::Result<Identity> {
    let mut token = ptr::null_mut();
    // SAFETY: process is a valid handle, token points to writable storage.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);
    let groups = information(token.0, TokenLogonSid)?;
    let label = information(token.0, TokenIntegrityLevel)?;
    // SAFETY: successful token queries return the requested Windows structures,
    // whose contained SIDs remain backed by these buffers until conversion ends.
    unsafe {
        let groups = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
        if groups.GroupCount != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "logon identity unavailable",
            ));
        }
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(groups.Groups[0].Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        let label = &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "integrity unavailable",
            ));
        }
        let integrity = *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1));
        Ok(Identity {
            logon_sid: sid,
            integrity,
        })
    }
}

pub(crate) fn current_identity() -> io::Result<Identity> {
    // SAFETY: current-process pseudo-handle is always valid and is not closed.
    process_identity(unsafe { GetCurrentProcess() })
}

pub(crate) fn verify_process(pid: u32, expected: &Identity) -> io::Result<()> {
    // SAFETY: opens query-only access; ownership is transferred to the guard.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = Handle(process);
    if &process_identity(process.0)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MCP peer logon or integrity mismatch",
        ));
    }
    Ok(())
}

pub(crate) fn pipe_name(identity: &Identity) -> String {
    format!(r"\\.\pipe\winproc-tui.mcp.v1.{}", identity.logon_sid)
}

pub(crate) fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_process_matches_but_a_different_integrity_does_not() {
        let identity = current_identity().unwrap();
        verify_process(std::process::id(), &identity).unwrap();
        let mut other = identity.clone();
        other.integrity ^= 0x1000;
        assert_eq!(
            verify_process(std::process::id(), &other)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(pipe_name(&identity).starts_with(r"\\.\pipe\winproc-tui.mcp.v1.S-1-"));
    }
}
