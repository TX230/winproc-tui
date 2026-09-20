use std::{
    collections::BTreeMap,
    fs,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::{App, GraphSlotLayout, ProcessPanelHeight, ProcessViewMode};
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
    #[serde(skip_serializing)]
    pub(crate) tracking: TrackingConfig,
    #[serde(alias = "watch", alias = "process", skip_serializing)]
    pub(crate) tracked: Vec<TrackedConfig>,
    #[serde(skip_serializing)]
    pub(crate) tracked_lists: Vec<SavedTrackedList>,
    pub(crate) investigation: Option<InvestigationConfig>,
    pub(crate) investigation_profiles: Vec<SavedInvestigationProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GeneralConfig {
    pub(crate) mouse: bool,
    pub(crate) theme: String,
    pub(crate) high_contrast: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            mouse: true,
            theme: "Green".to_string(),
            high_contrast: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GraphConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) columns: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) time_span_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) samples: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delta: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) y_axis_zero_min: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProcessTableConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) view: Option<String>,
    #[serde(skip_serializing)]
    pub(crate) preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sort_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sort_order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tracked_only: Option<bool>,
    pub(crate) body_rows: ProcessPanelHeightConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) column_widths: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ProcessPanelHeightConfig {
    Rows(i64),
    Mode(String),
}

