use std::{
    ffi::OsString,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    ptr::{null_mut, read_unaligned},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use winapi::{
    shared::{
        minwindef::{DWORD, FALSE, FILETIME, LPVOID, ULONG},
        ntdef::{HANDLE, LARGE_INTEGER},
    },
    um::{
        fileapi::{GetFileSizeEx, GetFileTime, GetFileType, GetFinalPathNameByHandleW},
        handleapi::{CloseHandle, DuplicateHandle},
        processthreadsapi::{GetCurrentProcess, OpenProcess},
        winnt::{DUPLICATE_SAME_ACCESS, PROCESS_DUP_HANDLE},
    },
};

pub(crate) use crate::model::open_files::OpenFileEntry;
use crate::model::open_files::{
    FileAttributeError, OpenFileHandle, OpenFileKind, OpenFileMetadata,
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
// SAFETY contract: this declaration matches the documented NT system ABI and the x64 signature
// used by `SystemExtendedHandleInformation`; call sites provide a writable byte buffer and a live
// return-length output, with all returned data parsed through explicit length checks.
unsafe extern "system" {
    fn NtQuerySystemInformation(
        system_information_class: ULONG,
        system_information: LPVOID,
        system_information_length: ULONG,
        return_length: *mut ULONG,
    ) -> i32;
    fn NtQueryInformationFile(
        file_handle: HANDLE,
        io_status_block: *mut IoStatusBlock,
        file_information: LPVOID,
        length: ULONG,
        information_class: ULONG,
    ) -> i32;
}

#[repr(C)]
#[derive(Default)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
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
    ProcessExited,
    IdentityChanged,
    QueryFailed(String),
}

impl OpenFilesError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::AccessDenied => "<access denied>".to_string(),
            Self::ProcessExited => "<exited>".to_string(),
            Self::IdentityChanged => "<process changed>".to_string(),
            Self::QueryFailed(message) => format!("query failed: {message}"),
        }
    }
}

