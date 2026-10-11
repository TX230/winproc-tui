use crate::{
    App,
    app::{DetailsMetric, GraphSlot, GraphValueFormat},
    model::history::{ProcessHistory, SystemHistory, SystemMetric, SystemSample},
};
use chrono::{DateTime, Local, SecondsFormat};
use crossterm::event::{KeyCode, KeyEvent};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CsvRange {
    #[default]
    All,
    Visible,
    Ab,
}
impl CsvRange {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All retained history",
            Self::Visible => "Visible time range",
            Self::Ab => "A-B interval",
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct CsvDialog {
    pub(crate) path: String,
    pub(crate) cursor: usize,
    pub(crate) range: CsvRange,
    pub(crate) range_focused: bool,
    pub(crate) error: String,
    result: Option<Receiver<Result<(PathBuf, usize), String>>>,
}
impl CsvDialog {
    pub(crate) fn busy(&self) -> bool {
        self.result.is_some()
    }
}
pub(crate) struct CsvSnapshot {
    pub(crate) sources: Vec<GraphSlot>,
    processes: ProcessHistory,
    system: SystemHistory,
    bounds: Option<(DateTime<Local>, DateTime<Local>)>,
}
impl App {
    pub(crate) fn open_graph_csv(&mut self) {
        if self.graph_entries.is_empty() {
            self.status = "No Graphs to export".into();
            return;
        }
        let path = format!("graphs-{}.csv", Local::now().format("%Y%m%d-%H%M%S"));
        self.graph_csv_dialog = Some(CsvDialog {
            cursor: path.len(),
            path,
            ..Default::default()
        });
    }
    pub(crate) fn csv_ab_available(&self) -> bool {
        self.ab_comparison
            .as_ref()
            .is_some_and(|ab| ab.a.is_some() && ab.b.is_some())
    }
    pub(crate) fn cycle_csv_range(&mut self, forward: bool) {
        let ab = self.csv_ab_available();
        if let Some(d) = self.graph_csv_dialog.as_mut() {
            let count = if ab { 3 } else { 2 };
            let index = match d.range {
                CsvRange::All => 0,
                CsvRange::Visible => 1,
                CsvRange::Ab => 2,
            };
            d.range = [CsvRange::All, CsvRange::Visible, CsvRange::Ab]
                [(index + if forward { 1 } else { count - 1 }) % count];
        }
    }
    pub(crate) fn graph_csv_key(&mut self, key: KeyEvent) {
        let Some(d) = self.graph_csv_dialog.as_mut() else {
            return;
        };
        if d.busy() {
            return;
        }
        if !d.range_focused
            && key.modifiers == crossterm::event::KeyModifiers::CONTROL
            && matches!(key.code, KeyCode::Char('v' | 'V'))
        {
            match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(text) => {
                    let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
                    d.path.insert_str(d.cursor, &text);
                    d.cursor += text.len();
                }
                Err(e) => d.error = format!("Paste failed: {e}"),
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.graph_csv_dialog = None,
            KeyCode::Enter => self.start_graph_csv(),
            KeyCode::Tab | KeyCode::BackTab => d.range_focused = !d.range_focused,
            KeyCode::Left | KeyCode::Up if d.range_focused => self.cycle_csv_range(false),
            KeyCode::Right | KeyCode::Down | KeyCode::Char(' ') if d.range_focused => {
                self.cycle_csv_range(true)
            }
            _ if !d.range_focused => {
                super::text_input::edit(&mut d.path, &mut d.cursor, key);
            }
            _ => {}
        }
    }
    pub(crate) fn csv_snapshot(&self, range: CsvRange) -> Result<CsvSnapshot, String> {
        let bounds = match range {
            CsvRange::All => None,
            CsvRange::Visible => {
                let right = self
                    .graph_time_reference_at()
                    .ok_or("No retained samples")?
                    - chrono::Duration::seconds(self.effective_graph_time_offset_seconds().into());
                Some((
                    right
                        - chrono::Duration::seconds(
                            self.effective_graph_time_span_seconds().into(),
                        ),
                    right,
                ))
            }
            CsvRange::Ab => {
                let ab = self
                    .ab_comparison
                    .as_ref()
                    .ok_or("Set both A and B first")?;
                let a = ab.a.ok_or("Set A first")?.captured_at;
                let b = ab.b.ok_or("Set B first")?.captured_at;
                Some((a.min(b), a.max(b)))
            }
        };
        Ok(CsvSnapshot {
            sources: self
                .graph_entries
                .iter()
                .map(|e| e.source.clone())
                .collect(),
            processes: self.display_process_history().clone(),
            system: self.display_system_history().clone(),
            bounds,
        })
    }
    fn start_graph_csv(&mut self) {
        let Some(d) = self.graph_csv_dialog.as_ref() else {
            return;
        };
        if d.path.trim().is_empty() {
            self.graph_csv_dialog.as_mut().unwrap().error = "Enter a destination path".into();
            return;
        }
        let path = PathBuf::from(&d.path);
        let snapshot = match self.csv_snapshot(d.range) {
            Ok(s) => s,
            Err(e) => {
                self.graph_csv_dialog.as_mut().unwrap().error = e;
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("graph-csv".into())
            .spawn(move || {
                let result = snapshot.write_file(&path).map(|rows| (path, rows));
                let _ = tx.send(result);
            }) {
            Ok(_) => {
                let d = self.graph_csv_dialog.as_mut().unwrap();
                d.result = Some(rx);
                d.error.clear();
            }
            Err(e) => self.graph_csv_dialog.as_mut().unwrap().error = e.to_string(),
        }
    }
    pub(crate) fn poll_graph_csv(&mut self) -> bool {
        let Some(rx) = self
            .graph_csv_dialog
            .as_ref()
            .and_then(|d| d.result.as_ref())
        else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err("CSV export worker stopped".into()),
        };
        match result {
            Ok((path, rows)) => {
                self.graph_csv_dialog = None;
                self.status = format!("Exported {rows} CSV rows: {}", path.display());
            }
            Err(e) => {
                let d = self.graph_csv_dialog.as_mut().unwrap();
                d.result = None;
                d.error = format!("Export failed: {e}");
            }
        }
        true
    }
}
impl CsvSnapshot {
    pub(crate) fn rows(&self) -> BTreeMap<DateTime<Local>, Vec<String>> {
        let mut rows = BTreeMap::new();
        for (index, source) in self.sources.iter().enumerate() {
            let mut add = |time, value: Option<String>| {
                if self.bounds.is_some_and(|(a, b)| time < a || time > b) {
                    return;
                }
                rows.entry(time)
                    .or_insert_with(|| vec![String::new(); self.sources.len()])[index] =
                    value.unwrap_or_default();
            };
            match source {
                GraphSlot::Process { identity, metric } => {
                    for s in self.processes.samples_for_iter(identity) {
                        add(s.captured_at, process_value(s, *metric));
                    }
                }
                GraphSlot::System { metric } => {
                    for s in self.system.samples_iter() {
                        add(s.captured_at, system_value(s, *metric));
                    }
                }
                GraphSlot::Gpu {
                    adapter_id, metric, ..
                } => {
                    for s in self.system.samples_iter() {
                        add(s.captured_at, gpu_value(s, *adapter_id, *metric));
                    }
                }
            }
        }
        rows
    }
    pub(crate) fn write_to(&self, output: &mut impl Write) -> std::io::Result<usize> {
        let headers = self
            .sources
            .iter()
            .map(source_header)
            .map(|s| csv_escape(&s))
            .collect::<Vec<_>>();
        writeln!(output, "timestamp,{}", headers.join(","))?;
        let rows = self.rows();
        for (time, values) in &rows {
            writeln!(
                output,
                "{},{}",
                time.to_rfc3339_opts(SecondsFormat::Nanos, false),
                values.join(",")
            )?;
        }
        output.flush()?;
        Ok(rows.len())
    }
    pub(crate) fn write_file(&self, path: &Path) -> Result<usize, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "File exists. Choose a new destination.".to_string()
                } else {
                    e.to_string()
                }
            })?;
        self.write_to(&mut BufWriter::new(file))
            .map_err(|e| format!("{e} (partial file kept)"))
    }
}
fn source_header(source: &GraphSlot) -> String {
    let target = match source {
        GraphSlot::Process { identity, .. } => format!(
            "process={} pid={} start_unix={}",
            identity.name,
            identity.pid,
            identity
                .start_time
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
        GraphSlot::System { .. } => "system".into(),
        GraphSlot::Gpu {
            adapter_id,
            adapter_name,
            ..
        } => format!(
            "gpu={adapter_name} adapter={:08x}:{:08x}",
            adapter_id.high, adapter_id.low
        ),
    };
    let unit = match source {
        GraphSlot::System {
            metric: SystemMetric::PagesInput | SystemMetric::PagesOutput,
        } => "pages/s",
        _ => match source.value_format() {
            GraphValueFormat::Bytes => "bytes",
            GraphValueFormat::Percent => "%",
            GraphValueFormat::Count => "count",
            GraphValueFormat::QueueLength => "requests",
            GraphValueFormat::BytesPerSec
            | GraphValueFormat::AdaptiveBitsPerSec
            | GraphValueFormat::MegabitsPerSec
            | GraphValueFormat::MegabytesPerSec => "bytes/s",
        },
    };
    format!("{} [{}] {target}", source.metric_label(), unit)
}
fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into()
    }
}
fn float_text(v: Option<f64>) -> Option<String> {
    v.filter(|v| v.is_finite()).map(|v| v.to_string())
}
fn process_value(
    sample: &crate::model::history::ProcessSample,
    metric: DetailsMetric,
) -> Option<String> {
    match metric {
        DetailsMetric::CpuPercent => float_text(sample.cpu_percent),
        DetailsMetric::GpuPercent => float_text(sample.gpu_percent),
        DetailsMetric::Private => sample.private_bytes.map(|value| value.to_string()),
        DetailsMetric::Workset => sample.workset_bytes.map(|value| value.to_string()),
        DetailsMetric::WorksetPrivate => {
            sample.workset_private_bytes.map(|value| value.to_string())
        }
        DetailsMetric::WorksetShareable => sample
            .workset_shareable_bytes
            .map(|value| value.to_string()),
        DetailsMetric::ThreadCount => sample.thread_count.map(|value| value.to_string()),
        DetailsMetric::HandleCount => sample.handle_count.map(|value| value.to_string()),
        DetailsMetric::UserObjectCount => sample.user_object_count.map(|value| value.to_string()),
        DetailsMetric::GdiObjectCount => sample.gdi_object_count.map(|value| value.to_string()),
        DetailsMetric::DotNetHeap => sample.dotnet_heap_bytes.map(|value| value.to_string()),
        DetailsMetric::DotNetGcGen0Heap => sample
            .dotnet_gc_gen0_heap_bytes
            .map(|value| value.to_string()),
        DetailsMetric::DotNetGcGen1Heap => sample
            .dotnet_gc_gen1_heap_bytes
            .map(|value| value.to_string()),
        DetailsMetric::DotNetGcGen2Heap => sample
            .dotnet_gc_gen2_heap_bytes
            .map(|value| value.to_string()),
        DetailsMetric::DotNetGcLoh => sample.dotnet_gc_loh_bytes.map(|value| value.to_string()),
        DetailsMetric::DotNetGcPoh => sample.dotnet_gc_poh_bytes.map(|value| value.to_string()),
        DetailsMetric::DotNetGcCommitted => sample
            .dotnet_gc_committed_bytes
            .map(|value| value.to_string()),
        DetailsMetric::DotNetGcFragmentation => sample
            .dotnet_gc_fragmentation_bytes
            .map(|value| value.to_string()),
        DetailsMetric::DotNetAllocation => sample
            .dotnet_allocation_bytes_per_sec
            .map(|value| value.to_string()),
        DetailsMetric::GpuDedicated => sample.gpu_dedicated_bytes.map(|value| value.to_string()),
        DetailsMetric::GpuShared => sample.gpu_shared_bytes.map(|value| value.to_string()),
        DetailsMetric::IoRead => sample.io_read_bytes_per_sec.map(|value| value.to_string()),
        DetailsMetric::IoWrite => sample.io_write_bytes_per_sec.map(|value| value.to_string()),
    }
}

