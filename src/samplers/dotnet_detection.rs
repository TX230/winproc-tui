use super::dotnet_control::DotNetControl;
use crate::model::ProcessIdentity;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct DotNetDetection {
    requests: SyncSender<Vec<ProcessIdentity>>,
    cancelled: Arc<AtomicBool>,
}
impl DotNetDetection {
    pub(super) fn new(control: DotNetControl) -> Self {
        let (requests, receiver) = mpsc::sync_channel::<Vec<ProcessIdentity>>(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        thread::spawn(move || {
            let mut checked = HashMap::<ProcessIdentity, Instant>::new();
            while let Ok(identities) = receiver.recv() {
                checked.retain(|identity, _| identities.contains(identity));
                for identity in identities {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    if control.is_managed(&identity)
                        || checked
                            .get(&identity)
                            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
                    {
                        continue;
                    }
                    checked.insert(identity.clone(), Instant::now());
                    if super::dotnet_runtime::verify_identity(&identity).is_err() {
                        continue;
                    }
                    let detected = super::process_info::detect_dotnet_runtime(identity.pid);
                    if let Some(supported) = detected
                        && super::dotnet_runtime::verify_identity(&identity).is_ok()
                    {
                        control.report_managed(identity, supported);
                    }
                }
            }
        });
        Self {
            requests,
            cancelled,
        }
    }
    pub(super) fn request(&self, identities: Vec<ProcessIdentity>) {
        let _ = self.requests.try_send(identities);
    }
}
impl Drop for DotNetDetection {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
