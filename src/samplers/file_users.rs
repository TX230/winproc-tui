use std::{
    io::Read,
    mem::{size_of, zeroed},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    ptr::null_mut,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use winapi::{
    shared::minwindef::FALSE,
    um::{
        jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject},
        namedpipeapi::PeekNamedPipe,
        synchapi::WaitForSingleObject,
        winbase::CREATE_NO_WINDOW,
        winnt::{
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        },
    },
};

use super::open_files::OwnedHandle;
use crate::model::file_users::*;

type Latest = Arc<Mutex<Option<FileUsersUpdate>>>;
type Work = (u64, FileUsersRequest);
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub(crate) struct FileUsersWorker {
    tx: Option<SyncSender<Work>>,
    active: Arc<AtomicU64>,
    latest: Latest,
    join: Option<JoinHandle<()>>,
}

impl FileUsersWorker {
    pub(crate) fn spawn() -> Self {
        let (tx, rx) = mpsc::sync_channel::<Work>(1);
        let active = Arc::new(AtomicU64::new(0));
        let latest = Arc::new(Mutex::new(None));
        let running = active.clone();
        let updates = latest.clone();
        let join = thread::spawn(move || worker_loop(rx, running, updates));
        Self {
            tx: Some(tx),
            active,
            latest,
            join: Some(join),
        }
    }

    pub(crate) fn request(&self, id: u64, request: FileUsersRequest) -> Result<()> {
        if let FileUsersRequest::Search(query) = &request {
            query.validate().map_err(|e| anyhow!(e))?;
        }
        self.active.store(id, Ordering::Release);
        self.tx
            .as_ref()
            .ok_or_else(|| anyhow!("Search worker unavailable"))?
            .try_send((id, request))
            .map_err(|_| anyhow!("Search worker busy; try again"))
    }

    pub(crate) fn cancel(&self) {
        self.active.store(0, Ordering::Release);
    }

    pub(crate) fn take_update(&self) -> Option<FileUsersUpdate> {
        self.latest.lock().ok()?.take()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, Receiver<Work>, Latest) {
        let (tx, rx) = mpsc::sync_channel(1);
        let latest = Arc::new(Mutex::new(None));
        (
            Self {
                tx: Some(tx),
                active: Arc::new(AtomicU64::new(0)),
                latest: latest.clone(),
                join: None,
            },
            rx,
            latest,
        )
    }
}

impl Drop for FileUsersWorker {
    fn drop(&mut self) {
        self.cancel();
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn publish(latest: &Latest, update: FileUsersUpdate) {
    if let Ok(mut slot) = latest.lock() {
        *slot = Some(update);
    }
}

static SCAN_LOCK: Mutex<()> = Mutex::new(());

fn worker_loop(requests: Receiver<Work>, active: Arc<AtomicU64>, latest: Latest) {
    while let Ok((id, request)) = requests.recv() {
        if active.load(Ordering::Acquire) != id {
            continue;
        }
        publish(
            &latest,
            FileUsersUpdate {
                id,
                report: FileSearchReport::default(),
                owner: None,
            },
        );
        let requested_at = Instant::now();
        let result = (|| {
            let _scan = if matches!(&request, FileUsersRequest::Verify(_)) {
                None
            } else {
                Some(
                    SCAN_LOCK
                        .try_lock()
                        .map_err(|_| anyhow!("Another file-user scan is active"))?,
                )
            };
            let mut command = Command::new(std::env::current_exe()?);
            command
                .arg("--file-users-helper")
                .arg(serde_json::to_string(&request)?);
            run_command(
                command,
                id,
                &active,
                &latest,
                Duration::from_secs(30),
                Duration::from_secs(5),
            )
        })();
        if let Err(error) = result {
            publish(
                &latest,
                FileUsersUpdate {
                    id,
                    report: FileSearchReport {
                        elapsed: Some(requested_at.elapsed()),
                        end: Some(FileSearchEnd::Failed(error.to_string())),
                        ..FileSearchReport::default()
                    },
                    owner: None,
                },
            );
        }
    }
}

struct Helper {
    child: Child,
    _job: OwnedHandle,
}

impl Helper {
    fn spawn(mut command: Command) -> Result<Self> {
        // SAFETY: null security attributes create a non-inheritable, unnamed owned job.
        let raw_job = unsafe { CreateJobObjectW(null_mut(), null_mut()) };
        if raw_job.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let job = OwnedHandle(raw_job);
        // SAFETY: the job limit structure consists of integers and C-compatible value structs.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.ProcessMemoryLimit = 512 * 1024 * 1024;
        // SAFETY: the owned job and matching fully initialized limit structure remain live.
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == FALSE
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut child = command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        // SAFETY: both handles are owned and live. The helper creates no subprocesses.
        if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as _) } == FALSE {
            let error = std::io::Error::last_os_error();
            let _ = child.kill();
            return Err(anyhow!("Cannot isolate file search: {error}"));
        }
        Ok(Self { child, _job: job })
    }

