//! Read adapter over immutable application-owned samples. No Windows calls on the UI thread.
use super::{
    catalog, metrics,
    transport::{Event, Server, error},
};
use crate::model::{ProcessHistory, ProcessIdentity, Snapshot, SystemHistory};
use chrono::{Local, Utc};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct View {
    pub(crate) snapshot: Snapshot,
    pub(crate) processes: ProcessHistory,
    pub(crate) system: SystemHistory,
    pub(crate) activity: String,
    pub(crate) tracked_names: Vec<String>,
    pub(crate) host: Value,
    pub(crate) control: Value,
    pub(crate) log_root: Option<std::path::PathBuf>,
}
struct Capture {
    signature: Value,
    rows: Vec<Value>,
    created: Instant,
    metadata: Value,
}
struct Loaded {
    view: View,
    session: Box<Session>,
    created: Instant,
    bytes: usize,
    partial: bool,
    interval: u64,
}
#[derive(Default)]
struct Session {
    logs: HashMap<String, Loaded>,
    loading: HashMap<String, (String, std::sync::mpsc::Receiver<Loaded>)>,
    log_files: HashMap<String, std::path::PathBuf>,
    refs: HashMap<String, ProcessIdentity>,
    jobs: HashMap<String, super::jobs::Job>,
    captures: HashMap<String, Capture>,
}
impl Session {
    fn collect_jobs(&mut self) {
        self.logs
            .retain(|_, l| l.created.elapsed() < Duration::from_secs(300));
        self.loading.retain(|_, (job_id, _)| {
            self.jobs
                .get(job_id)
                .is_some_and(|j| j.outcome.as_ref().is_none_or(|r| r.get("error").is_none()))
        });
        let mut ready = Vec::new();
        for (id, (job_id, receiver)) in &self.loading {
            if let Ok(log) = receiver.try_recv() {
                ready.push((id.clone(), job_id.clone(), log));
            }
        }
        for (id, job_id, log) in ready {
            self.loading.remove(&id);
            if self
                .jobs
                .get(&job_id)
                .is_some_and(|j| j.outcome.as_ref().is_some_and(|r| r.get("error").is_some()))
            {
                continue;
            }
            if self.capture_count() >= 8
                || self.retained_bytes().saturating_add(log.bytes) > 64 * 1024 * 1024
            {
                if let Some(job) = self.jobs.get_mut(&job_id) {
                    job.cancel();
                    job.outcome = Some(error(
                        "RESULT_TOO_LARGE",
                        "Loaded log exceeds remaining result capacity",
                        false,
                    ));
                }
            } else {
                self.logs.insert(id, log);
            }
        }
        let log_bytes: usize = self
            .logs
            .values()
            .map(|l| l.bytes + l.session.retained_bytes())
            .sum();
        let log_count = self
            .logs
            .values()
            .map(|l| 1 + l.session.capture_count())
            .sum::<usize>()
            + self.loading.len();
        self.captures
            .retain(|_, capture| capture.created.elapsed() < Duration::from_secs(300));
        self.jobs.retain(|_, job| {
            job.completed
                .is_none_or(|time| time.elapsed() < Duration::from_secs(300))
        });
        for job in self.jobs.values_mut() {
            job.poll();
            if let Some(result) = job.outcome.as_mut()
                && let Some(rows) = result
                    .as_object_mut()
                    .and_then(|result| result.remove("rows"))
            {
                if self.captures.len() + log_count >= 8 {
                    *result = error("RESOURCE_LIMIT", "Capture limit reached", true);
                    continue;
                }
                let existing: usize = self
                    .captures
                    .values()
                    .map(|capture| {
                        serde_json::to_vec(&capture.rows)
                            .map_or(64 * 1024 * 1024, |bytes| bytes.len())
                    })
                    .sum();
                if serde_json::to_vec(&rows).map_or(true, |bytes| {
                    bytes
                        .len()
                        .saturating_add(existing)
                        .saturating_add(log_bytes)
                        > 64 * 1024 * 1024
                }) {
                    *result = error("RESOURCE_LIMIT", "Capture memory budget exceeded", false);
                    continue;
                }
                let id = uuid::Uuid::new_v4().to_string();
                let mut rows = rows;
                for row in rows.as_array_mut().unwrap() {
                    if let Some(identity) = row.as_object_mut().and_then(|r| r.remove("_identity"))
                        && let (Some(pid), Some(name), Some(start)) = (
                            identity["pid"].as_u64(),
                            identity["name"].as_str(),
                            identity["start_time"].as_u64(),
                        )
                    {
                        let id = ProcessIdentity {
                            pid: pid as u32,
                            name: name.into(),
                            start_time: Some(start),
                        };
                        let reference = self
                            .refs
                            .iter()
                            .find(|(_, v)| **v == id)
                            .map(|(r, _)| r.clone())
                            .unwrap_or_else(|| {
                                let r = uuid::Uuid::new_v4().to_string();
                                self.refs.insert(r.clone(), id);
                                r
                            });
                        row["process_ref"] = json!(reference);
                    }
                }
                if result["kind"] == "log_list" {
                    for row in rows.as_array_mut().unwrap() {
                        if let Some(path) = row
                            .as_object_mut()
                            .unwrap()
                            .remove("_path")
                            .and_then(|p| p.as_str().map(std::path::PathBuf::from))
                        {
                            self.log_files
                                .insert(row["log_id"].as_str().unwrap().into(), path);
                        }
                    }
                    if self.log_files.len() > 5000 {
                        self.log_files.clear();
                        *result = error("BUSY", "Log ID capacity reached; reconnect", false);
                        continue;
                    }
                }
                self.captures.insert(
                    id.clone(),
                    Capture {
                        signature: json!({}),
                        rows: rows.as_array().unwrap().clone(),
                        created: Instant::now(),
                        metadata: result.clone(),
                    },
                );
                result["capture_id"] = json!(id);
            }
        }
    }
    fn capture_count(&self) -> usize {
        self.captures.len()
            + self.logs.len()
            + self
                .logs
                .values()
                .map(|l| l.session.capture_count())
                .sum::<usize>()
            + self.loading.len()
    }
    fn retained_bytes(&self) -> usize {
        self.captures
            .values()
            .map(|c| serde_json::to_vec(&c.rows).map_or(64 * 1024 * 1024, |b| b.len()))
            .sum::<usize>()
            + self
                .logs
                .values()
                .map(|l| l.bytes + l.session.retained_bytes())
                .sum::<usize>()
    }
    fn reference(&mut self, identity: &ProcessIdentity) -> String {
        if let Some((reference, _)) = self.refs.iter().find(|(_, value)| *value == identity) {
            return reference.clone();
        }
        let reference = uuid::Uuid::new_v4().to_string();
        self.refs.insert(reference.clone(), identity.clone());
        reference
    }
    fn page(&mut self, args: &Value, rows: Option<Vec<Value>>) -> Value {
        self.page_with_context(args, rows, json!({}), Instant::now())
    }
    fn page_with_context(
        &mut self,
        args: &Value,
        rows: Option<Vec<Value>>,
        metadata: Value,
        created: Instant,
    ) -> Value {
        self.captures
            .retain(|_, capture| capture.created.elapsed() < Duration::from_secs(300));
        let mut signature = args.clone();
        signature
            .as_object_mut()
            .expect("validated arguments")
            .remove("cursor");
        let (id, offset) = if let Some(cursor) = args["cursor"].as_str() {
            let Some((id, offset)) = cursor.split_once(':') else {
                return error("CURSOR_INVALID", "Malformed cursor", false);
            };
            let Ok(offset) = offset.parse::<usize>() else {
                return error("CURSOR_INVALID", "Malformed cursor", false);
            };
            (id.to_owned(), offset)
        } else {
            if self.capture_count() >= 8 {
                return error(
                    "RESOURCE_LIMIT",
                    "Release a capture before creating another",
                    true,
                );
            }
            let rows = rows.unwrap_or_default();
            let bytes = serde_json::to_vec(&rows).map_or(usize::MAX, |bytes| bytes.len());
            let existing = self.retained_bytes();
            if bytes.saturating_add(existing) > 64 * 1024 * 1024 {
                return error("RESOURCE_LIMIT", "Capture memory budget exceeded", false);
            }
            let id = uuid::Uuid::new_v4().to_string();
            self.captures.insert(
                id.clone(),
                Capture {
                    signature: signature.clone(),
                    rows,
                    created,
                    metadata,
                },
            );
            (id, 0)
        };
        let Some(capture) = self.captures.get(&id) else {
            return error(
                "CURSOR_INVALID",
                "Capture expired or belongs to another connection",
                false,
            );
        };
        if capture.signature != signature || offset > capture.rows.len() {
            return error("CURSOR_INVALID", "Cursor arguments changed", false);
        }
        render_page(
            capture,
            &id,
            offset,
            args["limit"].as_u64().unwrap_or(100) as usize,
        )
    }
}

