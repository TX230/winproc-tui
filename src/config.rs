use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::model::{ColumnPreset, MetricColumn, SortColumn, SortDirection, SortSpec};
use crate::samplers::SamplingOptions;

const CONFIG_FILE_NAME: &str = "winproc-tui.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) general: GeneralConfig,
    pub(crate) process_table: ProcessTableConfig,
    pub(crate) recording: RecordingConfig,
    #[serde(alias = "watch", alias = "process")]
    pub(crate) tracked: Vec<TrackedConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            process_table: ProcessTableConfig::default(),
            recording: RecordingConfig::default(),
            tracked: Vec::new(),
        }
    }
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
pub(crate) struct ProcessTableConfig {
    pub(crate) preset: String,
    pub(crate) columns: Vec<String>,
    pub(crate) sort_by: String,
    pub(crate) sort_order: String,
    pub(crate) tracked_only: bool,
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
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RecordingConfig {
    pub(crate) last_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TrackedConfig {
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeConfig {
    pub(crate) mouse: bool,
    pub(crate) recording_last_dir: Option<PathBuf>,
    pub(crate) initial_theme: String,
    pub(crate) column_preset: ColumnPreset,
    pub(crate) process_columns: Vec<MetricColumn>,
    pub(crate) sort: SortSpec,
    pub(crate) initial_tracked_only: bool,
    pub(crate) process_filters: Vec<String>,
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
    let process_filters = config.tracked.into_iter().map(|item| item.name).collect();

    Ok(RuntimeConfig {
        mouse: config.general.mouse,
        recording_last_dir: config.recording.last_dir,
        initial_theme: config.general.theme,
        column_preset,
        process_columns,
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
        },
        recording: RecordingConfig {
            last_dir: app.recording_last_dir.clone(),
        },
        tracked: app
            .watch_list
            .iter()
            .map(|name| TrackedConfig { name: name.clone() })
            .collect(),
    };
    let content = toml::to_string_pretty(&config)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn parse_columns(columns: &[String]) -> Option<Vec<MetricColumn>> {
    let parsed = columns
        .iter()
        .filter_map(|column| column.parse().ok())
        .filter(|column: &MetricColumn| column.is_selectable())
        .collect::<Vec<_>>();
    (!parsed.is_empty()).then_some(parsed)
}
