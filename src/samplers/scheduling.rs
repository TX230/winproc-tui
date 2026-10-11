mod affinity;
use crate::model::affinity::{AffinityChange, AffinityReport};
use crate::model::{
    ProcessIdentity,
    scheduling::{PriorityChange, PriorityClass, PriorityOutcome, PriorityReport},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use winapi::{
    shared::{
        minwindef::{FALSE, FILETIME},
        ntdef::HANDLE,
    },
    um::{
        handleapi::CloseHandle,
        processthreadsapi::{GetPriorityClass, GetProcessTimes, OpenProcess, SetPriorityClass},
        winbase::QueryFullProcessImageNameW,
        winnt::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION},
    },
};

#[derive(Debug)]
pub(crate) struct SchedulingRequest {
    pub(crate) generation: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) change: Option<PriorityChange>,
    pub(crate) affinity_change: Option<AffinityChange>,
}

pub(crate) struct SchedulingResult {
    pub(crate) generation: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) report: PriorityReport,
    pub(crate) affinity: Option<AffinityReport>,
    pub(crate) qos: Option<crate::model::scheduling::ProcessQos>,
}

pub(crate) struct SchedulingWorker {
    sender: Option<SyncSender<SchedulingRequest>>,
    receiver: Receiver<SchedulingResult>,
    active: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

impl SchedulingWorker {
    pub(crate) fn spawn() -> Self {
        let (sender, requests) = mpsc::sync_channel::<SchedulingRequest>(1);
        let (results, receiver) = mpsc::channel();
        let active = Arc::new(AtomicU64::new(0));
        let worker_active = active.clone();
        let join = thread::spawn(move || {
            let mut session: Option<(u64, ProcessIdentity, PrioritySession<NativeProcess>)> = None;
            loop {
                if session.as_ref().is_some_and(|(generation, _, _)| {
                    *generation != worker_active.load(Ordering::SeqCst)
                }) {
                    session = None;
                }
                let request = match requests.recv_timeout(Duration::from_millis(50)) {
                    Ok(request) => request,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let still_active = || worker_active.load(Ordering::SeqCst) == request.generation;
                if !still_active() {
                    continue;
                }
                let matching = session.as_ref().is_some_and(|(generation, identity, _)| {
                    *generation == request.generation && *identity == request.identity
                });
                let report = if !matching
                    && (request.change.is_some() || request.affinity_change.is_some())
                {
                    PriorityReport::unavailable(
                        "Scheduling session expired; refresh before changing priority".into(),
                    )
                } else {
                    let opened = if matching {
                        Ok(())
                    } else {
                        session = None;
                        NativeProcess::open(&request.identity).map(|process| {
                            let writable = process.writable;
                            session = Some((
                                request.generation,
                                request.identity.clone(),
                                PrioritySession {
                                    process,
                                    writable,
                                    restore: None,
                                },
                            ));
                        })
                    };
                    match opened {
                        Ok(()) => session
                            .as_mut()
                            .expect("opened session")
                            .2
                            .execute(request.change, still_active),
                        Err(error) => PriorityReport::unavailable(error),
                    }
                };
                let affinity = if let Some((generation, identity, session)) = session.as_mut() {
                    if *generation == request.generation && *identity == request.identity {
                        let mut restore = session.process.affinity_restore.take();
                        let mut affinity = affinity::execute(
                            &mut session.process,
                            session.writable,
                            &mut restore,
                            request.affinity_change,
                            still_active,
                        );
                        session.process.affinity_restore = restore;
                        affinity.kinds = session.process.affinity_kinds.clone();
                        Some(affinity)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let qos = session
                    .as_ref()
                    .filter(|(generation, identity, _)| {
                        *generation == request.generation && *identity == request.identity
                    })
                    .and_then(|(_, _, session)| session.process.qos());
                if results
                    .send(SchedulingResult {
                        generation: request.generation,
                        identity: request.identity,
                        report,
                        affinity,
                        qos,
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
            active,
            join: Some(join),
        }
    }

    pub(crate) fn request(&self, request: SchedulingRequest) -> Result<(), String> {
        if request.change.is_none() && request.affinity_change.is_none() {
            self.active.store(request.generation, Ordering::SeqCst);
        }
        self.sender
            .as_ref()
            .ok_or("Scheduling worker stopped")?
            .try_send(request)
            .map_err(|error| format!("Scheduling request unavailable: {error}"))
    }

    pub(crate) fn cancel(&self) {
        self.active.store(0, Ordering::SeqCst);
    }
    pub(crate) fn try_recv(&self) -> Result<SchedulingResult, TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn test_pair() -> (
        Self,
        Receiver<SchedulingRequest>,
        mpsc::Sender<SchedulingResult>,
    ) {
        let (sender, requests) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::channel();
        (
            Self {
                sender: Some(sender),
                receiver,
                active: Arc::new(AtomicU64::new(0)),
                join: None,
            },
            requests,
            results,
        )
    }
}

impl Drop for SchedulingWorker {
    fn drop(&mut self) {
        self.cancel();
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

trait PriorityProcess {
    fn read(&mut self) -> Result<PriorityClass, String>;
    fn set(&mut self, class: PriorityClass) -> Result<(), String>;
}

struct PrioritySession<P> {
    process: P,
    writable: bool,
    restore: Option<PriorityChange>,
}

impl<P: PriorityProcess> PrioritySession<P> {
    fn execute(
        &mut self,
        change: Option<PriorityChange>,
        active: impl Fn() -> bool,
    ) -> PriorityReport {
        let mut report = PriorityReport {
            current: None,
            writable: self.writable,
            restore: self.restore,
            outcome: PriorityOutcome::Read,
        };
        let current = match self.process.read() {
            Ok(current) => {
                report.current = Some(current);
                current
            }
            Err(error) => {
                report.outcome = PriorityOutcome::Failed(error);
                return report;
            }
        };
        let Some(change) = change else {
            return report;
        };
        let rejection = if !change.desired.editable() {
            Some("This priority class cannot be selected".into())
        } else if !self.writable {
            Some("Read-only: permission to change priority is unavailable".into())
        } else if change.expected != current {
            Some(format!(
                "Priority changed externally: expected {}, now {}. Review the current value.",
                change.expected.label(),
                current.label()
            ))
        } else if change.restore && self.restore != Some(change) {
            Some("The previous priority is no longer eligible for restoration".into())
        } else if !active() {
            Some("Priority change cancelled before application".into())
        } else {
            None
        };
        if let Some(error) = rejection {
            report.outcome = PriorityOutcome::Failed(error);
            return report;
        }
        if let Err(error) = self.process.set(change.desired) {
            report.outcome = PriorityOutcome::Failed(error);
            return report;
        }
        self.restore = if change.restore {
            None
        } else {
            Some(PriorityChange {
                expected: change.desired,
                desired: current,
                restore: true,
            })
        };
        report.restore = self.restore;
        match self.process.read() {
            Ok(actual) => {
                report.current = Some(actual);
                report.outcome = if actual != change.desired {
                    PriorityOutcome::AppliedUnverified(format!(
                        "Change accepted, but readback is {} instead of {}. Refresh before continuing.",
                        actual.label(),
                        change.desired.label()
                    ))
                } else if change.restore {
                    PriorityOutcome::Restored
                } else {
                    PriorityOutcome::Changed
                };
            }
            Err(error) => {
                report.current = None;
                report.outcome = PriorityOutcome::AppliedUnverified(format!(
                    "Change to {} accepted; verification failed: {error}",
                    change.desired.label()
                ));
            }
        }
        report
    }
}

struct NativeProcess {
    affinity_restore: Option<AffinityChange>,
    affinity_kinds: Vec<Option<crate::model::CpuCoreKind>>,
    handle: HANDLE,
    creation_time: u64,
    writable: bool,
}

fn native_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
fn filetime(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

impl NativeProcess {
    fn open(identity: &ProcessIdentity) -> Result<Self, String> {
        let expected_start = identity
            .start_time
            .ok_or("Process creation time is unavailable; priority cannot be verified")?;
        // SAFETY: OpenProcess takes a PID and access flags, with no pointer arguments.
        let mut handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
                FALSE,
                identity.pid,
            )
        };
        let writable = !handle.is_null();
        if !writable {
            // SAFETY: query-only fallback uses the same PID; identity is checked on this held handle.
            handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, identity.pid) };
        }
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            let reason = match error.raw_os_error() {
                Some(5) => "Access denied",
                Some(87 | 1168) => "Process exited or is no longer available",
                _ => "OpenProcess failed",
            };
            return Err(format!("{reason}: {error}"));
        }
        let mut process = Self {
            handle,
            creation_time: 0,
            affinity_restore: None,
            affinity_kinds: super::cpu::affinity_core_kinds(),
            writable,
        };
        let creation_time = process.live_creation_time()?;
        if creation_time / 10_000_000 - 11_644_473_600 != expected_start {
            return Err("Process identity changed; reopen Process Info".into());
        }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        // SAFETY: this owned process handle is live; the buffer and its length remain valid for the call.
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) } == 0 {
            return Err(native_error("QueryFullProcessImageNameW"));
        }
        let path = String::from_utf16_lossy(&path[..length as usize]);
        if !Path::new(&path)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&identity.name))
        {
            return Err("Process identity changed; reopen Process Info".into());
        }
        process.creation_time = creation_time;
        Ok(process)
    }

    fn qos(&self) -> Option<crate::model::scheduling::ProcessQos> {
        if self.live_creation_time().ok()? != self.creation_time {
            return None;
        }
        // PROCESS_POWER_THROTTLING_STATE and ProcessPowerThrottling (4) are
        // documented Windows SDK declarations missing from winapi 0.3.9.
        #[repr(C)]
        struct PowerThrottling {
            version: u32,
            control: u32,
            state: u32,
        }
        let mut value = PowerThrottling {
            version: 1,
            control: 0,
            state: 0,
        };
        // SAFETY: retained verified process handle, DWORD-aligned C structure,
        // initialized version, and exact structure size for this information class.
        if unsafe {
            winapi::um::processthreadsapi::GetProcessInformation(
                self.handle,
                4,
                (&mut value as *mut PowerThrottling).cast(),
                std::mem::size_of::<PowerThrottling>() as u32,
            )
        } == 0
        {
            return None;
        }
        if self.live_creation_time().ok()? != self.creation_time {
            return None;
        }
        Some(crate::model::scheduling::ProcessQos::from_masks(
            value.control,
            value.state,
        ))
    }

    fn live_creation_time(&self) -> Result<u64, String> {
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        // SAFETY: four distinct writable FILETIME outputs accompany an owned process handle.
        let ok = unsafe {
            GetProcessTimes(
                self.handle,
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        };
        if ok == 0 {
            return Err(native_error("GetProcessTimes"));
        }
        if filetime(times[1]) != 0 {
            return Err("Process exited".into());
        }
        Ok(filetime(times[0]))
    }
}

impl PriorityProcess for NativeProcess {
    fn read(&mut self) -> Result<PriorityClass, String> {
        if self.live_creation_time()? != self.creation_time {
            return Err("Process identity changed".into());
        }
        // SAFETY: the retained, verified process handle includes query access.
        let value = unsafe { GetPriorityClass(self.handle) };
        if value == 0 {
            Err(native_error("GetPriorityClass"))
        } else {
            Ok(PriorityClass::from_native(value))
        }
    }

    fn set(&mut self, class: PriorityClass) -> Result<(), String> {
        if !class.editable() {
            return Err("This priority class cannot be selected".into());
        }
        if self.live_creation_time()? != self.creation_time {
            return Err("Process identity changed".into());
        }
        // SAFETY: the session retains this exact process object; only the five allowed CPU classes reach the API.
        if unsafe { SetPriorityClass(self.handle, class.native()) } == 0 {
            Err(native_error("SetPriorityClass"))
        } else {
            Ok(())
        }
    }
}

impl Drop for NativeProcess {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the OpenProcess handle and closes it exactly once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    struct FakeProcess {
        current: PriorityClass,
        writes: usize,
        fail_set: bool,
        fail_readback: bool,
    }
    impl PriorityProcess for FakeProcess {
        fn read(&mut self) -> Result<PriorityClass, String> {
            if self.fail_readback && self.writes > 0 {
                Err("GetPriorityClass: simulated readback failure".into())
            } else {
                Ok(self.current)
            }
        }
        fn set(&mut self, class: PriorityClass) -> Result<(), String> {
            if self.fail_set {
                return Err("SetPriorityClass: access denied".into());
            }
            self.writes += 1;
            self.current = class;
            Ok(())
        }
    }
    fn fake() -> PrioritySession<FakeProcess> {
        PrioritySession {
            process: FakeProcess {
                current: PriorityClass::Normal,
                writes: 0,
                fail_set: false,
                fail_readback: false,
            },
            writable: true,
            restore: None,
        }
    }
    fn change(desired: PriorityClass) -> PriorityChange {
        PriorityChange {
            expected: PriorityClass::Normal,
            desired,
            restore: false,
        }
    }

    #[test]
    fn priority_rejects_realtime_unknown_readonly_external_changes_and_cancelled_requests() {
        for desired in [PriorityClass::Realtime, PriorityClass::Unknown(0x100000)] {
            let mut session = fake();
            assert!(matches!(
                session.execute(Some(change(desired)), || true).outcome,
                PriorityOutcome::Failed(_)
            ));
            assert_eq!(session.process.writes, 0);
        }
        for (writable, current, active) in [
            (false, PriorityClass::Normal, true),
            (true, PriorityClass::BelowNormal, true),
            (true, PriorityClass::Normal, false),
        ] {
            let mut session = fake();
            session.writable = writable;
            session.process.current = current;
            let report = session.execute(Some(change(PriorityClass::High)), || active);
            assert!(matches!(report.outcome, PriorityOutcome::Failed(_)));
            assert_eq!(report.current, Some(current));
            assert_eq!(session.process.writes, 0);
        }
    }

    #[test]
    fn priority_distinguishes_failed_set_from_applied_but_unverified_and_guards_restore() {
        let mut session = fake();
        session.process.fail_set = true;
        let report = session.execute(Some(change(PriorityClass::BelowNormal)), || true);
        assert!(matches!(report.outcome, PriorityOutcome::Failed(_)));
        assert!(report.restore.is_none());
        session.process.fail_set = false;
        session.process.fail_readback = true;
        let report = session.execute(Some(change(PriorityClass::BelowNormal)), || true);
        assert!(matches!(
            report.outcome,
            PriorityOutcome::AppliedUnverified(_)
        ));
        assert_eq!(report.current, None);
        assert_eq!(session.process.writes, 1);
        session.process.fail_readback = false;
        let restore = report.restore.unwrap();
        session.process.current = PriorityClass::High;
        assert!(matches!(
            session.execute(Some(restore), || true).outcome,
            PriorityOutcome::Failed(_)
        ));
        assert_eq!(session.process.writes, 1);
        session.process.current = PriorityClass::BelowNormal;
        let restored = session.execute(Some(restore), || true);
        assert_eq!(restored.outcome, PriorityOutcome::Restored);
        assert_eq!(restored.current, Some(PriorityClass::Normal));
        assert!(restored.restore.is_none());
        session.process.current = PriorityClass::Realtime;
        let from_realtime = PriorityChange {
            expected: PriorityClass::Realtime,
            desired: PriorityClass::Normal,
            restore: false,
        };
        let report = session.execute(Some(from_realtime), || true);
        let writes = session.process.writes;
        assert!(matches!(
            session.execute(report.restore, || true).outcome,
            PriorityOutcome::Failed(_)
        ));
        assert_eq!(session.process.writes, writes);
    }

    #[test]
    fn priority_fixture_child() {
        if std::env::var_os("WINPROC_PRIORITY_FIXTURE").is_none() {
            return;
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }

    #[test]
    fn priority_native_child_change_readback_restore_external_change_and_exit() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "samplers::scheduling::tests::priority_fixture_child",
                    "--nocapture",
                ])
                .env("WINPROC_PRIORITY_FIXTURE", "1")
                .creation_flags(0x08000000 | 0x20)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let process = system.process(sysinfo::Pid::from_u32(pid)).unwrap();
        let identity = ProcessIdentity {
            pid,
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        let process = NativeProcess::open(&identity).unwrap();
        let mut external = NativeProcess::open(&identity).unwrap();
        let mut session = PrioritySession {
            writable: process.writable,
            process,
            restore: None,
        };
        let mut current = session.execute(None, || true).current.unwrap();
        for desired in PriorityClass::EDITABLE {
            let report = session.execute(
                Some(PriorityChange {
                    expected: current,
                    desired,
                    restore: false,
                }),
                || true,
            );
            assert_eq!(report.outcome, PriorityOutcome::Changed);
            assert_eq!(report.current, Some(desired));
            assert_eq!(external.read().unwrap(), desired);
            current = desired;
        }
        let report = session.execute(session.restore, || true);
        assert_eq!(report.outcome, PriorityOutcome::Restored);
        assert_eq!(external.read().unwrap(), PriorityClass::AboveNormal);
        external.set(PriorityClass::BelowNormal).unwrap();
        let report = session.execute(
            Some(PriorityChange {
                expected: PriorityClass::AboveNormal,
                desired: PriorityClass::Normal,
                restore: false,
            }),
            || true,
        );
        assert!(matches!(report.outcome, PriorityOutcome::Failed(_)));
        assert_eq!(external.read().unwrap(), PriorityClass::BelowNormal);
        assert!(external.set(PriorityClass::Realtime).is_err());
        let mut wrong = identity.clone();
        wrong.start_time = identity.start_time.map(|time| time + 1);
        assert!(NativeProcess::open(&wrong).is_err());
        // Exercise the real controller as well as the native session: closing invalidates
        // queued writes, reopening forgets restoration, and dropping never restores a value.
        let worker = SchedulingWorker::spawn();
        worker
            .request(SchedulingRequest {
                affinity_change: None,
                generation: 7,
                identity: identity.clone(),
                change: None,
            })
            .unwrap();
        let first = worker
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(first.report.current, Some(PriorityClass::BelowNormal));
        worker
            .request(SchedulingRequest {
                affinity_change: None,
                generation: 7,
                identity: identity.clone(),
                change: Some(PriorityChange {
                    expected: PriorityClass::BelowNormal,
                    desired: PriorityClass::Normal,
                    restore: false,
                }),
            })
            .unwrap();
        let applied = worker
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(applied.report.outcome, PriorityOutcome::Changed);
        worker.cancel();
        worker
            .request(SchedulingRequest {
                affinity_change: None,
                generation: 7,
                identity: identity.clone(),
                change: Some(change(PriorityClass::High)),
            })
            .unwrap();
        assert!(matches!(
            worker.receiver.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        worker
            .request(SchedulingRequest {
                affinity_change: None,
                generation: 8,
                identity: identity.clone(),
                change: None,
            })
            .unwrap();
        let reopened = worker
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(reopened.report.current, Some(PriorityClass::Normal));
        assert!(reopened.report.restore.is_none());
        drop(worker);
        assert_eq!(external.read().unwrap(), PriorityClass::Normal);
        // Affinity writes touch only this dedicated child, never the test runner.
        use affinity::AffinityProcess;
        // SAFETY: this API has no pointer arguments.
        if unsafe { winapi::um::winbase::GetActiveProcessorGroupCount() } == 1 {
            let (original, allowed) = external.read_affinity().unwrap();
            let first = 1usize << allowed.trailing_zeros();
            let rest = allowed & !first;
            let two = if rest == 0 {
                first
            } else {
                first | (1usize << rest.trailing_zeros())
            };
            let mut restore = None;
            for desired in [first, two, allowed] {
                let expected = external.read_affinity().unwrap().0;
                let report = affinity::execute(
                    &mut session.process,
                    true,
                    &mut restore,
                    Some(AffinityChange {
                        expected,
                        desired,
                        restore: false,
                    }),
                    || true,
                );
                assert_eq!(report.outcome, PriorityOutcome::Changed);
                assert_eq!(external.read_affinity().unwrap().0, desired);
            }
            let request = restore;
            assert_eq!(
                affinity::execute(&mut session.process, true, &mut restore, request, || true)
                    .outcome,
                PriorityOutcome::Restored
            );
            let expected = external.read_affinity().unwrap().0;
            let report = affinity::execute(
                &mut session.process,
                true,
                &mut restore,
                Some(AffinityChange {
                    expected,
                    desired: first,
                    restore: false,
                }),
                || true,
            );
            assert_eq!(report.outcome, PriorityOutcome::Changed);
            if allowed != first {
                external.set_affinity(allowed).unwrap();
                let request = restore;
                assert!(matches!(
                    affinity::execute(&mut session.process, true, &mut restore, request, || true)
                        .outcome,
                    PriorityOutcome::Failed(_)
                ));
            }
            assert!(external.set_affinity(0).is_err());
            // Verify the controller keeps the affinity restoration point only within
            // the current generation and never restores when its worker closes.
            let worker = SchedulingWorker::spawn();
            worker
                .request(SchedulingRequest {
                    generation: 11,
                    identity: identity.clone(),
                    change: None,
                    affinity_change: None,
                })
                .unwrap();
            let before = worker
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .affinity
                .unwrap()
                .current
                .unwrap();
            worker
                .request(SchedulingRequest {
                    generation: 11,
                    identity: identity.clone(),
                    change: None,
                    affinity_change: Some(AffinityChange {
                        expected: before,
                        desired: first,
                        restore: false,
                    }),
                })
                .unwrap();
            let changed = worker
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .affinity
                .unwrap();
            assert_eq!(changed.outcome, PriorityOutcome::Changed);
            worker
                .request(SchedulingRequest {
                    generation: 11,
                    identity: identity.clone(),
                    change: None,
                    affinity_change: changed.restore,
                })
                .unwrap();
            assert_eq!(
                worker
                    .receiver
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .affinity
                    .unwrap()
                    .outcome,
                PriorityOutcome::Restored
            );
            worker.cancel();
            worker
                .request(SchedulingRequest {
                    generation: 11,
                    identity: identity.clone(),
                    change: None,
                    affinity_change: Some(AffinityChange {
                        expected: before,
                        desired: first,
                        restore: false,
                    }),
                })
                .unwrap();
            assert!(matches!(
                worker.receiver.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            worker
                .request(SchedulingRequest {
                    generation: 12,
                    identity: identity.clone(),
                    change: None,
                    affinity_change: None,
                })
                .unwrap();
            let reopened = worker
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .affinity
                .unwrap();
            assert!(reopened.restore.is_none());
            assert_eq!(reopened.current, Some(before));
            drop(worker);
            assert_eq!(external.read_affinity().unwrap().0, before);
            external.set_affinity(original).unwrap();
        } else {
            assert!(external.read_affinity().is_err());
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(external.read_affinity().is_err());
        let report = session.execute(Some(change(PriorityClass::Normal)), || true);
        assert!(
            matches!(report.outcome, PriorityOutcome::Failed(message) if message == "Process exited")
        );
    }
}

#[cfg(test)]
mod qos_native_tests {
    use super::*;
    #[test]
    fn current_process_qos_can_be_queried_without_changing_it() {
        let pid = std::process::id();
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let process = system.process(sysinfo::Pid::from_u32(pid)).unwrap();
        let identity = ProcessIdentity {
            pid,
            name: process.name().to_string_lossy().into_owned(),
            start_time: Some(process.start_time()),
        };
        let native = NativeProcess::open(&identity).unwrap();
        assert!(native.qos().is_some());
    }
}
