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
    config::RuntimeConfig,
    model::{
        ColumnPreset, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY, MetricColumn, ProcessHistory,
        ProcessIdentity, ProcessInfo, ProcessRow, Snapshot, SortColumn, SortDirection, SortSpec,
        SystemHistory, SystemMetric, TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY, sort_process_rows,
    },
    samplers::{
        CollectSnapshotResult, SamplingRuntime, SamplingWorker,
        open_files::{OpenFilesReport, OpenFilesResult, OpenFilesWorker},
        process_info::{ProcessInfoResult, ProcessInfoWorker},
    },
    ui::{
        THEMES, Theme, column_picker_row_for_index, column_picker_scroll_max_for_page_size,
        help_scroll_max_for_page_size, log_list_total_rows_for_count, theme_index_by_name,
        widgets::scrollable_modal::ScrollableModalState,
    },
};

const GRAPH_TIME_SPAN_MIN_SECONDS: u16 = 60;
const LIVE_GRAPH_TIME_MAX_SECONDS: u32 = TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY as u32;
const FIXED_PROCESS_COLUMN_COUNT: usize = 2;
pub(crate) const GRAPH_SLOT_COUNT: usize = 4;
pub(crate) const GRAPH_SLOT_MIN_HEIGHT: u16 = 13;
pub(crate) const PROCESS_INFO_DEBOUNCE: Duration = Duration::from_millis(200);
const PROCESS_INFO_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OPEN_FILES_IN_FLIGHT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_NAVIGATION_ORDER_HOLD: Duration = Duration::from_millis(750);
pub(crate) const SAMPLE_STALE_AFTER_SECONDS: u64 = 3;

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
    Integer,
    Percent,
    MegabitsPerSec,
    MegabytesPerSec,
    QueueLength,
}

