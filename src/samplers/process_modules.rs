use std::{
    collections::HashSet,
    ffi::OsString,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    path::Path,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use chrono::Local;
use sysinfo::{Pid, System};
use winapi::{
    shared::winerror::{
        ERROR_ACCESS_DENIED, ERROR_BAD_LENGTH, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    },
    um::{
        errhandlingapi::GetLastError,
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        tlhelp32::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        },
        winnt::HANDLE,
    },
};

use crate::{
    model::{
        ProcessIdentity, ProcessModuleEntry, ProcessModulesError, ProcessModulesReport, ProcessRow,
    },
    samplers::process_info::file_metadata_values,
};

const SNAPSHOT_BAD_LENGTH_RETRIES: usize = 3;

#[derive(Debug, Clone)]
pub(crate) enum ProcessModulesRequest {
    Collect {
        generation: u64,
        request_id: u64,
        identity: ProcessIdentity,
        process: ProcessRow,
    },
    Stop,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessModulesResult {
    pub(crate) generation: u64,
    pub(crate) request_id: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) outcome: std::result::Result<ProcessModulesReport, ProcessModulesError>,
}

pub(crate) struct ProcessModulesWorker {
    request_tx: Sender<ProcessModulesRequest>,
    result_rx: Receiver<ProcessModulesResult>,
    join_handle: Option<JoinHandle<()>>,
}