fn render_page(capture: &Capture, id: &str, offset: usize, limit: usize) -> Value {
    if offset > capture.rows.len() {
        return error("CURSOR_INVALID", "Cursor offset is out of range", false);
    }
    let mut end = (offset + limit).min(capture.rows.len());
    // Reserve both structured JSON and escaped text JSON plus envelope overhead.
    while end > offset
        && serde_json::to_vec(&capture.rows[offset..end]).map_or(true, |b| {
            b.len().saturating_mul(3).saturating_add(16384) > super::protocol::MAX_FRAME_BYTES
        })
    {
        if end == offset + 1 {
            return error(
                "RESULT_TOO_LARGE",
                "One row exceeds the encoded response budget; request fewer fields",
                false,
            );
        }
        end = offset + (end - offset) / 2;
    }
    let next = (end < capture.rows.len()).then(|| format!("{id}:{end}"));
    let mut coverage = capture
        .metadata
        .get("coverage")
        .cloned()
        .unwrap_or_else(|| json!({}));
    coverage["listed"] = json!(capture.rows.len());
    let mut page = json!({"capture_id":id,"rows":capture.rows[offset..end],"next_cursor":next,"coverage":coverage});
    if let Some(temporal) = capture.metadata.get("temporal_coverage") {
        page["temporal_coverage"] = temporal.clone();
    }
    page
}

#[derive(Default)]
pub(crate) struct ConnectionStatus {
    pub(crate) label: String,
    clients: HashSet<String>,
}
impl ConnectionStatus {
    pub(crate) fn connected(&self) -> bool {
        !self.clients.is_empty()
    }
    fn update(&mut self, event: &Event) {
        match event {
            Event::Ready(_) => {
                self.clients.clear();
                self.label = "Listening".into();
            }
            Event::Failed(message) => {
                self.clients.clear();
                self.label = message.clone();
            }
            Event::Connected(id) => {
                self.clients.insert(id.clone());
                self.refresh_label();
            }
            Event::Disconnected(id) => {
                // A rejected handshake never became an authenticated client.
                if self.clients.remove(id) {
                    self.refresh_label();
                }
            }
        }
    }
    fn refresh_label(&mut self) {
        self.label = if self.clients.is_empty() {
            "Listening".into()
        } else {
            format!("Listening ({} clients)", self.clients.len())
        };
    }
}

