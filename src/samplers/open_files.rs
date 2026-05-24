use std::{
    collections::BTreeMap,
    ffi::OsString,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    ptr::{null_mut, read_unaligned},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow};
use winapi::{
    shared::{
        minwindef::{DWORD, FALSE, LPVOID, ULONG},
        ntdef::HANDLE,
    },
    um::{
        fileapi::{GetFileType, GetFinalPathNameByHandleW},
        handleapi::{CloseHandle, DuplicateHandle},
        processthreadsapi::{GetCurrentProcess, OpenProcess},
        winnt::{DUPLICATE_SAME_ACCESS, PROCESS_DUP_HANDLE},
    },
};

use crate::model::{ProcessIdentity, ProcessRow};

const SYSTEM_EXTENDED_HANDLE_INFORMATION: ULONG = 64;
const FILE_TYPE_DISK: DWORD = 0x0001;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;
const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005u32 as i32;
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
const INITIAL_HANDLE_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_HANDLE_BUFFER_BYTES: usize = 256 * 1024 * 1024;

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(
        system_information_class: ULONG,
        system_information: LPVOID,
        system_information_length: ULONG,
        return_length: *mut ULONG,
    ) -> i32;
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFileEntry {
    pub(crate) path: String,
    pub(crate) handle_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFilesReport {
    pub(crate) pid: u32,
    pub(crate) process_name: String,
    pub(crate) total_handles: usize,
    pub(crate) file_handles: usize,
    pub(crate) inaccessible_handles: usize,
    pub(crate) unnamed_file_handles: usize,
    pub(crate) entries: Vec<OpenFileEntry>,
    pub(crate) error: Option<OpenFilesError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenFilesError {
    AccessDenied,
    QueryFailed(String),
}

impl OpenFilesError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::AccessDenied => "<access denied>".to_string(),
            Self::QueryFailed(message) => format!("query failed: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFilesResult {
    pub(crate) identity: ProcessIdentity,
    pub(crate) report: OpenFilesReport,
}

#[derive(Debug, Clone)]
pub(crate) enum OpenFilesRequest {
    Collect {
        identity: ProcessIdentity,
        process: ProcessRow,
    },
    Stop,
}

pub(crate) struct OpenFilesWorker {
    request_tx: Sender<OpenFilesRequest>,
    result_rx: Receiver<OpenFilesResult>,
    join_handle: Option<JoinHandle<()>>,
}

impl OpenFilesWorker {
    pub(crate) fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<OpenFilesRequest>();
        let (result_tx, result_rx) = mpsc::channel::<OpenFilesResult>();
        let join_handle = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                match request {
                    OpenFilesRequest::Collect { identity, process } => {
                        let report = collect_open_files_for_process(&process);
                        if result_tx
                            .send(OpenFilesResult { identity, report })
                            .is_err()
                        {
                            break;
                        }
                    }
                    OpenFilesRequest::Stop => break,
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }

    pub(crate) fn request_open_files(
        &self,
        identity: ProcessIdentity,
        process: ProcessRow,
    ) -> Result<()> {
        self.request_tx
            .send(OpenFilesRequest::Collect { identity, process })
            .context("open files worker is unavailable")
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<OpenFilesResult, TryRecvError> {
        self.result_rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, Receiver<OpenFilesRequest>, Sender<OpenFilesResult>) {
        let (request_tx, request_rx) = mpsc::channel::<OpenFilesRequest>();
        let (result_tx, result_rx) = mpsc::channel::<OpenFilesResult>();
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

impl Drop for OpenFilesWorker {
    fn drop(&mut self) {
        let _ = self.request_tx.send(OpenFilesRequest::Stop);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub(crate) fn collect_open_files_for_process(process: &ProcessRow) -> OpenFilesReport {
    let handle_entries = match query_system_handles_for_pid(process.pid) {
        Ok(entries) => entries,
        Err(error) => {
            return OpenFilesReport {
                pid: process.pid,
                process_name: process.name.clone(),
                total_handles: 0,
                file_handles: 0,
                inaccessible_handles: 0,
                unnamed_file_handles: 0,
                entries: Vec::new(),
                error: Some(OpenFilesError::QueryFailed(error.to_string())),
            };
        }
    };
    let total_handles = handle_entries.len();

    let process_handle = unsafe { OpenProcess(PROCESS_DUP_HANDLE, FALSE, process.pid) };
    if process_handle.is_null() {
        return OpenFilesReport {
            pid: process.pid,
            process_name: process.name.clone(),
            total_handles,
            file_handles: 0,
            inaccessible_handles: total_handles,
            unnamed_file_handles: 0,
            entries: Vec::new(),
            error: Some(OpenFilesError::AccessDenied),
        };
    }
    let source_process = OwnedHandle(process_handle);

    let mut paths = Vec::new();
    let mut inaccessible_handles = 0;
    let mut file_handles = 0;
    let mut unnamed_file_handles = 0;

    for entry in handle_entries {
        match duplicate_process_handle(source_process.0, entry.handle_value) {
            Some(handle) => {
                let file_type = unsafe { GetFileType(handle.0) };
                if file_type != FILE_TYPE_DISK {
                    continue;
                }
                file_handles += 1;
                if let Some(path) = final_path_for_handle(handle.0) {
                    paths.push(path);
                } else {
                    unnamed_file_handles += 1;
                }
            }
            None => inaccessible_handles += 1,
        }
    }

    OpenFilesReport {
        pid: process.pid,
        process_name: process.name.clone(),
        total_handles,
        file_handles,
        inaccessible_handles,
        unnamed_file_handles,
        entries: aggregate_paths(paths),
        error: None,
    }
}

#[derive(Debug, Clone, Copy)]
struct HandleEntry {
    handle_value: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SystemHandleTableEntryInfoEx {
    object: usize,
    unique_process_id: usize,
    handle_value: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    handle_attributes: u32,
    reserved: u32,
}

fn query_system_handles_for_pid(pid: u32) -> Result<Vec<HandleEntry>> {
    let mut buffer_len = INITIAL_HANDLE_BUFFER_BYTES;
    loop {
        let mut buffer = vec![0u8; buffer_len];
        let mut return_len: ULONG = 0;
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buffer.as_mut_ptr() as LPVOID,
                buffer.len() as ULONG,
                &mut return_len,
            )
        };

        if nt_success(status) {
            return parse_handle_buffer(&buffer, pid);
        }

        if matches!(
            status,
            STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL
        ) {
            let requested = return_len as usize;
            let next_len = requested
                .max(buffer_len.saturating_mul(2))
                .min(MAX_HANDLE_BUFFER_BYTES);
            if next_len <= buffer_len {
                return Err(anyhow!("handle table is too large"));
            }
            buffer_len = next_len;
            continue;
        }

        return Err(anyhow!("NtQuerySystemInformation returned 0x{status:08X}"));
    }
}

fn nt_success(status: i32) -> bool {
    status >= 0
}

fn parse_handle_buffer(buffer: &[u8], pid: u32) -> Result<Vec<HandleEntry>> {
    let pointer_size = size_of::<usize>();
    if buffer.len() < pointer_size * 2 {
        return Err(anyhow!("short handle table header"));
    }

    let handle_count = unsafe { read_unaligned(buffer.as_ptr() as *const usize) };
    let entry_offset = pointer_size * 2;
    let entry_size = size_of::<SystemHandleTableEntryInfoEx>();
    let required_len = entry_offset.saturating_add(handle_count.saturating_mul(entry_size));
    if required_len > buffer.len() {
        return Err(anyhow!("short handle table body"));
    }

    let target_pid = pid as usize;
    let mut entries = Vec::new();
    for index in 0..handle_count {
        let offset = entry_offset + index * entry_size;
        let entry = unsafe {
            read_unaligned(buffer.as_ptr().add(offset) as *const SystemHandleTableEntryInfoEx)
        };
        if entry.unique_process_id == target_pid {
            entries.push(HandleEntry {
                handle_value: entry.handle_value,
            });
        }
    }
    Ok(entries)
}

fn duplicate_process_handle(source_process: HANDLE, handle_value: usize) -> Option<OwnedHandle> {
    let mut duplicated: HANDLE = null_mut();
    let ok = unsafe {
        DuplicateHandle(
            source_process,
            handle_value as HANDLE,
            GetCurrentProcess(),
            &mut duplicated,
            0,
            FALSE,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 || duplicated.is_null() {
        None
    } else {
        Some(OwnedHandle(duplicated))
    }
}

fn final_path_for_handle(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; 32_768];
    let len =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as DWORD, 0) };
    if len == 0 {
        return None;
    }
    let len = len as usize;
    if len >= buffer.len() {
        buffer.resize(len + 1, 0);
        let len = unsafe {
            GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as DWORD, 0)
        };
        if len == 0 || len as usize >= buffer.len() {
            return None;
        }
        return Some(normalize_final_path(&buffer[..len as usize]));
    }
    Some(normalize_final_path(&buffer[..len]))
}

fn normalize_final_path(wide: &[u16]) -> String {
    let value = OsString::from_wide(wide).to_string_lossy().into_owned();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value
    }
}

fn aggregate_paths(paths: Vec<String>) -> Vec<OpenFileEntry> {
    let mut counts = BTreeMap::<String, usize>::new();
    for path in paths {
        *counts.entry(path).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(path, handle_count)| OpenFileEntry { path, handle_count })
        .collect()
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_final_path_removes_extended_prefix() {
        let wide = r"\\?\C:\tmp\app.log".encode_utf16().collect::<Vec<_>>();

        assert_eq!(normalize_final_path(&wide), r"C:\tmp\app.log");
    }

    #[test]
    fn normalize_final_path_converts_unc_prefix() {
        let wide = r"\\?\UNC\server\share\app.log"
            .encode_utf16()
            .collect::<Vec<_>>();

        assert_eq!(normalize_final_path(&wide), r"\\server\share\app.log");
    }

    #[test]
    fn aggregate_paths_sorts_and_counts_paths() {
        let entries = aggregate_paths(vec![
            r"C:\b.log".to_string(),
            r"C:\a.log".to_string(),
            r"C:\b.log".to_string(),
        ]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, r"C:\a.log");
        assert_eq!(entries[0].handle_count, 1);
        assert_eq!(entries[1].path, r"C:\b.log");
        assert_eq!(entries[1].handle_count, 2);
    }
}
