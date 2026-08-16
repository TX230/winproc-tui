use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    ProcessHistory, ProcessRow, Snapshot, SortSpec, SystemHistory, sort_process_rows,
};

const SUPPORTED_LOG_SCHEMA_VERSION: u64 = 2;
const LOG_TAIL_READ_CHUNK_SIZE: usize = 8 * 1024;
const LOG_LOAD_READ_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct LogSummary {
    pub(crate) path: PathBuf,
    pub(crate) schema_version: Option<u64>,
    pub(crate) session_id: Option<String>,
    pub(crate) started_at: Option<DateTime<Local>>,
    pub(crate) ended_at: Option<DateTime<Local>>,
    pub(crate) host: Option<String>,
    pub(crate) tracked_names: Vec<String>,
    pub(crate) frame_count: usize,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct LogListResult {
    pub(crate) dir: PathBuf,
    pub(crate) summaries: Vec<LogSummary>,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct LoadedLog {
    pub(crate) path: PathBuf,
    pub(crate) summary: LogSummary,
    pub(crate) snapshot: Snapshot,
    pub(crate) process_history: ProcessHistory,
    pub(crate) system_history: SystemHistory,
    pub(crate) tracked_names: Vec<String>,
}

pub(crate) struct LogListWorker {
    receiver: Receiver<LogListResult>,
}

pub(crate) struct LogLoadWorker {
    receiver: Receiver<Result<LoadedLog, String>>,
}

impl LogListWorker {
    pub(crate) fn spawn(dir: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = scan_log_dir(&dir);
            let _ = sender.send(result);
        });
        Self { receiver }
    }

    pub(crate) fn try_recv(&self) -> Result<Option<LogListResult>, TryRecvError> {
        match self.receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl LogLoadWorker {
    pub(crate) fn spawn(path: PathBuf, sort: SortSpec) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = load_log(&path, sort).map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        Self { receiver }
    }

    pub(crate) fn try_recv(&self) -> Result<Option<Result<LoadedLog, String>>, TryRecvError> {
        match self.receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn scan_log_dir(dir: &Path) -> LogListResult {
    let mut summaries = Vec::new();
    let mut error = None;
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_log = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("log"));
                if !is_log {
                    continue;
                }
                let summary = summarize_log(&path);
                if summary.schema_version == Some(SUPPORTED_LOG_SCHEMA_VERSION) {
                    summaries.push(summary);
                }
            }
        }
        Err(read_error) => {
            error = Some(format!("failed to list {}: {read_error}", dir.display()));
        }
    }
    summaries.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.path.cmp(&left.path))
    });
    LogListResult {
        dir: dir.to_path_buf(),
        summaries,
        error,
    }
}

