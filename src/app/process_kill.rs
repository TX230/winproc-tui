use super::{App, state::ProcessKillTarget};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};
use winapi::{
    shared::{
        minwindef::{FALSE, FILETIME},
        ntdef::HANDLE,
        winerror::{ERROR_INVALID_PARAMETER, WAIT_TIMEOUT},
    },
    um::{
        handleapi::CloseHandle,
        processthreadsapi::{GetProcessTimes, OpenProcess, TerminateProcess},
        synchapi::WaitForSingleObject,
        winbase::{QueryFullProcessImageNameW, WAIT_OBJECT_0},
        winnt::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, SYNCHRONIZE},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KillOutcome {
    Killed,
    Exited,
    Failed(String),
    PendingExit,
}
impl KillOutcome {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Killed => "Killed".into(),
            Self::Exited => "Already exited".into(),
            Self::Failed(reason) => format!("Failed: {reason}"),
            Self::PendingExit => "Termination requested; exit not confirmed".into(),
        }
    }
}
pub(crate) struct KillJob {
    receiver: Receiver<(usize, KillOutcome)>,
    stop: Arc<AtomicBool>,
    pub(crate) outcomes: Vec<Option<KillOutcome>>,
}
impl KillJob {
    pub(crate) fn spawn(targets: Vec<ProcessKillTarget>) -> Self {
        Self::spawn_with(targets, terminate)
    }
    pub(crate) fn spawn_with(
        targets: Vec<ProcessKillTarget>,
        mut attempt: impl FnMut(&ProcessKillTarget) -> KillOutcome + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let mut job = Self {
            receiver,
            stop,
            outcomes: vec![None; targets.len()],
        };
        let result = thread::Builder::new()
            .name("process-kill".into())
            .spawn(move || {
                for (index, target) in targets.iter().enumerate() {
                    if worker_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let outcome = attempt(target);
                    if sender.send((index, outcome)).is_err() {
                        break;
                    }
                }
            });
        if let Err(error) = result {
            job.fail_remaining(format!("Cannot start worker: {error}"));
        }
        job
    }
    fn fail_remaining(&mut self, reason: String) {
        for outcome in &mut self.outcomes {
            if outcome.is_none() {
                *outcome = Some(KillOutcome::Failed(reason.clone()));
            }
        }
    }
    pub(crate) fn poll(&mut self) -> bool {
        if self.finished() {
            return false;
        }
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok((index, outcome)) => {
                    self.outcomes[index] = Some(outcome);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.finished() {
                        self.fail_remaining("Worker stopped; outcome unknown".into());
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }
    pub(crate) fn finished(&self) -> bool {
        self.outcomes.iter().all(Option::is_some)
    }
    pub(crate) fn summary(&self) -> String {
        let mut killed = 0;
        let mut exited = 0;
        let mut failed = 0;
        let mut pending = 0;
        for outcome in self.outcomes.iter().flatten() {
            match outcome {
                KillOutcome::Killed => killed += 1,
                KillOutcome::Exited => exited += 1,
                KillOutcome::Failed(_) => failed += 1,
                KillOutcome::PendingExit => pending += 1,
            }
        }
        format!(
            "{}/{} processed | {} killed | {} failed | {} exited | {} unconfirmed",
            killed + exited + failed + pending,
            self.outcomes.len(),
            killed,
            failed,
            exited,
            pending
        )
    }
}
impl Drop for KillJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
impl App {
    pub(crate) fn poll_process_kill(&mut self) -> bool {
        self.process_kill_job.as_mut().is_some_and(KillJob::poll)
    }
}

struct ProcessHandle(HANDLE);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn native_error(operation: &str) -> KillOutcome {
    KillOutcome::Failed(format!("{operation}: {}", std::io::Error::last_os_error()))
}
fn terminate(target: &ProcessKillTarget) -> KillOutcome {
    let Some(expected_start) = target.identity.start_time else {
        return KillOutcome::Failed("Creation time unavailable; target cannot be verified".into());
    };
    // SAFETY: scalar PID/access arguments. The owned handle is never reopened by PID after verification.
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
            FALSE,
            target.identity.pid,
        )
    };
    if raw.is_null() {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
            && target.identity.pid != 0
        {
            KillOutcome::Exited
        } else {
            KillOutcome::Failed(format!("OpenProcess: {error}"))
        };
    }
    let process = ProcessHandle(raw);
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    // SAFETY: valid owned handle and distinct initialized FILETIME outputs.
    if unsafe {
        GetProcessTimes(
            process.0,
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    } == 0
    {
        return native_error("GetProcessTimes");
    }
    let created = (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
    if (created / 10_000_000).checked_sub(11_644_473_600) != Some(expected_start) {
        return KillOutcome::Failed("Process identity changed; no termination attempted".into());
    }
    // An exited process may no longer expose its image name. No action is taken.
    if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
        return KillOutcome::Exited;
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    // SAFETY: writable UTF-16 buffer with its capacity, and owned query-capable handle.
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return native_error("QueryFullProcessImageNameW");
    }
    let path = String::from_utf16_lossy(&path[..length as usize]);
    if !Path::new(&path).file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case(&target.identity.name)
    }) {
        return KillOutcome::Failed("Process identity changed; no termination attempted".into());
    }
    // SAFETY: wait and termination use the same verified handle, preventing a PID-reuse race.
    match unsafe { WaitForSingleObject(process.0, 0) } {
        WAIT_OBJECT_0 => return KillOutcome::Exited,
        WAIT_TIMEOUT => {}
        _ => return native_error("WaitForSingleObject"),
    }
    if unsafe { TerminateProcess(process.0, 1) } == 0 {
        let error = native_error("TerminateProcess");
        return if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            KillOutcome::Exited
        } else {
            error
        };
    }
    match unsafe { WaitForSingleObject(process.0, 1000) } {
        WAIT_OBJECT_0 => KillOutcome::Killed,
        _ => KillOutcome::PendingExit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    #[test]
    #[ignore = "disposable child invoked only by controlled termination test"]
    fn disposable_child() {
        thread::sleep(Duration::from_secs(60));
    }
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn child() -> (OwnedChild, ProcessKillTarget) {
        let executable = std::env::current_exe().unwrap();
        let child = OwnedChild(
            Command::new(&executable)
                .args([
                    "--ignored",
                    "--exact",
                    "app::process_kill::tests::disposable_child",
                    "--nocapture",
                ])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
        );
        let process = system.process(sysinfo::Pid::from_u32(pid)).unwrap();
        assert_eq!(
            process.exe().unwrap().canonicalize().unwrap(),
            executable.canonicalize().unwrap()
        );
        let name = process.name().to_string_lossy().into_owned();
        let identity = crate::model::ProcessIdentity {
            pid,
            name: name.clone(),
            start_time: Some(process.start_time()),
        };
        eprintln!(
            "Verified disposable target: {:?}; executable={}",
            identity,
            executable.display()
        );
        (
            child,
            ProcessKillTarget {
                pid,
                name,
                identity,
            },
        )
    }
    #[test]
    fn process_kill_native_rejects_wrong_identity_and_reports_exit() {
        let (mut child, target) = child();
        let mut wrong = target.clone();
        wrong.identity.start_time = target.identity.start_time.map(|time| time + 1);
        assert!(matches!(terminate(&wrong), KillOutcome::Failed(_)));
        assert!(child.0.try_wait().unwrap().is_none());
        wrong = target.clone();
        wrong.identity.name = "wrong-executable.exe".into();
        assert!(matches!(terminate(&wrong), KillOutcome::Failed(_)));
        assert!(child.0.try_wait().unwrap().is_none());
        wrong = target.clone();
        wrong.identity.start_time = None;
        assert!(matches!(terminate(&wrong), KillOutcome::Failed(_)));
        assert!(child.0.try_wait().unwrap().is_none());
        assert_eq!(terminate(&target), KillOutcome::Killed);
        child.0.wait().unwrap();
        assert_eq!(terminate(&target), KillOutcome::Exited);
    }
    #[test]
    fn process_kill_worker_reports_mixed_outcomes_incrementally_and_disconnects() {
        let (mut first, target) = child();
        let (mut second, exited) = child();
        assert_eq!(terminate(&exited), KillOutcome::Killed);
        second.0.wait().unwrap();
        let mut invalid = target.clone();
        invalid.identity.start_time = None;
        let mut job = KillJob::spawn(vec![invalid, exited, target]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !job.finished() {
            job.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(job.outcomes[0], Some(KillOutcome::Failed(_))));
        assert_eq!(job.outcomes[1], Some(KillOutcome::Exited));
        assert_eq!(job.outcomes[2], Some(KillOutcome::Killed));
        first.0.wait().unwrap();
        assert!(
            job.summary()
                .contains("3/3 processed | 1 killed | 1 failed | 1 exited")
        );
        let (_, receiver) = mpsc::channel();
        let mut broken = KillJob {
            receiver,
            stop: Arc::new(AtomicBool::new(false)),
            outcomes: vec![None],
        };
        assert!(broken.poll());
        assert!(broken.finished());
        assert!(matches!(broken.outcomes[0], Some(KillOutcome::Failed(_))));
    }
    #[test]
    fn process_kill_drop_skips_unstarted_attempts() {
        let target = ProcessKillTarget {
            identity: crate::model::ProcessIdentity {
                pid: 0,
                name: "fake".into(),
                start_time: None,
            },
            pid: 0,
            name: "fake".into(),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let job = KillJob::spawn_with(vec![target.clone(), target], move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            KillOutcome::Exited
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(job);
        release_tx.send(()).unwrap();
        assert!(started_rx.recv_timeout(Duration::from_secs(2)).is_err());
    }
}