pub(crate) struct Backend {
    pub(crate) view: Arc<RwLock<Arc<View>>>,
    pub(crate) status: Arc<Mutex<ConnectionStatus>>,
    stop: Arc<AtomicBool>,
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Backend {
    pub(crate) fn start(view: View, operations: super::operations::Shared) -> io::Result<Self> {
        let server = Server::start()?;
        let view = Arc::new(RwLock::new(Arc::new(view)));
        let status = Arc::new(Mutex::new(ConnectionStatus {
            label: "Starting".into(),
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_view, thread_status, thread_stop) =
            (view.clone(), status.clone(), stop.clone());
        thread::Builder::new()
            .name("mcp-queries".into())
            .spawn(move || {
                let mut previous_activity = String::new();
                let mut instance = String::new();
                let mut sessions = HashMap::<String, Session>::new();
                while !thread_stop.load(Ordering::Acquire) {
                    let activity = thread_view.read().expect("view lock").activity.clone();
                    if activity != previous_activity {
                        if activity == "log_view" {
                            for session in sessions.values_mut() {
                                session
                                    .captures
                                    .retain(|_, c| c.metadata["source"] == "log");
                                for job in session.jobs.values_mut() {
                                    if job.live {
                                        job.cancel();
                                    }
                                }
                            }
                        }
                        previous_activity = activity;
                    }
                    for session in sessions.values_mut() {
                        session.collect_jobs();
                    }
                    while let Ok(event) = server.events.try_recv() {
                        thread_status.lock().expect("status lock").update(&event);
                        match event {
                            Event::Ready(id) => instance = id,
                            Event::Disconnected(connection) => {
                                operations
                                    .lock()
                                    .unwrap()
                                    .cancel_connection(Some(&connection));
                                sessions.remove(&connection);
                            }
                            Event::Failed(_) | Event::Connected(_) => {}
                        }
                    }
                    match server.requests.recv_timeout(Duration::from_millis(20)) {
                        Ok(request) => {
                            if request.reply.is_closed() {
                                continue;
                            }
                            let view = thread_view.read().expect("view lock").clone();
                            let session = sessions.entry(request.connection.clone()).or_default();
                            let result = if catalog::validate(&request.method, &request.arguments)
                                != Some(true)
                            {
                                error("INVALID_ARGUMENT", "Invalid tool arguments", false)
                            } else if request.method == "winproc_wait_condition" {
                                start_wait(session, thread_view.clone(), &request.arguments)
                            } else if request.method == "winproc_get_operation" {
                                operations
                                    .lock()
                                    .unwrap()
                                    .get(request.arguments["operation_id"].as_str().unwrap_or(""))
                            } else if super::operations::is_action(&request.method) {
                                let previous = operations
                                    .lock()
                                    .unwrap()
                                    .get(request.arguments["operation_id"].as_str().unwrap());
                                let reference = request.arguments["process_ref"].as_str();
                                let identity = reference.and_then(|r| session.refs.get(r)).cloned();
                                if previous.get("error").is_none() {
                                    operations.lock().unwrap().submit(
                                        &request.connection,
                                        &request.method,
                                        &request.arguments,
                                        None,
                                    )
                                } else if reference.is_some() && identity.is_none() {
                                    error(
                                        "REFERENCE_INVALID",
                                        "Refresh live process references",
                                        false,
                                    )
                                } else {
                                    operations.lock().unwrap().submit(
                                        &request.connection,
                                        &request.method,
                                        &request.arguments,
                                        identity,
                                    )
                                }
                            } else {
                                dispatch(session, &view, &request.method, &request.arguments)
                            };
                            let result = if result.get("api_version").is_some() {
                                let mut result = result;
                                result["backend_instance_id"] = json!(instance);
                                result
                            } else {
                                envelope(session, &view, &instance, result)
                            };
                            let _ = request.reply.send(result);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self { view, status, stop })
    }
}

fn envelope(session: &Session, view: &View, instance: &str, data: Value) -> Value {
    if data.get("error").is_some() {
        return data;
    }
    let metadata = data["capture_id"]
        .as_str()
        .and_then(|id| session.captures.get(id))
        .map(|capture| &capture.metadata);
    let mut result = json!({"api_version":"1.0","backend_instance_id":instance,"source":"live",
        "captured_at":view.snapshot.captured_at.with_timezone(&Utc).to_rfc3339(),"status":"complete","warnings":[]});
    if let Some(metadata) = metadata {
        if metadata.get("capture_started_at").is_some() {
            result.as_object_mut().unwrap().remove("captured_at");
        }
        for field in [
            "source",
            "captured_at",
            "capture_started_at",
            "capture_finished_at",
            "status",
            "warnings",
        ] {
            if let Some(value) = metadata.get(field) {
                result[field] = value.clone();
            }
        }
    }
    result["data"] = data;
    result
}

fn dispatch(session: &mut Session, view: &View, method: &str, args: &Value) -> Value {
    if catalog::validate(method, args) != Some(true) {
        return error("INVALID_ARGUMENT", "Invalid tool or arguments", false);
    }
    if args["source"] == "log" {
        return query_log(session, method, args);
    }
    match method {
        "winproc_list_logs"=>return list_logs(session,view,args),
        "winproc_open_log"=>return open_log(session,view,args),
        "winproc_close_log"=>{session.logs.remove(args["log_ref"].as_str().unwrap());return json!({"closed":true});},
        "winproc_get_status" => return json!({"activity":view.activity,"stale":(Local::now()-view.snapshot.captured_at).num_seconds()>=3,"sample_age_ms":(Local::now()-view.snapshot.captured_at).num_milliseconds().max(0),"version":env!("CARGO_PKG_VERSION"),"transport":"named_pipe","connected":true,"permissions":{"same_logon":true,"same_integrity":true},"capabilities":catalog::available_tools().iter().map(|t|t.name.to_string()).collect::<Vec<_>>(),"limits":{"connections":4,"active_jobs_per_connection":2,"retained_objects_per_connection":8,"result_bytes_per_connection":67108864,"result_ttl_seconds":300,"operation_entries":1024,"operation_ttl_seconds":86400,"pending_operations":1}}),
        "winproc_get_system_info" => return view.host.clone(),
        "winproc_list_metrics" => return metrics::catalog(args["target_kind"].as_str()),
        "winproc_get_tracking" => {
            let matching:Vec<_>=view.snapshot.processes.iter().filter(|p|view.tracked_names.iter().any(|n|n.eq_ignore_ascii_case(&p.name))).map(|p|json!({"pid":p.pid,"name":p.name,"process_ref":session.reference(&ProcessIdentity::from_row(p))})).collect();
            return json!({"tracked_names":view.tracked_names,"revision":view.control["revision"],"matching_processes":matching,"scope":"all current and future matching executable names"});
        },
        "winproc_list_profiles" => return json!({"profiles":view.control["profiles"],"active_profile":view.control["active_profile"],"revision":view.control["revision"]}),
        "winproc_get_recording" => return view.control["recording"].clone(),
        "winproc_get_dotnet_collection" => {
            if view.activity=="log_view" {return error("ACTIVITY_CONFLICT","Live collection is unavailable in Log view",false);}
            let Some(id)=session.refs.get(args["process_ref"].as_str().unwrap()) else {return error("REFERENCE_INVALID","Refresh live process references",false);};
            return view.control["dotnet"].as_array().and_then(|rows|rows.iter().find(|r|r["pid"]==id.pid && r["start_time"]==json!(id.start_time))).cloned().unwrap_or_else(||error("PROCESS_EXITED","Target exited",false));
        },
        "winproc_get_job" => return session.jobs.get(args["job_id"].as_str().unwrap()).map_or_else(||error("REFERENCE_INVALID","Unknown or expired job",false),|job|json!({"job_id":args["job_id"],"state":if job.completed.is_some(){if job.outcome.as_ref().is_some_and(|value|value.get("error").is_some()){"failed"}else{"succeeded"}}else{"running"},"result":job.outcome})),
        "winproc_cancel_job" => { if let Some(job)=session.jobs.get_mut(args["job_id"].as_str().unwrap()){job.cancel();} return json!({"cancel_requested":true}); },
        "winproc_read_capture" => return read_capture(session,args),
        "winproc_release_capture" => { let id=args["capture_id"].as_str().unwrap();session.captures.remove(id);for log in session.logs.values_mut(){log.session.captures.remove(id);}return json!({"released":true}); },
        _ => {},
    }
    if view.activity == "log_view" {
        return error(
            "ACTIVITY_CONFLICT",
            "Live investigation is unavailable in Log view",
            false,
        );
    }
    if args["source"] == "log" {
        return error(
            "REFERENCE_INVALID",
            "Open a saved log before querying it",
            false,
        );
    }
    match method {
        "winproc_start_capture" => start_capture(session, view, args),
        "winproc_get_process_info" => {
            let mut a = args.clone();
            a["kind"] = json!("process_info");
            start_capture(session, view, &a)
        }
        "winproc_list_processes" => list_processes(session, view, args),
        "winproc_get_metrics" => get_metrics(session, view, args),
        "winproc_get_history" => get_history(session, view, args),
        _ => error("INVALID_ARGUMENT", "Unknown adapter operation", false),
    }
}

fn list_processes(session: &mut Session, view: &View, args: &Value) -> Value {
    if args.get("cursor").is_some() {
        return session.page(args, None);
    }
    let scope = args["scope"].as_str().unwrap_or("live");
    let mut identities = std::collections::HashSet::new();
    if scope != "retained" {
        identities.extend(
            view.snapshot
                .processes
                .iter()
                .map(ProcessIdentity::from_row),
        );
    }
    if scope != "live" {
        identities.extend(view.processes.identities().cloned());
    }
    let mut identities: Vec<_> = identities.into_iter().collect();
    identities.sort_by(|a, b| {
        if args["sort"] == "pid" {
            a.pid
                .cmp(&b.pid)
                .then(a.start_time.cmp(&b.start_time))
                .then(a.name.cmp(&b.name))
        } else {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.pid.cmp(&b.pid))
                .then(a.start_time.cmp(&b.start_time))
        }
    });
    // References expire with their retained/live identity; an immutable page never pins history.
    session.refs.retain(|_, identity| {
        view.processes.time_range_for(identity).is_some()
            || view
                .snapshot
                .processes
                .iter()
                .any(|row| ProcessIdentity::from_row(row) == *identity)
    });
    let mut rows = Vec::new();
    for identity in identities {
        let live = view
            .snapshot
            .processes
            .iter()
            .find(|row| ProcessIdentity::from_row(row) == identity);
        let filter = &args["filter"];
        if filter["pid"]
            .as_u64()
            .is_some_and(|pid| pid != u64::from(identity.pid))
        {
            continue;
        }
        if filter["name"]
            .as_str()
            .is_some_and(|name| !identity.name.to_lowercase().contains(&name.to_lowercase()))
        {
            continue;
        }
        let path = live.and_then(|row| row.executable_path.as_deref());
        if filter["path"].as_str().is_some_and(|query| {
            !path.is_some_and(|path| path.to_lowercase().contains(&query.to_lowercase()))
        }) {
            continue;
        }
        let range = view.processes.time_range_for(&identity);
        let mut row = json!({"process_ref":session.reference(&identity),"pid":identity.pid,"name":identity.name,
            "lifecycle":if live.is_some() && view.activity != "saved_log" {"live"} else {"unknown"},
            "last_seen_at":range.map(|range|range.1).unwrap_or(view.snapshot.captured_at).with_timezone(&Utc).to_rfc3339()});
        // Normalize before freezing/projection so every read path sees recorded membership,
        // never a claim that a recorded process is currently alive.
        if view.activity == "saved_log" {
            row["source"] = json!("log");
        }
        row["parent"] = json!({"state":"unknown"});
        if view.activity != "saved_log"
            && let Some(child) = live
            && let (Some(parent_pid), Some(child_start)) = (child.parent_pid, child.start_time)
        {
            let candidates: Vec<_> = view
                .snapshot
                .processes
                .iter()
                .filter(|p| p.pid == parent_pid)
                .collect();
            if candidates.len() == 1
                && candidates[0]
                    .start_time
                    .is_some_and(|start| start < child_start)
            {
                row["parent"] = json!({"state":"verified","process_ref":session.reference(&ProcessIdentity::from_row(candidates[0]))});
            }
        }
        if let Some(path) = path {
            row["path"] = json!(path);
        }
        if let Some(start) = identity.start_time {
            row["start_time"] = json!(start.to_string());
        }
        if let Some((from, to)) = range {
            row["history_from"] = json!(from.with_timezone(&Utc).to_rfc3339());
            row["history_to"] = json!(to.with_timezone(&Utc).to_rfc3339());
        }
        if let Some(fields) = args["projection"].as_array() {
            row.as_object_mut()
                .unwrap()
                .retain(|name, _| fields.iter().any(|field| field.as_str() == Some(name)));
        }
        rows.push(row);
    }
    session.page_with_context(
        args,
        Some(rows),
        json!({"captured_at":view.snapshot.captured_at.with_timezone(&Utc).to_rfc3339()}),
        Instant::now(),
    )
}

fn get_metrics(session: &Session, view: &View, args: &Value) -> Value {
    let ids = args["metric_ids"].as_array().unwrap();
    let mut samples = Vec::new();
    for target in args["targets"].as_array().unwrap() {
        let mut values = serde_json::Map::new();
        let mut unavailable = serde_json::Map::new();
        let process = if let Some(reference) = target["process_ref"].as_str() {
            let Some(identity) = session.refs.get(reference) else {
                return error("REFERENCE_INVALID", "Refresh the process list", false);
            };
            let Some(row) = view
                .snapshot
                .processes
                .iter()
                .find(|row| ProcessIdentity::from_row(row) == *identity)
            else {
                return error(
                    "PROCESS_EXITED",
                    "Target is absent from the latest sample; query retained history",
                    false,
                );
            };
            Some(crate::model::ProcessSample::from_row(
                view.snapshot.captured_at,
                row,
            ))
        } else {
            None
        };
        let system = crate::model::history::SystemSample::from_snapshot(&view.snapshot);
        let allowed = if process.is_some() {
            metrics::PROCESS_METRICS
        } else {
            metrics::SYSTEM_METRICS
        };
        for id in ids {
            let id = id.as_str().unwrap();
            if !allowed.contains(&id) {
                return error("INVALID_ARGUMENT", "Unknown metric for target kind", false);
            }
            let value = process.as_ref().map_or_else(
                || metrics::system_value(&system, id),
                |sample| metrics::process_value(sample, id),
            );
            if let Some(value) = value {
                values.insert(id.into(), value);
            } else {
                unavailable.insert(id.into(), json!("unknown"));
            }
        }
        samples.push(json!({"target":target,"values":values,"unavailable":unavailable}));
    }
    json!({"samples":samples})
}

fn get_history(session: &mut Session, view: &View, args: &Value) -> Value {
    let Ok(from) = chrono::DateTime::parse_from_rfc3339(args["from"].as_str().unwrap()) else {
        return error("INVALID_ARGUMENT", "Invalid interval", false);
    };
    let Ok(to) = chrono::DateTime::parse_from_rfc3339(args["to"].as_str().unwrap()) else {
        return error("INVALID_ARGUMENT", "Invalid interval", false);
    };
    if from > to {
        return error("INVALID_ARGUMENT", "from must not exceed to", false);
    }
    if args.get("cursor").is_some() {
        return session.page(args, None);
    }
    let ids = args["metric_ids"].as_array().unwrap();
    let mut samples = Vec::new();
    let mut bounds = None;
    if let Some(reference) = args["target"]["process_ref"].as_str() {
        let Some(identity) = session.refs.get(reference) else {
            return error("REFERENCE_INVALID", "Refresh the process list", false);
        };
        if view.processes.time_range_for(identity).is_none() {
            return error("REFERENCE_INVALID", "Process history was pruned", false);
        }
        if ids
            .iter()
            .any(|id| !metrics::PROCESS_METRICS.contains(&id.as_str().unwrap()))
        {
            return error("INVALID_ARGUMENT", "Unknown process metric", false);
        }
        for sample in view.processes.samples_for_iter(identity) {
            bounds = Some((
                bounds.map_or(sample.captured_at, |(first, _)| first),
                sample.captured_at,
            ));
            if sample.captured_at >= from && sample.captured_at <= to {
                samples.push((
                    sample.captured_at,
                    ids.iter()
                        .map(|id| metrics::process_value(sample, id.as_str().unwrap()))
                        .collect::<Vec<_>>(),
                ));
            }
        }
    } else {
        if ids
            .iter()
            .any(|id| !metrics::SYSTEM_METRICS.contains(&id.as_str().unwrap()))
        {
            return error("INVALID_ARGUMENT", "Unknown system metric", false);
        }
        for sample in view.system.samples_iter() {
            bounds = Some((
                bounds.map_or(sample.captured_at, |(first, _)| first),
                sample.captured_at,
            ));
            if sample.captured_at >= from && sample.captured_at <= to {
                samples.push((
                    sample.captured_at,
                    ids.iter()
                        .map(|id| metrics::system_value(sample, id.as_str().unwrap()))
                        .collect::<Vec<_>>(),
                ));
            }
        }
    }
    let max_interval = samples
        .windows(2)
        .map(|pair| (pair[1].0 - pair[0].0).num_milliseconds())
        .max();
    let coverage = json!({"retained_from":bounds.map(|(first,_)|first.with_timezone(&Utc).to_rfc3339()),"retained_to":bounds.map(|(_,last)|last.with_timezone(&Utc).to_rfc3339()),"max_sample_interval_ms":max_interval,"sample_count":samples.len()});
    if args["mode"] == "summary" {
        let mut summaries = serde_json::Map::new();
        for (index, id) in ids.iter().enumerate() {
            let available: Vec<_> = samples
                .iter()
                .filter_map(|(time, values)| values[index].as_ref().map(|value| (*time, value)))
                .collect();
            let mut summary = json!({"sample_count":samples.len(),"available_value_count":available.len(),"missing_value_count":samples.len()-available.len()});
            if let Some((first_time, first)) = available.first() {
                let (last_time, last) = available.last().unwrap();
                summary["first"] = (*first).clone();
                summary["last"] = (*last).clone();
                summary["first_at"] = json!(first_time.with_timezone(&Utc).to_rfc3339());
                summary["last_at"] = json!(last_time.with_timezone(&Utc).to_rfc3339());
                if first.is_string() {
                    let values: Vec<u64> = available
                        .iter()
                        .map(|(_, value)| value.as_str().unwrap().parse().unwrap())
                        .collect();
                    summary["min"] = json!(values.iter().min().unwrap().to_string());
                    summary["max"] = json!(values.iter().max().unwrap().to_string());
                    let sum: u128 = values.iter().map(|value| u128::from(*value)).sum();
                    // Exact rational mean avoids rounding u64 values through IEEE-754.
                    summary["mean"] =
                        json!({"numerator":sum.to_string(),"denominator":values.len().to_string()});
                    if *first_time == from && *last_time == to {
                        summary["delta"] = json!(
                            (i128::from(*values.last().unwrap()) - i128::from(values[0]))
                                .to_string()
                        );
                    }
                } else {
                    let values: Vec<f64> = available
                        .iter()
                        .map(|(_, value)| value.as_f64().unwrap())
                        .collect();
                    summary["min"] = json!(values.iter().copied().fold(f64::INFINITY, f64::min));
                    summary["max"] =
                        json!(values.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                    let mean = values
                        .iter()
                        .map(|value| value / values.len() as f64)
                        .sum::<f64>();
                    if mean.is_finite() {
                        summary["mean"] = json!(mean);
                    }
                    if *first_time == from && *last_time == to {
                        let delta = values.last().unwrap() - values[0];
                        if delta.is_finite() {
                            summary["delta"] = json!(delta);
                        }
                    }
                }
            }
            summaries.insert(id.as_str().unwrap().into(), summary);
        }
        return json!({"summaries":summaries,"coverage":coverage});
    }
    let rows=samples.into_iter().map(|(time,entries)| {
        let mut values=serde_json::Map::new();let mut unavailable=serde_json::Map::new();
        for (id,value) in ids.iter().zip(entries) { let id=id.as_str().unwrap();if let Some(value)=value {values.insert(id.into(),value);}else{unavailable.insert(id.into(),json!("unknown"));} }
        json!({"captured_at":time.with_timezone(&Utc).to_rfc3339(),"values":values,"unavailable":unavailable})
    }).collect();
    session.page_with_context(args, Some(rows), json!({"captured_at":view.snapshot.captured_at.with_timezone(&Utc).to_rfc3339(),"temporal_coverage":coverage}), Instant::now())
}

fn start_capture(session: &mut Session, view: &View, args: &Value) -> Value {
    if session
        .jobs
        .values()
        .filter(|job| job.completed.is_none())
        .count()
        >= 2
        || session.jobs.len() >= 64
    {
        return error("BUSY", "Connection job limit reached", true);
    }
    if session.capture_count() >= 8 {
        return error(
            "RESOURCE_LIMIT",
            "Release a capture before collecting",
            true,
        );
    }
    let process = if let Some(reference) = args["process_ref"].as_str() {
        let Some(identity) = session.refs.get(reference) else {
            return error("REFERENCE_INVALID", "Refresh the process list", false);
        };
        let Some(row) = view
            .snapshot
            .processes
            .iter()
            .find(|row| ProcessIdentity::from_row(row) == *identity)
        else {
            return error(
                "PROCESS_EXITED",
                "Target no longer appears in live samples",
                false,
            );
        };
        if identity.start_time.is_none() {
            return error("IDENTITY_UNVERIFIABLE", "Creation time unavailable", false);
        }
        Some(row.clone())
    } else {
        None
    };
    match super::jobs::Job::start(
        args["kind"].as_str().unwrap().into(),
        process,
        args["fields"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .or_else(|| {
                (args["kind"] == "process_info").then(|| vec!["image".into(), "version".into()])
            }),
    ) {
        Ok(job) => {
            let id = uuid::Uuid::new_v4().to_string();
            session.jobs.insert(id.clone(), job);
            json!({"job_id":id,"state":"running"})
        }
        Err(error) => error,
    }
}
fn read_capture(session: &mut Session, args: &Value) -> Value {
    let id = args["capture_id"]
        .as_str()
        .or_else(|| {
            args["cursor"]
                .as_str()
                .and_then(|s| s.split_once(':').map(|p| p.0))
        })
        .unwrap_or("");
    let used = session.retained_bytes();
    let count = session.capture_count();
    for (log_ref, log) in &mut session.logs {
        if log.session.captures.contains_key(id) {
            if log.created.elapsed() >= Duration::from_secs(300) {
                return error("REFERENCE_INVALID", "Log expired", false);
            }
            let previous_bytes = log.session.retained_bytes();
            let previous_ids: std::collections::HashSet<_> =
                log.session.captures.keys().cloned().collect();
            let data = read_capture(&mut log.session, args);
            if count
                + log
                    .session
                    .captures
                    .len()
                    .saturating_sub(previous_ids.len())
                > 8
                || used
                    .saturating_sub(previous_bytes)
                    .saturating_add(log.session.retained_bytes())
                    > 64 * 1024 * 1024
            {
                log.session
                    .captures
                    .retain(|id, _| previous_ids.contains(id));
                return error("RESOURCE_LIMIT", "Connection result budget exceeded", true);
            }
            return log_envelope(log, log_ref, data);
        }
    }
    let Some(capture) = session.captures.get(id) else {
        return error(
            "REFERENCE_INVALID",
            "Capture expired or belongs to another connection",
            false,
        );
    };
    if capture.created.elapsed() >= Duration::from_secs(300) {
        return error("REFERENCE_INVALID", "Capture expired", false);
    }
    let unmodified =
        args.get("projection").is_none() && args["filter"].as_str().is_none_or(str::is_empty);
    if unmodified {
        let offset = if let Some(cursor) = args["cursor"].as_str() {
            let Some((cursor_id, offset)) = cursor.split_once(':') else {
                return error("CURSOR_INVALID", "Malformed cursor", false);
            };
            if cursor_id != id {
                return error("CURSOR_INVALID", "Capture ID changed", false);
            }
            let Ok(offset) = offset.parse::<usize>() else {
                return error("CURSOR_INVALID", "Malformed cursor", false);
            };
            offset
        } else {
            0
        };
        return render_page(
            capture,
            id,
            offset,
            args["limit"].as_u64().unwrap_or(100) as usize,
        );
    }
    if args.get("cursor").is_some() {
        return session.page(args, None);
    }
    if let Some((id, _)) = session.captures.iter().find(|(_, capture)| {
        capture.signature == *args && capture.created.elapsed() < Duration::from_secs(300)
    }) {
        let mut page_args = args.clone();
        page_args["cursor"] = json!(format!("{id}:0"));
        return session.page(&page_args, None);
    }
    let metadata = capture.metadata.clone();
    let created = capture.created;
    let query = args["filter"].as_str().unwrap_or("").to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut rows = Vec::new();
    for row in &capture.rows {
        if !terms.is_empty()
            && !row["path"]
                .as_str()
                .is_some_and(|path| terms.iter().all(|term| path.to_lowercase().contains(term)))
        {
            continue;
        }
        let mut row = row.clone();
        if let Some(fields) = args["projection"].as_array() {
            if fields.iter().any(|field| {
                !row.as_object()
                    .unwrap()
                    .contains_key(field.as_str().unwrap())
            }) {
                return error("INVALID_ARGUMENT", "Unsupported projection field", false);
            }
            row.as_object_mut()
                .unwrap()
                .retain(|key, _| fields.iter().any(|field| field.as_str() == Some(key)));
        }
        rows.push(row);
    }
    session.page_with_context(args, Some(rows), metadata, created)
}

fn start_wait(session: &mut Session, shared: Arc<RwLock<Arc<View>>>, args: &Value) -> Value {
    let view = shared.read().unwrap().clone();
    if view.activity == "log_view" {
        return error("ACTIVITY_CONFLICT", "Wait requires live sampling", false);
    }
    if session
        .jobs
        .values()
        .filter(|j| j.completed.is_none())
        .count()
        >= 2
        || session.jobs.len() >= 64
    {
        return error("BUSY", "Job limit reached", true);
    }
    let condition = args["condition"].as_str().unwrap();
    let identity = args["target"]["process_ref"]
        .as_str()
        .and_then(|r| session.refs.get(r))
        .cloned();
    if condition == "process_appeared" && args["process_name"].as_str().is_none()
        || condition == "process_exited" && identity.is_none()
        || condition == "metric_threshold"
            && (args.get("target").is_none()
                || args.get("metric_id").is_none()
                || args.get("comparison").is_none()
                || args.get("threshold").is_none())
    {
        return error(
            "INVALID_ARGUMENT",
            "Supply condition-specific name, target, metric and comparison",
            false,
        );
    }
    if args["target"].get("process_ref").is_some() && identity.is_none() {
        return error(
            "REFERENCE_INVALID",
            "Refresh live process references",
            false,
        );
    }
    if condition == "metric_threshold" {
        let metrics = if identity.is_some() {
            metrics::PROCESS_METRICS
        } else {
            metrics::SYSTEM_METRICS
        };
        if !metrics.contains(&args["metric_id"].as_str().unwrap())
            || numeric(&args["threshold"]).is_none()
        {
            return error(
                "INVALID_ARGUMENT",
                "Unknown metric or non-finite threshold",
                false,
            );
        }
    }
    let args = args.clone();
    let initial = view.snapshot.captured_at;
    let job = super::jobs::Job::custom(move |cancel| {
        let held = match identity
            .as_ref()
            .map(crate::samplers::verified_process::VerifiedProcess::open)
            .transpose()
        {
            Ok(h) => h,
            Err(code) => return error(code, "Cannot verify wait target", false),
        };
        let begin = Instant::now();
        loop {
            if cancel.load(Ordering::Acquire) {
                return error("CANCELLED", "Wait cancelled", false);
            }
            if begin.elapsed() >= Duration::from_millis(args["timeout_ms"].as_u64().unwrap()) {
                return error("TIMEOUT", "Condition deadline elapsed", false);
            }
            let view = shared.read().unwrap().clone();
            if view.activity == "log_view" {
                return error(
                    "ACTIVITY_CONFLICT",
                    "Live wait ended on entering Log view",
                    false,
                );
            }
            if args["condition"] == "process_exited" {
                match held.as_ref().unwrap().verify() {
                    Err("PROCESS_EXITED") => {
                        return json!({"condition":"process_exited","observed_at":Utc::now().to_rfc3339()});
                    }
                    Err(code) => return error(code, "Wait target cannot be verified", false),
                    Ok(()) => {}
                }
            } else if view.snapshot.captured_at > initial
                && (Local::now() - view.snapshot.captured_at).num_seconds() < 3
            {
                if args["condition"] == "process_appeared" {
                    if let Some(row) = view.snapshot.processes.iter().find(|p| {
                        p.name
                            .eq_ignore_ascii_case(args["process_name"].as_str().unwrap())
                    }) {
                        return json!({"condition":"process_appeared","pid":row.pid,"name":row.name,"start_time":row.start_time,"observed_at":view.snapshot.captured_at.to_rfc3339()});
                    }
                } else {
                    let metric = args["metric_id"].as_str().unwrap();
                    let value = if let Some(id) = &identity {
                        view.processes
                            .samples_for_iter(id)
                            .last()
                            .filter(|s| s.captured_at == view.snapshot.captured_at)
                            .and_then(|s| metrics::process_value(s, metric))
                    } else {
                        metrics::system_value(
                            &crate::model::history::SystemSample::from_snapshot(&view.snapshot),
                            metric,
                        )
                    };
                    if let Some(value) = value
                        && compare_metric(
                            &value,
                            &args["threshold"],
                            args["comparison"].as_str().unwrap(),
                        )
                    {
                        return json!({"condition":"metric_threshold","value":value,"observed_at":view.snapshot.captured_at.to_rfc3339()});
                    }
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
    });
    match job {
        Ok(job) => {
            let id = uuid::Uuid::new_v4().to_string();
            session.jobs.insert(id.clone(), job);
            json!({"job_id":id,"state":"running"})
        }
        Err(e) => e,
    }
}
fn numeric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
fn compare_metric(value: &Value, threshold: &Value, comparison: &str) -> bool {
    let order = if let (Some(a), Some(b)) = (
        value.as_str().and_then(|v| v.parse::<i128>().ok()),
        threshold
            .as_str()
            .and_then(|v| v.parse::<i128>().ok())
            .or_else(|| threshold.as_i64().map(i128::from)),
    ) {
        Some(a.cmp(&b))
    } else {
        numeric(value)
            .zip(numeric(threshold))
            .and_then(|(a, b)| a.partial_cmp(&b))
    };
    order.is_some_and(|o| match comparison {
        "gt" => o.is_gt(),
        "ge" => o.is_ge(),
        "lt" => o.is_lt(),
        "le" => o.is_le(),
        "eq" => o.is_eq(),
        _ => false,
    })
}

fn insert_job(session: &mut Session, job: super::jobs::Job) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    session.jobs.insert(id.clone(), job);
    json!({"job_id":id,"state":"running"})
}
fn room_for_job(session: &Session) -> bool {
    session.jobs.len() < 64
        && session
            .jobs
            .values()
            .filter(|j| j.completed.is_none())
            .count()
            < 2
        && session.capture_count() < 8
}
fn list_logs(session: &mut Session, view: &View, args: &Value) -> Value {
    if args.get("cursor").is_some() {
        return session.page(args, None);
    }
    if !room_for_job(session) {
        return error("BUSY", "Job or result capacity reached", true);
    }
    let Some(root) = view.log_root.clone() else {
        return error(
            "INVALID_ARGUMENT",
            "Configure the recording directory locally first",
            false,
        );
    };
    let filter = args["filter"].as_str().unwrap_or("").to_lowercase();
    let job = super::jobs::Job::custom(move |cancel| {
        let dir = match std::fs::read_dir(root) {
            Ok(d) => d,
            Err(_) => {
                return error(
                    "ACCESS_DENIED",
                    "Cannot list configured log directory",
                    false,
                );
            }
        };
        let mut rows = Vec::new();
        let mut partial = false;
        for entry in dir {
            if cancel.load(Ordering::Acquire) {
                return error("CANCELLED", "Log listing cancelled", false);
            }
            let Ok(entry) = entry else {
                partial = true;
                continue;
            };
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("log"))
                || !entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&filter)
            {
                continue;
            }
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            rows.push(json!({"log_id":log_id(&path),"name":entry.file_name().to_string_lossy(),"_path":path}));
            if rows.len() >= 5000 {
                partial = true;
                break;
            }
        }
        rows.sort_by_key(|r| r["name"].as_str().unwrap_or("").to_lowercase());
        json!({"rows":rows,"source":"log","kind":"log_list","status":if partial{"partial"}else{"complete"},"warnings":[],"coverage":{"limited":partial}})
    });
    match job {
        Ok(mut j) => {
            j.live = false;
            insert_job(session, j)
        }
        Err(e) => e,
    }
}
fn open_log(session: &mut Session, view: &View, args: &Value) -> Value {
    if !room_for_job(session) {
        return error("BUSY", "Job or result capacity reached", true);
    }
    let Some(path) = session
        .log_files
        .get(args["log_id"].as_str().unwrap())
        .cloned()
    else {
        return error("REFERENCE_INVALID", "List logs to obtain a log ID", false);
    };
    let Some(root) = view.log_root.clone() else {
        return error("REFERENCE_INVALID", "Configured root changed", false);
    };
    let available = (64 * 1024 * 1024usize).saturating_sub(session.retained_bytes());
    let id = uuid::Uuid::new_v4().to_string();
    let log_ref = id.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let host = view.host.clone();
    let job = super::jobs::Job::custom(move |cancel| {
        // Resolve aliases before checking root membership; reject replaced links out of scope.
        let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
            return error("ACCESS_DENIED", "Log path is unavailable", false);
        };
        if !path.starts_with(&root) {
            return error(
                "PERMISSION_DENIED",
                "Log is outside the configured root",
                false,
            );
        }
        match crate::app::logs::load_mcp_log(&path, available, cancel) {
            Ok((log, bytes, partial)) => {
                let interval = log.interval_seconds;
                let loaded = Loaded {
                    view: View {
                        snapshot: log.snapshot,
                        processes: log.process_history,
                        system: log.system_history,
                        tracked_names: log.tracked_names,
                        activity: "saved_log".into(),
                        host,
                        control: json!({}),
                        log_root: None,
                    },
                    session: Box::default(),
                    created: Instant::now(),
                    bytes,
                    partial,
                    interval,
                };
                let _ = sender.send(loaded);
                json!({"log_ref":log_ref,"interval_seconds":interval,"status":if partial{"partial"}else{"complete"},"warnings":if partial{vec!["Interrupted recording; only complete readable frames retained"]}else{vec![]}})
            }
            Err(e) => {
                let text = e.to_string();
                let code = if text.contains("RESULT_TOO_LARGE") {
                    "RESULT_TOO_LARGE"
                } else if text.contains("TIMEOUT") {
                    "TIMEOUT"
                } else if text.contains("CANCELLED") {
                    "CANCELLED"
                } else {
                    "INVALID_ARGUMENT"
                };
                error(
                    code,
                    "Log load failed: invalid recording, unavailable file, or resource limit",
                    false,
                )
            }
        }
    });
    match job {
        Ok(mut j) => {
            j.live = false;
            let result = insert_job(session, j);
            session
                .loading
                .insert(id, (result["job_id"].as_str().unwrap().into(), receiver));
            result
        }
        Err(e) => e,
    }
}
fn query_log(session: &mut Session, method: &str, args: &Value) -> Value {
    let Some(id) = args["log_ref"].as_str() else {
        return error("REFERENCE_INVALID", "log_ref is required", false);
    };
    let used = session.retained_bytes();
    let count = session.capture_count();
    let Some(log) = session
        .logs
        .get_mut(id)
        .filter(|l| l.created.elapsed() < Duration::from_secs(300))
    else {
        return error(
            "REFERENCE_INVALID",
            "Log reference expired or closed",
            false,
        );
    };
    if method != "winproc_get_metrics"
        && args.get("cursor").is_none()
        && (count >= 8 || used >= 64 * 1024 * 1024)
    {
        return error("BUSY", "Release retained results before querying", true);
    }
    let previous_bytes = log.session.retained_bytes();
    let previous_ids: std::collections::HashSet<_> = log.session.captures.keys().cloned().collect();
    let mut a = args.clone();
    a["source"] = json!("live");
    a.as_object_mut().unwrap().remove("log_ref");
    let data = match method {
        "winproc_list_processes" => list_processes(&mut log.session, &log.view, &a),
        "winproc_get_metrics" => get_metrics(&log.session, &log.view, &a),
        "winproc_get_history" => get_history(&mut log.session, &log.view, &a),
        _ => error("INVALID_ARGUMENT", "Unsupported log query", false),
    };
    if data.get("error").is_some() {
        return data;
    }
    let new_bytes = log.session.retained_bytes();
    if used
        .saturating_sub(previous_bytes)
        .saturating_add(new_bytes)
        > 64 * 1024 * 1024
    {
        log.session
            .captures
            .retain(|id, _| previous_ids.contains(id));
        return error(
            "RESULT_TOO_LARGE",
            "Log query exceeds retained budget",
            false,
        );
    }
    log_envelope(log, id, data)
}

// Both direct queries and generic capture reads preserve loaded-source metadata.
// Apply this after paging so projections cannot remove source-level warnings.
fn log_envelope(log: &Loaded, log_ref: &str, data: Value) -> Value {
    let mut result = envelope(&log.session, &log.view, "", data);
    if result.get("error").is_some() {
        return result;
    }
    result["source"] = json!("log");
    result["log_ref"] = json!(log_ref);
    result["interval_seconds"] = json!(log.interval);
    if log.partial {
        result["status"] = json!("partial");
        let warning = json!("Interrupted recording");
        let warnings = result["warnings"]
            .as_array_mut()
            .expect("envelope warnings");
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
    result
}

pub(crate) fn log_id(path: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    format!("log-{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> View {
        let snapshot = Snapshot {
            captured_at: Local::now(),
            total_memory: 0,
            used_memory: 0,
            available_memory: None,
            modified_memory: None,
            standby_memory: None,
            free_zeroed_memory: None,
            committed_memory: None,
            commit_limit: None,
            paged_pool_memory: None,
            nonpaged_pool_memory: None,
            pages_input_per_sec: None,
            pages_output_per_sec: None,
            cpu_name: None,
            cpu_frequency_mhz: None,
            cpu_current_frequency_mhz: None,
            cpu_p_core_frequency_mhz: None,
            cpu_e_core_frequency_mhz: None,
            cpu_total_usage_percent: None,
            cpu_user_usage_percent: None,
            cpu_kernel_usage_percent: None,
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_adapters: Vec::new(),
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            process_count: 0,
            thread_count: None,
            processes: Vec::new(),
        };
        View {
            snapshot,
            processes: ProcessHistory::default(),
            system: SystemHistory::default(),
            activity: "live".into(),
            tracked_names: Vec::new(),
            host: json!({}),
            control: json!({}),
            log_root: None,
        }
    }
    fn saved_session() -> Session {
        let mut s = Session::default();
        let mut view = view();
        view.activity = "saved_log".into();
        s.logs.insert(
            "saved".into(),
            Loaded {
                view,
                session: Box::default(),
                created: Instant::now(),
                bytes: 1024,
                partial: false,
                interval: 1,
            },
        );
        s
    }
    #[test]
    fn frozen_reads_preserve_source_cursor_and_capacity() {
        let mut s = Session::default();
        let args = json!({"limit":1});
        let first = s.page(&args, Some(vec![json!({"pid":1}), json!({"pid":2})]));
        for _ in 0..7 {
            s.page(&args, Some(vec![]));
        }
        let id = first["capture_id"].as_str().unwrap();
        let created = s.captures[id].created;
        let read = read_capture(&mut s, &json!({"capture_id":id,"limit":1}));
        assert_eq!(read["capture_id"], id);
        assert_eq!(s.capture_count(), 8);
        assert_eq!(s.captures[id].created, created);
        let next = s.page(&json!({"limit":1,"cursor":first["next_cursor"]}), None);
        assert_eq!(next["rows"][0]["pid"], 2);
        let read_next = read_capture(
            &mut s,
            &json!({"capture_id":id,"cursor":read["next_cursor"],"limit":1}),
        );
        assert_eq!(read_next["rows"][0]["pid"], 2);
    }
    #[test]
    fn completion_and_derived_pages_enforce_connection_wide_capacity() {
        let mut s = saved_session();
        let child = &mut s.logs.get_mut("saved").unwrap().session;
        let first = child.page(&json!({}), Some(vec![json!({"pid":1})]));
        for _ in 0..6 {
            child.page(&json!({}), Some(vec![]));
        }
        let derived = read_capture(
            &mut s,
            &json!({"capture_id":first["capture_id"],"projection":["pid"]}),
        );
        assert_eq!(derived["error"]["code"], "RESOURCE_LIMIT");
        assert_eq!(s.capture_count(), 8);
        let mut job = super::super::jobs::Job::custom(|_| json!({"rows":[{"pid":2}]})).unwrap();
        for _ in 0..100 {
            job.poll();
            if job.completed.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(job.completed.is_some());
        s.jobs.insert("late".into(), job);
        s.collect_jobs();
        assert_eq!(
            s.jobs["late"].outcome.as_ref().unwrap()["error"]["code"],
            "RESOURCE_LIMIT"
        );
        assert_eq!(s.capture_count(), 8);
    }
    #[test]
    fn log_budget_counts_existing_child_bytes_once_and_preserves_rejected_sources() {
        let mut s = saved_session();
        let child = &mut s.logs.get_mut("saved").unwrap().session;
        let first = child.page(
            &json!({}),
            Some(vec![json!({"text":"x".repeat(1024*1024)})]),
        );
        let previous = child.retained_bytes();
        // Leave 100 bytes free: an empty new list fits, a duplicate 1-MiB projection does not.
        s.logs.get_mut("saved").unwrap().bytes = 64 * 1024 * 1024 - previous - 100;
        let result = query_log(
            &mut s,
            "winproc_list_processes",
            &json!({"source":"log","log_ref":"saved"}),
        );
        assert!(result.get("error").is_none(), "{result}");
        let count = s.capture_count();
        let result = read_capture(
            &mut s,
            &json!({"capture_id":first["capture_id"],"projection":["text"]}),
        );
        assert_eq!(result["error"]["code"], "RESOURCE_LIMIT");
        assert_eq!(s.capture_count(), count);
        assert!(
            s.logs["saved"]
                .session
                .captures
                .contains_key(first["capture_id"].as_str().unwrap())
        );
    }
    #[test]
    fn log_references_are_isolated_and_expire_without_os_queries() {
        let mut view = view();
        view.activity = "saved_log".into();
        let row = crate::model::ProcessRow {
            pid: 42,
            name: "saved.exe".into(),
            start_time: Some(1),
            private_bytes: Some(9007199254740993),
            ..Default::default()
        };
        view.processes
            .record_snapshot_unbounded(view.snapshot.captured_at, std::slice::from_ref(&row));
        view.snapshot.processes = vec![row];
        let mut session = Session::default();
        session.logs.insert(
            "saved".into(),
            Loaded {
                view,
                session: Box::default(),
                created: Instant::now(),
                bytes: 1024,
                partial: true,
                interval: 5,
            },
        );
        let listed = query_log(
            &mut session,
            "winproc_list_processes",
            &json!({"source":"log","log_ref":"saved","scope":"all"}),
        );
        assert_eq!(listed["source"], "log");
        assert_eq!(listed["data"]["rows"][0]["lifecycle"], "unknown");
        let reference = listed["data"]["rows"][0]["process_ref"].clone();
        assert!(!session.refs.contains_key(reference.as_str().unwrap()));
        let metrics = query_log(
            &mut session,
            "winproc_get_metrics",
            &json!({"source":"log","log_ref":"saved","targets":[{"process_ref":reference}],"metric_ids":["private_bytes"]}),
        );
        assert_eq!(metrics["source"], "log");
        assert_eq!(metrics["status"], "partial");
        assert_eq!(metrics["interval_seconds"], 5);
        session.logs.get_mut("saved").unwrap().created = Instant::now() - Duration::from_secs(301);
        assert_eq!(
            query_log(
                &mut session,
                "winproc_list_processes",
                &json!({"source":"log","log_ref":"saved"})
            )["error"]["code"],
            "REFERENCE_INVALID"
        );
    }
    #[test]
    fn saved_log_capture_routes_preserve_provenance_and_interruption() {
        for partial in [false, true] {
            let mut session = saved_session();
            let log = session.logs.get_mut("saved").unwrap();
            log.view.activity = "saved_log".into();
            log.partial = partial;
            log.interval = 5;
            for pid in 1..=3 {
                let row = crate::model::ProcessRow {
                    pid,
                    name: format!("recorded-{pid}.exe"),
                    start_time: Some(1),
                    executable_path: Some(format!("C:/recorded/{pid}.exe")),
                    private_bytes: Some(100),
                    ..Default::default()
                };
                log.view.snapshot.processes.push(row);
            }
            log.view.processes.record_snapshot_unbounded(
                log.view.snapshot.captured_at,
                &log.view.snapshot.processes,
            );
            let args = json!({"source":"log","log_ref":"saved","limit":1});
            let direct = query_log(&mut session, "winproc_list_processes", &args);
            let capture = direct["data"]["capture_id"].clone();
            let original_created =
                session.logs["saved"].session.captures[capture.as_str().unwrap()].created;
            let read = read_capture(&mut session, &json!({"capture_id":capture,"limit":1}));
            assert_eq!(
                read, direct,
                "generic read must preserve the full direct response"
            );
            let mut next_args = args.clone();
            next_args["cursor"] = direct["data"]["next_cursor"].clone();
            let next = query_log(&mut session, "winproc_list_processes", &next_args);
            let next_read = read_capture(
                &mut session,
                &json!({
                    "capture_id":capture,"limit":1,"cursor":direct["data"]["next_cursor"]
                }),
            );
            assert_eq!(next_read, next);
            let projected = read_capture(
                &mut session,
                &json!({
                    "capture_id":capture,"filter":"recorded","projection":["pid","lifecycle"],"limit":1
                }),
            );
            assert_eq!(projected["source"], "log");
            assert_eq!(projected["log_ref"], "saved");
            assert_eq!(projected["interval_seconds"], 5);
            assert_eq!(projected["captured_at"], direct["captured_at"]);
            assert_eq!(projected["status"], direct["status"]);
            assert_eq!(projected["warnings"], direct["warnings"]);
            assert_eq!(projected["data"]["rows"][0]["lifecycle"], "unknown");
            assert!(projected["data"]["rows"][0].get("name").is_none());
            let derived = projected["data"]["capture_id"].clone();
            assert_eq!(
                session.logs["saved"].session.captures[derived.as_str().unwrap()].created,
                original_created
            );
            let reread = read_capture(&mut session, &json!({"capture_id":derived,"limit":1}));
            assert_eq!(reread, projected);
            let time = session.logs["saved"].view.snapshot.captured_at.to_rfc3339();
            let history = query_log(
                &mut session,
                "winproc_get_history",
                &json!({
                    "source":"log","log_ref":"saved","target":{"process_ref":direct["data"]["rows"][0]["process_ref"]},
                    "metric_ids":["private_bytes"],"from":time,"to":time,"mode":"raw"
                }),
            );
            let history_read = read_capture(
                &mut session,
                &json!({"capture_id":history["data"]["capture_id"]}),
            );
            assert_eq!(history_read, history);
            assert_eq!(
                direct["status"],
                if partial { "partial" } else { "complete" }
            );
            assert_eq!(direct["data"]["rows"][0]["lifecycle"], "unknown");
            assert_eq!(direct["data"]["rows"][0]["source"], "log");
            session.logs.remove("saved");
            assert_eq!(
                read_capture(&mut session, &json!({"capture_id":capture}))["error"]["code"],
                "REFERENCE_INVALID"
            );
        }
    }

    #[test]
    fn saved_log_envelope_preserves_capture_warnings_and_errors() {
        let mut session = saved_session();
        let log = session.logs.get_mut("saved").unwrap();
        log.partial = true;
        let data = log.session.page_with_context(
            &json!({}),
            Some(vec![json!({"path":"recorded"})]),
            json!({"status":"partial","warnings":["Capture warning","Interrupted recording"],
                "capture_started_at":"2026-10-31T00:00:00Z",
                "capture_finished_at":"2026-10-31T00:00:02Z","coverage":{"limited":true}}),
            Instant::now(),
        );
        let response = read_capture(&mut session, &json!({"capture_id":data["capture_id"]}));
        assert_eq!(response["status"], "partial");
        assert_eq!(
            response["warnings"],
            json!(["Capture warning", "Interrupted recording"])
        );
        assert_eq!(response["source"], "log");
        assert_eq!(response["log_ref"], "saved");
        assert!(response.get("captured_at").is_none());
        assert_eq!(response["capture_finished_at"], "2026-10-31T00:00:02Z");
        assert_eq!(response["data"]["coverage"]["limited"], true);
        let failed = read_capture(
            &mut session,
            &json!({
                "capture_id":data["capture_id"],"projection":["nonexistent"]
            }),
        );
        assert_eq!(
            failed,
            error("INVALID_ARGUMENT", "Unsupported projection field", false)
        );
    }

    #[test]
    fn condition_jobs_use_fresh_samples_and_cancel_or_time_out() {
        let mut view = view();
        view.snapshot.captured_at = Local::now();
        let shared = Arc::new(RwLock::new(Arc::new(view.clone())));
        let mut session = Session::default();
        let started = start_wait(
            &mut session,
            shared.clone(),
            &json!({"condition":"process_appeared","process_name":"future.exe","timeout_ms":1000}),
        );
        let id = started["job_id"].as_str().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        session.jobs.get_mut(id).unwrap().poll();
        assert!(session.jobs[id].completed.is_none());
        view.snapshot.captured_at += chrono::Duration::milliseconds(100);
        view.snapshot.processes.push(crate::model::ProcessRow {
            name: "future.exe".into(),
            pid: 55,
            ..Default::default()
        });
        *shared.write().unwrap() = Arc::new(view);
        for _ in 0..50 {
            session.jobs.get_mut(id).unwrap().poll();
            if session.jobs[id].completed.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.jobs[id].outcome.as_ref().unwrap()["pid"], 55);
        let started = start_wait(
            &mut session,
            shared.clone(),
            &json!({"condition":"process_appeared","process_name":"never.exe","timeout_ms":1}),
        );
        let id = started["job_id"].as_str().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        session.jobs.get_mut(id).unwrap().poll();
        assert_eq!(
            session.jobs[id].outcome.as_ref().unwrap()["error"]["code"],
            "TIMEOUT"
        );
        let started = start_wait(
            &mut session,
            shared,
            &json!({"condition":"process_appeared","process_name":"never.exe","timeout_ms":1000}),
        );
        let id = started["job_id"].as_str().unwrap();
        session.jobs.get_mut(id).unwrap().cancel();
        assert_eq!(
            session.jobs[id].outcome.as_ref().unwrap()["error"]["code"],
            "CANCELLED"
        );
    }
    #[test]
    fn pages_reduce_to_encoded_budget_and_reject_a_single_oversized_row() {
        let mut session = Session::default();
        let row = json!({"text":"x".repeat(400_000)});
        let result = session.page(&json!({"limit":10}), Some(vec![row; 10]));
        assert!(result["rows"].as_array().unwrap().len() < 10);
        assert!(result["next_cursor"].is_string());
        let result = session.page(
            &json!({"limit":1}),
            Some(vec![json!({"text":"x".repeat(2_000_000)})]),
        );
        assert_eq!(result["error"]["code"], "RESULT_TOO_LARGE");
    }
    #[test]
    fn threshold_comparison_preserves_large_integers() {
        assert!(compare_metric(
            &json!("9007199254740993"),
            &json!("9007199254740992"),
            "gt"
        ));
        assert!(!compare_metric(
            &json!("9007199254740993"),
            &json!("9007199254740992"),
            "eq"
        ));
    }

    #[test]
    fn ended_history_is_discoverable_and_references_do_not_cross_sessions() {
        let mut view = view();
        let mut session = Session::default();
        let row = crate::model::ProcessRow {
            pid: 100,
            name: "ended.exe".into(),
            start_time: Some(10),
            private_bytes: Some(123),
            ..Default::default()
        };
        view.processes
            .record_snapshot(view.snapshot.captured_at, &[row], &Default::default());
        let listed = list_processes(&mut session, &view, &json!({"scope":"retained"}));
        let reference = listed["rows"][0]["process_ref"].as_str().unwrap();
        let args = json!({"source":"live","targets":[{"process_ref":reference}],"metric_ids":["private_bytes"]});
        assert_eq!(
            get_metrics(&session, &view, &args)["error"]["code"],
            "PROCESS_EXITED"
        );
        assert_eq!(
            get_metrics(&Session::default(), &view, &args)["error"]["code"],
            "REFERENCE_INVALID"
        );
        assert_eq!(
            list_processes(&mut Session::default(), &view, &json!({"scope":"live"}))["rows"],
            json!([])
        );
    }
    #[test]
    fn summary_counts_stored_samples_only_and_keeps_exact_integer_statistics() {
        let mut view = view();
        let mut session = Session::default();
        let first = view.snapshot.captured_at;
        for (seconds, value) in [(0, Some(0)), (1, None), (7, Some(u64::MAX)), (8, None)] {
            let row = crate::model::ProcessRow {
                pid: 1,
                name: "test.exe".into(),
                start_time: Some(1),
                private_bytes: value,
                ..Default::default()
            };
            view.processes.record_snapshot(
                first + chrono::Duration::seconds(seconds),
                &[row],
                &Default::default(),
            );
        }
        let listed = list_processes(&mut session, &view, &json!({"scope":"retained"}));
        let result = get_history(
            &mut session,
            &view,
            &json!({"source":"live","target":{"process_ref":listed["rows"][0]["process_ref"]},"metric_ids":["private_bytes"],"from":(first-chrono::Duration::seconds(20)).to_rfc3339(),"to":(first+chrono::Duration::seconds(20)).to_rfc3339(),"mode":"summary"}),
        );
        let summary = &result["summaries"]["private_bytes"];
        assert_eq!(summary["sample_count"], 4);
        assert_eq!(summary["available_value_count"], 2);
        assert_eq!(summary["missing_value_count"], 2);
        assert_eq!(summary["min"], "0");
        assert_eq!(summary["max"], u64::MAX.to_string());
        assert!(summary.get("delta").is_none());
        assert_eq!(
            summary["mean"],
            json!({"numerator":u64::MAX.to_string(),"denominator":"2"})
        );
        assert_eq!(result["coverage"]["max_sample_interval_ms"], 6000);
    }
    #[test]
    fn capture_metadata_and_expiry_survive_filtering_and_pagination() {
        let view = view();
        let mut session = Session::default();
        let created = Instant::now() - Duration::from_secs(290);
        let metadata = json!({"source":"log","status":"partial","capture_started_at":"2026-10-31T00:00:00Z","capture_finished_at":"2026-10-31T00:00:02Z","coverage":{"denied_processes":3},"temporal_coverage":{"sample_count":2}});
        session.captures.insert(
            "source".into(),
            Capture {
                signature: json!({}),
                rows: vec![json!({"path":"alpha"}), json!({"path":"alpha-two"})],
                created,
                metadata: metadata.clone(),
            },
        );
        let first = read_capture(
            &mut session,
            &json!({"capture_id":"source","filter":"alpha","limit":1}),
        );
        let second = read_capture(
            &mut session,
            &json!({"capture_id":"source","filter":"alpha","limit":1,"cursor":first["next_cursor"]}),
        );
        assert_eq!(first["coverage"]["denied_processes"], 3);
        assert_eq!(second["coverage"], first["coverage"]);
        assert_eq!(second["temporal_coverage"], first["temporal_coverage"]);
        let response = envelope(&session, &view, "backend", second);
        assert_eq!(response["source"], "log");
        assert_eq!(response["status"], "partial");
        assert_eq!(
            response["capture_finished_at"],
            metadata["capture_finished_at"]
        );
        assert!(response.get("captured_at").is_none());
        assert_eq!(
            session.captures[first["capture_id"].as_str().unwrap()].created,
            created
        );
    }
    #[test]
    fn frozen_sample_timestamp_survives_new_live_samples() {
        let mut view = view();
        let mut session = Session::default();
        let captured = view.snapshot.captured_at.with_timezone(&Utc).to_rfc3339();
        let first = session.page_with_context(
            &json!({"limit":1}),
            Some(vec![json!({"pid":1}), json!({"pid":2})]),
            json!({"captured_at":captured}),
            Instant::now(),
        );
        view.snapshot.captured_at += chrono::Duration::seconds(10);
        let next = session.page(&json!({"limit":1,"cursor":first["next_cursor"]}), None);
        assert_eq!(
            envelope(&session, &view, "backend", next)["captured_at"],
            captured
        );
    }
    #[test]
    fn full_capture_budget_still_allows_existing_capture_reads() {
        let mut session = Session::default();
        for index in 0..8 {
            session.captures.insert(
                index.to_string(),
                Capture {
                    signature: json!({}),
                    rows: vec![json!({"path":"one"})],
                    created: Instant::now(),
                    metadata: json!({}),
                },
            );
        }
        for _ in 0..20 {
            let result = read_capture(&mut session, &json!({"capture_id":"0","limit":1}));
            assert_eq!(result["rows"], json!([{"path":"one"}]));
            assert_eq!(result["capture_id"], "0");
        }
        assert_eq!(session.captures.len(), 8);
    }
    #[test]
    fn pagination_is_immutable_and_bound_to_arguments() {
        let mut session = Session::default();
        let first = session.page(
            &json!({"limit":1}),
            Some(vec![json!({"pid":1}), json!({"pid":2})]),
        );
        let next = first["next_cursor"].clone();
        assert_eq!(
            session.page(&json!({"limit":1,"cursor":next}), None)["rows"],
            json!([{"pid":2}])
        );
        assert_eq!(
            session.page(&json!({"limit":2,"cursor":next}), None)["error"]["code"],
            "CURSOR_INVALID"
        );
        assert_eq!(
            Session::default().page(&json!({"limit":1,"cursor":next}), None)["error"]["code"],
            "CURSOR_INVALID"
        );
    }
}

#[cfg(test)]
mod connection_status_tests {
    use super::*;
    #[test]
    fn authenticated_connections_survive_rejected_and_partial_disconnects() {
        let mut status = ConnectionStatus::default();
        status.update(&Event::Ready("backend".into()));
        assert!(!status.connected());
        status.update(&Event::Connected("one".into()));
        status.update(&Event::Connected("two".into()));
        status.update(&Event::Disconnected("unverified".into()));
        assert_eq!(status.label, "Listening (2 clients)");
        status.update(&Event::Disconnected("one".into()));
        assert!(status.connected());
        status.update(&Event::Disconnected("two".into()));
        assert!(!status.connected());
        assert_eq!(status.label, "Listening");
        status.update(&Event::Connected("three".into()));
        status.update(&Event::Failed("unavailable".into()));
        assert!(!status.connected());
        status.update(&Event::Disconnected("three".into()));
        assert_eq!(status.label, "unavailable");
    }
}