impl GraphValueFormat {
    pub(crate) fn from_details_metric(metric: DetailsMetric) -> Self {
        match metric {
            DetailsMetric::CpuPercent | DetailsMetric::GpuPercent => Self::Percent,
            DetailsMetric::IoRead | DetailsMetric::IoWrite => Self::MegabitsPerSec,
            _ => Self::Integer,
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
    pub(crate) moved: bool,
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

    pub(crate) fn process_metric(&self) -> Option<DetailsMetric> {
        match self {
            Self::Process { metric, .. } => Some(*metric),
            Self::System { .. } => None,
        }
    }

    pub(crate) fn system_metric(&self) -> Option<SystemMetric> {
        match self {
            Self::Process { .. } => None,
            Self::System { metric } => Some(*metric),
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
            Self::System { .. } => GraphValueFormat::Integer,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSelection {
    SamplesPanel,
    Delta,
}

impl SettingsSelection {
    fn toggled(self) -> Self {
        match self {
            Self::SamplesPanel => Self::Delta,
            Self::Delta => Self::SamplesPanel,
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
pub(crate) enum RecordingPathSelection {
    Start,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LogListClick {
    pub(crate) index: usize,
    pub(crate) at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogDirSelection {
    Apply,
    Cancel,
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
    pub(crate) show_settings_dialog: bool,
    pub(crate) settings_selection: SettingsSelection,
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
    pub(crate) show_log_dir_dialog: bool,
    pub(crate) log_dir_draft: String,
    pub(crate) log_dir_cursor: usize,
    pub(crate) log_dir_completion: PathCompletionState,
    pub(crate) log_dir_selection: LogDirSelection,
    pub(crate) log_dir_error: Option<String>,
    pub(crate) show_open_files: bool,
    pub(crate) open_files_scroll: ScrollableModalState,
    pub(crate) open_files_result: Option<OpenFilesReport>,
    pub(crate) open_files_in_flight: Option<ProcessIdentity>,
    pub(crate) open_files_filter: String,
    pub(crate) open_files_filter_cursor: usize,
    pub(crate) show_process_info_dialog: bool,
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
    pub(crate) graph_slots: [Option<GraphSlot>; GRAPH_SLOT_COUNT],
    pub(crate) active_graph_slot_index: usize,
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
    pub(crate) show_samples_panel: bool,
    pub(crate) show_sample_delta: bool,
    pub(crate) details_live: bool,
    pub(crate) column_preset: ColumnPreset,
    pub(crate) process_columns: Vec<MetricColumn>,
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
        let selected_process_identity = process_table_state
            .selected()
            .and_then(|index| initial.snapshot.processes.get(index))
            .map(ProcessIdentity::from_row);
        let last_tracked_live_identities =
            tracked_live_identities(&initial.snapshot.processes, &normalized_watch_names);
        let mut app = Self {
            theme_index: theme_index_by_name(&runtime.initial_theme),
            runtime,
            sampling_worker,
            process_info_worker,
            open_files_worker,
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
            show_settings_dialog: false,
            settings_selection: SettingsSelection::SamplesPanel,
            show_quit_confirmation: false,
            quit_confirm_selection: QuitConfirmSelection::Cancel,
            show_recording_no_tracked_warning: false,
            show_recording_path_dialog: false,
            recording_path_draft: String::new(),
            recording_path_cursor: 0,
            recording_path_completion: PathCompletionState::default(),
            recording_path_selection: RecordingPathSelection::Start,
            show_recording_overwrite_confirmation: false,
            recording_overwrite_selection: RecordingOverwriteSelection::Cancel,
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
            show_log_dir_dialog: false,
            log_dir_draft: String::new(),
            log_dir_cursor: 0,
            log_dir_completion: PathCompletionState::default(),
            log_dir_selection: LogDirSelection::Apply,
            log_dir_error: None,
            show_open_files: false,
            open_files_scroll: ScrollableModalState {
                page_size: 1,
                ..ScrollableModalState::default()
            },
            open_files_result: None,
            open_files_in_flight: None,
            open_files_filter: String::new(),
            open_files_filter_cursor: 0,
            show_process_info_dialog: false,
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
            graph_slots: std::array::from_fn(|_| None),
            active_graph_slot_index: 0,
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
            show_samples_panel: true,
            show_sample_delta: true,
            details_live: true,
            column_preset,
            process_columns,
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
        Duration::from_secs(1)
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
            || self.show_open_files
            || self.show_process_info_dialog
            || self.show_system_info_dialog
            || self.show_quit_confirmation
            || self.show_recording_no_tracked_warning
            || self.show_recording_path_dialog
            || self.show_recording_overwrite_confirmation
            || self.show_tracked_remove_confirmation
            || self.show_process_kill_confirmation
            || self.show_display_area_warning
            || self.show_metric_column_warning
            || self.show_no_graph_metrics_warning
            || self.show_settings_dialog
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
        if self.focused_panel == FocusedPanel::DetailsSamples && !self.show_samples_panel {
            self.focused_panel = FocusedPanel::DetailsGraph;
            return;
        }
        if self.show_details
            && matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            )
            && self.visible_graph_slot_indices().is_empty()
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
        if !self.show_details && !self.graph_slots_fit(self.active_graph_slot_count()) {
            self.show_display_area_warning = true;
            self.status = "Not enough display area.".to_string();
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

    pub(crate) fn clear_graph_slots(&mut self) {
        self.graph_slots = std::array::from_fn(|_| None);
        self.active_graph_slot_index = 0;
        self.show_details = false;
        self.ab_comparison = None;
        if matches!(
            self.focused_panel,
            FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
        ) {
            self.focused_panel = FocusedPanel::Processes;
        }
        self.status = "Graph metrics cleared".to_string();
    }

    pub(crate) fn clear_selected_graph_metric(&mut self) -> bool {
        let Some(index) = self.selected_process_graph_slot_index() else {
            return false;
        };
        self.clear_graph_slot(index);
        self.status = format!("Graph#{} cleared", index + 1);
        true
    }

    fn clear_graph_slot(&mut self, slot_index: usize) {
        if slot_index >= GRAPH_SLOT_COUNT {
            return;
        }
        self.graph_slots[slot_index] = None;
        if self.active_graph_slot_index == slot_index {
            self.active_graph_slot_index = self
                .graph_slots
                .iter()
                .position(Option::is_some)
                .unwrap_or(0);
        }
        if self.active_graph_slot_count() == 0 {
            self.show_details = false;
            self.ab_comparison = None;
            if matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            ) {
                self.focused_panel = FocusedPanel::Processes;
            }
        }
    }

    pub(crate) fn open_settings_dialog(&mut self) {
        self.show_settings_dialog = true;
        self.status = "Settings".to_string();
    }

    pub(crate) fn close_settings_dialog(&mut self) {
        self.show_settings_dialog = false;
        if !self.show_samples_panel && self.focused_panel == FocusedPanel::DetailsSamples {
            self.focused_panel = FocusedPanel::DetailsGraph;
        }
        self.ensure_visible_panel_focus();
        self.status = "Ready".to_string();
    }

    pub(crate) fn select_next_setting(&mut self) {
        self.settings_selection = self.settings_selection.toggled();
    }

    pub(crate) fn select_previous_setting(&mut self) {
        self.settings_selection = self.settings_selection.toggled();
    }

    pub(crate) fn toggle_selected_setting(&mut self) {
        match self.settings_selection {
            SettingsSelection::SamplesPanel => {
                self.show_samples_panel = !self.show_samples_panel;
                if !self.show_samples_panel && self.focused_panel == FocusedPanel::DetailsSamples {
                    self.focused_panel = FocusedPanel::DetailsGraph;
                }
                self.status = if self.show_samples_panel {
                    "Samples panel shown".to_string()
                } else {
                    "Samples panel hidden".to_string()
                };
            }
            SettingsSelection::Delta => {
                self.show_sample_delta = !self.show_sample_delta;
                self.status = if self.show_sample_delta {
                    "Delta shown".to_string()
                } else {
                    "Delta hidden".to_string()
                };
            }
        }
    }

    pub(crate) fn active_graph_slot_count(&self) -> usize {
        self.graph_slots
            .iter()
            .filter(|slot| slot.is_some())
            .count()
    }

    pub(crate) fn graph_slots_fit(&self, slot_count: usize) -> bool {
        if slot_count == 0 {
            return true;
        }
        crate::ui::details_slots_area_for_screen(self.last_screen_area, true)
            .is_some_and(|area| !crate::ui::layout::details_slot_areas(area, slot_count).is_empty())
    }

    pub(crate) fn graph_slot(&self, index: usize) -> Option<&GraphSlot> {
        self.graph_slots.get(index).and_then(Option::as_ref)
    }

    pub(crate) fn visible_graph_slot_indices(&self) -> Vec<usize> {
        self.graph_slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|_| index))
            .collect()
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
        self.graph_slot(self.active_graph_slot_index)
            .or_else(|| self.graph_slots.iter().find_map(Option::as_ref))
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
        if slot_index == self.active_graph_slot_index {
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

    pub(crate) fn active_graph_visible_index(&self) -> usize {
        self.graph_slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .position(|(index, _)| index == self.active_graph_slot_index)
            .unwrap_or(0)
    }

    pub(crate) fn close_graph_slots_that_do_not_fit(&mut self) {
        if !self.show_details {
            return;
        }

        let mut closed = Vec::new();
        while self.active_graph_slot_count() > 0
            && !self.graph_slots_fit(self.active_graph_slot_count())
        {
            let Some(index) = self.graph_slots.iter().rposition(Option::is_some) else {
                break;
            };
            self.graph_slots[index] = None;
            closed.push(index + 1);
        }

        if self.active_graph_slot_count() == 0 {
            self.show_details = false;
            self.ab_comparison = None;
            if matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            ) {
                self.focused_panel = FocusedPanel::Processes;
            }
        } else if self.graph_slots[self.active_graph_slot_index].is_none() {
            self.active_graph_slot_index = self
                .graph_slots
                .iter()
                .position(Option::is_some)
                .unwrap_or(0);
            if matches!(
                self.focused_panel,
                FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
            ) {
                self.focused_panel = FocusedPanel::DetailsGraph;
            }
        }

        if !closed.is_empty() {
            closed.sort_unstable();
            let labels = closed
                .into_iter()
                .map(|index| format!("#{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.status = format!("Closed Graph{labels}: not enough display area");
        }
    }

    pub(crate) fn toggle_selected_metric_for_graph_slot(&mut self, slot_index: usize) {
        if slot_index >= GRAPH_SLOT_COUNT {
            return;
        }
        if self.focused_panel != FocusedPanel::Processes {
            self.status = "Graph slots require Processes focus".to_string();
            return;
        }
        let Some(identity) = self.selected_visible_process_identity() else {
            self.status = "No process selected".to_string();
            return;
        };
        let Some(column) = self.selected_process_metric_column() else {
            self.show_metric_column_warning = true;
            self.status = "Select a metric cell before pressing 1-4".to_string();
            return;
        };
        let Some(metric) = DetailsMetric::from_graphable_column(column) else {
            self.show_metric_column_warning = true;
            self.status = "Full Path cannot be graphed".to_string();
            return;
        };
        let next_slot = GraphSlot::process(identity, metric);
        if self.graph_slots[slot_index].as_ref() == Some(&next_slot) {
            self.clear_graph_slot(slot_index);
            self.status = format!("Graph#{} cleared", slot_index + 1);
            return;
        }

        if let Some(duplicate_index) =
            self.graph_slots
                .iter()
                .enumerate()
                .find_map(|(index, slot)| {
                    (index != slot_index && slot.as_ref() == Some(&next_slot)).then_some(index)
                })
        {
            self.graph_slots[duplicate_index] = None;
            self.graph_slots[slot_index] = Some(next_slot);
            self.active_graph_slot_index = slot_index;
            self.show_details = true;
            self.clamp_details_sample_selection();
            self.select_details_sample_latest();
            self.status = format!("Graph metric moved to Graph#{}", slot_index + 1);
            return;
        }

        let was_empty = self.graph_slots[slot_index].is_none();
        let next_count = self.active_graph_slot_count() + usize::from(was_empty);
        if !self.graph_slots_fit(next_count) {
            self.show_display_area_warning = true;
            self.status = "Not enough display area.".to_string();
            return;
        }

        self.graph_slots[slot_index] = Some(next_slot);
        self.active_graph_slot_index = slot_index;
        self.show_details = true;
        self.clamp_details_sample_selection();
        self.select_details_sample_latest();
        self.status = format!("Graph#{} metric selected", slot_index + 1);
    }

    fn selected_process_graph_slot_index(&self) -> Option<usize> {
        if self.focused_panel != FocusedPanel::Processes {
            return None;
        }
        let identity = self.selected_visible_process_identity()?;
        let metric = DetailsMetric::from_graphable_column(self.selected_process_metric_column()?)?;
        let selected_slot = GraphSlot::process(identity, metric);
        self.graph_slots
            .iter()
            .position(|slot| slot.as_ref() == Some(&selected_slot))
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
        let selected_time = self.selected_details_sample_time();
        let next = self.next_focus_target();
        self.focused_panel = next.0;
        if let Some(slot_index) = next.1 {
            self.active_graph_slot_index = slot_index;
            if let Some(time) = selected_time {
                self.align_details_sample_selection_to_time(time);
            }
        }
        self.status = self.focus_status();
    }

    pub(crate) fn cycle_focus_previous(&mut self) {
        let selected_time = self.selected_details_sample_time();
        let previous = self.previous_focus_target();
        self.focused_panel = previous.0;
        if let Some(slot_index) = previous.1 {
            self.active_graph_slot_index = slot_index;
            if let Some(time) = selected_time {
                self.align_details_sample_selection_to_time(time);
            }
        }
        self.status = self.focus_status();
    }

    fn next_focus_target(&self) -> (FocusedPanel, Option<usize>) {
        let slots = self.visible_graph_slot_indices();
        if !self.show_details || slots.is_empty() {
            return (self.focused_panel.next(false), None);
        }

        match self.focused_panel {
            FocusedPanel::System => (FocusedPanel::SystemActivity, None),
            FocusedPanel::SystemActivity => (FocusedPanel::Cpu, None),
            FocusedPanel::Cpu => (FocusedPanel::Processes, None),
            FocusedPanel::Processes => (FocusedPanel::DetailsGraph, slots.first().copied()),
            FocusedPanel::DetailsGraph => (
                if self.show_samples_panel {
                    FocusedPanel::DetailsSamples
                } else {
                    FocusedPanel::System
                },
                Some(self.active_graph_slot_index),
            ),
            FocusedPanel::DetailsSamples => {
                let next_slot = slots
                    .iter()
                    .copied()
                    .find(|index| *index > self.active_graph_slot_index);
                next_slot
                    .map(|index| (FocusedPanel::DetailsGraph, Some(index)))
                    .unwrap_or((FocusedPanel::System, None))
            }
        }
    }

    fn previous_focus_target(&self) -> (FocusedPanel, Option<usize>) {
        let slots = self.visible_graph_slot_indices();
        if !self.show_details || slots.is_empty() {
            return (self.focused_panel.previous(false), None);
        }

        match self.focused_panel {
            FocusedPanel::System => (
                if self.show_samples_panel {
                    FocusedPanel::DetailsSamples
                } else {
                    FocusedPanel::DetailsGraph
                },
                slots.last().copied(),
            ),
            FocusedPanel::SystemActivity => (FocusedPanel::System, None),
            FocusedPanel::Cpu => (FocusedPanel::SystemActivity, None),
            FocusedPanel::Processes => (FocusedPanel::Cpu, None),
            FocusedPanel::DetailsGraph => {
                let previous_slot = slots
                    .iter()
                    .rev()
                    .copied()
                    .find(|index| *index < self.active_graph_slot_index);
                if self.show_samples_panel {
                    previous_slot
                        .map(|index| (FocusedPanel::DetailsSamples, Some(index)))
                        .unwrap_or((FocusedPanel::Processes, None))
                } else {
                    previous_slot
                        .map(|index| (FocusedPanel::DetailsGraph, Some(index)))
                        .unwrap_or((FocusedPanel::Processes, None))
                }
            }
            FocusedPanel::DetailsSamples => (
                FocusedPanel::DetailsGraph,
                Some(self.active_graph_slot_index),
            ),
        }
    }

    fn focus_status(&self) -> String {
        match self.focused_panel {
            FocusedPanel::DetailsGraph => {
                format!("Focus: Graph#{}", self.active_graph_slot_index + 1)
            }
            FocusedPanel::DetailsSamples => {
                format!("Focus: Samples#{}", self.active_graph_slot_index + 1)
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

    pub(crate) fn toggle_selected_system_metric_for_graph_slot(&mut self, slot_index: usize) {
        self.toggle_system_metric_for_graph_slot(
            slot_index,
            self.selected_system_metric(),
            FocusedPanel::System,
        );
    }

    pub(crate) fn toggle_selected_system_activity_metric_for_graph_slot(
        &mut self,
        slot_index: usize,
    ) {
        self.toggle_system_metric_for_graph_slot(
            slot_index,
            self.selected_system_activity_metric(),
            FocusedPanel::SystemActivity,
        );
    }

    pub(crate) fn toggle_cpu_average_for_graph_slot(&mut self, slot_index: usize) {
        self.toggle_system_metric_for_graph_slot(
            slot_index,
            SystemMetric::CpuAverage,
            FocusedPanel::Cpu,
        );
    }

    fn toggle_system_metric_for_graph_slot(
        &mut self,
        slot_index: usize,
        metric: SystemMetric,
        required_focus: FocusedPanel,
    ) {
        if slot_index >= GRAPH_SLOT_COUNT {
            return;
        }
        if self.focused_panel != required_focus {
            self.status = format!("Graph slots require {} focus", required_focus.label());
            return;
        }
        let next_slot = GraphSlot::system(metric);
        if self.graph_slots[slot_index].as_ref() == Some(&next_slot) {
            self.graph_slots[slot_index] = None;
            if self.active_graph_slot_index == slot_index {
                self.active_graph_slot_index = self
                    .graph_slots
                    .iter()
                    .position(Option::is_some)
                    .unwrap_or(0);
            }
            if self.active_graph_slot_count() == 0 {
                self.show_details = false;
                self.ab_comparison = None;
                if matches!(
                    self.focused_panel,
                    FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
                ) {
                    self.focused_panel = FocusedPanel::Processes;
                }
            }
            self.status = format!("Graph#{} cleared", slot_index + 1);
            return;
        }

        if let Some(duplicate_index) =
            self.graph_slots
                .iter()
                .enumerate()
                .find_map(|(index, slot)| {
                    (index != slot_index && slot.as_ref() == Some(&next_slot)).then_some(index)
                })
        {
            self.status = format!(
                "Same graph metric is already selected in Graph#{}",
                duplicate_index + 1
            );
            return;
        }

        let was_empty = self.graph_slots[slot_index].is_none();
        let next_count = self.active_graph_slot_count() + usize::from(was_empty);
        if !self.graph_slots_fit(next_count) {
            self.show_display_area_warning = true;
            self.status = "Not enough display area.".to_string();
            return;
        }

        self.graph_slots[slot_index] = Some(next_slot);
        self.active_graph_slot_index = slot_index;
        self.show_details = true;
        self.clamp_details_sample_selection();
        self.select_details_sample_latest();
        self.status = format!("Graph#{} metric selected", slot_index + 1);
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
        self.status = "Samples selection moved older".to_string();
    }

    pub(crate) fn select_details_sample_newer(&mut self, amount: usize) {
        self.details_sample_selected = self.details_sample_selected.saturating_add(amount);
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.status = "Samples selection moved newer".to_string();
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
        self.status = "Samples scrolled".to_string();
    }

    pub(crate) fn select_details_sample_oldest(&mut self) {
        self.details_sample_selected = 0;
        self.details_sample_offset = 0;
        self.details_live = false;
        self.status = "Samples selection: oldest".to_string();
    }

    pub(crate) fn select_details_sample_latest(&mut self) {
        self.details_sample_selected = self.selected_sample_count().saturating_sub(1);
        self.scroll_details_samples_to_latest();
        self.details_live = true;
        self.status = "Samples selection: latest".to_string();
    }

    pub(crate) fn set_details_sample_selected(&mut self, index: usize) {
        self.details_sample_selected = index;
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = self.details_sample_selected + 1 == self.selected_sample_count();
        self.status = format!("Samples selection: {}", self.details_sample_selected + 1);
    }

    pub(crate) fn set_details_sample_selected_manual(&mut self, index: usize) {
        self.details_sample_selected = index;
        self.clamp_details_sample_selection();
        self.ensure_details_sample_visible();
        self.details_live = false;
        self.status = format!("Samples selection: {}", self.details_sample_selected + 1);
    }

    pub(crate) fn select_details_sample_nearest_age_seconds(&mut self, age_seconds: i64) {
        let Some(slot) = self.active_graph_slot() else {
            return;
        };
        let samples = self.graph_slot_samples(slot);
        let Some(latest) = samples.last().map(|sample| sample.captured_at) else {
            return;
        };
        let Some((index, _)) = samples.iter().enumerate().min_by_key(|(_, sample)| {
            let age = latest
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
        let area =
            crate::ui::process_table_area_for_screen(self.last_screen_area, self.show_details);
        area.width
    }

    fn visible_process_metric_range(&self) -> std::ops::Range<usize> {
        crate::ui::process_table_visible_metric_range(
            self.process_table_area_width(),
            &self.process_columns,
            self.process_metric_column_offset,
        )
    }

    fn ensure_selected_process_column_visible(&mut self) {
        if self.process_columns.is_empty() {
            self.process_metric_column_offset = 0;
            return;
        }
        let Some(metric_index) = self
            .selected_process_column_index
            .checked_sub(FIXED_PROCESS_COLUMN_COUNT)
        else {
            return;
        };
        let metric_index = metric_index.min(self.process_columns.len().saturating_sub(1));
        self.process_metric_column_offset = self
            .process_metric_column_offset
            .min(self.process_columns.len().saturating_sub(1));
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

    pub(crate) fn reset_graph_to_live_edge(&mut self) {
        self.graph_show_all_samples = false;
        self.graph_time_offset_seconds = 0;
        self.graph_time_window_right_at = None;
        self.status = "Graph right edge: 0s".to_string();
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

    pub(crate) fn zoom_graph_time_span(&mut self, zoom_in: bool) {
        self.graph_show_all_samples = false;
        let max_span = self.graph_time_max_seconds();
        let next = if zoom_in {
            self.graph_time_span_seconds
                .saturating_sub(graph_zoom_step(self.graph_time_span_seconds))
                .max(u32::from(GRAPH_TIME_SPAN_MIN_SECONDS))
        } else {
            self.graph_time_span_seconds
                .saturating_add(graph_zoom_step(self.graph_time_span_seconds))
                .min(max_span)
        };
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
        let latest = self.active_graph_latest_sample_at()?;
        Some(latest - chrono::Duration::seconds(i64::from(self.graph_time_offset_seconds)))
    }

    fn active_graph_latest_sample_at(&self) -> Option<DateTime<Local>> {
        let slot = self.active_graph_slot()?;
        self.graph_slot_samples(slot)
            .last()
            .map(|sample| sample.captured_at)
    }

    fn restore_frozen_graph_time_window(&mut self) {
        if self.graph_show_all_samples {
            return;
        }
        let Some(right_edge) = self.graph_time_window_right_at else {
            return;
        };
        let Some(latest) = self.active_graph_latest_sample_at() else {
            return;
        };
        let offset = rounded_nonnegative_seconds_between(latest, right_edge);
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
        let Some(latest) = samples.last().map(|sample| sample.captured_at) else {
            return Vec::new();
        };
        let mut ages = samples
            .iter()
            .map(|sample| {
                latest
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
            self.selected_sample_time_span_seconds()
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

    fn selected_sample_time_span_seconds(&self) -> u32 {
        self.active_graph_slot()
            .and_then(|slot| {
                let samples = self
                    .graph_slot_samples(slot)
                    .into_iter()
                    .map(|sample| sample.captured_at)
                    .collect::<Vec<_>>();
                sample_time_span_seconds(&samples)
            })
            .unwrap_or(self.graph_time_span_seconds)
    }

    fn graph_time_max_seconds(&self) -> u32 {
        if self.activity() == AppActivity::LogView {
            self.selected_sample_time_span_seconds()
                .max(LIVE_GRAPH_TIME_MAX_SECONDS)
        } else {
            LIVE_GRAPH_TIME_MAX_SECONDS
        }
    }

    pub(crate) fn toggle_watch_list(&mut self) {
        if self.watch_list.is_empty() {
            self.watch_enabled = false;
            self.status = "Tracked List is empty".to_string();
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

    pub(crate) fn add_selected_process_to_watch_list(&mut self) {
        let Some(name) = self.selected_visible_process_name() else {
            self.status = "No process selected".to_string();
            return;
        };

        self.add_process_name_to_tracked_list(name);
    }

    pub(crate) fn toggle_selected_process_tracking(&mut self) {
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
        let name = self.tracked_remove_name.clone();
        let discarded = self
            .process_history
            .prune_name_to_latest(&name, GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
        self.reset_tracked_remove_confirmation();
        self.remove_process_name_from_tracked_list(name.clone());
        self.status =
            format!("Removed from Tracked List: {name}; discarded {discarded} older samples");
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
        self.status = format!("Added to Tracked List: {name}");
    }

    #[cfg(test)]
    pub(crate) fn remove_selected_process_from_watch_list(&mut self) {
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
            format!("Not in Tracked List: {name}")
        } else {
            format!("Removed from Tracked List: {name}")
        };
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
        let identity = self.selected_visible_process_identity()?;
        let cache = self.display_process_info_cache();
        cache.get(&identity).or_else(|| {
            self.display_process_info_identity()
                .and_then(|identity| cache.get(identity))
        })
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
        let Some(identity) = self.selected_visible_process_identity() else {
            self.pending_process_info = None;
            return;
        };
        if !force_refresh && self.process_info_cache.contains_key(&identity) {
            self.process_info_display_identity = Some(identity);
            self.pending_process_info = None;
            return;
        }
        let Some(process) = self.selected_visible_process().cloned() else {
            self.pending_process_info = None;
            return;
        };
        let lifecycle = self
            .selected_visible_process_lifecycle()
            .unwrap_or(ProcessLifecycle::Live);
        if self.process_info_in_flight.as_ref() == Some(&identity) {
            self.pending_process_info = None;
            return;
        }
        self.pending_process_info = Some(PendingProcessInfo {
            identity,
            process,
            lifecycle,
            changed_at: Instant::now(),
            force_refresh,
        });
    }

    fn cancel_process_info_request(&mut self) {
        self.pending_process_info = None;
        self.process_info_in_flight = None;
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
            pending.identity.clone(),
            pending.process,
            pending.lifecycle,
        )?;
        self.process_info_in_flight = Some(pending.identity);
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
                    self.status = "Warning: process info worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_process_info_result(&mut self, result: ProcessInfoResult) -> bool {
        if self.process_info_in_flight.as_ref() != Some(&result.identity) {
            return false;
        }
        self.process_info_in_flight = None;
        if !self.show_process_info_dialog
            || self.selected_visible_process_identity().as_ref() != Some(&result.identity)
        {
            return false;
        }
        self.process_info_display_identity = Some(result.identity.clone());
        self.process_info_cache.insert(result.identity, result.info);
        true
    }

    pub(crate) fn open_selected_process_files(&mut self) -> Result<()> {
        self.request_open_files_for_selected_process(true, "Loading open files for")
    }

    pub(crate) fn open_selected_process_info_dialog(&mut self) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            self.status = "Process Info is unavailable in Log view".to_string();
            return Ok(());
        }
        let Some(process) = self.selected_visible_process() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };
        let process_name = process.name.clone();
        self.show_process_info_dialog = true;
        self.ensure_selected_process_info();
        self.status = format!("Process Info: {process_name}");
        Ok(())
    }

    pub(crate) fn close_process_info_dialog(&mut self) {
        self.show_process_info_dialog = false;
        self.cancel_process_info_request();
        self.status = "Process Info closed".to_string();
    }

    pub(crate) fn refresh_open_files(&mut self) -> Result<()> {
        if self.open_files_in_flight.is_some() {
            self.status = "Open files refresh already in progress".to_string();
            return Ok(());
        }
        self.request_open_files_for_selected_process(false, "Refreshing open files for")
    }

    fn request_open_files_for_selected_process(
        &mut self,
        clear_previous_result: bool,
        status_prefix: &str,
    ) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            self.status = "Open files are unavailable in Log view".to_string();
            return Ok(());
        }
        let Some(identity) = self.selected_visible_process_identity() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };
        let Some(process) = self.selected_visible_process().cloned() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };
        if !matches!(
            self.selected_visible_process_lifecycle(),
            Some(ProcessLifecycle::Live)
        ) {
            self.status = "Open files require a live process".to_string();
            return Ok(());
        }

        self.open_files_worker
            .request_open_files(identity.clone(), process.clone())?;
        self.show_open_files = true;
        if clear_previous_result {
            self.open_files_result = None;
        }
        self.open_files_in_flight = Some(identity);
        self.open_files_scroll.reset();
        self.status = format!("{status_prefix} {}", process.name);
        Ok(())
    }

    pub(crate) fn close_open_files(&mut self) {
        self.show_open_files = false;
        self.open_files_scroll.stop_drag();
        self.status = "Open files closed".to_string();
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
                    self.status = "Warning: open files worker stopped".to_string();
                    return Ok(true);
                }
            }
        }
    }

    fn apply_open_files_result(&mut self, result: OpenFilesResult) -> bool {
        if self.open_files_in_flight.as_ref() != Some(&result.identity) {
            return false;
        }
        self.open_files_in_flight = None;
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
        self.open_files_result = Some(result.report);
        self.open_files_scroll.set_page_size(
            self.open_files_scroll.page_size,
            self.open_files_total_rows(),
        );
        true
    }

    pub(crate) fn set_open_files_page_size(&mut self, page_size: usize) {
        self.open_files_scroll
            .set_page_size(page_size, self.open_files_total_rows());
    }

    pub(crate) fn scroll_open_files_up(&mut self, amount: usize) {
        self.open_files_scroll.scroll_up(amount);
    }

    pub(crate) fn scroll_open_files_down(&mut self, amount: usize) {
        self.open_files_scroll
            .scroll_down(amount, self.open_files_total_rows());
    }

    pub(crate) fn scroll_open_files_home(&mut self) {
        self.open_files_scroll.scroll_home();
    }

    pub(crate) fn scroll_open_files_end(&mut self) {
        self.open_files_scroll
            .scroll_end(self.open_files_total_rows());
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

    pub(crate) fn start_open_files_scrollbar_drag(&mut self, x: u16, y: u16, area: Rect) -> bool {
        let Some(scrollbar) = crate::ui::open_files_scrollbar_area_for_screen(area, self) else {
            return false;
        };
        if x != scrollbar.x || y < scrollbar.y || y >= scrollbar.bottom() {
            return false;
        }
        self.open_files_scroll
            .start_drag(scrollbar, y, self.open_files_total_rows());
        true
    }

    pub(crate) fn drag_open_files_scrollbar(&mut self, y: u16, area: Rect) {
        if let Some(scrollbar) = crate::ui::open_files_scrollbar_area_for_screen(area, self) {
            self.open_files_scroll
                .drag_to(scrollbar, y, self.open_files_total_rows());
        }
    }

    pub(crate) fn open_files_total_rows(&self) -> usize {
        crate::ui::open_files_total_rows(self)
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
            self.stop_recording()?;
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
        self.log_dir_selection = LogDirSelection::Apply;
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
            LogDirSelection::Apply => self.confirm_log_dir(),
            LogDirSelection::Cancel => {
                self.cancel_log_dir_dialog();
                Ok(())
            }
        }
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
        self.graph_slots = std::array::from_fn(|_| None);
        self.show_details = false;
        self.ab_comparison = None;
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
        self.graph_slots = std::array::from_fn(|_| None);
        self.show_details = false;
        self.ab_comparison = None;
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
                    self.recording_session = None;
                    recording_stopped = true;
                    status_parts.push(format!("Recording stopped: {error}"));
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

fn graph_zoom_step(span_seconds: u32) -> u32 {
    if span_seconds <= u32::from(GRAPH_TIME_SPAN_MIN_SECONDS) {
        u32::from(GRAPH_TIME_SPAN_MIN_SECONDS)
    } else {
        60
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

fn sample_time_span_seconds(samples: &[DateTime<Local>]) -> Option<u32> {
    let first = samples.first()?;
    let last = samples.last()?;
    let span = last
        .signed_duration_since(*first)
        .num_seconds()
        .max(1)
        .min(i64::from(u32::MAX));
    Some(span as u32)
}
