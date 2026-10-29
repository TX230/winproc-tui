use std::{
    collections::BTreeMap,
    io::Write,
    mem::zeroed,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use winapi::{
    shared::{
        minwindef::{FALSE, FILETIME},
        ntdef::HANDLE,
    },
    um::{
        fileapi::GetFileType,
        processthreadsapi::{GetProcessTimes, OpenProcess},
        winbase::QueryFullProcessImageNameW,
        winnt::{PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

use super::open_files::{
    OwnedHandle, duplicate_process_handle, final_path_for_handle, query_system_handles,
};
use crate::model::file_users::*;

fn open_process(pid: u32, duplicate: bool) -> Result<OwnedHandle, std::io::Error> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | if duplicate { PROCESS_DUP_HANDLE } else { 0 };
    // SAFETY: Windows validates the PID; a successful handle is placed in a unique owner.
    let handle = unsafe { OpenProcess(access, FALSE, pid) };
    if handle.is_null() {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(OwnedHandle(handle))
    }
}

fn live_creation_time(handle: HANDLE) -> Result<u64> {
    // SAFETY: FILETIME is a plain pair of integers with valid zero initialization.
    let mut times: [FILETIME; 4] = unsafe { zeroed() };
    let [created, exited, kernel, user] = &mut times;
    // SAFETY: the caller holds the process handle and all four outputs are separate writable values.
    if unsafe { GetProcessTimes(handle, created, exited, kernel, user) } == 0 {
        return Err(anyhow!("Process timing unavailable"));
    }
    if exited.dwHighDateTime != 0 || exited.dwLowDateTime != 0 {
        return Err(anyhow!("Process exited"));
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn owner_from_handle(handle: HANDLE, pid: u32) -> Result<FileUserOwner> {
    let creation_time = live_creation_time(handle)?;
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    // SAFETY: the process handle is held; the initialized UTF-16 buffer and length are live outputs.
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) } == 0
        || len as usize > buffer.len()
    {
        return Err(anyhow!("Process image unavailable"));
    }
    let executable_path = String::from_utf16_lossy(&buffer[..len as usize]);
    let name = Path::new(&executable_path)
        .file_name()
        .ok_or_else(|| anyhow!("Process name unavailable"))?
        .to_string_lossy()
        .into_owned();
    Ok(FileUserOwner {
        pid,
        name,
        executable_path,
        creation_time,
    })
}

pub(crate) fn run_helper(argument: &str) -> Result<()> {
    if argument.len() > 64 * 1024 {
        return Err(anyhow!("File search request too large"));
    }
    let request: FileUsersRequest = serde_json::from_str(argument)?;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut emit = |message: FileUsersMessage| -> Result<()> {
        serde_json::to_writer(&mut output, &message)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    let result = match request {
        FileUsersRequest::CaptureAll => scan(
            FileSearchQuery {
                text: String::new(),
                mode: FileSearchMode::Path,
            },
            &mut emit,
        ),
        FileUsersRequest::Search(query) => query
            .validate()
            .map_err(|e| anyhow!(e))
            .and_then(|()| scan(query, &mut emit)),
        FileUsersRequest::Verify(owner) => (|| {
            let handle = open_process(owner.pid, false)?;
            if live_creation_time(handle.0)? != owner.creation_time {
                return Err(anyhow!("Process identity changed"));
            }
            let verified = owner_from_handle(handle.0, owner.pid)?;
            if verified != owner {
                return Err(anyhow!("Process identity changed"));
            }
            emit(FileUsersMessage::Owner(verified))?;
            Ok(FileSearchEnd::Complete)
        })(),
    };
    emit(FileUsersMessage::End(
        result.unwrap_or_else(|e| FileSearchEnd::Failed(e.to_string())),
    ))
}

fn scan(
    query: FileSearchQuery,
    emit: &mut impl FnMut(FileUsersMessage) -> Result<()>,
) -> Result<FileSearchEnd> {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let own_pid = std::process::id();
    scan_processes(
        query,
        system.processes().keys().map(|p| p.as_u32()).collect(),
        own_pid,
        emit,
    )
}

fn scan_processes(
    query: FileSearchQuery,
    pids: Vec<u32>,
    own_pid: u32,
    emit: &mut impl FnMut(FileUsersMessage) -> Result<()>,
) -> Result<FileSearchEnd> {
    let mut progress = FileSearchProgress::default();
    emit(FileUsersMessage::Progress(progress.clone()))?;
    // Hold source lifetimes before taking the handle table. A recycled PID cannot attach a new
    // process name to an old table row. Exclude this helper's temporary duplicated handles.
    let mut processes = BTreeMap::new();
    let mut denied = BTreeMap::new();
    for pid in pids.into_iter().filter(|&p| p != own_pid) {
        match open_process(pid, true) {
            Ok(handle) => {
                if let Ok(owner) = owner_from_handle(handle.0, pid) {
                    processes.insert(pid, (owner, handle));
                }
            }
            Err(error) => {
                denied.insert(pid, error.raw_os_error() == Some(5));
            }
        }
    }
    let handles = query_system_handles()?;
    let mut groups = BTreeMap::<u32, Vec<usize>>::new();
    for entry in handles.into_iter().filter(|e| e.pid != own_pid) {
        groups
            .entry(entry.pid)
            .or_default()
            .push(entry.handle_value);
    }
    progress.total_processes = groups.len();
    progress.total_handles = groups.values().map(Vec::len).sum();
    emit(FileUsersMessage::Progress(progress.clone()))?;
    let mut last_progress = Instant::now();
    let mut match_count = 0;
    let mut retained_bytes = 0;
    for (pid, handles) in groups {
        let Some((owner, source)) = processes.remove(&pid) else {
            if denied.get(&pid) == Some(&true) {
                progress.denied_processes += 1;
            } else {
                progress.unavailable_processes += 1;
            }
            progress.skipped_handles += handles.len();
            emit(FileUsersMessage::Progress(progress.clone()))?;
            continue;
        };
        if live_creation_time(source.0).ok() != Some(owner.creation_time) {
            progress.exited_processes += 1;
            progress.skipped_handles += handles.len();
            emit(FileUsersMessage::Progress(progress.clone()))?;
            continue;
        }
        let mut paths = BTreeMap::<String, usize>::new();
        let mut end = None;
        for value in handles {
            if progress.inspected_handles >= MAX_SCAN_HANDLES {
                end = Some(FileSearchEnd::HandleLimit);
                break;
            }
            progress.inspected_handles += 1;
            if let Some(handle) = duplicate_process_handle(source.0, value) {
                // SAFETY: the uniquely owned duplicate is live; GetFileType accepts a kernel handle.
                if unsafe { GetFileType(handle.0) } == 1 {
                    if let Some(path) = final_path_for_handle(handle.0) {
                        if query.matches(&path) {
                            if !paths.contains_key(&path) {
                                let bytes = path.len()
                                    + owner.name.len()
                                    + owner.executable_path.len()
                                    + 128;
                                if match_count + paths.len() >= MAX_MATCHES
                                    || retained_bytes + bytes > MAX_RESULT_BYTES
                                {
                                    end = Some(FileSearchEnd::ResultLimit);
                                    break;
                                }
                                retained_bytes += bytes;
                            }
                            *paths.entry(path).or_default() += 1;
                        }
                    } else {
                        progress.unnamed_disk_handles += 1;
                    }
                }
            } else {
                progress.unreadable_handles += 1;
            }
            if last_progress.elapsed() >= Duration::from_millis(100) {
                emit(FileUsersMessage::Progress(progress.clone()))?;
                last_progress = Instant::now();
            }
        }
        if live_creation_time(source.0).ok() == Some(owner.creation_time) {
            for (path, handle_count) in paths {
                emit(FileUsersMessage::Match(FileUserMatch {
                    path,
                    owner: owner.clone(),
                    handle_count,
                }))?;
                match_count += 1;
            }
            progress.inspected_processes += 1;
        } else {
            progress.exited_processes += 1;
        }
        emit(FileUsersMessage::Progress(progress.clone()))?;
        if let Some(end) = end {
            return Ok(end);
        }
    }
    Ok(FileSearchEnd::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn file_users_native_scan_groups_duplicate_handles_and_long_unicode_paths() {
        let unique = format!(
            "winproc-file-users-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(&unique);
        fs::create_dir(&root).unwrap();
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(fs::canonicalize(&root).unwrap());
        let directory = fixture
            .0
            .join("映像検証".repeat(45))
            .join("long".repeat(25));
        fs::create_dir_all(&directory).unwrap();
        let name = format!("{unique}.mxf");
        let first = directory.join(&name);
        let second = fixture.0.join(&name);
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let _handles = [
            File::open(&first).unwrap(),
            File::open(&first).unwrap(),
            File::open(&second).unwrap(),
        ];
        assert!(first.as_os_str().len() > 260);
        let mut matches = Vec::new();
        let mut emit = |message| {
            if let FileUsersMessage::Match(entry) = message {
                matches.push(entry);
            }
            Ok(())
        };
        let end = scan_processes(
            FileSearchQuery {
                text: name,
                mode: FileSearchMode::FileName,
            },
            vec![std::process::id()],
            u32::MAX,
            &mut emit,
        )
        .unwrap();
        assert_eq!(end, FileSearchEnd::Complete);
        assert_eq!(matches.len(), 2);
        let query = FileSearchQuery {
            text: first.to_string_lossy().into_owned(),
            mode: FileSearchMode::ExactPath,
        };
        let found = matches
            .iter()
            .find(|entry| query.matches(&entry.path))
            .unwrap();
        assert_eq!(found.handle_count, 2);
        assert_eq!(found.owner.pid, std::process::id());
        assert_eq!(
            matches
                .iter()
                .map(|entry| entry.handle_count)
                .sum::<usize>(),
            3
        );
        let held = open_process(found.owner.pid, false).unwrap();
        assert_eq!(
            live_creation_time(held.0).unwrap(),
            found.owner.creation_time
        );
    }
}
