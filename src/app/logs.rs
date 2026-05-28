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
use serde_json::{Map, Value};

use crate::model::{
    ProcessHistory, ProcessRow, Snapshot, SortSpec, SystemHistory, sort_process_rows,
};

const SUPPORTED_LOG_SCHEMA_VERSION: u64 = 2;
const LOG_TAIL_READ_CHUNK_SIZE: usize = 8 * 1024;

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

    read_json_lines(path, |record| {
        require_supported_schema(record)?;
        update_summary_from_record(&mut summary, record);
        match record_type(record) {
            Some("session") => {
                session = SessionMeta::from_record(record);
            }
            Some("end") => {}
            Some("frame") => {
                let frame = parse_frame(record, &session)?;
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
            Some(other) => {
                return Err(anyhow!("unsupported record_type {other:?}"));
            }
            None => {
                return Err(anyhow!("log record is missing record_type"));
            }
        }
        Ok(())
    })?;

    let mut snapshot = last_snapshot.context("log contains no frames")?;
    sort_process_rows(&mut snapshot.processes, sort);
    snapshot.process_count = snapshot.processes.len();
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

fn read_json_lines<F>(path: &Path, mut handle: F) -> Result<()>
where
    F: FnMut(&Value) -> Result<()>,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read {} line {}", path.display(), line_index + 1)
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("invalid JSON at line {}", line_index + 1))?;
        handle(&value).with_context(|| format!("invalid log record at line {}", line_index + 1))?;
    }
    Ok(())
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

fn require_supported_schema(record: &Value) -> Result<()> {
    match record.get("schema_version").and_then(Value::as_u64) {
        Some(SUPPORTED_LOG_SCHEMA_VERSION) => Ok(()),
        Some(version) => Err(anyhow!("unsupported log schema_version {version}")),
        None => Err(anyhow!("log record is missing schema_version")),
    }
}

#[derive(Debug, Clone, Default)]
struct SessionMeta {
    cpu_name: Option<String>,
    cpu_frequency_mhz: Option<u64>,
    cpu_topology: Option<String>,
    cpu_cache: Option<String>,
    gpu_name: Option<String>,
}

impl SessionMeta {
    fn from_record(record: &Value) -> Self {
        let system = record.get("system").and_then(Value::as_object);
        Self {
            cpu_name: system.and_then(|value| string_from_map(value, "cpu_name")),
            cpu_frequency_mhz: system.and_then(|value| u64_from_map(value, "cpu_frequency_mhz")),
            cpu_topology: system.and_then(|value| string_from_map(value, "cpu_topology")),
            cpu_cache: system.and_then(|value| string_from_map(value, "cpu_cache")),
            gpu_name: system.and_then(|value| string_from_map(value, "gpu_name")),
        }
    }
}

struct ParsedFrame {
    snapshot: Snapshot,
    has_system_metrics: bool,
}

fn parse_frame(record: &Value, session: &SessionMeta) -> Result<ParsedFrame> {
    let captured_at = datetime_field(record, "captured_at")
        .ok_or_else(|| anyhow!("frame is missing captured_at"))?;
    let system = record.get("system_metrics").and_then(Value::as_object);
    let processes = record
        .get("processes")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_process).collect::<Result<Vec<_>>>())
        .transpose()?
        .unwrap_or_default();

    let snapshot = Snapshot {
        captured_at,
        total_memory: system
            .and_then(|metrics| u64_from_map(metrics, "total_memory_bytes"))
            .unwrap_or_default(),
        used_memory: system
            .and_then(|metrics| u64_from_map(metrics, "physical_memory_bytes"))
            .unwrap_or_default(),
        committed_memory: system.and_then(|metrics| u64_from_map(metrics, "committed_bytes")),
        commit_limit: system.and_then(|metrics| u64_from_map(metrics, "commit_limit_bytes")),
        gpu_dedicated_used: system.and_then(|metrics| u64_from_map(metrics, "gpu_dedicated_bytes")),
        gpu_dedicated_total: system
            .and_then(|metrics| u64_from_map(metrics, "gpu_dedicated_total_bytes")),
        gpu_shared_used: system.and_then(|metrics| u64_from_map(metrics, "gpu_shared_bytes")),
        gpu_shared_total: system
            .and_then(|metrics| u64_from_map(metrics, "gpu_shared_total_bytes")),
        cpu_name: session.cpu_name.clone(),
        cpu_frequency_mhz: session.cpu_frequency_mhz,
        cpu_current_frequency_mhz: None,
        cpu_p_core_frequency_mhz: None,
        cpu_e_core_frequency_mhz: None,
        cpu_total_usage_percent: system
            .and_then(|metrics| u64_from_map(metrics, "cpu_percent"))
            .and_then(|value| u8::try_from(value.min(100)).ok()),
        cpu_logical_processors: Vec::new(),
        cpu_topology: session.cpu_topology.clone(),
        cpu_cache: session.cpu_cache.clone(),
        gpu_name: session.gpu_name.clone(),
        disks: Vec::new(),
        process_count: processes.len(),
        processes,
    };

    Ok(ParsedFrame {
        snapshot,
        has_system_metrics: system.is_some(),
    })
}