    fn read_available(&mut self) -> Result<Vec<u8>> {
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("Search pipe unavailable"))?;
        let mut available = 0;
        // SAFETY: this thread is the only pipe reader; no buffer is requested and the byte-count
        // output is live. Read is limited to the bytes reported as already available.
        if unsafe {
            PeekNamedPipe(
                stdout.as_raw_handle() as _,
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } == FALSE
        {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(109) {
                return Ok(Vec::new());
            }
            return Err(error.into());
        }
        let mut bytes = vec![0; (available as usize).min(64 * 1024)];
        if !bytes.is_empty() {
            stdout.read_exact(&mut bytes)?;
        }
        Ok(bytes)
    }

    fn terminate(&mut self) -> bool {
        let _ = self.child.kill();
        // SAFETY: Child retains its process handle throughout this bounded wait. Never use an
        // unbounded Child::wait: a faulty driver may retain I/O even after termination is requested.
        unsafe { WaitForSingleObject(self.child.as_raw_handle() as _, 1000) == 0 }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Also covers parser/pipe errors. The job closes after Child and requests cleanup even
        // when Windows has not yet completed a pending kernel operation.
        let _ = self.child.kill();
    }
}

fn run_command(
    command: Command,
    id: u64,
    active: &AtomicU64,
    latest: &Latest,
    total_timeout: Duration,
    idle_timeout: Duration,
) -> Result<()> {
    let capture_started = Instant::now();
    let mut helper = Helper::spawn(command)?;
    let started = Instant::now();
    let mut last_message = started;
    let mut last_publish = started;
    let mut buffer = Vec::new();
    let mut report = FileSearchReport::default();
    let mut retained_bytes = 0;
    let mut owner = None;
    let result: Result<()> = (|| {
        loop {
            if active.load(Ordering::Acquire) != id {
                report.end = Some(FileSearchEnd::Cancelled);
                break;
            }
            if started.elapsed() >= total_timeout || last_message.elapsed() >= idle_timeout {
                report.end = Some(FileSearchEnd::TimedOut);
                break;
            }
            let bytes = helper.read_available()?;
            let received = !bytes.is_empty();
            buffer.extend(bytes);
            if buffer.len() > MAX_MESSAGE_BYTES {
                return Err(anyhow!("Search reply exceeded its size limit"));
            }
            while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
                let message: FileUsersMessage = serde_json::from_slice(&buffer[..end])?;
                buffer.drain(..=end);
                last_message = Instant::now();
                match message {
                    FileUsersMessage::Progress(progress) => report.progress = progress,
                    FileUsersMessage::Match(entry) => {
                        retained_bytes += entry.retained_bytes();
                        if report.matches.len() >= MAX_MATCHES || retained_bytes > MAX_RESULT_BYTES
                        {
                            return Err(anyhow!("Search result exceeded its size limit"));
                        }
                        report.matches.push(entry);
                    }
                    FileUsersMessage::Owner(value) => owner = Some(value),
                    FileUsersMessage::End(end) => report.end = Some(end),
                }
            }
            if report.end.is_some() {
                break;
            }
            if !received && helper.child.try_wait()?.is_some() {
                return Err(anyhow!(
                    "Search helper exited without completing its report"
                ));
            }
            if last_publish.elapsed() >= Duration::from_millis(100) {
                publish(
                    latest,
                    FileUsersUpdate {
                        id,
                        report: report.clone(),
                        owner: None,
                    },
                );
                last_publish = Instant::now();
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.end = Some(FileSearchEnd::Failed(error.to_string()));
    }
    if !helper.terminate() {
        report.cleanup_warning =
            Some("Windows has not confirmed helper termination; cleanup remains pending".into());
    }
    report
        .matches
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.owner.pid.cmp(&b.owner.pid)));
    report.elapsed = Some(capture_started.elapsed());
    publish(latest, FileUsersUpdate { id, report, owner });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_users_cancels_and_times_out_stalled_helpers_repeatedly() {
        for cancel in [true, true, false] {
            let active = Arc::new(AtomicU64::new(1));
            let latest: Latest = Arc::new(Mutex::new(None));
            let running = active.clone();
            let updates = latest.clone();
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 60",
            ]);
            let started = Instant::now();
            let worker = thread::spawn(move || {
                run_command(
                    command,
                    1,
                    &running,
                    &updates,
                    Duration::from_millis(400),
                    Duration::from_millis(400),
                )
                .unwrap()
            });
            if cancel {
                thread::sleep(Duration::from_millis(100));
                active.store(0, Ordering::Release);
            }
            worker.join().unwrap();
            assert!(started.elapsed() < Duration::from_secs(5));
            let update = latest.lock().unwrap().take().unwrap();
            assert_eq!(
                update.report.end,
                Some(if cancel {
                    FileSearchEnd::Cancelled
                } else {
                    FileSearchEnd::TimedOut
                })
            );
            assert!(update.report.elapsed.is_some_and(|time| !time.is_zero()));
            assert!(update.report.cleanup_warning.is_none());
        }
    }
}
