use crate::model::{ProcessIdentity, ProcessRow};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct DotNetControl {
    state: Arc<Mutex<State>>,
    generation: Arc<AtomicU64>,
    pending_stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DotNetRuntimeKind {
    Framework,
    EventPipe,
    Other,
}
#[derive(Default)]
struct State {
    enabled: bool,
    managed: HashMap<ProcessIdentity, DotNetRuntimeKind>,
    collected_at: HashMap<ProcessIdentity, Instant>,
    reported_errors: HashSet<ProcessIdentity>,
    draining: HashMap<ProcessIdentity, Weak<AtomicBool>>,
    pending_removal: HashSet<ProcessIdentity>,
    targets: HashMap<ProcessIdentity, (Arc<AtomicBool>, String)>,
}
pub(crate) struct Permit {
    cancelled: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Permit {
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
impl DotNetControl {
    pub(crate) fn report_managed(&self, identity: ProcessIdentity, supported: DotNetRuntimeKind) {
        self.state
            .lock()
            .unwrap()
            .managed
            .insert(identity, supported);
    }
    pub(crate) fn is_managed(&self, identity: &ProcessIdentity) -> bool {
        self.state.lock().unwrap().managed.contains_key(identity)
    }
    pub(crate) fn runtime_kind(&self, identity: &ProcessIdentity) -> Option<DotNetRuntimeKind> {
        self.state.lock().unwrap().managed.get(identity).copied()
    }
    pub(crate) fn supports_metrics(&self, identity: &ProcessIdentity) -> bool {
        self.runtime_kind(identity) == Some(DotNetRuntimeKind::EventPipe)
    }
    pub(crate) fn is_starting(&self, identity: &ProcessIdentity) -> bool {
        let state = self.state.lock().unwrap();
        state
            .targets
            .get(identity)
            .is_some_and(|(_, status)| status == "Waiting")
            && !state.collected_at.contains_key(identity)
    }
    pub(crate) fn enable_target(&self, identity: ProcessIdentity) -> bool {
        if self.stopping() {
            return false;
        }
        let mut state = self.state.lock().unwrap();
        if identity.start_time.is_none()
            || self.pending_stop.load(Ordering::Acquire)
            || state.pending_removal.contains(&identity)
            || state
                .draining
                .get(&identity)
                .is_some_and(|weak| weak.strong_count() > 0)
        {
            return false;
        }
        if state.targets.contains_key(&identity) {
            return true;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.enabled = true;
        state.collected_at.remove(&identity);
        state.reported_errors.remove(&identity);
        state.targets.insert(
            identity,
            (Arc::new(AtomicBool::new(false)), "Waiting".into()),
        );
        true
    }
    pub(crate) fn disable_target(&self, identity: &ProcessIdentity) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some((cancelled, _)) = state.targets.remove(identity) else {
            return false;
        };
        cancelled.store(true, Ordering::Release);
        state
            .draining
            .insert(identity.clone(), Arc::downgrade(&cancelled));
        state.pending_removal.insert(identity.clone());
        if state.targets.is_empty() {
            state.enabled = false;
        }
        true
    }
    pub(crate) fn finish_remove(&self, identities: &[ProcessIdentity]) {
        let mut state = self.state.lock().unwrap();
        for identity in identities {
            state.pending_removal.remove(identity);
        }
        state.draining.retain(|_, weak| weak.strong_count() > 0);
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub(crate) fn accepts(&self, identity: &ProcessIdentity, generation: Option<u64>) -> bool {
        generation.is_none() || (self.allows(identity) && generation == Some(self.generation()))
    }
    pub(crate) fn enabled(&self) -> bool {
        self.state.lock().unwrap().enabled
    }
    pub(crate) fn stopping(&self) -> bool {
        !self.enabled()
            && (self.pending_stop.load(Ordering::Acquire)
                || self.active.load(Ordering::Acquire) > 0)
    }
    pub(crate) fn enable(&self, identities: Vec<ProcessIdentity>) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.enabled
            || self.pending_stop.load(Ordering::Acquire)
            || self.active.load(Ordering::Acquire) > 0
        {
            return false;
        }
        if !identities.is_empty() && identities.iter().all(|id| id.start_time.is_none()) {
            return false;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.targets = identities
            .into_iter()
            .filter(|id| id.start_time.is_some())
            .map(|id| (id, (Arc::new(AtomicBool::new(false)), "Waiting".into())))
            .collect();
        state.collected_at.clear();
        state.reported_errors.clear();
        state.enabled = true;
        true
    }
    pub(crate) fn finish_suspend(&self) {
        self.pending_stop.store(false, Ordering::Release);
    }
    // Empty authorization has no sessions to drain, so off/on stays immediate.
    pub(crate) fn disable(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let needs_cleanup = !state.targets.is_empty()
            || self.active.load(Ordering::Acquire) > 0
            || self.pending_stop.load(Ordering::Acquire);
        self.pending_stop.store(needs_cleanup, Ordering::Release);
        state.enabled = false;
        for (cancelled, _) in state.targets.values() {
            cancelled.store(true, Ordering::Release);
        }
        state.targets.clear();
        needs_cleanup
    }
    pub(crate) fn retain(&self, live: &HashSet<ProcessIdentity>) {
        let mut state = self.state.lock().unwrap();
        state.managed.retain(|id, _| live.contains(id));
        state.collected_at.retain(|id, _| live.contains(id));
        state.reported_errors.retain(|id| live.contains(id));
        let had_targets = !state.targets.is_empty();
        let removed = state
            .targets
            .keys()
            .filter(|id| !live.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for identity in removed {
            if let Some((cancelled, _)) = state.targets.remove(&identity) {
                cancelled.store(true, Ordering::Release);
                state.draining.insert(identity, Arc::downgrade(&cancelled));
            }
        }
        state.draining.retain(|_, weak| weak.strong_count() > 0);
        if had_targets && state.targets.is_empty() {
            state.enabled = false;
        }
    }
    pub(crate) fn allows(&self, id: &ProcessIdentity) -> bool {
        self.state.lock().unwrap().targets.contains_key(id)
    }
    pub(crate) fn permit(&self, id: &ProcessIdentity) -> Option<Permit> {
        let state = self.state.lock().unwrap();
        let (cancelled, _) = state.targets.get(id)?;
        self.active.fetch_add(1, Ordering::AcqRel);
        Some(Permit {
            cancelled: cancelled.clone(),
            active: self.active.clone(),
        })
    }
    pub(crate) fn report(&self, id: &ProcessIdentity, status: String) {
        let mut state = self.state.lock().unwrap();
        if state.targets.contains_key(id) {
            // A late sampling pass must not hide a terminal reader failure.
            if matches!(status.as_str(), "Waiting" | "Collecting")
                && state
                    .targets
                    .get(id)
                    .is_some_and(|(_, current)| current.starts_with("Unavailable:"))
            {
                return;
            }
            if status == "Collecting" {
                state.collected_at.insert(id.clone(), Instant::now());
            } else if status != "Waiting" {
                state.collected_at.remove(id);
            }
            if let Some((_, value)) = state.targets.get_mut(id) {
                *value = status;
            }
        }
    }
    pub(crate) fn is_collecting(&self, id: &ProcessIdentity) -> bool {
        let state = self.state.lock().unwrap();
        state
            .targets
            .get(id)
            .is_some_and(|(_, status)| status == "Collecting")
            && state
                .collected_at
                .get(id)
                .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
    }
    pub(crate) fn take_failure_notices(&self) -> Vec<(ProcessIdentity, String)> {
        let mut state = self.state.lock().unwrap();
        let failures = state
            .targets
            .iter()
            .filter_map(|(id, (_, status))| {
                let reason = status.strip_prefix("Unavailable: ")?;
                (!state.reported_errors.contains(id)).then(|| (id.clone(), reason.to_string()))
            })
            .collect::<Vec<_>>();
        state
            .reported_errors
            .extend(failures.iter().map(|(id, _)| id.clone()));
        failures
    }

    pub(crate) fn status(&self, id: &ProcessIdentity) -> String {
        let state = self.state.lock().unwrap();
        if !state.enabled {
            "Disabled".into()
        } else {
            state
                .targets
                .get(id)
                .map(|(_, status)| status.clone())
                .unwrap_or_else(|| "Not enabled".into())
        }
    }
}

// Remove late EventPipe measurements after permission has been revoked. Keep PDH
// values in normal collection; callers use this only for an already queued frame.
pub(crate) fn clear_runtime_values(row: &mut ProcessRow) {
    if row.dotnet_eventpipe.is_none() {
        return;
    }
    row.dotnet_eventpipe = None;
    row.dotnet_heap_bytes = None;
    row.dotnet_gc_gen0_heap_bytes = None;
    row.dotnet_gc_gen1_heap_bytes = None;
    row.dotnet_gc_gen2_heap_bytes = None;
    row.dotnet_gc_loh_bytes = None;
    row.dotnet_gc_poh_bytes = None;
    row.dotnet_gc_committed_bytes = None;
    row.dotnet_gc_fragmentation_bytes = None;
    row.dotnet_allocation_bytes_per_sec = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(start: Option<u64>) -> ProcessIdentity {
        ProcessIdentity {
            pid: 42,
            name: "app.exe".into(),
            start_time: start,
        }
    }
    #[test]
    fn per_target_stop_does_not_stop_other_targets_or_reenable_a_draining_reader() {
        let control = DotNetControl::default();
        let first = id(Some(1));
        let second = ProcessIdentity {
            pid: 43,
            ..id(Some(2))
        };
        assert!(control.enable_target(first.clone()));
        let first_permit = control.permit(&first).unwrap();
        assert!(control.enable_target(second.clone()));
        let second_permit = control.permit(&second).unwrap();
        let generation = control.generation();
        assert!(control.disable_target(&first));
        assert!(first_permit.cancelled());
        assert!(!second_permit.cancelled());
        assert!(control.allows(&second));
        assert!(!control.enable_target(first.clone()));
        control.finish_remove(std::slice::from_ref(&first));
        assert!(!control.enable_target(first.clone()));
        drop(first_permit);
        assert!(control.enable_target(first.clone()));
        assert!(!control.accepts(&first, Some(generation)));
        assert!(control.allows(&second));
    }

    #[test]
    fn per_target_enable_waits_for_bulk_stop_readers() {
        let control = DotNetControl::default();
        let identity = id(Some(1));
        assert!(control.enable_target(identity.clone()));
        let permit = control.permit(&identity).unwrap();
        control.disable();
        control.finish_suspend();
        assert!(!control.enable_target(identity.clone()));
        drop(permit);
        assert!(control.enable_target(identity));
    }

    #[test]
    fn collecting_marker_expires_without_fresh_reports() {
        let control = DotNetControl::default();
        let identity = id(Some(1));
        assert!(control.enable_target(identity.clone()));
        assert!(!control.is_collecting(&identity));
        control.report(&identity, "Collecting".into());
        assert!(control.is_collecting(&identity));
        control
            .state
            .lock()
            .unwrap()
            .collected_at
            .insert(identity.clone(), Instant::now() - Duration::from_secs(4));
        assert!(!control.is_collecting(&identity));
    }

    #[test]
    fn starting_feedback_is_immediate_but_not_reused_for_stale_or_failed_sessions() {
        let control = DotNetControl::default();
        let identity = id(Some(1));
        assert!(control.enable_target(identity.clone()));
        assert!(control.is_starting(&identity));
        control.report(&identity, "Collecting".into());
        assert!(!control.is_starting(&identity));
        control.report(&identity, "Waiting".into());
        assert!(!control.is_starting(&identity));
        assert!(!control.is_collecting(&identity));
        control.report(&identity, "Unavailable: pipe closed".into());
        control.report(&identity, "Waiting".into());
        control.report(&identity, "Collecting".into());
        assert_eq!(control.status(&identity), "Unavailable: pipe closed");
        assert!(!control.is_starting(&identity));
        assert!(!control.is_collecting(&identity));
        assert!(control.disable_target(&identity));
        control.finish_remove(std::slice::from_ref(&identity));
        assert!(control.enable_target(identity.clone()));
        assert!(control.is_starting(&identity));
    }

    #[test]
    fn permission_never_follows_restarts_and_drains_before_reenable() {
        let control = DotNetControl::default();
        assert!(!control.enable(vec![id(None)]));
        assert!(control.enable(vec![id(Some(1))]));
        let permit = control.permit(&id(Some(1))).unwrap();
        assert!(control.permit(&id(Some(2))).is_none());
        control.disable();
        assert!(permit.cancelled());
        assert!(control.stopping());
        assert!(!control.enable(vec![id(Some(2))]));
        control.finish_suspend();
        assert!(control.stopping());
        drop(permit);
        assert!(!control.stopping());
        assert!(control.enable(vec![id(Some(2))]));
        assert!(!control.allows(&id(Some(1))));
        control.retain(&HashSet::new());
        assert!(!control.enabled());
    }
}