pub(crate) fn summarize_log(path: &Path) -> LogSummary {
    let mut summary = empty_summary(path);

    let result = read_first_json_line(path).and_then(|record| {
        update_summary_from_record(&mut summary, &record);
        if summary.schema_version == Some(SUPPORTED_LOG_SCHEMA_VERSION) {
            let tail = read_last_json_line(path)?;
            if tail != record {
                update_summary_from_record(&mut summary, &tail);
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        summary.error = Some(error.to_string());
    }
    summary
}

pub(crate) fn load_log(path: &Path, sort: SortSpec) -> Result<LoadedLog> {
    let mut summary = empty_summary(path);
    let mut session = SessionMeta::default();
    let mut process_history = ProcessHistory::default();
    let mut system_history = SystemHistory::default();
    let mut last_snapshot = None;
    let mut tracked_names = Vec::new();
    let mut tracked_name_set = HashSet::new();

    read_load_records(path, |record| {
        match record {
            LoadRecord::Session {
                schema_version,
                session_id,
                host,
                started_at,
                system,
            } => {
                require_supported_schema_version(schema_version)?;
                update_loaded_summary_common(&mut summary, schema_version, session_id);
                if summary.host.is_none() {
                    summary.host = host;
                }
                if summary.started_at.is_none() {
                    summary.started_at = started_at.as_deref().and_then(parse_datetime);
                }
                session = SessionMeta::from_loaded_record(system);
            }
            LoadRecord::End {
                schema_version,
                session_id,
                ended_at,
            } => {
                require_supported_schema_version(schema_version)?;
                update_loaded_summary_common(&mut summary, schema_version, session_id);
                if let Some(ended_at) = ended_at.as_deref().and_then(parse_datetime) {
                    summary.ended_at = Some(ended_at);
                }
            }
            LoadRecord::Frame {
                schema_version,
                session_id,
                captured_at,
                system_metrics,
                processes,
            } => {
                require_supported_schema_version(schema_version)?;
                update_loaded_summary_common(&mut summary, schema_version, session_id);
                let frame = parse_loaded_frame(
                    captured_at,
                    system_metrics,
                    processes.unwrap_or_default(),
                    &session,
                )?;
                summary.frame_count = summary.frame_count.saturating_add(1);
                if summary.started_at.is_none() {
                    summary.started_at = Some(frame.snapshot.captured_at);
                }
                summary.ended_at = Some(frame.snapshot.captured_at);
                add_process_names(&mut tracked_names, &mut tracked_name_set, &frame.snapshot);
                process_history.record_snapshot_unbounded(
                    frame.snapshot.captured_at,
                    &frame.snapshot.processes,
                );
                if frame.has_system_metrics {
                    system_history.record_snapshot_unbounded(&frame.snapshot);
                }
                last_snapshot = Some(frame.snapshot);
            }
        }
        Ok(())
    })?;

    let mut snapshot = last_snapshot.context("log contains no frames")?;
    sort_process_rows(&mut snapshot.processes, sort);
    summary.tracked_names = tracked_names.clone();
    summary.error = None;
    Ok(LoadedLog {
        path: path.to_path_buf(),
        summary,
        snapshot,
        process_history,
        system_history,
        tracked_names,
    })
}

fn read_load_records<F>(path: &Path, mut handle: F) -> Result<()>
where
    F: FnMut(LoadRecord) -> Result<()>,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(LOG_LOAD_READ_BUFFER_SIZE, file);
    let mut line = Vec::new();
    let mut line_index = 0_usize;
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("failed to read {} line {}", path.display(), line_index + 1))?
            == 0
        {
            break;
        }
        line_index += 1;
        let line = trim_ascii_whitespace(&line);
        if line.is_empty() {
            continue;
        }
        let record = match serde_json::from_slice(line) {
            Ok(record) => record,
            Err(error) => {
                if let Ok(probe) = serde_json::from_slice::<SchemaVersionProbe>(line) {
                    require_supported_schema_version(probe.schema_version)
                        .with_context(|| format!("invalid log record at line {line_index}"))?;
                }
                let label = if error.is_syntax() || error.is_eof() {
                    "invalid JSON"
                } else {
                    "invalid log record"
                };
                return Err(error).with_context(|| format!("{label} at line {line_index}"));
            }
        };
        handle(record).with_context(|| format!("invalid log record at line {line_index}"))?;
    }
    Ok(())
}

fn empty_summary(path: &Path) -> LogSummary {
    LogSummary {
        path: path.to_path_buf(),
        schema_version: None,
        session_id: None,
        started_at: None,
        ended_at: None,
        host: None,
        tracked_names: Vec::new(),
        frame_count: 0,
        error: None,
    }
}

fn read_first_json_line(path: &Path) -> Result<Value> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read {} line {}", path.display(), line_index + 1)
        })?;
        if line.trim().is_empty() {
            continue;
        }
        return serde_json::from_str(&line)
            .with_context(|| format!("invalid JSON at line {}", line_index + 1));
    }
    Err(anyhow!("log is empty"))
}

fn read_last_json_line(path: &Path) -> Result<Value> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut position = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("failed to seek {}", path.display()))?;
    let mut buffer = Vec::new();
    let mut chunk = vec![0; LOG_TAIL_READ_CHUNK_SIZE];

    while position > 0 {
        let read_len = usize::try_from(position.min(LOG_TAIL_READ_CHUNK_SIZE as u64))
            .unwrap_or(LOG_TAIL_READ_CHUNK_SIZE);
        position -= read_len as u64;
        file.seek(SeekFrom::Start(position))
            .with_context(|| format!("failed to seek {}", path.display()))?;
        file.read_exact(&mut chunk[..read_len])
            .with_context(|| format!("failed to read {}", path.display()))?;

        let mut combined = Vec::with_capacity(read_len + buffer.len());
        combined.extend_from_slice(&chunk[..read_len]);
        combined.extend_from_slice(&buffer);
        buffer = combined;

        let parts = buffer.split(|byte| *byte == b'\n').collect::<Vec<_>>();
        for (index, line) in parts.iter().enumerate().rev() {
            if index == 0 && position > 0 {
                break;
            }
            let line = trim_ascii_whitespace(line);
            if line.is_empty() {
                continue;
            }
            let text = std::str::from_utf8(line).context("last log line is not UTF-8")?;
            return serde_json::from_str(text).context("invalid JSON in last log line");
        }
    }

    Err(anyhow!("log is empty"))
}

