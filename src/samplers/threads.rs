use crate::model::{
    ProcessIdentity,
    threads::{ThreadReport, ThreadRow},
};
use std::{
    mem::{size_of, zeroed},
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::{
    shared::minwindef::FILETIME,
    um::{
        errhandlingapi::GetLastError,
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        processthreadsapi::{
            GetProcessIdOfThread, GetProcessTimes, GetThreadIdealProcessorEx, GetThreadTimes,
            OpenProcess, OpenThread,
        },
        sysinfoapi::GetSystemTimeAsFileTime,
        tlhelp32::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        winnt::{
            HANDLE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESSOR_NUMBER,
            THREAD_QUERY_LIMITED_INFORMATION,
        },
    },
};
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetThreadDescription(thread: HANDLE, description: *mut *mut u16) -> i32;
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}
pub(crate) struct ThreadResult {
    pub(crate) generation: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) outcome: Result<ThreadReport, String>,
}
pub(crate) struct ThreadWorker {
    sender: Option<SyncSender<(u64, ProcessIdentity)>>,
    receiver: Receiver<ThreadResult>,
    join: Option<JoinHandle<()>>,
}
impl ThreadWorker {
    #[cfg(test)]
    pub(crate) fn test_pair() -> (
        Self,
        Receiver<(u64, ProcessIdentity)>,
        mpsc::Sender<ThreadResult>,
    ) {
        let (sender, requests) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::channel();
        (
            Self {
                sender: Some(sender),
                receiver,
                join: None,
            },
            requests,
            results,
        )
    }
    pub(crate) fn spawn() -> Self {
        let (sender, requests) = mpsc::sync_channel::<(u64, ProcessIdentity)>(1);
        let (results, receiver) = mpsc::channel();
        let join = thread::spawn(move || {
            while let Ok((generation, identity)) = requests.recv() {
                let outcome = collect(&identity);
                if results
                    .send(ThreadResult {
                        generation,
                        identity,
                        outcome,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender: Some(sender),
            receiver,
            join: Some(join),
        }
    }
    pub(crate) fn request(&self, generation: u64, identity: ProcessIdentity) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("Thread worker unavailable")?
            .try_send((generation, identity))
            .map_err(|_| "Thread capture is busy; retry refresh".into())
    }
    pub(crate) fn try_recv(&self) -> Result<ThreadResult, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}
impl Drop for ThreadWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a valid native handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn ticks(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}
fn process_start(handle: HANDLE) -> Result<u64, String> {
    // SAFETY: FILETIME has an all-zero valid representation; outputs are distinct and writable.
    let mut times: [FILETIME; 4] = unsafe { zeroed() };
    if unsafe {
        GetProcessTimes(
            handle,
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    } == 0
    {
        return Err("Process times unavailable".into());
    }
    if ticks(times[1]) != 0 {
        return Err("Process exited".into());
    }
    Ok(ticks(times[0]))
}
pub(crate) fn collect(identity: &ProcessIdentity) -> Result<ThreadReport, String> {
    let expected = identity
        .start_time
        .ok_or("Process creation time unavailable")?;
    // SAFETY: query-only access; the returned handle is checked and owned below.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, identity.pid) };
    if raw.is_null() {
        return Err("Process unavailable or access denied".into());
    }
    let process = Handle(raw);
    let start = process_start(process.0)?;
    if (start / 10_000_000).checked_sub(11_644_473_600) != Some(expected) {
        return Err("Process identity changed".into());
    }
    let mut path = vec![0u16; 32768];
    let mut len = path.len() as u32;
    // SAFETY: the owned query handle and writable UTF-16 buffer are valid for the call.
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut len) } == 0
        || !Path::new(&String::from_utf16_lossy(&path[..len as usize]))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&identity.name))
    {
        return Err("Process image could not be verified".into());
    }
    // SAFETY: valid FILETIME output and documented snapshot API.
    let mut capture_start: FILETIME = unsafe { zeroed() };
    unsafe {
        GetSystemTimeAsFileTime(&mut capture_start);
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Thread snapshot failed".into());
    }
    let snapshot = Handle(snapshot);
    // SAFETY: structure size is initialized before enumeration; snapshot remains owned.
    let mut entry: THREADENTRY32 = unsafe { zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    let mut ok = unsafe { Thread32First(snapshot.0, &mut entry) };
    let mut report = ThreadReport {
        rows: Vec::new(),
        unavailable: 0,
    };
    while ok != 0 {
        if entry.th32OwnerProcessID == identity.pid {
            let mut row = ThreadRow {
                tid: entry.th32ThreadID,
                name: None,
                cpu_seconds: None,
                base_priority: entry.tpBasePri,
                ideal_cpu: None,
            };
            // SAFETY: opening query-only handles cannot modify the inspected thread.
            let raw = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, row.tid) };
            if !raw.is_null() {
                let thread = Handle(raw);
                // SAFETY: distinct writable times and a retained query handle.
                let mut times: [FILETIME; 4] = unsafe { zeroed() };
                let valid = unsafe { GetProcessIdOfThread(thread.0) } == identity.pid
                    && unsafe {
                        GetThreadTimes(
                            thread.0,
                            &mut times[0],
                            &mut times[1],
                            &mut times[2],
                            &mut times[3],
                        )
                    } != 0
                    && ticks(times[0]) <= ticks(capture_start)
                    && ticks(times[1]) == 0;
                if valid {
                    row.cpu_seconds =
                        Some((ticks(times[2]) as f64 + ticks(times[3]) as f64) / 10_000_000.0);
                    // SAFETY: writable processor-number structure and retained query handle.
                    // Native thread-information queries require a DWORD-aligned output,
                    // although PROCESSOR_NUMBER itself has only WORD alignment.
                    #[repr(C, align(4))]
                    struct AlignedProcessor(PROCESSOR_NUMBER);
                    let mut ideal = AlignedProcessor(unsafe { zeroed() });
                    if unsafe { GetThreadIdealProcessorEx(thread.0, &mut ideal.0) } != 0 {
                        row.ideal_cpu = Some((ideal.0.Group, ideal.0.Number));
                    }
                    let mut description = std::ptr::null_mut();
                    // SAFETY: API allocates a terminated UTF-16 buffer; free that same allocation exactly once.
                    if unsafe { GetThreadDescription(thread.0, &mut description) } >= 0
                        && !description.is_null()
                    {
                        let mut len = 0;
                        unsafe {
                            while *description.add(len) != 0 {
                                len += 1;
                            }
                            row.name = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                                description,
                                len,
                            )));
                            LocalFree(description.cast());
                        }
                    }
                }
            }
            if row.cpu_seconds.is_none() || row.ideal_cpu.is_none() || row.name.is_none() {
                report.unavailable += 1;
            }
            report.rows.push(row);
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: snapshot and entry remain valid until enumeration finishes.
        ok = unsafe { Thread32Next(snapshot.0, &mut entry) };
    }
    // SAFETY: inspect the failure from the immediately preceding enumeration call.
    if unsafe { GetLastError() } != 18 {
        return Err("Thread enumeration failed before completion".into());
    }
    if process_start(process.0)? != start {
        return Err("Process identity changed".into());
    }
    report.rows.sort_by_key(|row| row.tid);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_threads_capture_current_process_and_reject_wrong_lifetime() {
        let pid = std::process::id();
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let process = system.process(sysinfo::Pid::from_u32(pid)).unwrap();
        let mut identity = ProcessIdentity {
            pid,
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        let report = collect(&identity).unwrap();
        assert!(!report.rows.is_empty());
        assert!(
            report
                .rows
                .iter()
                .any(|r| r.cpu_seconds.is_some() && r.ideal_cpu.is_some()),
            "{report:?}"
        );
        assert!(
            report
                .rows
                .windows(2)
                .all(|pair| pair[0].tid <= pair[1].tid)
        );
        identity.start_time = identity.start_time.map(|t| t + 1);
        assert!(collect(&identity).is_err());
    }
}
