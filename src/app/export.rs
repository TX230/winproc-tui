use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde_json::{Map, Value, json};

use crate::app::{
    App, AppActivity, RecordingOverwriteSelection, RecordingPathSelection,
    path_completion::PathCompletion,
};
use crate::model::{ProcessRow, Snapshot};

pub(crate) struct RecordingSession {
    pub(crate) path: PathBuf,
    session_id: String,
    started_at: DateTime<Local>,
    host: String,
    writer: BufWriter<File>,
}

impl App {
    pub(crate) fn toggle_recording(&mut self) -> Result<()> {
        match self.activity() {
            AppActivity::Recording => self.stop_recording(),
            AppActivity::Playback => {
                self.status = "Recording is unavailable during playback".to_string();
                Ok(())
            }
            AppActivity::Live => {
                if self.watch_list.is_empty() {
                    self.show_recording_no_tracked_warning = true;
                    self.status = "No tracked processes to record".to_string();
                    return Ok(());
                }
                self.open_recording_path_dialog()
            }
        }
    }

    pub(crate) fn dismiss_recording_no_tracked_warning(&mut self) {
        self.show_recording_no_tracked_warning = false;
        self.ensure_visible_panel_focus();
        self.status = "Recording canceled".to_string();
    }

    pub(crate) fn open_recording_path_dialog(&mut self) -> Result<()> {
        if let Some(session) = &self.recording_session {
            self.status = format!("Recording already active: {}", session.path.display());
            return Ok(());
        }

        let path = default_recording_path(self.recording_last_dir.as_deref())?;
        self.recording_path_draft = path.display().to_string();
        self.recording_path_cursor = self.recording_path_draft.len();
        self.recording_path_completion.reset();
        self.recording_path_selection = RecordingPathSelection::Start;
        self.show_recording_path_dialog = true;
        self.show_recording_overwrite_confirmation = false;
        self.recording_overwrite_selection = RecordingOverwriteSelection::Cancel;
        self.status = "Choose recording log path".to_string();
        Ok(())
    }

    pub(crate) fn cancel_recording_path_dialog(&mut self) {
        self.show_recording_path_dialog = false;
        self.show_recording_overwrite_confirmation = false;
        self.recording_path_completion.reset();
        self.recording_path_selection = RecordingPathSelection::Start;
        self.ensure_visible_panel_focus();
        self.status = "Recording canceled".to_string();
    }

    pub(crate) fn push_recording_path_char(&mut self, ch: char) {
        self.recording_path_draft
            .insert(self.recording_path_cursor, ch);
        self.recording_path_cursor += ch.len_utf8();
    }

    pub(crate) fn pop_recording_path_char(&mut self) {
        if self.recording_path_cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.recording_path_draft[..self.recording_path_cursor]
            .char_indices()
            .next_back()
        {
            self.recording_path_draft.remove(index);
            self.recording_path_cursor = index;
        }
    }

    pub(crate) fn delete_recording_path_char(&mut self) {
        if self.recording_path_cursor >= self.recording_path_draft.len() {
            return;
        }
        self.recording_path_draft.remove(self.recording_path_cursor);
    }

    pub(crate) fn move_recording_path_cursor_left(&mut self) {
        if self.recording_path_cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.recording_path_draft[..self.recording_path_cursor]
            .char_indices()
            .next_back()
        {
            self.recording_path_cursor = index;
        }
    }

    pub(crate) fn move_recording_path_cursor_right(&mut self) {
        if self.recording_path_cursor >= self.recording_path_draft.len() {
            return;
        }
        let next = self.recording_path_draft[self.recording_path_cursor..]
            .chars()
            .next()
            .map(|ch| self.recording_path_cursor + ch.len_utf8())
            .unwrap_or(self.recording_path_draft.len());
        self.recording_path_cursor = next;
    }

    pub(crate) fn move_recording_path_cursor_home(&mut self) {
        self.recording_path_cursor = 0;
    }