fn trim_ascii_whitespace(value: &[u8]) -> &[u8] {
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    let end = value
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &value[start..end]
}

fn update_summary_from_record(summary: &mut LogSummary, record: &Value) {
    if summary.schema_version.is_none() {
        summary.schema_version = record.get("schema_version").and_then(Value::as_u64);
    }
    if summary.session_id.is_none() {
        summary.session_id = string_field(record, "session_id");
    }
    if summary.host.is_none() {
        summary.host = string_field(record, "host");
    }

    match record_type(record) {
        Some("session") => {
            summary.started_at = summary
                .started_at
                .or_else(|| datetime_field(record, "started_at"));
        }
        Some("end") => {
            if let Some(ended_at) = datetime_field(record, "ended_at") {
                summary.ended_at = Some(ended_at);
            }
        }
        Some("frame") => {
            summary.frame_count = summary.frame_count.saturating_add(1);
            let captured_at = datetime_field(record, "captured_at");
            if summary.started_at.is_none() {
                summary.started_at = captured_at;
            }
            if let Some(captured_at) = captured_at {
                summary.ended_at = Some(captured_at);
            }
            add_summary_process_names(summary, record);
        }
        Some(_) | None => {}
    }
}

fn add_summary_process_names(summary: &mut LogSummary, record: &Value) {
    let Some(processes) = record.get("processes").and_then(Value::as_array) else {
        return;
    };
    let mut seen = normalized_names(&summary.tracked_names);
    for name in processes
        .iter()
        .filter_map(|process| process.get("name").and_then(Value::as_str))
    {
        let normalized = name.trim().to_ascii_lowercase();
        if normalized.is_empty() || seen.contains(&normalized) {
            continue;
        }
        seen.insert(normalized);
        summary.tracked_names.push(name.to_string());
    }
}

