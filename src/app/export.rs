use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local};

use crate::app::{
    App, AppActivity,
    log_format::{
        CURRENT_LOG_SCHEMA_VERSION, GPU_F64_FIELD_COUNT, GPU_U64_FIELD_COUNT,
        PROCESS_F64_FIELD_COUNT, PROCESS_U64_FIELD_COUNT, SYSTEM_U64_FIELD_COUNT, V3EndRecord,
        V3FrameRecord, V3GpuDefinition, V3GpuSample, V3ProcessDefinition, V3ProcessSample,
        V3Record, V3SessionRecord, V3SessionSystem, V3SystemMetrics,
    },
    path_completion::PathCompletion,
    state::{RecordingDialogFocus, RecordingErrorDialog, RecordingErrorKind},
};
use crate::model::{GpuAdapterId, GpuAdapterSample, ProcessIdentity, ProcessRow, Snapshot};

pub(crate) const MAX_RECORDING_DURATION: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const RECORDING_INTERVAL_OPTIONS_SECONDS: [u64; 4] = [1, 2, 5, 10];
const RECORDING_DURATION_LIMIT_REASON: &str = "duration_limit";

#[derive(Debug, Default)]
struct U64Mean {
    sum: u128,
    count: u64,
}

impl U64Mean {
    fn add(&mut self, value: Option<u64>) {
        let Some(value) = value else {
            return;
        };
        self.sum = self.sum.saturating_add(u128::from(value));
        self.count = self.count.saturating_add(1);
    }

    fn finish(self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let count = u128::from(self.count);
        let rounded = self.sum.saturating_add(count / 2) / count;
        u64::try_from(rounded).ok()
    }
}

#[derive(Debug, Default)]
struct F64Mean {
    mean: f64,
    count: u64,
}

impl F64Mean {
    fn add(&mut self, value: Option<f64>) {
        let Some(value) = value.filter(|value| value.is_finite()) else {
            return;
        };
        self.count = self.count.saturating_add(1);
        self.mean += (value - self.mean) / self.count as f64;
    }

    fn finish(self) -> Option<f64> {
        (self.count > 0 && self.mean.is_finite()).then_some(self.mean)
    }
}

#[derive(Debug)]
struct ProcessAggregate {
    latest: ProcessRow,
    floats: [F64Mean; PROCESS_F64_FIELD_COUNT],
    integers: [U64Mean; PROCESS_U64_FIELD_COUNT],
}

impl ProcessAggregate {
    fn new(process: &ProcessRow) -> Self {
        let mut aggregate = Self {
            latest: process.clone(),
            floats: Default::default(),
            integers: Default::default(),
        };
        aggregate.add(process);
        aggregate
    }

    fn add(&mut self, process: &ProcessRow) {
        self.latest = process.clone();
        let V3ProcessSample(_, floats, integers) = V3ProcessSample::from_row(0, process);
        for (aggregate, value) in self.floats.iter_mut().zip(floats) {
            aggregate.add(value);
        }
        for (aggregate, value) in self.integers.iter_mut().zip(integers) {
            aggregate.add(value);
        }
    }

    fn finish(self, process_id: u32) -> V3ProcessSample {
        V3ProcessSample(
            process_id,
            self.floats.map(F64Mean::finish),
            self.integers.map(U64Mean::finish),
        )
    }
}

#[derive(Debug)]
struct GpuAggregate {
    latest: GpuAdapterSample,
    floats: [F64Mean; GPU_F64_FIELD_COUNT],
    integers: [U64Mean; GPU_U64_FIELD_COUNT],
}

impl GpuAggregate {
    fn new(adapter: &GpuAdapterSample) -> Self {
        let mut aggregate = Self {
            latest: adapter.clone(),
            floats: Default::default(),
            integers: Default::default(),
        };
        aggregate.add(adapter);
        aggregate
    }

    fn add(&mut self, adapter: &GpuAdapterSample) {
        self.latest = adapter.clone();
        let V3GpuSample(_, floats, integers) = V3GpuSample::from_adapter(0, adapter);
        for (aggregate, value) in self.floats.iter_mut().zip(floats) {
            aggregate.add(value);
        }
        for (aggregate, value) in self.integers.iter_mut().zip(integers) {
            aggregate.add(value);
        }
    }

    fn finish(self, adapter_id: u32) -> V3GpuSample {
        V3GpuSample(
            adapter_id,
            self.floats.map(F64Mean::finish),
            self.integers.map(U64Mean::finish),
        )
    }
}