    pub(crate) fn move_recording_path_cursor_end(&mut self) {
        self.recording_path_cursor = self.recording_path_draft.len();
    }

    pub(crate) fn complete_recording_path(&mut self) {
        match self
            .recording_path_completion
            .complete_directory_path(&self.recording_path_draft, self.recording_path_cursor)
        {
            PathCompletion::None => {
                self.status = "No directory completion match".to_string();
            }
            PathCompletion::Replaced {
                value,
                cursor,
                match_count,
                candidate_index,
            } => {
                self.recording_path_draft = value;
                self.recording_path_cursor = cursor;
                self.status = if match_count == 1 {
                    "Completed directory".to_string()
                } else {
                    format!(
                        "Completed directory ({}/{match_count})",
                        candidate_index + 1
                    )
                };
            }
        }
    }

    pub(crate) fn confirm_recording_path(&mut self) -> Result<()> {
        let draft = self.recording_path_draft.trim();
        if draft.is_empty() {
            self.status = "Recording path is empty".to_string();
            return Ok(());
        }

        let path = PathBuf::from(draft);
        if path.exists() {
            self.show_recording_overwrite_confirmation = true;
            self.recording_overwrite_selection = RecordingOverwriteSelection::Cancel;
            self.status = format!("Overwrite existing log? {}", path.display());
            return Ok(());
        }

        self.start_recording(path, false)
    }

    pub(crate) fn activate_recording_path_selection(&mut self) -> Result<()> {
        match self.recording_path_selection {
            RecordingPathSelection::Start => self.confirm_recording_path(),
            RecordingPathSelection::Cancel => {
                self.cancel_recording_path_dialog();
                Ok(())
            }
        }
    }

    pub(crate) fn toggle_recording_overwrite_selection(&mut self) {
        self.recording_overwrite_selection = self.recording_overwrite_selection.toggled();
    }

    pub(crate) fn cancel_recording_overwrite_confirmation(&mut self) {
        self.show_recording_overwrite_confirmation = false;
        self.recording_overwrite_selection = RecordingOverwriteSelection::Cancel;
        self.ensure_visible_panel_focus();
        self.status = "Overwrite canceled".to_string();
    }

    pub(crate) fn confirm_recording_overwrite(&mut self) -> Result<()> {
        let path = PathBuf::from(self.recording_path_draft.trim());
        self.start_recording(path, true)
    }

    pub(crate) fn activate_recording_overwrite_selection(&mut self) -> Result<()> {
        match self.recording_overwrite_selection {
            RecordingOverwriteSelection::Overwrite => self.confirm_recording_overwrite(),
            RecordingOverwriteSelection::Cancel => {
                self.cancel_recording_overwrite_confirmation();
                Ok(())
            }
        }
    }

    pub(crate) fn stop_recording(&mut self) -> Result<()> {
        let Some(mut session) = self.recording_session.take() else {
            self.status = "Recording is not active".to_string();
            return Ok(());
        };
        let line = recording_end_line(&session, "stopped")?;
        session
            .writer
            .write_all(line.as_bytes())
            .with_context(|| format!("failed to write {}", session.path.display()))?;
        session
            .writer
            .write_all(b"\n")
            .with_context(|| format!("failed to write {}", session.path.display()))?;
        session
            .writer
            .flush()
            .with_context(|| format!("failed to flush {}", session.path.display()))?;
        self.status = format!("Saved log to: {}", session.path.display());
        Ok(())
    }

    pub(crate) fn write_current_recording_frame(&mut self) -> Result<()> {
        let Some(session) = &self.recording_session else {
            return Ok(());
        };
        let line = recording_frame_line(
            session,
            &self.snapshot,
            &self.watch_list,
            &self.normalized_watch_names,
        )?;
        let session = self
            .recording_session
            .as_mut()
            .expect("recording session exists");
        session
            .writer
            .write_all(line.as_bytes())
            .with_context(|| format!("failed to write {}", session.path.display()))?;
        session
            .writer
            .write_all(b"\n")
            .with_context(|| format!("failed to write {}", session.path.display()))
    }

