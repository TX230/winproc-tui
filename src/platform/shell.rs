//! Explicit current-path shell actions. Call only on a background thread.
use std::{os::windows::ffi::OsStrExt, path::Path, ptr};
use winapi::um::{
    combaseapi::{CoInitializeEx, CoTaskMemFree, CoUninitialize},
    objbase::COINIT_APARTMENTTHREADED,
    shellapi::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
    shlobj::SHOpenFolderAndSelectItems,
    shtypes::PIDLIST_ABSOLUTE,
    winuser::SW_SHOWNORMAL,
};

// winapi does not declare this Shell32 API. The output PIDL uses the COM allocator.
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHParseDisplayName(
        name: *const u16,
        bind: *mut std::ffi::c_void,
        pidl: *mut PIDLIST_ABSOLUTE,
        attributes: u32,
        output_attributes: *mut u32,
    ) -> i32;
}

// Shell association lookup, including packaged-app ProgIDs (no executable assumption).
#[link(name = "shlwapi")]
unsafe extern "system" {
    fn AssocQueryStringW(
        flags: u32,
        kind: u32,
        association: *const u16,
        extra: *const u16,
        output: *mut u16,
        length: *mut u32,
    ) -> i32;
}
fn require_association(path: &Path) -> Result<(), String> {
    let mut extension = vec![b'.' as u16];
    if let Some(value) = path.extension() {
        extension.extend(value.encode_wide());
    }
    extension.push(0);
    let mut length = 0;
    // SAFETY: terminated extension, valid output length, null output requests the size.
    // ASSOCF_INIT_IGNOREUNKNOWN prevents the Unknown/OpenWith fallback; ASSOCSTR_PROGID
    // works for both desktop and packaged apps. This never executes a registry command.
    let result = unsafe {
        AssocQueryStringW(
            0x400,
            20,
            extension.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            &mut length,
        )
    };
    if result < 0 || length <= 1 {
        Err(format!(
            "No default app association available (AssocQueryStringW HRESULT 0x{:08X})",
            result as u32
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAction {
    Reveal,
    Open,
}
impl PathAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Reveal => "Open containing folder",
            Self::Open => "Open with default app",
        }
    }
}

fn wide_path(path: &Path) -> Result<Vec<u16>, String> {
    if !path.is_absolute() {
        return Err("An absolute filesystem path is required".into());
    }
    let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err("The path contains a NUL character".into());
    }
    wide.push(0);
    Ok(wide)
}
fn hr(operation: &str, code: i32) -> Result<(), String> {
    if code < 0 {
        Err(format!("{operation}: HRESULT 0x{:08X}", code as u32))
    } else {
        Ok(())
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balanced with the successful initialization on this worker thread.
        unsafe { CoUninitialize() }
    }
}

pub(crate) fn launch(path: &Path, action: PathAction) -> Result<(), String> {
    let wide = wide_path(path)?;
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Cannot access current path: {e}"))?;
    // SAFETY: this dedicated thread initializes its own COM apartment; no pointers supplied.
    hr("CoInitializeEx", unsafe {
        CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED)
    })?;
    let _apartment = Apartment;
    if action == PathAction::Open && !metadata.is_dir() {
        require_association(path)?;
    }
    if action == PathAction::Reveal && !metadata.is_dir() {
        let mut pidl = ptr::null_mut();
        // SAFETY: live terminated UTF-16 input and writable PIDL output; optional inputs null.
        hr("SHParseDisplayName", unsafe {
            SHParseDisplayName(
                wide.as_ptr(),
                ptr::null_mut(),
                &mut pidl,
                0,
                ptr::null_mut(),
            )
        })?;
        if pidl.is_null() {
            return Err("SHParseDisplayName returned no item".into());
        }
        // SAFETY: a valid absolute PIDL, zero children means reveal/select this item in its parent.
        let result = unsafe { SHOpenFolderAndSelectItems(pidl, 0, ptr::null(), 0) };
        // SAFETY: the PIDL is owned by this call and uses the COM task allocator.
        unsafe { CoTaskMemFree(pidl.cast()) };
        hr("SHOpenFolderAndSelectItems", result)
    } else {
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        // SAFETY: zero is valid for all optional fields; required fields are assigned below.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of_val(&info) as u32;
        info.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = wide.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        // SAFETY: initialized struct and live UTF-16 buffers. No parameters, command interpreter,
        // environment expansion, process handle request, or security-check bypass.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            Err(format!(
                "ShellExecuteExW: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_path_preserves_unicode_spaces_and_metacharacters() {
        let p = Path::new(r"C:\日本語 & space\a;$(x).txt");
        assert_eq!(
            String::from_utf16(&wide_path(p).unwrap()[..p.as_os_str().encode_wide().count()])
                .unwrap(),
            p.to_str().unwrap()
        );
        assert!(wide_path(Path::new("relative.txt")).is_err());
        assert!(wide_path(Path::new("C:\\bad\0name")).is_err());
    }
    #[test]
    fn shell_unregistered_extension_reports_association_error_without_launch() {
        // This test only queries association metadata; it never launches a handler.
        hr("CoInitializeEx", unsafe {
            CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED)
        })
        .unwrap();
        let _apartment = Apartment;
        let error =
            require_association(Path::new("C:\\fixture.issue167-unregistered-367489")).unwrap_err();
        assert!(error.contains("No default app association available"));
        assert!(error.contains("HRESULT"));
    }
    #[test]
    fn shell_missing_path_reports_os_error_without_launch() {
        let result = launch(
            Path::new(r"C:\nonexistent-issue167-367489\missing.txt"),
            PathAction::Open,
        );
        assert!(result.unwrap_err().contains("Cannot access current path"));
    }
}