fn parse_process(value: &Value) -> Result<ProcessRow> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("process entry is not an object"))?;
    let metrics = object
        .get("metrics")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(ProcessRow {
        pid: u32_from_map(object, "pid").ok_or_else(|| anyhow!("process is missing pid"))?,
        name: string_from_map(object, "name").ok_or_else(|| anyhow!("process is missing name"))?,
        start_time: u64_from_map(object, "start_time"),
        cpu_percent: f64_from_map(&metrics, "cpu_percent"),
        private_bytes: u64_from_map(&metrics, "private_bytes"),
        workset_bytes: u64_from_map(&metrics, "workset_bytes"),
        workset_private_bytes: u64_from_map(&metrics, "workset_private_bytes"),
        workset_shareable_bytes: u64_from_map(&metrics, "workset_shareable_bytes"),
        workset_shared_bytes: u64_from_map(&metrics, "workset_shared_bytes"),
        thread_count: u64_from_map(&metrics, "thread_count"),
        handle_count: u64_from_map(&metrics, "handle_count"),
        user_object_count: u64_from_map(&metrics, "user_object_count"),
        gdi_object_count: u64_from_map(&metrics, "gdi_object_count"),
        gpu_percent: f64_from_map(&metrics, "gpu_percent"),
        gpu_dedicated_bytes: u64_from_map(&metrics, "gpu_dedicated_bytes"),
        gpu_shared_bytes: u64_from_map(&metrics, "gpu_shared_bytes"),
        dotnet_heap_bytes: u64_from_map(&metrics, "dotnet_heap_bytes"),
        io_read_bytes_per_sec: u64_from_map(&metrics, "io_read_bytes_per_sec"),
        io_write_bytes_per_sec: u64_from_map(&metrics, "io_write_bytes_per_sec"),
    })
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

fn string_from_map(map: &Map<String, Value>, name: &str) -> Option<String> {
    map.get(name).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn u32_from_map(map: &Map<String, Value>, name: &str) -> Option<u32> {
    map.get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn u64_from_map(map: &Map<String, Value>, name: &str) -> Option<u64> {
    map.get(name).and_then(Value::as_u64)
}

fn f64_from_map(map: &Map<String, Value>, name: &str) -> Option<f64> {
    map.get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
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
                r#"{"schema_version":2,"record_type":"session","session_id":"s2","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system":{"cpu_name":"CPU"}}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s2","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":1000,"total_memory_bytes":8000,"cpu_percent":37},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":null,"handle_count":5}}]}"#,
            ],
        );

        let loaded = load_log(&path, SortSpec::default()).unwrap();

        assert_eq!(loaded.summary.schema_version, Some(2));
        assert_eq!(loaded.snapshot.cpu_name.as_deref(), Some("CPU"));
        assert_eq!(loaded.snapshot.cpu_total_usage_percent, Some(37));
        assert_eq!(loaded.snapshot.used_memory, 1000);
        assert_eq!(loaded.system_history.len(), 1);
        assert_eq!(
            loaded.system_history.samples()[0].value(SystemMetric::CpuAverage),
            Some(37)
        );
        assert_eq!(loaded.snapshot.processes[0].private_bytes, None);
        assert_eq!(loaded.snapshot.processes[0].handle_count, Some(5));
    }

    #[test]
    fn replay_load_keeps_all_log_frames_without_history_pruning() {
        let path = unique_log_path("long-replay");
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