impl OpenFilesReport {
    pub(crate) fn unavailable(process: &ProcessRow, error: OpenFilesError) -> Self {
        Self {
            pid: process.pid,
            process_name: process.name.clone(),
            total_handles: 0,
            file_handles: 0,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: Vec::new(),
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFilesResult {
    pub(crate) generation: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) report: OpenFilesReport,
    pub(crate) elapsed: Duration,
}

#[derive(Debug, Clone)]
pub(crate) enum OpenFilesRequest {
    Collect {
        generation: u64,
        identity: ProcessIdentity,
        process: Box<ProcessRow>,
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
                    OpenFilesRequest::Collect {
                        generation,
                        identity,
                        process,
                    } => {
                        let started = Instant::now();
                        let report = match verify_process_identity(&identity) {
                            Ok(()) => {
                                let report = collect_open_files_for_process(&process);
                                if report.error.is_none() {
                                    match verify_process_identity(&identity) {
                                        Ok(()) => report,
                                        Err(error) => OpenFilesReport::unavailable(&process, error),
                                    }
                                } else {
                                    report
                                }
                            }
                            Err(error) => OpenFilesReport::unavailable(&process, error),
                        };
                        if result_tx
                            .send(OpenFilesResult {
                                generation,
                                identity,
                                report,
                                elapsed: started.elapsed(),
                            })
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
        generation: u64,
        identity: ProcessIdentity,
        process: ProcessRow,
    ) -> Result<()> {
        self.request_tx
            .send(OpenFilesRequest::Collect {
                generation,
                identity,
                process: Box::new(process),
            })
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

fn verify_process_identity(identity: &ProcessIdentity) -> std::result::Result<(), OpenFilesError> {
    // A fresh target-only refresh still supplies name and creation time without collecting
    // CPU, memory, environment, or executable metadata for every process on the host.
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(identity.pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let Some(process) = system.process(Pid::from_u32(identity.pid)) else {
        return Err(OpenFilesError::ProcessExited);
    };
    if !process
        .name()
        .to_string_lossy()
        .eq_ignore_ascii_case(&identity.name)
        || identity
            .start_time
            .is_some_and(|start_time| process.start_time() != start_time)
    {
        return Err(OpenFilesError::IdentityChanged);
    }
    Ok(())
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

    // SAFETY: no pointers are passed; the returned process handle is checked for null before it is
    // placed in a unique owner and used only for handle duplication.
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
                // SAFETY: `handle` uniquely owns a live duplicated handle for the duration of this
                // call; `GetFileType` accepts any valid kernel handle and has no pointer outputs.
                let file_type = unsafe { GetFileType(handle.0) };
                if file_type != FILE_TYPE_DISK {
                    continue;
                }
                file_handles += 1;
                if let Some(path) = final_path_for_handle(handle.0) {
                    let kind = query_file_kind(handle.0);
                    let metadata = query_file_metadata(handle.0, &kind);
                    paths.push((
                        path,
                        OpenFileHandle {
                            metadata,
                            kind,
                            value: entry.handle_value,
                            access: query_file_attribute(handle.0, 8),
                            mode: query_file_attribute(handle.0, 16),
                        },
                    ));
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
        entries: sort_file_handles(paths),
        error: None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct HandleEntry {
    pub(super) pid: u32,
    pub(super) handle_value: usize,
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
    Ok(query_system_handles()?
        .into_iter()
        .filter(|entry| entry.pid == pid)
        .collect())
}

pub(super) fn query_system_handles() -> Result<Vec<HandleEntry>> {
    let mut buffer_len = INITIAL_HANDLE_BUFFER_BYTES;
    loop {
        let mut buffer = vec![0u8; buffer_len];
        let mut return_len: ULONG = 0;
        // SAFETY: `buffer_len` never exceeds the configured 256 MiB limit and therefore fits
        // `ULONG`; the byte allocation and return-length output remain live and writable for the
        // call. Returned records are consumed only after the status and lengths are checked.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buffer.as_mut_ptr() as LPVOID,
                buffer.len() as ULONG,
                &mut return_len,
            )
        };

        if nt_success(status) {
            return parse_handle_buffer(&buffer);
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

fn parse_handle_buffer(buffer: &[u8]) -> Result<Vec<HandleEntry>> {
    let pointer_size = size_of::<usize>();
    if buffer.len() < pointer_size * 2 {
        return Err(anyhow!("short handle table header"));
    }

    // SAFETY: the header-length check above proves that one possibly unaligned `usize` is fully
    // contained in the initialized byte slice.
    let handle_count = unsafe { read_unaligned(buffer.as_ptr() as *const usize) };
    let entry_offset = pointer_size * 2;
    let entry_size = size_of::<SystemHandleTableEntryInfoEx>();
    let required_len = entry_offset.saturating_add(handle_count.saturating_mul(entry_size));
    if required_len > buffer.len() {
        return Err(anyhow!("short handle table body"));
    }

    let mut entries = Vec::new();
    for index in 0..handle_count {
        let offset = entry_offset + index * entry_size;
        // SAFETY: `required_len <= buffer.len()` proves every indexed, possibly unaligned entry is
        // fully contained in the initialized byte slice.
        let entry = unsafe {
            read_unaligned(buffer.as_ptr().add(offset) as *const SystemHandleTableEntryInfoEx)
        };
        if let Ok(pid) = u32::try_from(entry.unique_process_id) {
            entries.push(HandleEntry {
                pid,
                handle_value: entry.handle_value,
            });
        }
    }
    Ok(entries)
}

pub(super) fn duplicate_process_handle(
    source_process: HANDLE,
    handle_value: usize,
) -> Option<OwnedHandle> {
    let mut duplicated: HANDLE = null_mut();
    // SAFETY: `source_process` is a live process handle with duplication access, the remote handle
    // value is passed opaquely for Windows to validate, the target is the current-process pseudo
    // handle, and `duplicated` is a live output checked before ownership is constructed.
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

pub(super) fn final_path_for_handle(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: `handle` is live, and `buffer` is a writable UTF-16 allocation whose length fits
    // `DWORD`; the returned length is checked before slicing.
    let len =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as DWORD, 0) };
    if len == 0 {
        return None;
    }
    let len = len as usize;
    if len >= buffer.len() {
        // The initial buffer already covers the extended Windows path limit. Do not allocate
        // an unbounded buffer from a length returned by an unsupported filesystem/filter.
        return None;
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

#[repr(C)]
#[derive(Default)]
struct FileStandardInformation {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: u32,
    delete_pending: u8,
    directory: u8,
}

fn query_file_metadata(
    handle: HANDLE,
    kind: &Result<OpenFileKind, FileAttributeError>,
) -> OpenFileMetadata {
    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut modified = created;
    // SAFETY: the caller holds the duplicated disk-file handle; both distinct FILETIME
    // outputs remain writable for this synchronous call. No pathname is reopened.
    let times = if unsafe { GetFileTime(handle, &mut created, null_mut(), &mut modified) } == 0 {
        Err(FileAttributeError::Win32(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ))
    } else {
        Ok(())
    };
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    let size = match kind {
        Ok(OpenFileKind::Directory) => Ok(None),
        Err(error) => Err(error.clone()),
        Ok(OpenFileKind::File) => {
            // SAFETY: zero is a valid initial LARGE_INTEGER; GetFileSizeEx writes its
            // signed QuadPart through a live output while the duplicate remains held.
            let mut size: LARGE_INTEGER = unsafe { std::mem::zeroed() };
            if unsafe { GetFileSizeEx(handle, &mut size) } == 0 {
                Err(FileAttributeError::Win32(
                    std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
                ))
            } else {
                // SAFETY: successful GetFileSizeEx initialized the QuadPart union member.
                u64::try_from(unsafe { *size.QuadPart() })
                    .map(Some)
                    .map_err(|_| FileAttributeError::ShortReply)
            }
        }
    };
    OpenFileMetadata {
        created: times.clone().map(|()| ticks(created)),
        modified: times.map(|()| ticks(modified)),
        size,
    }
}

fn query_file_kind(
    handle: HANDLE,
) -> Result<crate::model::open_files::OpenFileKind, FileAttributeError> {
    use crate::model::open_files::OpenFileKind;
    let mut io_status = IoStatusBlock::default();
    let mut value = FileStandardInformation::default();
    // SAFETY: the caller owns the duplicated handle, and both aligned output buffers
    // remain live throughout this FileStandardInformation query. No file data is read.
    let status = unsafe {
        NtQueryInformationFile(
            handle,
            &mut io_status,
            &mut value as *mut FileStandardInformation as LPVOID,
            size_of::<FileStandardInformation>() as ULONG,
            5,
        )
    };
    if status != 0 {
        return Err(FileAttributeError::NtStatus(status));
    }
    if io_status.information != size_of::<FileStandardInformation>() {
        return Err(FileAttributeError::ShortReply);
    }
    Ok(if value.directory != 0 {
        OpenFileKind::Directory
    } else {
        OpenFileKind::File
    })
}

fn query_file_attribute(
    handle: HANDLE,
    information_class: ULONG,
) -> Result<u32, FileAttributeError> {
    let mut io_status = IoStatusBlock::default();
    let mut value = 0u32;
    // SAFETY: the duplicated disk-file handle remains owned by the caller. These synchronous
    // FileAccessInformation/FileModeInformation queries each return one ULONG, and both aligned
    // output buffers remain live for the call. No file data or current position is changed.
    let status = unsafe {
        NtQueryInformationFile(
            handle,
            &mut io_status,
            &mut value as *mut u32 as LPVOID,
            size_of::<u32>() as ULONG,
            information_class,
        )
    };
    if status != 0 {
        return Err(FileAttributeError::NtStatus(status));
    }
    if io_status.information != size_of::<u32>() {
        return Err(FileAttributeError::ShortReply);
    }
    Ok(value)
}

fn sort_file_handles(paths: Vec<(String, OpenFileHandle)>) -> Vec<OpenFileEntry> {
    let mut entries = paths
        .into_iter()
        .map(|(path, handle)| OpenFileEntry { path, handle })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.handle.value.cmp(&b.handle.value))
    });
    entries
}

pub(super) struct OwnedHandle(pub(super) HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper is constructed only for a checked non-null process or duplicated
        // handle and uniquely owns it until this single close.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_only_file_identity_check_rejects_another_name_or_lifetime() {
        let system = System::new_all();
        let pid = std::process::id();
        let process = system.process(Pid::from_u32(pid)).unwrap();
        let mut identity = ProcessIdentity {
            pid,
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        assert_eq!(verify_process_identity(&identity), Ok(()));
        identity.start_time = Some(process.start_time() + 1);
        assert_eq!(
            verify_process_identity(&identity),
            Err(OpenFilesError::IdentityChanged)
        );
        identity.start_time = Some(process.start_time());
        identity.name.push_str("-another-process");
        assert_eq!(
            verify_process_identity(&identity),
            Err(OpenFilesError::IdentityChanged)
        );
        identity.pid = u32::MAX;
        assert_eq!(
            verify_process_identity(&identity),
            Err(OpenFilesError::ProcessExited)
        );
    }

    #[test]
    #[ignore = "opt-in native Files timing probe; optional WINPROC_FILES_TIMING_PIDS comma-separated PIDs"]
    fn open_files_collection_timings() {
        use std::time::{Duration, Instant};
        let system = System::new_all();
        let mut pids = vec![std::process::id()];
        if let Ok(extra) = std::env::var("WINPROC_FILES_TIMING_PIDS") {
            pids.extend(extra.split(',').map(|pid| pid.parse::<u32>().unwrap()));
        }
        let worker = OpenFilesWorker::spawn();
        for pid in pids {
            let source = system.process(Pid::from_u32(pid)).expect("live probe PID");
            let process = ProcessRow {
                pid,
                name: source.name().to_string_lossy().into_owned(),
                start_time: Some(source.start_time()),
                ..Default::default()
            };
            let identity = ProcessIdentity::from_row(&process);
            for generation in 1..=3 {
                let started = Instant::now();
                worker
                    .request_open_files(generation, identity.clone(), process.clone())
                    .unwrap();
                let result = worker
                    .result_rx
                    .recv_timeout(Duration::from_secs(30))
                    .unwrap();
                println!(
                    "PID {pid}, request {generation}: {} ms, {} handles, {} named, error {:?}",
                    started.elapsed().as_millis(),
                    result.report.total_handles,
                    result.report.entries.len(),
                    result.report.error
                );
                assert!(result.report.error.is_none());
            }
        }
    }

    #[test]
    fn file_attribute_queries_preserve_native_failure_codes() {
        let metadata = query_file_metadata(null_mut(), &Ok(OpenFileKind::File));
        assert!(matches!(
            metadata.created,
            Err(FileAttributeError::Win32(_))
        ));
        assert!(matches!(
            metadata.modified,
            Err(FileAttributeError::Win32(_))
        ));
        assert!(matches!(metadata.size, Err(FileAttributeError::Win32(_))));
        assert_eq!(
            query_file_kind(null_mut()),
            Err(FileAttributeError::NtStatus(0xc0000008u32 as i32))
        );
        for class in [8, 16] {
            assert_eq!(
                query_file_attribute(null_mut(), class),
                Err(FileAttributeError::NtStatus(0xc0000008u32 as i32))
            );
        }
    }

    #[test]
    fn file_metadata_uses_held_object_and_refreshes_size_and_times() {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use winapi::um::{fileapi::SetFileTime, winbase::FILE_FLAG_BACKUP_SEMANTICS};
        let root = std::env::temp_dir().join(format!(
            "winproc-metadata-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("original.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(12345).unwrap();
        let created = FILETIME {
            dwLowDateTime: 0x12345678,
            dwHighDateTime: 0x01db0000,
        };
        let modified = FILETIME {
            dwLowDateTime: 0x23456789,
            dwHighDateTime: 0x01dc0000,
        };
        // SAFETY: file owns a valid handle, and both timestamp pointers remain live.
        assert_ne!(
            unsafe {
                SetFileTime(
                    file.as_raw_handle() as HANDLE,
                    &created,
                    null_mut(),
                    &modified,
                )
            },
            0
        );
        // SAFETY: GetCurrentProcess returns the current-process pseudo handle for duplication.
        let duplicate = duplicate_process_handle(
            unsafe { GetCurrentProcess() },
            file.as_raw_handle() as usize,
        )
        .unwrap();
        std::fs::rename(&path, root.join("renamed.bin")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        let metadata = query_file_metadata(duplicate.0, &query_file_kind(duplicate.0));
        assert_eq!(metadata.size, Ok(Some(12345)));
        assert_eq!(
            metadata.created,
            Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        );
        assert_eq!(
            metadata.modified,
            Ok((u64::from(modified.dwHighDateTime) << 32) | u64::from(modified.dwLowDateTime))
        );
        file.set_len(54321).unwrap();
        assert_eq!(
            query_file_metadata(duplicate.0, &Ok(OpenFileKind::File)).size,
            Ok(Some(54321))
        );
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&root)
            .unwrap();
        let directory_metadata = query_file_metadata(
            directory.as_raw_handle() as HANDLE,
            &Ok(OpenFileKind::Directory),
        );
        assert_eq!(directory_metadata.size, Ok(None));
        assert!(directory_metadata.created.is_ok());
        drop(directory);
        drop(duplicate);
        drop(file);
        std::fs::remove_dir_all(root).unwrap();
    }

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
    fn file_handles_sort_by_path_and_handle_without_collapsing_duplicates() {
        let entries = sort_file_handles(
            vec![r"C:\b.log", r"C:\a.log", r"C:\b.log"]
                .into_iter()
                .enumerate()
                .map(|(value, path)| {
                    (
                        path.to_string(),
                        OpenFileHandle {
                            metadata: Default::default(),
                            kind: Ok(crate::model::open_files::OpenFileKind::File),
                            value,
                            access: Ok(1),
                            mode: Ok(0),
                        },
                    )
                })
                .collect(),
        );

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, r"C:\a.log");
        assert_eq!(entries[0].handle.value, 1);
        assert_eq!(entries[1].path, r"C:\b.log");
        assert_eq!(entries[1].handle.value, 0);
        assert_eq!(entries[2].path, r"C:\b.log");
        assert_eq!(entries[2].handle.value, 2);
    }
}