impl Default for ProcessPanelHeightConfig {
    fn default() -> Self {
        Self::Mode("auto".to_string())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RecordingConfig {
    pub(crate) last_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interval_seconds: Option<u64>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvestigationStartup {
    #[default]
    ResumeLast,
    ChooseProfile,
    StartEmpty,
}

impl InvestigationStartup {
    pub(crate) const ALL: [Self; 3] = [Self::ResumeLast, Self::ChooseProfile, Self::StartEmpty];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ResumeLast => "Resume last",
            Self::ChooseProfile => "Ask at startup",
            Self::StartEmpty => "Start empty",
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::ResumeLast => Self::ChooseProfile,
            Self::ChooseProfile => Self::StartEmpty,
            Self::StartEmpty => Self::ResumeLast,
        }
    }

    pub(crate) const fn previous(self) -> Self {
        match self {
            Self::ResumeLast => Self::StartEmpty,
            Self::ChooseProfile => Self::ResumeLast,
            Self::StartEmpty => Self::ChooseProfile,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InvestigationStateConfig {
    pub(crate) tracked_names: Vec<String>,
    #[serde(skip_serializing)]
    pub(crate) tracked_only: Option<bool>,
    #[serde(skip_serializing)]
    pub(crate) process_view: Option<String>,
    #[serde(skip_serializing)]
    pub(crate) process_columns: Option<Vec<String>>,
    #[serde(skip_serializing)]
    pub(crate) sort_by: Option<String>,
    #[serde(skip_serializing)]
    pub(crate) sort_order: Option<String>,
    #[serde(skip_serializing)]
    pub(crate) graphs: Vec<InvestigationGraphConfig>,
    #[serde(skip_serializing)]
    pub(crate) graph_columns: Option<u8>,
    #[serde(skip_serializing)]
    pub(crate) graph_time_span_seconds: Option<u32>,
    #[serde(skip_serializing)]
    pub(crate) samples: Option<bool>,
    #[serde(skip_serializing)]
    pub(crate) delta: Option<bool>,
    #[serde(skip_serializing)]
    pub(crate) y_axis_zero_min: Option<bool>,
    #[serde(skip_serializing)]
    pub(crate) recording_interval_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InvestigationConfig {
    pub(crate) startup: InvestigationStartup,
    pub(crate) active_profile: Option<String>,
    #[serde(flatten)]
    pub(crate) last: InvestigationStateConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SavedInvestigationProfile {
    pub(crate) name: String,
    #[serde(flatten)]
    pub(crate) investigation: InvestigationStateConfig,
}

impl Deref for SavedInvestigationProfile {
    type Target = InvestigationStateConfig;

    fn deref(&self) -> &Self::Target {
        &self.investigation
    }
}

impl DerefMut for SavedInvestigationProfile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.investigation
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InvestigationGraphConfig {
    pub(crate) kind: String,
    pub(crate) metric: String,
    pub(crate) display_mode: String,
    pub(crate) process_name: Option<String>,
    pub(crate) executable_path: Option<String>,
    pub(crate) gpu_adapter_name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeConfig {
    pub(crate) mouse: bool,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) recording_last_dir: Option<PathBuf>,
    pub(crate) initial_theme: String,
    pub(crate) initial_high_contrast: bool,
    pub(crate) initial_graph_slot_layout: GraphSlotLayout,
    pub(crate) initial_graph_time_span_seconds: u32,
    pub(crate) initial_graph_y_axis_zero_min: bool,
    pub(crate) initial_show_samples_panel: bool,
    pub(crate) initial_show_sample_delta: bool,
    pub(crate) initial_recording_interval_seconds: u64,
    pub(crate) column_preset: ColumnPreset,
    pub(crate) process_columns: Vec<MetricColumn>,
    pub(crate) process_column_widths: ProcessColumnWidths,
    pub(crate) sort: SortSpec,
    pub(crate) initial_tracked_only: bool,
    pub(crate) initial_process_view_mode: ProcessViewMode,
    pub(crate) initial_process_panel_height: ProcessPanelHeight,
    pub(crate) process_filters: Vec<String>,
    pub(crate) investigation_startup: InvestigationStartup,
    pub(crate) active_investigation_profile: Option<String>,
    pub(crate) saved_investigation_profiles: Vec<SavedInvestigationProfile>,
    pub(crate) sampling_options: SamplingOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigPaths {
    pub(crate) active: PathBuf,
    pub(crate) legacy: Option<PathBuf>,
}

pub(crate) fn resolve_config_paths() -> Result<ConfigPaths> {
    let launched_exe = std::env::current_exe().context("failed to resolve executable path")?;
    resolve_config_paths_from_executable(&launched_exe)
}

pub(crate) fn resolve_config_paths_from_executable(launched_exe: &Path) -> Result<ConfigPaths> {
    let real_exe = fs::canonicalize(launched_exe).with_context(|| {
        format!(
            "failed to resolve real executable path {}",
            launched_exe.display()
        )
    })?;
    let launched_dir = launched_exe
        .parent()
        .context("failed to resolve executable directory")?;
    let real_dir = real_exe
        .parent()
        .context("failed to resolve real executable directory")?;
    let resolved_launched_dir = fs::canonicalize(launched_dir).with_context(|| {
        format!(
            "failed to resolve executable directory {}",
            launched_dir.display()
        )
    })?;

    Ok(config_paths_from_resolved_dirs(
        launched_dir,
        &resolved_launched_dir,
        real_dir,
    ))
}

pub(crate) fn config_paths_from_resolved_dirs(
    launched_dir: &Path,
    resolved_launched_dir: &Path,
    real_dir: &Path,
) -> ConfigPaths {
    ConfigPaths {
        active: real_dir.join(CONFIG_FILE_NAME),
        legacy: (resolved_launched_dir != real_dir).then(|| launched_dir.join(CONFIG_FILE_NAME)),
    }
}

pub(crate) fn migrate_legacy_config(paths: &ConfigPaths) -> Result<()> {
    let Some(legacy_path) = paths.legacy.as_deref() else {
        return Ok(());
    };
    if paths.active.exists() || !legacy_path.exists() {
        return Ok(());
    }

    match fs::rename(legacy_path, &paths.active) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(legacy_path, &paths.active).with_context(|| {
                format!(
                    "failed to copy legacy config {} to {}",
                    legacy_path.display(),
                    paths.active.display()
                )
            })?;
            if let Err(error) = fs::remove_file(legacy_path) {
                let _ = fs::remove_file(&paths.active);
                return Err(error).with_context(|| {
                    format!(
                        "failed to remove migrated legacy config {}",
                        legacy_path.display()
                    )
                });
            }
            Ok(())
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to move legacy config {} to {}",
                legacy_path.display(),
                paths.active.display()
            )
        }),
    }
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

pub(crate) fn prepare_app_config(config: &mut AppConfig) {
    let legacy_state = legacy_investigation_state(config);
    let legacy_startup = match config.tracking.startup {
        TrackedListStartup::ResumeLast => InvestigationStartup::ResumeLast,
        TrackedListStartup::ChooseList => InvestigationStartup::ChooseProfile,
        TrackedListStartup::StartEmpty => InvestigationStartup::StartEmpty,
    };
    let legacy_active_list = config.tracking.active_list.clone();
    let existing_investigation = config.investigation.take();
    if let Some(investigation) = existing_investigation.as_ref() {
        migrate_legacy_investigation_preferences(config, &investigation.last);
    }
    normalize_global_preferences(config);

    let mut profiles =
        normalize_saved_investigation_profiles(std::mem::take(&mut config.investigation_profiles));
    let mut migrated_names = Vec::<(String, String)>::new();
    for list in normalize_saved_tracked_lists(std::mem::take(&mut config.tracked_lists)) {
        let migrated_name = unique_migrated_profile_name(&list.name, &profiles);
        let mut investigation = legacy_state.clone();
        investigation.tracked_names = list.processes;
        profiles.push(SavedInvestigationProfile {
            name: migrated_name.clone(),
            investigation: normalize_investigation_state(investigation),
        });
        migrated_names.push((list.name, migrated_name));
    }

    let mut investigation = existing_investigation.unwrap_or_else(|| {
        let active_profile = legacy_active_list.as_deref().and_then(|active| {
            migrated_names
                .iter()
                .find(|(legacy, _)| legacy.eq_ignore_ascii_case(active))
                .map(|(_, migrated)| migrated.clone())
        });
        InvestigationConfig {
            startup: legacy_startup,
            active_profile,
            last: if legacy_startup == InvestigationStartup::StartEmpty {
                InvestigationStateConfig::default()
            } else {
                legacy_state
            },
        }
    });
    investigation.last = normalize_investigation_state(investigation.last);
    investigation.active_profile = investigation
        .active_profile
        .map(|name| name.trim().to_string())
        .filter(|name| {
            profiles
                .iter()
                .any(|profile| profile.name.eq_ignore_ascii_case(name))
        });
    config.investigation = Some(investigation);
    config.investigation_profiles = profiles;
}

pub(crate) fn build_runtime_config(mut config: AppConfig) -> Result<RuntimeConfig> {
    prepare_app_config(&mut config);
    let mut investigation = config
        .investigation
        .take()
        .expect("prepared config must contain an investigation");
    let state = match investigation.startup {
        InvestigationStartup::StartEmpty => {
            investigation.active_profile = None;
            InvestigationStateConfig::default()
        }
        InvestigationStartup::ResumeLast => {
            investigation.active_profile = None;
            normalize_investigation_state(investigation.last)
        }
        InvestigationStartup::ChooseProfile => normalize_investigation_state(investigation.last),
    };
    let process_columns = parse_columns(
        config
            .process_table
            .columns
            .as_deref()
            .expect("prepared config must contain process columns"),
    )
    .unwrap_or_else(|| ColumnPreset::Default.effective_columns().to_vec());
    let column_preset = matching_column_preset(&process_columns);
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
    Ok(RuntimeConfig {
        mouse: config.general.mouse,
        config_path: None,
        recording_last_dir: config.recording.last_dir,
        initial_theme: config.general.theme,
        initial_high_contrast: config.general.high_contrast,
        initial_graph_slot_layout: match config.graphs.columns.unwrap_or_default() {
            1 => GraphSlotLayout::OneColumn,
            2 => GraphSlotLayout::TwoColumns,
            3 => GraphSlotLayout::ThreeColumns,
            _ => GraphSlotLayout::Auto,
        },
        initial_graph_time_span_seconds: config.graphs.time_span_seconds.unwrap_or(60),
        initial_graph_y_axis_zero_min: config.graphs.y_axis_zero_min.unwrap_or(true),
        initial_show_samples_panel: config.graphs.samples.unwrap_or(true),
        initial_show_sample_delta: config.graphs.delta.unwrap_or(true),
        initial_recording_interval_seconds: config.recording.interval_seconds.unwrap_or(1),
        column_preset,
        process_columns,
        process_column_widths,
        sort: SortSpec {
            column: config
                .process_table
                .sort_by
                .as_deref()
                .unwrap_or(MetricColumn::WorksetPrivateBytes.label())
                .parse()
                .unwrap_or(SortColumn::Metric(MetricColumn::WorksetPrivateBytes)),
            direction: config
                .process_table
                .sort_order
                .as_deref()
                .unwrap_or(SortDirection::Desc.label())
                .parse()
                .unwrap_or(SortDirection::Desc),
        },
        initial_tracked_only: config.process_table.tracked_only.unwrap_or(false),
        initial_process_view_mode: config
            .process_table
            .view
            .as_deref()
            .unwrap_or(ProcessViewMode::Flat.label())
            .parse()
            .unwrap_or(ProcessViewMode::Flat),
        initial_process_panel_height: process_panel_height(config.process_table.body_rows),
        process_filters: state.tracked_names,
        investigation_startup: investigation.startup,
        active_investigation_profile: investigation.active_profile,
        saved_investigation_profiles: config.investigation_profiles,
        sampling_options: SamplingOptions {
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
            high_contrast: app.high_contrast,
        },
        graphs: GraphConfig {
            columns: Some(app.graph_slot_layout.columns()),
            time_span_seconds: Some(app.graph_time_span_seconds.clamp(60, 7_200)),
            samples: Some(app.show_samples_panel),
            delta: Some(app.show_sample_delta),
            y_axis_zero_min: Some(app.graph_y_axis_zero_min),
        },
        process_table: ProcessTableConfig {
            view: Some(app.process_view_mode.label().to_string()),
            preset: None,
            columns: Some(
                app.process_columns
                    .iter()
                    .map(|column| column.label().to_string())
                    .collect(),
            ),
            sort_by: Some(app.sort.column.label().to_string()),
            sort_order: Some(app.sort.direction.label().to_string()),
            tracked_only: Some(app.watch_enabled),
            body_rows: match app.process_panel_height {
                ProcessPanelHeight::Auto => ProcessPanelHeightConfig::default(),
                ProcessPanelHeight::Manual(rows) => {
                    ProcessPanelHeightConfig::Rows(i64::from(rows.max(1)))
                }
            },
            column_widths: app
                .process_column_widths
                .overrides()
                .map(|(column, width)| (column.label().to_string(), i64::from(width)))
                .collect(),
        },
        recording: RecordingConfig {
            last_dir: app.recording_last_dir.clone(),
            interval_seconds: Some(app.selected_recording_interval_seconds()),
        },
        tracking: TrackingConfig::default(),
        tracked: Vec::new(),
        tracked_lists: Vec::new(),
        investigation: Some(InvestigationConfig {
            startup: app.runtime.investigation_startup,
            active_profile: app.active_investigation_profile.clone(),
            last: app.capture_investigation_state(),
        }),
        investigation_profiles: normalize_saved_investigation_profiles(
            app.runtime.saved_investigation_profiles.clone(),
        ),
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

fn normalize_saved_investigation_profiles(
    profiles: Vec<SavedInvestigationProfile>,
) -> Vec<SavedInvestigationProfile> {
    let mut normalized = Vec::<SavedInvestigationProfile>::new();
    for mut profile in profiles {
        profile.name = profile.name.trim().to_string();
        if profile.name.is_empty()
            || normalized
                .iter()
                .any(|saved| saved.name.eq_ignore_ascii_case(&profile.name))
        {
            continue;
        }
        profile.investigation = normalize_investigation_state(profile.investigation);
        normalized.push(profile);
    }
    normalized
}

fn legacy_investigation_state(config: &AppConfig) -> InvestigationStateConfig {
    normalize_investigation_state(InvestigationStateConfig {
        tracked_names: config
            .tracked
            .iter()
            .map(|tracked| tracked.name.clone())
            .collect(),
        ..InvestigationStateConfig::default()
    })
}

fn migrate_legacy_investigation_preferences(
    config: &mut AppConfig,
    legacy: &InvestigationStateConfig,
) {
    if config.process_table.tracked_only.is_none() {
        config.process_table.tracked_only = legacy.tracked_only;
    }
    if config.process_table.view.is_none() {
        config.process_table.view = legacy.process_view.clone();
    }
    if config.process_table.columns.is_none() {
        config.process_table.columns = legacy.process_columns.clone();
    }
    if config.process_table.sort_by.is_none() {
        config.process_table.sort_by = legacy.sort_by.clone();
    }
    if config.process_table.sort_order.is_none() {
        config.process_table.sort_order = legacy.sort_order.clone();
    }
    if config.graphs.columns.is_none() {
        config.graphs.columns = legacy.graph_columns;
    }
    if config.graphs.time_span_seconds.is_none() {
        config.graphs.time_span_seconds = legacy.graph_time_span_seconds;
    }
    if config.graphs.samples.is_none() {
        config.graphs.samples = legacy.samples;
    }
    if config.graphs.delta.is_none() {
        config.graphs.delta = legacy.delta;
    }
    if config.graphs.y_axis_zero_min.is_none() {
        config.graphs.y_axis_zero_min = legacy.y_axis_zero_min;
    }
    if config.recording.interval_seconds.is_none() {
        config.recording.interval_seconds = legacy.recording_interval_seconds;
    }
}

fn normalize_global_preferences(config: &mut AppConfig) {
    let preset = config
        .process_table
        .preset
        .as_deref()
        .and_then(|value| value.parse::<ColumnPreset>().ok())
        .unwrap_or(ColumnPreset::Default);
    let columns = config
        .process_table
        .columns
        .take()
        .and_then(|columns| parse_columns(&columns))
        .unwrap_or_else(|| preset.effective_columns().to_vec());
    config.process_table.columns = Some(
        columns
            .iter()
            .map(|column| column.label().to_string())
            .collect(),
    );
    config.process_table.view = Some(
        config
            .process_table
            .view
            .as_deref()
            .and_then(|value| value.parse::<ProcessViewMode>().ok())
            .unwrap_or(ProcessViewMode::Flat)
            .label()
            .to_string(),
    );
    config.process_table.sort_by = Some(
        config
            .process_table
            .sort_by
            .as_deref()
            .and_then(|value| value.parse::<SortColumn>().ok())
            .and_then(|column| match column {
                SortColumn::Metric(metric) if !metric.is_selectable() => None,
                _ => Some(column.label().to_string()),
            })
            .unwrap_or_else(|| MetricColumn::WorksetPrivateBytes.label().to_string()),
    );
    config.process_table.sort_order = Some(
        config
            .process_table
            .sort_order
            .as_deref()
            .and_then(|value| value.parse::<SortDirection>().ok())
            .unwrap_or(SortDirection::Desc)
            .label()
            .to_string(),
    );
    config.process_table.tracked_only = Some(config.process_table.tracked_only.unwrap_or(false));

    config.graphs.columns = Some(match config.graphs.columns.unwrap_or_default() {
        1..=3 => config.graphs.columns.unwrap_or_default(),
        _ => 0,
    });
    config.graphs.time_span_seconds = Some(
        config
            .graphs
            .time_span_seconds
            .unwrap_or(60)
            .clamp(60, 7_200),
    );
    config.graphs.samples = Some(config.graphs.samples.unwrap_or(true));
    config.graphs.delta = Some(config.graphs.delta.unwrap_or(true));
    config.graphs.y_axis_zero_min = Some(config.graphs.y_axis_zero_min.unwrap_or(true));

    let interval = config.recording.interval_seconds.unwrap_or(1);
    config.recording.interval_seconds = Some(if [1, 2, 5, 10].contains(&interval) {
        interval
    } else {
        1
    });
}

fn normalize_investigation_state(state: InvestigationStateConfig) -> InvestigationStateConfig {
    InvestigationStateConfig {
        tracked_names: dedupe_process_names(state.tracked_names),
        ..InvestigationStateConfig::default()
    }
}

fn unique_migrated_profile_name(
    legacy_name: &str,
    profiles: &[SavedInvestigationProfile],
) -> String {
    if !profiles
        .iter()
        .any(|profile| profile.name.eq_ignore_ascii_case(legacy_name))
    {
        return legacy_name.to_string();
    }
    let base = format!("{legacy_name} (Tracking List)");
    if !profiles
        .iter()
        .any(|profile| profile.name.eq_ignore_ascii_case(&base))
    {
        return base;
    }
    for suffix in 2_u32.. {
        let candidate = format!("{legacy_name} (Tracking List {suffix})");
        if !profiles
            .iter()
            .any(|profile| profile.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    unreachable!("profile suffix space exhausted")
}

fn matching_column_preset(columns: &[MetricColumn]) -> ColumnPreset {
    [
        ColumnPreset::Default,
        ColumnPreset::Memory,
        ColumnPreset::Resources,
        ColumnPreset::DotNet,
        ColumnPreset::Gpu,
        ColumnPreset::Io,
    ]
    .into_iter()
    .find(|preset| preset.effective_columns() == columns)
    .unwrap_or(ColumnPreset::Custom)
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

fn process_panel_height(config: ProcessPanelHeightConfig) -> ProcessPanelHeight {
    match config {
        ProcessPanelHeightConfig::Rows(rows) if (1..=i64::from(u16::MAX)).contains(&rows) => {
            ProcessPanelHeight::Manual(rows as u16)
        }
        ProcessPanelHeightConfig::Rows(_) | ProcessPanelHeightConfig::Mode(_) => {
            ProcessPanelHeight::Auto
        }
    }
}
