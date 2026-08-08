use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::{App, GraphSlotLayout};
use crate::model::{
    ColumnPreset, MetricColumn, ProcessColumnWidths, SortColumn, SortDirection, SortSpec,
};
use crate::samplers::SamplingOptions;

const CONFIG_FILE_NAME: &str = "winproc-tui.toml";
pub(crate) const EMPTY_TRACKED_LIST_NAME: &str = "Empty (default)";

pub(crate) fn is_empty_tracked_list_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(EMPTY_TRACKED_LIST_NAME)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) general: GeneralConfig,
    pub(crate) graphs: GraphConfig,
    pub(crate) process_table: ProcessTableConfig,
    pub(crate) recording: RecordingConfig,
    pub(crate) tracking: TrackingConfig,
    #[serde(alias = "watch", alias = "process")]
    pub(crate) tracked: Vec<TrackedConfig>,
    pub(crate) tracked_lists: Vec<SavedTrackedList>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GeneralConfig {
    pub(crate) mouse: bool,
    pub(crate) theme: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            mouse: true,
            theme: "Dark".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GraphConfig {
    pub(crate) columns: u8,
    pub(crate) samples: bool,
    pub(crate) delta: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            columns: 0,
            samples: true,
            delta: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProcessTableConfig {
    pub(crate) preset: String,
    pub(crate) columns: Vec<String>,
    pub(crate) sort_by: String,
    pub(crate) sort_order: String,
    pub(crate) tracked_only: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) column_widths: BTreeMap<String, i64>,
}

impl Default for ProcessTableConfig {
    fn default() -> Self {
        Self {
            preset: ColumnPreset::Default.label().to_string(),
            columns: ColumnPreset::Default
                .columns()
                .iter()
                .map(|column| column.label().to_string())
                .collect(),
            sort_by: MetricColumn::WorksetPrivateBytes.label().to_string(),
            sort_order: SortDirection::Desc.label().to_string(),
            tracked_only: false,
            column_widths: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RecordingConfig {
    pub(crate) last_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TrackingConfig {
    pub(crate) startup: TrackedListStartup,
    pub(crate) active_list: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackedListStartup {
    #[default]
    ResumeLast,
    ChooseList,
    StartEmpty,
}

impl TrackedListStartup {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ResumeLast => "Resume last",
            Self::ChooseList => "Choose list",
            Self::StartEmpty => "Start empty",
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::ResumeLast => Self::ChooseList,
            Self::ChooseList => Self::StartEmpty,
            Self::StartEmpty => Self::ResumeLast,
        }
    }

    pub(crate) const fn previous(self) -> Self {
        match self {
            Self::ResumeLast => Self::StartEmpty,
            Self::ChooseList => Self::ResumeLast,
            Self::StartEmpty => Self::ChooseList,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TrackedConfig {
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedTrackedList {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) processes: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeConfig {
    pub(crate) mouse: bool,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) recording_last_dir: Option<PathBuf>,
    pub(crate) initial_theme: String,
    pub(crate) initial_graph_slot_layout: GraphSlotLayout,
    pub(crate) initial_show_samples_panel: bool,
    pub(crate) initial_show_sample_delta: bool,
    pub(crate) column_preset: ColumnPreset,
    pub(crate) process_columns: Vec<MetricColumn>,
    pub(crate) process_column_widths: ProcessColumnWidths,
    pub(crate) sort: SortSpec,
    pub(crate) initial_tracked_only: bool,
    pub(crate) process_filters: Vec<String>,
    pub(crate) tracked_list_startup: TrackedListStartup,
    pub(crate) active_tracked_list: Option<String>,
    pub(crate) saved_tracked_lists: Vec<SavedTrackedList>,
    pub(crate) sampling_options: SamplingOptions,
}

pub(crate) fn resolve_config_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("failed to resolve executable path")?;
    let exe_dir = exe
        .parent()
        .context("failed to resolve executable directory")?;
    Ok(exe_dir.join(CONFIG_FILE_NAME))
}

pub(crate) fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    match toml::from_str::<AppConfig>(&raw) {
        Ok(config) => Ok(config),
        Err(error) => {
            eprintln!(
                "Config parse failed for {}: {error}. Falling back to defaults.",
                path.display()
            );
            Ok(AppConfig::default())
        }
    }
}

pub(crate) fn build_runtime_config(config: AppConfig) -> Result<RuntimeConfig> {
    let column_preset = config
        .process_table
        .preset
        .parse()
        .unwrap_or(ColumnPreset::Default);
    let process_columns = parse_columns(&config.process_table.columns)
        .unwrap_or_else(|| column_preset.effective_columns().to_vec());
    let process_column_widths =
        ProcessColumnWidths::from_overrides(config.process_table.column_widths.iter().filter_map(
            |(label, width)| {
                parse_width_column(label).map(|column| {
                    let width = (*width).clamp(
                        i64::from(column.min_width()),
                        i64::from(crate::model::columns::PROCESS_COLUMN_WIDTH_MAX),
                    ) as u16;
                    (column, width)
                })
            },
        ));
    let saved_tracked_lists = normalize_saved_tracked_lists(config.tracked_lists);
    let process_filters = if config.tracking.startup == TrackedListStartup::StartEmpty {
        Vec::new()
    } else {
        config.tracked.into_iter().map(|item| item.name).collect()
    };
    let active_tracked_list = if config.tracking.startup == TrackedListStartup::StartEmpty {
        None
    } else {
        config
            .tracking
            .active_list
            .filter(|name| !is_empty_tracked_list_name(name))
    };

    Ok(RuntimeConfig {
        mouse: config.general.mouse,
        config_path: None,
        recording_last_dir: config.recording.last_dir,
        initial_theme: config.general.theme,
        initial_graph_slot_layout: match config.graphs.columns {
            1 => GraphSlotLayout::OneColumn,
            2 => GraphSlotLayout::TwoColumns,
            _ => GraphSlotLayout::Auto,
        },
        initial_show_samples_panel: config.graphs.samples,
        initial_show_sample_delta: config.graphs.delta,
        column_preset,
        process_columns,
        process_column_widths,
        sort: SortSpec {
            column: config
                .process_table
                .sort_by
                .parse()
                .unwrap_or(SortColumn::Metric(MetricColumn::WorksetPrivateBytes)),
            direction: config
                .process_table
                .sort_order
                .parse()
                .unwrap_or(SortDirection::Desc),
        },
        initial_tracked_only: config.process_table.tracked_only,
        process_filters,
        tracked_list_startup: config.tracking.startup,
        active_tracked_list,
        saved_tracked_lists,
        sampling_options: SamplingOptions {
            collect_ws_share: false,
            collect_gpu: true,
            collect_gui_resources: true,
        },
    })
}

pub(crate) fn write_app_config(path: &Path, app: &App) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let config = AppConfig {
        general: GeneralConfig {
            mouse: app.runtime.mouse,
            theme: app.theme().name.to_string(),
        },
        graphs: GraphConfig {
            columns: app.graph_slot_layout.columns(),
            samples: app.samples_panel_before_two_columns,
            delta: app.show_sample_delta,
        },
        process_table: ProcessTableConfig {
            preset: app.column_preset.label().to_string(),
            columns: app
                .process_columns
                .iter()
                .map(|column| column.label().to_string())
                .collect(),
            sort_by: app.sort.column.label().to_string(),
            sort_order: app.sort.direction.label().to_string(),
            tracked_only: app.watch_enabled,
            column_widths: app
                .process_column_widths
                .overrides()
                .map(|(column, width)| (column.label().to_string(), i64::from(width)))
                .collect(),
        },
        recording: RecordingConfig {
            last_dir: app.recording_last_dir.clone(),
        },
        tracking: TrackingConfig {
            startup: app.runtime.tracked_list_startup,
            active_list: app
                .runtime
                .active_tracked_list
                .clone()
                .filter(|name| !is_empty_tracked_list_name(name)),
        },
        tracked: app
            .watch_list
            .iter()
            .map(|name| TrackedConfig { name: name.clone() })
            .collect(),
        tracked_lists: normalize_saved_tracked_lists(app.runtime.saved_tracked_lists.clone()),
    };
    let content = toml::to_string_pretty(&config)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn normalize_saved_tracked_lists(lists: Vec<SavedTrackedList>) -> Vec<SavedTrackedList> {
    let mut normalized = Vec::<SavedTrackedList>::new();
    for mut list in lists {
        list.name = list.name.trim().to_string();
        if list.name.is_empty()
            || is_empty_tracked_list_name(&list.name)
            || normalized
                .iter()
                .any(|saved| saved.name.eq_ignore_ascii_case(&list.name))
        {
            continue;
        }
        list.processes = dedupe_process_names(list.processes);
        normalized.push(list);
    }
    normalized
}

fn dedupe_process_names(names: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::<String>::new();
    for name in names {
        let name = name.trim().to_string();
        if !name.is_empty()
            && !deduped
                .iter()
                .any(|saved| saved.eq_ignore_ascii_case(&name))
        {
            deduped.push(name);
        }
    }
    deduped
}

fn parse_columns(columns: &[String]) -> Option<Vec<MetricColumn>> {
    let parsed = columns
        .iter()
        .filter_map(|column| column.parse().ok())
        .filter(|column: &MetricColumn| column.is_selectable())
        .collect::<Vec<_>>();
    (!parsed.is_empty()).then_some(parsed)
}

fn parse_width_column(label: &str) -> Option<SortColumn> {
    let column = label.parse::<SortColumn>().ok()?;
    match column {
        SortColumn::Metric(metric) if !metric.is_selectable() => None,
        _ => Some(column),
    }
}