#[derive(Debug, Default)]
struct RecordingFrameAggregate {
    sample_count: u64,
    captured_at_ms: Option<i64>,
    system_integers: [U64Mean; SYSTEM_U64_FIELD_COUNT],
    disk_queue_length: F64Mean,
    gpu_indices: HashMap<GpuAdapterId, usize>,
    gpus: Vec<GpuAggregate>,
    process_indices: HashMap<ProcessIdentity, usize>,
    processes: Vec<ProcessAggregate>,
}

impl RecordingFrameAggregate {
    fn add_snapshot(&mut self, snapshot: &Snapshot, tracked_names: &HashSet<String>) {
        self.sample_count = self.sample_count.saturating_add(1);
        self.captured_at_ms = Some(snapshot.captured_at.timestamp_millis());

        let V3SystemMetrics(integers, disk_queue_length, _) =
            V3SystemMetrics::from_snapshot(snapshot, Vec::new());
        for (aggregate, value) in self.system_integers.iter_mut().zip(integers) {
            aggregate.add(value);
        }
        self.disk_queue_length.add(disk_queue_length);

        for adapter in &snapshot.gpu_adapters {
            if let Some(index) = self.gpu_indices.get(&adapter.id).copied() {
                self.gpus[index].add(adapter);
            } else {
                let index = self.gpus.len();
                self.gpu_indices.insert(adapter.id, index);
                self.gpus.push(GpuAggregate::new(adapter));
            }
        }

        for process in snapshot
            .processes
            .iter()
            .filter(|process| tracked_names.contains(&process.name.to_ascii_lowercase()))
        {
            let identity = ProcessIdentity::from_row(process);
            if let Some(index) = self.process_indices.get(&identity).copied() {
                self.processes[index].add(process);
            } else {
                let index = self.processes.len();
                self.process_indices.insert(identity, index);
                self.processes.push(ProcessAggregate::new(process));
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.sample_count == 0
    }

    fn into_records(self, session: &mut RecordingSession) -> Result<Vec<V3Record>> {
        let captured_at_ms = self
            .captured_at_ms
            .ok_or_else(|| anyhow!("recording aggregate contains no samples"))?;
        let mut records = Vec::new();
        let mut gpu_samples = Vec::with_capacity(self.gpus.len());
        for aggregate in self.gpus {
            let (adapter_id, definition) = session.gpu_id_for(&aggregate.latest)?;
            if let Some(definition) = definition {
                records.push(V3Record::Gpu(definition));
            }
            gpu_samples.push(aggregate.finish(adapter_id));
        }

        let mut process_samples = Vec::with_capacity(self.processes.len());
        for aggregate in self.processes {
            let (process_id, definition) = session.process_id_for(&aggregate.latest)?;
            if let Some(definition) = definition {
                records.push(V3Record::Process(definition));
            }
            process_samples.push(aggregate.finish(process_id));
        }

        records.push(V3Record::Frame(V3FrameRecord(
            captured_at_ms,
            V3SystemMetrics(
                self.system_integers.map(U64Mean::finish),
                self.disk_queue_length.finish(),
                gpu_samples,
            ),
            process_samples,
        )));
        Ok(records)
    }
}

#[derive(Debug)]
struct RegisteredProcess {
    id: u32,
    path: Option<String>,
}

#[derive(Debug)]
struct RegisteredGpu {
    id: u32,
    name: Option<String>,
}

pub(crate) struct RecordingSession {
    pub(crate) path: PathBuf,
    session_id: String,
    started_at: DateTime<Local>,
    started_at_instant: Instant,
    host: String,
    tracked_names: Vec<String>,
    normalized_tracked_names: HashSet<String>,
    interval_seconds: u64,
    pending_frame: RecordingFrameAggregate,
    registered_processes: HashMap<ProcessIdentity, RegisteredProcess>,
    next_process_id: u32,
    registered_gpus: HashMap<GpuAdapterId, RegisteredGpu>,
    next_gpu_id: u32,
    writer: BufWriter<Box<dyn Write>>,
}

impl RecordingSession {
    fn duration_limit_reached_at(&self, now: Instant) -> bool {
        now.checked_duration_since(self.started_at_instant)
            .is_some_and(|elapsed| elapsed >= MAX_RECORDING_DURATION)
    }

    fn process_id_for(
        &mut self,
        process: &ProcessRow,
    ) -> Result<(u32, Option<V3ProcessDefinition>)> {
        let identity = ProcessIdentity::from_row(process);
        if let Some(registered) = self.registered_processes.get_mut(&identity) {
            if registered.path != process.executable_path {
                registered.path = process.executable_path.clone();
                return Ok((
                    registered.id,
                    Some(V3ProcessDefinition(
                        registered.id,
                        process.pid,
                        process.name.clone(),
                        process.start_time,
                        process.executable_path.clone(),
                    )),
                ));
            }
            return Ok((registered.id, None));
        }

        let id = self.next_process_id;
        self.next_process_id = self
            .next_process_id
            .checked_add(1)
            .ok_or_else(|| anyhow!("recording process ID space is exhausted"))?;
        self.registered_processes.insert(
            identity,
            RegisteredProcess {
                id,
                path: process.executable_path.clone(),
            },
        );
        Ok((
            id,
            Some(V3ProcessDefinition(
                id,
                process.pid,
                process.name.clone(),
                process.start_time,
                process.executable_path.clone(),
            )),
        ))
    }

    fn gpu_id_for(
        &mut self,
        adapter: &crate::model::GpuAdapterSample,
    ) -> Result<(u32, Option<V3GpuDefinition>)> {
        if let Some(registered) = self.registered_gpus.get_mut(&adapter.id) {
            if registered.name != adapter.name {
                registered.name = adapter.name.clone();
                return Ok((
                    registered.id,
                    Some(V3GpuDefinition(
                        registered.id,
                        adapter.id.high,
                        adapter.id.low,
                        adapter.name.clone(),
                    )),
                ));
            }
            return Ok((registered.id, None));
        }

        let id = self.next_gpu_id;
        self.next_gpu_id = self
            .next_gpu_id
            .checked_add(1)
            .ok_or_else(|| anyhow!("recording GPU ID space is exhausted"))?;
        self.registered_gpus.insert(
            adapter.id,
            RegisteredGpu {
                id,
                name: adapter.name.clone(),
            },
        );
        Ok((
            id,
            Some(V3GpuDefinition(
                id,
                adapter.id.high,
                adapter.id.low,
                adapter.name.clone(),
            )),
        ))
    }
}

impl App {
    pub(crate) fn toggle_recording(&mut self) -> Result<()> {
        match self.activity() {
            AppActivity::Recording => {
                self.request_recording_stop();
                Ok(())
            }
            AppActivity::LogView => {
                self.status = "Recording is unavailable in Log view".to_string();
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

    pub(crate) fn request_recording_stop(&mut self) {
        if self.recording_session.is_none() {
            self.status = "Recording is not active".to_string();
            return;
        }
        self.show_recording_stop_confirmation = true;
        self.status = "Stop recording?".to_string();
    }

    pub(crate) fn cancel_recording_stop(&mut self) {
        self.show_recording_stop_confirmation = false;
        self.ensure_visible_panel_focus();
        self.status = "Recording continues".to_string();
    }

    pub(crate) fn confirm_recording_stop(&mut self) {
        let Some(path) = self
            .recording_session
            .as_ref()
            .map(|session| session.path.clone())
        else {
            self.cancel_recording_stop();
            return;
        };
        self.show_recording_stop_confirmation = false;
        if let Err(error) = self.stop_recording() {
            self.present_active_recording_error(path, error);
        }
    }

    pub(crate) fn dismiss_recording_tracking_fixed(&mut self) {
        self.show_recording_tracking_fixed = false;
        self.ensure_visible_panel_focus();
        self.status = "Recording continues".to_string();
    }

    pub(crate) fn dismiss_recording_error(&mut self) {
        let return_to_path_dialog = self
            .recording_error
            .take()
            .is_some_and(|error| error.return_to_path_dialog);
        if return_to_path_dialog {
            self.show_recording_path_dialog = true;
            self.recording_path_cursor = self
                .recording_path_cursor
                .min(self.recording_path_draft.len());
            self.status = "Choose recording log path".to_string();
        } else {
            self.ensure_visible_panel_focus();
            self.status = "Recording is not active".to_string();
        }
    }

    fn present_recording_error(
        &mut self,
        path: PathBuf,
        error: anyhow::Error,
        kind: RecordingErrorKind,
        return_to_path_dialog: bool,
    ) {
        self.recording_session = None;
        self.show_quit_confirmation = false;
        self.show_recording_path_dialog = return_to_path_dialog;
        self.show_recording_stop_confirmation = false;
        self.show_recording_tracking_fixed = false;
        self.show_recording_overwrite_confirmation = false;
        self.recording_error = Some(RecordingErrorDialog {
            path,
            message: error.root_cause().to_string(),
            kind,
            return_to_path_dialog,
        });
        self.status = match kind {
            RecordingErrorKind::CouldNotStart => "Recording could not start".to_string(),
            RecordingErrorKind::Stopped => {
                "Recording stopped because the log could not be written".to_string()
            }
        };
    }

    pub(crate) fn present_active_recording_error(&mut self, path: PathBuf, error: anyhow::Error) {
        self.present_recording_error(path, error, RecordingErrorKind::Stopped, false);
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
        self.recording_dialog_focus = RecordingDialogFocus::Path;
        self.recording_interval_index = 0;
        self.show_recording_path_dialog = true;
        self.show_recording_overwrite_confirmation = false;
        self.status = "Choose recording log path".to_string();
        Ok(())
    }

    pub(crate) fn cancel_recording_path_dialog(&mut self) {
        self.show_recording_path_dialog = false;
        self.show_recording_overwrite_confirmation = false;
        self.recording_path_completion.reset();
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

    pub(crate) fn recording_path_focused(&self) -> bool {
        self.recording_dialog_focus == RecordingDialogFocus::Path
    }

    pub(crate) fn recording_interval_focused(&self) -> bool {
        self.recording_dialog_focus == RecordingDialogFocus::Interval
    }

    pub(crate) fn focus_next_recording_control(&mut self) {
        self.recording_dialog_focus = match self.recording_dialog_focus {
            RecordingDialogFocus::Path => RecordingDialogFocus::Interval,
            RecordingDialogFocus::Interval => RecordingDialogFocus::Path,
        };
    }

    pub(crate) fn focus_recording_path(&mut self) {
        self.recording_dialog_focus = RecordingDialogFocus::Path;
    }

    pub(crate) fn focus_recording_interval(&mut self) {
        self.recording_dialog_focus = RecordingDialogFocus::Interval;
    }

    pub(crate) fn selected_recording_interval_seconds(&self) -> u64 {
        RECORDING_INTERVAL_OPTIONS_SECONDS
            .get(self.recording_interval_index)
            .copied()
            .unwrap_or(super::SAMPLING_INTERVAL_SECONDS)
    }

    pub(crate) fn select_recording_interval(&mut self, index: usize) {
        self.recording_interval_index =
            index.min(RECORDING_INTERVAL_OPTIONS_SECONDS.len().saturating_sub(1));
    }

    pub(crate) fn select_previous_recording_interval(&mut self) {
        self.recording_interval_index = self.recording_interval_index.saturating_sub(1);
    }

    pub(crate) fn select_next_recording_interval(&mut self) {
        self.recording_interval_index = self
            .recording_interval_index
            .saturating_add(1)
            .min(RECORDING_INTERVAL_OPTIONS_SECONDS.len().saturating_sub(1));
    }

    pub(crate) fn active_recording_interval_seconds(&self) -> Option<u64> {
        self.recording_session
            .as_ref()
            .map(|session| session.interval_seconds)
    }

    pub(crate) fn confirm_recording_path(&mut self) -> Result<()> {
        let draft = self.recording_path_draft.trim();
        if draft.is_empty() {
            self.status = "Recording path is empty".to_string();
            return Ok(());
        }

        let path = PathBuf::from(draft);
        if self.reject_recording_directory_path(&path) {
            return Ok(());
        }
        if path.exists() {
            self.show_recording_overwrite_confirmation = true;
            self.status = format!("Overwrite existing log? {}", path.display());
            return Ok(());
        }

        self.start_recording(path, false)
    }

    pub(crate) fn cancel_recording_overwrite_confirmation(&mut self) {
        self.show_recording_overwrite_confirmation = false;
        self.ensure_visible_panel_focus();
        self.status = "Overwrite canceled".to_string();
    }

    pub(crate) fn confirm_recording_overwrite(&mut self) -> Result<()> {
        let path = PathBuf::from(self.recording_path_draft.trim());
        if self.reject_recording_directory_path(&path) {
            return Ok(());
        }
        self.start_recording(path, true)
    }

    fn reject_recording_directory_path(&mut self, path: &Path) -> bool {
        if !path.is_dir() {
            return false;
        }

        self.show_recording_overwrite_confirmation = false;
        self.status = "Recording path must be a file, not a directory".to_string();
        true
    }

    pub(crate) fn stop_recording(&mut self) -> Result<()> {
        let Some(path) = self.finish_recording("stopped")? else {
            self.status = "Recording is not active".to_string();
            return Ok(());
        };
        self.status = format!("Saved log to: {}", path.display());
        Ok(())
    }

    pub(crate) fn enforce_recording_duration_limit(&mut self) -> bool {
        self.enforce_recording_duration_limit_at(Instant::now())
    }

    fn enforce_recording_duration_limit_at(&mut self, now: Instant) -> bool {
        let Some(path) = self.recording_session.as_ref().and_then(|session| {
            session
                .duration_limit_reached_at(now)
                .then(|| session.path.clone())
        }) else {
            return false;
        };

        self.show_recording_stop_confirmation = false;
        self.show_recording_tracking_fixed = false;
        match self.finish_recording(RECORDING_DURATION_LIMIT_REASON) {
            Ok(Some(saved_path)) => {
                self.ensure_visible_panel_focus();
                self.status = format!(
                    "24-hour recording limit reached; saved log to: {}",
                    saved_path.display()
                );
            }
            Ok(None) => return false,
            Err(error) => self.present_active_recording_error(path, error),
        }
        true
    }

    fn finish_recording(&mut self, reason: &str) -> Result<Option<PathBuf>> {
        let Some(mut session) = self.recording_session.take() else {
            return Ok(None);
        };
        let path = session.path.clone();
        flush_pending_recording_frame(&mut session)?;
        let record = recording_end_record(reason);
        write_recording_record(&mut session, &record)?;
        session
            .writer
            .flush()
            .with_context(|| format!("failed to flush {}", path.display()))?;
        Ok(Some(path))
    }

    pub(crate) fn write_current_recording_frame(&mut self) -> Result<()> {
        let Some(session) = self.recording_session.as_mut() else {
            return Ok(());
        };
        write_recording_snapshot(session, &self.snapshot).map(|_| ())
    }

    #[cfg(test)]
    pub(crate) fn replace_recording_writer_for_test(&mut self, writer: Box<dyn Write>) {
        if let Some(session) = self.recording_session.as_mut() {
            session.writer = BufWriter::with_capacity(0, writer);
        }
    }

    #[cfg(test)]
    pub(crate) fn enforce_recording_duration_limit_for_test(&mut self, elapsed: Duration) -> bool {
        let Some(now) = self
            .recording_session
            .as_ref()
            .and_then(|session| session.started_at_instant.checked_add(elapsed))
        else {
            return false;
        };
        self.enforce_recording_duration_limit_at(now)
    }

    fn start_recording(&mut self, path: PathBuf, overwrite: bool) -> Result<()> {
        let recording_last_dir = match recording_parent_dir(&path) {
            Ok(parent) => parent,
            Err(error) => {
                self.present_recording_error(path, error, RecordingErrorKind::CouldNotStart, true);
                return Ok(());
            }
        };
        match open_recording_file(&path, overwrite) {
            Ok(file) => {
                let started_at = Local::now();
                let started_at_instant = Instant::now();
                let session_id = started_at.format("%Y%m%d%H%M%S").to_string();
                let host = host_name();
                let interval_seconds = self.selected_recording_interval_seconds();
                self.recording_session = Some(RecordingSession {
                    path: path.clone(),
                    session_id,
                    started_at,
                    started_at_instant,
                    host,
                    tracked_names: self.watch_list.clone(),
                    normalized_tracked_names: self.normalized_watch_names.clone(),
                    interval_seconds,
                    pending_frame: RecordingFrameAggregate::default(),
                    registered_processes: HashMap::new(),
                    next_process_id: 0,
                    registered_gpus: HashMap::new(),
                    next_gpu_id: 0,
                    writer: BufWriter::new(Box::new(file)),
                });
                self.recording_spinner_index = 0;
                self.recording_last_dir = recording_last_dir;
                self.show_recording_path_dialog = false;
                self.show_recording_overwrite_confirmation = false;
                self.recording_path_completion.reset();

                if let Err(error) = self.write_recording_session_header() {
                    self.recording_session = None;
                    self.present_recording_error(
                        path,
                        error,
                        RecordingErrorKind::CouldNotStart,
                        false,
                    );
                    return Ok(());
                }
                if let Err(error) = self.write_current_recording_frame() {
                    self.recording_session = None;
                    self.present_recording_error(
                        path,
                        error,
                        RecordingErrorKind::CouldNotStart,
                        false,
                    );
                    return Ok(());
                }
                if let Err(error) = self.flush_recording_writer() {
                    self.recording_session = None;
                    self.present_recording_error(
                        path,
                        error,
                        RecordingErrorKind::CouldNotStart,
                        false,
                    );
                    return Ok(());
                }

                self.status = format!("Recording started: {}", path.display());
            }
            Err(error) => {
                self.present_recording_error(path, error, RecordingErrorKind::CouldNotStart, true);
            }
        }
        Ok(())
    }

    fn write_recording_session_header(&mut self) -> Result<()> {
        let Some(session) = &self.recording_session else {
            return Ok(());
        };
        let record = recording_session_record(session, self);
        let session = self
            .recording_session
            .as_mut()
            .expect("recording session exists");
        write_recording_record(session, &record)
    }

    fn flush_recording_writer(&mut self) -> Result<()> {
        let Some(session) = self.recording_session.as_mut() else {
            return Ok(());
        };
        session
            .writer
            .flush()
            .with_context(|| format!("failed to flush {}", session.path.display()))
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

#[cfg(test)]
fn recording_frame_records(
    session: &mut RecordingSession,
    snapshot: &Snapshot,
) -> Result<Vec<V3Record>> {
    let mut aggregate = RecordingFrameAggregate::default();
    aggregate.add_snapshot(snapshot, &session.normalized_tracked_names);
    aggregate.into_records(session)
}

fn flush_pending_recording_frame(session: &mut RecordingSession) -> Result<()> {
    if session.pending_frame.is_empty() {
        return Ok(());
    }
    let aggregate = std::mem::take(&mut session.pending_frame);
    let records = aggregate.into_records(session)?;
    write_recording_records(session, &records)
}

fn write_recording_snapshot(session: &mut RecordingSession, snapshot: &Snapshot) -> Result<bool> {
    session
        .pending_frame
        .add_snapshot(snapshot, &session.normalized_tracked_names);
    if session.pending_frame.sample_count < session.interval_seconds {
        return Ok(false);
    }
    flush_pending_recording_frame(session)?;
    Ok(true)
}

fn recording_session_record(session: &RecordingSession, app: &App) -> V3Record {
    let snapshot = app.display_snapshot();
    let columns = app
        .process_columns
        .iter()
        .map(|column| column.label().to_string())
        .collect::<Vec<_>>();
    V3Record::Session(V3SessionRecord {
        schema_version: CURRENT_LOG_SCHEMA_VERSION,
        session_id: session.session_id.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        host: session.host.clone(),
        started_at_ms: session.started_at.timestamp_millis(),
        interval_seconds: session.interval_seconds,
        tracked_names: session.tracked_names.clone(),
        columns,
        sort: [
            app.sort.column.label().to_string(),
            match app.sort.direction {
                crate::model::SortDirection::Asc => "asc",
                crate::model::SortDirection::Desc => "desc",
            }
            .to_string(),
        ],
        system: V3SessionSystem {
            cpu_name: snapshot.cpu_name.clone(),
            cpu_frequency_mhz: snapshot.cpu_frequency_mhz,
            cpu_topology: snapshot.cpu_topology.clone(),
            cpu_cache: snapshot.cpu_cache.clone(),
        },
    })
}

fn recording_end_record(reason: &str) -> V3Record {
    V3Record::End(V3EndRecord(
        Local::now().timestamp_millis(),
        reason.to_string(),
    ))
}

fn write_recording_records(session: &mut RecordingSession, records: &[V3Record]) -> Result<()> {
    for record in records {
        write_recording_record(session, record)?;
    }
    Ok(())
}

fn write_recording_record(session: &mut RecordingSession, record: &V3Record) -> Result<()> {
    let path = session.path.display().to_string();
    serde_json::to_writer(&mut session.writer, record)
        .with_context(|| format!("failed to write {path}"))?;
    session
        .writer
        .write_all(b"\n")
        .with_context(|| format!("failed to write {path}"))
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
        let mut session = test_session(now, &["app.exe"]);
        let snapshot = Snapshot {
            captured_at: now,
            total_memory: 0,
            used_memory: 0,
            available_memory: None,
            modified_memory: Some(123_000_000),
            standby_memory: None,
            free_zeroed_memory: None,
            committed_memory: None,
            commit_limit: None,
            paged_pool_memory: None,
            nonpaged_pool_memory: None,
            pages_input_per_sec: Some(11),
            pages_output_per_sec: Some(7),
            cpu_name: None,
            cpu_frequency_mhz: None,
            cpu_current_frequency_mhz: None,
            cpu_p_core_frequency_mhz: None,
            cpu_e_core_frequency_mhz: None,
            cpu_total_usage_percent: Some(37),
            cpu_user_usage_percent: Some(29),
            cpu_kernel_usage_percent: Some(8),
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_adapters: Vec::new(),
            disks: Vec::new(),
            disk_read_bytes_per_sec: Some(10_000_000),
            disk_write_bytes_per_sec: Some(20_000_000),
            disk_queue_length: Some(1.5),
            network_received_bytes_per_sec: Some(30_000_000),
            network_sent_bytes_per_sec: Some(40_000_000),
            process_count: 2,
            thread_count: None,
            processes: vec![
                row(1, "app.exe", Some(120), None),
                row(2, "other.exe", Some(999), None),
            ],
        };
        let records = recording_frame_records(&mut session, &snapshot).unwrap();

        assert_eq!(records.len(), 2);
        let V3Record::Process(definition) = &records[0] else {
            panic!("first tracked-process frame must define the process");
        };
        assert_eq!(definition.0, 0);
        assert_eq!(definition.1, 1);
        assert_eq!(definition.2, "app.exe");
        assert_eq!(definition.3, Some(1001));
        assert_eq!(definition.4.as_deref(), Some(r"C:\work\app.exe"));

        let V3Record::Frame(V3FrameRecord(_, system, processes)) = &records[1] else {
            panic!("last record must be a frame");
        };
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].0, 0);
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::PRIVATE_BYTES],
            Some(120)
        );
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::HANDLE_COUNT],
            None
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::PHYSICAL_MEMORY],
            Some(0)
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::MODIFIED_MEMORY],
            Some(123_000_000)
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::PAGES_INPUT],
            Some(11)
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::PAGES_OUTPUT],
            Some(7)
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::PROCESS_COUNT],
            Some(2)
        );
        assert_eq!(
            system.0[crate::app::log_format::system_u64::CPU_TOTAL],
            Some(37)
        );
        assert_eq!(system.1, Some(1.5));
    }

    #[test]
    fn process_samples_preserve_missing_values() {
        let sample = V3ProcessSample::from_row(7, &row(1, "app.exe", Some(120), None));

        assert_eq!(
            sample.2[crate::app::log_format::process_u64::PRIVATE_BYTES],
            Some(120)
        );
        assert_eq!(
            sample.2[crate::app::log_format::process_u64::HANDLE_COUNT],
            None
        );
    }

    #[test]
    fn recording_aggregate_averages_available_values_and_uses_final_timestamp() {
        let first_at = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let second_at = first_at + chrono::Duration::seconds(1);
        let mut session = test_session(first_at, &["app.exe"]);
        let mut first_row = row(1, "app.exe", Some(100), None);
        first_row.cpu_percent = Some(10.0);
        let mut second_row = row(1, "app.exe", Some(101), Some(7));
        second_row.cpu_percent = Some(20.0);
        let mut first = test_snapshot(first_at, vec![first_row]);
        first.used_memory = 100;
        first.disk_queue_length = Some(1.0);
        let mut second = test_snapshot(second_at, vec![second_row]);
        second.used_memory = 101;
        second.disk_queue_length = Some(2.0);

        let mut aggregate = RecordingFrameAggregate::default();
        aggregate.add_snapshot(&first, &session.normalized_tracked_names);
        aggregate.add_snapshot(&second, &session.normalized_tracked_names);
        let records = aggregate.into_records(&mut session).unwrap();

        let V3Record::Frame(V3FrameRecord(captured_at_ms, system, processes)) =
            records.last().expect("aggregate must produce a frame")
        else {
            panic!("aggregate must end with a frame");
        };
        assert_eq!(*captured_at_ms, second_at.timestamp_millis());
        assert_eq!(
            system.0[crate::app::log_format::system_u64::PHYSICAL_MEMORY],
            Some(101)
        );
        assert_eq!(system.1, Some(1.5));
        assert_eq!(processes.len(), 1);
        assert_eq!(
            processes[0].1[crate::app::log_format::process_f64::CPU_PERCENT],
            Some(15.0)
        );
        assert!(processes[0].1[2..=4].iter().all(Option::is_none));
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::PRIVATE_BYTES],
            Some(101)
        );
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::HANDLE_COUNT],
            Some(7)
        );
    }

    #[test]
    fn recording_aggregate_does_not_treat_an_absent_process_as_zero() {
        let first_at = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let mut session = test_session(first_at, &["app.exe"]);
        let first = test_snapshot(first_at, vec![row(1, "app.exe", Some(120), Some(9))]);
        let second = test_snapshot(first_at + chrono::Duration::seconds(1), Vec::new());
        let mut aggregate = RecordingFrameAggregate::default();
        aggregate.add_snapshot(&first, &session.normalized_tracked_names);
        aggregate.add_snapshot(&second, &session.normalized_tracked_names);

        let records = aggregate.into_records(&mut session).unwrap();
        let V3Record::Frame(V3FrameRecord(_, _, processes)) =
            records.last().expect("aggregate must produce a frame")
        else {
            panic!("aggregate must end with a frame");
        };
        assert_eq!(processes.len(), 1);
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::PRIVATE_BYTES],
            Some(120)
        );
        assert_eq!(
            processes[0].2[crate::app::log_format::process_u64::HANDLE_COUNT],
            Some(9)
        );
    }

    #[test]
    fn recording_writes_only_when_the_selected_sample_window_is_complete() {
        let first_at = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let mut session = test_session(first_at, &["app.exe"]);
        session.interval_seconds = 2;
        let first = test_snapshot(first_at, vec![row(1, "app.exe", Some(100), None)]);
        let second = test_snapshot(
            first_at + chrono::Duration::seconds(1),
            vec![row(1, "app.exe", Some(200), None)],
        );

        assert!(!write_recording_snapshot(&mut session, &first).unwrap());
        assert_eq!(session.pending_frame.sample_count, 1);
        assert!(write_recording_snapshot(&mut session, &second).unwrap());
        assert!(session.pending_frame.is_empty());
    }

    #[test]
    fn recording_defines_a_tracked_process_when_it_starts_later() {
        let now = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let mut session = test_session(now, &["app.exe"]);
        let initial = test_snapshot(now, Vec::new());

        let initial_records = recording_frame_records(&mut session, &initial).unwrap();
        assert!(matches!(initial_records.as_slice(), [V3Record::Frame(_)]));

        let later = test_snapshot(
            now + chrono::Duration::seconds(1),
            vec![row(42, "app.exe", Some(120), None)],
        );
        let later_records = recording_frame_records(&mut session, &later).unwrap();

        assert_eq!(later_records.len(), 2);
        let V3Record::Process(definition) = &later_records[0] else {
            panic!("the first observed process must be defined before its frame");
        };
        assert_eq!((definition.0, definition.1), (0, 42));
        let V3Record::Frame(V3FrameRecord(_, _, samples)) = &later_records[1] else {
            panic!("last record must be a frame");
        };
        assert_eq!(samples[0].0, definition.0);
    }

    #[test]
    fn recording_assigns_distinct_ids_to_concurrent_same_name_processes() {
        let now = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let mut session = test_session(now, &["app.exe"]);
        let snapshot = test_snapshot(
            now,
            vec![
                row(42, "app.exe", Some(120), None),
                row(84, "app.exe", Some(240), None),
            ],
        );

        let first_records = recording_frame_records(&mut session, &snapshot).unwrap();

        assert_eq!(first_records.len(), 3);
        let V3Record::Process(first) = &first_records[0] else {
            panic!("first process definition is missing");
        };
        let V3Record::Process(second) = &first_records[1] else {
            panic!("second process definition is missing");
        };
        assert_eq!((first.0, first.1), (0, 42));
        assert_eq!((second.0, second.1), (1, 84));
        assert_eq!(first.2, second.2);
        let V3Record::Frame(V3FrameRecord(_, _, samples)) = &first_records[2] else {
            panic!("last record must be a frame");
        };
        assert_eq!(
            samples.iter().map(|sample| sample.0).collect::<Vec<_>>(),
            [0, 1]
        );

        let next_records = recording_frame_records(&mut session, &snapshot).unwrap();
        let [V3Record::Frame(V3FrameRecord(_, _, samples))] = next_records.as_slice() else {
            panic!("known processes must reuse their definitions");
        };
        assert_eq!(
            samples.iter().map(|sample| sample.0).collect::<Vec<_>>(),
            [0, 1]
        );
    }

    fn test_session(now: DateTime<Local>, tracked_names: &[&str]) -> RecordingSession {
        RecordingSession {
            path: PathBuf::from("test.log"),
            session_id: "20260504143012".to_string(),
            started_at: now,
            started_at_instant: Instant::now(),
            host: "PC01".to_string(),
            tracked_names: tracked_names
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            normalized_tracked_names: tracked_names
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
            interval_seconds: 1,
            pending_frame: RecordingFrameAggregate::default(),
            registered_processes: HashMap::new(),
            next_process_id: 0,
            registered_gpus: HashMap::new(),
            next_gpu_id: 0,
            writer: BufWriter::new(Box::new(
                OpenOptions::new()
                    .write(true)
                    .open(if cfg!(windows) { "NUL" } else { "/dev/null" })
                    .unwrap(),
            )),
        }
    }

    #[test]
    fn recording_duration_limit_is_reached_at_exactly_24_hours() {
        let now = Local.with_ymd_and_hms(2026, 5, 4, 14, 30, 12).unwrap();
        let session = test_session(now, &["app.exe"]);
        let limit = session
            .started_at_instant
            .checked_add(MAX_RECORDING_DURATION)
            .expect("24 hours must fit in Instant");

        assert!(session.duration_limit_reached_at(limit));

        let just_before_limit = session
            .started_at_instant
            .checked_add(MAX_RECORDING_DURATION - Duration::from_millis(1))
            .expect("24 hours must fit in Instant");
        assert!(!session.duration_limit_reached_at(just_before_limit));
    }

    fn test_snapshot(captured_at: DateTime<Local>, processes: Vec<ProcessRow>) -> Snapshot {
        Snapshot {
            captured_at,
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
            process_count: processes.len(),
            thread_count: None,
            processes,
        }
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
            executable_path: Some(format!(r"C:\work\{name}")),
            start_time: Some(1000 + pid as u64),
            cpu_percent: None,
            private_bytes,
            workset_bytes: None,
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }
}