impl ProcessModulesWorker {
    pub(crate) fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ProcessModulesRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessModulesResult>();
        let join_handle = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                match request {
                    ProcessModulesRequest::Collect {
                        generation,
                        request_id,
                        identity,
                        process,
                    } => {
                        let outcome = collect_process_modules(&identity, &process);
                        if result_tx
                            .send(ProcessModulesResult {
                                generation,
                                request_id,
                                identity,
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    ProcessModulesRequest::Stop => break,
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }

    pub(crate) fn request_modules(
        &self,
        generation: u64,
        request_id: u64,
        identity: ProcessIdentity,
        process: ProcessRow,
    ) -> Result<()> {
        self.request_tx
            .send(ProcessModulesRequest::Collect {
                generation,
                request_id,
                identity,
                process,
            })
            .context("process modules worker is unavailable")
    }

    pub(crate) fn try_recv(&self) -> std::result::Result<ProcessModulesResult, TryRecvError> {
        self.result_rx.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (
        Self,
        Receiver<ProcessModulesRequest>,
        Sender<ProcessModulesResult>,
    ) {
        let (request_tx, request_rx) = mpsc::channel::<ProcessModulesRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessModulesResult>();
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

    #[cfg(test)]
    pub(crate) fn test_noop() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ProcessModulesRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ProcessModulesResult>();
        let join_handle = thread::spawn(move || {
            let _keep_result_channel_open = result_tx;
            while let Ok(request) = request_rx.recv() {
                if matches!(request, ProcessModulesRequest::Stop) {
                    break;
                }
            }
        });
        Self {
            request_tx,
            result_rx,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for ProcessModulesWorker {
    fn drop(&mut self) {
        let _ = self.request_tx.send(ProcessModulesRequest::Stop);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub(crate) fn collect_process_modules(
    identity: &ProcessIdentity,
    process: &ProcessRow,
) -> std::result::Result<ProcessModulesReport, ProcessModulesError> {
    verify_process_identity(identity)?;
    let native = enumerate_module_paths(process.pid, TH32CS_SNAPMODULE)?;
    let wow64 = enumerate_module_paths(process.pid, TH32CS_SNAPMODULE32)?;
    let entries = module_entries_from_paths(native.into_iter().chain(wow64), process);
    verify_process_identity(identity)?;
    Ok(ProcessModulesReport {
        pid: process.pid,
        process_name: process.name.clone(),
        captured_at: Local::now(),
        entries,
    })
}

fn verify_process_identity(
    identity: &ProcessIdentity,
) -> std::result::Result<(), ProcessModulesError> {
    let system = System::new_all();
    let Some(process) = system.process(Pid::from_u32(identity.pid)) else {
        return Err(ProcessModulesError::ProcessExited);
    };
    if !process
        .name()
        .to_string_lossy()
        .eq_ignore_ascii_case(&identity.name)
        || identity
            .start_time
            .is_some_and(|start_time| process.start_time() != start_time)
    {
        return Err(ProcessModulesError::IdentityChanged);
    }
    Ok(())
}

fn enumerate_module_paths(
    pid: u32,
    flags: u32,
) -> std::result::Result<Vec<String>, ProcessModulesError> {
    let snapshot =
        retry_bad_length(|| create_module_snapshot(pid, flags)).map_err(module_error_from_win32)?;
    unsafe {
        let mut entry: MODULEENTRY32W = zeroed();
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(snapshot.0, &mut entry) == 0 {
            let error = GetLastError();
            return if error == ERROR_NO_MORE_FILES {
                Ok(Vec::new())
            } else {
                Err(module_error_from_win32(error))
            };
        }

        let mut paths = Vec::new();
        loop {
            let path = wide_array_to_string(&entry.szExePath);
            if !path.is_empty() {
                paths.push(path);
            }
            if Module32NextW(snapshot.0, &mut entry) == 0 {
                let error = GetLastError();
                if error != ERROR_NO_MORE_FILES {
                    return Err(module_error_from_win32(error));
                }
                break;
            }
        }
        Ok(paths)
    }
}

fn create_module_snapshot(pid: u32, flags: u32) -> std::result::Result<OwnedHandle, u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(flags, pid) };
    if snapshot == INVALID_HANDLE_VALUE {
        Err(unsafe { GetLastError() })
    } else {
        Ok(OwnedHandle(snapshot))
    }
}

fn retry_bad_length<T>(
    mut operation: impl FnMut() -> std::result::Result<T, u32>,
) -> std::result::Result<T, u32> {
    let mut retries = 0usize;
    loop {
        match operation() {
            Err(error) if error == ERROR_BAD_LENGTH && retries < SNAPSHOT_BAD_LENGTH_RETRIES => {
                retries += 1;
            }
            result => return result,
        }
    }
}

fn module_error_from_win32(error: u32) -> ProcessModulesError {
    match error {
        ERROR_ACCESS_DENIED => ProcessModulesError::AccessDenied,
        ERROR_INVALID_PARAMETER => ProcessModulesError::ProcessExited,
        _ => ProcessModulesError::QueryFailed,
    }
}

fn module_entries_from_paths(
    paths: impl IntoIterator<Item = String>,
    process: &ProcessRow,
) -> Vec<ProcessModuleEntry> {
    normalize_dll_paths(paths, process.executable_path.as_deref())
        .into_iter()
        .map(|path| {
            let (directory, dll_name) = split_path(&path);
            let directory = directory.to_string();
            let dll_name = dll_name.to_string();
            let metadata = file_metadata_values(Path::new(&path));
            ProcessModuleEntry {
                path,
                dll_name,
                directory,
                company_name: metadata.company_name,
                product_version: metadata.product_version,
                file_version: metadata.file_version,
                modified: metadata.file_modified,
            }
        })
        .collect()
}

fn normalize_dll_paths(
    paths: impl IntoIterator<Item = String>,
    executable_path: Option<&str>,
) -> Vec<String> {
    let executable_path = executable_path.map(str::to_lowercase);
    let mut seen = HashSet::new();
    let mut paths = paths
        .into_iter()
        .filter(|path| {
            let normalized = path.to_lowercase();
            normalized.ends_with(".dll")
                && executable_path.as_deref() != Some(normalized.as_str())
                && seen.insert(normalized)
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        let (left_dir, left_name) = split_path(left);
        let (right_dir, right_name) = split_path(right);
        left_name
            .to_lowercase()
            .cmp(&right_name.to_lowercase())
            .then_with(|| left_dir.to_lowercase().cmp(&right_dir.to_lowercase()))
            .then_with(|| left.cmp(right))
    });
    paths
}

fn split_path(path: &str) -> (&str, &str) {
    path.rfind(['\\', '/'])
        .map(|index| (&path[..index], &path[index + 1..]))
        .unwrap_or(("", path))
}

fn wide_array_to_string(value: &[u16]) -> String {
    let len = value
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(value.len());
    OsString::from_wide(&value[..len])
        .to_string_lossy()
        .into_owned()
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

    fn process_row(
        pid: u32,
        name: String,
        executable_path: Option<String>,
        start_time: u64,
    ) -> ProcessRow {
        ProcessRow {
            pid,
            name,
            executable_path,
            start_time: Some(start_time),
            cpu_percent: None,
            private_bytes: None,
            workset_bytes: None,
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            workset_shared_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }

    #[test]
    fn module_paths_merge_native_and_wow64_deduplicate_and_sort() {
        let paths = normalize_dll_paths(
            [
                r"C:\app\app.exe".to_string(),
                r"C:\Windows\System32\z.dll".to_string(),
                r"C:\Windows\SysWOW64\A.DLL".to_string(),
                r"c:\windows\syswow64\a.dll".to_string(),
                r"C:\app\plugin.txt".to_string(),
            ],
            Some(r"C:\app\app.exe"),
        );

        assert_eq!(
            paths,
            [
                r"C:\Windows\SysWOW64\A.DLL".to_string(),
                r"C:\Windows\System32\z.dll".to_string(),
            ]
        );
    }

    #[test]
    fn bad_length_snapshot_is_retried_at_most_three_times() {
        let mut attempts = 0;
        let result = retry_bad_length(|| {
            attempts += 1;
            Err::<(), _>(ERROR_BAD_LENGTH)
        });

        assert_eq!(result, Err(ERROR_BAD_LENGTH));
        assert_eq!(attempts, 4);
    }

    #[test]
    fn missing_dll_is_kept_with_missing_metadata() {
        let process = process_row(
            1,
            "app.exe".to_string(),
            Some(r"C:\app\app.exe".to_string()),
            1,
        );
        let entries = module_entries_from_paths([r"C:\missing\gone.dll".to_string()], &process);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].company_name,
            crate::model::InfoValue::FileMissing
        );
        assert_eq!(
            entries[0].file_version,
            crate::model::InfoValue::FileMissing
        );
    }

    #[test]
    fn current_process_modules_are_collectable_without_elevation() {
        let pid = std::process::id();
        let system = System::new_all();
        let current = system
            .process(Pid::from_u32(pid))
            .expect("test process should be visible");
        let name = current.name().to_string_lossy().into_owned();
        let process = process_row(
            pid,
            name.clone(),
            current.exe().map(|path| path.display().to_string()),
            current.start_time(),
        );
        let identity = ProcessIdentity {
            pid,
            name,
            start_time: Some(current.start_time()),
        };

        let report = collect_process_modules(&identity, &process)
            .expect("the current process module snapshot should be readable");
        assert!(!report.entries.is_empty());
        assert!(
            report
                .entries
                .iter()
                .all(|entry| entry.path.to_lowercase().ends_with(".dll"))
        );
    }
}