    fn start_recording(&mut self, path: PathBuf, overwrite: bool) -> Result<()> {
        match open_recording_file(&path, overwrite) {
            Ok(file) => {
                let started_at = Local::now();
                let session_id = started_at.format("%Y%m%d%H%M%S").to_string();
                let host = host_name();
                self.recording_session = Some(RecordingSession {
                    path: path.clone(),
                    session_id,
                    started_at,
                    host,
                    writer: BufWriter::new(file),
                });
                self.recording_spinner_index = 0;
                self.recording_last_dir = recording_parent_dir(&path)?;
                self.show_recording_path_dialog = false;
                self.show_recording_overwrite_confirmation = false;
                self.recording_path_completion.reset();
                self.recording_path_selection = RecordingPathSelection::Start;

                if let Err(error) = self.write_recording_session_header() {
                    self.recording_session = None;
                    self.status = format!("Recording stopped: {error}");
                    return Ok(());
                }
                if let Err(error) = self.write_current_recording_frame() {
                    self.recording_session = None;
                    self.status = format!("Recording stopped: {error}");
                    return Ok(());
                }

                self.status = format!("Recording started: {}", path.display());
            }
            Err(error) => {
                self.recording_session = None;
                self.status = format!("Recording failed: {error}");
            }
        }
        Ok(())
    }

    fn write_recording_session_header(&mut self) -> Result<()> {
        let Some(session) = &self.recording_session else {
            return Ok(());
        };
        let line = recording_session_line(session, self)?;
        let session = self
            .recording_session
            .as_mut()
            .expect("recording session exists");
        session
            .writer
            .write_all(line.as_bytes())
            .with_context(|| format!("failed to write {}", session.path.display()))?;
        session
            .writer
            .write_all(b"\n")
            .with_context(|| format!("failed to write {}", session.path.display()))
    }
}

fn open_recording_file(path: &Path, overwrite: bool) -> Result<File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut options = OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    options
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn default_recording_path(last_dir: Option<&Path>) -> Result<PathBuf> {
    let filename = default_recording_filename(Local::now());
    let dir = match last_dir {
        Some(path) => path.to_path_buf(),
        None => env::current_dir().context("failed to resolve current directory")?,
    };
    Ok(dir.join(filename))
}

fn default_recording_filename(now: DateTime<Local>) -> String {
    format!("winproc-tui-{}.log", now.format("%Y%m%d%H%M%S"))
}

fn recording_parent_dir(path: &Path) -> Result<Option<PathBuf>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    match parent {
        Some(parent) if parent.is_absolute() => Ok(Some(parent.to_path_buf())),
        Some(parent) => Ok(Some(
            env::current_dir()
                .context("failed to resolve current directory")?
                .join(parent),
        )),
        None => Ok(Some(
            env::current_dir().context("failed to resolve current directory")?,
        )),
    }
}

fn recording_frame_line(
    session: &RecordingSession,
    snapshot: &Snapshot,
    tracked_names: &[String],
    normalized_tracked_names: &std::collections::HashSet<String>,
) -> Result<String> {
    let processes = snapshot
        .processes
        .iter()
        .filter(|process| normalized_tracked_names.contains(&process.name.to_ascii_lowercase()))
        .map(process_json)
        .collect::<Vec<_>>();

    let frame = json!({
        "schema_version": 2,
        "record_type": "frame",
        "session_id": session.session_id,
        "captured_at": snapshot.captured_at.to_rfc3339(),
        "tracked_names": tracked_names,
        "system_metrics": system_metrics_json(snapshot),
        "processes": processes,
    });
    serde_json::to_string(&frame).context("failed to serialize recording frame")
}

