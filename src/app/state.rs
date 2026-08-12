use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use ratatui::{layout::Rect, widgets::TableState};

use crate::{
    app::export::RecordingSession,
    app::logs::{LoadedLog, LogListResult, LogListWorker, LogLoadWorker, LogSummary},
    app::path_completion::{PathCompletion, PathCompletionState},
    config::{
        EMPTY_TRACKED_LIST_NAME, RuntimeConfig, TrackedListStartup, is_empty_tracked_list_name,
    },
    model::{
        ColumnPreset, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY, MetricColumn, ProcessColumnWidths,
        ProcessEnvironmentError, ProcessEnvironmentReport, ProcessHistory, ProcessIdentity,
        ProcessInfo, ProcessModulesError, ProcessModulesReport, ProcessRow, ProcessSample,
        Snapshot, SortColumn, SortDirection, SortSpec, SystemHistory, SystemMetric,
        TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY, sort_process_rows,
    },
    samplers::{
        CollectSnapshotResult, SamplingRuntime, SamplingWorker,
        open_files::{OpenFilesReport, OpenFilesResult, OpenFilesWorker},
        process_environment::{ProcessEnvironmentResult, ProcessEnvironmentWorker},
        process_info::{ProcessInfoResult, ProcessInfoWorker},
        process_modules::{ProcessModulesResult, ProcessModulesWorker},
    },
    ui::{
        THEMES, Theme, column_picker_row_for_index, column_picker_scroll_max_for_page_size,
        format::{
            format_compact_bytes, format_integer, format_io_rate, format_signed_compact_bytes,
            format_signed_integer, format_signed_io_rate,
        },
        help_scroll_max_for_page_size, log_list_total_rows_for_count, theme_index_by_name,
        widgets::scrollable_modal::ScrollableModalState,
    },
};

const GRAPH_TIME_SPAN_MIN_SECONDS: u16 = 60;
const LIVE_GRAPH_TIME_MAX_SECONDS: u32 = TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY as u32;
const GRAPH_TIME_SPAN_STEPS_SECONDS: &[u32] = &[60, 120, 300, 600, 900, 1_800, 3_600, 7_200];
const FIXED_PROCESS_COLUMN_COUNT: usize = 2;
pub(crate) const GRAPH_LIMIT: usize = 16;
pub(crate) const GRAPH_SLOT_MIN_HEIGHT: u16 = 13;
pub(crate) const GRAPH_SLOT_MIN_WIDTH: u16 = 50;
pub(crate) const GRAPH_SOURCE_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);
pub(crate) const PROCESS_INFO_DEBOUNCE: Duration = Duration::from_millis(200);
const PROCESS_INFO_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OPEN_FILES_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_MODULES_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_ENVIRONMENT_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_NAVIGATION_ORDER_HOLD: Duration = Duration::from_millis(750);
pub(crate) const SAMPLE_STALE_AFTER_SECONDS: u64 = 3;
const PROCESS_INFO_METRIC_COLUMNS: [MetricColumn; 14] = [
    MetricColumn::CpuPercent,
    MetricColumn::PrivateBytes,
    MetricColumn::WorksetBytes,
    MetricColumn::WorksetPrivateBytes,
    MetricColumn::ThreadCount,
    MetricColumn::HandleCount,
    MetricColumn::UserObjectCount,
    MetricColumn::GdiObjectCount,
    MetricColumn::GpuPercent,
    MetricColumn::DotNetHeapBytes,
    MetricColumn::GpuDedicatedBytes,
    MetricColumn::GpuSharedBytes,
    MetricColumn::IoReadBytesPerSec,
    MetricColumn::IoWriteBytesPerSec,
];

