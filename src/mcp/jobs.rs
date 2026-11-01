//! Bounded native collection jobs. Timed-out native calls retain their slot until cleanup.
use super::transport::error;
use crate::{
    model::{InfoValue, ProcessIdentity, ProcessRow},
    samplers,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) struct Job {
    pub(crate) started: Instant,
    pub(crate) live: bool,
    pub(crate) completed: Option<Instant>,
    pub(crate) outcome: Option<Value>,
    receiver: Receiver<Value>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Job {
    pub(crate) fn start(
        kind: String,
        process: Option<ProcessRow>,
        fields: Option<Vec<String>>,
    ) -> Result<Self, Value> {
        Self::custom(move |cancelled| {
            let started = chrono::Utc::now();
            match collect(&kind, process.as_ref(), &cancelled) {
                Ok(mut value) => {
                    if kind == "process_info"
                        && let Some(fields) = &fields
                        && let Some(rows) = value["rows"].as_array_mut()
                    {
                        for row in rows {
                            row.as_object_mut()
                                .unwrap()
                                .retain(|key, _| fields.contains(key));
                        }
                    }
                    if kind == "process_info" {
                        fn unavailable(v: &Value) -> bool {
                            match v {
                                Value::Object(o) => {
                                    o.contains_key("unavailable") || o.values().any(unavailable)
                                }
                                Value::Array(a) => a.iter().any(unavailable),
                                _ => false,
                            }
                        }
                        let partial = unavailable(&value["rows"]);
                        value["status"] = json!(if partial { "partial" } else { "complete" });
                        value["warnings"] = json!(if partial {
                            vec!["Some requested fields are unavailable"]
                        } else {
                            vec![]
                        });
                    }
                    value["capture_started_at"] = json!(started.to_rfc3339());
                    value["capture_finished_at"] = json!(chrono::Utc::now().to_rfc3339());
                    value
                }
                Err(value) => value,
            }
        })
    }
    pub(crate) fn custom(
        work: impl FnOnce(Arc<AtomicBool>) -> Value + Send + 'static,
    ) -> Result<Self, Value> {
        ACTIVE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 8).then_some(n + 1)
            })
            .map_err(|_| error("BUSY", "Workers are at capacity", true))?;
        let slot = Slot;
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        thread::Builder::new()
            .name("mcp-worker".into())
            .spawn(move || {
                let _slot = slot;
                let result = work(cancelled.clone());
                if !cancelled.load(Ordering::Acquire) {
                    let _ = sender.send(result);
                }
            })
            .map_err(|_| error("BUSY", "Cannot start worker", true))?;
        Ok(Self {
            live: true,
            started: Instant::now(),
            completed: None,
            outcome: None,
            receiver,
            cancel,
        })
    }
    pub(crate) fn poll(&mut self) {
        if self.completed.is_some() {
            return;
        }
        if let Ok(result) = self.receiver.try_recv() {
            self.outcome = Some(result);
            self.completed = Some(Instant::now());
        } else if self.started.elapsed() >= Duration::from_secs(60) {
            self.cancel();
            self.outcome = Some(error(
                "TIMEOUT",
                "Collection deadline reached; native cleanup may still be running",
                true,
            ));
        }
    }
    pub(crate) fn cancel(&mut self) {
        if self.completed.is_none() {
            self.cancel.store(true, Ordering::Release);
            self.completed = Some(Instant::now());
            self.outcome = Some(error(
                "CANCELLED",
                "Collection cancelled; native cleanup may still be running",
                false,
            ));
        }
    }
}
fn field(value: &InfoValue) -> Value {
    match value {
        InfoValue::Value(value) => json!({"value":value}),
        InfoValue::AccessDenied => json!({"unavailable":"access_denied"}),
        InfoValue::NotAvailable => json!({"unavailable":"unsupported"}),
        _ => json!({"unavailable":"unknown"}),
    }
}
fn collect(kind: &str, process: Option<&ProcessRow>, cancel: &AtomicBool) -> Result<Value, Value> {
    let identity = process.map(ProcessIdentity::from_row);
    let held = identity
        .as_ref()
        .map(samplers::verified_process::VerifiedProcess::open)
        .transpose()
        .map_err(|code| error(code, "Cannot verify requested process lifetime", false))?;
    let result = match kind {
        "process_info" => {
            let info = samplers::process_info::collect_process_info(
                process.unwrap(),
                crate::app::ProcessLifecycle::Live,
            );
            json!({"rows":[{"image":field(&info.executable),"command_line":field(&info.command_line),"user":field(&info.user),"version":{"company":field(&info.company_name),"product":field(&info.product_name),"product_version":field(&info.product_version),"file_version":field(&info.file_version)},"signature":{"unavailable":"unsupported"}}],"status":"partial","warnings":["Signature verification is unavailable"],"coverage":{}})
        }

        "modules" => {
            let report = samplers::process_modules::collect_process_modules(
                identity.as_ref().unwrap(),
                process.unwrap(),
            )
            .map_err(|_| error("COLLECTION_FAILED", "Module enumeration failed", false))?;
            let rows:Vec<_>=report.entries.iter().map(|entry|json!({"path":entry.path,"name":entry.dll_name,"directory":entry.directory,"company":field(&entry.company_name),"product_version":field(&entry.product_version),"file_version":field(&entry.file_version),"modified":field(&entry.modified)})).collect();
            json!({"rows":rows,"status":"complete","coverage":{}})
        }
        "threads" => {
            let report = samplers::threads::collect(identity.as_ref().unwrap())
                .map_err(|_| error("COLLECTION_FAILED", "Thread enumeration failed", false))?;
            let rows: Vec<_> = report
                .rows
                .iter()
                .map(|row| {
                    let mut value = json!({"tid":row.tid,"base_priority":row.base_priority});
                    if let Some(name) = &row.name {
                        value["name"] = json!(name);
                    }
                    if let Some(cpu) = row.cpu_seconds.filter(|v| v.is_finite()) {
                        value["cpu_seconds"] = json!(cpu);
                    }
                    if let Some((group, cpu)) = row.ideal_cpu {
                        value["ideal_cpu"] = json!({"group":group,"index":cpu});
                    }
                    value
                })
                .collect();
            json!({"rows":rows,"status":if report.unavailable>0{"partial"}else{"complete"},"coverage":{"unavailable_threads":report.unavailable}})
        }
        "network" => {
            let report = samplers::network::collect_endpoints(identity.as_ref())
                .map_err(|_| error("COLLECTION_FAILED", "Endpoint enumeration failed", false))?;
            let rows:Vec<_>=report.endpoints.iter().map(|entry|json!({"protocol":entry.key.protocol.label(),"local":entry.key.local.to_string(),"remote":entry.key.remote.map(|remote|remote.to_string()),"pid":entry.key.pid,"owner_name":entry.owner.as_ref().map(|owner|owner.identity.name.clone()),"state":entry.state_label()})).collect();
            json!({"rows":rows,"status":if report.failures.is_empty(){"complete"}else{"partial"},"coverage":{"successful_tables":report.successful_tables,"failed_tables":report.failures.len()}})
        }
        "open_files" => {
            let report = samplers::open_files::collect_open_files_for_process(process.unwrap());
            let rows:Vec<_>=report.entries.iter().map(|entry|{let mut value=json!({"path":entry.path,"handle":entry.handle.value.to_string(),"kind":entry.handle.kind_label()});if let Ok(Some(size))=entry.handle.metadata.size{value["size_bytes"]=json!(size.to_string());}value}).collect();
            json!({"rows":rows,"status":if report.error.is_some()||report.inaccessible_handles>0||report.unnamed_file_handles>0{"partial"}else{"complete"},"coverage":{"total_handles":report.total_handles,"file_handles":report.file_handles,"inaccessible_handles":report.inaccessible_handles,"unnamed_file_handles":report.unnamed_file_handles,"query_failed":report.error.is_some()}})
        }
        "scheduling" => {
            let worker = samplers::scheduling::SchedulingWorker::spawn();
            worker
                .request(samplers::scheduling::SchedulingRequest {
                    generation: 1,
                    identity: identity.as_ref().unwrap().clone(),
                    change: None,
                    affinity_change: None,
                })
                .map_err(|_| error("BUSY", "Scheduling worker unavailable", true))?;
            loop {
                if cancel.load(Ordering::Acquire) {
                    worker.cancel();
                    return Err(error("CANCELLED", "Scheduling capture cancelled", false));
                }
                if let Ok(report) = worker.try_recv() {
                    let mut row = json!({});
                    if let Some(priority) = report.report.current {
                        row["priority_class"] = json!(priority.label());
                    }
                    if let Some(affinity) = report.affinity {
                        if let Some(mask) = affinity.current {
                            row["affinity_mask"] = json!(mask.to_string());
                        }
                        row["allowed_affinity_mask"] = json!(affinity.allowed.to_string());
                    }
                    if let Some(qos) = report.qos {
                        row["ecoqos"] = json!(format!("{qos:?}"));
                    }
                    break json!({"rows":[row],"status":if report.report.current.is_some(){"complete"}else{"partial"},"coverage":{}});
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        "file_users" => {
            use crate::model::file_users::{FileSearchEnd, FileUsersRequest};
            let worker = samplers::file_users::FileUsersWorker::spawn();
            worker
                .request(1, FileUsersRequest::CaptureAll)
                .map_err(|_| error("BUSY", "File scan unavailable", true))?;
            loop {
                if cancel.load(Ordering::Acquire) {
                    worker.cancel();
                    return Err(error("CANCELLED", "File scan cancelled", false));
                }
                if let Some(update) = worker.take_update()
                    && let Some(end) = update.report.end
                {
                    let rows:Vec<_>=update.report.matches.iter().map(|entry|json!({"path":entry.path,"pid":entry.owner.pid,"name":entry.owner.name,"executable_path":entry.owner.executable_path,"creation_time":entry.owner.creation_time.to_string(),"_identity":{"pid":entry.owner.pid,"name":entry.owner.name,"start_time":entry.owner.identity().start_time},"handle_count":entry.handle_count.to_string()})).collect();
                    break json!({"rows":rows,"status":if matches!(end,FileSearchEnd::Complete)&&!update.report.progress.incomplete(){"complete"}else{"partial"},"coverage":update.report.progress,"end":end.label()});
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        _ => {
            return Err(error(
                "NOT_AVAILABLE",
                "Collector is not connected yet",
                false,
            ));
        }
    };
    if let Some(held) = held {
        held.verify().map_err(|code| {
            error(
                code,
                "Process changed during capture; result discarded",
                false,
            )
        })?;
    }
    if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 64 * 1024 * 1024) {
        return Err(error(
            "RESOURCE_LIMIT",
            "Capture exceeds memory budget",
            false,
        ));
    }
    Ok(result)
}
