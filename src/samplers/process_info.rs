use std::{
    ffi::{OsStr, OsString},
    fs,
    path::Path,
    ptr::null_mut,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use sysinfo::{Pid, ProcessesToUpdate, System, Users};
use winapi::{
    ctypes::c_void,
    um::{
        handleapi::CloseHandle,
        processthreadsapi::OpenProcess,
        winnt::PROCESS_QUERY_LIMITED_INFORMATION,
        winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        wow64apiset::IsWow64Process,
    },
};

use crate::{
    app::ProcessLifecycle,
    model::{InfoValue, ProcessInfo, ProcessRow},
    platform::to_wide,
    samplers::process::collect_working_set_share_bytes_for_process,
};

#[derive(Debug, Clone)]
pub(crate) enum ProcessInfoRequest {
    Collect {
        identity: crate::model::ProcessIdentity,
        process: ProcessRow,
        lifecycle: ProcessLifecycle,
    },
    Stop,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfoResult {
    pub(crate) identity: crate::model::ProcessIdentity,
    pub(crate) info: ProcessInfo,
}

pub(crate) struct ProcessInfoWorker {
    request_tx: Sender<ProcessInfoRequest>,
    result_rx: Receiver<ProcessInfoResult>,
    join_handle: Option<JoinHandle<()>>,
}

impl ProcessInfoWorker {
    pub(crate) fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ProcessInfoRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessInfoResult>();
        let join_handle = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                match request {
                    ProcessInfoRequest::Collect {
                        identity,
                        process,
                        lifecycle,
                    } => {
                        let info = collect_process_info(&process, lifecycle);
                        if result_tx
                            .send(ProcessInfoResult { identity, info })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProcessInfoRequest::Stop => break,
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }

    pub(crate) fn request_info(
        &self,
        identity: crate::model::ProcessIdentity,
        process: ProcessRow,
        lifecycle: ProcessLifecycle,
    ) -> Result<()> {
        self.request_tx
            .send(ProcessInfoRequest::Collect {
                identity,
                process,
                lifecycle,
            })
            .context("process info worker is unavailable")
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<ProcessInfoResult, TryRecvError> {
        self.result_rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (
        Self,
        Receiver<ProcessInfoRequest>,
        Sender<ProcessInfoResult>,
    ) {
        let (request_tx, request_rx) = mpsc::channel::<ProcessInfoRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessInfoResult>();
        (
            Self {
                request_tx,
                result_rx,
                join_handle: None,
            },
            request_rx,
            result_tx,
        )
    }
}

impl Drop for ProcessInfoWorker {
    fn drop(&mut self) {
        let _ = self.request_tx.send(ProcessInfoRequest::Stop);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub(crate) fn collect_process_info(
    process: &ProcessRow,
    lifecycle: ProcessLifecycle,
) -> ProcessInfo {
    if matches!(lifecycle, ProcessLifecycle::Exited { .. }) {
        return exited_process_info(process);
    }

    let pid = Pid::from_u32(process.pid);
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let users = Users::new_with_refreshed_list();
    let sys_process = system.process(pid);
    let executable = sys_process
        .and_then(|process| process.exe())
        .map(|path| path.display().to_string())
        .filter(|path| !path.is_empty());
    let command_line = sys_process
        .map(|process| format_command_line(process.cmd()))
        .filter(|command| !command.is_empty());
    let ppid = sys_process
        .and_then(|process| process.parent())
        .map(|pid| pid.as_u32().to_string());
    let parent_process = sys_process
        .and_then(|process| process.parent())
        .map(|parent_pid| {
            let pid = parent_pid.as_u32();
            system
                .process(parent_pid)
                .map(|process| format!("{} / PID {}", process.name().to_string_lossy(), pid))
                .unwrap_or_else(|| format!("PID {pid}"))
        });
    let user = sys_process
        .and_then(|process| process.user_id())
        .and_then(|user_id| users.get_user_by_id(user_id))
        .map(|user| user.name().to_string())
        .or_else(|| {
            sys_process
                .and_then(|process| process.user_id())
                .map(|user_id| format!("{user_id:?}"))
        });
    let executable_value = InfoValue::from_option(executable.clone());
    let (file_modified, file_size, product_version) = executable
        .as_deref()
        .map(file_info_values)
        .unwrap_or_else(|| (InfoValue::Missing, InfoValue::Missing, InfoValue::Missing));
    let workset_bytes = format_optional_bytes(process.workset_bytes);
    let workset_private_bytes = format_optional_bytes(process.workset_private_bytes);
    let (ws_shareable_bytes, ws_shared_bytes) =
        working_set_share_info(process.pid, process.workset_bytes);

    ProcessInfo {
        name: process.name.clone(),
        pid: process.pid,
        start_time: process.start_time,
        ppid: InfoValue::from_option(ppid),
        parent_process: InfoValue::from_option(parent_process),
        arch: process_arch(process.pid),
        user: InfoValue::from_option(user),
        executable: executable_value,
        command_line: InfoValue::from_option(command_line),
        file_modified,
        file_size,
        product_version,
        workset_bytes,
        workset_private_bytes,
        ws_shareable_bytes,
        ws_shared_bytes,
    }
}

fn exited_process_info(process: &ProcessRow) -> ProcessInfo {
    ProcessInfo {
        name: process.name.clone(),
        pid: process.pid,
        start_time: process.start_time,
        ppid: InfoValue::Exited,
        parent_process: InfoValue::Exited,
        arch: InfoValue::Exited,
        user: InfoValue::Exited,
        executable: InfoValue::Exited,
        command_line: InfoValue::Exited,
        file_modified: InfoValue::Exited,
        file_size: InfoValue::Exited,
        product_version: InfoValue::Exited,
        workset_bytes: InfoValue::Exited,
        workset_private_bytes: InfoValue::Exited,
        ws_shareable_bytes: InfoValue::Exited,
        ws_shared_bytes: InfoValue::Exited,
    }
}

fn format_command_line(parts: &[OsString]) -> String {
    let Some((program, args)) = parts.split_first() else {
        return String::new();
    };
    std::iter::once(short_program_name(program))
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn short_program_name(program: &OsStr) -> String {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| program.to_string_lossy().into_owned())
}

fn process_arch(pid: u32) -> InfoValue {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return InfoValue::AccessDenied;
        }

        let mut wow64 = 0;
        let ok = IsWow64Process(handle, &mut wow64);
        CloseHandle(handle);
        if ok == 0 {
            InfoValue::Missing
        } else if wow64 != 0 {
            InfoValue::Value("x86".to_string())
        } else {
            InfoValue::Value("x64".to_string())
        }
    }
}

fn file_info_values(path: &str) -> (InfoValue, InfoValue, InfoValue) {
    let path = Path::new(path);
    if !path.exists() {
        return (
            InfoValue::FileMissing,
            InfoValue::FileMissing,
            InfoValue::FileMissing,
        );
    }

    let metadata = fs::metadata(path).ok();
    let modified = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .map(format_system_time)
        .map(InfoValue::Value)
        .unwrap_or(InfoValue::Missing);
    let size = metadata
        .map(|metadata| format_file_size(metadata.len()))
        .map(InfoValue::Value)
        .unwrap_or(InfoValue::Missing);
    let version = file_version_info(path);
    (modified, size, version.product_version)
}

#[derive(Default)]
struct FileVersionValues {
    product_version: InfoValue,
}

fn file_version_info(path: &Path) -> FileVersionValues {
    let Some(path) = path.to_str() else {
        return not_available_version();
    };
    let wide_path = to_wide(path);
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle);
        if size == 0 {
            return not_available_version();
        }

        let mut buffer = vec![0u8; size as usize];
        if GetFileVersionInfoW(
            wide_path.as_ptr(),
            0,
            size,
            buffer.as_mut_ptr() as *mut c_void,
        ) == 0
        {
            return not_available_version();
        }

        let translation = query_translation(&buffer).unwrap_or((0x0409, 0x04b0));
        FileVersionValues {
            product_version: query_version_string(&buffer, translation, "ProductVersion")
                .unwrap_or(InfoValue::NotAvailable),
        }
    }
}

fn not_available_version() -> FileVersionValues {
    FileVersionValues {
        product_version: InfoValue::NotAvailable,
    }
}

fn working_set_share_info(pid: u32, workset_bytes: Option<u64>) -> (InfoValue, InfoValue) {
    let Some(workset_bytes) = workset_bytes else {
        return (InfoValue::Missing, InfoValue::Missing);
    };
    collect_working_set_share_bytes_for_process(pid, workset_bytes)
        .map(|sample| {
            (
                InfoValue::Value(format_integer(sample.shareable_bytes)),
                InfoValue::Value(format_integer(sample.shared_bytes)),
            )
        })
        .unwrap_or((InfoValue::AccessDenied, InfoValue::AccessDenied))
}

fn format_optional_bytes(value: Option<u64>) -> InfoValue {
    value
        .map(|value| InfoValue::Value(format_integer(value)))
        .unwrap_or(InfoValue::Missing)
}

fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        let remaining = digits.len() - index;
        out.push(ch);
        if remaining > 1 && remaining % 3 == 1 {
            out.push(',');
        }
    }
    out
}

unsafe fn query_translation(buffer: &[u8]) -> Option<(u16, u16)> {
    let mut ptr = null_mut();
    let mut len = 0u32;
    let block = to_wide("\\VarFileInfo\\Translation");
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr() as *const c_void,
            block.as_ptr(),
            &mut ptr,
            &mut len,
        )
    } == 0
        || len < 4
        || ptr.is_null()
    {
        return None;
    }
    let values = unsafe { std::slice::from_raw_parts(ptr as *const u16, 2) };
    Some((values[0], values[1]))
}

