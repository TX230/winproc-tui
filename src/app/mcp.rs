//! Application publishing boundary for external investigation clients.
use super::{App, AppActivity};
use crate::mcp::backend::{Backend, View};
use crate::mcp::{
    operations::{self, Action},
    transport::error,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
#[derive(Default)]
pub(crate) struct ControlState {
    pub operations: operations::Shared,
    fingerprint: Value,
    revision: u64,
    epoch: Option<String>,
    profile_ids: HashMap<String, String>,
    recording_key: Option<(String, std::time::Instant)>,
    recording_id: Option<String>,
    pub confirmation: Option<(Action, String)>,
    pub confirmation_scroll: usize,
}

impl App {
    fn mcp_view(&self) -> View {
        // App.snapshot remains the last live sample; display_snapshot() may be a loaded log.
        View {
            snapshot: self.snapshot.clone(),
            processes: self.process_history.clone(),
            system: self.system_history.clone(),
            tracked_names: self.watch_list.clone(),
            control: self.mcp_control(),
            log_root: self.recording_last_dir.clone(),
            host: serde_json::json!({"windows_version":self.system_info_host.windows_version,"windows_build":self.system_info_host.windows_build,"architecture":self.system_info_host.architecture,"cpu_name":self.snapshot.cpu_name,"cpu_topology":self.snapshot.cpu_topology,"cpu_cache":self.snapshot.cpu_cache,"total_memory_bytes":self.snapshot.total_memory.to_string()}),
            activity: match self.activity() {
                AppActivity::Live => "live",
                AppActivity::Recording => "recording",
                AppActivity::LogView => "log_view",
            }
            .into(),
        }
    }
    pub(crate) fn poll_mcp(&mut self) -> bool {
        self.refresh_mcp_control();
        let mut action_changed = false;
        if let Some((action, _)) = &self.mcp_state.confirmation {
            let active = self.mcp_state.operations.lock().unwrap().active.clone();
            if active.as_deref() != Some(&action.id)
                || action.created.elapsed().as_secs() >= 60
                || self.mcp_connections_disabled
            {
                let action = action.clone();
                self.mcp_state.operations.lock().unwrap().finish(
                    &action.id,
                    if self.mcp_connections_disabled || active.as_deref() != Some(&action.id) {
                        error(
                            "CANCELLED",
                            "Confirmation disconnected or MCP disabled",
                            false,
                        )
                    } else {
                        error("TIMEOUT", "Confirmation expired", false)
                    },
                );
                self.mcp_state.confirmation = None;
                action_changed = true;
            }
        }
        if self.mcp_state.confirmation.is_none() && !self.mcp_connections_disabled {
            let hub = self.mcp_state.operations.clone();
            let mut operations = hub.lock().unwrap();
            let pending = operations.pending.clone();
            if let Some(action) = pending {
                let result = self.apply_mcp_action(&action, false);
                if let Some(description) = result.get("confirmation").and_then(Value::as_str) {
                    operations.confirming(&action.id, description);
                    self.mcp_state.confirmation_scroll = 0;
                    self.mcp_state.confirmation = Some((action, description.into()));
                } else {
                    operations.finish(&action.id, result);
                }
                action_changed = true;
                self.refresh_mcp_control();
            }
        }
        if self.mcp_connections_disabled {
            self.mcp_state
                .operations
                .lock()
                .unwrap()
                .cancel_connection(None);
            self.mcp = None;
            let was_connected = std::mem::take(&mut self.mcp_connected);
            if self.mcp_status != "Disabled" {
                self.mcp_status = "Disabled".into();
                return true;
            }
            return action_changed || was_connected;
        }
        if !self.mcp_started {
            self.mcp_started = true;
            match Backend::start(self.mcp_view(), self.mcp_state.operations.clone()) {
                Ok(backend) => self.mcp = Some(backend),
                Err(_) => {
                    self.mcp_connected = false;
                    self.mcp_status = "Listener unavailable".into();
                    self.status="MCP listener unavailable; retry by disabling and enabling connections in Settings".into();
                    return true;
                }
            }
        }
        if let Some(backend) = &self.mcp {
            let changed = backend.view.try_read().ok().is_some_and(|view| {
                view.snapshot.captured_at != self.snapshot.captured_at
                    || view.activity
                        != match self.activity() {
                            AppActivity::Live => "live",
                            AppActivity::Recording => "recording",
                            AppActivity::LogView => "log_view",
                        }
                    || view.tracked_names != self.watch_list
                    || view.control != self.mcp_control()
                    || view.log_root != self.recording_last_dir
            });
            if changed {
                let next = Arc::new(self.mcp_view());
                if let Ok(mut view) = backend.view.try_write() {
                    *view = next;
                }
            }
            if let Ok(status) = backend.status.try_lock()
                && (self.mcp_status != status.label || self.mcp_connected != status.connected())
            {
                self.mcp_status = status.label.clone();
                self.mcp_connected = status.connected();
                return true;
            }
        }
        action_changed
    }
}

impl App {
    fn refresh_mcp_control(&mut self) {
        self.mcp_state
            .epoch
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string());
        let fingerprint = json!({"names":self.watch_list,"profiles":self.runtime.saved_investigation_profiles,"active":self.active_investigation_profile,"dotnet_generation":self.sampling_worker.dotnet.generation(),"dotnet_authorized":({let mut ids:Vec<_>=self.snapshot.processes.iter().map(crate::model::ProcessIdentity::from_row).filter(|id|self.sampling_worker.dotnet.allows(id)).map(|id|format!("{}:{:?}",id.pid,id.start_time)).collect();ids.sort();ids})});
        if fingerprint != self.mcp_state.fingerprint {
            self.mcp_state.revision += 1;
            self.mcp_state.fingerprint = fingerprint;
        }
        self.mcp_state.profile_ids.retain(|name, _| {
            self.runtime
                .saved_investigation_profiles
                .iter()
                .any(|p| p.name == *name)
        });
        for profile in &self.runtime.saved_investigation_profiles {
            self.mcp_state
                .profile_ids
                .entry(profile.name.clone())
                .or_insert_with(|| uuid::Uuid::new_v4().to_string());
        }
        let key = self
            .recording_session
            .as_ref()
            .map(|s| (s.path.to_string_lossy().into_owned(), s.started_at_instant));
        if key != self.mcp_state.recording_key {
            self.mcp_state.recording_id = key.as_ref().map(|_| uuid::Uuid::new_v4().to_string());
            self.mcp_state.recording_key = key;
        }
    }
    fn mcp_revision(&self) -> String {
        format!(
            "{}:{}",
            self.mcp_state.epoch.as_deref().unwrap_or("initial"),
            self.mcp_state.revision
        )
    }
    fn mcp_control(&self) -> Value {
        let revision = self.mcp_revision();
        let profiles:Vec<_>=self.runtime.saved_investigation_profiles.iter().map(|p|json!({"profile_id":self.mcp_state.profile_ids.get(&p.name),"name":p.name,"tracked_names":p.tracked_names,"revision":revision})).collect();
        let dotnet:Vec<_>=self.snapshot.processes.iter().map(|row|{
            let id=crate::model::ProcessIdentity::from_row(row);
            let control=&self.sampling_worker.dotnet;
            let detail=control.status(&id);
            let state=if control.target_stopping(&id){"stopping"}else if control.is_collecting(&id){"collecting"}else if control.allows(&id){"waiting"}else if detail.starts_with("Unavailable"){"unavailable"}else{"disabled"};
            json!({"pid":id.pid,"start_time":id.start_time,"enabled":control.allows(&id),"supported":control.supports_metrics(&id),"state":state,"detail":detail,"revision":revision})
        }).collect();
        json!({"revision":revision,"profiles":profiles,"active_profile":self.active_investigation_profile,"dotnet":dotnet,"recording":{"active":self.recording_session.is_some(),"recording_session_id":self.mcp_state.recording_id,"interval_seconds":self.active_recording_interval_seconds(),"tracked_names":if self.recording_session.is_some(){self.watch_list.clone()}else{vec![]},"output_log_id":self.recording_session.as_ref().map(|s|crate::mcp::backend::log_id(&s.path)),"output_configured":self.recording_last_dir.is_some()}})
    }
    pub(crate) fn resolve_mcp_confirmation(&mut self, accepted: bool) {
        let Some((action, _)) = self.mcp_state.confirmation.take() else {
            return;
        };
        self.refresh_mcp_control();
        let hub = self.mcp_state.operations.clone();
        let mut operations = hub.lock().unwrap();
        if operations.active.as_deref() != Some(&action.id) {
            return;
        }
        let result = if !accepted {
            error("CANCELLED", "Local user declined the operation", false)
        } else if action.created.elapsed().as_secs() >= 60 {
            error("TIMEOUT", "Confirmation expired", false)
        } else {
            self.apply_mcp_action(&action, true)
        };
        operations.finish(&action.id, result);
        self.refresh_mcp_control();
    }
    fn apply_mcp_action(&mut self, action: &Action, confirmed: bool) -> Value {
        if self.mcp_connections_disabled {
            return error("CANCELLED", "Operation cancelled", false);
        }
        let args = &action.args;
        if action.method != "winproc_stop_recording"
            && args["expected_revision"].as_str() != Some(&self.mcp_revision())
        {
            return error(
                "STATE_CONFLICT",
                "Application state changed; inspect current revision",
                false,
            );
        }
        if let Some(identity) = &action.identity
            && action.method == "winproc_set_dotnet_collection"
            && !self
                .snapshot
                .processes
                .iter()
                .any(|row| crate::model::ProcessIdentity::from_row(row) == *identity)
        {
            return error("PROCESS_EXITED", "Target lifetime no longer exists", false);
        }
        let action_name = args["action"].as_str().unwrap_or("");
        let requires_live = matches!(
            action.method.as_str(),
            "winproc_update_tracking" | "winproc_start_recording"
        ) || (action.method == "winproc_update_profile"
            && matches!(action_name, "load" | "save" | "save_as"));
        if requires_live && self.activity() != AppActivity::Live {
            return error(
                "ACTIVITY_CONFLICT",
                "Operation requires Live activity",
                false,
            );
        }
        match action.method.as_str() {
            "winproc_update_tracking" => {
                let name = args["process_name"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| action.identity.as_ref().map(|id| id.name.clone()))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if name.is_empty() || name.contains(['/', '\\', '\n', '\r']) {
                    return error(
                        "INVALID_ARGUMENT",
                        "process_name must be an executable name",
                        false,
                    );
                }
                let mut names = self.watch_list.clone();
                if action_name == "add" {
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        names.push(name.clone());
                    }
                } else {
                    names.retain(|n| !n.eq_ignore_ascii_case(&name));
                }
                let pending = self.prepare_tracked_list_switch(names);
                if pending.discarded_sample_count > 0 && !confirmed {
                    return json!({"confirmation":format!("Remove {name}: discard {} older samples; keep latest 120 per lifetime.",pending.discarded_sample_count)});
                }
                self.apply_tracked_list_switch(pending);
                self.refresh_mcp_control();
                json!({"tracked_names":self.watch_list,"revision":self.mcp_revision(),"scope":"all current and future matching executable names"})
            }
            "winproc_update_profile" => self.apply_mcp_profile(action, confirmed),
            "winproc_start_recording" => {
                if self.watch_list.is_empty() {
                    return error(
                        "INVALID_ARGUMENT",
                        "Add a Tracking List entry before recording",
                        false,
                    );
                }
                let Some(root) = self.recording_last_dir.clone() else {
                    return error(
                        "INVALID_ARGUMENT",
                        "Configure the recording directory locally by recording once in the TUI",
                        false,
                    );
                };
                let interval = args["interval_seconds"].as_u64().unwrap();
                self.select_recording_interval(
                    super::export::RECORDING_INTERVAL_OPTIONS_SECONDS
                        .iter()
                        .position(|v| *v == interval)
                        .unwrap(),
                );
                let path = root.join(format!("winproc-mcp-{}.log", uuid::Uuid::new_v4()));
                if self.start_recording(path, false).is_err() || self.recording_session.is_none() {
                    return error(
                        "INTERNAL_ERROR",
                        "Recording could not start; inspect the local error dialog",
                        false,
                    );
                }
                self.refresh_mcp_control();
                self.mcp_control()["recording"].clone()
            }
            "winproc_stop_recording" => {
                if self.mcp_state.recording_id.as_deref() != args["recording_session_id"].as_str()
                    || self.recording_session.is_none()
                {
                    return error(
                        "STATE_CONFLICT",
                        "Recording session no longer matches",
                        false,
                    );
                }
                if !confirmed {
                    return json!({"confirmation":"Stop the requested recording? Pending samples will be flushed and the log closed."});
                }
                let path = self.recording_session.as_ref().unwrap().path.clone();
                if let Err(e) = self.stop_recording() {
                    self.present_active_recording_error(path, e);
                    return error(
                        "INTERNAL_ERROR",
                        "Recording cleanup failed; partial log preserved",
                        false,
                    );
                }
                json!({"stopped":true,"recording_session_id":args["recording_session_id"]})
            }
            "winproc_set_dotnet_collection" => {
                if self.activity() == AppActivity::LogView {
                    return error(
                        "ACTIVITY_CONFLICT",
                        "Live collection is unavailable in Log view",
                        false,
                    );
                }
                let Some(id) = &action.identity else {
                    return error("REFERENCE_INVALID", "Missing process identity", false);
                };
                if args["enabled"] == true {
                    if !self.sampling_worker.dotnet.supports_metrics(id) {
                        return error("UNSUPPORTED", "Supported .NET runtime not detected", false);
                    }
                    if !confirmed {
                        return json!({"confirmation":format!("Enable .NET metrics for {} (PID {})? Confirm the application has initialized. Attaching during startup can hang the target (dotnet/runtime #119014).",id.name,id.pid)});
                    }
                    if !self.sampling_worker.dotnet.enable_target(id.clone()) {
                        return error("BUSY", "Collection cleanup is still pending", true);
                    }
                } else {
                    self.sampling_worker.stop_dotnet_target(id);
                    for row in &mut self.snapshot.processes {
                        if crate::model::ProcessIdentity::from_row(row) == *id {
                            crate::samplers::dotnet_control::clear_runtime_values(row);
                        }
                    }
                }
                json!({"enabled":self.sampling_worker.dotnet.allows(id),"state":self.sampling_worker.dotnet.status(id)})
            }
            _ => error("INVALID_ARGUMENT", "Unknown application operation", false),
        }
    }
    fn apply_mcp_profile(&mut self, action: &Action, confirmed: bool) -> Value {
        let a = &action.args;
        let kind = a["action"].as_str().unwrap();
        let index = a["profile_id"].as_str().and_then(|id| {
            self.runtime
                .saved_investigation_profiles
                .iter()
                .position(|p| {
                    self.mcp_state
                        .profile_ids
                        .get(&p.name)
                        .is_some_and(|v| v == id)
                })
        });
        if kind != "save_as"
            && (index.is_none()
                || a["expected_profile_revision"].as_str() != Some(&self.mcp_revision()))
        {
            return error(
                "STATE_CONFLICT",
                "Inspect profiles and provide current profile ID and revision",
                false,
            );
        }
        let name = a["name"].as_str().unwrap_or("").trim();
        if matches!(kind, "rename" | "save_as")
            && (name.is_empty()
                || name.chars().any(char::is_control)
                || self
                    .runtime
                    .saved_investigation_profiles
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(name)))
        {
            return error(
                "STATE_CONFLICT",
                "Choose a nonempty unique profile name",
                false,
            );
        }
        let previous = self.runtime.saved_investigation_profiles.clone();
        let previous_active = self.active_investigation_profile.clone();
        let previous_names = self.watch_list.clone();
        match kind {
            "load" => {
                let p = previous[index.unwrap()].clone();
                let pending = self.prepare_tracked_list_switch(p.tracked_names.clone());
                if !confirmed
                    && (pending.discarded_sample_count > 0
                        || (self.active_investigation_profile_dirty()
                            && self.watch_list != p.tracked_names))
                {
                    return json!({"confirmation":format!("Load profile {} with names {:?}? Replace current tracking {:?}; discard {} older samples and unsaved investigation changes.",p.name,p.tracked_names,self.watch_list,pending.discarded_sample_count)});
                }
                self.active_investigation_profile = Some(p.name.clone());
                // Persist before pruning so a write failure cannot discard retained history.
                self.watch_list = p.tracked_names.clone();
                if !self.persist_investigation_profile_changes() {
                    self.watch_list = previous_names;
                    self.active_investigation_profile = previous_active;
                    return error("INTERNAL_ERROR", "Profile persistence failed", false);
                }
                self.watch_list = previous_names;
                self.apply_tracked_list_switch(pending);
            }
            "save" => {
                let i = index.unwrap();
                if self.active_investigation_profile.as_deref() != Some(&previous[i].name) {
                    return error(
                        "STATE_CONFLICT",
                        "Save applies only to the active profile",
                        false,
                    );
                }
                self.runtime.saved_investigation_profiles[i].investigation =
                    self.capture_investigation_state();
            }
            "save_as" => {
                self.runtime.saved_investigation_profiles.push(
                    crate::config::SavedInvestigationProfile {
                        name: name.into(),
                        investigation: self.capture_investigation_state(),
                    },
                );
                self.active_investigation_profile = Some(name.into());
            }
            "rename" => {
                let i = index.unwrap();
                if self.active_investigation_profile.as_deref() == Some(&previous[i].name) {
                    self.active_investigation_profile = Some(name.into());
                }
                self.runtime.saved_investigation_profiles[i].name = name.into();
            }
            "delete" => {
                let i = index.unwrap();
                if !confirmed {
                    return json!({"confirmation":format!("Delete saved Investigation Profile {}?",previous[i].name)});
                }
                self.runtime.saved_investigation_profiles.remove(i);
                if self.active_investigation_profile.as_deref() == Some(&previous[i].name) {
                    self.active_investigation_profile = None;
                }
            }
            _ => return error("INVALID_ARGUMENT", "Unknown profile action", false),
        }
        if kind != "load" && !self.persist_investigation_profile_changes() {
            self.runtime.saved_investigation_profiles = previous;
            self.active_investigation_profile = previous_active;
            return error("INTERNAL_ERROR", "Profile persistence failed", false);
        }
        self.refresh_mcp_control();
        json!({"profiles":self.mcp_control()["profiles"],"revision":self.mcp_revision()})
    }
}

#[cfg(test)]
impl App {
    pub(crate) fn mcp_test_view(&self) -> View {
        self.mcp_view()
    }

    pub(crate) fn mcp_test_revision(&self) -> String {
        self.mcp_revision()
    }
}