const fn process_info_metric_label(column: MetricColumn) -> &'static str {
    match column {
        MetricColumn::CpuPercent => "CPU Usage",
        MetricColumn::PrivateBytes => "Private Bytes",
        MetricColumn::WorksetBytes => "Working Set",
        MetricColumn::WorksetPrivateBytes => "Working Set - Private",
        MetricColumn::WorksetShareableBytes => "Working Set - Shareable",
        MetricColumn::WorksetSharedBytes => "Working Set - Shared",
        MetricColumn::ThreadCount => "Threads",
        MetricColumn::HandleCount => "Handles",
        MetricColumn::UserObjectCount => "USER Objects",
        MetricColumn::GdiObjectCount => "GDI Objects",
        MetricColumn::GpuPercent => "GPU Usage",
        MetricColumn::DotNetHeapBytes => ".NET Heap",
        MetricColumn::GpuDedicatedBytes => "GPU Dedicated Memory",
        MetricColumn::GpuSharedBytes => "GPU Shared Memory",
        MetricColumn::IoReadBytesPerSec => "I/O Read Throughput",
        MetricColumn::IoWriteBytesPerSec => "I/O Write Throughput",
        MetricColumn::FullPath => "Full Path",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessLifecycle {
    Live,
    Exited { exited_at: DateTime<Local> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VisibleProcessEntry {
    Live(usize),
    Ghost(ProcessIdentity),
}

#[derive(Debug, Clone)]
pub(crate) struct VisibleProcessRow<'a> {
    pub(crate) process: &'a ProcessRow,
    pub(crate) tracked: bool,
    pub(crate) lifecycle: ProcessLifecycle,
    pub(crate) multi_selected: bool,
    pub(crate) is_tracked_total: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ExitedTrackedRow {
    pub(crate) process: ProcessRow,
    pub(crate) exited_at: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingProcessInfo {
    pub(crate) generation: u64,
    pub(crate) identity: ProcessIdentity,
    pub(crate) process: ProcessRow,
    pub(crate) lifecycle: ProcessLifecycle,
    pub(crate) changed_at: Instant,
    pub(crate) force_refresh: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PausedDisplay {
    pub(crate) snapshot: Snapshot,
    pub(crate) exited_tracked_rows: HashMap<ProcessIdentity, ExitedTrackedRow>,
    pub(crate) process_history: ProcessHistory,
    pub(crate) system_history: SystemHistory,
    pub(crate) process_info_cache: HashMap<ProcessIdentity, ProcessInfo>,
    pub(crate) process_info_display_identity: Option<ProcessIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailsMetric {
    CpuPercent,
    Private,
    Workset,
    WorksetPrivate,
    WorksetShareable,
    WorksetShared,
    ThreadCount,
    HandleCount,
    UserObjectCount,
    GdiObjectCount,
    GpuPercent,
    DotNetHeap,
    GpuDedicated,
    GpuShared,
    IoRead,
    IoWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphValueFormat {
    Bytes,
    Count,
    Percent,
    AdaptiveBitsPerSec,
    MegabitsPerSec,
    MegabytesPerSec,
    QueueLength,
}

impl GraphValueFormat {
    pub(crate) fn from_details_metric(metric: DetailsMetric) -> Self {
        match metric {
            DetailsMetric::CpuPercent | DetailsMetric::GpuPercent => Self::Percent,
            DetailsMetric::IoRead | DetailsMetric::IoWrite => Self::AdaptiveBitsPerSec,
            DetailsMetric::Private
            | DetailsMetric::Workset
            | DetailsMetric::WorksetPrivate
            | DetailsMetric::WorksetShareable
            | DetailsMetric::WorksetShared
            | DetailsMetric::DotNetHeap
            | DetailsMetric::GpuDedicated
            | DetailsMetric::GpuShared => Self::Bytes,
            DetailsMetric::ThreadCount
            | DetailsMetric::HandleCount
            | DetailsMetric::UserObjectCount
            | DetailsMetric::GdiObjectCount => Self::Count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailsTarget {
    Process,
    #[cfg(test)]
    System(SystemMetric),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AbComparisonPoint {
    pub(crate) captured_at: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbComparison {
    pub(crate) a: Option<AbComparisonPoint>,
    pub(crate) b: Option<AbComparisonPoint>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfoDialogTarget {
    pub(crate) identity: ProcessIdentity,
    pub(crate) process: ProcessRow,
    pub(crate) lifecycle: ProcessLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessInfoTab {
    Metrics,
    Image,
    Files,
    Dlls,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessInfoFocus {
    Content,
    Close,
}

impl ProcessInfoTab {
    pub(crate) const ALL: [Self; 5] = [
        Self::Metrics,
        Self::Image,
        Self::Files,
        Self::Dlls,
        Self::Environment,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Metrics => "Metrics",
            Self::Image => "Image",
            Self::Files => "Files",
            Self::Dlls => "DLLs",
            Self::Environment => "Environment",
        }
    }

    pub(crate) fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub(crate) fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Metrics => 0,
            Self::Image => 1,
            Self::Files => 2,
            Self::Dlls => 3,
            Self::Environment => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessInfoMetricRow {
    pub(crate) label: &'static str,
    pub(crate) value: String,
    pub(crate) delta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessInfoMetricsView {
    pub(crate) value_heading: &'static str,
    pub(crate) delta_heading: Option<&'static str>,
    pub(crate) range: String,
    pub(crate) rows: Vec<ProcessInfoMetricRow>,
}

#[derive(Debug, Clone, Copy)]
enum ProcessMetricValue {
    Percent(f64),
    Bytes(u64),
    Count(u64),
    IoRate(u64),
}

impl ProcessMetricValue {
    fn from_sample(sample: &ProcessSample, column: MetricColumn) -> Option<Self> {
        match column {
            MetricColumn::CpuPercent => sample.cpu_percent.map(Self::Percent),
            MetricColumn::PrivateBytes => sample.private_bytes.map(Self::Bytes),
            MetricColumn::WorksetBytes => sample.workset_bytes.map(Self::Bytes),
            MetricColumn::WorksetPrivateBytes => sample.workset_private_bytes.map(Self::Bytes),
            MetricColumn::ThreadCount => sample.thread_count.map(Self::Count),
            MetricColumn::HandleCount => sample.handle_count.map(Self::Count),
            MetricColumn::UserObjectCount => sample.user_object_count.map(Self::Count),
            MetricColumn::GdiObjectCount => sample.gdi_object_count.map(Self::Count),
            MetricColumn::GpuPercent => sample.gpu_percent.map(Self::Percent),
            MetricColumn::DotNetHeapBytes => sample.dotnet_heap_bytes.map(Self::Bytes),
            MetricColumn::GpuDedicatedBytes => sample.gpu_dedicated_bytes.map(Self::Bytes),
            MetricColumn::GpuSharedBytes => sample.gpu_shared_bytes.map(Self::Bytes),
            MetricColumn::IoReadBytesPerSec => sample.io_read_bytes_per_sec.map(Self::IoRate),
            MetricColumn::IoWriteBytesPerSec => sample.io_write_bytes_per_sec.map(Self::IoRate),
            MetricColumn::WorksetShareableBytes
            | MetricColumn::WorksetSharedBytes
            | MetricColumn::FullPath => None,
        }
    }

    fn format(self) -> String {
        match self {
            Self::Percent(value) => format!("{value:.1}%"),
            Self::Bytes(value) => format_compact_bytes(value),
            Self::Count(value) => format_integer(value),
            Self::IoRate(value) => format_io_rate(value),
        }
    }

    fn format_delta(self, baseline: Self) -> Option<String> {
        match (self, baseline) {
            (Self::Percent(value), Self::Percent(baseline)) => {
                Some(format!("{:+.1}%", value - baseline))
            }
            (Self::Bytes(value), Self::Bytes(baseline)) => Some(format_signed_compact_bytes(
                i128::from(value) - i128::from(baseline),
            )),
            (Self::Count(value), Self::Count(baseline)) => Some(format_signed_integer(
                i128::from(value) - i128::from(baseline),
            )),
            (Self::IoRate(value), Self::IoRate(baseline)) => Some(format_signed_io_rate(
                i128::from(value) - i128::from(baseline),
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphSlot {
    Process {
        identity: ProcessIdentity,
        metric: DetailsMetric,
    },
    System {
        metric: SystemMetric,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GraphId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphHoverTarget {
    ZoomOut,
    ZoomIn,
    Remove(GraphId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphEntry {
    pub(crate) id: GraphId,
    pub(crate) source: GraphSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GraphSourceState {
    pub(crate) ordinal: usize,
    pub(crate) active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphSourceClick {
    pub(crate) source: GraphSlot,
    pub(crate) clicked_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GraphSample {
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DetailsSampleViewState {
    pub(crate) selected_index: usize,
    pub(crate) selected_exact: bool,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphPanDragButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GraphPanDrag {
    pub(crate) button: GraphPanDragButton,
    pub(crate) start_x: u16,
    pub(crate) start_offset_seconds: u32,
}

impl GraphSlot {
    pub(crate) fn process(identity: ProcessIdentity, metric: DetailsMetric) -> Self {
        Self::Process { identity, metric }
    }

    pub(crate) fn system(metric: SystemMetric) -> Self {
        Self::System { metric }
    }

    pub(crate) fn process_identity(&self) -> Option<&ProcessIdentity> {
        match self {
            Self::Process { identity, .. } => Some(identity),
            Self::System { .. } => None,
        }
    }

    pub(crate) fn metric_label(&self) -> &'static str {
        match self {
            Self::Process { metric, .. } => metric.label(),
            Self::System { metric } => metric.label(),
        }
    }

    pub(crate) fn item_label(&self) -> String {
        match self {
            Self::Process { identity, .. } => identity.name.clone(),
            Self::System { metric } => metric.panel_label().to_string(),
        }
    }

    pub(crate) fn graph_title_target_label(&self) -> &str {
        match self {
            Self::Process { identity, .. } => &identity.name,
            Self::System { .. } => "SYSTEM",
        }
    }

    pub(crate) fn description(&self) -> String {
        format!("{} · {}", self.item_label(), self.metric_label())
    }

    pub(crate) fn value_format(&self) -> GraphValueFormat {
        match self {
            Self::Process { metric, .. } => GraphValueFormat::from_details_metric(*metric),
            Self::System {
                metric: SystemMetric::CpuAverage,
            } => GraphValueFormat::Percent,
            Self::System {
                metric: SystemMetric::NetworkReceived | SystemMetric::NetworkSent,
            } => GraphValueFormat::MegabitsPerSec,
            Self::System {
                metric: SystemMetric::DiskRead | SystemMetric::DiskWrite,
            } => GraphValueFormat::MegabytesPerSec,
            Self::System {
                metric: SystemMetric::DiskQueueLength,
            } => GraphValueFormat::QueueLength,
            Self::System { .. } => GraphValueFormat::Bytes,
        }
    }
}

impl DetailsMetric {
    pub(crate) fn label(self) -> &'static str {
        self.column().label()
    }

    pub(crate) fn column(self) -> crate::model::MetricColumn {
        match self {
            Self::CpuPercent => crate::model::MetricColumn::CpuPercent,
            Self::Private => crate::model::MetricColumn::PrivateBytes,
            Self::Workset => crate::model::MetricColumn::WorksetBytes,
            Self::WorksetPrivate => crate::model::MetricColumn::WorksetPrivateBytes,
            Self::WorksetShareable => crate::model::MetricColumn::WorksetShareableBytes,
            Self::WorksetShared => crate::model::MetricColumn::WorksetSharedBytes,
            Self::ThreadCount => crate::model::MetricColumn::ThreadCount,
            Self::HandleCount => crate::model::MetricColumn::HandleCount,
            Self::UserObjectCount => crate::model::MetricColumn::UserObjectCount,
            Self::GdiObjectCount => crate::model::MetricColumn::GdiObjectCount,
            Self::GpuPercent => crate::model::MetricColumn::GpuPercent,
            Self::DotNetHeap => crate::model::MetricColumn::DotNetHeapBytes,
            Self::GpuDedicated => crate::model::MetricColumn::GpuDedicatedBytes,
            Self::GpuShared => crate::model::MetricColumn::GpuSharedBytes,
            Self::IoRead => crate::model::MetricColumn::IoReadBytesPerSec,
            Self::IoWrite => crate::model::MetricColumn::IoWriteBytesPerSec,
        }
    }

    #[cfg(test)]
    fn toggled(self) -> Self {
        if self == Self::Private {
            Self::WorksetPrivate
        } else {
            Self::Private
        }
    }
}

impl DetailsMetric {
    pub(crate) fn from_graphable_column(column: MetricColumn) -> Option<Self> {
        if !column.is_graphable() {
            return None;
        }
        match column {
            MetricColumn::CpuPercent => Some(Self::CpuPercent),
            MetricColumn::PrivateBytes => Some(Self::Private),
            MetricColumn::WorksetBytes => Some(Self::Workset),
            MetricColumn::WorksetPrivateBytes => Some(Self::WorksetPrivate),
            MetricColumn::WorksetShareableBytes => Some(Self::WorksetShareable),
            MetricColumn::WorksetSharedBytes => Some(Self::WorksetShared),
            MetricColumn::ThreadCount => Some(Self::ThreadCount),
            MetricColumn::HandleCount => Some(Self::HandleCount),
            MetricColumn::UserObjectCount => Some(Self::UserObjectCount),
            MetricColumn::GdiObjectCount => Some(Self::GdiObjectCount),
            MetricColumn::GpuPercent => Some(Self::GpuPercent),
            MetricColumn::DotNetHeapBytes => Some(Self::DotNetHeap),
            MetricColumn::GpuDedicatedBytes => Some(Self::GpuDedicated),
            MetricColumn::GpuSharedBytes => Some(Self::GpuShared),
            MetricColumn::IoReadBytesPerSec => Some(Self::IoRead),
            MetricColumn::IoWriteBytesPerSec => Some(Self::IoWrite),
            MetricColumn::FullPath => unreachable!("non-graphable column returned early"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusedPanel {
    System,
    SystemActivity,
    Cpu,
    Processes,
    DetailsGraph,
    DetailsSamples,
}

impl FocusedPanel {
    fn next(self, details_visible: bool) -> Self {
        match (self, details_visible) {
            (Self::System, _) => Self::SystemActivity,
            (Self::SystemActivity, _) => Self::Cpu,
            (Self::Cpu, _) => Self::Processes,
            (Self::Processes, true) => Self::DetailsGraph,
            (Self::Processes, false) => Self::System,
            (Self::DetailsGraph, _) => Self::DetailsSamples,
            (Self::DetailsSamples, _) => Self::System,
        }
    }

    fn previous(self, details_visible: bool) -> Self {
        match (self, details_visible) {
            (Self::System, true) => Self::DetailsSamples,
            (Self::System, false) => Self::Processes,
            (Self::SystemActivity, _) => Self::System,
            (Self::Cpu, _) => Self::SystemActivity,
            (Self::Processes, _) => Self::Cpu,
            (Self::DetailsGraph, _) => Self::Processes,
            (Self::DetailsSamples, _) => Self::DetailsGraph,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "RAM/VRAM",
            Self::SystemActivity => "NW/DISK",
            Self::Cpu => "CPUs",
            Self::Processes => "Processes",
            Self::DetailsGraph => "Graph",
            Self::DetailsSamples => "Samples",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitConfirmSelection {
    Quit,
    Cancel,
}

impl QuitConfirmSelection {
    fn toggled(self) -> Self {
        match self {
            Self::Quit => Self::Cancel,
            Self::Cancel => Self::Quit,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GraphSlotLayout {
    #[default]
    Auto,
    OneColumn,
    TwoColumns,
    ThreeColumns,
}

impl GraphSlotLayout {
    pub(crate) const fn columns(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::OneColumn => 1,
            Self::TwoColumns => 2,
            Self::ThreeColumns => 3,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::OneColumn => "1 col",
            Self::TwoColumns => "2 cols",
            Self::ThreeColumns => "3 cols",
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Auto => Self::OneColumn,
            Self::OneColumn => Self::TwoColumns,
            Self::TwoColumns => Self::ThreeColumns,
            Self::ThreeColumns => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordingOverwriteSelection {
    Overwrite,
    Cancel,
}

impl RecordingOverwriteSelection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Overwrite => Self::Cancel,
            Self::Cancel => Self::Overwrite,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordingStopSelection {
    Stop,
    Continue,
}

impl RecordingStopSelection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Stop => Self::Continue,
            Self::Continue => Self::Stop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordingErrorKind {
    CouldNotStart,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordingErrorDialog {
    pub(crate) path: PathBuf,
    pub(crate) message: String,
    pub(crate) kind: RecordingErrorKind,
    pub(crate) return_to_path_dialog: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordingPathSelection {
    Path,
    Start,
    Cancel,
}

impl RecordingPathSelection {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Path => Self::Start,
            Self::Start => Self::Cancel,
            Self::Cancel => Self::Path,
        }
    }

    pub(crate) const fn previous(self) -> Self {
        match self {
            Self::Path => Self::Cancel,
            Self::Start => Self::Path,
            Self::Cancel => Self::Start,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LogListClick {
    pub(crate) index: usize,
    pub(crate) at: Instant,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LogListFocus {
    #[default]
    List,
    Open,
    Directory,
    Refresh,
    Close,
}

impl LogListFocus {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::List => Self::Open,
            Self::Open => Self::Directory,
            Self::Directory => Self::Refresh,
            Self::Refresh => Self::Close,
            Self::Close => Self::List,
        }
    }

    pub(crate) const fn previous(self) -> Self {
        match self {
            Self::List => Self::Close,
            Self::Open => Self::List,
            Self::Directory => Self::Open,
            Self::Refresh => Self::Directory,
            Self::Close => Self::Refresh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogDirSelection {
    Path,
    Apply,
    Cancel,
}

impl LogDirSelection {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Path => Self::Apply,
            Self::Apply => Self::Cancel,
            Self::Cancel => Self::Path,
        }
    }

    pub(crate) const fn previous(self) -> Self {
        match self {
            Self::Path => Self::Cancel,
            Self::Apply => Self::Path,
            Self::Cancel => Self::Apply,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackedRemoveSelection {
    Remove,
    Cancel,
}

impl TrackedRemoveSelection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Remove => Self::Cancel,
            Self::Cancel => Self::Remove,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackedListsButton {
    Save,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackedListConfirmSelection {
    Apply,
    Cancel,
}

impl TrackedListConfirmSelection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Apply => Self::Cancel,
            Self::Cancel => Self::Apply,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingTrackedListSwitch {
    pub(crate) target_name: Option<String>,
    pub(crate) target_processes: Vec<String>,
    pub(crate) removed_name_count: usize,
    pub(crate) affected_name_count: usize,
    pub(crate) discarded_sample_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum TrackedListsView {
    Browse,
    NameInput {
        draft: String,
        cursor: usize,
        error: Option<String>,
    },
    ConfirmDelete {
        name: String,
        selection: TrackedListConfirmSelection,
    },
    ConfirmSwitch {
        pending: PendingTrackedListSwitch,
        selection: TrackedListConfirmSelection,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct TrackedListsDialog {
    pub(crate) index: usize,
    pub(crate) scroll: ScrollableModalState,
    pub(crate) view: TrackedListsView,
    pub(crate) focused_button: Option<TrackedListsButton>,
    pub(crate) save_name_focused: bool,
    pub(crate) startup_focused: bool,
    pub(crate) save_name_draft: String,
    pub(crate) save_name_cursor: usize,
    pub(crate) save_name_error: Option<String>,
    pub(crate) save_name_feedback: Option<String>,
    pub(crate) hovered_button: Option<TrackedListsButton>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessKillSelection {
    Kill,
    Cancel,
}

impl ProcessKillSelection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Kill => Self::Cancel,
            Self::Cancel => Self::Kill,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessKillTarget {
    pub(crate) identity: ProcessIdentity,
    pub(crate) pid: u32,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppActivity {
    Live,
    Recording,
    LogView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleFreshness {
    Fresh,
    Stale { age_seconds: u64 },
}

pub(crate) struct App {
    pub(crate) runtime: RuntimeConfig,
    pub(crate) sampling_worker: SamplingWorker,
    pub(crate) process_info_worker: ProcessInfoWorker,
    pub(crate) open_files_worker: OpenFilesWorker,
    pub(crate) process_modules_worker: ProcessModulesWorker,
    pub(crate) process_environment_worker: ProcessEnvironmentWorker,
    pub(crate) sampling_in_progress: bool,
    pub(crate) snapshot: Snapshot,
    pub(crate) process_table_state: TableState,
    pub(crate) process_page_size: usize,
    pub(crate) selected_process_identity: Option<ProcessIdentity>,
    pub(crate) process_selection_anchor: Option<ProcessIdentity>,
    pub(crate) selected_process_identities: HashSet<ProcessIdentity>,
    pub(crate) selected_process_column_index: usize,
    pub(crate) process_metric_column_offset: usize,
    pub(crate) process_order_hold_until: Option<Instant>,
    pub(crate) show_help: bool,
    pub(crate) help_scroll: ScrollableModalState,
    pub(crate) show_column_picker: bool,
    pub(crate) tracked_lists_dialog: Option<TrackedListsDialog>,
    pub(crate) show_quit_confirmation: bool,
    pub(crate) quit_confirm_selection: QuitConfirmSelection,
    pub(crate) show_recording_no_tracked_warning: bool,
    pub(crate) show_recording_path_dialog: bool,
    pub(crate) recording_path_draft: String,
    pub(crate) recording_path_cursor: usize,
    pub(crate) recording_path_completion: PathCompletionState,
    pub(crate) recording_path_selection: RecordingPathSelection,
    pub(crate) show_recording_overwrite_confirmation: bool,
    pub(crate) recording_overwrite_selection: RecordingOverwriteSelection,
    pub(crate) show_recording_stop_confirmation: bool,
    pub(crate) recording_stop_selection: RecordingStopSelection,
    pub(crate) recording_stop_hovered: Option<RecordingStopSelection>,
    pub(crate) show_recording_tracking_fixed: bool,
    pub(crate) recording_tracking_fixed_ok_hovered: bool,
    pub(crate) recording_error: Option<RecordingErrorDialog>,
    pub(crate) recording_error_ok_hovered: bool,
    pub(crate) show_tracked_remove_confirmation: bool,
    pub(crate) tracked_remove_selection: TrackedRemoveSelection,
    pub(crate) tracked_remove_name: String,
    pub(crate) tracked_remove_total_samples: usize,
    pub(crate) tracked_remove_discarded_samples: usize,
    pub(crate) show_process_kill_confirmation: bool,
    pub(crate) process_kill_selection: ProcessKillSelection,
    pub(crate) process_kill_targets: Vec<ProcessKillTarget>,
    pub(crate) show_display_area_warning: bool,
    pub(crate) show_metric_column_warning: bool,
    pub(crate) show_no_graph_metrics_warning: bool,
    pub(crate) recording_session: Option<RecordingSession>,
    pub(crate) recording_last_dir: Option<PathBuf>,
    pub(crate) recording_spinner_index: usize,
    pub(crate) log_view_path: Option<PathBuf>,
    pub(crate) should_quit: bool,
    pub(crate) column_picker_index: usize,
    pub(crate) column_picker_scroll: ScrollableModalState,
    pub(crate) show_log_list: bool,
    pub(crate) log_list_index: usize,
    pub(crate) log_list_scroll: ScrollableModalState,
    pub(crate) log_list_focus: LogListFocus,
    pub(crate) show_log_dir_dialog: bool,
    pub(crate) log_dir_draft: String,
    pub(crate) log_dir_cursor: usize,
    pub(crate) log_dir_completion: PathCompletionState,
    pub(crate) log_dir_selection: LogDirSelection,
    pub(crate) log_dir_error: Option<String>,
    pub(crate) open_files_scroll: ScrollableModalState,
    pub(crate) open_files_result: Option<OpenFilesReport>,
    pub(crate) open_files_result_identity: Option<ProcessIdentity>,
    pub(crate) open_files_in_flight: Option<ProcessIdentity>,
    pub(crate) open_files_in_flight_generation: Option<u64>,
    pub(crate) open_files_filter: String,
    pub(crate) open_files_filter_cursor: usize,
    pub(crate) process_modules_result: Option<ProcessModulesReport>,
    pub(crate) process_modules_result_identity: Option<ProcessIdentity>,
    pub(crate) process_modules_error: Option<ProcessModulesError>,
    pub(crate) process_modules_in_flight: Option<ProcessIdentity>,
    pub(crate) process_modules_in_flight_generation: Option<u64>,
    pub(crate) process_modules_in_flight_request_id: Option<u64>,
    pub(crate) process_modules_next_request_id: u64,
    pub(crate) process_modules_filter: String,
    pub(crate) process_modules_filter_cursor: usize,
    pub(crate) process_modules_selected: usize,
    pub(crate) process_modules_show_detail: bool,
    pub(crate) process_environment_result: Option<ProcessEnvironmentReport>,
    pub(crate) process_environment_result_identity: Option<ProcessIdentity>,
    pub(crate) process_environment_error: Option<ProcessEnvironmentError>,
    pub(crate) process_environment_in_flight: Option<ProcessIdentity>,
    pub(crate) process_environment_in_flight_generation: Option<u64>,
    pub(crate) process_environment_in_flight_request_id: Option<u64>,
    pub(crate) process_environment_next_request_id: u64,
    pub(crate) process_environment_filter: String,
    pub(crate) process_environment_filter_cursor: usize,
    pub(crate) process_environment_selected: usize,
    pub(crate) process_environment_show_detail: bool,
    pub(crate) show_process_info_dialog: bool,
    pub(crate) process_info_tab: ProcessInfoTab,
    pub(crate) process_info_focus: ProcessInfoFocus,
    pub(crate) process_info_scroll: ScrollableModalState,
    pub(crate) process_info_image_scroll: ScrollableModalState,
    pub(crate) process_info_dlls_scroll: ScrollableModalState,
    pub(crate) process_info_environment_scroll: ScrollableModalState,
    pub(crate) process_info_target: Option<ProcessInfoDialogTarget>,
    pub(crate) process_info_generation: u64,
    pub(crate) show_system_info_dialog: bool,
    pub(crate) log_summaries: Vec<LogSummary>,
    pub(crate) log_list_dir: Option<PathBuf>,
    pub(crate) log_list_worker: Option<LogListWorker>,
    pub(crate) log_list_last_click: Option<LogListClick>,
    pub(crate) log_load_worker: Option<LogLoadWorker>,
    pub(crate) log_view_watch_list: Vec<String>,
    pub(crate) log_view_normalized_watch_names: HashSet<String>,
    pub(crate) focused_panel: FocusedPanel,
    pub(crate) show_details: bool,
    pub(crate) graph_entries: Vec<GraphEntry>,
    pub(crate) active_graph_id: Option<GraphId>,
    pub(crate) next_graph_id: u64,
    pub(crate) graph_scroll_row: usize,
    pub(crate) graph_scrollbar_dragging: bool,
    pub(crate) graph_scrollbar_grab_offset: usize,
    pub(crate) graph_hovered_target: Option<GraphHoverTarget>,
    pub(crate) graph_return_focus: FocusedPanel,
    pub(crate) graph_source_last_click: Option<GraphSourceClick>,
    pub(crate) details_target: DetailsTarget,
    pub(crate) details_metric: DetailsMetric,
    pub(crate) details_sample_selected: usize,
    pub(crate) details_sample_offset: usize,
    pub(crate) details_sample_page_size: usize,
    pub(crate) samples_scrollbar_dragging: bool,
    pub(crate) samples_scrollbar_grab_offset: usize,
    pub(crate) graph_pan_drag: Option<GraphPanDrag>,
    pub(crate) graph_time_span_seconds: u32,
    pub(crate) graph_time_offset_seconds: u32,
    pub(crate) graph_time_window_right_at: Option<DateTime<Local>>,
    pub(crate) graph_show_all_samples: bool,
    pub(crate) graph_y_axis_zero_min: bool,
    pub(crate) graph_slot_layout: GraphSlotLayout,
    /// User-requested Samples visibility. Width/height-based collapse is separate.
    pub(crate) show_samples_panel: bool,
    pub(crate) samples_temporarily_collapsed: bool,
    pub(crate) show_sample_delta: bool,
    pub(crate) details_live: bool,
    pub(crate) column_preset: ColumnPreset,
    pub(crate) process_columns: Vec<MetricColumn>,
    pub(crate) process_column_widths: ProcessColumnWidths,
    pub(crate) sort: SortSpec,
    pub(crate) paused_display: Option<PausedDisplay>,
    pub(crate) log_view_display: Option<PausedDisplay>,
    pub(crate) filter_text: String,
    pub(crate) filter_draft: String,
    pub(crate) filter_editing: bool,
    pub(crate) jump_draft: String,
    pub(crate) jump_editing: bool,
    pub(crate) watch_list: Vec<String>,
    pub(crate) normalized_watch_names: HashSet<String>,
    pub(crate) watch_enabled: bool,
    pub(crate) visible_process_entries: Vec<VisibleProcessEntry>,
    pub(crate) tracked_total_row: Option<ProcessRow>,
    pub(crate) exited_tracked_rows: HashMap<ProcessIdentity, ExitedTrackedRow>,
    pub(crate) last_tracked_live_identities: HashSet<ProcessIdentity>,
    pub(crate) process_history: ProcessHistory,
    pub(crate) system_history: SystemHistory,
    pub(crate) ram_vram_selected_index: usize,
    pub(crate) system_activity_selected_index: usize,
    pub(crate) process_info_cache: HashMap<ProcessIdentity, ProcessInfo>,
    pub(crate) process_info_display_identity: Option<ProcessIdentity>,
    pub(crate) pending_process_info: Option<PendingProcessInfo>,
    pub(crate) process_info_in_flight: Option<ProcessIdentity>,
    pub(crate) process_info_in_flight_generation: Option<u64>,
    pub(crate) ab_comparison: Option<AbComparison>,
    pub(crate) last_screen_area: Rect,
    pub(crate) theme_index: usize,
    pub(crate) status: String,
}

impl App {
    pub(crate) fn new(runtime: RuntimeConfig) -> Result<Self> {
        let mut sampling_runtime = SamplingRuntime::new(runtime.sampling_options);
        let mut initial = sampling_runtime.collect();
        let sort = runtime.sort;
        sort_process_rows(&mut initial.snapshot.processes, sort);
        let recording_last_dir = runtime.recording_last_dir.clone();
        let watch_list = dedupe_process_names(runtime.process_filters.clone());
        let normalized_watch_names = normalized_process_names(&watch_list);
        let watch_enabled = runtime.initial_tracked_only && !watch_list.is_empty();
        let mut process_history = ProcessHistory::default();
        process_history.record_snapshot(
            initial.snapshot.captured_at,
            &initial.snapshot.processes,
            &normalized_watch_names,
        );
        let mut system_history = SystemHistory::default();
        system_history.record_snapshot(&initial.snapshot);
        let sampling_worker = SamplingWorker::spawn(runtime.sampling_options);
        let process_info_worker = ProcessInfoWorker::spawn();
        let open_files_worker = OpenFilesWorker::spawn();
        let process_modules_worker = ProcessModulesWorker::spawn();
        let process_environment_worker = ProcessEnvironmentWorker::spawn();
        let mut process_table_state = TableState::default();
        if !initial.snapshot.processes.is_empty() {
            process_table_state.select(Some(0));
        }
        let column_preset = runtime.column_preset;
        let process_columns = if runtime.process_columns.is_empty() {
            column_preset.effective_columns().to_vec()
        } else {
            runtime.process_columns.clone()
        };
        let selected_process_column_index =
            process_column_index_for_sort(sort.column, &process_columns);
        let process_column_widths = runtime.process_column_widths.clone();
        let selected_process_identity = process_table_state
            .selected()
            .and_then(|index| initial.snapshot.processes.get(index))
            .map(ProcessIdentity::from_row);
        let last_tracked_live_identities =
            tracked_live_identities(&initial.snapshot.processes, &normalized_watch_names);
        let graph_slot_layout = runtime.initial_graph_slot_layout;
        let show_samples_panel = runtime.initial_show_samples_panel;
        let show_sample_delta = runtime.initial_show_sample_delta;
        let mut app = Self {
            theme_index: theme_index_by_name(&runtime.initial_theme),
            runtime,
            sampling_worker,
            process_info_worker,
            open_files_worker,
            process_modules_worker,
            process_environment_worker,
            sampling_in_progress: false,
            snapshot: initial.snapshot,
            process_table_state,
            process_page_size: 1,
            selected_process_identity,
            process_selection_anchor: None,
            selected_process_identities: HashSet::new(),
            selected_process_column_index,
            process_metric_column_offset: 0,
            process_order_hold_until: None,
            show_help: false,
            help_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            show_column_picker: false,
            tracked_lists_dialog: None,
            show_quit_confirmation: false,
            quit_confirm_selection: QuitConfirmSelection::Cancel,
            show_recording_no_tracked_warning: false,
            show_recording_path_dialog: false,
            recording_path_draft: String::new(),
            recording_path_cursor: 0,
            recording_path_completion: PathCompletionState::default(),
            recording_path_selection: RecordingPathSelection::Path,
            show_recording_overwrite_confirmation: false,
            recording_overwrite_selection: RecordingOverwriteSelection::Cancel,
            show_recording_stop_confirmation: false,
            recording_stop_selection: RecordingStopSelection::Continue,
            recording_stop_hovered: None,
            show_recording_tracking_fixed: false,
            recording_tracking_fixed_ok_hovered: false,
            recording_error: None,
            recording_error_ok_hovered: false,
            show_tracked_remove_confirmation: false,
            tracked_remove_selection: TrackedRemoveSelection::Cancel,
            tracked_remove_name: String::new(),
            tracked_remove_total_samples: 0,
            tracked_remove_discarded_samples: 0,
            show_process_kill_confirmation: false,
            process_kill_selection: ProcessKillSelection::Cancel,
            process_kill_targets: Vec::new(),
            show_display_area_warning: false,
            show_metric_column_warning: false,
            show_no_graph_metrics_warning: false,
            recording_session: None,
            recording_last_dir,
            recording_spinner_index: 0,
            log_view_path: None,
            should_quit: false,
            column_picker_index: 0,
            column_picker_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            show_log_list: false,
            log_list_index: 0,
            log_list_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            log_list_focus: LogListFocus::List,
            show_log_dir_dialog: false,
            log_dir_draft: String::new(),
            log_dir_cursor: 0,
            log_dir_completion: PathCompletionState::default(),
            log_dir_selection: LogDirSelection::Path,
            log_dir_error: None,
            open_files_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            open_files_result: None,
            open_files_result_identity: None,
            open_files_in_flight: None,
            open_files_in_flight_generation: None,
            open_files_filter: String::new(),
            open_files_filter_cursor: 0,
            process_modules_result: None,
            process_modules_result_identity: None,
            process_modules_error: None,
            process_modules_in_flight: None,
            process_modules_in_flight_generation: None,
            process_modules_in_flight_request_id: None,
            process_modules_next_request_id: 0,
            process_modules_filter: String::new(),
            process_modules_filter_cursor: 0,
            process_modules_selected: 0,
            process_modules_show_detail: false,
            process_environment_result: None,
            process_environment_result_identity: None,
            process_environment_error: None,
            process_environment_in_flight: None,
            process_environment_in_flight_generation: None,
            process_environment_in_flight_request_id: None,
            process_environment_next_request_id: 0,
            process_environment_filter: String::new(),
            process_environment_filter_cursor: 0,
            process_environment_selected: 0,
            process_environment_show_detail: false,
            show_process_info_dialog: false,
            process_info_tab: ProcessInfoTab::Metrics,
            process_info_focus: ProcessInfoFocus::Content,
            process_info_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            process_info_image_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            process_info_dlls_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            process_info_environment_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            process_info_target: None,
            process_info_generation: 0,
            show_system_info_dialog: false,
            log_summaries: Vec::new(),
            log_list_dir: None,
            log_list_worker: None,
            log_list_last_click: None,
            log_load_worker: None,
            log_view_watch_list: Vec::new(),
            log_view_normalized_watch_names: HashSet::new(),
            focused_panel: FocusedPanel::Processes,
            show_details: false,
            graph_entries: Vec::new(),
            active_graph_id: None,
            next_graph_id: 1,
            graph_scroll_row: 0,
            graph_scrollbar_dragging: false,
            graph_scrollbar_grab_offset: 0,
            graph_hovered_target: None,
            graph_return_focus: FocusedPanel::Processes,
            graph_source_last_click: None,
            details_target: DetailsTarget::Process,
            details_metric: DetailsMetric::Private,
            details_sample_selected: 0,
            details_sample_offset: 0,
            details_sample_page_size: 1,
            samples_scrollbar_dragging: false,
            samples_scrollbar_grab_offset: 0,
            graph_pan_drag: None,
            graph_time_span_seconds: 60,
            graph_time_offset_seconds: 0,
            graph_time_window_right_at: None,
            graph_show_all_samples: false,
            graph_y_axis_zero_min: true,
            graph_slot_layout,
            show_samples_panel,
            samples_temporarily_collapsed: false,
            show_sample_delta,
            details_live: true,
            column_preset,
            process_columns,
            process_column_widths,
            sort,
            paused_display: None,
            log_view_display: None,
            filter_text: String::new(),
            filter_draft: String::new(),
            filter_editing: false,
            jump_draft: String::new(),
            jump_editing: false,
            watch_list,
            normalized_watch_names,
            watch_enabled,
            visible_process_entries: Vec::new(),
            tracked_total_row: None,
            exited_tracked_rows: HashMap::new(),
            last_tracked_live_identities,
            process_history,
            system_history,
            ram_vram_selected_index: 0,
            system_activity_selected_index: 0,
            process_info_cache: HashMap::new(),
            process_info_display_identity: None,
            pending_process_info: None,
            process_info_in_flight: None,
            process_info_in_flight_generation: None,
            ab_comparison: None,
            last_screen_area: Rect::new(0, 0, 100, 45),
            status: initial.warning.unwrap_or_else(|| "Ready".to_string()),
        };
        app.ensure_sort_column_visible();
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();

        Ok(app)
    }

    pub(crate) fn tick_interval(&self) -> Duration {
        Duration::from_secs(super::SAMPLING_INTERVAL_SECONDS)
    }

    pub(crate) fn theme(&self) -> Theme {
        THEMES[self.theme_index]
    }

    pub(crate) fn activity(&self) -> AppActivity {
        if self.recording_session.is_some() {
            return AppActivity::Recording;
        }
        if self.log_view_path.is_some() {
            AppActivity::LogView
        } else {
            AppActivity::Live
        }
    }

    pub(crate) fn sample_freshness(&self) -> Option<SampleFreshness> {
        self.sample_freshness_at(Local::now())
    }

    pub(crate) fn sample_freshness_at(&self, now: DateTime<Local>) -> Option<SampleFreshness> {
        if self.activity() == AppActivity::LogView {
            return None;
        }
        let age_seconds = now
            .signed_duration_since(self.snapshot.captured_at)
            .num_seconds()
            .max(0) as u64;
        if age_seconds >= SAMPLE_STALE_AFTER_SECONDS {
            Some(SampleFreshness::Stale { age_seconds })
        } else {
            Some(SampleFreshness::Fresh)
        }
    }

    pub(crate) fn active_log_path(&self) -> Option<&PathBuf> {
        self.recording_session
            .as_ref()
            .map(|session| &session.path)
            .or(self.log_view_path.as_ref())
    }

    pub(crate) fn set_process_page_size(&mut self, page_size: usize) {
        self.process_page_size = page_size;
    }

    pub(crate) fn set_details_sample_page_size(&mut self, page_size: usize) {
        self.details_sample_page_size = page_size.max(1);
        self.clamp_details_sample_offset();
    }

    pub(crate) fn set_log_list_page_size(&mut self, page_size: usize) {
        self.log_list_scroll
            .set_page_size(page_size, self.log_list_total_rows());
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn set_screen_area(&mut self, area: Rect) {
        self.last_screen_area = area;
        self.ensure_selected_process_column_visible();
    }

    pub(crate) fn is_filter_editing(&self) -> bool {
        self.filter_editing
    }

    pub(crate) fn is_process_jump_editing(&self) -> bool {
        self.jump_editing
    }

    pub(crate) fn process_jump_draft(&self) -> &str {
        &self.jump_draft
    }

    pub(crate) fn is_column_picker_open(&self) -> bool {
        self.show_column_picker
    }

    pub(crate) fn is_log_list_open(&self) -> bool {
        self.show_log_list
    }

    pub(crate) fn has_modal_focus(&self) -> bool {
        self.show_help
            || self.show_column_picker
            || self.show_log_list
            || self.show_log_dir_dialog
            || self.show_process_info_dialog
            || self.show_system_info_dialog
            || self.show_quit_confirmation
            || self.show_recording_no_tracked_warning
            || self.show_recording_path_dialog
            || self.show_recording_overwrite_confirmation
            || self.show_recording_stop_confirmation
            || self.show_recording_tracking_fixed
            || self.recording_error.is_some()
            || self.show_tracked_remove_confirmation
            || self.show_process_kill_confirmation
            || self.show_display_area_warning
            || self.show_metric_column_warning
            || self.show_no_graph_metrics_warning
            || self.tracked_lists_dialog.is_some()
    }

    pub(crate) fn panel_has_focus(&self, panel: FocusedPanel) -> bool {
        !self.has_modal_focus() && self.focused_panel == panel
    }

    pub(crate) fn ensure_visible_panel_focus(&mut self) {
        if !self.show_details
            && matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            )
        {
            self.focused_panel = FocusedPanel::Processes;
            return;
        }
        if self.focused_panel == FocusedPanel::DetailsSamples
            && !self.effective_show_samples_panel()
        {
            self.focused_panel = FocusedPanel::DetailsGraph;
            return;
        }
        if self.show_details
            && matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            )
            && self.graph_entries.is_empty()
        {
            self.focused_panel = FocusedPanel::Processes;
        }
    }

    pub(crate) fn active_filter_text(&self) -> &str {
        if self.filter_editing {
            &self.filter_draft
        } else {
            &self.filter_text
        }
    }

    #[cfg(test)]
    pub(crate) fn visible_processes(&self) -> Vec<&ProcessRow> {
        self.visible_process_entries
            .iter()
            .filter_map(|entry| self.process_for_visible_entry(entry))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn visible_process_window(
        &self,
        offset: usize,
        rows: usize,
    ) -> Vec<(usize, &ProcessRow)> {
        self.visible_process_entries
            .iter()
            .enumerate()
            .skip(offset)
            .take(rows)
            .filter_map(|(visible_index, entry)| {
                self.process_for_visible_entry(entry)
                    .map(|process| (visible_index, process))
            })
            .collect()
    }

    pub(crate) fn visible_process_row_window(
        &self,
        offset: usize,
        rows: usize,
    ) -> Vec<VisibleProcessRow<'_>> {
        let has_multi_selection = !self.selected_process_identities.is_empty();
        self.visible_process_entries
            .iter()
            .skip(offset)
            .take(rows)
            .filter_map(|entry| {
                let process = self.process_for_visible_entry(entry)?;
                Some(VisibleProcessRow {
                    process,
                    tracked: self.is_tracked_process_name(&process.name),
                    lifecycle: self.lifecycle_for_visible_entry(entry),
                    multi_selected: has_multi_selection
                        && self
                            .identity_for_visible_entry(entry)
                            .is_some_and(|identity| {
                                self.selected_process_identities.contains(&identity)
                            }),
                    is_tracked_total: false,
                })
            })
            .collect()
    }

    pub(crate) fn tracked_total_visible_row(&self) -> Option<VisibleProcessRow<'_>> {
        self.has_visible_tracked_total_row()
            .then(|| VisibleProcessRow {
                process: self.tracked_total_row.as_ref().expect("tracked total row"),
                tracked: false,
                lifecycle: ProcessLifecycle::Live,
                multi_selected: false,
                is_tracked_total: true,
            })
    }

    pub(crate) fn has_visible_tracked_total_row(&self) -> bool {
        self.watch_enabled && self.tracked_total_row.is_some()
    }

    pub(crate) fn visible_process_count(&self) -> usize {
        self.visible_process_entries.len()
    }

    pub(crate) fn sort_indicator_for_column(&self, column: SortColumn) -> Option<SortDirection> {
        (self.sort.column == column).then_some(self.sort.direction)
    }

    pub(crate) fn is_display_paused(&self) -> bool {
        self.paused_display.is_some()
    }

    pub(crate) fn hold_process_order_during_navigation(&mut self) {
        self.process_order_hold_until = Some(Instant::now() + PROCESS_NAVIGATION_ORDER_HOLD);
    }

    fn process_order_hold_active(&self) -> bool {
        self.process_order_hold_until
            .is_some_and(|until| Instant::now() < until)
    }

    fn clear_process_order_hold(&mut self) {
        self.process_order_hold_until = None;
    }

    pub(crate) fn display_snapshot(&self) -> &Snapshot {
        self.log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
            .map(|display| &display.snapshot)
            .unwrap_or(&self.snapshot)
    }

    pub(crate) fn display_process_history(&self) -> &ProcessHistory {
        self.log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
            .map(|display| &display.process_history)
            .unwrap_or(&self.process_history)
    }

    pub(crate) fn display_system_history(&self) -> &SystemHistory {
        self.log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
            .map(|display| &display.system_history)
            .unwrap_or(&self.system_history)
    }

    fn display_exited_tracked_rows(&self) -> &HashMap<ProcessIdentity, ExitedTrackedRow> {
        self.log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
            .map(|display| &display.exited_tracked_rows)
            .unwrap_or(&self.exited_tracked_rows)
    }

    fn display_process_info_cache(&self) -> &HashMap<ProcessIdentity, ProcessInfo> {
        self.log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
            .map(|display| &display.process_info_cache)
            .unwrap_or(&self.process_info_cache)
    }

    fn display_process_info_identity(&self) -> Option<&ProcessIdentity> {
        match self
            .log_view_display
            .as_ref()
            .or(self.paused_display.as_ref())
        {
            Some(display) => display.process_info_display_identity.as_ref(),
            None => self.process_info_display_identity.as_ref(),
        }
    }

    pub(crate) fn visible_tracked_process_count(&self) -> usize {
        self.visible_process_entries
            .iter()
            .filter_map(|entry| self.process_for_visible_entry(entry))
            .filter(|process| self.is_tracked_process_name(&process.name))
            .count()
    }

    pub(crate) fn visible_process_at(&self, index: usize) -> Option<&ProcessRow> {
        self.visible_process_entries
            .get(index)
            .and_then(|entry| self.process_for_visible_entry(entry))
    }

    pub(crate) fn visible_process_identity_at(&self, index: usize) -> Option<ProcessIdentity> {
        self.visible_process_entries
            .get(index)
            .and_then(|entry| self.identity_for_visible_entry(entry))
    }

    pub(crate) fn visible_process_position(&self, identity: &ProcessIdentity) -> Option<usize> {
        self.visible_process_entries
            .iter()
            .enumerate()
            .find_map(|(visible_index, entry)| {
                (self.identity_for_visible_entry(entry).as_ref() == Some(identity))
                    .then_some(visible_index)
            })
    }

    pub(crate) fn first_selectable_process_index(&self) -> Option<usize> {
        self.visible_process_entries
            .iter()
            .position(|entry| self.identity_for_visible_entry(entry).is_some())
    }

    pub(crate) fn rebuild_visible_process_cache(&mut self) {
        let filter = self.active_filter_text().trim().to_ascii_lowercase();
        let filter_includes_path = self.process_columns.contains(&MetricColumn::FullPath);
        let normalized_watch_names = self.active_normalized_watch_names().clone();

        self.tracked_total_row =
            tracked_total_row(&self.display_snapshot().processes, &normalized_watch_names);
        self.visible_process_entries = {
            let snapshot = self.display_snapshot();
            snapshot
                .processes
                .iter()
                .enumerate()
                .filter(|(_, process)| {
                    let name = process.name.to_ascii_lowercase();
                    let filter_matches = filter.is_empty()
                        || process_matches_filter(process, &filter, filter_includes_path);
                    let watch_matches =
                        !self.watch_enabled || normalized_watch_names.contains(&name);
                    filter_matches && watch_matches
                })
                .map(|(index, _)| VisibleProcessEntry::Live(index))
                .collect::<Vec<_>>()
        };
        self.visible_process_entries
            .extend(self.visible_ghost_entries(&filter, filter_includes_path));
        self.prune_process_selection_to_visible_live_rows();
        if let Some(selected) = self.process_table_state.selected() {
            if selected < self.visible_process_entries.len()
                && self.visible_process_identity_at(selected).is_none()
                && let Some(index) = self.first_selectable_process_index()
            {
                self.process_table_state.select(Some(index));
                self.selected_process_identity = self.visible_process_identity_at(index);
            }
        }
    }

    fn rebuild_normalized_watch_names(&mut self) {
        self.normalized_watch_names = normalized_process_names(&self.watch_list);
    }

    pub(crate) fn is_tracked_process_name(&self, name: &str) -> bool {
        self.active_normalized_watch_names()
            .contains(&name.trim().to_ascii_lowercase())
    }

    fn active_normalized_watch_names(&self) -> &HashSet<String> {
        if self.log_view_path.is_some() {
            &self.log_view_normalized_watch_names
        } else {
            &self.normalized_watch_names
        }
    }

    fn process_for_visible_entry(&self, entry: &VisibleProcessEntry) -> Option<&ProcessRow> {
        match entry {
            VisibleProcessEntry::Live(index) => self.display_snapshot().processes.get(*index),
            VisibleProcessEntry::Ghost(identity) => self
                .display_exited_tracked_rows()
                .get(identity)
                .map(|row| &row.process),
        }
    }

    fn identity_for_visible_entry(&self, entry: &VisibleProcessEntry) -> Option<ProcessIdentity> {
        match entry {
            VisibleProcessEntry::Live(index) => self
                .display_snapshot()
                .processes
                .get(*index)
                .map(ProcessIdentity::from_row),
            VisibleProcessEntry::Ghost(identity) => Some(identity.clone()),
        }
    }

    pub(crate) fn live_identity_for_visible_entry(
        &self,
        entry: &VisibleProcessEntry,
    ) -> Option<ProcessIdentity> {
        match entry {
            VisibleProcessEntry::Live(index) => self
                .display_snapshot()
                .processes
                .get(*index)
                .map(ProcessIdentity::from_row),
            VisibleProcessEntry::Ghost(_) => None,
        }
    }

    fn lifecycle_for_visible_entry(&self, entry: &VisibleProcessEntry) -> ProcessLifecycle {
        match entry {
            VisibleProcessEntry::Live(_) => ProcessLifecycle::Live,
            VisibleProcessEntry::Ghost(identity) => self
                .display_exited_tracked_rows()
                .get(identity)
                .map(|row| ProcessLifecycle::Exited {
                    exited_at: row.exited_at,
                })
                .unwrap_or(ProcessLifecycle::Live),
        }
    }

    pub(crate) fn selected_visible_process_lifecycle(&self) -> Option<ProcessLifecycle> {
        let selected = self.process_table_state.selected()?;
        self.visible_process_entries
            .get(selected)
            .map(|entry| self.lifecycle_for_visible_entry(entry))
    }

    pub(crate) fn selected_live_process_identity(&self) -> Option<ProcessIdentity> {
        let selected = self.process_table_state.selected()?;
        self.visible_process_entries
            .get(selected)
            .and_then(|entry| self.live_identity_for_visible_entry(entry))
    }

    pub(crate) fn prune_process_selection_to_visible_live_rows(&mut self) {
        if self.selected_process_identities.is_empty() && self.process_selection_anchor.is_none() {
            return;
        }
        let visible_live = self
            .visible_process_entries
            .iter()
            .filter_map(|entry| self.live_identity_for_visible_entry(entry))
            .collect::<HashSet<_>>();
        self.selected_process_identities
            .retain(|identity| visible_live.contains(identity));
        if self
            .process_selection_anchor
            .as_ref()
            .is_some_and(|identity| !visible_live.contains(identity))
        {
            self.process_selection_anchor = None;
        }
    }

    pub(crate) fn clear_process_multi_selection(&mut self) {
        self.selected_process_identities.clear();
    }

    #[cfg(test)]
    pub(crate) fn selected_process_identities_count(&self) -> usize {
        self.selected_process_identities.len()
    }

    pub(crate) fn toggle_focused_process_multi_selection(&mut self) {
        let Some(identity) = self.selected_live_process_identity() else {
            self.status = "Only live process rows can be multi-selected".to_string();
            return;
        };
        if !self.selected_process_identities.insert(identity.clone()) {
            self.selected_process_identities.remove(&identity);
        }
        self.process_selection_anchor = Some(identity);
        let count = self.selected_process_identities.len();
        self.status = if count == 0 {
            "Process multi-selection cleared".to_string()
        } else {
            format!("Selected {count} live process rows")
        };
    }

    fn visible_ghost_entries(
        &self,
        filter: &str,
        filter_includes_path: bool,
    ) -> Vec<VisibleProcessEntry> {
        let mut latest_by_name: HashMap<String, (&ProcessIdentity, DateTime<Local>)> =
            HashMap::new();
        for (identity, row) in self.display_exited_tracked_rows() {
            let name = identity.name.to_ascii_lowercase();
            if !self.active_normalized_watch_names().contains(&name) {
                continue;
            }
            if !filter.is_empty()
                && !process_matches_filter(&row.process, filter, filter_includes_path)
            {
                continue;
            }
            match latest_by_name.get(&name) {
                Some((_, exited_at)) if *exited_at >= row.exited_at => {}
                _ => {
                    latest_by_name.insert(name, (identity, row.exited_at));
                }
            }
        }

        let mut ghosts = latest_by_name
            .into_values()
            .map(|(identity, exited_at)| (identity.clone(), exited_at))
            .collect::<Vec<_>>();
        ghosts.sort_by(|left, right| {
            left.0
                .name
                .to_ascii_lowercase()
                .cmp(&right.0.name.to_ascii_lowercase())
                .then_with(|| left.0.pid.cmp(&right.0.pid))
                .then_with(|| left.0.start_time.cmp(&right.0.start_time))
                .then_with(|| right.1.cmp(&left.1))
        });
        ghosts
            .into_iter()
            .map(|(identity, _)| VisibleProcessEntry::Ghost(identity))
            .collect()
    }

    pub(crate) fn begin_filter_edit(&mut self) {
        self.filter_draft = self.filter_text.clone();
        self.filter_editing = true;
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = "Filter editing".to_string();
    }

    pub(crate) fn push_filter_char(&mut self, ch: char) {
        self.filter_draft.push(ch);
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
    }

    pub(crate) fn pop_filter_char(&mut self) {
        self.filter_draft.pop();
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
    }

    pub(crate) fn commit_filter_edit(&mut self) {
        self.filter_text = self.filter_draft.trim().to_string();
        self.filter_draft.clear();
        self.filter_editing = false;
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = if self.filter_text.is_empty() {
            "Filter cleared".to_string()
        } else {
            format!("Filter applied: {}", self.filter_text)
        };
    }

    pub(crate) fn clear_filter(&mut self) {
        self.filter_text.clear();
        self.filter_draft.clear();
        self.filter_editing = false;
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = "Filter cleared".to_string();
    }

    pub(crate) fn begin_process_jump_edit(&mut self) {
        self.jump_draft.clear();
        self.jump_editing = true;
        self.focused_panel = FocusedPanel::Processes;
        self.status = "Jump: type process name".to_string();
    }

    pub(crate) fn close_process_jump_edit(&mut self) {
        self.jump_draft.clear();
        self.jump_editing = false;
        self.status = "Ready".to_string();
    }

    pub(crate) fn push_process_jump_char(&mut self, ch: char) {
        self.jump_draft.push(ch);
        self.jump_to_process_match(false);
    }

    pub(crate) fn pop_process_jump_char(&mut self) {
        self.jump_draft.pop();
        self.jump_to_process_match(false);
    }

    pub(crate) fn jump_to_next_process_match(&mut self) {
        self.jump_to_process_match(true);
    }

    fn jump_to_process_match(&mut self, next_only: bool) {
        let query = self.jump_draft.trim().to_ascii_lowercase();
        if query.is_empty() {
            self.status = "Jump: type process name".to_string();
            return;
        }
        let visible_count = self.visible_process_count();
        if visible_count == 0 {
            self.status = format!("No matching process: {}", self.jump_draft);
            return;
        }
        let current = self.process_table_state.selected().unwrap_or(0);
        let start = current.saturating_add(usize::from(next_only));
        let match_index = (0..visible_count).find_map(|offset| {
            let index = (start + offset) % visible_count;
            let identity = self.visible_process_identity_at(index)?;
            identity
                .name
                .to_ascii_lowercase()
                .contains(&query)
                .then_some(index)
        });
        let Some(index) = match_index else {
            self.status = format!("No matching process: {}", self.jump_draft);
            return;
        };
        self.select_process_index(index);
        self.ensure_selected_row_visible();
        self.status = format!("Jumped to {}", self.visible_process_at(index).unwrap().name);
    }

    pub(crate) fn toggle_details(&mut self) {
        if !self.show_details && self.active_graph_slot_count() == 0 {
            self.show_no_graph_metrics_warning = true;
            self.status = "No metric is selected for graphing.".to_string();
            return;
        }
        self.show_details = !self.show_details;
        if !self.show_details
            && matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            )
        {
            self.focused_panel = FocusedPanel::Processes;
        }
        self.status = if self.show_details {
            "Graphs shown".to_string()
        } else {
            "Graphs hidden".to_string()
        };
    }

    pub(crate) fn dismiss_display_area_warning(&mut self) {
        self.show_display_area_warning = false;
        self.ensure_visible_panel_focus();
        self.status = "Ready".to_string();
    }

    pub(crate) fn dismiss_metric_column_warning(&mut self) {
        self.show_metric_column_warning = false;
        self.ensure_visible_panel_focus();
        self.status = "Ready".to_string();
    }

    pub(crate) fn dismiss_no_graph_metrics_warning(&mut self) {
        self.show_no_graph_metrics_warning = false;
        self.ensure_visible_panel_focus();
        self.status = "Ready".to_string();
    }

    fn clear_graph_workspace_state(&mut self) {
        self.graph_entries.clear();
        self.active_graph_id = None;
        self.graph_scroll_row = 0;
        self.graph_scrollbar_dragging = false;
        self.graph_scrollbar_grab_offset = 0;
        self.graph_hovered_target = None;
        self.show_details = false;
        self.ab_comparison = None;
        if matches!(
            self.focused_panel,
            FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
        ) {
            self.focused_panel = FocusedPanel::Processes;
        }
    }

    pub(crate) fn remove_active_graph(&mut self) -> bool {
        let Some(id) = self.active_graph_id else {
            return false;
        };
        self.remove_graph(id)
    }

    pub(crate) fn remove_graph(&mut self, id: GraphId) -> bool {
        let Some(index) = self.graph_entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        let selected_time = self.selected_details_sample_time();
        let description = self.graph_entries[index].source.description();
        let removed_active = self.active_graph_id == Some(id);
        self.graph_entries.remove(index);

        if self.graph_entries.is_empty() {
            let return_focus = self.graph_return_focus;
            self.clear_graph_workspace_state();
            self.focused_panel = match return_focus {
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples => {
                    FocusedPanel::Processes
                }
                panel => panel,
            };
        } else if removed_active {
            let next_index = index.min(self.graph_entries.len() - 1);
            self.active_graph_id = Some(self.graph_entries[next_index].id);
            if let Some(time) = selected_time {
                self.align_details_sample_selection_to_time(time);
            } else {
                self.select_details_sample_latest();
            }
        }
        if !self.graph_entries.is_empty() {
            self.sync_graph_layout_visibility();
            if removed_active {
                self.reveal_active_graph();
            }
        }
        self.status = format!("Graph removed: {description}");
        true
    }

    pub(crate) fn toggle_samples_panel(&mut self) {
        if self.show_samples_panel {
            self.show_samples_panel = false;
            self.samples_temporarily_collapsed = false;
            if self.focused_panel == FocusedPanel::DetailsSamples {
                self.focused_panel = FocusedPanel::DetailsGraph;
            }
            self.status = "Samples panel hidden".to_string();
            return;
        }

        self.show_samples_panel = true;
        self.status = "Samples panel shown".to_string();
        self.sync_graph_layout_visibility();
        self.reveal_active_graph();
    }

    pub(crate) fn toggle_sample_delta(&mut self) {
        if !self.effective_show_samples_panel() {
            self.status = "Delta is unavailable while Samples are hidden".to_string();
            return;
        }
        self.show_sample_delta = !self.show_sample_delta;
        self.status = if self.show_sample_delta {
            "Delta shown".to_string()
        } else {
            "Delta hidden".to_string()
        };
    }

    pub(crate) fn toggle_graph_slot_layout(&mut self) {
        self.graph_slot_layout = self.graph_slot_layout.next();
        self.status = format!("Graph layout: {}", self.graph_slot_layout.label());
        self.sync_graph_layout_visibility();
        self.reveal_active_graph();
    }

    pub(crate) fn active_graph_slot_count(&self) -> usize {
        self.graph_entries.len()
    }

    pub(crate) fn effective_show_samples_panel(&self) -> bool {
        self.show_samples_panel && !self.samples_temporarily_collapsed
    }

    pub(crate) fn graph_slot(&self, index: usize) -> Option<&GraphSlot> {
        self.graph_entries.get(index).map(|entry| &entry.source)
    }

    pub(crate) fn graph_entry(&self, index: usize) -> Option<&GraphEntry> {
        self.graph_entries.get(index)
    }

    pub(crate) fn graph_entry_by_id(&self, id: GraphId) -> Option<&GraphEntry> {
        self.graph_entries.iter().find(|entry| entry.id == id)
    }

    pub(crate) fn graph_index_by_id(&self, id: GraphId) -> Option<usize> {
        self.graph_entries.iter().position(|entry| entry.id == id)
    }

    pub(crate) fn active_graph_index(&self) -> Option<usize> {
        self.active_graph_id
            .and_then(|id| self.graph_index_by_id(id))
    }

    pub(crate) fn graph_slot_samples(&self, slot: &GraphSlot) -> Vec<GraphSample> {
        match slot {
            GraphSlot::Process { identity, metric } => self
                .display_process_history()
                .samples_for(identity)
                .into_iter()
                .map(|sample| GraphSample {
                    captured_at: sample.captured_at,
                    value: process_sample_metric_value(sample, *metric),
                })
                .collect(),
            GraphSlot::System { metric } => self
                .display_system_history()
                .samples()
                .iter()
                .map(|sample| GraphSample {
                    captured_at: sample.captured_at,
                    value: sample.value(*metric),
                })
                .collect(),
        }
    }

    pub(crate) fn graph_slot_peak(&self, slot: &GraphSlot) -> Option<f64> {
        let GraphSlot::Process { identity, metric } = slot else {
            return None;
        };
        self.display_process_history()
            .peak_for(identity)
            .and_then(|peak| process_peak_metric_value(peak, *metric).map(|value| value as f64))
    }

    pub(crate) fn active_graph_slot(&self) -> Option<&GraphSlot> {
        self.active_graph_id
            .and_then(|id| self.graph_entry_by_id(id))
            .map(|entry| &entry.source)
            .or_else(|| self.graph_entries.first().map(|entry| &entry.source))
    }

    pub(crate) fn selected_details_sample_time(&self) -> Option<DateTime<Local>> {
        let slot = self.active_graph_slot()?;
        self.graph_slot_samples(slot)
            .get(self.details_sample_selected)
            .map(|sample| sample.captured_at)
    }

    pub(crate) fn details_sample_view_state_for_slot(
        &self,
        slot_index: usize,
        rows: usize,
    ) -> Option<DetailsSampleViewState> {
        let slot = self.graph_slot(slot_index)?;
        let samples = self.graph_slot_samples(slot);
        if samples.is_empty() {
            return None;
        }
        let selected = self.details_sample_selected.min(samples.len() - 1);
        if Some(slot_index) == self.active_graph_index() {
            return Some(DetailsSampleViewState {
                selected_index: selected,
                selected_exact: true,
                offset: self.details_sample_offset,
            });
        }

        let selected_time = self.selected_details_sample_time();
        let selected_index = selected_time
            .and_then(|time| sample_index_nearest_time(&samples, time))
            .unwrap_or(selected);
        let selected_exact =
            selected_time.is_some_and(|time| sample_index_at_time(&samples, time).is_some());
        Some(DetailsSampleViewState {
            selected_index,
            selected_exact,
            offset: synced_sample_viewport_offset(
                samples.len(),
                rows,
                selected_index,
                self.details_sample_selected,
                self.details_sample_offset,
            ),
        })
    }

    pub(crate) fn sync_graph_layout_visibility(&mut self) {
        if self.graph_entries.is_empty() {
            self.active_graph_id = None;
            self.graph_scroll_row = 0;
            self.samples_temporarily_collapsed = self.show_samples_panel;
        } else if self
            .active_graph_id
            .is_none_or(|id| self.graph_entry_by_id(id).is_none())
        {
            self.active_graph_id = self.graph_entries.first().map(|entry| entry.id);
        }
        if let Some(details) =
            crate::ui::main_panel_areas_for_app(self.last_screen_area, self).details
        {
            let layout = crate::ui::layout::graph_workspace_layout(details, self);
            self.samples_temporarily_collapsed =
                self.show_samples_panel && layout.samples.is_none();
            self.graph_scroll_row = self.graph_scroll_row.min(layout.max_scroll_row);
            if layout.max_scroll_row == 0 {
                self.graph_scrollbar_dragging = false;
                self.graph_scrollbar_grab_offset = 0;
            }
        } else {
            self.samples_temporarily_collapsed = self.show_samples_panel;
            self.graph_scroll_row = 0;
        }
        if !self.effective_show_samples_panel()
            && self.focused_panel == FocusedPanel::DetailsSamples
        {
            self.focused_panel = FocusedPanel::DetailsGraph;
        }
    }

    pub(crate) fn reveal_active_graph(&mut self) {
        let Some(active_index) = self.active_graph_index() else {
            return;
        };
        let Some(details) =
            crate::ui::main_panel_areas_for_app(self.last_screen_area, self).details
        else {
            return;
        };
        let layout = crate::ui::layout::graph_workspace_layout(details, self);
        let active_row = active_index / layout.columns.max(1);
        if active_row < self.graph_scroll_row {
            self.graph_scroll_row = active_row;
        } else if active_row
            >= self
                .graph_scroll_row
                .saturating_add(layout.visible_rows.max(1))
        {
            self.graph_scroll_row = active_row
                .saturating_add(1)
                .saturating_sub(layout.visible_rows.max(1))
                .min(layout.max_scroll_row);
        }
    }

    pub(crate) fn set_graph_scroll_row(&mut self, row: usize) {
        let Some(details) =
            crate::ui::main_panel_areas_for_app(self.last_screen_area, self).details
        else {
            self.graph_scroll_row = 0;
            return;
        };
        let layout = crate::ui::layout::graph_workspace_layout(details, self);
        self.graph_scroll_row = row.min(layout.max_scroll_row);
    }

    pub(crate) fn scroll_graph_rows_up(&mut self, rows: usize) {
        self.set_graph_scroll_row(self.graph_scroll_row.saturating_sub(rows));
    }

    pub(crate) fn scroll_graph_rows_down(&mut self, rows: usize) {
        self.set_graph_scroll_row(self.graph_scroll_row.saturating_add(rows));
    }

    pub(crate) fn selected_process_graph_source(&self) -> Option<GraphSlot> {
        let identity = self.selected_visible_process_identity()?;
        let column = self.selected_process_metric_column()?;
        let metric = DetailsMetric::from_graphable_column(column)?;
        Some(GraphSlot::process(identity, metric))
    }

    pub(crate) fn toggle_selected_process_graph(&mut self) {
        if self.focused_panel != FocusedPanel::Processes {
            self.status = "Graph registration requires Processes focus".to_string();
            return;
        }
        if self.selected_visible_process_identity().is_none() {
            self.status = "No process selected".to_string();
            return;
        }
        let Some(source) = self.selected_process_graph_source() else {
            self.status = "Select a graphable metric cell".to_string();
            return;
        };
        self.toggle_graph_source(source, FocusedPanel::Processes);
    }

    pub(crate) fn graph_id_for_source(&self, source: &GraphSlot) -> Option<GraphId> {
        self.graph_entries
            .iter()
            .find(|entry| &entry.source == source)
            .map(|entry| entry.id)
    }

    pub(crate) fn graph_source_state(&self, source: &GraphSlot) -> Option<GraphSourceState> {
        self.graph_entries
            .iter()
            .enumerate()
            .find(|(_, entry)| &entry.source == source)
            .map(|(ordinal, entry)| GraphSourceState {
                ordinal,
                active: self.active_graph_id == Some(entry.id),
            })
    }

    pub(crate) fn set_active_graph(&mut self, id: GraphId) -> bool {
        if self.graph_entry_by_id(id).is_none() {
            return false;
        }
        if self.active_graph_id == Some(id) {
            self.show_details = true;
            self.sync_graph_layout_visibility();
            self.reveal_active_graph();
            return true;
        }
        let selected_time = self.selected_details_sample_time();
        self.active_graph_id = Some(id);
        if let Some(time) = selected_time {
            self.align_details_sample_selection_to_time(time);
        } else {
            self.select_details_sample_latest();
        }
        self.show_details = true;
        self.sync_graph_layout_visibility();
        self.reveal_active_graph();
        true
    }

    pub(crate) fn select_graph(&mut self, id: GraphId) -> bool {
        if !self.set_active_graph(id) {
            return false;
        }
        if let Some(entry) = self.graph_entry_by_id(id) {
            self.status = format!("Graph selected: {}", entry.source.description());
        }
        true
    }

    pub(crate) fn select_graph_index(&mut self, index: usize) -> bool {
        self.graph_entries
            .get(index)
            .map(|entry| entry.id)
            .is_some_and(|id| self.select_graph(id))
    }

    pub(crate) fn select_previous_graph(&mut self) {
        let Some(active) = self.active_graph_index() else {
            return;
        };
        let next = active.saturating_sub(1);
        self.select_graph_index(next);
    }

    pub(crate) fn select_next_graph(&mut self) {
        let Some(active) = self.active_graph_index() else {
            return;
        };
        let next = active
            .saturating_add(1)
            .min(self.graph_entries.len().saturating_sub(1));
        self.select_graph_index(next);
    }

    fn add_graph_source(&mut self, source: GraphSlot, return_focus: FocusedPanel) -> bool {
        if self.graph_entries.len() >= GRAPH_LIMIT {
            self.status = format!("Graph limit reached ({GRAPH_LIMIT})");
            return false;
        }
        let selected_time = self.selected_details_sample_time();
        let id = GraphId(self.next_graph_id);
        self.next_graph_id = self
            .next_graph_id
            .checked_add(1)
            .expect("GraphId space exhausted");
        let description = source.description();
        self.graph_entries.push(GraphEntry { id, source });
        self.active_graph_id = Some(id);
        self.graph_return_focus = return_focus;
        self.show_details = true;
        if let Some(time) = selected_time {
            self.align_details_sample_selection_to_time(time);
        } else {
            self.select_details_sample_latest();
        }
        self.sync_graph_layout_visibility();
        self.reveal_active_graph();
        self.status = format!("Graph added: {description}");
        true
    }

    pub(crate) fn toggle_graph_source(
        &mut self,
        source: GraphSlot,
        return_focus: FocusedPanel,
    ) -> bool {
        if let Some(id) = self.graph_id_for_source(&source) {
            return self.remove_graph(id);
        }
        self.add_graph_source(source, return_focus)
    }

    #[cfg(test)]
    pub(crate) fn add_or_reveal_graph_source(
        &mut self,
        source: GraphSlot,
        return_focus: FocusedPanel,
    ) -> bool {
        if let Some(id) = self.graph_id_for_source(&source) {
            return self.select_graph(id);
        }
        self.add_graph_source(source, return_focus)
    }

    pub(crate) fn register_graph_source_click(
        &mut self,
        source: GraphSlot,
        clicked_at: Instant,
        return_focus: FocusedPanel,
    ) {
        let is_double_click = self.graph_source_last_click.as_ref().is_some_and(|last| {
            last.source == source
                && clicked_at.saturating_duration_since(last.clicked_at)
                    <= GRAPH_SOURCE_DOUBLE_CLICK_WINDOW
        });
        if is_double_click {
            self.graph_source_last_click = None;
            self.toggle_graph_source(source, return_focus);
        } else {
            self.graph_source_last_click = Some(GraphSourceClick { source, clicked_at });
        }
    }

    pub(crate) fn clear_graph_source_click(&mut self) {
        self.graph_source_last_click = None;
    }

    #[cfg(test)]
    pub(crate) fn toggle_details_metric(&mut self) {
        self.details_target = DetailsTarget::Process;
        self.details_metric = self.details_metric.toggled();
        self.clear_ab_comparison();
        if let Some(index) = self
            .process_columns
            .iter()
            .position(|column| *column == self.details_metric.column())
        {
            self.selected_process_column_index = index;
        }
        self.show_details = true;
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.status = format!("Details graph: {}", self.details_metric.label());
    }

    pub(crate) fn cycle_focus(&mut self) {
        self.focused_panel = self.next_focus_target();
        self.status = self.focus_status();
    }

    pub(crate) fn cycle_focus_previous(&mut self) {
        self.focused_panel = self.previous_focus_target();
        self.status = self.focus_status();
    }

    fn next_focus_target(&self) -> FocusedPanel {
        if !self.show_details || self.graph_entries.is_empty() {
            return self.focused_panel.next(false);
        }

        match self.focused_panel {
            FocusedPanel::System => FocusedPanel::SystemActivity,
            FocusedPanel::SystemActivity => FocusedPanel::Cpu,
            FocusedPanel::Cpu => FocusedPanel::Processes,
            FocusedPanel::Processes => FocusedPanel::DetailsGraph,
            FocusedPanel::DetailsGraph if self.effective_show_samples_panel() => {
                FocusedPanel::DetailsSamples
            }
            FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples => FocusedPanel::System,
        }
    }

    fn previous_focus_target(&self) -> FocusedPanel {
        if !self.show_details || self.graph_entries.is_empty() {
            return self.focused_panel.previous(false);
        }

        match self.focused_panel {
            FocusedPanel::System if self.effective_show_samples_panel() => {
                FocusedPanel::DetailsSamples
            }
            FocusedPanel::System => FocusedPanel::DetailsGraph,
            FocusedPanel::SystemActivity => FocusedPanel::System,
            FocusedPanel::Cpu => FocusedPanel::SystemActivity,
            FocusedPanel::Processes => FocusedPanel::Cpu,
            FocusedPanel::DetailsGraph => FocusedPanel::Processes,
            FocusedPanel::DetailsSamples => FocusedPanel::DetailsGraph,
        }
    }

    fn focus_status(&self) -> String {
        match self.focused_panel {
            FocusedPanel::DetailsGraph => {
                format!(
                    "Focus: Graph {}/{}",
                    self.active_graph_index().map_or(0, |index| index + 1),
                    self.graph_entries.len()
                )
            }
            FocusedPanel::DetailsSamples => {
                format!(
                    "Focus: Samples · Graph {}/{}",
                    self.active_graph_index().map_or(0, |index| index + 1),
                    self.graph_entries.len()
                )
            }
            panel => format!("Focus: {}", panel.label()),
        }
    }

    pub(crate) fn selected_system_metric(&self) -> SystemMetric {
        SystemMetric::RAM_VRAM_PANEL
            .get(self.ram_vram_selected_index)
            .copied()
            .unwrap_or(SystemMetric::PhysicalMemory)
    }

    pub(crate) fn selected_system_activity_metric(&self) -> SystemMetric {
        SystemMetric::SYSTEM_ACTIVITY_PANEL
            .get(self.system_activity_selected_index)
            .copied()
            .unwrap_or(SystemMetric::NetworkReceived)
    }

    pub(crate) fn select_previous_system_metric(&mut self) {
        self.ram_vram_selected_index = self.ram_vram_selected_index.saturating_sub(1);
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn select_previous_system_activity_metric(&mut self) {
        self.system_activity_selected_index = self.system_activity_selected_index.saturating_sub(1);
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn select_next_system_metric(&mut self) {
        self.ram_vram_selected_index = self
            .ram_vram_selected_index
            .saturating_add(1)
            .min(SystemMetric::RAM_VRAM_PANEL.len().saturating_sub(1));
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn select_next_system_activity_metric(&mut self) {
        self.system_activity_selected_index = self
            .system_activity_selected_index
            .saturating_add(1)
            .min(SystemMetric::SYSTEM_ACTIVITY_PANEL.len().saturating_sub(1));
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn select_first_system_metric(&mut self) {
        self.ram_vram_selected_index = 0;
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn select_first_system_activity_metric(&mut self) {
        self.system_activity_selected_index = 0;
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn select_last_system_metric(&mut self) {
        self.ram_vram_selected_index = SystemMetric::RAM_VRAM_PANEL.len().saturating_sub(1);
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn select_last_system_activity_metric(&mut self) {
        self.system_activity_selected_index =
            SystemMetric::SYSTEM_ACTIVITY_PANEL.len().saturating_sub(1);
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn select_system_metric_index(&mut self, index: usize) {
        self.ram_vram_selected_index =
            index.min(SystemMetric::RAM_VRAM_PANEL.len().saturating_sub(1));
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn select_system_activity_metric_index(&mut self, index: usize) {
        self.system_activity_selected_index =
            index.min(SystemMetric::SYSTEM_ACTIVITY_PANEL.len().saturating_sub(1));
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn apply_selected_system_metric_to_details(&mut self) {
        let metric = self.selected_system_metric();
        self.status = format!("RAM/VRAM metric selected: {}", metric.label());
    }

    pub(crate) fn apply_selected_system_activity_metric_to_details(&mut self) {
        let metric = self.selected_system_activity_metric();
        self.status = format!("NW/DISK metric selected: {}", metric.label());
    }

    pub(crate) fn toggle_selected_system_graph(&mut self) {
        if self.focused_panel != FocusedPanel::System {
            self.status = "Graph registration requires RAM/VRAM focus".to_string();
            return;
        }
        self.toggle_graph_source(
            GraphSlot::system(self.selected_system_metric()),
            FocusedPanel::System,
        );
    }

    pub(crate) fn toggle_selected_system_activity_graph(&mut self) {
        if self.focused_panel != FocusedPanel::SystemActivity {
            self.status = "Graph registration requires NW/DISK focus".to_string();
            return;
        }
        self.toggle_graph_source(
            GraphSlot::system(self.selected_system_activity_metric()),
            FocusedPanel::SystemActivity,
        );
    }

    pub(crate) fn toggle_cpu_average_graph(&mut self) {
        if self.focused_panel != FocusedPanel::Cpu {
            self.status = "Graph registration requires CPUs focus".to_string();
            return;
        }
        self.toggle_graph_source(
            GraphSlot::system(SystemMetric::CpuAverage),
            FocusedPanel::Cpu,
        );
    }

    pub(crate) fn apply_selected_system_metric_to_visible_details(&mut self) {
        self.status = format!("RAM/VRAM row: {}", self.selected_system_metric().label());
    }

    pub(crate) fn apply_selected_system_activity_metric_to_visible_details(&mut self) {
        self.status = format!(
            "NW/DISK row: {}",
            self.selected_system_activity_metric().label()
        );
    }

    pub(crate) fn select_details_sample_older(&mut self, amount: usize) {
        self.details_sample_selected = self.details_sample_selected.saturating_sub(amount);
        self.ensure_details_sample_visible();
        self.details_live = false;
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection moved older".to_string();
    }

    pub(crate) fn select_details_sample_newer(&mut self, amount: usize) {
        self.details_sample_selected = self.details_sample_selected.saturating_add(amount);
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection moved newer".to_string();
    }

    pub(crate) fn select_details_sample_page_older(&mut self) {
        let amount = self.details_sample_page_size.max(1);
        self.details_sample_selected = self.details_sample_selected.saturating_sub(amount);
        self.details_sample_offset = self.details_sample_offset.saturating_sub(amount);
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = false;
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection moved one page older".to_string();
    }

    pub(crate) fn select_details_sample_page_newer(&mut self) {
        let amount = self.details_sample_page_size.max(1);
        self.details_sample_selected = self.details_sample_selected.saturating_add(amount);
        self.details_sample_offset = self.details_sample_offset.saturating_add(amount);
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection moved one page newer".to_string();
    }

    pub(crate) fn set_details_sample_offset(&mut self, offset: usize) {
        let sample_count = self.selected_sample_count();
        if sample_count == 0 {
            self.details_sample_offset = 0;
            self.details_sample_selected = 0;
            self.details_live = false;
            return;
        }

        let rows = self.details_sample_page_size.max(1).min(sample_count);
        let max_offset = sample_count.saturating_sub(rows);
        self.details_sample_offset = offset.min(max_offset);
        let visible_end = self.details_sample_offset + rows - 1;
        self.details_sample_selected = self
            .details_sample_selected
            .clamp(self.details_sample_offset, visible_end);
        self.details_live = self.details_sample_selected + 1 == sample_count;
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples scrolled".to_string();
    }

    pub(crate) fn select_details_sample_oldest(&mut self) {
        self.details_sample_selected = 0;
        self.details_sample_offset = 0;
        self.details_live = false;
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection: oldest".to_string();
    }

    pub(crate) fn select_details_sample_latest(&mut self) {
        self.details_sample_selected = self.selected_sample_count().saturating_sub(1);
        self.scroll_details_samples_to_latest();
        self.details_live = true;
        self.ensure_selected_sample_in_graph_window();
        self.status = "Samples selection: latest".to_string();
    }

    pub(crate) fn set_details_sample_selected(&mut self, index: usize) {
        self.details_sample_selected = index;
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.ensure_selected_sample_in_graph_window();
        self.status = format!("Samples selection: {}", self.details_sample_selected + 1);
    }

    pub(crate) fn set_details_sample_selected_manual(&mut self, index: usize) {
        self.details_sample_selected = index;
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = false;
        self.ensure_selected_sample_in_graph_window();
        self.status = format!("Samples selection: {}", self.details_sample_selected + 1);
    }

    pub(crate) fn select_details_sample_nearest_age_seconds(&mut self, age_seconds: i64) {
        let Some(slot) = self.active_graph_slot() else {
            return;
        };
        let samples = self.graph_slot_samples(slot);
        let Some(time_reference_at) = self.graph_time_reference_at() else {
            return;
        };
        let Some((index, _)) = samples.iter().enumerate().min_by_key(|(_, sample)| {
            let age = time_reference_at
                .signed_duration_since(sample.captured_at)
                .num_seconds()
                .max(0);
            (age - age_seconds).abs()
        }) else {
            return;
        };
        self.set_details_sample_selected_manual(index);
    }

    fn align_details_sample_selection_to_time(&mut self, captured_at: DateTime<Local>) {
        let Some(slot) = self.active_graph_slot() else {
            return;
        };
        let samples = self.graph_slot_samples(slot);
        let Some(index) = sample_index_nearest_time(&samples, captured_at) else {
            return;
        };
        self.details_sample_selected = index;
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.ensure_selected_sample_in_graph_window();
    }

    fn ensure_selected_sample_in_graph_window(&mut self) {
        if self.graph_show_all_samples {
            return;
        }
        let Some(slot) = self.active_graph_slot() else {
            return;
        };
        let samples = self.graph_slot_samples(slot);
        let Some(time_reference_at) = self.graph_time_reference_at() else {
            return;
        };
        let Some(selected) = samples.get(self.details_sample_selected) else {
            return;
        };
        let selected_age = time_reference_at
            .signed_duration_since(selected.captured_at)
            .num_seconds()
            .clamp(0, i64::from(u32::MAX)) as u32;
        let span = self.graph_time_span_seconds.max(1);
        let current_offset = self.graph_time_offset_seconds;
        let next_offset = if selected_age < current_offset {
            selected_age
        } else if selected_age > current_offset.saturating_add(span) {
            selected_age.saturating_sub(span)
        } else {
            return;
        };
        let max_offset = self.graph_time_max_seconds().saturating_sub(span);
        self.graph_time_offset_seconds = next_offset.min(max_offset);
        self.update_graph_time_window_right_edge();
    }

    pub(crate) fn select_process_details_target(&mut self) {
        self.details_target = DetailsTarget::Process;
    }

    pub(crate) fn selected_sample_count(&self) -> usize {
        self.active_graph_slot()
            .map(|slot| self.graph_slot_samples(slot).len())
            .unwrap_or(0)
    }

    pub(crate) fn active_ab_comparison(&self) -> Option<&AbComparison> {
        self.ab_comparison.as_ref()
    }

    pub(crate) fn set_ab_point_a(&mut self) {
        self.set_ab_point('A');
    }

    pub(crate) fn set_ab_point_b(&mut self) {
        self.set_ab_point('B');
    }

    pub(crate) fn jump_to_ab_point_a(&mut self) {
        self.jump_to_ab_point('A');
    }

    pub(crate) fn jump_to_ab_point_b(&mut self) {
        self.jump_to_ab_point('B');
    }

    pub(crate) fn clear_ab_comparison_with_status(&mut self) {
        self.ab_comparison = None;
        self.status = "A/B comparison cleared".to_string();
    }

    fn set_ab_point(&mut self, label: char) {
        if !self.show_details {
            self.status = "A/B requires Details".to_string();
            return;
        }
        let Some(point) = self.selected_ab_point() else {
            self.status = "Selected sample has no value".to_string();
            return;
        };

        let comparison = self
            .ab_comparison
            .get_or_insert_with(|| AbComparison { a: None, b: None });
        match label {
            'A' => comparison.a = Some(point),
            'B' => comparison.b = Some(point),
            _ => {}
        }
        self.status = format!(
            "{label} point set: {}",
            point.captured_at.format("%H:%M:%S"),
        );
    }

    fn jump_to_ab_point(&mut self, label: char) {
        if !self.show_details {
            self.status = "A/B requires Details".to_string();
            return;
        }
        let Some(comparison) = self.active_ab_comparison() else {
            self.status = "A/B not set".to_string();
            return;
        };
        let point = match label {
            'A' => comparison.a,
            'B' => comparison.b,
            _ => None,
        };
        let Some(point) = point else {
            self.status = format!("{label} point is not set");
            return;
        };
        let Some(index) = self.sample_index_at(point.captured_at) else {
            self.status = format!("{label} point sample is unavailable");
            return;
        };
        self.set_details_sample_selected_manual(index);
        self.status = format!("{label} point selected");
    }

    fn clear_ab_comparison(&mut self) {
        self.ab_comparison = None;
    }

    fn selected_ab_point(&self) -> Option<AbComparisonPoint> {
        let slot = self.active_graph_slot()?;
        let samples = self.graph_slot_samples(slot);
        let sample = samples.get(self.details_sample_selected)?;
        sample.value?;
        Some(AbComparisonPoint {
            captured_at: sample.captured_at,
        })
    }

    fn sample_index_at(&self, captured_at: DateTime<Local>) -> Option<usize> {
        let slot = self.active_graph_slot()?;
        self.graph_slot_samples(slot)
            .iter()
            .position(|sample| sample.captured_at == captured_at)
    }

    pub(crate) fn selected_process_column(&self) -> SortColumn {
        match self.selected_process_column_index {
            0 => SortColumn::Pid,
            1 => SortColumn::ProcessName,
            index => self
                .process_columns
                .get(index.saturating_sub(FIXED_PROCESS_COLUMN_COUNT))
                .copied()
                .map(SortColumn::Metric)
                .unwrap_or(SortColumn::Metric(MetricColumn::PrivateBytes)),
        }
    }

    pub(crate) fn selected_process_metric_column(&self) -> Option<MetricColumn> {
        match self.selected_process_column() {
            SortColumn::Metric(column) => Some(column),
            SortColumn::Pid | SortColumn::ProcessName => None,
        }
    }

    pub(crate) fn widen_selected_process_column(&mut self) {
        self.adjust_selected_process_column_width(1);
    }

    pub(crate) fn narrow_selected_process_column(&mut self) {
        self.adjust_selected_process_column_width(-1);
    }

    fn adjust_selected_process_column_width(&mut self, direction: i16) {
        let column = self.selected_process_column();
        let current = self.process_column_widths.resolved(column);
        let requested = if direction > 0 {
            current.saturating_add(1)
        } else {
            current.saturating_sub(1)
        };
        let next = column.clamp_width(requested);
        if next == current {
            self.status = format!("Column width limit: {} {current}", column.label());
            return;
        }
        self.process_column_widths.set(column, next);
        self.ensure_selected_process_column_visible();
        self.status = format!("Column width: {} {next}", column.label());
    }

    pub(crate) fn select_previous_process_column(&mut self) {
        self.details_target = DetailsTarget::Process;
        self.selected_process_column_index = self
            .selected_process_column_index
            .min(self.process_column_count().saturating_sub(1))
            .saturating_sub(1);
        self.ensure_selected_process_column_visible();
        self.status = format!(
            "Selected column: {}",
            self.selected_process_column().label()
        );
    }

    pub(crate) fn select_next_process_column(&mut self) {
        self.details_target = DetailsTarget::Process;
        self.selected_process_column_index = self
            .selected_process_column_index
            .min(self.process_column_count().saturating_sub(1))
            .saturating_add(1)
            .min(self.process_column_count().saturating_sub(1));
        self.ensure_selected_process_column_visible();
        self.status = format!(
            "Selected column: {}",
            self.selected_process_column().label()
        );
    }

    pub(crate) fn select_process_column_index(&mut self, index: usize) {
        self.details_target = DetailsTarget::Process;
        self.selected_process_column_index =
            index.min(self.process_column_count().saturating_sub(1));
        self.ensure_selected_process_column_visible();
        self.status = format!(
            "Selected column: {}",
            self.selected_process_column().label()
        );
    }

    pub(crate) fn move_selected_process_column_left(&mut self) {
        self.move_selected_process_metric_column(-1);
    }

    pub(crate) fn move_selected_process_column_right(&mut self) {
        self.move_selected_process_metric_column(1);
    }

    fn move_selected_process_metric_column(&mut self, direction: isize) {
        let Some(metric_index) = self
            .selected_process_column_index
            .checked_sub(FIXED_PROCESS_COLUMN_COUNT)
        else {
            self.status = "Only metric columns can be reordered".to_string();
            return;
        };
        if metric_index >= self.process_columns.len() {
            self.clamp_selected_process_column();
            return;
        }

        let next_metric_index = if direction < 0 {
            metric_index.checked_sub(1)
        } else {
            metric_index
                .checked_add(1)
                .filter(|index| *index < self.process_columns.len())
        };
        let Some(next_metric_index) = next_metric_index else {
            self.status = format!(
                "Column already at {} edge: {}",
                if direction < 0 { "left" } else { "right" },
                self.process_columns[metric_index].label()
            );
            return;
        };

        self.process_columns.swap(metric_index, next_metric_index);
        self.column_preset = ColumnPreset::Custom;
        self.selected_process_column_index = next_metric_index + FIXED_PROCESS_COLUMN_COUNT;
        self.ensure_selected_process_column_visible();
        self.apply_selected_process_column_to_details_metric();
        self.status = format!(
            "Moved column {}",
            self.process_columns[next_metric_index].label()
        );
    }

    fn process_column_count(&self) -> usize {
        FIXED_PROCESS_COLUMN_COUNT + self.process_columns.len()
    }

    fn process_table_area_width(&self) -> u16 {
        crate::ui::main_panel_areas_for_app(self.last_screen_area, self)
            .processes
            .area
            .width
    }

    fn visible_process_metric_range(&self) -> std::ops::Range<usize> {
        crate::ui::process_table_visible_metric_range(
            self.process_table_area_width(),
            &self.process_columns,
            self.process_metric_column_offset,
            &self.process_column_widths,
        )
    }

    fn ensure_selected_process_column_visible(&mut self) {
        if self.process_columns.is_empty() {
            self.process_metric_column_offset = 0;
            return;
        }
        self.process_metric_column_offset = self
            .process_metric_column_offset
            .min(self.process_columns.len().saturating_sub(1));
        let Some(metric_index) = self
            .selected_process_column_index
            .checked_sub(FIXED_PROCESS_COLUMN_COUNT)
        else {
            return;
        };
        let metric_index = metric_index.min(self.process_columns.len().saturating_sub(1));
        let range = self.visible_process_metric_range();
        if range.contains(&metric_index) {
            return;
        }
        if metric_index < range.start {
            self.process_metric_column_offset = metric_index;
            return;
        }

        while self.process_metric_column_offset < metric_index {
            self.process_metric_column_offset += 1;
            if self.visible_process_metric_range().contains(&metric_index) {
                return;
            }
        }
    }

    pub(crate) fn enter_details_live_mode(&mut self) {
        self.show_details = true;
        self.graph_time_offset_seconds = 0;
        self.graph_time_window_right_at = None;
        self.details_live = true;
        self.select_details_sample_latest();
        self.status = "Details live mode enabled".to_string();
    }

    pub(crate) fn toggle_graph_y_axis_zero_min(&mut self) {
        self.graph_y_axis_zero_min = !self.graph_y_axis_zero_min;
        self.status = if self.graph_y_axis_zero_min {
            "Graph Y axis: minimum fixed at 0".to_string()
        } else {
            "Graph Y axis: minimum follows visible data".to_string()
        };
    }

    pub(crate) fn toggle_graph_all_samples(&mut self) {
        self.graph_show_all_samples = !self.graph_show_all_samples;
        if self.graph_show_all_samples {
            self.graph_time_offset_seconds = 0;
            self.graph_time_window_right_at = None;
            self.details_live = true;
            self.status = format!(
                "Graph span: fit all ({}s)",
                self.effective_graph_time_span_seconds()
            );
        } else {
            self.status = format!("Graph span: {}s", self.graph_time_span_seconds);
        }
    }

    pub(crate) fn clamp_details_sample_selection(&mut self) {
        let sample_count = self.selected_sample_count();
        if sample_count == 0 {
            self.details_sample_selected = 0;
        } else {
            self.details_sample_selected = self.details_sample_selected.min(sample_count - 1);
        }
        self.clamp_details_sample_offset();
    }

    fn clamp_details_sample_offset(&mut self) {
        let sample_count = self.selected_sample_count();
        if sample_count == 0 {
            self.details_sample_offset = 0;
            return;
        }
        let rows = self.details_sample_page_size.max(1).min(sample_count);
        self.details_sample_offset = self
            .details_sample_offset
            .min(sample_count.saturating_sub(rows));
    }

    fn ensure_details_sample_visible(&mut self) {
        let sample_count = self.selected_sample_count();
        if sample_count == 0 {
            self.details_sample_offset = 0;
            return;
        }
        let rows = self.details_sample_page_size.max(1).min(sample_count);
        if self.details_sample_selected < self.details_sample_offset {
            self.details_sample_offset = self.details_sample_selected;
        } else if self.details_sample_selected >= self.details_sample_offset + rows {
            self.details_sample_offset = self.details_sample_selected + 1 - rows;
        }
        self.clamp_details_sample_offset();
    }

    fn scroll_details_samples_to_latest(&mut self) {
        let sample_count = self.selected_sample_count();
        if sample_count == 0 {
            self.details_sample_offset = 0;
            return;
        }
        let rows = self.details_sample_page_size.max(1).min(sample_count);
        self.details_sample_offset = sample_count.saturating_sub(rows);
    }

    pub(crate) fn can_zoom_graph_time_span(&self, zoom_in: bool) -> bool {
        if self.graph_show_all_samples && !zoom_in {
            return false;
        }
        let current = self.effective_graph_time_span_seconds();
        graph_zoom_target(current, self.graph_time_max_seconds(), zoom_in) != current
    }

    pub(crate) fn zoom_graph_time_span(&mut self, zoom_in: bool) {
        let max_span = self.graph_time_max_seconds();
        let current = self.effective_graph_time_span_seconds();
        if self.graph_show_all_samples && !zoom_in {
            return;
        }
        let next = graph_zoom_target(current, max_span, zoom_in);
        if next == current {
            return;
        }
        self.graph_show_all_samples = false;
        self.graph_time_span_seconds = next;
        self.graph_time_offset_seconds = self
            .graph_time_offset_seconds
            .min(max_span.saturating_sub(self.graph_time_span_seconds));
        self.update_graph_time_window_right_edge();
        self.status = format!("Graph span: {}s", self.graph_time_span_seconds);
    }

    pub(crate) fn shift_graph_time_window(&mut self, older: bool) {
        self.graph_show_all_samples = false;
        let max_offset = self
            .graph_time_max_seconds()
            .saturating_sub(self.graph_time_span_seconds);
        let step = graph_pan_step(self.graph_time_span_seconds);
        let candidate = if older {
            self.graph_time_offset_seconds
                .saturating_add(step)
                .min(max_offset)
        } else {
            self.graph_time_offset_seconds.saturating_sub(step)
        };
        if let Some(offset) = self.nearest_non_empty_graph_offset(candidate, older) {
            self.graph_time_offset_seconds = offset.min(max_offset);
        }
        self.details_live = self.graph_time_offset_seconds == 0;
        self.update_graph_time_window_right_edge();
        self.status = format!("Graph offset: -{}s", self.graph_time_offset_seconds);
    }

    pub(crate) fn set_graph_time_window_offset(&mut self, offset_seconds: u32) {
        self.graph_show_all_samples = false;
        let max_offset = self
            .graph_time_max_seconds()
            .saturating_sub(self.graph_time_span_seconds);
        let candidate = offset_seconds.min(max_offset);
        self.graph_time_offset_seconds = self
            .nearest_graph_offset_with_visible_sample(candidate)
            .unwrap_or(0)
            .min(max_offset);
        self.details_live = self.graph_time_offset_seconds == 0;
        self.update_graph_time_window_right_edge();
        self.status = format!("Graph offset: -{}s", self.graph_time_offset_seconds);
    }

    pub(crate) fn graph_visible_range_includes_latest_sample(&self) -> bool {
        self.graph_show_all_samples || self.graph_time_offset_seconds == 0
    }

    pub(crate) fn stop_graph_live_scroll_if_latest_sample_is_outside_visible_range(&mut self) {
        if !self.graph_visible_range_includes_latest_sample() {
            self.details_live = false;
            self.freeze_graph_time_window();
        }
    }

    fn freeze_graph_time_window(&mut self) {
        self.details_live = false;
        if self.graph_show_all_samples {
            return;
        }
        self.graph_time_window_right_at = self.graph_time_window_right_edge();
    }

    fn update_graph_time_window_right_edge(&mut self) {
        if self.graph_show_all_samples || self.graph_time_offset_seconds == 0 {
            self.graph_time_window_right_at = None;
        } else {
            self.graph_time_window_right_at = self.graph_time_window_right_edge();
        }
    }

    fn graph_time_window_right_edge(&self) -> Option<DateTime<Local>> {
        let time_reference_at = self.graph_time_reference_at()?;
        Some(
            time_reference_at
                - chrono::Duration::seconds(i64::from(self.graph_time_offset_seconds)),
        )
    }

    fn restore_frozen_graph_time_window(&mut self) {
        if self.graph_show_all_samples {
            return;
        }
        let Some(right_edge) = self.graph_time_window_right_at else {
            return;
        };
        let Some(time_reference_at) = self.graph_time_reference_at() else {
            return;
        };
        let offset = rounded_nonnegative_seconds_between(time_reference_at, right_edge);
        let max_offset = self
            .graph_time_max_seconds()
            .saturating_sub(self.graph_time_span_seconds);
        self.graph_time_offset_seconds = offset.min(max_offset);
    }

    fn nearest_graph_offset_with_visible_sample(&self, candidate: u32) -> Option<u32> {
        let span = self.graph_time_span_seconds;
        let max_offset = self.graph_time_max_seconds().saturating_sub(span);
        let ages = self.active_graph_sample_ages_seconds();
        if ages.is_empty() {
            return Some(0);
        }

        if ages
            .iter()
            .any(|age| *age >= candidate && *age <= candidate.saturating_add(span))
        {
            return Some(candidate);
        }

        let mut nearest = None;
        for age in ages {
            let lower = age.saturating_sub(span);
            let upper = age.min(max_offset);
            if lower > upper {
                continue;
            }
            let offset = candidate.clamp(lower, upper);
            let distance = candidate.abs_diff(offset);
            if nearest.is_none_or(|(_, best_distance)| distance < best_distance) {
                nearest = Some((offset, distance));
            }
        }
        nearest.map(|(offset, _)| offset)
    }

    fn nearest_non_empty_graph_offset(&self, candidate: u32, older: bool) -> Option<u32> {
        let span = self.graph_time_span_seconds;
        let end = candidate.saturating_add(span);
        let ages = self.active_graph_sample_ages_seconds();
        if ages.is_empty() {
            return Some(0);
        }
        if ages.iter().any(|age| *age >= candidate && *age <= end) {
            return Some(candidate);
        }
        if older {
            ages.into_iter()
                .filter(|age| *age > end)
                .min()
                .map(|age| age.saturating_sub(span))
        } else {
            ages.into_iter().filter(|age| *age < candidate).max()
        }
    }

    fn active_graph_sample_ages_seconds(&self) -> Vec<u32> {
        let Some(slot) = self.active_graph_slot() else {
            return Vec::new();
        };
        let samples = self.graph_slot_samples(slot);
        let Some(time_reference_at) = self.graph_time_reference_at() else {
            return Vec::new();
        };
        let mut ages = samples
            .iter()
            .map(|sample| {
                time_reference_at
                    .signed_duration_since(sample.captured_at)
                    .num_seconds()
                    .clamp(0, i64::from(u32::MAX)) as u32
            })
            .collect::<Vec<_>>();
        ages.sort_unstable();
        ages.dedup();
        ages
    }

    pub(crate) fn effective_graph_time_span_seconds(&self) -> u32 {
        if self.graph_show_all_samples {
            self.graph_sample_time_span_seconds()
                .max(u32::from(GRAPH_TIME_SPAN_MIN_SECONDS))
        } else {
            self.graph_time_span_seconds
        }
    }

    pub(crate) fn effective_graph_time_offset_seconds(&self) -> u32 {
        if self.graph_show_all_samples {
            0
        } else {
            self.graph_time_offset_seconds
        }
    }

    pub(crate) fn graph_time_reference_at(&self) -> Option<DateTime<Local>> {
        self.graph_sample_time_range().map(|(_, latest)| latest)
    }

    fn graph_sample_time_span_seconds(&self) -> u32 {
        self.graph_sample_time_range()
            .map(|(earliest, latest)| sample_time_span_seconds(earliest, latest))
            .unwrap_or(self.graph_time_span_seconds)
    }

    fn graph_sample_time_range(&self) -> Option<(DateTime<Local>, DateTime<Local>)> {
        self.graph_entries
            .iter()
            .filter_map(|entry| self.graph_slot_time_range(&entry.source))
            .fold(None, |range, (first, last)| {
                Some(match range {
                    Some((earliest, latest)) => (earliest.min(first), latest.max(last)),
                    None => (first, last),
                })
            })
    }

    fn graph_slot_time_range(
        &self,
        slot: &GraphSlot,
    ) -> Option<(DateTime<Local>, DateTime<Local>)> {
        match slot {
            GraphSlot::Process { identity, .. } => {
                self.display_process_history().time_range_for(identity)
            }
            GraphSlot::System { .. } => self.display_system_history().time_range(),
        }
    }

    fn graph_time_max_seconds(&self) -> u32 {
        if self.activity() == AppActivity::LogView {
            self.graph_sample_time_span_seconds()
                .max(LIVE_GRAPH_TIME_MAX_SECONDS)
        } else {
            LIVE_GRAPH_TIME_MAX_SECONDS
        }
    }

    pub(crate) fn toggle_watch_list(&mut self) {
        if self.watch_list.is_empty() {
            self.watch_enabled = false;
            self.status = "Tracking List is empty".to_string();
            return;
        }

        self.watch_enabled = !self.watch_enabled;
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = if self.watch_enabled {
            format!(
                "Tracked-only enabled ({} visible)",
                self.visible_tracked_process_count()
            )
        } else {
            "Tracked-only disabled".to_string()
        };
    }

    #[cfg(test)]
    pub(crate) fn add_selected_process_to_watch_list(&mut self) {
        if self.reject_tracking_list_change_while_recording() {
            return;
        }
        let Some(name) = self.selected_visible_process_name() else {
            self.status = "No process selected".to_string();
            return;
        };

        self.add_process_name_to_tracked_list(name);
    }

    pub(crate) fn toggle_selected_process_tracking(&mut self) {
        if self.reject_tracking_list_change_while_recording() {
            return;
        }
        let Some(name) = self.selected_visible_process_name() else {
            self.status = "No process selected".to_string();
            return;
        };

        if self.is_tracked_process_name(&name) {
            let (total_samples, discarded_samples) = self
                .process_history
                .prune_summary_for_name(&name, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
            if discarded_samples > 0 {
                self.request_tracked_remove_confirmation(name, total_samples, discarded_samples);
            } else {
                self.remove_process_name_from_tracked_list(name);
            }
        } else {
            self.add_process_name_to_tracked_list(name);
        }
    }

    fn request_tracked_remove_confirmation(
        &mut self,
        name: String,
        total_samples: usize,
        discarded_samples: usize,
    ) {
        self.show_tracked_remove_confirmation = true;
        self.tracked_remove_selection = TrackedRemoveSelection::Cancel;
        self.tracked_remove_name = name;
        self.tracked_remove_total_samples = total_samples;
        self.tracked_remove_discarded_samples = discarded_samples;
        self.status = "Removing this tracked process will discard older samples".to_string();
    }

    pub(crate) fn confirm_tracked_remove(&mut self) {
        if self.reject_tracking_list_change_while_recording() {
            self.reset_tracked_remove_confirmation();
            return;
        }
        let name = self.tracked_remove_name.clone();
        let discarded = self
            .process_history
            .prune_name_to_latest(&name, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
        self.reset_tracked_remove_confirmation();
        self.remove_process_name_from_tracked_list(name.clone());
        self.status =
            format!("Removed from Tracking List: {name}; discarded {discarded} older samples");
    }

    pub(crate) fn cancel_tracked_remove_confirmation(&mut self) {
        self.reset_tracked_remove_confirmation();
        self.status = "Tracked removal canceled".to_string();
    }

    pub(crate) fn toggle_tracked_remove_selection(&mut self) {
        self.tracked_remove_selection = self.tracked_remove_selection.toggled();
    }

    pub(crate) fn activate_tracked_remove_selection(&mut self) {
        match self.tracked_remove_selection {
            TrackedRemoveSelection::Remove => self.confirm_tracked_remove(),
            TrackedRemoveSelection::Cancel => self.cancel_tracked_remove_confirmation(),
        }
    }

    fn reset_tracked_remove_confirmation(&mut self) {
        self.show_tracked_remove_confirmation = false;
        self.tracked_remove_selection = TrackedRemoveSelection::Cancel;
        self.tracked_remove_name.clear();
        self.tracked_remove_total_samples = 0;
        self.tracked_remove_discarded_samples = 0;
    }

    fn add_process_name_to_tracked_list(&mut self, name: String) {
        if !self
            .watch_list
            .iter()
            .any(|watch_name| watch_name.eq_ignore_ascii_case(&name))
        {
            self.watch_list.push(name.clone());
            self.rebuild_normalized_watch_names();
            self.refresh_tracked_live_identities();
        }
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = format!("Added to Tracking List: {name}");
    }

    #[cfg(test)]
    pub(crate) fn remove_selected_process_from_watch_list(&mut self) {
        if self.reject_tracking_list_change_while_recording() {
            return;
        }
        let Some(name) = self.selected_visible_process_name() else {
            self.status = "No process selected".to_string();
            return;
        };

        self.remove_process_name_from_tracked_list(name);
    }

    fn remove_process_name_from_tracked_list(&mut self, name: String) {
        let before = self.watch_list.len();
        self.watch_list
            .retain(|watch_name| !watch_name.eq_ignore_ascii_case(&name));
        self.rebuild_normalized_watch_names();
        self.refresh_tracked_live_identities();
        if self.watch_list.is_empty() {
            self.watch_enabled = false;
        }
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = if self.watch_list.len() == before {
            format!("Not in Tracking List: {name}")
        } else {
            format!("Removed from Tracking List: {name}")
        };
    }

    pub(crate) fn reject_tracking_list_change_while_recording(&mut self) -> bool {
        if self.activity() != AppActivity::Recording {
            return false;
        }

        self.show_recording_tracking_fixed = true;
        self.recording_tracking_fixed_ok_hovered = false;
        self.status = "Tracking List is fixed while recording".to_string();
        true
    }

    pub(crate) fn open_tracked_lists(&mut self) {
        if self.reject_tracking_list_change_while_recording() {
            return;
        }
        if self.activity() == AppActivity::LogView {
            self.status = "Tracking Lists are unavailable in Log view".to_string();
            return;
        }
        let index = self
            .runtime
            .active_tracked_list
            .as_deref()
            .and_then(|active| {
                self.runtime
                    .saved_tracked_lists
                    .iter()
                    .position(|list| list.name.eq_ignore_ascii_case(active))
            })
            .map(|index| index.saturating_add(1))
            .unwrap_or(0)
            .min(self.runtime.saved_tracked_lists.len());
        let save_name_draft = self.runtime.active_tracked_list.clone().unwrap_or_default();
        let save_name_cursor = save_name_draft.len();
        self.tracked_lists_dialog = Some(TrackedListsDialog {
            index,
            scroll: ScrollableModalState {
                page_size: 8,
                ..ScrollableModalState::default()
            },
            view: TrackedListsView::Browse,
            focused_button: None,
            save_name_focused: false,
            startup_focused: false,
            save_name_draft,
            save_name_cursor,
            save_name_error: None,
            save_name_feedback: None,
            hovered_button: None,
        });
        self.ensure_tracked_list_selection_visible();
        self.status = "Tracking Lists".to_string();
    }

    pub(crate) fn close_tracked_lists(&mut self) {
        self.tracked_lists_dialog = None;
        self.status = "Ready".to_string();
    }

    pub(crate) fn tracked_lists_view(&self) -> Option<&TrackedListsView> {
        self.tracked_lists_dialog
            .as_ref()
            .map(|dialog| &dialog.view)
    }

    pub(crate) fn tracked_lists_index(&self) -> usize {
        self.tracked_lists_dialog
            .as_ref()
            .map(|dialog| dialog.index)
            .unwrap_or(0)
    }

    pub(crate) fn tracked_lists_scroll_offset(&self) -> usize {
        self.tracked_lists_dialog
            .as_ref()
            .map(|dialog| dialog.scroll.offset)
            .unwrap_or(0)
    }

    pub(crate) fn tracked_lists_entry_count(&self) -> usize {
        self.runtime.saved_tracked_lists.len().saturating_add(1)
    }

    pub(crate) fn tracked_lists_empty_selected(&self) -> bool {
        self.tracked_lists_index() == 0
    }

    pub(crate) fn empty_tracked_list_active(&self) -> bool {
        self.runtime.active_tracked_list.is_none() && self.watch_list.is_empty()
    }

    fn selected_saved_tracked_list_index(&self) -> Option<usize> {
        self.tracked_lists_index()
            .checked_sub(1)
            .filter(|index| *index < self.runtime.saved_tracked_lists.len())
    }

    pub(crate) fn tracked_lists_focused_button(&self) -> Option<TrackedListsButton> {
        self.tracked_lists_dialog
            .as_ref()
            .and_then(|dialog| dialog.focused_button)
    }

    pub(crate) fn tracked_lists_hovered_button(&self) -> Option<TrackedListsButton> {
        self.tracked_lists_dialog
            .as_ref()
            .and_then(|dialog| dialog.hovered_button)
    }

    pub(crate) fn tracked_lists_save_name_focused(&self) -> bool {
        self.tracked_lists_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.save_name_focused)
    }

    pub(crate) fn tracked_lists_startup_focused(&self) -> bool {
        self.tracked_lists_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.startup_focused)
    }

    pub(crate) fn tracked_lists_save_name(&self) -> Option<(&str, usize, Option<&str>)> {
        self.tracked_lists_dialog.as_ref().map(|dialog| {
            (
                dialog.save_name_draft.as_str(),
                dialog.save_name_cursor,
                dialog.save_name_error.as_deref(),
            )
        })
    }

    pub(crate) fn tracked_lists_save_feedback(&self) -> Option<&str> {
        self.tracked_lists_dialog
            .as_ref()
            .and_then(|dialog| dialog.save_name_feedback.as_deref())
    }

    pub(crate) fn focus_next_tracked_lists_control(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            if dialog.save_name_focused {
                dialog.save_name_focused = false;
                dialog.focused_button = Some(TrackedListsButton::Save);
                return;
            }
            if dialog.startup_focused {
                dialog.startup_focused = false;
                dialog.focused_button = Some(TrackedListsButton::Close);
                return;
            }
            dialog.focused_button = match dialog.focused_button {
                None => {
                    dialog.save_name_focused = true;
                    None
                }
                Some(TrackedListsButton::Save) => {
                    dialog.startup_focused = true;
                    None
                }
                Some(TrackedListsButton::Close) => None,
            };
        }
    }

    pub(crate) fn focus_previous_tracked_lists_control(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            if dialog.save_name_focused {
                dialog.save_name_focused = false;
                dialog.focused_button = None;
                return;
            }
            if dialog.startup_focused {
                dialog.startup_focused = false;
                dialog.focused_button = Some(TrackedListsButton::Save);
                return;
            }
            dialog.focused_button = match dialog.focused_button {
                None => Some(TrackedListsButton::Close),
                Some(TrackedListsButton::Save) => {
                    dialog.save_name_focused = true;
                    None
                }
                Some(TrackedListsButton::Close) => {
                    dialog.startup_focused = true;
                    None
                }
            };
        }
    }

    pub(crate) fn focus_tracked_lists_button(&mut self, button: TrackedListsButton) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.focused_button = Some(button);
            dialog.save_name_focused = false;
            dialog.startup_focused = false;
        }
    }

    pub(crate) fn focus_tracked_lists_list(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.focused_button = None;
            dialog.save_name_focused = false;
            dialog.startup_focused = false;
        }
    }

    pub(crate) fn focus_tracked_lists_save_name(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.focused_button = None;
            dialog.save_name_focused = true;
            dialog.startup_focused = false;
            dialog.save_name_cursor = dialog.save_name_draft.len();
        }
    }

    pub(crate) fn focus_tracked_lists_startup(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.focused_button = None;
            dialog.save_name_focused = false;
            dialog.startup_focused = true;
        }
    }

    pub(crate) fn select_next_tracked_list_startup(&mut self) {
        self.set_tracked_list_startup(self.runtime.tracked_list_startup.next());
    }

    pub(crate) fn select_previous_tracked_list_startup(&mut self) {
        self.set_tracked_list_startup(self.runtime.tracked_list_startup.previous());
    }

    fn set_tracked_list_startup(&mut self, startup: TrackedListStartup) {
        let previous = self.runtime.tracked_list_startup;
        self.runtime.tracked_list_startup = startup;
        if self.persist_tracked_list_changes() {
            self.status = format!("Tracking List startup: {}", startup.label());
        } else {
            self.runtime.tracked_list_startup = previous;
        }
    }

    pub(crate) fn set_tracked_lists_hovered_button(&mut self, button: Option<TrackedListsButton>) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.hovered_button = button;
        }
    }

    pub(crate) fn set_tracked_lists_page_size(&mut self, page_size: usize) {
        let total = self.tracked_lists_entry_count();
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.scroll.set_page_size(page_size, total);
        }
        self.ensure_tracked_list_selection_visible();
    }

    pub(crate) fn move_tracked_list_selection_up(&mut self, amount: usize) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.index = dialog.index.saturating_sub(amount);
        }
        self.ensure_tracked_list_selection_visible();
    }

    pub(crate) fn move_tracked_list_selection_down(&mut self, amount: usize) {
        let last = self.tracked_lists_entry_count().saturating_sub(1);
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.index = dialog.index.saturating_add(amount).min(last);
        }
        self.ensure_tracked_list_selection_visible();
    }

    pub(crate) fn move_tracked_list_selection_home(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.index = 0;
        }
        self.ensure_tracked_list_selection_visible();
    }

    pub(crate) fn move_tracked_list_selection_end(&mut self) {
        let last = self.tracked_lists_entry_count().saturating_sub(1);
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.index = last;
        }
        self.ensure_tracked_list_selection_visible();
    }

    pub(crate) fn select_tracked_list_index(&mut self, index: usize) {
        let last = self.tracked_lists_entry_count().saturating_sub(1);
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.index = index.min(last);
        }
        self.ensure_tracked_list_selection_visible();
    }

    fn ensure_tracked_list_selection_visible(&mut self) {
        let total = self.tracked_lists_entry_count();
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        dialog.index = dialog.index.min(total - 1);
        dialog.scroll.ensure_visible(dialog.index, total);
    }

    pub(crate) fn load_selected_tracked_list(&mut self) {
        if self.tracked_lists_empty_selected() {
            self.request_tracked_list_switch(None, Vec::new());
            return;
        }
        let Some(index) = self.selected_saved_tracked_list_index() else {
            self.status = "No saved Tracking List selected".to_string();
            return;
        };
        let Some(list) = self.runtime.saved_tracked_lists.get(index).cloned() else {
            self.status = "No saved Tracking List selected".to_string();
            return;
        };
        self.request_tracked_list_switch(Some(list.name), list.processes);
    }

    pub(crate) fn push_tracked_list_save_name_char(&mut self, ch: char) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.save_name_error = None;
            dialog.save_name_feedback = None;
            dialog.save_name_cursor = dialog.save_name_cursor.min(dialog.save_name_draft.len());
            dialog.save_name_draft.insert(dialog.save_name_cursor, ch);
            dialog.save_name_cursor += ch.len_utf8();
        }
    }

    pub(crate) fn pop_tracked_list_save_name_char(&mut self) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        if dialog.save_name_cursor == 0 {
            return;
        }
        dialog.save_name_error = None;
        dialog.save_name_feedback = None;
        let previous = dialog.save_name_draft[..dialog.save_name_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
        dialog
            .save_name_draft
            .drain(previous..dialog.save_name_cursor);
        dialog.save_name_cursor = previous;
    }

    pub(crate) fn delete_tracked_list_save_name_char(&mut self) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        if dialog.save_name_cursor >= dialog.save_name_draft.len() {
            return;
        }
        dialog.save_name_error = None;
        dialog.save_name_feedback = None;
        let next = dialog.save_name_draft[dialog.save_name_cursor..]
            .chars()
            .next()
            .map(|ch| dialog.save_name_cursor + ch.len_utf8())
            .unwrap_or(dialog.save_name_draft.len());
        dialog.save_name_draft.drain(dialog.save_name_cursor..next);
    }

    pub(crate) fn move_tracked_list_save_name_cursor_left(&mut self) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        if dialog.save_name_cursor > 0 {
            dialog.save_name_cursor = dialog.save_name_draft[..dialog.save_name_cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_tracked_list_save_name_cursor_right(&mut self) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        if dialog.save_name_cursor < dialog.save_name_draft.len() {
            dialog.save_name_cursor = dialog.save_name_draft[dialog.save_name_cursor..]
                .chars()
                .next()
                .map(|ch| dialog.save_name_cursor + ch.len_utf8())
                .unwrap_or(dialog.save_name_draft.len());
        }
    }

    pub(crate) fn move_tracked_list_save_name_cursor_home(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.save_name_cursor = 0;
        }
    }

    pub(crate) fn move_tracked_list_save_name_cursor_end(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.save_name_cursor = dialog.save_name_draft.len();
        }
    }

    pub(crate) fn save_current_tracked_list(&mut self) {
        let Some(name) = self
            .tracked_lists_dialog
            .as_ref()
            .map(|dialog| dialog.save_name_draft.trim().to_string())
        else {
            return;
        };
        if name.is_empty() {
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.save_name_error = Some("Name is required.".to_string());
                dialog.save_name_feedback = None;
            }
            self.status = "Name is required.".to_string();
            return;
        }
        if is_empty_tracked_list_name(&name) {
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.save_name_error = Some(format!(
                    "{EMPTY_TRACKED_LIST_NAME} is built in and cannot be overwritten."
                ));
                dialog.save_name_feedback = None;
            }
            self.status =
                format!("{EMPTY_TRACKED_LIST_NAME} is built in and cannot be overwritten");
            return;
        }

        let previous_lists = self.runtime.saved_tracked_lists.clone();
        let previous_active = self.runtime.active_tracked_list.clone();
        let (index, saved_name) = if let Some(index) = self
            .runtime
            .saved_tracked_lists
            .iter()
            .position(|list| list.name.eq_ignore_ascii_case(&name))
        {
            self.runtime.saved_tracked_lists[index].processes = self.watch_list.clone();
            (index, self.runtime.saved_tracked_lists[index].name.clone())
        } else {
            self.runtime
                .saved_tracked_lists
                .push(crate::config::SavedTrackedList {
                    name: name.clone(),
                    processes: self.watch_list.clone(),
                });
            (
                self.runtime.saved_tracked_lists.len().saturating_sub(1),
                name,
            )
        };
        self.runtime.active_tracked_list = Some(saved_name.clone());

        if self.persist_tracked_list_changes() {
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.index = index.saturating_add(1);
                dialog.save_name_draft = saved_name.clone();
                dialog.save_name_cursor = saved_name.len();
                dialog.save_name_error = None;
                dialog.save_name_feedback = Some(format!(
                    "Saved: {saved_name} · {} process{}",
                    self.watch_list.len(),
                    if self.watch_list.len() == 1 { "" } else { "es" }
                ));
            }
            self.ensure_tracked_list_selection_visible();
            self.status = format!("Saved Tracking List: {saved_name}");
        } else {
            self.runtime.saved_tracked_lists = previous_lists;
            self.runtime.active_tracked_list = previous_active;
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.save_name_feedback = None;
                dialog.save_name_error = Some("Save failed.".to_string());
            }
        }
    }

    pub(crate) fn begin_tracked_list_rename(&mut self) {
        let Some(index) = self.selected_saved_tracked_list_index() else {
            self.status = format!("{EMPTY_TRACKED_LIST_NAME} cannot be renamed");
            return;
        };
        let draft = self
            .runtime
            .saved_tracked_lists
            .get(index)
            .map(|list| list.name.clone())
            .unwrap_or_default();
        if draft.is_empty() {
            self.status = "No saved Tracking List selected".to_string();
            return;
        }
        let cursor = draft.len();
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.view = TrackedListsView::NameInput {
                draft,
                cursor,
                error: None,
            };
        }
    }

    pub(crate) fn cancel_tracked_list_subdialog(&mut self) {
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.view = TrackedListsView::Browse;
        }
        self.status = "Tracking Lists".to_string();
    }

    pub(crate) fn push_tracked_list_name_char(&mut self, ch: char) {
        if let Some(TrackedListsDialog {
            view:
                TrackedListsView::NameInput {
                    draft,
                    cursor,
                    error,
                    ..
                },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            *error = None;
            *cursor = (*cursor).min(draft.len());
            draft.insert(*cursor, ch);
            *cursor += ch.len_utf8();
        }
    }

    pub(crate) fn pop_tracked_list_name_char(&mut self) {
        if let Some(TrackedListsDialog {
            view:
                TrackedListsView::NameInput {
                    draft,
                    cursor,
                    error,
                    ..
                },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            if *cursor == 0 {
                return;
            }
            *error = None;
            let previous = draft[..*cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
            draft.drain(previous..*cursor);
            *cursor = previous;
        }
    }

    pub(crate) fn delete_tracked_list_name_char(&mut self) {
        if let Some(TrackedListsDialog {
            view:
                TrackedListsView::NameInput {
                    draft,
                    cursor,
                    error,
                    ..
                },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            if *cursor >= draft.len() {
                return;
            }
            *error = None;
            let next = draft[*cursor..]
                .chars()
                .next()
                .map(|ch| *cursor + ch.len_utf8())
                .unwrap_or(draft.len());
            draft.drain(*cursor..next);
        }
    }

    pub(crate) fn move_tracked_list_name_cursor_left(&mut self) {
        if let Some(TrackedListsDialog {
            view: TrackedListsView::NameInput { draft, cursor, .. },
            ..
        }) = self.tracked_lists_dialog.as_mut()
            && *cursor > 0
        {
            *cursor = draft[..*cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_tracked_list_name_cursor_right(&mut self) {
        if let Some(TrackedListsDialog {
            view: TrackedListsView::NameInput { draft, cursor, .. },
            ..
        }) = self.tracked_lists_dialog.as_mut()
            && *cursor < draft.len()
        {
            *cursor = draft[*cursor..]
                .chars()
                .next()
                .map(|ch| *cursor + ch.len_utf8())
                .unwrap_or(draft.len());
        }
    }

    pub(crate) fn move_tracked_list_name_cursor_home(&mut self) {
        if let Some(TrackedListsDialog {
            view: TrackedListsView::NameInput { cursor, .. },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            *cursor = 0;
        }
    }

    pub(crate) fn move_tracked_list_name_cursor_end(&mut self) {
        if let Some(TrackedListsDialog {
            view: TrackedListsView::NameInput { draft, cursor, .. },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            *cursor = draft.len();
        }
    }

    pub(crate) fn commit_tracked_list_name_input(&mut self) {
        let Some(name) = self.tracked_lists_dialog.as_ref().and_then(|dialog| {
            if let TrackedListsView::NameInput { draft, .. } = &dialog.view {
                Some(draft.trim().to_string())
            } else {
                None
            }
        }) else {
            return;
        };
        if name.is_empty() {
            self.set_tracked_list_name_error("Name is required.");
            return;
        }
        if is_empty_tracked_list_name(&name) {
            self.set_tracked_list_name_error(&format!(
                "{EMPTY_TRACKED_LIST_NAME} is built in and cannot be overwritten."
            ));
            return;
        }

        self.rename_selected_tracked_list(name);
    }

    fn set_tracked_list_name_error(&mut self, message: &str) {
        if let Some(TrackedListsDialog {
            view: TrackedListsView::NameInput { error, .. },
            ..
        }) = self.tracked_lists_dialog.as_mut()
        {
            *error = Some(message.to_string());
        }
        self.status = message.to_string();
    }

    fn rename_selected_tracked_list(&mut self, name: String) {
        let Some(index) = self.selected_saved_tracked_list_index() else {
            self.set_tracked_list_name_error(&format!(
                "{EMPTY_TRACKED_LIST_NAME} cannot be renamed."
            ));
            return;
        };
        let Some(old_name) = self
            .runtime
            .saved_tracked_lists
            .get(index)
            .map(|list| list.name.clone())
        else {
            self.set_tracked_list_name_error("No saved Tracking List selected.");
            return;
        };
        if self
            .runtime
            .saved_tracked_lists
            .iter()
            .enumerate()
            .any(|(saved_index, list)| {
                saved_index != index && list.name.eq_ignore_ascii_case(&name)
            })
        {
            self.set_tracked_list_name_error(
                "A saved Tracking List with that name already exists.",
            );
            return;
        }
        let previous_lists = self.runtime.saved_tracked_lists.clone();
        let previous_active = self.runtime.active_tracked_list.clone();
        self.runtime.saved_tracked_lists[index].name = name.clone();
        if self
            .runtime
            .active_tracked_list
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(&old_name))
        {
            self.runtime.active_tracked_list = Some(name.clone());
        }
        if self.persist_tracked_list_changes() {
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.view = TrackedListsView::Browse;
            }
            self.status = format!("Renamed Tracking List: {name}");
        } else {
            self.runtime.saved_tracked_lists = previous_lists;
            self.runtime.active_tracked_list = previous_active;
        }
    }

    pub(crate) fn request_delete_selected_tracked_list(&mut self) {
        let Some(index) = self.selected_saved_tracked_list_index() else {
            self.status = format!("{EMPTY_TRACKED_LIST_NAME} cannot be deleted");
            return;
        };
        let Some(name) = self
            .runtime
            .saved_tracked_lists
            .get(index)
            .map(|list| list.name.clone())
        else {
            self.status = "No saved Tracking List selected".to_string();
            return;
        };
        if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
            dialog.view = TrackedListsView::ConfirmDelete {
                name,
                selection: TrackedListConfirmSelection::Cancel,
            };
        }
    }

    pub(crate) fn toggle_tracked_list_confirmation_selection(&mut self) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        match &mut dialog.view {
            TrackedListsView::ConfirmDelete { selection, .. }
            | TrackedListsView::ConfirmSwitch { selection, .. } => {
                *selection = selection.toggled();
            }
            _ => {}
        }
    }

    pub(crate) fn set_tracked_list_confirmation_selection(
        &mut self,
        selection: TrackedListConfirmSelection,
    ) {
        let Some(dialog) = self.tracked_lists_dialog.as_mut() else {
            return;
        };
        match &mut dialog.view {
            TrackedListsView::ConfirmDelete {
                selection: current, ..
            }
            | TrackedListsView::ConfirmSwitch {
                selection: current, ..
            } => *current = selection,
            _ => {}
        }
    }

    pub(crate) fn activate_tracked_list_confirmation(&mut self) {
        let view = self
            .tracked_lists_dialog
            .as_ref()
            .map(|dialog| dialog.view.clone());
        match view {
            Some(TrackedListsView::ConfirmDelete {
                name,
                selection: TrackedListConfirmSelection::Apply,
            }) => self.delete_saved_tracked_list(name),
            Some(TrackedListsView::ConfirmSwitch {
                pending,
                selection: TrackedListConfirmSelection::Apply,
            }) => self.apply_tracked_list_switch(pending),
            Some(TrackedListsView::ConfirmDelete { .. })
            | Some(TrackedListsView::ConfirmSwitch { .. }) => self.cancel_tracked_list_subdialog(),
            _ => {}
        }
    }

    fn delete_saved_tracked_list(&mut self, name: String) {
        let previous_lists = self.runtime.saved_tracked_lists.clone();
        let previous_active = self.runtime.active_tracked_list.clone();
        self.runtime
            .saved_tracked_lists
            .retain(|list| !list.name.eq_ignore_ascii_case(&name));
        if self
            .runtime
            .active_tracked_list
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(&name))
        {
            self.runtime.active_tracked_list = None;
        }
        if self.persist_tracked_list_changes() {
            let last = self.tracked_lists_entry_count().saturating_sub(1);
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.index = dialog.index.min(last);
                dialog.view = TrackedListsView::Browse;
            }
            self.ensure_tracked_list_selection_visible();
            self.status = format!("Deleted Tracking List: {name}");
        } else {
            self.runtime.saved_tracked_lists = previous_lists;
            self.runtime.active_tracked_list = previous_active;
        }
    }

    fn request_tracked_list_switch(
        &mut self,
        target_name: Option<String>,
        target_processes: Vec<String>,
    ) {
        if self.reject_tracking_list_change_while_recording() {
            self.tracked_lists_dialog = None;
            return;
        }
        let target_processes = dedupe_process_names(target_processes);
        let target_normalized = normalized_process_names(&target_processes);
        let removed_names = self
            .watch_list
            .iter()
            .filter(|name| !target_normalized.contains(&name.trim().to_ascii_lowercase()))
            .cloned()
            .collect::<Vec<_>>();
        let mut affected_name_count = 0;
        let mut discarded_sample_count = 0;
        for name in &removed_names {
            let (_, discarded) = self
                .process_history
                .prune_summary_for_name(name, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
            if discarded > 0 {
                affected_name_count += 1;
                discarded_sample_count += discarded;
            }
        }
        let pending = PendingTrackedListSwitch {
            target_name,
            target_processes,
            removed_name_count: removed_names.len(),
            affected_name_count,
            discarded_sample_count,
        };
        if discarded_sample_count > 0 {
            if let Some(dialog) = self.tracked_lists_dialog.as_mut() {
                dialog.view = TrackedListsView::ConfirmSwitch {
                    pending,
                    selection: TrackedListConfirmSelection::Cancel,
                };
            }
            self.status = "Loading this Tracking List will discard older samples".to_string();
        } else {
            self.apply_tracked_list_switch(pending);
        }
    }

    fn apply_tracked_list_switch(&mut self, pending: PendingTrackedListSwitch) {
        if self.reject_tracking_list_change_while_recording() {
            self.tracked_lists_dialog = None;
            return;
        }
        let target_normalized = normalized_process_names(&pending.target_processes);
        let removed_names = self
            .watch_list
            .iter()
            .filter(|name| !target_normalized.contains(&name.trim().to_ascii_lowercase()))
            .cloned()
            .collect::<Vec<_>>();
        for name in &removed_names {
            self.process_history
                .prune_name_to_latest(name, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
        }
        self.watch_list = pending.target_processes;
        self.runtime.active_tracked_list = pending.target_name.clone();
        self.rebuild_normalized_watch_names();
        self.refresh_tracked_live_identities();
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.tracked_lists_dialog = None;
        self.status = pending
            .target_name
            .map(|name| format!("Loaded Tracking List: {name}"))
            .unwrap_or_else(|| "Started with an empty Tracking List".to_string());
    }

    fn persist_tracked_list_changes(&mut self) -> bool {
        let Some(path) = self.runtime.config_path.clone() else {
            return true;
        };
        match crate::config::write_app_config(&path, self) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Failed to save Tracking Lists: {error}");
                false
            }
        }
    }

    pub(crate) fn active_tracked_list_dirty(&self) -> bool {
        let Some(active_name) = self.runtime.active_tracked_list.as_deref() else {
            return !self.watch_list.is_empty();
        };
        let Some(saved) = self
            .runtime
            .saved_tracked_lists
            .iter()
            .find(|list| list.name.eq_ignore_ascii_case(active_name))
        else {
            return true;
        };
        normalized_process_names(&saved.processes) != self.normalized_watch_names
    }

    pub(crate) fn hide_selected_ghost_row(&mut self) {
        let Some(selected) = self.process_table_state.selected() else {
            self.status = "No process selected".to_string();
            return;
        };
        let Some(VisibleProcessEntry::Ghost(identity)) =
            self.visible_process_entries.get(selected).cloned()
        else {
            self.status = "Delete only hides exited tracked rows".to_string();
            return;
        };

        self.exited_tracked_rows.remove(&identity);
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = format!("Hidden exited tracked row: {}", identity.name);
    }

    pub(crate) fn request_process_kill_confirmation(&mut self) -> bool {
        if self.activity() == AppActivity::LogView {
            self.status = "Process kill is unavailable in Log view".to_string();
            return false;
        }
        if self.is_display_paused() {
            self.status = "Resume the live display before killing processes".to_string();
            return false;
        }

        let targets = self.selected_live_processes_for_kill();
        if targets.is_empty() {
            return false;
        }

        let image_count = distinct_process_kill_image_names(&targets).len();
        let row_count = targets.len();
        self.process_kill_targets = targets;
        self.process_kill_selection = ProcessKillSelection::Cancel;
        self.show_process_kill_confirmation = true;
        self.status = format!("Confirm kill for {row_count} selected rows / {image_count} images");
        true
    }

    pub(crate) fn confirm_process_kill(&mut self) {
        let image_names = distinct_process_kill_image_names(&self.process_kill_targets);
        let attempts = image_names
            .iter()
            .map(|image_name| taskkill_force_image(image_name))
            .collect::<Vec<_>>();
        self.reset_process_kill_confirmation();
        self.clear_process_multi_selection();

        let succeeded = attempts.iter().filter(|attempt| attempt.success).count();
        let failed = attempts.len().saturating_sub(succeeded);
        self.status = match (succeeded, failed) {
            (0, 0) => "No process image names selected".to_string(),
            (_, 0) => format!("Killed {succeeded} process image name(s)"),
            (0, _) => format!(
                "Kill failed for {failed} image name(s): {}",
                failed_taskkill_names(&attempts)
            ),
            _ => format!(
                "Killed {succeeded}; failed {failed}: {}",
                failed_taskkill_names(&attempts)
            ),
        };
    }

    pub(crate) fn cancel_process_kill_confirmation(&mut self) {
        self.reset_process_kill_confirmation();
        self.status = "Process kill canceled".to_string();
    }

    pub(crate) fn toggle_process_kill_selection(&mut self) {
        self.process_kill_selection = self.process_kill_selection.toggled();
    }

    pub(crate) fn activate_process_kill_selection(&mut self) {
        match self.process_kill_selection {
            ProcessKillSelection::Kill => self.confirm_process_kill(),
            ProcessKillSelection::Cancel => self.cancel_process_kill_confirmation(),
        }
    }

    fn reset_process_kill_confirmation(&mut self) {
        self.show_process_kill_confirmation = false;
        self.process_kill_selection = ProcessKillSelection::Cancel;
        self.process_kill_targets.clear();
    }

    fn selected_live_processes_for_kill(&self) -> Vec<ProcessKillTarget> {
        if !self.selected_process_identities.is_empty() {
            return self
                .visible_process_entries
                .iter()
                .filter_map(|entry| self.process_kill_target_for_entry(entry))
                .filter(|target| self.selected_process_identities.contains(&target.identity))
                .collect();
        }

        let Some(selected) = self.process_table_state.selected() else {
            return Vec::new();
        };
        self.visible_process_entries
            .get(selected)
            .and_then(|entry| self.process_kill_target_for_entry(entry))
            .into_iter()
            .collect()
    }

    fn process_kill_target_for_entry(
        &self,
        entry: &VisibleProcessEntry,
    ) -> Option<ProcessKillTarget> {
        let VisibleProcessEntry::Live(index) = entry else {
            return None;
        };
        let process = self.display_snapshot().processes.get(*index)?;
        Some(ProcessKillTarget {
            identity: ProcessIdentity::from_row(process),
            pid: process.pid,
            name: process.name.clone(),
        })
    }

    fn selected_visible_process_name(&self) -> Option<String> {
        let selected = self.process_table_state.selected()?;
        self.visible_process_at(selected)
            .map(|process| process.name.clone())
    }

    pub(crate) fn selected_visible_process_identity(&self) -> Option<ProcessIdentity> {
        let selected = self.process_table_state.selected()?;
        self.visible_process_identity_at(selected)
    }

    pub(crate) fn selected_visible_process(&self) -> Option<&ProcessRow> {
        let selected = self.process_table_state.selected()?;
        let entry = self.visible_process_entries.get(selected)?;
        self.identity_for_visible_entry(entry)?;
        self.process_for_visible_entry(entry)
    }

    pub(crate) fn process_info_for_selected(&self) -> Option<&ProcessInfo> {
        let cache = self.display_process_info_cache();
        if let Some(target) = &self.process_info_target {
            return cache.get(&target.identity);
        }
        let identity = self.selected_visible_process_identity()?;
        cache.get(&identity).or_else(|| {
            self.display_process_info_identity()
                .and_then(|identity| cache.get(identity))
        })
    }

    pub(crate) fn process_info_target_process(&self) -> Option<&ProcessRow> {
        self.process_info_target
            .as_ref()
            .map(|target| &target.process)
    }

    pub(crate) fn process_info_target_is_currently_live(&self) -> bool {
        let Some(target) = &self.process_info_target else {
            return false;
        };
        matches!(target.lifecycle, ProcessLifecycle::Live)
            && self
                .snapshot
                .processes
                .iter()
                .any(|process| ProcessIdentity::from_row(process) == target.identity)
    }

    pub(crate) fn process_info_metrics_view(&self) -> Option<ProcessInfoMetricsView> {
        let target = self.process_info_target.as_ref()?;
        let current_at = self.display_snapshot().captured_at;
        let history = self.display_process_history();
        let comparison = self.ab_comparison.as_ref();
        let point_a = comparison.and_then(|comparison| comparison.a);
        let point_b = comparison.and_then(|comparison| comparison.b);
        let (value_at, value_heading, delta_heading) = match (point_a, point_b) {
            (Some(_), Some(point_b)) => (point_b.captured_at, "At B", Some("B-A")),
            (Some(_), None) => (current_at, "Current", Some("Delta from A")),
            _ => (current_at, "Current", None),
        };
        let value_sample = history.sample_at(&target.identity, value_at);
        let baseline_sample =
            point_a.and_then(|point| history.sample_at(&target.identity, point.captured_at));
        let range = match (point_a, point_b) {
            (Some(point_a), Some(point_b)) => format!(
                "A {} -> B {}",
                format_process_info_time(point_a.captured_at),
                format_process_info_time(point_b.captured_at)
            ),
            (Some(point_a), None) => format!(
                "A {} -> Current {}",
                format_process_info_time(point_a.captured_at),
                format_process_info_time(current_at)
            ),
            _ => format!("Current {}", format_process_info_time(current_at)),
        };
        let rows = PROCESS_INFO_METRIC_COLUMNS
            .into_iter()
            .map(|column| {
                let value =
                    value_sample.and_then(|sample| ProcessMetricValue::from_sample(sample, column));
                let baseline = baseline_sample
                    .and_then(|sample| ProcessMetricValue::from_sample(sample, column));
                ProcessInfoMetricRow {
                    label: process_info_metric_label(column),
                    value: value
                        .map(ProcessMetricValue::format)
                        .unwrap_or_else(|| "--".to_string()),
                    delta: delta_heading.map(|_| {
                        value
                            .zip(baseline)
                            .and_then(|(value, baseline)| value.format_delta(baseline))
                            .unwrap_or_else(|| "--".to_string())
                    }),
                }
            })
            .collect();
        Some(ProcessInfoMetricsView {
            value_heading,
            delta_heading,
            range,
            rows,
        })
    }

    pub(crate) fn set_process_info_page_size(&mut self, page_size: usize) {
        let total = self.process_info_total_rows();
        self.active_process_info_scroll_mut()
            .set_page_size(page_size, total);
    }

    pub(crate) fn process_info_page_size(&self) -> usize {
        self.active_process_info_scroll().page_size.max(1)
    }

    pub(crate) fn process_info_scroll_offset(&self) -> usize {
        self.active_process_info_scroll().offset
    }

    pub(crate) fn scroll_process_info_up(&mut self, amount: usize) {
        self.active_process_info_scroll_mut().scroll_up(amount);
    }

    pub(crate) fn scroll_process_info_down(&mut self, amount: usize) {
        let total = self.process_info_total_rows();
        self.active_process_info_scroll_mut()
            .scroll_down(amount, total);
    }

    pub(crate) fn scroll_process_info_home(&mut self) {
        self.active_process_info_scroll_mut().scroll_home();
    }

    pub(crate) fn scroll_process_info_end(&mut self) {
        let total = self.process_info_total_rows();
        self.active_process_info_scroll_mut().scroll_end(total);
    }

    pub(crate) fn next_process_info_tab(&mut self) -> Result<()> {
        self.activate_process_info_tab(self.process_info_tab.next())
    }

    pub(crate) fn previous_process_info_tab(&mut self) -> Result<()> {
        self.activate_process_info_tab(self.process_info_tab.previous())
    }

    pub(crate) fn activate_process_info_tab(&mut self, tab: ProcessInfoTab) -> Result<()> {
        if !self.show_process_info_dialog {
            return Ok(());
        }
        self.process_info_focus = ProcessInfoFocus::Content;
        if self.process_info_tab == tab {
            return Ok(());
        }
        self.active_process_info_scroll_mut().stop_drag();
        self.process_modules_show_detail = false;
        self.process_environment_show_detail = false;
        self.process_info_tab = tab;
        match tab {
            ProcessInfoTab::Image => self.ensure_selected_process_info(),
            ProcessInfoTab::Files => self.ensure_open_files_for_target()?,
            ProcessInfoTab::Dlls => self.ensure_process_modules_for_target()?,
            ProcessInfoTab::Environment => self.ensure_process_environment_for_target()?,
            ProcessInfoTab::Metrics => {}
        }
        Ok(())
    }

    pub(crate) fn focus_next_process_info_control(&mut self) {
        self.process_info_focus = match self.process_info_focus {
            ProcessInfoFocus::Content => ProcessInfoFocus::Close,
            ProcessInfoFocus::Close => ProcessInfoFocus::Content,
        };
    }

    pub(crate) fn focus_previous_process_info_control(&mut self) {
        self.focus_next_process_info_control();
    }

    fn active_process_info_scroll(&self) -> &ScrollableModalState {
        match self.process_info_tab {
            ProcessInfoTab::Metrics => &self.process_info_scroll,
            ProcessInfoTab::Image => &self.process_info_image_scroll,
            ProcessInfoTab::Files => &self.open_files_scroll,
            ProcessInfoTab::Dlls => &self.process_info_dlls_scroll,
            ProcessInfoTab::Environment => &self.process_info_environment_scroll,
        }
    }

    fn active_process_info_scroll_mut(&mut self) -> &mut ScrollableModalState {
        match self.process_info_tab {
            ProcessInfoTab::Metrics => &mut self.process_info_scroll,
            ProcessInfoTab::Image => &mut self.process_info_image_scroll,
            ProcessInfoTab::Files => &mut self.open_files_scroll,
            ProcessInfoTab::Dlls => &mut self.process_info_dlls_scroll,
            ProcessInfoTab::Environment => &mut self.process_info_environment_scroll,
        }
    }

    fn process_info_total_rows(&self) -> usize {
        crate::ui::process_info_total_rows(self)
    }

    pub(crate) fn open_system_info_dialog(&mut self) {
        self.show_system_info_dialog = true;
        self.status = "System Info shown".to_string();
    }

    pub(crate) fn close_system_info_dialog(&mut self) {
        self.show_system_info_dialog = false;
        self.status = "System Info closed".to_string();
    }

    pub(crate) fn ensure_selected_process_info(&mut self) {
        self.schedule_selected_process_info(false);
    }

    pub(crate) fn refresh_selected_process_info(&mut self) {
        self.schedule_selected_process_info(true);
    }

    fn schedule_selected_process_info(&mut self, force_refresh: bool) {
        if self.activity() == AppActivity::LogView {
            self.pending_process_info = None;
            return;
        }
        if !self.show_process_info_dialog {
            return;
        }
        let Some(target) = self.process_info_target.clone() else {
            self.pending_process_info = None;
            return;
        };
        let identity = target.identity;
        if !force_refresh && self.process_info_cache.contains_key(&identity) {
            self.process_info_display_identity = Some(identity);
            self.pending_process_info = None;
            return;
        }
        if self.process_info_in_flight.as_ref() == Some(&identity)
            && self.process_info_in_flight_generation == Some(self.process_info_generation)
        {
            self.pending_process_info = None;
            return;
        }
        self.pending_process_info = Some(PendingProcessInfo {
            generation: self.process_info_generation,
            identity,
            process: target.process,
            lifecycle: target.lifecycle,
            changed_at: Instant::now(),
            force_refresh,
        });
    }

    fn cancel_process_info_request(&mut self) {
        self.pending_process_info = None;
        self.process_info_in_flight = None;
        self.process_info_in_flight_generation = None;
    }

    pub(crate) fn process_info_poll_timeout(&self) -> Option<Duration> {
        if let Some(pending) = &self.pending_process_info {
            return Some(
                PROCESS_INFO_DEBOUNCE
                    .checked_sub(pending.changed_at.elapsed())
                    .unwrap_or_else(|| Duration::from_secs(0)),
            );
        }
        self.process_info_in_flight
            .as_ref()
            .map(|_| PROCESS_INFO_IN_FLIGHT_POLL_INTERVAL)
    }

    pub(crate) fn request_due_process_info(&mut self) -> Result<bool> {
        if self.process_info_in_flight.is_some() {
            return Ok(false);
        }
        let Some(pending) = self.pending_process_info.as_ref() else {
            return Ok(false);
        };
        if pending.changed_at.elapsed() < PROCESS_INFO_DEBOUNCE {
            return Ok(false);
        }
        if !pending.force_refresh && self.process_info_cache.contains_key(&pending.identity) {
            self.process_info_display_identity = Some(pending.identity.clone());
            self.pending_process_info = None;
            return Ok(true);
        }

        let pending = self
            .pending_process_info
            .take()
            .expect("pending process info should exist");
        self.process_info_worker.request_info(
            pending.generation,
            pending.identity.clone(),
            pending.process,
            pending.lifecycle,
        )?;
        self.process_info_in_flight = Some(pending.identity);
        self.process_info_in_flight_generation = Some(pending.generation);
        Ok(false)
    }

    pub(crate) fn poll_process_info_results(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.process_info_worker.try_recv() {
                Ok(result) => {
                    changed |= self.apply_process_info_result(result);
                }
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => {
                    self.process_info_in_flight = None;
                    self.process_info_in_flight_generation = None;
                    self.status = "Warning: process info worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_process_info_result(&mut self, result: ProcessInfoResult) -> bool {
        if self.process_info_in_flight.as_ref() != Some(&result.identity)
            || self.process_info_in_flight_generation != Some(result.generation)
        {
            return false;
        }
        self.process_info_in_flight = None;
        self.process_info_in_flight_generation = None;
        if !self.show_process_info_dialog
            || result.generation != self.process_info_generation
            || self
                .process_info_target
                .as_ref()
                .map(|target| &target.identity)
                != Some(&result.identity)
        {
            return false;
        }
        self.process_info_display_identity = Some(result.identity.clone());
        self.process_info_cache.insert(result.identity, result.info);
        true
    }

    pub(crate) fn open_selected_process_files(&mut self) -> Result<()> {
        let Some(target) = self.selected_process_info_target() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };
        if !matches!(target.lifecycle, ProcessLifecycle::Live) {
            self.status = "Open files require a live process".to_string();
            return Ok(());
        }
        self.open_process_info_dialog(target, ProcessInfoTab::Files)
    }

    pub(crate) fn open_active_graph_process_info_dialog(&mut self) -> Result<()> {
        let Some(slot) = self.active_graph_slot() else {
            self.status = "No active Graph selected".to_string();
            return Ok(());
        };
        let Some(identity) = slot.process_identity().cloned() else {
            self.status = "Process Info is available only for process Graphs".to_string();
            return Ok(());
        };
        let Some(target) = self.process_info_target_for_identity(&identity) else {
            self.status = "Graphed process is unavailable".to_string();
            return Ok(());
        };
        let initial_tab = self.process_info_tab;
        self.open_process_info_dialog(target, initial_tab)
    }

    pub(crate) fn open_selected_process_info_dialog(&mut self) -> Result<()> {
        let Some(target) = self.selected_process_info_target() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };
        let initial_tab = self.process_info_tab;
        self.open_process_info_dialog(target, initial_tab)
    }

    fn selected_process_info_target(&self) -> Option<ProcessInfoDialogTarget> {
        let process = self.selected_visible_process().cloned()?;
        let identity = ProcessIdentity::from_row(&process);
        let lifecycle = self
            .selected_visible_process_lifecycle()
            .unwrap_or(ProcessLifecycle::Live);
        Some(ProcessInfoDialogTarget {
            identity,
            process,
            lifecycle,
        })
    }

    fn process_info_target_for_identity(
        &self,
        identity: &ProcessIdentity,
    ) -> Option<ProcessInfoDialogTarget> {
        if let Some(process) = self
            .display_snapshot()
            .processes
            .iter()
            .find(|process| ProcessIdentity::from_row(process) == *identity)
        {
            return Some(ProcessInfoDialogTarget {
                identity: identity.clone(),
                process: process.clone(),
                lifecycle: ProcessLifecycle::Live,
            });
        }
        self.display_exited_tracked_rows()
            .get(identity)
            .map(|row| ProcessInfoDialogTarget {
                identity: identity.clone(),
                process: row.process.clone(),
                lifecycle: ProcessLifecycle::Exited {
                    exited_at: row.exited_at,
                },
            })
    }

    fn open_process_info_dialog(
        &mut self,
        target: ProcessInfoDialogTarget,
        initial_tab: ProcessInfoTab,
    ) -> Result<()> {
        let process_name = target.process.name.clone();
        self.process_info_generation = self.process_info_generation.wrapping_add(1).max(1);
        self.process_info_target = Some(target);
        self.process_info_tab = initial_tab;
        self.process_info_focus = ProcessInfoFocus::Content;
        self.show_process_info_dialog = true;
        self.process_info_scroll.reset();
        self.process_info_image_scroll.reset();
        self.process_info_dlls_scroll.reset();
        self.process_info_environment_scroll.reset();
        self.open_files_scroll.reset();
        self.open_files_filter.clear();
        self.open_files_filter_cursor = 0;
        self.open_files_result = None;
        self.open_files_result_identity = None;
        self.open_files_in_flight = None;
        self.open_files_in_flight_generation = None;
        self.process_modules_result = None;
        self.process_modules_result_identity = None;
        self.process_modules_error = None;
        self.process_modules_in_flight = None;
        self.process_modules_in_flight_generation = None;
        self.process_modules_in_flight_request_id = None;
        self.process_modules_filter.clear();
        self.process_modules_filter_cursor = 0;
        self.process_modules_selected = 0;
        self.process_modules_show_detail = false;
        self.process_environment_result = None;
        self.process_environment_result_identity = None;
        self.process_environment_error = None;
        self.process_environment_in_flight = None;
        self.process_environment_in_flight_generation = None;
        self.process_environment_in_flight_request_id = None;
        self.process_environment_filter.clear();
        self.process_environment_filter_cursor = 0;
        self.process_environment_selected = 0;
        self.process_environment_show_detail = false;
        if self.activity() == AppActivity::LogView {
            self.pending_process_info = None;
        } else {
            self.ensure_selected_process_info();
        }
        self.status = format!("Process Info: {process_name}");
        if initial_tab == ProcessInfoTab::Files {
            self.ensure_open_files_for_target()?;
        } else if initial_tab == ProcessInfoTab::Dlls {
            self.ensure_process_modules_for_target()?;
        } else if initial_tab == ProcessInfoTab::Environment {
            self.ensure_process_environment_for_target()?;
        }
        Ok(())
    }

    pub(crate) fn close_process_info_dialog(&mut self) {
        self.show_process_info_dialog = false;
        self.process_info_target = None;
        self.process_info_scroll.stop_drag();
        self.process_info_image_scroll.stop_drag();
        self.process_info_dlls_scroll.stop_drag();
        self.process_info_environment_scroll.stop_drag();
        self.open_files_scroll.stop_drag();
        self.cancel_process_info_request();
        self.open_files_in_flight = None;
        self.open_files_in_flight_generation = None;
        self.process_modules_in_flight = None;
        self.process_modules_in_flight_generation = None;
        self.process_modules_in_flight_request_id = None;
        self.process_modules_show_detail = false;
        self.process_environment_in_flight = None;
        self.process_environment_in_flight_generation = None;
        self.process_environment_in_flight_request_id = None;
        self.process_environment_result = None;
        self.process_environment_result_identity = None;
        self.process_environment_error = None;
        self.process_environment_show_detail = false;
        self.status = "Process Info closed".to_string();
    }

    pub(crate) fn refresh_open_files(&mut self) -> Result<()> {
        if self.open_files_in_flight.is_some()
            && self.open_files_in_flight_generation == Some(self.process_info_generation)
        {
            self.status = "Open files refresh already in progress".to_string();
            return Ok(());
        }
        self.request_open_files_for_target(false, "Refreshing open files for")
    }

    fn ensure_open_files_for_target(&mut self) -> Result<()> {
        let Some(target) = self.process_info_target.as_ref() else {
            return Ok(());
        };
        if self.open_files_result_identity.as_ref() == Some(&target.identity)
            || (self.open_files_in_flight.as_ref() == Some(&target.identity)
                && self.open_files_in_flight_generation == Some(self.process_info_generation))
        {
            return Ok(());
        }
        self.request_open_files_for_target(true, "Loading open files for")
    }

    fn request_open_files_for_target(
        &mut self,
        clear_previous_result: bool,
        status_prefix: &str,
    ) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            self.status = "Open files are unavailable in Log view".to_string();
            return Ok(());
        }
        let Some(target) = self.process_info_target.clone() else {
            self.status = "No Process Info target".to_string();
            return Ok(());
        };
        if !matches!(target.lifecycle, ProcessLifecycle::Live) {
            self.status = "Open files require a live process".to_string();
            return Ok(());
        }
        if !self.process_info_target_is_currently_live() {
            self.open_files_result = Some(OpenFilesReport::unavailable(
                &target.process,
                crate::samplers::open_files::OpenFilesError::ProcessExited,
            ));
            self.open_files_result_identity = Some(target.identity);
            self.status = "Process has exited".to_string();
            return Ok(());
        }
        let identity = target.identity;
        let process = target.process;

        self.open_files_worker.request_open_files(
            self.process_info_generation,
            identity.clone(),
            process.clone(),
        )?;
        if clear_previous_result {
            self.open_files_result = None;
            self.open_files_result_identity = None;
        }
        self.open_files_in_flight = Some(identity);
        self.open_files_in_flight_generation = Some(self.process_info_generation);
        self.status = format!("{status_prefix} {}", process.name);
        Ok(())
    }

    pub(crate) fn open_files_poll_timeout(&self) -> Option<Duration> {
        self.open_files_in_flight
            .as_ref()
            .map(|_| OPEN_FILES_IN_FLIGHT_POLL_INTERVAL)
    }

    pub(crate) fn poll_open_files_results(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.open_files_worker.try_recv() {
                Ok(result) => {
                    changed |= self.apply_open_files_result(result);
                }
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => {
                    self.open_files_in_flight = None;
                    self.open_files_in_flight_generation = None;
                    self.status = "Warning: open files worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_open_files_result(&mut self, result: OpenFilesResult) -> bool {
        if self.open_files_in_flight.as_ref() != Some(&result.identity)
            || self.open_files_in_flight_generation != Some(result.generation)
        {
            return false;
        }
        self.open_files_in_flight = None;
        self.open_files_in_flight_generation = None;
        if !self.show_process_info_dialog
            || result.generation != self.process_info_generation
            || self
                .process_info_target
                .as_ref()
                .map(|target| &target.identity)
                != Some(&result.identity)
        {
            return false;
        }
        let entry_count = result.report.entries.len();
        let process_name = result.report.process_name.clone();
        self.status = if let Some(error) = &result.report.error {
            format!(
                "Open files unavailable for {process_name}: {}",
                error.message()
            )
        } else {
            format!("Loaded {entry_count} open file paths for {process_name}")
        };
        self.open_files_result_identity = Some(result.identity);
        self.open_files_result = Some(result.report);
        self.open_files_scroll.set_page_size(
            self.open_files_scroll.page_size,
            self.open_files_total_rows(),
        );
        true
    }

    pub(crate) fn push_open_files_filter_char(&mut self, ch: char) {
        self.open_files_filter_cursor = self
            .open_files_filter_cursor
            .min(self.open_files_filter.len());
        self.open_files_filter
            .insert(self.open_files_filter_cursor, ch);
        self.open_files_filter_cursor += ch.len_utf8();
        self.open_files_scroll.scroll_home();
        self.open_files_scroll.set_page_size(
            self.open_files_scroll.page_size,
            self.open_files_total_rows(),
        );
    }

    pub(crate) fn pop_open_files_filter_char(&mut self) {
        if self.open_files_filter_cursor > 0 {
            let previous = self.open_files_filter[..self.open_files_filter_cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.open_files_filter
                .drain(previous..self.open_files_filter_cursor);
            self.open_files_filter_cursor = previous;
        }
        self.open_files_scroll.scroll_home();
        self.open_files_scroll.set_page_size(
            self.open_files_scroll.page_size,
            self.open_files_total_rows(),
        );
    }

    pub(crate) fn delete_open_files_filter_char(&mut self) {
        if self.open_files_filter_cursor < self.open_files_filter.len() {
            let next = self.open_files_filter[self.open_files_filter_cursor..]
                .chars()
                .next()
                .map(|ch| self.open_files_filter_cursor + ch.len_utf8())
                .unwrap_or(self.open_files_filter.len());
            self.open_files_filter
                .drain(self.open_files_filter_cursor..next);
        }
        self.open_files_scroll.scroll_home();
        self.open_files_scroll.set_page_size(
            self.open_files_scroll.page_size,
            self.open_files_total_rows(),
        );
    }

    pub(crate) fn move_open_files_filter_cursor_left(&mut self) {
        if self.open_files_filter_cursor == 0 {
            return;
        }
        self.open_files_filter_cursor = self.open_files_filter[..self.open_files_filter_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub(crate) fn move_open_files_filter_cursor_right(&mut self) {
        if self.open_files_filter_cursor >= self.open_files_filter.len() {
            return;
        }
        self.open_files_filter_cursor = self.open_files_filter[self.open_files_filter_cursor..]
            .chars()
            .next()
            .map(|ch| self.open_files_filter_cursor + ch.len_utf8())
            .unwrap_or(self.open_files_filter.len());
    }

    pub(crate) fn start_process_info_scrollbar_drag(&mut self, x: u16, y: u16, area: Rect) -> bool {
        let Some(scrollbar) = crate::ui::process_info_scrollbar_area_for_screen(area, self) else {
            return false;
        };
        if x != scrollbar.x || y < scrollbar.y || y >= scrollbar.bottom() {
            return false;
        }
        let total = self.process_info_total_rows();
        self.active_process_info_scroll_mut()
            .start_drag(scrollbar, y, total);
        true
    }

    pub(crate) fn drag_process_info_scrollbar(&mut self, y: u16, area: Rect) {
        if let Some(scrollbar) = crate::ui::process_info_scrollbar_area_for_screen(area, self) {
            let total = self.process_info_total_rows();
            self.active_process_info_scroll_mut()
                .drag_to(scrollbar, y, total);
        }
    }

    pub(crate) fn stop_process_info_scrollbar_drag(&mut self) {
        self.active_process_info_scroll_mut().stop_drag();
    }

    pub(crate) fn process_info_scrollbar_dragging(&self) -> bool {
        self.active_process_info_scroll().dragging
    }

    pub(crate) fn open_files_total_rows(&self) -> usize {
        crate::ui::open_files_total_rows(self)
    }

    pub(crate) fn process_info_detail_is_open(&self) -> bool {
        match self.process_info_tab {
            ProcessInfoTab::Dlls => self.process_modules_show_detail,
            ProcessInfoTab::Environment => self.process_environment_show_detail,
            ProcessInfoTab::Metrics | ProcessInfoTab::Image | ProcessInfoTab::Files => false,
        }
    }

    pub(crate) fn open_selected_process_info_detail(&mut self) -> bool {
        match self.process_info_tab {
            ProcessInfoTab::Dlls if crate::ui::process_modules::selected_entry(self).is_some() => {
                self.process_modules_show_detail = true;
                self.process_info_dlls_scroll.scroll_home();
            }
            ProcessInfoTab::Environment
                if crate::ui::process_environment::selected_entry(self).is_some() =>
            {
                self.process_environment_show_detail = true;
                self.process_info_environment_scroll.scroll_home();
            }
            _ => return false,
        }
        let total = self.process_info_total_rows();
        let page_size = self.process_info_page_size();
        self.active_process_info_scroll_mut()
            .set_page_size(page_size, total);
        true
    }

    pub(crate) fn close_process_info_detail(&mut self) -> bool {
        match self.process_info_tab {
            ProcessInfoTab::Dlls if self.process_modules_show_detail => {
                self.process_modules_show_detail = false;
                self.process_info_dlls_scroll.scroll_home();
                self.ensure_selected_process_module_visible();
            }
            ProcessInfoTab::Environment if self.process_environment_show_detail => {
                self.process_environment_show_detail = false;
                self.process_info_environment_scroll.scroll_home();
                self.ensure_selected_process_environment_visible();
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn refresh_process_modules(&mut self) -> Result<()> {
        if self.process_modules_in_flight.is_some()
            && self.process_modules_in_flight_generation == Some(self.process_info_generation)
        {
            self.status = "DLL refresh already in progress".to_string();
            return Ok(());
        }
        self.request_process_modules_for_target(false, "Refreshing DLLs for")
    }

    fn ensure_process_modules_for_target(&mut self) -> Result<()> {
        let Some(target) = self.process_info_target.as_ref() else {
            return Ok(());
        };
        if self.process_modules_result_identity.as_ref() == Some(&target.identity)
            || (self.process_modules_in_flight.as_ref() == Some(&target.identity)
                && self.process_modules_in_flight_generation == Some(self.process_info_generation))
        {
            return Ok(());
        }
        self.request_process_modules_for_target(true, "Loading DLLs for")
    }

    fn request_process_modules_for_target(
        &mut self,
        clear_previous_result: bool,
        status_prefix: &str,
    ) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            self.status = "DLLs are unavailable in Log view".to_string();
            return Ok(());
        }
        let Some(target) = self.process_info_target.clone() else {
            self.status = "No Process Info target".to_string();
            return Ok(());
        };
        if !matches!(target.lifecycle, ProcessLifecycle::Live) {
            self.status = "Process has exited".to_string();
            return Ok(());
        }
        if !self.process_info_target_is_currently_live() {
            self.process_modules_error = Some(ProcessModulesError::ProcessExited);
            self.status = "Process has exited".to_string();
            return Ok(());
        }

        self.process_modules_next_request_id =
            self.process_modules_next_request_id.wrapping_add(1).max(1);
        let request_id = self.process_modules_next_request_id;
        self.process_modules_worker.request_modules(
            self.process_info_generation,
            request_id,
            target.identity.clone(),
            target.process.clone(),
        )?;
        if clear_previous_result {
            self.process_modules_result = None;
            self.process_modules_result_identity = None;
            self.process_modules_selected = 0;
            self.process_modules_show_detail = false;
            self.process_info_dlls_scroll.scroll_home();
        }
        self.process_modules_error = None;
        self.process_modules_in_flight = Some(target.identity);
        self.process_modules_in_flight_generation = Some(self.process_info_generation);
        self.process_modules_in_flight_request_id = Some(request_id);
        self.status = format!("{status_prefix} {}", target.process.name);
        Ok(())
    }

    pub(crate) fn process_modules_poll_timeout(&self) -> Option<Duration> {
        self.process_modules_in_flight
            .as_ref()
            .map(|_| PROCESS_MODULES_IN_FLIGHT_POLL_INTERVAL)
    }

    pub(crate) fn poll_process_modules_results(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.process_modules_worker.try_recv() {
                Ok(result) => changed |= self.apply_process_modules_result(result),
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => {
                    self.process_modules_in_flight = None;
                    self.process_modules_in_flight_generation = None;
                    self.process_modules_in_flight_request_id = None;
                    self.status = "Warning: process modules worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_process_modules_result(&mut self, result: ProcessModulesResult) -> bool {
        if self.process_modules_in_flight.as_ref() != Some(&result.identity)
            || self.process_modules_in_flight_generation != Some(result.generation)
            || self.process_modules_in_flight_request_id != Some(result.request_id)
        {
            return false;
        }
        self.process_modules_in_flight = None;
        self.process_modules_in_flight_generation = None;
        self.process_modules_in_flight_request_id = None;
        if !self.show_process_info_dialog
            || result.generation != self.process_info_generation
            || self
                .process_info_target
                .as_ref()
                .map(|target| &target.identity)
                != Some(&result.identity)
        {
            return false;
        }

        match result.outcome {
            Ok(report) => {
                let count = report.entries.len();
                let process_name = report.process_name.clone();
                self.process_modules_result_identity = Some(result.identity);
                self.process_modules_result = Some(report);
                self.process_modules_error = None;
                self.clamp_process_modules_selection();
                self.status = format!("Loaded {count} DLLs for {process_name}");
            }
            Err(error) => {
                self.process_modules_error = Some(error);
                self.status = format!(
                    "DLLs unavailable for {}: {}",
                    result.identity.name,
                    error.message()
                );
            }
        }
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_modules::process_modules_total_rows(self, width);
        self.process_info_dlls_scroll
            .set_page_size(self.process_info_dlls_scroll.page_size, total);
        true
    }

    pub(crate) fn move_process_modules_up(&mut self, amount: usize) {
        self.process_modules_selected = self.process_modules_selected.saturating_sub(amount);
        self.ensure_selected_process_module_visible();
    }

    pub(crate) fn move_process_modules_down(&mut self, amount: usize) {
        let count = crate::ui::process_modules::filtered_entries(self).len();
        self.process_modules_selected = self
            .process_modules_selected
            .saturating_add(amount)
            .min(count.saturating_sub(1));
        self.ensure_selected_process_module_visible();
    }

    pub(crate) fn move_process_modules_home(&mut self) {
        self.process_modules_selected = 0;
        self.ensure_selected_process_module_visible();
    }

    pub(crate) fn move_process_modules_end(&mut self) {
        self.process_modules_selected = crate::ui::process_modules::filtered_entries(self)
            .len()
            .saturating_sub(1);
        self.ensure_selected_process_module_visible();
    }

    pub(crate) fn select_process_module(&mut self, index: usize) {
        let count = crate::ui::process_modules::filtered_entries(self).len();
        if index < count {
            self.process_modules_selected = index;
            self.ensure_selected_process_module_visible();
        }
    }

    fn clamp_process_modules_selection(&mut self) {
        let count = crate::ui::process_modules::filtered_entries(self).len();
        self.process_modules_selected = self.process_modules_selected.min(count.saturating_sub(1));
        self.ensure_selected_process_module_visible();
    }

    fn ensure_selected_process_module_visible(&mut self) {
        let count = crate::ui::process_modules::filtered_entries(self).len();
        if count == 0 {
            self.process_modules_selected = 0;
            self.process_modules_show_detail = false;
            self.process_info_dlls_scroll.scroll_home();
            return;
        }
        if self.process_modules_show_detail {
            self.process_info_dlls_scroll.scroll_home();
            let width =
                crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
            let total = crate::ui::process_modules::process_modules_total_rows(self, width);
            self.process_info_dlls_scroll
                .set_page_size(self.process_info_dlls_scroll.page_size, total);
            return;
        }
        let prefix = 3 + usize::from(self.process_modules_error.is_some());
        let selected_line = prefix.saturating_add(self.process_modules_selected);
        let page_size = self.process_info_dlls_scroll.page_size.max(1);
        if selected_line < self.process_info_dlls_scroll.offset {
            self.process_info_dlls_scroll.offset = selected_line;
        } else if selected_line
            >= self
                .process_info_dlls_scroll
                .offset
                .saturating_add(page_size)
        {
            self.process_info_dlls_scroll.offset =
                selected_line.saturating_add(1).saturating_sub(page_size);
        }
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_modules::process_modules_total_rows(self, width);
        self.process_info_dlls_scroll
            .set_page_size(page_size, total);
    }

    pub(crate) fn push_process_modules_filter_char(&mut self, ch: char) {
        self.process_modules_filter_cursor = self
            .process_modules_filter_cursor
            .min(self.process_modules_filter.len());
        self.process_modules_filter
            .insert(self.process_modules_filter_cursor, ch);
        self.process_modules_filter_cursor += ch.len_utf8();
        self.reset_process_modules_filter_selection();
    }

    pub(crate) fn pop_process_modules_filter_char(&mut self) {
        if self.process_modules_filter_cursor > 0 {
            let previous = self.process_modules_filter[..self.process_modules_filter_cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.process_modules_filter
                .drain(previous..self.process_modules_filter_cursor);
            self.process_modules_filter_cursor = previous;
        }
        self.reset_process_modules_filter_selection();
    }

    pub(crate) fn delete_process_modules_filter_char(&mut self) {
        if self.process_modules_filter_cursor < self.process_modules_filter.len() {
            let next = self.process_modules_filter[self.process_modules_filter_cursor..]
                .chars()
                .next()
                .map(|ch| self.process_modules_filter_cursor + ch.len_utf8())
                .unwrap_or(self.process_modules_filter.len());
            self.process_modules_filter
                .drain(self.process_modules_filter_cursor..next);
        }
        self.reset_process_modules_filter_selection();
    }

    pub(crate) fn move_process_modules_filter_cursor_left(&mut self) {
        if self.process_modules_filter_cursor == 0 {
            return;
        }
        self.process_modules_filter_cursor = self.process_modules_filter
            [..self.process_modules_filter_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub(crate) fn move_process_modules_filter_cursor_right(&mut self) {
        if self.process_modules_filter_cursor >= self.process_modules_filter.len() {
            return;
        }
        self.process_modules_filter_cursor = self.process_modules_filter
            [self.process_modules_filter_cursor..]
            .chars()
            .next()
            .map(|ch| self.process_modules_filter_cursor + ch.len_utf8())
            .unwrap_or(self.process_modules_filter.len());
    }

    fn reset_process_modules_filter_selection(&mut self) {
        self.process_modules_selected = 0;
        self.process_info_dlls_scroll.scroll_home();
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_modules::process_modules_total_rows(self, width);
        self.process_info_dlls_scroll
            .set_page_size(self.process_info_dlls_scroll.page_size, total);
    }

    pub(crate) fn refresh_process_environment(&mut self) -> Result<()> {
        if self.process_environment_in_flight.is_some()
            && self.process_environment_in_flight_generation == Some(self.process_info_generation)
        {
            self.status = "Environment refresh already in progress".to_string();
            return Ok(());
        }
        self.request_process_environment_for_target(false, "Refreshing Environment for")
    }

    fn ensure_process_environment_for_target(&mut self) -> Result<()> {
        let Some(target) = self.process_info_target.as_ref() else {
            return Ok(());
        };
        if self.process_environment_result_identity.as_ref() == Some(&target.identity)
            || (self.process_environment_in_flight.as_ref() == Some(&target.identity)
                && self.process_environment_in_flight_generation
                    == Some(self.process_info_generation))
        {
            return Ok(());
        }
        self.request_process_environment_for_target(true, "Loading Environment for")
    }

    fn request_process_environment_for_target(
        &mut self,
        clear_previous_result: bool,
        status_prefix: &str,
    ) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            self.status = "Environment is unavailable in Log view".to_string();
            return Ok(());
        }
        let Some(target) = self.process_info_target.clone() else {
            self.status = "No Process Info target".to_string();
            return Ok(());
        };
        if !matches!(target.lifecycle, ProcessLifecycle::Live)
            || !self.process_info_target_is_currently_live()
        {
            self.process_environment_error = Some(ProcessEnvironmentError::ProcessExited);
            self.status = "Process has exited".to_string();
            return Ok(());
        }

        self.process_environment_next_request_id = self
            .process_environment_next_request_id
            .wrapping_add(1)
            .max(1);
        let request_id = self.process_environment_next_request_id;
        self.process_environment_worker.request_environment(
            self.process_info_generation,
            request_id,
            target.identity.clone(),
            target.process.clone(),
        )?;
        if clear_previous_result {
            self.process_environment_result = None;
            self.process_environment_result_identity = None;
            self.process_environment_selected = 0;
            self.process_environment_show_detail = false;
            self.process_info_environment_scroll.scroll_home();
        }
        self.process_environment_error = None;
        self.process_environment_in_flight = Some(target.identity);
        self.process_environment_in_flight_generation = Some(self.process_info_generation);
        self.process_environment_in_flight_request_id = Some(request_id);
        self.status = format!("{status_prefix} {}", target.process.name);
        Ok(())
    }

    pub(crate) fn process_environment_poll_timeout(&self) -> Option<Duration> {
        self.process_environment_in_flight
            .as_ref()
            .map(|_| PROCESS_ENVIRONMENT_IN_FLIGHT_POLL_INTERVAL)
    }

    pub(crate) fn poll_process_environment_results(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.process_environment_worker.try_recv() {
                Ok(result) => changed |= self.apply_process_environment_result(result),
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => {
                    self.process_environment_in_flight = None;
                    self.process_environment_in_flight_generation = None;
                    self.process_environment_in_flight_request_id = None;
                    self.status = "Warning: process environment worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_process_environment_result(&mut self, result: ProcessEnvironmentResult) -> bool {
        if self.process_environment_in_flight.as_ref() != Some(&result.identity)
            || self.process_environment_in_flight_generation != Some(result.generation)
            || self.process_environment_in_flight_request_id != Some(result.request_id)
        {
            return false;
        }
        self.process_environment_in_flight = None;
        self.process_environment_in_flight_generation = None;
        self.process_environment_in_flight_request_id = None;
        if !self.show_process_info_dialog
            || result.generation != self.process_info_generation
            || self
                .process_info_target
                .as_ref()
                .map(|target| &target.identity)
                != Some(&result.identity)
        {
            return false;
        }

        match result.outcome {
            Ok(report) => {
                let count = report.entries.len();
                let process_name = report.process_name.clone();
                self.process_environment_result_identity = Some(result.identity);
                self.process_environment_result = Some(report);
                self.process_environment_error = None;
                self.clamp_process_environment_selection();
                self.status = format!("Loaded {count} environment variables for {process_name}");
            }
            Err(error) => {
                self.process_environment_error = Some(error);
                self.status = format!(
                    "Environment unavailable for {}: {}",
                    result.identity.name,
                    error.message()
                );
            }
        }
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_environment::process_environment_total_rows(self, width);
        self.process_info_environment_scroll
            .set_page_size(self.process_info_environment_scroll.page_size, total);
        true
    }

    pub(crate) fn move_process_environment_up(&mut self, amount: usize) {
        self.process_environment_selected =
            self.process_environment_selected.saturating_sub(amount);
        self.ensure_selected_process_environment_visible();
    }

    pub(crate) fn move_process_environment_down(&mut self, amount: usize) {
        let count = crate::ui::process_environment::filtered_entries(self).len();
        self.process_environment_selected = self
            .process_environment_selected
            .saturating_add(amount)
            .min(count.saturating_sub(1));
        self.ensure_selected_process_environment_visible();
    }

    pub(crate) fn move_process_environment_home(&mut self) {
        self.process_environment_selected = 0;
        self.ensure_selected_process_environment_visible();
    }

    pub(crate) fn move_process_environment_end(&mut self) {
        self.process_environment_selected = crate::ui::process_environment::filtered_entries(self)
            .len()
            .saturating_sub(1);
        self.ensure_selected_process_environment_visible();
    }

    pub(crate) fn select_process_environment(&mut self, index: usize) {
        let count = crate::ui::process_environment::filtered_entries(self).len();
        if index < count {
            self.process_environment_selected = index;
            self.ensure_selected_process_environment_visible();
        }
    }

    fn clamp_process_environment_selection(&mut self) {
        let count = crate::ui::process_environment::filtered_entries(self).len();
        self.process_environment_selected = self
            .process_environment_selected
            .min(count.saturating_sub(1));
        self.ensure_selected_process_environment_visible();
    }

    fn ensure_selected_process_environment_visible(&mut self) {
        let count = crate::ui::process_environment::filtered_entries(self).len();
        if count == 0 {
            self.process_environment_selected = 0;
            self.process_environment_show_detail = false;
            self.process_info_environment_scroll.scroll_home();
            return;
        }
        if self.process_environment_show_detail {
            self.process_info_environment_scroll.scroll_home();
            let width =
                crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
            let total = crate::ui::process_environment::process_environment_total_rows(self, width);
            self.process_info_environment_scroll
                .set_page_size(self.process_info_environment_scroll.page_size, total);
            return;
        }
        let prefix = 4
            + usize::from(self.process_environment_error.is_some())
            + self
                .process_environment_result
                .as_ref()
                .map(|report| usize::from(report.malformed_entries > 0))
                .unwrap_or(0);
        let selected_line = prefix.saturating_add(self.process_environment_selected);
        let page_size = self.process_info_environment_scroll.page_size.max(1);
        if selected_line < self.process_info_environment_scroll.offset {
            self.process_info_environment_scroll.offset = selected_line;
        } else if selected_line
            >= self
                .process_info_environment_scroll
                .offset
                .saturating_add(page_size)
        {
            self.process_info_environment_scroll.offset =
                selected_line.saturating_add(1).saturating_sub(page_size);
        }
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_environment::process_environment_total_rows(self, width);
        self.process_info_environment_scroll
            .set_page_size(page_size, total);
    }

    pub(crate) fn push_process_environment_filter_char(&mut self, ch: char) {
        self.process_environment_filter_cursor = self
            .process_environment_filter_cursor
            .min(self.process_environment_filter.len());
        self.process_environment_filter
            .insert(self.process_environment_filter_cursor, ch);
        self.process_environment_filter_cursor += ch.len_utf8();
        self.reset_process_environment_filter_selection();
    }

    pub(crate) fn pop_process_environment_filter_char(&mut self) {
        if self.process_environment_filter_cursor > 0 {
            let previous = self.process_environment_filter
                [..self.process_environment_filter_cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.process_environment_filter
                .drain(previous..self.process_environment_filter_cursor);
            self.process_environment_filter_cursor = previous;
        }
        self.reset_process_environment_filter_selection();
    }

    pub(crate) fn delete_process_environment_filter_char(&mut self) {
        if self.process_environment_filter_cursor < self.process_environment_filter.len() {
            let next = self.process_environment_filter[self.process_environment_filter_cursor..]
                .chars()
                .next()
                .map(|ch| self.process_environment_filter_cursor + ch.len_utf8())
                .unwrap_or(self.process_environment_filter.len());
            self.process_environment_filter
                .drain(self.process_environment_filter_cursor..next);
        }
        self.reset_process_environment_filter_selection();
    }

    pub(crate) fn move_process_environment_filter_cursor_left(&mut self) {
        if self.process_environment_filter_cursor == 0 {
            return;
        }
        self.process_environment_filter_cursor = self.process_environment_filter
            [..self.process_environment_filter_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub(crate) fn move_process_environment_filter_cursor_right(&mut self) {
        if self.process_environment_filter_cursor >= self.process_environment_filter.len() {
            return;
        }
        self.process_environment_filter_cursor = self.process_environment_filter
            [self.process_environment_filter_cursor..]
            .chars()
            .next()
            .map(|ch| self.process_environment_filter_cursor + ch.len_utf8())
            .unwrap_or(self.process_environment_filter.len());
    }

    fn reset_process_environment_filter_selection(&mut self) {
        self.process_environment_selected = 0;
        self.process_info_environment_scroll.scroll_home();
        let width = crate::ui::process_info_content_area_for_screen(self.last_screen_area).width;
        let total = crate::ui::process_environment::process_environment_total_rows(self, width);
        self.process_info_environment_scroll
            .set_page_size(self.process_info_environment_scroll.page_size, total);
    }

    pub(crate) fn request_quit_confirmation(&mut self) {
        self.show_quit_confirmation = true;
        self.quit_confirm_selection = QuitConfirmSelection::Cancel;
        self.status = if self.recording_session.is_some() {
            "Recording is active. Stop recording and quit?".to_string()
        } else {
            "Quit? Enter activates selected button, Esc cancels".to_string()
        };
    }

    pub(crate) fn confirm_quit(&mut self) -> Result<()> {
        if self.recording_session.is_some() {
            let path = self
                .recording_session
                .as_ref()
                .expect("recording session exists")
                .path
                .clone();
            if let Err(error) = self.stop_recording() {
                self.show_quit_confirmation = false;
                self.present_active_recording_error(path, error);
                return Ok(());
            }
        }
        self.should_quit = true;
        self.show_quit_confirmation = false;
        Ok(())
    }

    pub(crate) fn cancel_quit_confirmation(&mut self) {
        self.show_quit_confirmation = false;
        self.ensure_visible_panel_focus();
        self.status = "Quit canceled".to_string();
    }

    pub(crate) fn select_next_quit_action(&mut self) {
        self.quit_confirm_selection = self.quit_confirm_selection.toggled();
    }

    pub(crate) fn select_previous_quit_action(&mut self) {
        self.quit_confirm_selection = self.quit_confirm_selection.toggled();
    }

    pub(crate) fn activate_quit_selection(&mut self) -> Result<()> {
        match self.quit_confirm_selection {
            QuitConfirmSelection::Quit => self.confirm_quit(),
            QuitConfirmSelection::Cancel => {
                self.cancel_quit_confirmation();
                Ok(())
            }
        }
    }

    pub(crate) fn open_help(&mut self) {
        self.show_help = true;
        self.help_scroll.reset();
    }

    pub(crate) fn close_help(&mut self) {
        self.show_help = false;
        self.help_scroll.reset();
        self.ensure_visible_panel_focus();
        self.status = "Help closed".to_string();
    }

    pub(crate) fn toggle_help(&mut self) {
        if self.show_help {
            self.close_help();
        } else {
            self.open_help();
        }
    }

    pub(crate) fn set_help_page_size(&mut self, page_size: usize) {
        let page_size = page_size.max(1);
        let total = page_size.saturating_add(help_scroll_max_for_page_size(page_size));
        self.help_scroll.set_page_size(page_size, total);
    }

    pub(crate) fn scroll_help_up(&mut self, amount: usize) {
        self.help_scroll.scroll_up(amount);
    }

    pub(crate) fn scroll_help_down(&mut self, amount: usize) {
        let total = self.help_scroll_total();
        self.help_scroll.scroll_down(amount, total);
    }

    pub(crate) fn scroll_help_home(&mut self) {
        self.help_scroll.scroll_home();
    }

    pub(crate) fn scroll_help_end(&mut self) {
        let total = self.help_scroll_total();
        self.help_scroll.scroll_end(total);
    }

    pub(crate) fn help_scroll_total(&self) -> usize {
        let page_size = self.help_scroll.page_size.max(1);
        page_size.saturating_add(help_scroll_max_for_page_size(page_size))
    }

    pub(crate) fn open_column_picker(&mut self) {
        self.show_column_picker = true;
        self.column_picker_scroll.reset();
        self.column_picker_index = self
            .process_columns
            .first()
            .and_then(|column| MetricColumn::ALL.iter().position(|item| item == column))
            .unwrap_or(0);
        self.ensure_column_picker_selection_visible();
        self.status = "Column picker opened".to_string();
    }

    pub(crate) fn close_column_picker(&mut self) {
        self.show_column_picker = false;
        self.column_picker_scroll.stop_drag();
        self.clamp_process_table_state();
        self.ensure_visible_panel_focus();
        self.status = format!("Columns: {} selected", self.process_columns.len());
    }

    pub(crate) fn set_column_picker_page_size(&mut self, page_size: usize) {
        let page_size = page_size.max(1);
        let total = page_size.saturating_add(column_picker_scroll_max_for_page_size(page_size));
        self.column_picker_scroll.set_page_size(page_size, total);
        self.ensure_column_picker_selection_visible();
    }

    pub(crate) fn scroll_column_picker_up(&mut self, amount: usize) {
        self.column_picker_scroll.scroll_up(amount);
    }

    pub(crate) fn scroll_column_picker_down(&mut self, amount: usize) {
        let total = self.column_picker_scroll_total();
        self.column_picker_scroll.scroll_down(amount, total);
    }

    pub(crate) fn column_picker_scroll_total(&self) -> usize {
        let page_size = self.column_picker_scroll.page_size.max(1);
        page_size.saturating_add(column_picker_scroll_max_for_page_size(page_size))
    }

    pub(crate) fn move_column_picker_up(&mut self) {
        self.move_column_picker_up_by(1);
    }

    pub(crate) fn move_column_picker_up_by(&mut self, amount: usize) {
        self.column_picker_index = self.column_picker_index.saturating_sub(amount);
        self.ensure_column_picker_selection_visible();
    }

    pub(crate) fn move_column_picker_down(&mut self) {
        self.move_column_picker_down_by(1);
    }

    pub(crate) fn move_column_picker_down_by(&mut self, amount: usize) {
        self.column_picker_index = self
            .column_picker_index
            .saturating_add(amount)
            .min(MetricColumn::ALL.len().saturating_sub(1));
        self.ensure_column_picker_selection_visible();
    }

    pub(crate) fn move_column_picker_home(&mut self) {
        self.column_picker_index = 0;
        self.ensure_column_picker_selection_visible();
    }

    pub(crate) fn move_column_picker_end(&mut self) {
        self.column_picker_index = MetricColumn::ALL.len().saturating_sub(1);
        self.ensure_column_picker_selection_visible();
    }

    pub(crate) fn toggle_picker_column(&mut self) {
        let column = MetricColumn::ALL[self.column_picker_index];
        if let Some(index) = self
            .process_columns
            .iter()
            .position(|existing| *existing == column)
        {
            if self.process_columns.len() > 1 {
                self.process_columns.remove(index);
            }
        } else {
            self.process_columns.push(column);
        }

        self.column_preset = ColumnPreset::Custom;
        self.clamp_selected_process_column();
        self.ensure_sort_column_visible();
        self.refresh_process_order();
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
    }

    pub(crate) fn toggle_picker_column_at(&mut self, index: usize) {
        self.column_picker_index = index.min(MetricColumn::ALL.len().saturating_sub(1));
        self.toggle_picker_column();
    }

    pub(crate) fn open_log_list(&mut self) -> Result<()> {
        if self.recording_session.is_some() {
            self.status = "Log view is unavailable during recording".to_string();
            return Ok(());
        }
        self.show_log_list = true;
        self.log_list_focus = LogListFocus::List;
        self.show_log_dir_dialog = false;
        self.log_list_dir = Some(self.default_log_list_dir()?);
        self.log_list_index = self
            .log_list_index
            .min(self.log_summaries.len().saturating_sub(1));
        self.refresh_log_list()
    }

    pub(crate) fn close_log_list(&mut self) {
        self.show_log_list = false;
        self.show_log_dir_dialog = false;
        self.log_list_last_click = None;
        self.log_list_focus = LogListFocus::List;
        self.log_list_scroll.stop_drag();
        if self.activity() == AppActivity::LogView {
            self.exit_log_view();
        } else {
            self.ensure_visible_panel_focus();
            self.status = "Log list closed".to_string();
        }
    }

    pub(crate) fn refresh_log_list(&mut self) -> Result<()> {
        let dir = self
            .log_list_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.default_log_list_dir())?;
        self.log_list_dir = Some(dir.clone());
        self.log_summaries.clear();
        self.log_list_index = 0;
        self.log_list_scroll
            .set_page_size(self.log_list_scroll.page_size, self.log_list_total_rows());
        self.log_list_worker = Some(LogListWorker::spawn(dir.clone()));
        self.log_list_last_click = None;
        self.status = format!("Loading logs from {}", dir.display());
        Ok(())
    }

    fn default_log_list_dir(&self) -> Result<PathBuf> {
        self.recording_last_dir.clone().map(Ok).unwrap_or_else(|| {
            std::env::current_dir().context("failed to resolve current directory")
        })
    }

    pub(crate) fn open_log_dir_dialog(&mut self) -> Result<()> {
        let dir = self
            .log_list_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.default_log_list_dir())?;
        self.log_dir_draft = dir.display().to_string();
        self.log_dir_cursor = self.log_dir_draft.len();
        self.log_dir_completion.reset();
        self.log_dir_selection = LogDirSelection::Path;
        self.log_dir_error = None;
        self.show_log_dir_dialog = true;
        self.status = "Edit log directory".to_string();
        Ok(())
    }

    pub(crate) fn cancel_log_dir_dialog(&mut self) {
        self.show_log_dir_dialog = false;
        self.log_dir_error = None;
        self.log_dir_completion.reset();
        self.status = "Log directory unchanged".to_string();
    }

    pub(crate) fn activate_log_dir_selection(&mut self) -> Result<()> {
        match self.log_dir_selection {
            LogDirSelection::Path | LogDirSelection::Apply => self.confirm_log_dir(),
            LogDirSelection::Cancel => {
                self.cancel_log_dir_dialog();
                Ok(())
            }
        }
    }

    pub(crate) fn focus_next_log_dir_control(&mut self) {
        self.log_dir_selection = self.log_dir_selection.next();
    }

    pub(crate) fn focus_previous_log_dir_control(&mut self) {
        self.log_dir_selection = self.log_dir_selection.previous();
    }

    pub(crate) fn focus_next_log_list_control(&mut self) {
        self.log_list_focus = self.log_list_focus.next();
    }

    pub(crate) fn focus_previous_log_list_control(&mut self) {
        self.log_list_focus = self.log_list_focus.previous();
    }

    pub(crate) fn activate_log_list_control(&mut self) -> Result<()> {
        match self.log_list_focus {
            LogListFocus::List | LogListFocus::Open => self.load_selected_log(),
            LogListFocus::Directory => self.open_log_dir_dialog()?,
            LogListFocus::Refresh => self.refresh_log_list()?,
            LogListFocus::Close => self.close_log_list(),
        }
        Ok(())
    }

    pub(crate) fn confirm_log_dir(&mut self) -> Result<()> {
        let draft = self.log_dir_draft.trim();
        if draft.is_empty() {
            self.log_dir_error = Some("Directory is empty.".to_string());
            self.status = "Log directory is empty".to_string();
            return Ok(());
        }
        let dir = PathBuf::from(draft);
        if !dir.exists() {
            self.log_dir_error = Some("Directory does not exist.".to_string());
            self.status = format!("Log directory does not exist: {}", dir.display());
            return Ok(());
        }
        if !dir.is_dir() {
            self.log_dir_error = Some("Path is not a directory.".to_string());
            self.status = format!("Log path is not a directory: {}", dir.display());
            return Ok(());
        }
        self.show_log_dir_dialog = false;
        self.log_dir_error = None;
        self.log_dir_completion.reset();
        self.log_list_dir = Some(dir);
        self.refresh_log_list()
    }

    pub(crate) fn push_log_dir_char(&mut self, ch: char) {
        self.log_dir_error = None;
        self.log_dir_cursor = self.log_dir_cursor.min(self.log_dir_draft.len());
        self.log_dir_draft.insert(self.log_dir_cursor, ch);
        self.log_dir_cursor += ch.len_utf8();
    }

    pub(crate) fn pop_log_dir_char(&mut self) {
        if self.log_dir_cursor == 0 {
            return;
        }
        self.log_dir_error = None;
        let prev = self.log_dir_draft[..self.log_dir_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.log_dir_draft.drain(prev..self.log_dir_cursor);
        self.log_dir_cursor = prev;
    }

    pub(crate) fn delete_log_dir_char(&mut self) {
        if self.log_dir_cursor >= self.log_dir_draft.len() {
            return;
        }
        self.log_dir_error = None;
        let next = self.log_dir_draft[self.log_dir_cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.log_dir_cursor + index)
            .unwrap_or(self.log_dir_draft.len());
        self.log_dir_draft.drain(self.log_dir_cursor..next);
    }

    pub(crate) fn complete_log_dir(&mut self) {
        self.log_dir_error = None;
        match self
            .log_dir_completion
            .complete_directory_path(&self.log_dir_draft, self.log_dir_cursor)
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
                self.log_dir_draft = value;
                self.log_dir_cursor = cursor;
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

    pub(crate) fn move_log_dir_cursor_left(&mut self) {
        if self.log_dir_cursor == 0 {
            return;
        }
        self.log_dir_cursor = self.log_dir_draft[..self.log_dir_cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub(crate) fn move_log_dir_cursor_right(&mut self) {
        if self.log_dir_cursor >= self.log_dir_draft.len() {
            return;
        }
        self.log_dir_cursor = self.log_dir_draft[self.log_dir_cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.log_dir_cursor + index)
            .unwrap_or(self.log_dir_draft.len());
    }

    pub(crate) fn move_log_dir_cursor_home(&mut self) {
        self.log_dir_cursor = 0;
    }

    pub(crate) fn move_log_dir_cursor_end(&mut self) {
        self.log_dir_cursor = self.log_dir_draft.len();
    }

    pub(crate) fn select_log_list_index(&mut self, index: usize) {
        self.log_list_index = index.min(self.log_summaries.len().saturating_sub(1));
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn click_log_list_index(&mut self, index: usize, now: Instant) {
        let is_double_click = self.log_list_last_click.is_some_and(|last| {
            last.index == index && now.duration_since(last.at) <= Duration::from_millis(500)
        });
        self.select_log_list_index(index);
        if is_double_click {
            self.log_list_last_click = None;
            self.load_selected_log();
        } else {
            self.log_list_last_click = Some(LogListClick { index, at: now });
        }
    }

    pub(crate) fn move_log_list_up(&mut self, amount: usize) {
        self.log_list_index = self.log_list_index.saturating_sub(amount);
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn move_log_list_down(&mut self, amount: usize) {
        self.log_list_index = self
            .log_list_index
            .saturating_add(amount)
            .min(self.log_summaries.len().saturating_sub(1));
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn move_log_list_home(&mut self) {
        self.log_list_index = 0;
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn move_log_list_end(&mut self) {
        self.log_list_index = self.log_summaries.len().saturating_sub(1);
        self.ensure_log_list_selection_visible();
    }

    pub(crate) fn scroll_log_list_up(&mut self, amount: usize) {
        self.log_list_scroll.scroll_up(amount);
        self.log_list_index = self
            .log_list_index
            .min(self.log_summaries.len().saturating_sub(1));
    }

    pub(crate) fn scroll_log_list_down(&mut self, amount: usize) {
        self.log_list_scroll
            .scroll_down(amount, self.log_list_total_rows());
    }

    pub(crate) fn load_selected_log(&mut self) {
        if self.recording_session.is_some() {
            self.status = "Log view is unavailable during recording".to_string();
            return;
        }
        let Some(summary) = self.log_summaries.get(self.log_list_index) else {
            self.status = "No log selected".to_string();
            return;
        };
        if let Some(error) = &summary.error {
            self.status = format!("Cannot open log: {error}");
            return;
        }
        let path = summary.path.clone();
        self.log_list_last_click = None;
        self.log_load_worker = Some(LogLoadWorker::spawn(path.clone(), self.sort));
        self.status = format!("Opening log: {}", path.display());
    }

    pub(crate) fn poll_log_workers(&mut self) -> bool {
        let mut changed = false;
        if let Some(worker) = &self.log_list_worker {
            match worker.try_recv() {
                Ok(Some(result)) => {
                    self.apply_log_list_result(result);
                    self.log_list_worker = None;
                    changed = true;
                }
                Ok(None) => {}
                Err(_) => {
                    self.log_list_worker = None;
                    self.status = "Log list worker stopped".to_string();
                    changed = true;
                }
            }
        }
        if let Some(worker) = &self.log_load_worker {
            match worker.try_recv() {
                Ok(Some(Ok(loaded))) => {
                    self.apply_loaded_log(loaded);
                    self.log_load_worker = None;
                    changed = true;
                }
                Ok(Some(Err(error))) => {
                    self.status = format!("Failed to open log: {error}");
                    self.log_load_worker = None;
                    changed = true;
                }
                Ok(None) => {}
                Err(_) => {
                    self.log_load_worker = None;
                    self.status = "Log load worker stopped".to_string();
                    changed = true;
                }
            }
        }
        changed
    }

    fn apply_log_list_result(&mut self, result: LogListResult) {
        if self
            .log_list_dir
            .as_ref()
            .is_some_and(|dir| dir != &result.dir)
        {
            return;
        }
        self.log_list_dir = Some(result.dir);
        self.log_summaries = result.summaries;
        self.log_list_index = self
            .log_list_index
            .min(self.log_summaries.len().saturating_sub(1));
        self.log_list_scroll
            .set_page_size(self.log_list_scroll.page_size, self.log_list_total_rows());
        self.ensure_log_list_selection_visible();
        self.status = result.error.unwrap_or_else(|| {
            format!(
                "Loaded {} log{}",
                self.log_summaries.len(),
                if self.log_summaries.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        });
    }

    pub(crate) fn apply_loaded_log(&mut self, loaded: LoadedLog) {
        if self.recording_session.is_some() {
            self.status = "Log view is unavailable during recording".to_string();
            return;
        }
        self.log_view_path = Some(loaded.path.clone());
        self.log_view_watch_list = loaded.tracked_names.clone();
        self.log_view_normalized_watch_names = normalized_process_names(&self.log_view_watch_list);
        self.log_view_display = Some(PausedDisplay {
            snapshot: loaded.snapshot,
            exited_tracked_rows: HashMap::new(),
            process_history: loaded.process_history,
            system_history: loaded.system_history,
            process_info_cache: HashMap::new(),
            process_info_display_identity: None,
        });
        self.show_log_list = false;
        self.paused_display = None;
        self.clear_graph_workspace_state();
        self.process_table_state.select(Some(0));
        self.selected_process_identity = None;
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.selected_process_identity = self
            .process_table_state
            .selected()
            .and_then(|index| self.visible_process_identity_at(index));
        self.status = format!(
            "Opened log: {} ({} frames)",
            loaded.path.display(),
            loaded.summary.frame_count
        );
    }

    pub(crate) fn exit_log_view(&mut self) {
        self.log_view_path = None;
        self.log_view_display = None;
        self.log_view_watch_list.clear();
        self.log_view_normalized_watch_names.clear();
        self.clear_graph_workspace_state();
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = "Log view closed".to_string();
    }

    pub(crate) fn log_list_total_rows(&self) -> usize {
        log_list_total_rows_for_count(self.log_summaries.len())
    }

    fn ensure_log_list_selection_visible(&mut self) {
        let row = 1usize.saturating_add(self.log_list_index);
        self.log_list_scroll
            .ensure_visible(row, self.log_list_total_rows());
    }

    fn ensure_column_picker_selection_visible(&mut self) {
        let row = column_picker_row_for_index(self.column_picker_index);
        let total = self.column_picker_scroll_total();
        self.column_picker_scroll.ensure_visible(row, total);
    }

    pub(crate) fn cycle_sort_column(&mut self) {
        self.clear_process_order_hold();
        let selected_column = self.selected_process_column();
        if self.sort.column == selected_column {
            self.sort.direction = self.sort.direction.toggled();
        } else {
            self.sort = SortSpec {
                column: selected_column,
                direction: selected_column.default_direction(),
            };
        }

        self.apply_process_sort();
        self.clamp_process_table_state();
        self.status = format!(
            "Sorted by {} {}",
            self.sort.column.label(),
            match self.sort.direction {
                SortDirection::Asc => "asc",
                SortDirection::Desc => "desc",
            }
        );
    }

    pub(crate) fn toggle_display_pause(&mut self) {
        if self.activity() == AppActivity::LogView {
            self.status = "Display pause is unavailable in Log view".to_string();
            return;
        }
        if self.paused_display.is_some() {
            self.paused_display = None;
            self.rebuild_visible_process_cache();
            self.clamp_process_table_state();
            self.status = "Display resumed".to_string();
            return;
        }

        self.paused_display = Some(PausedDisplay {
            snapshot: self.snapshot.clone(),
            exited_tracked_rows: self.exited_tracked_rows.clone(),
            process_history: self.process_history.clone(),
            system_history: self.system_history.clone(),
            process_info_cache: self.process_info_cache.clone(),
            process_info_display_identity: self.process_info_display_identity.clone(),
        });
        self.rebuild_visible_process_cache();
        self.clamp_process_table_state();
        self.status = "Display paused".to_string();
    }

    pub(crate) fn request_sample(&mut self) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            return Ok(());
        }
        if self.sampling_in_progress {
            return Ok(());
        }

        self.sampling_worker.request_sample()?;
        self.sampling_in_progress = true;
        if self.recording_session.is_some() {
            self.recording_spinner_index = self.recording_spinner_index.wrapping_add(1);
        }
        Ok(())
    }

    pub(crate) fn poll_sample_results(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.sampling_worker.try_recv() {
                Ok(collected) => {
                    changed |= self.apply_sample_result(collected)?;
                }
                Err(TryRecvError::Empty) => return Ok(changed),
                Err(TryRecvError::Disconnected) => {
                    self.sampling_in_progress = false;
                    self.status = "Warning: sampling worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_sample_result(&mut self, collected: CollectSnapshotResult) -> Result<bool> {
        self.sampling_in_progress = false;
        if self.activity() == AppActivity::LogView {
            return Ok(false);
        }
        if self.details_live && !self.graph_visible_range_includes_latest_sample() {
            self.freeze_graph_time_window();
        }
        let next_tracked_live_identities =
            tracked_live_identities(&collected.snapshot.processes, &self.normalized_watch_names);
        self.record_exited_tracked_rows(
            &next_tracked_live_identities,
            collected.snapshot.captured_at,
        );
        self.last_tracked_live_identities = next_tracked_live_identities;
        let mut next_snapshot = collected.snapshot;
        if self.process_order_hold_active() {
            preserve_process_row_order(
                &mut next_snapshot.processes,
                &self.snapshot.processes,
                self.sort,
            );
        } else {
            self.clear_process_order_hold();
            sort_process_rows(&mut next_snapshot.processes, self.sort);
        }
        self.snapshot = next_snapshot;
        self.process_history.record_snapshot(
            self.snapshot.captured_at,
            &self.snapshot.processes,
            &self.normalized_watch_names,
        );
        self.system_history.record_snapshot(&self.snapshot);
        if !self.is_display_paused() {
            self.rebuild_visible_process_cache();
            self.clamp_details_sample_selection();
            if self.details_live {
                self.stop_graph_live_scroll_if_latest_sample_is_outside_visible_range();
            }
            if self.details_live {
                self.graph_time_offset_seconds = 0;
                self.graph_time_window_right_at = None;
                self.details_sample_selected = self.selected_sample_count().saturating_sub(1);
                self.scroll_details_samples_to_latest();
            } else if !self.graph_show_all_samples {
                self.restore_frozen_graph_time_window();
            }
            self.clamp_process_table_state();
            self.refresh_selected_process_info();
        }

        let mut status_parts = Vec::new();
        if let Some(warning) = collected.warning {
            status_parts.push(warning);
        }

        let mut recording_stopped = false;
        if self.recording_session.is_some() {
            match self.write_current_recording_frame() {
                Ok(()) => {}
                Err(error) => {
                    let path = self
                        .recording_session
                        .as_ref()
                        .expect("recording session exists")
                        .path
                        .clone();
                    self.recording_session = None;
                    self.present_active_recording_error(path, error);
                    recording_stopped = true;
                }
            }
        }

        if (!self.is_display_paused() || recording_stopped) && !status_parts.is_empty() {
            self.status = status_parts.join(" | ");
        }
        Ok(!self.is_display_paused() || recording_stopped)
    }

    fn apply_process_sort(&mut self) {
        sort_process_rows(&mut self.snapshot.processes, self.sort);
        if let Some(display) = self.paused_display.as_mut() {
            sort_process_rows(&mut display.snapshot.processes, self.sort);
        }
        self.rebuild_visible_process_cache();
    }

    fn refresh_process_order(&mut self) {
        self.apply_process_sort();
    }

    fn record_exited_tracked_rows(
        &mut self,
        next_tracked_live_identities: &HashSet<ProcessIdentity>,
        exited_at: DateTime<Local>,
    ) {
        let exited_identities = self
            .last_tracked_live_identities
            .difference(next_tracked_live_identities)
            .cloned()
            .collect::<Vec<_>>();

        for identity in exited_identities {
            if !self.is_tracked_process_name(&identity.name) {
                continue;
            }
            let Some(process) = self
                .snapshot
                .processes
                .iter()
                .find(|process| ProcessIdentity::from_row(process) == identity)
                .cloned()
            else {
                continue;
            };
            self.exited_tracked_rows
                .insert(identity, ExitedTrackedRow { process, exited_at });
        }
    }

    fn refresh_tracked_live_identities(&mut self) {
        self.last_tracked_live_identities =
            tracked_live_identities(&self.snapshot.processes, &self.normalized_watch_names);
    }

    fn ensure_sort_column_visible(&mut self) {
        if !matches!(
            self.sort.column,
            SortColumn::Metric(column) if !self.process_columns.contains(&column)
        ) {
            return;
        }
        self.sort.column = self
            .process_columns
            .first()
            .copied()
            .map(SortColumn::Metric)
            .unwrap_or(SortColumn::ProcessName);
    }

    fn clamp_selected_process_column(&mut self) {
        self.selected_process_column_index = self
            .selected_process_column_index
            .min(self.process_column_count().saturating_sub(1));
        self.ensure_selected_process_column_visible();
        self.apply_selected_process_column_to_details_metric();
    }

    fn apply_selected_process_column_to_details_metric(&mut self) {
        let Some(column) = self.selected_process_metric_column() else {
            return;
        };
        let Some(next_metric) = DetailsMetric::from_graphable_column(column) else {
            return;
        };
        if self.details_metric != next_metric {
            self.details_metric = next_metric;
            self.clear_ab_comparison();
        }
    }
}

fn process_column_index_for_sort(sort_column: SortColumn, columns: &[MetricColumn]) -> usize {
    match sort_column {
        SortColumn::Pid => 0,
        SortColumn::ProcessName => 1,
        SortColumn::Metric(column) => columns
            .iter()
            .position(|candidate| *candidate == column)
            .map(|index| index + FIXED_PROCESS_COLUMN_COUNT)
            .unwrap_or(FIXED_PROCESS_COLUMN_COUNT),
    }
}

fn process_matches_filter(process: &ProcessRow, filter: &str, include_path: bool) -> bool {
    process.name.to_ascii_lowercase().contains(filter)
        || include_path
            && process
                .executable_path
                .as_deref()
                .is_some_and(|path| path.to_ascii_lowercase().contains(filter))
}

fn process_sample_metric_value(
    sample: &crate::model::history::ProcessSample,
    metric: DetailsMetric,
) -> Option<f64> {
    match metric {
        DetailsMetric::CpuPercent => sample.cpu_percent,
        DetailsMetric::GpuPercent => sample.gpu_percent,
        DetailsMetric::Private => sample.private_bytes.map(|value| value as f64),
        DetailsMetric::Workset => sample.workset_bytes.map(|value| value as f64),
        DetailsMetric::WorksetPrivate => sample.workset_private_bytes.map(|value| value as f64),
        DetailsMetric::WorksetShareable => sample.workset_shareable_bytes.map(|value| value as f64),
        DetailsMetric::WorksetShared => sample.workset_shared_bytes.map(|value| value as f64),
        DetailsMetric::ThreadCount => sample.thread_count.map(|value| value as f64),
        DetailsMetric::HandleCount => sample.handle_count.map(|value| value as f64),
        DetailsMetric::UserObjectCount => sample.user_object_count.map(|value| value as f64),
        DetailsMetric::GdiObjectCount => sample.gdi_object_count.map(|value| value as f64),
        DetailsMetric::DotNetHeap => sample.dotnet_heap_bytes.map(|value| value as f64),
        DetailsMetric::GpuDedicated => sample.gpu_dedicated_bytes.map(|value| value as f64),
        DetailsMetric::GpuShared => sample.gpu_shared_bytes.map(|value| value as f64),
        DetailsMetric::IoRead => sample.io_read_bytes_per_sec.map(|value| value as f64),
        DetailsMetric::IoWrite => sample.io_write_bytes_per_sec.map(|value| value as f64),
    }
}

fn process_peak_metric_value(
    peak: &crate::model::history::ProcessPeak,
    metric: DetailsMetric,
) -> Option<u64> {
    match metric {
        DetailsMetric::Private => peak.private_bytes,
        DetailsMetric::WorksetPrivate => peak.workset_private_bytes,
        _ => None,
    }
}

fn dedupe_process_names(names: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if !deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(name))
        {
            deduped.push(name.to_string());
        }
    }
    deduped
}

fn normalized_process_names(names: &[String]) -> HashSet<String> {
    names
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn preserve_process_row_order(
    rows: &mut [ProcessRow],
    previous_rows: &[ProcessRow],
    sort: SortSpec,
) {
    sort_process_rows(rows, sort);
    let previous_positions = previous_rows
        .iter()
        .enumerate()
        .map(|(index, process)| (ProcessIdentity::from_row(process), index))
        .collect::<HashMap<_, _>>();
    rows.sort_by(|left, right| {
        let left_position = previous_positions.get(&ProcessIdentity::from_row(left));
        let right_position = previous_positions.get(&ProcessIdentity::from_row(right));
        match (left_position, right_position) {
            (Some(left), Some(right)) => left.cmp(right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

pub(crate) fn distinct_process_kill_image_names(targets: &[ProcessKillTarget]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for target in targets {
        let key = target.name.trim().to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        names.push(target.name.clone());
    }
    names
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskkillAttempt {
    image_name: String,
    success: bool,
}

fn taskkill_force_image(image_name: &str) -> TaskkillAttempt {
    let success = Command::new("taskkill")
        .args(["/f", "/im", image_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    TaskkillAttempt {
        image_name: image_name.to_string(),
        success,
    }
}

fn failed_taskkill_names(attempts: &[TaskkillAttempt]) -> String {
    attempts
        .iter()
        .filter(|attempt| !attempt.success)
        .map(|attempt| attempt.image_name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn tracked_live_identities(
    processes: &[ProcessRow],
    normalized_tracked_names: &HashSet<String>,
) -> HashSet<ProcessIdentity> {
    processes
        .iter()
        .filter(|process| normalized_tracked_names.contains(&process.name.to_ascii_lowercase()))
        .map(ProcessIdentity::from_row)
        .collect()
}

fn tracked_total_row(
    processes: &[ProcessRow],
    normalized_tracked_names: &HashSet<String>,
) -> Option<ProcessRow> {
    let tracked = processes
        .iter()
        .filter(|process| normalized_tracked_names.contains(&process.name.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    if tracked.is_empty() {
        return None;
    }

    Some(ProcessRow {
        pid: 0,
        name: "Tracked Total".to_string(),
        executable_path: None,
        start_time: None,
        cpu_percent: sum_optional_f64(tracked.iter().filter_map(|process| process.cpu_percent)),
        private_bytes: sum_optional_u64(tracked.iter().filter_map(|process| process.private_bytes)),
        workset_bytes: sum_optional_u64(tracked.iter().filter_map(|process| process.workset_bytes)),
        workset_private_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.workset_private_bytes),
        ),
        workset_shareable_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.workset_shareable_bytes),
        ),
        workset_shared_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.workset_shared_bytes),
        ),
        thread_count: sum_optional_u64(tracked.iter().filter_map(|process| process.thread_count)),
        handle_count: sum_optional_u64(tracked.iter().filter_map(|process| process.handle_count)),
        user_object_count: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.user_object_count),
        ),
        gdi_object_count: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.gdi_object_count),
        ),
        gpu_percent: sum_optional_f64(tracked.iter().filter_map(|process| process.gpu_percent)),
        gpu_dedicated_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.gpu_dedicated_bytes),
        ),
        gpu_shared_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.gpu_shared_bytes),
        ),
        dotnet_heap_bytes: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.dotnet_heap_bytes),
        ),
        io_read_bytes_per_sec: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.io_read_bytes_per_sec),
        ),
        io_write_bytes_per_sec: sum_optional_u64(
            tracked
                .iter()
                .filter_map(|process| process.io_write_bytes_per_sec),
        ),
    })
}

fn sum_optional_u64(values: impl Iterator<Item = u64>) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for value in values {
        total = total.saturating_add(value);
        found = true;
    }
    found.then_some(total)
}

fn sum_optional_f64(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut found = false;
    let mut total = 0.0;
    for value in values {
        total += value;
        found = true;
    }
    found.then_some(total)
}

fn graph_zoom_target(current: u32, max_span: u32, zoom_in: bool) -> u32 {
    let min_span = u32::from(GRAPH_TIME_SPAN_MIN_SECONDS);
    if zoom_in {
        GRAPH_TIME_SPAN_STEPS_SECONDS
            .iter()
            .rev()
            .copied()
            .find(|span| *span < current)
            .unwrap_or(min_span)
    } else {
        GRAPH_TIME_SPAN_STEPS_SECONDS
            .iter()
            .copied()
            .find(|span| *span > current && *span <= max_span)
            .unwrap_or(max_span)
    }
}

fn graph_pan_step(span_seconds: u32) -> u32 {
    span_seconds.div_ceil(8).max(1)
}

fn synced_sample_viewport_offset(
    total: usize,
    rows: usize,
    selected_index: usize,
    active_selected: usize,
    active_offset: usize,
) -> usize {
    if total == 0 {
        return 0;
    }
    let rows = rows.max(1).min(total);
    let max_offset = total.saturating_sub(rows);
    let selected_index = selected_index.min(total.saturating_sub(1));
    let active_row = active_selected.saturating_sub(active_offset).min(rows - 1);
    selected_index.saturating_sub(active_row).min(max_offset)
}

fn sample_index_at_time(samples: &[GraphSample], captured_at: DateTime<Local>) -> Option<usize> {
    samples
        .iter()
        .position(|sample| sample.captured_at == captured_at)
}

fn format_process_info_time(captured_at: DateTime<Local>) -> String {
    captured_at.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn sample_index_nearest_time(
    samples: &[GraphSample],
    captured_at: DateTime<Local>,
) -> Option<usize> {
    samples
        .iter()
        .enumerate()
        .min_by_key(|(index, sample)| {
            let diff = sample
                .captured_at
                .signed_duration_since(captured_at)
                .num_milliseconds()
                .unsigned_abs();
            (diff, usize::MAX - *index)
        })
        .map(|(index, _)| index)
}

fn rounded_nonnegative_seconds_between(later: DateTime<Local>, earlier: DateTime<Local>) -> u32 {
    let milliseconds = later
        .signed_duration_since(earlier)
        .num_milliseconds()
        .max(0);
    (milliseconds.saturating_add(500) / 1_000).min(i64::from(u32::MAX)) as u32
}

fn sample_time_span_seconds(first: DateTime<Local>, last: DateTime<Local>) -> u32 {
    let span = last
        .signed_duration_since(first)
        .num_seconds()
        .max(1)
        .min(i64::from(u32::MAX));
    span as u32
}