unsafe fn query_version_string(
    buffer: &[u8],
    translation: (u16, u16),
    key: &str,
) -> Option<InfoValue> {
    let sub_block = format!(
        "\\StringFileInfo\\{:04x}{:04x}\\{}",
        translation.0, translation.1, key
    );
    let wide_block = to_wide(&sub_block);
    let mut ptr = null_mut();
    let mut len = 0u32;
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr() as *const c_void,
            wide_block.as_ptr(),
            &mut ptr,
            &mut len,
        )
    } == 0
        || len == 0
        || ptr.is_null()
    {
        return None;
    }
    let chars = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize) };
    let value = String::from_utf16_lossy(chars)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(InfoValue::Value(value))
}

fn format_system_time(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).ok();
    duration
        .and_then(|duration| DateTime::from_timestamp(duration.as_secs() as i64, 0))
        .map(|date| {
            date.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "--".to_string())
}

fn format_file_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_value_text_uses_failure_markers() {
        assert_eq!(InfoValue::Missing.text(), "--");
        assert_eq!(InfoValue::AccessDenied.text(), "<access denied>");
        assert_eq!(InfoValue::Exited.text(), "<exited>");
        assert_eq!(InfoValue::NotAvailable.text(), "<not available>");
        assert_eq!(InfoValue::FileMissing.text(), "<missing>");
    }

    #[test]
    fn file_size_is_human_readable() {
        assert_eq!(format_file_size(999), "999 B");
        assert_eq!(format_file_size(1_500), "1.5 KB");
        assert_eq!(format_file_size(2_500_000), "2.5 MB");
    }

    #[test]
    fn command_line_shortens_executable_path_only() {
        let command = format_command_line(&[
            OsString::from("C:\\Program Files\\App\\app.exe"),
            OsString::from("--config"),
            OsString::from("C:\\work\\config.toml"),
        ]);

        assert_eq!(command, "app.exe --config C:\\work\\config.toml");
    }
}