fn recording_session_line(session: &RecordingSession, app: &App) -> Result<String> {
    let snapshot = app.display_snapshot();
    let columns = app
        .process_columns
        .iter()
        .map(|column| column.label())
        .collect::<Vec<_>>();
    let frame = json!({
        "schema_version": 2,
        "record_type": "session",
        "session_id": session.session_id,
        "winproc_tui_version": env!("CARGO_PKG_VERSION"),
        "host": session.host,
        "started_at": session.started_at.to_rfc3339(),
        "interval_seconds": app.runtime.interval_seconds,
        "tracked_names": &app.watch_list,
        "columns": columns,
        "sort": {
            "column": app.sort.column.label(),
            "direction": match app.sort.direction {
                crate::model::SortDirection::Asc => "asc",
                crate::model::SortDirection::Desc => "desc",
            },
        },
        "system": {
            "cpu_name": snapshot.cpu_name.as_deref(),
            "cpu_frequency_mhz": snapshot.cpu_frequency_mhz,
            "cpu_topology": snapshot.cpu_topology.as_deref(),
            "cpu_cache": snapshot.cpu_cache.as_deref(),
            "gpu_name": snapshot.gpu_name.as_deref(),
        },
    });
    serde_json::to_string(&frame).context("failed to serialize recording session")
}

fn recording_end_line(session: &RecordingSession, reason: &str) -> Result<String> {
    let frame = json!({
        "schema_version": 2,
        "record_type": "end",
        "session_id": session.session_id,
        "ended_at": Local::now().to_rfc3339(),
        "reason": reason,
    });
    serde_json::to_string(&frame).context("failed to serialize recording end")
}

fn system_metrics_json(snapshot: &Snapshot) -> Value {
    let mut metrics = Map::new();
    metrics.insert(
        "physical_memory_bytes".to_string(),
        json!(snapshot.used_memory),
    );
    metrics.insert(
        "total_memory_bytes".to_string(),
        json!(snapshot.total_memory),
    );
    insert_u64(&mut metrics, "committed_bytes", snapshot.committed_memory);
    insert_u64(&mut metrics, "commit_limit_bytes", snapshot.commit_limit);
    insert_u64(
        &mut metrics,
        "gpu_dedicated_bytes",
        snapshot.gpu_dedicated_used,
    );
    insert_u64(
        &mut metrics,
        "gpu_dedicated_total_bytes",
        snapshot.gpu_dedicated_total,
    );
    insert_u64(&mut metrics, "gpu_shared_bytes", snapshot.gpu_shared_used);
    insert_u64(
        &mut metrics,
        "gpu_shared_total_bytes",
        snapshot.gpu_shared_total,
    );
    insert_u64(
        &mut metrics,
        "cpu_percent",
        snapshot.cpu_total_usage_percent.map(u64::from),
    );
    Value::Object(metrics)
}

fn process_json(process: &ProcessRow) -> Value {
    let mut object = Map::new();
    object.insert("pid".to_string(), json!(process.pid));
    object.insert("name".to_string(), json!(process.name));
    if let Some(start_time) = process.start_time {
        object.insert("start_time".to_string(), json!(start_time));
    }
    object.insert("metrics".to_string(), Value::Object(metrics_json(process)));
    Value::Object(object)
}

fn metrics_json(process: &ProcessRow) -> Map<String, Value> {
    let mut metrics = Map::new();
    insert_f64(&mut metrics, "cpu_percent", process.cpu_percent);
    insert_u64(&mut metrics, "private_bytes", process.private_bytes);
    insert_u64(&mut metrics, "workset_bytes", process.workset_bytes);
    insert_u64(
        &mut metrics,
        "workset_private_bytes",
        process.workset_private_bytes,
    );
    insert_u64(
        &mut metrics,
        "workset_shareable_bytes",
        process.workset_shareable_bytes,
    );
    insert_u64(
        &mut metrics,
        "workset_shared_bytes",
        process.workset_shared_bytes,
    );
    insert_u64(&mut metrics, "thread_count", process.thread_count);
    insert_u64(&mut metrics, "handle_count", process.handle_count);
    insert_u64(&mut metrics, "user_object_count", process.user_object_count);
    insert_u64(&mut metrics, "gdi_object_count", process.gdi_object_count);
    insert_f64(&mut metrics, "gpu_percent", process.gpu_percent);
    insert_u64(&mut metrics, "dotnet_heap_bytes", process.dotnet_heap_bytes);
    insert_u64(
        &mut metrics,
        "gpu_dedicated_bytes",
        process.gpu_dedicated_bytes,
    );
    insert_u64(&mut metrics, "gpu_shared_bytes", process.gpu_shared_bytes);
    insert_u64(
        &mut metrics,
        "io_read_bytes_per_sec",
        process.io_read_bytes_per_sec,
    );
    insert_u64(
        &mut metrics,
        "io_write_bytes_per_sec",
        process.io_write_bytes_per_sec,
    );
    metrics
}