fn add_process_names(names: &mut Vec<String>, seen: &mut HashSet<String>, snapshot: &Snapshot) {
    for process in &snapshot.processes {
        let normalized = process.name.trim().to_ascii_lowercase();
        if normalized.is_empty() || seen.contains(&normalized) {
            continue;
        }
        seen.insert(normalized);
        names.push(process.name.clone());
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "record_type")]
enum LoadRecord {
    #[serde(rename = "session")]
    Session {
        schema_version: Option<u64>,
        session_id: Option<String>,
        host: Option<String>,
        started_at: Option<String>,
        system: Option<SessionSystemRecord>,
    },
    #[serde(rename = "frame")]
    Frame {
        schema_version: Option<u64>,
        session_id: Option<String>,
        captured_at: Option<String>,
        system_metrics: Option<SystemMetricsRecord>,
        processes: Option<Vec<ProcessRecord>>,
    },
    #[serde(rename = "end")]
    End {
        schema_version: Option<u64>,
        session_id: Option<String>,
        ended_at: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct SchemaVersionProbe {
    schema_version: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct SessionSystemRecord {
    cpu_name: Option<String>,
    cpu_frequency_mhz: Option<u64>,
    cpu_topology: Option<String>,
    cpu_cache: Option<String>,
    gpu_name: Option<String>,
    gpu_adapters: Option<Vec<GpuAdapterRecord>>,
}

#[derive(Debug, Default, Deserialize)]
struct SystemMetricsRecord {
    physical_memory_bytes: Option<u64>,
    total_memory_bytes: Option<u64>,
    available_memory_bytes: Option<u64>,
    modified_memory_bytes: Option<u64>,
    standby_memory_bytes: Option<u64>,
    free_zeroed_memory_bytes: Option<u64>,
    committed_bytes: Option<u64>,
    commit_limit_bytes: Option<u64>,
    paged_pool_bytes: Option<u64>,
    nonpaged_pool_bytes: Option<u64>,
    pages_input_per_sec: Option<u64>,
    pages_output_per_sec: Option<u64>,
    cpu_percent: Option<u64>,
    cpu_user_percent: Option<u64>,
    cpu_kernel_percent: Option<u64>,
    gpu_adapters: Option<Vec<GpuAdapterRecord>>,
    gpu_dedicated_bytes: Option<u64>,
    gpu_dedicated_total_bytes: Option<u64>,
    gpu_shared_bytes: Option<u64>,
    gpu_shared_total_bytes: Option<u64>,
    disk_read_bytes_per_sec: Option<u64>,
    disk_write_bytes_per_sec: Option<u64>,
    disk_queue_length: Option<f64>,
    network_received_bytes_per_sec: Option<u64>,
    network_sent_bytes_per_sec: Option<u64>,
    process_count: Option<u64>,
    thread_count: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct GpuAdapterRecord {
    luid_high: Option<u32>,
    luid_low: Option<u32>,
    name: Option<String>,
    utilization_percent: Option<f64>,
    encode_average_percent: Option<f64>,
    encode_max_percent: Option<f64>,
    encode_engine_count: Option<u32>,
    decode_average_percent: Option<f64>,
    decode_max_percent: Option<f64>,
    decode_engine_count: Option<u32>,
    dedicated_bytes: Option<u64>,
    dedicated_total_bytes: Option<u64>,
    shared_bytes: Option<u64>,
    shared_total_bytes: Option<u64>,
}

impl GpuAdapterRecord {
    fn into_sample(self) -> crate::model::GpuAdapterSample {
        crate::model::GpuAdapterSample {
            id: crate::model::GpuAdapterId {
                high: self.luid_high.unwrap_or_default(),
                low: self.luid_low.unwrap_or_default(),
            },
            name: self.name,
            utilization_percent: self.utilization_percent.filter(|value| value.is_finite()),
            encode: crate::model::GpuEngineSummary {
                average_percent: self
                    .encode_average_percent
                    .filter(|value| value.is_finite()),
                max_percent: self.encode_max_percent.filter(|value| value.is_finite()),
                engine_count: self.encode_engine_count.unwrap_or_default(),
            },
            decode: crate::model::GpuEngineSummary {
                average_percent: self
                    .decode_average_percent
                    .filter(|value| value.is_finite()),
                max_percent: self.decode_max_percent.filter(|value| value.is_finite()),
                engine_count: self.decode_engine_count.unwrap_or_default(),
            },
            dedicated_used: self.dedicated_bytes,
            dedicated_total: self.dedicated_total_bytes,
            shared_used: self.shared_bytes,
            shared_total: self.shared_total_bytes,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ProcessRecord {
    pid: Option<u32>,
    name: Option<String>,
    path: Option<String>,
    start_time: Option<u64>,
    metrics: Option<ProcessMetricsRecord>,
}

impl ProcessRecord {
    fn into_row(self) -> Result<ProcessRow> {
        let metrics = self.metrics.unwrap_or_default();
        Ok(ProcessRow {
            pid: self.pid.ok_or_else(|| anyhow!("process is missing pid"))?,
            name: self
                .name
                .ok_or_else(|| anyhow!("process is missing name"))?,
            executable_path: self.path,
            start_time: self.start_time,
            cpu_percent: metrics.cpu_percent.filter(|value| value.is_finite()),
            private_bytes: metrics.private_bytes,
            workset_bytes: metrics.workset_bytes,
            workset_private_bytes: metrics.workset_private_bytes,
            workset_shareable_bytes: metrics.workset_shareable_bytes,
            thread_count: metrics.thread_count,
            handle_count: metrics.handle_count,
            user_object_count: metrics.user_object_count,
            gdi_object_count: metrics.gdi_object_count,
            gpu_percent: metrics.gpu_percent.filter(|value| value.is_finite()),
            gpu_dedicated_bytes: metrics.gpu_dedicated_bytes,
            gpu_shared_bytes: metrics.gpu_shared_bytes,
            dotnet_heap_bytes: metrics.dotnet_heap_bytes,
            io_read_bytes_per_sec: metrics.io_read_bytes_per_sec,
            io_write_bytes_per_sec: metrics.io_write_bytes_per_sec,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct ProcessMetricsRecord {
    cpu_percent: Option<f64>,
    private_bytes: Option<u64>,
    workset_bytes: Option<u64>,
    workset_private_bytes: Option<u64>,
    workset_shareable_bytes: Option<u64>,
    thread_count: Option<u64>,
    handle_count: Option<u64>,
    user_object_count: Option<u64>,
    gdi_object_count: Option<u64>,
    gpu_percent: Option<f64>,
    gpu_dedicated_bytes: Option<u64>,
    gpu_shared_bytes: Option<u64>,
    dotnet_heap_bytes: Option<u64>,
    io_read_bytes_per_sec: Option<u64>,
    io_write_bytes_per_sec: Option<u64>,
}

fn update_loaded_summary_common(
    summary: &mut LogSummary,
    schema_version: Option<u64>,
    session_id: Option<String>,
) {
    if summary.schema_version.is_none() {
        summary.schema_version = schema_version;
    }
    if summary.session_id.is_none() {
        summary.session_id = session_id;
    }
}

fn require_supported_schema_version(schema_version: Option<u64>) -> Result<()> {
    match schema_version {
        Some(SUPPORTED_LOG_SCHEMA_VERSION) => Ok(()),
        Some(version) => Err(anyhow!("unsupported log schema_version {version}")),
        None => Err(anyhow!("log record is missing schema_version")),
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Local))
}

fn parse_loaded_frame(
    captured_at: Option<String>,
    system_metrics: Option<SystemMetricsRecord>,
    processes: Vec<ProcessRecord>,
    session: &SessionMeta,
) -> Result<ParsedFrame> {
    let captured_at = captured_at
        .as_deref()
        .and_then(parse_datetime)
        .ok_or_else(|| anyhow!("frame is missing captured_at"))?;
    let processes = processes
        .into_iter()
        .map(ProcessRecord::into_row)
        .collect::<Result<Vec<_>>>()?;
    let has_system_metrics = system_metrics.is_some();
    let mut system = system_metrics.unwrap_or_default();
    let gpu_adapters = parse_loaded_gpu_adapters(&mut system, session);
    let snapshot = Snapshot {
        captured_at,
        total_memory: system.total_memory_bytes.unwrap_or_default(),
        used_memory: system.physical_memory_bytes.unwrap_or_default(),
        available_memory: system.available_memory_bytes,
        modified_memory: system.modified_memory_bytes,
        standby_memory: system.standby_memory_bytes,
        free_zeroed_memory: system.free_zeroed_memory_bytes,
        committed_memory: system.committed_bytes,
        commit_limit: system.commit_limit_bytes,
        paged_pool_memory: system.paged_pool_bytes,
        nonpaged_pool_memory: system.nonpaged_pool_bytes,
        pages_input_per_sec: system.pages_input_per_sec,
        pages_output_per_sec: system.pages_output_per_sec,
        cpu_name: session.cpu_name.clone(),
        cpu_frequency_mhz: session.cpu_frequency_mhz,
        cpu_current_frequency_mhz: None,
        cpu_p_core_frequency_mhz: None,
        cpu_e_core_frequency_mhz: None,
        cpu_total_usage_percent: system
            .cpu_percent
            .and_then(|value| u8::try_from(value.min(100)).ok()),
        cpu_user_usage_percent: system
            .cpu_user_percent
            .and_then(|value| u8::try_from(value.min(100)).ok()),
        cpu_kernel_usage_percent: system
            .cpu_kernel_percent
            .and_then(|value| u8::try_from(value.min(100)).ok()),
        cpu_logical_processors: Vec::new(),
        cpu_topology: session.cpu_topology.clone(),
        cpu_cache: session.cpu_cache.clone(),
        gpu_adapters,
        disks: Vec::new(),
        disk_read_bytes_per_sec: system.disk_read_bytes_per_sec,
        disk_write_bytes_per_sec: system.disk_write_bytes_per_sec,
        disk_queue_length: system.disk_queue_length.filter(|value| value.is_finite()),
        network_received_bytes_per_sec: system.network_received_bytes_per_sec,
        network_sent_bytes_per_sec: system.network_sent_bytes_per_sec,
        process_count: system
            .process_count
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(processes.len()),
        thread_count: system.thread_count,
        processes,
    };
    Ok(ParsedFrame {
        snapshot,
        has_system_metrics,
    })
}

fn parse_loaded_gpu_adapters(
    system: &mut SystemMetricsRecord,
    session: &SessionMeta,
) -> Vec<crate::model::GpuAdapterSample> {
    let mut adapters = system
        .gpu_adapters
        .take()
        .unwrap_or_default()
        .into_iter()
        .map(GpuAdapterRecord::into_sample)
        .collect::<Vec<_>>();
    if !adapters.is_empty() {
        for adapter in &mut adapters {
            if let Some(metadata) = session
                .gpu_adapters
                .iter()
                .find(|metadata| metadata.id == adapter.id)
            {
                adapter.name = adapter.name.take().or_else(|| metadata.name.clone());
                adapter.dedicated_total = adapter.dedicated_total.or(metadata.dedicated_total);
                adapter.shared_total = adapter.shared_total.or(metadata.shared_total);
            }
        }
        return adapters;
    }

    let adapter = crate::model::GpuAdapterSample {
        name: session.gpu_name.clone(),
        dedicated_used: system.gpu_dedicated_bytes,
        dedicated_total: system.gpu_dedicated_total_bytes,
        shared_used: system.gpu_shared_bytes,
        shared_total: system.gpu_shared_total_bytes,
        ..crate::model::GpuAdapterSample::default()
    };
    if adapter.name.is_some()
        || adapter.dedicated_used.is_some()
        || adapter.dedicated_total.is_some()
        || adapter.shared_used.is_some()
        || adapter.shared_total.is_some()
    {
        vec![adapter]
    } else {
        session.gpu_adapters.clone()
    }
}

#[derive(Debug, Clone, Default)]
struct SessionMeta {
    cpu_name: Option<String>,
    cpu_frequency_mhz: Option<u64>,
    cpu_topology: Option<String>,
    cpu_cache: Option<String>,
    gpu_name: Option<String>,
    gpu_adapters: Vec<crate::model::GpuAdapterSample>,
}

impl SessionMeta {
    fn from_loaded_record(system: Option<SessionSystemRecord>) -> Self {
        let system = system.unwrap_or_default();
        Self {
            cpu_name: system.cpu_name,
            cpu_frequency_mhz: system.cpu_frequency_mhz,
            cpu_topology: system.cpu_topology,
            cpu_cache: system.cpu_cache,
            gpu_name: system.gpu_name,
            gpu_adapters: system
                .gpu_adapters
                .unwrap_or_default()
                .into_iter()
                .map(GpuAdapterRecord::into_sample)
                .collect(),
        }
    }
}

struct ParsedFrame {
    snapshot: Snapshot,
    has_system_metrics: bool,
}

fn record_type(record: &Value) -> Option<&str> {
    record.get("record_type").and_then(Value::as_str)
}

fn datetime_field(record: &Value, name: &str) -> Option<DateTime<Local>> {
    record
        .get(name)
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Local))
}

fn string_field(record: &Value, name: &str) -> Option<String> {
    record
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn normalized_names(names: &[String]) -> HashSet<String> {
    names
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SystemMetric;
    use std::io::Write;

    #[test]
    fn scan_log_dir_hides_non_v2_logs() {
        let dir = std::env::temp_dir().join(format!(
            "winproc-tui-log-scan-{}-{}",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&dir).unwrap();
        let v1_path = dir.join("old.log");
        let v2_path = dir.join("current.log");
        write_lines(
            &v1_path,
            &[
                r#"{"schema_version":1,"session_id":"s1","host":"PC","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":120}}]}"#,
            ],
        );
        write_lines(
            &v2_path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":120}}]}"#,
            ],
        );

        let result = scan_log_dir(&dir);

        assert_eq!(result.summaries.len(), 1);
        assert_eq!(result.summaries[0].path, v2_path);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_log_rejects_non_v2_logs() {
        let path = unique_log_path("v1");
        write_lines(
            &path,
            &[
                r#"{"schema_version":1,"session_id":"s1","host":"PC","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":120}}]}"#,
            ],
        );

        let error = format!("{:?}", load_log(&path, SortSpec::default()).unwrap_err());

        assert!(
            error.contains("unsupported log schema_version 1"),
            "{error}"
        );
    }

    #[test]
    fn v2_log_loads_system_history_and_missing_metrics_as_none() {
        let path = unique_log_path("v2");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system":{"cpu_name":"CPU","gpu_adapters":[{"luid_high":1,"luid_low":2,"name":"GPU","dedicated_total_bytes":8000}]}}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":1000,"total_memory_bytes":8000,"available_memory_bytes":7000,"modified_memory_bytes":750,"pages_input_per_sec":11,"pages_output_per_sec":7,"process_count":214,"thread_count":321,"gpu_adapters":[{"luid_high":1,"luid_low":2,"utilization_percent":74.0,"encode_average_percent":60.0,"encode_max_percent":100.0,"encode_engine_count":2,"dedicated_bytes":2000}],"cpu_percent":37,"cpu_user_percent":29,"cpu_kernel_percent":8,"disk_read_bytes_per_sec":10000000,"disk_write_bytes_per_sec":20000000,"disk_queue_length":1.5,"network_received_bytes_per_sec":30000000,"network_sent_bytes_per_sec":40000000},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":null,"handle_count":5,"workset_shareable_bytes":512}}]}"#,
            ],
        );

        let loaded = load_log(&path, SortSpec::default()).unwrap();

        assert_eq!(loaded.summary.schema_version, Some(2));
        assert_eq!(loaded.snapshot.cpu_name.as_deref(), Some("CPU"));
        assert_eq!(loaded.snapshot.cpu_total_usage_percent, Some(37));
        assert_eq!(loaded.snapshot.cpu_user_usage_percent, Some(29));
        assert_eq!(loaded.snapshot.cpu_kernel_usage_percent, Some(8));
        assert_eq!(loaded.snapshot.disk_read_bytes_per_sec, Some(10_000_000));
        assert_eq!(loaded.snapshot.disk_write_bytes_per_sec, Some(20_000_000));
        assert_eq!(loaded.snapshot.disk_queue_length, Some(1.5));
        assert_eq!(
            loaded.snapshot.network_received_bytes_per_sec,
            Some(30_000_000)
        );
        assert_eq!(loaded.snapshot.network_sent_bytes_per_sec, Some(40_000_000));
        assert_eq!(loaded.snapshot.used_memory, 1000);
        assert_eq!(loaded.snapshot.available_memory, Some(7000));
        assert_eq!(loaded.snapshot.modified_memory, Some(750));
        assert_eq!(loaded.snapshot.pages_input_per_sec, Some(11));
        assert_eq!(loaded.snapshot.pages_output_per_sec, Some(7));
        assert_eq!(loaded.snapshot.process_count, 214);
        assert_eq!(loaded.snapshot.thread_count, Some(321));
        assert_eq!(loaded.snapshot.gpu_adapters.len(), 1);
        assert_eq!(loaded.snapshot.gpu_adapters[0].id.high, 1);
        assert_eq!(loaded.snapshot.gpu_adapters[0].name.as_deref(), Some("GPU"));
        assert_eq!(loaded.snapshot.gpu_adapters[0].dedicated_total, Some(8000));
        assert_eq!(
            loaded.snapshot.gpu_adapters[0].utilization_percent,
            Some(74.0)
        );
        assert_eq!(loaded.snapshot.gpu_adapters[0].encode.engine_count, 2);
        assert_eq!(loaded.system_history.len(), 1);
        assert_eq!(
            loaded.system_history.samples()[0].value(SystemMetric::ModifiedMemory),
            Some(750.0)
        );
        assert_eq!(
            loaded.system_history.samples()[0].value(SystemMetric::PagesOutput),
            Some(7.0)
        );
        assert_eq!(
            loaded.system_history.samples()[0].value(SystemMetric::CpuAverage),
            Some(37.0)
        );
        assert_eq!(
            loaded.system_history.samples()[0].value(SystemMetric::ProcessCount),
            Some(214.0)
        );
        assert_eq!(loaded.snapshot.processes[0].private_bytes, None);
        assert_eq!(loaded.snapshot.processes[0].handle_count, Some(5));
        assert_eq!(
            loaded.snapshot.processes[0].workset_shareable_bytes,
            Some(512)
        );
    }

    #[test]
    fn v2_log_loads_all_process_fields() {
        let path = unique_log_path("v2-process-fields");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","started_at":"2026-05-04T14:30:12+09:00"}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","processes":[{"pid":42,"name":"app.exe","path":"C:\\app.exe","start_time":100,"metrics":{"cpu_percent":12.5,"private_bytes":1000,"workset_bytes":900,"workset_private_bytes":700,"workset_shareable_bytes":200,"thread_count":10,"handle_count":20,"user_object_count":30,"gdi_object_count":40,"gpu_percent":5.5,"gpu_dedicated_bytes":50,"gpu_shared_bytes":60,"dotnet_heap_bytes":70,"io_read_bytes_per_sec":80,"io_write_bytes_per_sec":90}}]}"#,
            ],
        );

        let loaded = load_log(&path, SortSpec::default()).unwrap();
        let process = &loaded.snapshot.processes[0];

        assert_eq!(process.pid, 42);
        assert_eq!(process.name, "app.exe");
        assert_eq!(process.executable_path.as_deref(), Some(r"C:\app.exe"));
        assert_eq!(process.start_time, Some(100));
        assert_eq!(process.cpu_percent, Some(12.5));
        assert_eq!(process.private_bytes, Some(1000));
        assert_eq!(process.workset_bytes, Some(900));
        assert_eq!(process.workset_private_bytes, Some(700));
        assert_eq!(process.workset_shareable_bytes, Some(200));
        assert_eq!(process.thread_count, Some(10));
        assert_eq!(process.handle_count, Some(20));
        assert_eq!(process.user_object_count, Some(30));
        assert_eq!(process.gdi_object_count, Some(40));
        assert_eq!(process.gpu_percent, Some(5.5));
        assert_eq!(process.gpu_dedicated_bytes, Some(50));
        assert_eq!(process.gpu_shared_bytes, Some(60));
        assert_eq!(process.dotnet_heap_bytes, Some(70));
        assert_eq!(process.io_read_bytes_per_sec, Some(80));
        assert_eq!(process.io_write_bytes_per_sec, Some(90));
    }

    #[test]
    fn v2_log_loader_ignores_unknown_fields() {
        let path = unique_log_path("v2-unknown-fields");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","started_at":"2026-05-04T14:30:12+09:00","future_session_field":{"enabled":true}}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","future_frame_field":1,"system_metrics":{"physical_memory_bytes":1000,"future_system_metric":2},"processes":[{"pid":1,"name":"app.exe","future_process_field":3,"metrics":{"private_bytes":120,"future_process_metric":4}}]}"#,
            ],
        );

        let loaded = load_log(&path, SortSpec::default()).unwrap();

        assert_eq!(loaded.summary.frame_count, 1);
        assert_eq!(loaded.snapshot.used_memory, 1000);
        assert_eq!(loaded.snapshot.processes[0].private_bytes, Some(120));
    }

    #[test]
    fn v2_log_without_cpu_components_keeps_them_unavailable() {
        let path = unique_log_path("v2-cpu-compat");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":[]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":[],"system_metrics":{"cpu_percent":37},"processes":[]}"#,
            ],
        );

        let loaded = load_log(&path, SortSpec::default()).unwrap();

        assert_eq!(loaded.snapshot.cpu_total_usage_percent, Some(37));
        assert_eq!(loaded.snapshot.cpu_user_usage_percent, None);
        assert_eq!(loaded.snapshot.cpu_kernel_usage_percent, None);
    }

    #[test]
    fn log_view_load_keeps_all_log_frames_without_history_pruning() {
        let path = unique_log_path("long-log-view");
        let mut lines = vec![
            r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#.to_string(),
        ];
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-05-04T14:30:12+09:00").unwrap();
        for offset in 0..7_201 {
            let captured_at = started_at + chrono::Duration::seconds(offset);
            lines.push(format!(
                r#"{{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"{}","tracked_names":["app.exe"],"system_metrics":{{"physical_memory_bytes":{},"total_memory_bytes":8000}},"processes":[{{"pid":1,"name":"app.exe","start_time":100,"metrics":{{"private_bytes":{}}}}}]}}"#,
                captured_at.to_rfc3339(),
                offset,
                offset
            ));
        }
        let line_refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
        write_lines(&path, &line_refs);

        let loaded = load_log(&path, SortSpec::default()).unwrap();
        let identity = crate::model::ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(100),
        };

        assert_eq!(loaded.summary.frame_count, 7_201);
        assert_eq!(loaded.process_history.sample_count_for(&identity), 7_201);
        assert_eq!(loaded.system_history.len(), 7_201);
        assert_eq!(
            loaded
                .process_history
                .samples_for(&identity)
                .first()
                .and_then(|sample| sample.private_bytes),
            Some(0)
        );
    }

    #[test]
    fn log_summary_reads_session_and_tail_without_scanning_frames() {
        let path = unique_log_path("summary-process-names");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["ConfiguredButMissing.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["ConfiguredButMissing.exe"],"processes":[{"pid":1,"name":"Actual.exe","start_time":100,"metrics":{"private_bytes":120}},{"pid":2,"name":"Worker.exe","start_time":200,"metrics":{"private_bytes":220}}]}"#,
                r#"not json"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:13+09:00","tracked_names":["ConfiguredButMissing.exe"],"processes":[{"pid":3,"name":"Actual.exe","start_time":300,"metrics":{"private_bytes":320}}]}"#,
                r#"{"schema_version":2,"record_type":"end","session_id":"s2","ended_at":"2026-05-04T14:30:20+09:00","reason":"stopped"}"#,
            ],
        );

        let summary = summarize_log(&path);

        assert!(summary.tracked_names.is_empty());
        assert_eq!(summary.frame_count, 0);
        assert!(summary.error.is_none());
        assert_eq!(
            summary.ended_at.map(|value| value.timestamp()),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-05-04T14:30:20+09:00")
                    .unwrap()
                    .timestamp()
            )
        );
        assert!(load_log(&path, SortSpec::default()).is_err());
    }

    #[test]
    fn log_summary_uses_last_frame_time_when_end_record_is_missing() {
        let path = unique_log_path("summary-open-log");
        write_lines(
            &path,
            &[
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:15+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":120}}]}"#,
            ],
        );

        let summary = summarize_log(&path);

        assert_eq!(
            summary.ended_at.map(|value| value.timestamp()),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-05-04T14:30:15+09:00")
                    .unwrap()
                    .timestamp()
            )
        );
    }

    fn unique_log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "winproc-tui-{name}-{}-{}.log",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    fn write_lines(path: &Path, lines: &[&str]) {
        let mut file = File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }
}