fn system_value(sample: &SystemSample, metric: SystemMetric) -> Option<String> {
    match metric {
        SystemMetric::CpuAverage => sample.cpu_average_percent.map(|value| value.to_string()),
        SystemMetric::PhysicalMemory => sample.physical_memory_bytes.map(|value| value.to_string()),
        SystemMetric::ModifiedMemory => sample.modified_memory_bytes.map(|value| value.to_string()),
        SystemMetric::StandbyMemory => sample.standby_memory_bytes.map(|value| value.to_string()),
        SystemMetric::FreeZeroedMemory => sample
            .free_zeroed_memory_bytes
            .map(|value| value.to_string()),
        SystemMetric::Committed => sample.committed_bytes.map(|value| value.to_string()),
        SystemMetric::PagedPool => sample.paged_pool_bytes.map(|value| value.to_string()),
        SystemMetric::NonpagedPool => sample.nonpaged_pool_bytes.map(|value| value.to_string()),
        SystemMetric::PagesInput => sample.pages_input_per_sec.map(|value| value.to_string()),
        SystemMetric::PagesOutput => sample.pages_output_per_sec.map(|value| value.to_string()),
        SystemMetric::ThreadCount => sample.thread_count.map(|value| value.to_string()),
        SystemMetric::ProcessCount => sample.process_count.map(|value| value.to_string()),
        SystemMetric::GpuUtilization
        | SystemMetric::GpuEncode
        | SystemMetric::GpuDecode
        | SystemMetric::GpuDedicated
        | SystemMetric::GpuShared => None,
        SystemMetric::NetworkReceived => sample
            .network_received_bytes_per_sec
            .map(|value| value.to_string()),
        SystemMetric::NetworkSent => sample
            .network_sent_bytes_per_sec
            .map(|value| value.to_string()),
        SystemMetric::DiskRead => sample
            .disk_read_bytes_per_sec
            .map(|value| value.to_string()),
        SystemMetric::DiskWrite => sample
            .disk_write_bytes_per_sec
            .map(|value| value.to_string()),
        SystemMetric::DiskQueueLength => float_text(sample.disk_queue_length),
    }
}

fn gpu_value(
    sample: &SystemSample,
    adapter_id: crate::model::GpuAdapterId,
    metric: SystemMetric,
) -> Option<String> {
    let adapter = sample
        .gpu_adapters
        .iter()
        .find(|adapter| adapter.id == adapter_id)?;
    match metric {
        SystemMetric::GpuUtilization => float_text(adapter.utilization_percent),
        SystemMetric::GpuEncode => float_text(adapter.encode.average_percent),
        SystemMetric::GpuDecode => float_text(adapter.decode.average_percent),
        SystemMetric::GpuDedicated => adapter.dedicated_used.map(|value| value.to_string()),
        SystemMetric::GpuShared => adapter.shared_used.map(|value| value.to_string()),
        _ => None,
    }
}