fn insert_u64(metrics: &mut Map<String, Value>, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        metrics.insert(name.to_string(), json!(value));
    }
}

fn insert_f64(metrics: &mut Map<String, Value>, name: &str, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        metrics.insert(name.to_string(), json!(value));
    }
}

fn host_name() -> String {
    env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashSet;
    use std::fs::OpenOptions;

    #[test]
    fn default_recording_filename_uses_compact_timestamp() {
        let now = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();

        assert_eq!(
            default_recording_filename(now),
            "winproc-tui-20260504143012.log"
        );
    }

    #[test]
    fn recording_frame_contains_tracked_processes_only() {
        let now = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let session = RecordingSession {
            path: PathBuf::from("test.log"),
            session_id: "20260504143012".to_string(),
            started_at: now,
            host: "PC01".to_string(),
            writer: BufWriter::new(
                OpenOptions::new()
                    .write(true)
                    .open(if cfg!(windows) { "NUL" } else { "/dev/null" })
                    .unwrap(),
            ),
        };
        let snapshot = Snapshot {
            captured_at: now,
            total_memory: 0,
            used_memory: 0,
            committed_memory: None,
            commit_limit: None,
            gpu_dedicated_used: None,
            gpu_dedicated_total: None,
            gpu_shared_used: None,
            gpu_shared_total: None,
            cpu_name: None,
            cpu_frequency_mhz: None,
            cpu_current_frequency_mhz: None,
            cpu_p_core_frequency_mhz: None,
            cpu_e_core_frequency_mhz: None,
            cpu_total_usage_percent: Some(37),
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_name: None,
            disks: Vec::new(),
            process_count: 2,
            processes: vec![
                row(1, "app.exe", Some(120), None),
                row(2, "other.exe", Some(999), None),
            ],
        };
        let tracked_names = vec!["app.exe".to_string()];
        let normalized = HashSet::from(["app.exe".to_string()]);

        let line = recording_frame_line(&session, &snapshot, &tracked_names, &normalized).unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();

        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["record_type"], "frame");
        assert_eq!(value["session_id"], "20260504143012");
        assert_eq!(value["tracked_names"][0], "app.exe");
        assert_eq!(value["processes"].as_array().unwrap().len(), 1);
        assert_eq!(value["processes"][0]["name"], "app.exe");
        assert_eq!(value["processes"][0]["metrics"]["private_bytes"], 120);
        assert!(value["processes"][0]["metrics"]["handle_count"].is_null());
        assert_eq!(value["system_metrics"]["physical_memory_bytes"], 0);
        assert_eq!(value["system_metrics"]["cpu_percent"], 37);
    }

    #[test]
    fn metrics_omit_missing_values() {
        let metrics = metrics_json(&row(1, "app.exe", Some(120), None));

        assert!(metrics.contains_key("private_bytes"));
        assert!(!metrics.contains_key("handle_count"));
    }

    fn row(
        pid: u32,
        name: &str,
        private_bytes: Option<u64>,
        handle_count: Option<u64>,
    ) -> ProcessRow {
        ProcessRow {
            pid,
            name: name.to_string(),
            start_time: Some(1000 + pid as u64),
            cpu_percent: None,
            private_bytes,
            workset_bytes: None,
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            workset_shared_bytes: None,
            thread_count: None,
            handle_count,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            dotnet_heap_bytes: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }
}
