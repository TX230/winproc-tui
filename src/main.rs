use std::io::{self, Stdout};
#[cfg(test)]
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
#[cfg(test)]
use chrono::{Local, TimeZone};
use clap::Parser;
#[cfg(test)]
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(test)]
use ratatui::layout::Position;
#[cfg(test)]
use ratatui::style::Modifier;
use ratatui::{Terminal, backend::CrosstermBackend};
#[cfg(test)]
use ratatui::{backend::TestBackend, layout::Rect, widgets::TableState};
#[cfg(test)]
use winapi::shared::dxgi::{DXGI_ADAPTER_FLAG_REMOTE, DXGI_ADAPTER_FLAG_SOFTWARE};

mod app;
mod cli;
mod config;
mod model;
mod platform;
mod samplers;
mod startup;
mod ui;

pub(crate) use app::App;
use app::run_tui;
#[cfg(test)]
use app::{
    AppActivity, DetailsMetric, DetailsTarget, FocusedPanel, GraphSlot, GraphSlotLayout,
    GraphValueFormat, PROCESS_INFO_DEBOUNCE, QuitConfirmSelection, SAMPLE_STALE_AFTER_SECONDS,
    SampleFreshness, TrackedRemoveSelection, VisibleProcessEntry,
};
use cli::Cli;
#[cfg(test)]
use config::AppConfig;
#[cfg(test)]
use config::RuntimeConfig;
use config::{build_runtime_config, load_config, resolve_config_path, write_app_config};
#[cfg(test)]
use model::Snapshot;
#[cfg(test)]
use model::SystemCounterSample;
#[cfg(test)]
use model::{
    ColumnPreset, CpuCoreKind, CpuLogicalProcessorSample, GpuUsageSample, InfoValue, MetricColumn,
    ProcessColumnWidths, ProcessEnvironmentEntry, ProcessEnvironmentError,
    ProcessEnvironmentReport, ProcessIdentity, ProcessInfo, ProcessModuleEntry,
    ProcessModulesError, ProcessModulesReport, ProcessRow, SortColumn, SortDirection, SortSpec,
};
#[cfg(test)]
use model::{ProcessHistory, SystemHistory, SystemMetric};
#[cfg(test)]
use samplers::SampleRequest;
#[cfg(test)]
use samplers::gpu::is_filtered_dxgi_adapter;
#[cfg(test)]
use samplers::memory::map_memory_counters;
#[cfg(test)]
use samplers::open_files::{
    OpenFileEntry, OpenFilesError, OpenFilesReport, OpenFilesRequest, OpenFilesResult,
    OpenFilesWorker,
};
#[cfg(test)]
use samplers::pdh::map_process_counter_instances_to_pids;
#[cfg(test)]
use samplers::pdh::{normalize_process_cpu_percent, sum_optional_values};
#[cfg(test)]
use samplers::process::{working_set_page_is_shareable, working_set_page_is_shared};
#[cfg(test)]
use samplers::process_environment::{
    ProcessEnvironmentRequest, ProcessEnvironmentResult, ProcessEnvironmentWorker,
};
#[cfg(test)]
use samplers::process_info::{ProcessInfoRequest, ProcessInfoResult, ProcessInfoWorker};
#[cfg(test)]
use samplers::process_modules::{
    ProcessModulesRequest, ProcessModulesResult, ProcessModulesWorker,
};
#[cfg(test)]
use samplers::{CollectSnapshotResult, SamplingWorker};
#[cfg(test)]
use std::sync::mpsc::{self, TryRecvError};
#[cfg(test)]
use ui::layout::centered_rect;
#[cfg(test)]
use ui::{
    GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, THEMES, details_graph_area_for_app,
    details_samples_area_for_app, details_shared_controls_area_for_app, main_panel_areas,
    main_panel_areas_for_app, process_kill_button_at, process_kill_dialog_area,
    process_table_visible_column_count, screen_layout, tracked_remove_dialog_area,
};
#[cfg(test)]
use ui::{
    SummaryInfoStyle, optional_value_color, render_summary_info_line,
    render_summary_info_value_spans, render_summary_line,
};
#[cfg(test)]
use ui::{column_picker_area, column_picker_scrollbar_area, help_area, help_scrollbar_area};

fn main() -> Result<()> {
    Cli::parse();
    platform::install_console_control_handler()
        .context("failed to install console control handler")?;
    let config_path = resolve_config_path()?;

    let result = (|| {
        let mut config = load_config(&config_path)?;
        if config.tracking.startup == config::TrackedListStartup::ChooseList {
            let mouse_enabled = config.general.mouse;
            let mut terminal = setup_terminal(mouse_enabled)?;
            let choice_result = startup::choose_startup_tracked_list(&mut terminal, &mut config);
            let restore_result = restore_terminal(&mut terminal, mouse_enabled);
            restore_result?;
            choice_result?;
        }
        let mut runtime = build_runtime_config(config)?;
        runtime.config_path = Some(config_path.clone());
        let mut app = App::new(runtime)?;
        let mut terminal = setup_terminal(app.runtime.mouse)?;
        let run_result = run_tui(&mut terminal, &mut app);
        restore_terminal(&mut terminal, app.runtime.mouse)?;
        if run_result.is_ok() {
            write_app_config(&config_path, &app)?;
        }
        run_result
    })();
    platform::mark_shutdown_complete();
    result
}

fn setup_terminal(mouse_enabled: bool) -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_enabled {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("failed to create terminal")
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mouse_enabled: bool,
) -> Result<()> {
    disable_raw_mode()?;
    if mouse_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_runtime_config_uses_config_process_filters() {
        let mut config = AppConfig::default();
        config.tracked.push(config::TrackedConfig {
            name: "app.exe".to_string(),
        });

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.process_filters, vec!["app.exe"]);
    }

    #[test]
    fn build_runtime_config_restores_process_table_settings() {
        let mut config = AppConfig::default();
        config.process_table.preset = "Custom".to_string();
        config.process_table.columns = vec!["CPU %".to_string(), "Private".to_string()];
        config.process_table.sort_by = "CPU %".to_string();
        config.process_table.sort_order = "asc".to_string();
        config.process_table.tracked_only = true;

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.column_preset, ColumnPreset::Custom);
        assert_eq!(
            runtime.process_columns,
            vec![MetricColumn::CpuPercent, MetricColumn::PrivateBytes]
        );
        assert_eq!(
            runtime.sort,
            SortSpec {
                column: SortColumn::Metric(MetricColumn::CpuPercent),
                direction: SortDirection::Asc,
            }
        );
        assert!(runtime.initial_tracked_only);
    }

    #[test]
    fn default_runtime_config_selects_all_process_columns() {
        let runtime = build_runtime_config(AppConfig::default()).unwrap();

        assert_eq!(runtime.process_columns, MetricColumn::ALL);
        assert_eq!(
            runtime.process_column_widths.resolved(SortColumn::Pid),
            SortColumn::Pid.default_width()
        );
    }

    #[test]
    fn build_runtime_config_restores_and_clamps_column_width_overrides() {
        let mut config = AppConfig::default();
        config.process_table.column_widths.extend([
            ("PID".to_string(), -10),
            ("Process".to_string(), 999),
            ("Private".to_string(), 14),
            ("Full Path".to_string(), 60),
            ("WS Shrbl".to_string(), 40),
            ("Unknown".to_string(), 50),
        ]);

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.process_column_widths.resolved(SortColumn::Pid), 5);
        assert_eq!(
            runtime
                .process_column_widths
                .resolved(SortColumn::ProcessName),
            120
        );
        assert_eq!(
            runtime
                .process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            14
        );
        assert_eq!(
            runtime
                .process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::FullPath)),
            60
        );
        assert_eq!(
            runtime
                .process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::WorksetShareableBytes)),
            MetricColumn::WorksetShareableBytes.width()
        );
    }

    #[test]
    fn tracked_entries_do_not_enable_tracked_only_without_saved_state() {
        let mut config = AppConfig::default();
        config.tracked.push(config::TrackedConfig {
            name: "app.exe".to_string(),
        });

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.process_filters, vec!["app.exe"]);
        assert!(!runtime.initial_tracked_only);
    }

    #[test]
    fn build_runtime_config_falls_back_when_custom_columns_are_empty() {
        let mut config = AppConfig::default();
        config.process_table.preset = "Custom".to_string();
        config.process_table.columns.clear();

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.column_preset, ColumnPreset::Custom);
        assert_eq!(
            runtime.process_columns,
            ColumnPreset::Default.columns().to_vec()
        );
    }

    #[test]
    fn cli_rejects_removed_runtime_options() {
        let removed_args: &[&[&str]] = &[
            &["winproc-tui", "-c", "C:/work/winproc-tui.toml"],
            &["winproc-tui", "--config", "C:/work/winproc-tui.toml"],
            &["winproc-tui", "-p", "app.exe"],
            &["winproc-tui", "--process", "app.exe"],
            &["winproc-tui", "--preset", "io"],
            &["winproc-tui", "--no-mouse"],
            &["winproc-tui", "--interval", "5"],
            &["winproc-tui", "--ws-share"],
            &["winproc-tui", "--no-ws-share"],
            &["winproc-tui", "--no-gpu-metrics"],
            &["winproc-tui", "--no-gui-resources"],
            &["winproc-tui", "config"],
            &["winproc-tui", "config", "init"],
            &["winproc-tui", "config", "path"],
        ];

        for args in removed_args {
            assert!(Cli::try_parse_from(*args).is_err());
        }
    }

    #[test]
    fn build_runtime_config_restores_recording_last_dir() {
        let mut config = AppConfig::default();
        let last_dir = std::path::PathBuf::from("C:/reports/winproc-tui");
        config.recording.last_dir = Some(last_dir.clone());

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.recording_last_dir, Some(last_dir));
    }

    #[test]
    fn runtime_config_uses_no_recording_dir_by_default() {
        let config = AppConfig::default();

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.recording_last_dir, None);
    }

    #[test]
    fn build_runtime_config_restores_graph_view_settings() {
        let mut config = AppConfig::default();
        config.graphs.columns = 2;
        config.graphs.samples = false;
        config.graphs.delta = false;

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(
            runtime.initial_graph_slot_layout,
            GraphSlotLayout::TwoColumns
        );
        assert!(!runtime.initial_show_samples_panel);
        assert!(!runtime.initial_show_sample_delta);
    }

    #[test]
    fn invalid_graph_column_count_falls_back_to_auto() {
        let mut config = AppConfig::default();
        config.graphs.columns = 3;

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.initial_graph_slot_layout, GraphSlotLayout::Auto);
    }

    #[test]
    fn cli_rejects_export_dir_option() {
        let error = Cli::try_parse_from(["winproc-tui", "--export-dir", "C:/logs"]).unwrap_err();

        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn sampling_interval_is_fixed_to_one_second() {
        let app = make_test_app(1, 10);

        assert_eq!(app.tick_interval(), Duration::from_secs(1));
    }

    #[test]
    fn app_config_accepts_and_omits_legacy_interval_seconds() {
        let config: AppConfig = toml::from_str(
            r#"
[general]
interval_seconds = 30
mouse = false
theme = "Light"
"#,
        )
        .unwrap();

        assert!(!config.general.mouse);
        assert_eq!(config.general.theme, "Light");

        let rendered = toml::to_string(&config).unwrap();
        assert!(!rendered.contains("interval_seconds"), "{rendered}");
    }

    #[test]
    fn sample_freshness_turns_stale_at_defined_threshold() {
        let mut app = make_test_app(1, 10);
        let captured_at = app.snapshot.captured_at;

        assert_eq!(
            app.sample_freshness_at(
                captured_at + chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64 - 1)
            ),
            Some(SampleFreshness::Fresh)
        );
        assert_eq!(
            app.sample_freshness_at(
                captured_at + chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64)
            ),
            Some(SampleFreshness::Stale {
                age_seconds: SAMPLE_STALE_AFTER_SECONDS
            })
        );

        app.log_view_path = Some(std::path::PathBuf::from("session.log"));
        assert_eq!(app.sample_freshness_at(captured_at), None);
    }

    #[test]
    fn app_config_accepts_legacy_process_entries_as_watch_list() {
        let config: AppConfig = toml::from_str(
            r#"
[[process]]
name = "legacy.exe"
"#,
        )
        .unwrap();

        assert_eq!(config.tracked[0].name, "legacy.exe");
    }

    #[test]
    fn app_config_accepts_legacy_watch_entries_as_tracked_list() {
        let config: AppConfig = toml::from_str(
            r#"
[[watch]]
name = "legacy-watch.exe"
"#,
        )
        .unwrap();

        assert_eq!(config.tracked[0].name, "legacy-watch.exe");
    }

    #[test]
    fn app_config_saves_tracked_entries() {
        let mut config = AppConfig::default();
        config.tracked.push(config::TrackedConfig {
            name: "app.exe".to_string(),
        });

        let rendered = toml::to_string(&config).unwrap();

        assert!(rendered.contains("[[tracked]]"));
        assert!(!rendered.contains("[[watch]]"));
    }

    #[test]
    fn app_config_accepts_named_tracked_lists_and_startup_mode() {
        let config: AppConfig = toml::from_str(
            r#"
[tracking]
startup = "choose_list"
active_list = "API"

[[tracked_lists]]
name = "API"
processes = ["api.exe", "worker.exe"]
"#,
        )
        .unwrap();

        assert_eq!(
            config.tracking.startup,
            config::TrackedListStartup::ChooseList
        );
        assert_eq!(config.tracking.active_list.as_deref(), Some("API"));
        assert_eq!(config.tracked_lists[0].name, "API");
        assert_eq!(
            config.tracked_lists[0].processes,
            vec!["api.exe", "worker.exe"]
        );
    }

    #[test]
    fn start_empty_runtime_ignores_last_working_tracked_list() {
        let mut config = AppConfig::default();
        config.tracking.startup = config::TrackedListStartup::StartEmpty;
        config.tracking.active_list = Some("API".to_string());
        config.tracked.push(config::TrackedConfig {
            name: "api.exe".to_string(),
        });

        let runtime = build_runtime_config(config).unwrap();

        assert!(runtime.process_filters.is_empty());
        assert_eq!(runtime.active_tracked_list, None);
    }

    #[test]
    fn runtime_normalizes_named_tracked_lists_case_insensitively() {
        let mut config = AppConfig::default();
        config.tracked_lists = vec![
            config::SavedTrackedList {
                name: " API ".to_string(),
                processes: vec![
                    "api.exe".to_string(),
                    "API.EXE".to_string(),
                    " worker.exe ".to_string(),
                ],
            },
            config::SavedTrackedList {
                name: "api".to_string(),
                processes: vec!["duplicate.exe".to_string()],
            },
            config::SavedTrackedList {
                name: " empty (DEFAULT) ".to_string(),
                processes: Vec::new(),
            },
        ];

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.saved_tracked_lists.len(), 1);
        assert_eq!(runtime.saved_tracked_lists[0].name, "API");
        assert_eq!(
            runtime.saved_tracked_lists[0].processes,
            vec!["api.exe", "worker.exe"]
        );
    }

    #[test]
    fn runtime_does_not_restore_builtin_empty_as_a_named_list() {
        let mut config = AppConfig::default();
        config.tracking.active_list = Some("Empty (default)".to_string());
        config.tracked_lists = vec![config::SavedTrackedList {
            name: "Empty (default)".to_string(),
            processes: Vec::new(),
        }];

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.active_tracked_list, None);
        assert!(runtime.saved_tracked_lists.is_empty());
    }

    #[test]
    fn app_config_saves_named_tracked_lists_and_startup_mode() {
        let mut app = make_test_app(1, 10);
        app.runtime.tracked_list_startup = config::TrackedListStartup::ChooseList;
        app.runtime.active_tracked_list = Some("API".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        let path = unique_config_path("named-tracked-lists");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(rendered.contains("startup = \"choose_list\""), "{rendered}");
        assert!(rendered.contains("active_list = \"API\""), "{rendered}");
        assert!(rendered.contains("[[tracked_lists]]"), "{rendered}");
        assert!(rendered.contains("processes = [\"api.exe\"]"), "{rendered}");
    }

    #[test]
    fn app_config_never_writes_builtin_empty_as_a_named_list() {
        let mut app = make_test_app(1, 10);
        app.runtime.active_tracked_list = Some("Empty (default)".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "Empty (default)".to_string(),
            processes: Vec::new(),
        }];
        let path = unique_config_path("builtin-empty-tracked-list");

        config::write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(!rendered.contains("Empty (default)"), "{rendered}");
        assert!(!rendered.contains("[[tracked_lists]]"), "{rendered}");
    }

    #[test]
    fn app_config_saves_tracked_only_state() {
        let mut app = make_test_app(3, 10);
        app.watch_enabled = true;
        let path = unique_config_path("tracked-only");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(rendered.contains("tracked_only = true"), "{rendered}");
    }

    #[test]
    fn app_config_saves_graph_layout_and_explicit_samples_preference() {
        let mut app = make_test_app(3, 10);
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = false;
        app.show_sample_delta = false;
        let path = unique_config_path("graph-layout");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(rendered.contains("[graphs]"), "{rendered}");
        assert!(rendered.contains("columns = 2"), "{rendered}");
        assert!(rendered.contains("samples = false"), "{rendered}");
        assert!(rendered.contains("delta = false"), "{rendered}");
    }

    #[test]
    fn app_config_saves_auto_graph_layout() {
        let app = make_test_app(3, 10);
        let path = unique_config_path("graph-layout-auto");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(rendered.contains("[graphs]"), "{rendered}");
        assert!(rendered.contains("columns = 0"), "{rendered}");
    }

    #[test]
    fn app_config_round_trips_only_non_default_column_widths() {
        let mut app = make_test_app(3, 10);
        app.process_column_widths.set(SortColumn::Pid, 8);
        app.process_column_widths
            .set(SortColumn::Metric(MetricColumn::PrivateBytes), 14);
        app.process_column_widths.set(
            SortColumn::ProcessName,
            SortColumn::ProcessName.default_width(),
        );
        let path = unique_config_path("column-widths");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let loaded = load_config(&path).unwrap();
        let runtime = build_runtime_config(loaded).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(
            rendered.contains("[process_table.column_widths]"),
            "{rendered}"
        );
        assert!(rendered.contains("PID = 8"), "{rendered}");
        assert!(rendered.contains("Private = 14"), "{rendered}");
        assert!(!rendered.contains("Process = 18"), "{rendered}");
        assert_eq!(runtime.process_column_widths.resolved(SortColumn::Pid), 8);
        assert_eq!(
            runtime
                .process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            14
        );
    }

    #[test]
    fn app_config_does_not_save_filter_state() {
        let mut app = make_test_app(3, 10);
        app.filter_text = "proc".to_string();
        let path = unique_config_path("filter");

        write_app_config(&path, &app).unwrap();
        let rendered = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(!rendered.contains("[filter]"), "{rendered}");
        assert!(!rendered.contains("initial ="), "{rendered}");
        assert!(!rendered.contains("initial = \"proc\""), "{rendered}");
    }

    #[test]
    fn app_config_saves_recording_last_dir() {
        let mut config = AppConfig::default();
        config.recording.last_dir = Some(std::path::PathBuf::from("C:/logs"));

        let rendered = toml::to_string(&config).unwrap();

        assert!(rendered.contains("[recording]"));
        assert!(rendered.contains("last_dir"));
    }

    #[test]
    fn map_memory_counters_uses_real_commit_values() {
        let mapped = map_memory_counters(
            32_000,
            12_000,
            Ok(Some(SystemCounterSample {
                available_memory: 10_000,
                committed_memory: 9_000,
                commit_limit: 24_000,
                cache_bytes: Some(1_000),
                standby_cache_bytes: Some(2_000),
                disk_read_bytes_per_sec: Some(3_000),
                disk_write_bytes_per_sec: Some(4_000),
                disk_queue_length: Some(1.5),
                network_received_bytes_per_sec: Some(5_000),
                network_sent_bytes_per_sec: Some(6_000),
                cpu_frequencies_mhz: Vec::new(),
            })),
        );

        assert_eq!(mapped.available_memory, 10_000);
        assert_eq!(mapped.committed_memory, Some(9_000));
        assert_eq!(mapped.commit_limit, Some(24_000));
        assert_eq!(mapped.cache_bytes, Some(1_000));
        assert_eq!(mapped.standby_cache_bytes, Some(2_000));
        assert_eq!(mapped.disk_read_bytes_per_sec, Some(3_000));
        assert_eq!(mapped.disk_write_bytes_per_sec, Some(4_000));
        assert_eq!(mapped.disk_queue_length, Some(1.5));
        assert_eq!(mapped.network_received_bytes_per_sec, Some(5_000));
        assert_eq!(mapped.network_sent_bytes_per_sec, Some(6_000));
        assert_eq!(mapped.warning, None);
    }

    #[test]
    fn map_memory_counters_drops_commit_fields_on_failure() {
        let mapped = map_memory_counters(32_000, 12_000, Err(anyhow::anyhow!("pdh failed")));

        assert_eq!(mapped.available_memory, 12_000);
        assert_eq!(mapped.committed_memory, None);
        assert_eq!(mapped.commit_limit, None);
        assert_eq!(mapped.cache_bytes, None);
        assert_eq!(mapped.standby_cache_bytes, None);
        assert_eq!(mapped.disk_read_bytes_per_sec, None);
        assert_eq!(mapped.disk_write_bytes_per_sec, None);
        assert_eq!(mapped.disk_queue_length, None);
        assert_eq!(mapped.network_received_bytes_per_sec, None);
        assert_eq!(mapped.network_sent_bytes_per_sec, None);
        assert!(
            mapped
                .warning
                .unwrap()
                .contains("commit counters unavailable")
        );
    }

    #[test]
    fn optional_value_color_uses_presence_not_magnitude() {
        assert_eq!(optional_value_color(Some(0), THEMES[0]), THEMES[0].text);
        assert_eq!(optional_value_color(Some(999), THEMES[0]), THEMES[0].text);
        assert_eq!(optional_value_color(None, THEMES[0]), THEMES[0].muted);
    }

    #[test]
    fn render_summary_info_value_spans_separates_numbers_from_units() {
        let spans = render_summary_info_value_spans("2.11 GHz / 930.43 GiB (97%)", THEMES[0]);
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec!["2.11", " GHz / ", "930.43", " GiB (", "97", "%)"]
        );
        assert_eq!(spans[0].style.fg, Some(THEMES[0].text));
        assert_eq!(spans[1].style.fg, Some(THEMES[0].muted));
    }

    #[test]
    fn render_summary_info_value_spans_keeps_comma_numbers_together() {
        let spans = render_summary_info_value_spans("C: 861/999 GB, X: 400/2,000 GB", THEMES[0]);
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "C: ", "861", "/", "999", " GB, X: ", "400", "/", "2,000", " GB"
            ]
        );
        assert_eq!(spans[7].style.fg, Some(THEMES[0].text));
    }

    #[test]
    fn render_summary_info_value_spans_keeps_cache_labels_as_text() {
        let spans =
            render_summary_info_value_spans("L1 1.00 MiB  L2 12.00 MiB  L3 25.00 MiB", THEMES[0]);
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "L1 ",
                "1.00",
                " MiB  L2 ",
                "12.00",
                " MiB  L3 ",
                "25.00",
                " MiB"
            ]
        );
        assert_eq!(spans[0].style.fg, Some(THEMES[0].muted));
        assert_eq!(spans[1].style.fg, Some(THEMES[0].text));
    }

    #[test]
    fn render_summary_line_formats_percent_in_parentheses() {
        let line = render_summary_line(
            "Physical Memory",
            Some(12_345_600_000),
            Some(24_691_200_000),
            None,
            THEMES[0],
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>();
        let joined = rendered.join("");

        assert!(joined.contains("12,346 MB / 24,691 MB"));
        assert!(joined.contains("( 50%)"));
        assert_eq!(line.spans[0].style.fg, Some(THEMES[0].muted));
    }

    #[test]
    fn render_summary_info_line_keeps_identity_values_plain() {
        let line = render_summary_info_line(
            "GPU",
            "NVIDIA GeForce RTX 3070 Ti",
            SummaryInfoStyle::Plain,
            THEMES[0],
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["GPU     ", "NVIDIA GeForce RTX 3070 Ti"]);
        assert_eq!(line.spans[0].style.fg, Some(THEMES[0].muted));
        assert_eq!(line.spans[1].style.fg, Some(THEMES[0].text));
    }

    #[test]
    fn process_page_size_uses_full_height_without_graphs_and_caps_with_graphs() {
        assert_eq!(
            main_panel_areas(Rect::new(0, 0, 120, 40), false, 30, false)
                .processes
                .page_size,
            27
        );
        assert_eq!(
            main_panel_areas(Rect::new(0, 0, 120, 60), true, 30, false)
                .processes
                .page_size,
            10
        );
    }

    #[test]
    fn process_navigation_moves_up_after_overflowing_down() {
        let mut app = make_test_app(30, 10);
        app.move_selection_down(20);
        assert_eq!(app.process_table_state.selected(), Some(20));
        assert_eq!(app.process_table_state.offset(), 11);

        app.move_selection_up(1);
        assert_eq!(app.process_table_state.selected(), Some(19));
        assert_eq!(app.process_table_state.offset(), 11);
    }

    #[test]
    fn process_navigation_page_moves_by_visible_rows() {
        let mut app = make_test_app(30, 10);
        app.move_selection_down(app.process_page_size);
        assert_eq!(app.process_table_state.selected(), Some(10));
        assert_eq!(app.process_table_state.offset(), 1);

        app.move_selection_up(app.process_page_size);
        assert_eq!(app.process_table_state.selected(), Some(0));
        assert_eq!(app.process_table_state.offset(), 0);
    }

    #[test]
    fn process_navigation_home_and_end_jump_to_bounds() {
        let mut app = make_test_app(30, 10);
        app.select_last_row();
        assert_eq!(app.process_table_state.selected(), Some(29));
        assert_eq!(app.process_table_state.offset(), 20);

        app.select_first_row();
        assert_eq!(app.process_table_state.selected(), Some(0));
        assert_eq!(app.process_table_state.offset(), 0);
    }

    #[test]
    fn process_shift_up_down_selects_live_row_range() {
        let mut app = make_test_app(5, 10);

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(1));
        assert_eq!(app.selected_process_identities_count(), 2);
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[0]
                ))
        );
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[1]
                ))
        );

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(app.selected_process_identities_count(), 0);
    }

    #[test]
    fn normal_process_navigation_does_not_keep_multi_selection_anchor() {
        let mut app = make_test_app(5, 10);

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(1));
        assert!(app.process_selection_anchor.is_none());
        assert_eq!(app.selected_process_identities_count(), 0);

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(app.selected_process_identities_count(), 2);
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[1]
                ))
        );
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[2]
                ))
        );
    }

    #[test]
    #[ignore = "manual performance probe; run with --ignored --nocapture"]
    fn perf_process_cursor_navigation_and_refresh_frames() {
        fn summarize(label: &str, durations: &[Duration]) {
            let mut micros = durations
                .iter()
                .map(|duration| duration.as_micros() as u64)
                .collect::<Vec<_>>();
            micros.sort_unstable();
            let percentile = |percent: usize| -> u64 {
                let index = micros.len().saturating_sub(1).saturating_mul(percent) / 100;
                micros[index]
            };
            let avg = micros.iter().sum::<u64>() / micros.len().max(1) as u64;
            println!(
                "{label}: avg={}us p50={}us p95={}us p99={}us max={}us",
                avg,
                percentile(50),
                percentile(95),
                percentile(99),
                micros.last().copied().unwrap_or(0)
            );
        }

        let screen = Rect::new(0, 0, 100, 45);
        let page_size = main_panel_areas(screen, false, 1_000, false)
            .processes
            .page_size;

        for row_count in [120usize, 1_000usize] {
            let mut app = make_test_app(row_count, page_size);
            app.focused_panel = FocusedPanel::Processes;
            app.set_screen_area(screen);
            let backend = TestBackend::new(screen.width, screen.height);
            let mut terminal = Terminal::new(backend).expect("test terminal should be created");
            terminal
                .draw(|frame| ui::draw(frame, &app))
                .expect("warmup render should succeed");

            let mut moving_down = true;
            let mut frame_durations = Vec::new();
            for _ in 0..300 {
                let selected = app.process_table_state.selected().unwrap_or(0);
                if selected >= row_count.saturating_sub(1) {
                    moving_down = false;
                } else if selected == 0 {
                    moving_down = true;
                }
                let key = if moving_down {
                    KeyCode::Down
                } else {
                    KeyCode::Up
                };
                let start = Instant::now();
                app.on_key(KeyEvent::new(key, KeyModifiers::NONE))
                    .expect("navigation should succeed");
                terminal
                    .draw(|frame| ui::draw(frame, &app))
                    .expect("render should succeed");
                frame_durations.push(start.elapsed());
            }
            summarize(&format!("cursor+render rows={row_count}"), &frame_durations);

            let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
            let mut app = make_test_app_with_worker(row_count, page_size, sampling_worker);
            app.focused_panel = FocusedPanel::Processes;
            app.set_screen_area(screen);
            let backend = TestBackend::new(screen.width, screen.height);
            let mut terminal = Terminal::new(backend).expect("test terminal should be created");
            terminal
                .draw(|frame| ui::draw(frame, &app))
                .expect("warmup render should succeed");

            let snapshots = (0..40)
                .map(|index| {
                    let mut snapshot = test_snapshot(row_count);
                    snapshot.captured_at =
                        app.snapshot.captured_at + chrono::Duration::seconds(index + 1);
                    CollectSnapshotResult {
                        snapshot,
                        warning: None,
                    }
                })
                .collect::<Vec<_>>();
            let mut refresh_durations = Vec::new();
            for sample in snapshots {
                app.sampling_in_progress = true;
                result_tx.send(sample).unwrap();
                let start = Instant::now();
                app.poll_sample_results()
                    .expect("sample poll should succeed");
                terminal
                    .draw(|frame| ui::draw(frame, &app))
                    .expect("render should succeed");
                refresh_durations.push(start.elapsed());
            }
            summarize(
                &format!("sample+render rows={row_count}"),
                &refresh_durations,
            );
        }
    }

    #[test]
    fn process_ctrl_space_toggles_discontiguous_live_rows() {
        let mut app = make_test_app(5, 10);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(app.selected_process_identities_count(), 2);
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[0]
                ))
        );
        assert!(
            app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[2]
                ))
        );

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.selected_process_identities_count(), 1);
        assert!(
            !app.selected_process_identities
                .contains(&model::ProcessIdentity::from_row(
                    &app.snapshot.processes[2]
                ))
        );
    }

    #[test]
    fn process_kill_confirmation_uses_multi_selection_and_distinct_image_names() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "same.exe".to_string();
        app.snapshot.processes[1].name = "same.exe".to_string();
        app.snapshot.processes[2].name = "other.exe".to_string();
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();

        assert!(app.request_process_kill_confirmation());
        assert!(app.show_process_kill_confirmation);
        assert_eq!(app.process_kill_targets.len(), 3);
        assert_eq!(
            app::distinct_process_kill_image_names(&app.process_kill_targets),
            vec!["same.exe".to_string(), "other.exe".to_string()]
        );
    }

    #[test]
    fn process_kill_confirmation_dialog_is_compact_and_clickable() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.snapshot.processes[0].name = "msedge.exe".to_string();
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();

        assert!(app.request_process_kill_confirmation());

        let screen = Rect::new(0, 0, 100, 45);
        let popup = process_kill_dialog_area(screen);
        assert_eq!(popup.width, 64);
        assert_eq!(popup.height, 11);

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        assert!(
            find_text_position(&buffer, "Enter Select  Esc Cancel  y Kill").is_some(),
            "process-kill shortcuts should follow footer formatting"
        );
        let (kill_x, kill_y) =
            find_text_position(&buffer, "[ Kill ]").expect("kill button should render");

        assert!(kill_y < popup.bottom());
        assert_eq!(
            process_kill_button_at(screen, kill_x + 2, kill_y),
            Some(app::ProcessKillSelection::Kill)
        );
    }

    #[test]
    fn process_navigation_clamps_after_refresh_shrink() {
        let mut app = make_test_app(30, 10);
        app.select_last_row();
        app.snapshot.processes.truncate(5);
        app.snapshot.process_count = 5;
        app.rebuild_visible_process_cache();

        app.clamp_process_table_state();

        assert_eq!(app.process_table_state.selected(), Some(4));
        assert_eq!(app.process_table_state.offset(), 0);
    }

    #[test]
    fn process_filter_matches_names_incrementally() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.snapshot.processes[2].name = "CARGO-watch.exe".to_string();

        app.begin_filter_edit();
        app.push_filter_char('c');
        app.push_filter_char('a');
        app.push_filter_char('r');

        let visible = app
            .visible_processes()
            .into_iter()
            .map(|process| process.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec!["cargo.exe", "CARGO-watch.exe"]);
    }

    #[test]
    fn process_filter_matches_paths_only_when_full_path_column_is_selected() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "app.exe".to_string();
        app.snapshot.processes[0].executable_path = Some(r"C:\work\alpha\app.exe".to_string());
        app.snapshot.processes[1].name = "app.exe".to_string();
        app.snapshot.processes[1].executable_path = Some(r"C:\work\beta\app.exe".to_string());

        app.begin_filter_edit();
        app.push_filter_char('b');
        app.push_filter_char('e');
        app.push_filter_char('t');
        app.push_filter_char('a');

        assert!(app.visible_processes().is_empty());

        app.process_columns.push(MetricColumn::FullPath);
        app.rebuild_visible_process_cache();

        let visible = app
            .visible_processes()
            .into_iter()
            .map(|process| process.executable_path.as_deref())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec![Some(r"C:\work\beta\app.exe")]);
    }

    #[test]
    fn column_picker_full_path_toggle_rebuilds_active_filter_matches() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "app.exe".to_string();
        app.snapshot.processes[0].executable_path = Some(r"C:\work\alpha\app.exe".to_string());
        app.snapshot.processes[1].name = "app.exe".to_string();
        app.snapshot.processes[1].executable_path = Some(r"C:\work\beta\app.exe".to_string());

        app.begin_filter_edit();
        for ch in "beta".chars() {
            app.push_filter_char(ch);
        }
        assert!(app.visible_processes().is_empty());

        app.column_picker_index = MetricColumn::ALL
            .iter()
            .position(|column| *column == MetricColumn::FullPath)
            .unwrap();
        app.toggle_picker_column();

        let visible = app
            .visible_processes()
            .into_iter()
            .map(|process| process.executable_path.as_deref())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec![Some(r"C:\work\beta\app.exe")]);
    }

    #[test]
    fn visible_process_window_returns_only_requested_rows() {
        let app = make_test_app(10, 10);

        let rows = app
            .visible_process_window(3, 4)
            .into_iter()
            .map(|(index, process)| (index, process.pid))
            .collect::<Vec<_>>();

        assert_eq!(rows, vec![(3, 3), (4, 4), (5, 5), (6, 6)]);
    }

    #[test]
    fn process_filter_clamps_selection_to_visible_rows() {
        let mut app = make_test_app(4, 10);
        app.snapshot.processes[0].name = "alpha.exe".to_string();
        app.snapshot.processes[1].name = "beta.exe".to_string();
        app.snapshot.processes[2].name = "gamma.exe".to_string();
        app.snapshot.processes[3].name = "delta.exe".to_string();
        app.select_last_row();

        app.begin_filter_edit();
        app.push_filter_char('a');
        app.push_filter_char('l');

        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(app.process_table_state.selected(), Some(0));
        assert_eq!(app.process_table_state.offset(), 0);
    }

    #[test]
    fn process_filter_editing_blocks_row_navigation_keys() {
        let mut app = make_test_app(20, 5);
        app.select_process_index(7);

        app.begin_filter_edit();
        for key in [
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
        ] {
            app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();
        }

        assert!(app.filter_editing);
        assert_eq!(app.filter_draft, "");
        assert_eq!(app.process_table_state.selected(), Some(7));
    }

    #[test]
    fn filter_editing_space_edits_the_filter_instead_of_tracking() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "alpha.exe".to_string();
        app.snapshot.processes[1].name = "beta.exe".to_string();
        app.snapshot.processes[2].name = "gamma.exe".to_string();
        app.select_process_index(1);

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.filter_editing);
        assert_eq!(app.filter_draft, "b ");
        assert!(app.watch_list.is_empty());
    }

    #[test]
    fn filter_text_is_committed_by_up_or_down_then_selection_moves() {
        let cases = [(KeyCode::Up, 1, 0), (KeyCode::Down, 1, 2)];
        for (key, initial_selection, expected_selection) in cases {
            let mut app = make_test_app(3, 10);
            app.select_process_index(initial_selection);

            app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
                .unwrap();
            app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
                .unwrap();
            app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();

            assert!(!app.filter_editing);
            assert_eq!(app.filter_text, "p");
            assert_eq!(app.filter_draft, "");
            assert_eq!(app.process_table_state.selected(), Some(expected_selection));
            assert_eq!(app.status, "Filter applied: p");
        }
    }

    #[test]
    fn ordinary_character_does_not_start_filter_editing() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.filter_editing);
        assert_eq!(app.filter_draft, "");
    }

    #[test]
    fn ctrl_f_starts_filter_editing() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.filter_editing);
        assert_eq!(app.filter_draft, "");
    }

    #[test]
    fn ctrl_f_only_starts_filter_editing_when_processes_are_focused() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.filter_editing);
        assert_eq!(app.filter_draft, "");
    }

    #[test]
    fn ctrl_i_starts_process_jump_editing() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.jump_editing);
        assert_eq!(app.jump_draft, "");
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        assert!(!app.show_system_info_dialog);
    }

    #[test]
    fn process_jump_typing_moves_selection_without_filtering_rows() {
        let mut app = make_test_app(4, 10);
        app.snapshot.processes[0].name = "alpha.exe".to_string();
        app.snapshot.processes[1].name = "beta.exe".to_string();
        app.snapshot.processes[2].name = "alphabet.exe".to_string();
        app.snapshot.processes[3].name = "gamma.exe".to_string();

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.visible_process_count(), 4);
        assert_eq!(app.process_table_state.selected(), Some(0));
        assert_eq!(app.selected_visible_process().unwrap().name, "alpha.exe");

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(app.selected_visible_process().unwrap().name, "alphabet.exe");

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_table_state.selected(), Some(0));

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.jump_editing);
        assert_eq!(app.jump_draft, "");
    }

    #[test]
    fn ctrl_j_starts_process_jump_and_moves_to_next_match() {
        let mut app = make_test_app(4, 10);
        app.snapshot.processes[0].name = "winproc-tui.exe".to_string();
        app.snapshot.processes[1].name = "codex.exe".to_string();
        app.snapshot.processes[2].name = "win-helper.exe".to_string();
        app.snapshot.processes[3].name = "other.exe".to_string();

        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.jump_editing);
        assert_eq!(app.process_table_state.selected(), Some(0));

        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(
            app.selected_visible_process().unwrap().name,
            "win-helper.exe"
        );

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.jump_editing);
    }

    #[test]
    fn process_jump_up_down_exits_jump_and_moves_selection() {
        let cases = [(KeyCode::Up, 2, 1), (KeyCode::Down, 1, 2)];

        for (key, start, expected) in cases {
            let mut app = make_test_app(4, 10);
            app.process_table_state.select(Some(start));

            app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
                .unwrap();
            app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
                .unwrap();
            assert!(app.jump_editing);

            app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();

            assert!(!app.jump_editing);
            assert_eq!(app.jump_draft, "");
            assert_eq!(app.process_table_state.selected(), Some(expected));
        }
    }

    #[test]
    fn slash_does_not_start_process_jump() {
        let mut app = make_test_app(2, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.jump_editing);
        assert_eq!(app.process_table_state.selected(), Some(0));
    }

    #[test]
    fn process_jump_title_shows_inline_query() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(rendered.contains("Jump c_"), "{rendered}");
    }

    #[test]
    fn process_jump_highlights_matching_name_text() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "winproc-tui.exe".to_string();
        app.snapshot.processes[1].name = "codex.exe".to_string();

        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .unwrap();

        let buffer = render_app_to_buffer(&app, 100, 45);
        let (x, y) = find_text_position(&buffer, "winproc-tui.exe")
            .expect("jump target name should be rendered");

        assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(x + 1, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(x + 2, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(x + 3, y)].fg, ui::THEMES[0].text);
    }

    #[test]
    fn process_filter_highlights_matching_name_text() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "winproc-tui.exe".to_string();
        app.snapshot.processes[1].name = "codex.exe".to_string();

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        for ch in "win".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }
        app.process_table_state.select(None);

        let buffer = render_app_to_buffer(&app, 100, 45);
        let (x, y) = find_text_position(&buffer, "winproc-tui.exe")
            .expect("filter target name should be rendered");

        assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(x + 1, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(x + 2, y)].fg, ui::THEMES[0].warning);
        assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(x + 3, y)].fg, ui::THEMES[0].text);
    }

    #[test]
    fn process_filter_highlights_matching_full_path_text() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![MetricColumn::FullPath];
        app.snapshot.processes[0].name = "app.exe".to_string();
        app.snapshot.processes[0].executable_path = Some(r"C:\work\alpha\app.exe".to_string());
        app.snapshot.processes[1].name = "app.exe".to_string();
        app.snapshot.processes[1].executable_path = Some(r"C:\work\beta\app.exe".to_string());

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        for ch in "beta".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }

        let buffer = render_app_to_buffer(&app, 160, 45);
        let path = r"C:\work\beta\app.exe";
        let (x, y) =
            find_text_position(&buffer, path).expect("filter target path should be rendered");
        let beta_x = x + r"C:\work\".chars().count() as u16;

        assert_eq!(buffer[(beta_x, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(beta_x + 1, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(beta_x + 2, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(beta_x + 3, y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(beta_x + 4, y)].fg, ui::THEMES[0].text);
    }

    #[test]
    fn truncated_full_path_keeps_raw_filter_match_and_highlights_visible_tail() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![MetricColumn::FullPath];
        app.process_column_widths.set(SortColumn::ProcessName, 8);
        app.snapshot.processes[0].name = "app.exe".to_string();
        let raw_path = format!(r"C:\{}\beta\app.exe", "hidden".repeat(16));
        app.snapshot.processes[0].executable_path = Some(raw_path.clone());

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        for ch in "beta".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }

        let buffer = render_app_to_buffer(&app, 80, 30);
        let (beta_x, y) = find_text_position(&buffer, r"beta\app.exe")
            .expect("the retained Full Path tail should be rendered");

        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(
            app.snapshot.processes[0].executable_path.as_deref(),
            Some(raw_path.as_str())
        );
        assert!((0..beta_x).any(|x| buffer[(x, y)].symbol() == "⋯"));
        for offset in 0.."beta".len() as u16 {
            assert_eq!(buffer[(beta_x + offset, y)].fg, ui::THEMES[0].warning);
        }
    }

    #[test]
    fn process_filter_does_not_duplicate_name_match_in_full_path() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![MetricColumn::FullPath];
        app.snapshot.processes[0].name = "app.exe".to_string();
        app.snapshot.processes[0].executable_path = Some(r"C:\work\app.exe".to_string());

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        for ch in "app".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }

        let buffer = render_app_to_buffer(&app, 160, 45);
        let (name_x, name_y) =
            find_text_position(&buffer, "app.exe").expect("process name should be rendered");
        let path = r"C:\work\app.exe";
        let (path_x, path_y) =
            find_text_position(&buffer, path).expect("full path should be rendered");
        let path_match_x = path_x + r"C:\work\".chars().count() as u16;

        assert_eq!(buffer[(name_x, name_y)].fg, ui::THEMES[0].warning);
        assert_eq!(buffer[(path_match_x, path_y)].fg, ui::THEMES[0].text);
    }

    #[test]
    fn filter_text_is_committed_by_enter() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.filter_editing);
        assert_eq!(app.filter_text, "c");
    }

    #[test]
    fn esc_clears_filter_and_exits_filter_editing() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.filter_text, "");
        assert!(!app.filter_editing);
        assert_eq!(app.filter_draft, "");
        assert_eq!(app.visible_process_count(), 3);
        assert_eq!(app.status, "Filter cleared");
    }

    #[test]
    fn esc_clears_existing_filter_from_filter_editing() {
        let mut app = make_test_app(3, 10);
        app.filter_text = "proc".to_string();
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.filter_editing);
        assert_eq!(app.filter_text, "");
        assert_eq!(app.filter_draft, "");
        assert_eq!(app.visible_process_count(), 3);
        assert_eq!(app.status, "Filter cleared");
    }

    #[test]
    fn details_toggle_changes_visibility_without_resetting_graph_workspace_state() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        add_test_graph(&mut app, 1);
        app.ab_comparison = Some(app::AbComparison { a: None, b: None });
        app.details_sample_selected = 7;
        app.details_sample_offset = 3;
        app.details_live = false;
        app.graph_scroll_row = 1;
        app.graph_time_span_seconds = 240;
        app.graph_time_offset_seconds = 30;
        let entries = app.graph_entries.clone();
        let active = app.active_graph_id;
        let comparison = app.ab_comparison.clone();
        assert!(app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_details);
        assert_eq!(app.graph_entries, entries);
        assert_eq!(app.active_graph_id, active);
        assert_eq!(app.ab_comparison, comparison);
        assert_eq!(app.details_sample_selected, 7);
        assert_eq!(app.details_sample_offset, 3);
        assert!(!app.details_live);
        assert_eq!(app.graph_scroll_row, 1);
        assert_eq!(app.graph_time_span_seconds, 240);
        assert_eq!(app.graph_time_offset_seconds, 30);
    }

    #[test]
    fn process_panel_shrinks_with_graphs_and_restores_full_height_when_hidden() {
        let mut app = make_test_app(2, 10);
        assign_private_graph(&mut app);
        let screen = Rect::new(0, 0, 120, 60);

        app::sync_layout_state(&mut app, screen);
        let shown = main_panel_areas_for_app(screen, &app);
        assert_eq!(shown.processes.area.height, 5);
        assert_eq!(shown.processes.page_size, 2);
        assert_eq!(shown.details.unwrap().y, shown.processes.area.bottom());

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        app::sync_layout_state(&mut app, screen);
        let hidden = main_panel_areas_for_app(screen, &app);
        assert!(hidden.processes.area.height > shown.processes.area.height);
        assert!(hidden.details.is_none());

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        app::sync_layout_state(&mut app, screen);
        assert_eq!(
            main_panel_areas_for_app(screen, &app).processes,
            shown.processes
        );
    }

    #[test]
    fn dynamic_process_page_size_preserves_selection_and_clamps_offset() {
        let mut app = make_test_app(20, 10);
        assign_private_graph(&mut app);
        let screen = Rect::new(0, 0, 120, 60);
        app::sync_layout_state(&mut app, screen);
        app.select_process_index(15);
        app.ensure_selected_row_visible();
        assert_eq!(app.process_table_state.offset(), 6);

        app.filter_text = "proc-15".to_string();
        app.rebuild_visible_process_cache();
        app::sync_layout_state(&mut app, screen);

        assert_eq!(app.process_page_size, 1);
        assert_eq!(app.process_table_state.offset(), 0);
        assert_eq!(app.selected_visible_process().unwrap().name, "proc-15");

        app.filter_text.clear();
        app.rebuild_visible_process_cache();
        app::sync_layout_state(&mut app, screen);

        assert_eq!(app.process_page_size, 10);
        assert_eq!(app.process_table_state.offset(), 6);
        assert_eq!(app.selected_visible_process().unwrap().name, "proc-15");
    }

    #[test]
    fn dynamic_graph_and_samples_regions_recompute_on_resize() {
        let mut app = make_test_app(2, 10);
        assign_private_graph(&mut app);
        let short_screen = Rect::new(0, 0, 120, 45);
        let tall_screen = Rect::new(0, 0, 120, 60);

        app::sync_layout_state(&mut app, short_screen);
        let short = main_panel_areas_for_app(short_screen, &app);
        let short_sample_page_size = app.details_sample_page_size;

        app::sync_layout_state(&mut app, tall_screen);
        let tall = main_panel_areas_for_app(tall_screen, &app);

        assert_eq!(short.processes.area.height, tall.processes.area.height);
        assert_eq!(
            tall.details.unwrap().height - short.details.unwrap().height,
            15
        );
        assert_eq!(app.details_sample_page_size - short_sample_page_size, 15);
    }

    #[test]
    fn tracked_total_is_rendered_when_it_is_the_only_process_row() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();
        assign_private_graph(&mut app);
        track_process_name(&mut app, "target.exe");
        app.filter_text = "missing".to_string();
        app.rebuild_visible_process_cache();
        let screen = Rect::new(0, 0, 120, 45);

        app::sync_layout_state(&mut app, screen);
        let panels = main_panel_areas_for_app(screen, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (_, total_y) =
            find_text_position(&buffer, "Tracked Total").expect("Tracked Total should render");

        assert_eq!(app.visible_process_count(), 0);
        assert_eq!(panels.processes.area.height, 4);
        assert_eq!(panels.processes.page_size, 0);
        assert!(panels.processes.show_tracked_total);
        assert_eq!(total_y, panels.processes.area.y + 2);
    }

    #[test]
    fn mouse_selection_uses_dynamic_process_graph_boundary() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        let screen = Rect::new(0, 0, 120, 45);
        app::sync_layout_state(&mut app, screen);
        let panels = main_panel_areas_for_app(screen, &app);
        let process_area = panels.processes.area;

        app.on_mouse(
            left_click(process_area.x + 4, process_area.bottom() - 2),
            screen,
        );
        assert_eq!(app.process_table_state.selected(), Some(2));

        let graph = details_graph_area_for_app(screen, &app).unwrap();
        app.on_mouse(left_click(graph.x + 1, graph.y + 1), screen);

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.process_table_state.selected(), Some(2));
    }

    #[test]
    fn g_without_graph_metrics_shows_warning_dialog() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_details);
        assert!(app.show_no_graph_metrics_warning);
        assert_eq!(app.status, "No metric is selected for graphing.");

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(
            rendered.contains("No metric is selected for graphing."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Select a metric, then press Space or double-click it."),
            "{rendered}"
        );

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_no_graph_metrics_warning);
    }

    #[test]
    fn source_number_keys_only_show_graph_migration_guidance() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));

        for key in ['1', '2', '3', '4'] {
            app.on_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE))
                .unwrap();
            assert!(app.graph_entries.is_empty());
            assert_eq!(app.status, "Use Space or double-click to graph this metric");
        }
        app.on_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.graph_entries.is_empty());
        assert_eq!(app.status, "Remove Graphs with Delete or the remove button");
    }

    #[test]
    fn delete_on_live_process_opens_kill_confirm_before_graph_clear() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::Processes;
        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_process_kill_confirmation);
        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(app.process_kill_targets.len(), 1);
    }

    #[test]
    fn space_on_non_graphable_process_column_only_shows_status() {
        let mut app = make_test_app(3, 10);
        app.selected_process_column_index = 1;

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_entries.is_empty());
        assert!(!app.show_details);
        assert!(!app.show_metric_column_warning);
        assert_eq!(app.status, "Select a graphable metric cell");
    }

    #[test]
    fn space_adds_and_removes_selected_graph_without_tracking() {
        let mut app = make_test_app(30, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 45));
        let source = app.selected_process_graph_source().unwrap();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(app.graph_entries[0].source, source);
        assert!(app.show_details);
        assert!(app.watch_list.is_empty());
        assert_eq!(app.focused_panel, FocusedPanel::Processes);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();
        assert!(app.graph_entries.is_empty());
        assert!(!app.show_details);
        assert!(app.watch_list.is_empty());
    }

    #[test]
    fn graph_collection_accepts_required_counts_without_duplicates() {
        for count in [0, 1, 2, 4, 8, 15, app::GRAPH_LIMIT] {
            let mut app = make_test_app(1, 10);
            for index in 0..count {
                add_test_graph(&mut app, index);
            }

            assert_eq!(app.graph_entries.len(), count);
            let ids = app
                .graph_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(ids.len(), count);
            assert_eq!(
                app.active_graph_id,
                app.graph_entries.last().map(|entry| entry.id)
            );
        }

        let mut app = make_test_app(1, 10);
        let source = test_graph_source(&app, 0);
        assert!(app.add_or_reveal_graph_source(source.clone(), FocusedPanel::Processes));
        let id = app.graph_entries[0].id;
        assert!(app.add_or_reveal_graph_source(source, FocusedPanel::Processes));
        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(app.active_graph_id, Some(id));
    }

    #[test]
    fn graph_collection_rejects_the_seventeenth_entry_without_replacement() {
        let mut app = make_test_app(1, 10);
        for index in 0..app::GRAPH_LIMIT {
            add_test_graph(&mut app, index);
        }
        let entries = app.graph_entries.clone();
        let active = app.active_graph_id;

        assert!(!app.add_or_reveal_graph_source(
            test_graph_source(&app, app::GRAPH_LIMIT),
            FocusedPanel::Processes,
        ));

        assert_eq!(app.graph_entries, entries);
        assert_eq!(app.active_graph_id, active);
        assert_eq!(app.status, "Graph limit reached (16)");
    }

    #[test]
    fn graph_ids_are_monotonic_and_are_not_reused_after_removal() {
        let mut app = make_test_app(1, 10);
        let first = add_test_graph(&mut app, 0);
        assert!(app.remove_graph(first));
        let second = add_test_graph(&mut app, 1);

        assert!(second.0 > first.0);
        assert_ne!(second, first);
    }

    #[test]
    fn graph_source_ordinals_follow_visual_order_after_removal() {
        let mut app = make_test_app(1, 10);
        let ids = (0..app::GRAPH_LIMIT)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        assert!(app.set_active_graph(ids[10]));
        app.details_sample_selected = 7;
        app.details_sample_offset = 3;
        app.ab_comparison = Some(app::AbComparison { a: None, b: None });
        let expected_ids = ids
            .iter()
            .copied()
            .filter(|id| *id != ids[4])
            .collect::<Vec<_>>();

        assert!(app.remove_graph(ids[4]));

        assert_eq!(
            app.graph_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(app.active_graph_id, Some(ids[10]));
        assert_eq!(app.details_sample_selected, 7);
        assert_eq!(app.details_sample_offset, 3);
        assert_eq!(
            app.ab_comparison,
            Some(app::AbComparison { a: None, b: None })
        );
        for (ordinal, entry) in app.graph_entries.iter().enumerate() {
            let state = app
                .graph_source_state(&entry.source)
                .expect("registered source should have a slot number");
            assert_eq!(state.ordinal, ordinal);
            assert_eq!(state.active, entry.id == ids[10]);
        }
        assert_eq!(app.graph_entries.len(), app::GRAPH_LIMIT - 1);
        assert_eq!(
            app.graph_source_state(&app.graph_entries[9].source),
            Some(app::GraphSourceState {
                ordinal: 9,
                active: true,
            })
        );
    }

    #[test]
    fn graph_removal_selects_next_then_previous_and_preserves_non_active_id() {
        for (active_index, remove_index, expected_active_index) in [(0, 0, 1), (2, 2, 3), (4, 4, 3)]
        {
            let mut app = make_test_app(1, 10);
            let ids = (0..5)
                .map(|index| add_test_graph(&mut app, index))
                .collect::<Vec<_>>();
            assert!(app.set_active_graph(ids[active_index]));

            assert!(app.remove_graph(ids[remove_index]));

            assert_eq!(app.active_graph_id, Some(ids[expected_active_index]));
            assert_eq!(app.graph_entries.len(), 4);
            assert!(app.graph_entry_by_id(ids[remove_index]).is_none());
        }

        let mut app = make_test_app(1, 10);
        let ids = (0..5)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        assert!(app.set_active_graph(ids[2]));
        assert!(app.remove_graph(ids[0]));
        assert_eq!(app.active_graph_id, Some(ids[2]));
    }

    #[test]
    fn removing_last_graph_closes_workspace_clears_ab_and_preserves_history() {
        let mut app = make_test_app(1, 10);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
        let sample_count = app.process_history.sample_count_for(&identity);
        let id = app
            .add_or_reveal_graph_source(
                GraphSlot::system(SystemMetric::CpuAverage),
                FocusedPanel::Cpu,
            )
            .then(|| app.active_graph_id.unwrap())
            .unwrap();
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.ab_comparison = Some(app::AbComparison { a: None, b: None });

        assert!(app.remove_graph(id));

        assert!(app.graph_entries.is_empty());
        assert_eq!(app.active_graph_id, None);
        assert!(!app.show_details);
        assert!(app.ab_comparison.is_none());
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        assert_eq!(
            app.process_history.sample_count_for(&identity),
            sample_count
        );
    }

    #[test]
    fn same_name_processes_with_distinct_identities_create_distinct_graphs() {
        let mut app = make_test_app(1, 10);
        let mut first = app.snapshot.processes[0].clone();
        first.name = "worker.exe".to_string();
        first.pid = 100;
        first.start_time = Some(1_000);
        let mut second = first.clone();
        second.pid = 200;
        second.start_time = Some(2_000);

        assert!(app.add_or_reveal_graph_source(
            GraphSlot::process(ProcessIdentity::from_row(&first), DetailsMetric::Private),
            FocusedPanel::Processes,
        ));
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::process(ProcessIdentity::from_row(&second), DetailsMetric::Private),
            FocusedPanel::Processes,
        ));

        assert_eq!(app.graph_entries.len(), 2);
        assert_ne!(app.graph_entries[0].source, app.graph_entries[1].source);
    }

    #[test]
    fn resizing_preserves_graph_order_active_id_and_scrolls_to_active() {
        let mut app = make_test_app(30, 10);
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        for index in 0..8 {
            add_test_graph(&mut app, index);
        }
        let ids = app
            .graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let active_id = app.active_graph_id;

        app::sync_layout_state(&mut app, Rect::new(0, 0, 120, 58));

        assert_eq!(
            app.graph_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(app.active_graph_id, active_id);
        assert!(app.graph_scroll_row > 0);
        assert!(app.show_details);

        app::sync_layout_state(&mut app, Rect::new(0, 0, 120, 100));

        assert_eq!(
            app.graph_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(app.active_graph_id, active_id);
    }

    #[test]
    fn d_on_live_process_opens_kill_confirm() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_details);
        assert!(app.show_process_kill_confirmation);
        assert_eq!(app.process_kill_targets.len(), 1);
    }

    #[test]
    fn ctrl_d_does_not_open_process_kill_confirm() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.show_process_kill_confirmation);
    }

    #[test]
    fn details_metric_defaults_to_private_and_toggles() {
        let mut app = make_test_app(3, 10);

        assert_eq!(app.details_metric, DetailsMetric::Private);
        app.toggle_details_metric();

        assert!(app.show_details);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.details_metric, DetailsMetric::WorksetPrivate);
    }

    #[test]
    fn details_sample_selection_moves_within_samples() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.set_details_sample_page_size(2);
        for offset in [0, 30, 60] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.set_details_sample_selected(2);

        app.select_details_sample_older(100);
        assert_eq!(app.details_sample_selected, 0);

        app.select_details_sample_newer(15);
        assert_eq!(app.details_sample_selected, 2);

        app.select_details_sample_latest();
        assert_eq!(app.details_sample_selected, 2);
        assert_eq!(app.details_sample_offset, 1);
    }

    #[test]
    fn details_sample_selection_scrolls_only_at_view_edges() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.set_details_sample_page_size(3);
        for offset in 0..6 {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        app.set_details_sample_selected(1);
        assert_eq!(app.details_sample_offset, 0);

        app.select_details_sample_newer(1);
        assert_eq!(app.details_sample_selected, 2);
        assert_eq!(app.details_sample_offset, 0);

        app.select_details_sample_newer(1);
        assert_eq!(app.details_sample_selected, 3);
        assert_eq!(app.details_sample_offset, 1);

        app.select_details_sample_older(1);
        assert_eq!(app.details_sample_selected, 2);
        assert_eq!(app.details_sample_offset, 1);
    }

    #[test]
    fn sample_selection_moves_graph_window_only_when_selected_value_is_outside_it() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.graph_time_span_seconds = 60;
        for offset in [0, 60, 120] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        app.select_details_sample_latest();
        assert_eq!(app.graph_time_offset_seconds, 0);

        app.select_details_sample_oldest();
        assert_eq!(app.graph_time_offset_seconds, 60);
        assert!(app.graph_time_window_right_at.is_some());

        app.set_details_sample_selected(1);
        assert_eq!(app.graph_time_offset_seconds, 60);

        app.select_details_sample_latest();
        assert_eq!(app.graph_time_offset_seconds, 0);
        assert!(app.graph_time_window_right_at.is_none());
    }

    #[test]
    fn samples_mouse_wheel_moves_cursor_row() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.set_details_sample_page_size(3);
        for offset in 0..8 {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.select_details_sample_latest();
        assert_eq!(app.details_sample_offset, 5);
        assert_eq!(app.details_sample_selected, 7);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 70,
                row: 20,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 100, 30),
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.details_sample_offset, 5);
        assert_eq!(app.details_sample_selected, 6);

        let graph_span = app.graph_time_span_seconds;
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 70,
                row: 20,
                modifiers: KeyModifiers::SHIFT,
            },
            Rect::new(0, 0, 100, 30),
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.details_sample_selected, 7);
        assert_eq!(app.graph_time_span_seconds, graph_span);
    }

    #[test]
    fn samples_scrollbar_drag_scrolls_viewport() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.set_details_sample_page_size(10);
        let tracked_names = ["proc-0".to_string()].into_iter().collect();
        for offset in 0..100 {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &tracked_names,
            );
        }
        let screen = Rect::new(0, 0, 120, 60);
        let samples = details_samples_area_for_app(screen, &app).unwrap();
        let scrollbar_x = samples.right().saturating_sub(1);
        let scrollbar_top = samples.y;
        let scrollbar_bottom = samples.bottom().saturating_sub(1);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar_x,
                row: scrollbar_top,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.samples_scrollbar_dragging);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.details_sample_offset, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar_x,
                row: scrollbar_bottom,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert_eq!(app.details_sample_offset, 90);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar_x,
                row: scrollbar_bottom,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(!app.samples_scrollbar_dragging);
    }

    #[test]
    fn graph_focus_keys_zoom_pan_and_select_samples() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        for offset in [0, 30, 60] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.select_details_sample_latest();

        app.on_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.graph_time_span_seconds, 60);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.details_sample_selected, 1);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.details_sample_selected, 2);

        app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.details_sample_selected, 0);

        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.details_sample_selected, 2);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 8);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 0);

        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.graph_time_span_seconds, 120);
    }

    #[test]
    fn graph_up_down_changes_graph_while_samples_up_down_changes_sample() {
        let mut app = make_test_app(1, 10);
        let ids = (0..3)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        assert!(app.set_active_graph(ids[1]));
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.active_graph_id, Some(ids[0]));
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.active_graph_id, Some(ids[1]));

        for offset in [0, 30, 60] {
            let mut row = app.snapshot.processes[0].clone();
            row.pid = 10_001;
            row.start_time = Some(1_800_000_001);
            row.name = "graph-1.exe".to_string();
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &[row],
                &app.normalized_watch_names,
            );
        }
        app.select_details_sample_latest();
        app.focused_panel = FocusedPanel::DetailsSamples;

        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.active_graph_id, Some(ids[1]));
        assert_eq!(app.details_sample_selected, 1);
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.active_graph_id, Some(ids[1]));
        assert_eq!(app.details_sample_selected, 2);
    }

    #[test]
    fn samples_page_keys_scroll_the_list_without_changing_graph_span() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        for offset in 0..12 {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.set_details_sample_page_size(4);
        app.select_details_sample_latest();
        app.focused_panel = FocusedPanel::DetailsSamples;
        let graph_span = app.graph_time_span_seconds;

        assert_eq!(app.details_sample_selected, 11);
        assert_eq!(app.details_sample_offset, 8);

        app.on_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.details_sample_selected, 7);
        assert_eq!(app.details_sample_offset, 4);
        assert_eq!(app.graph_time_span_seconds, graph_span);

        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.details_sample_selected, 11);
        assert_eq!(app.details_sample_offset, 8);
        assert_eq!(app.graph_time_span_seconds, graph_span);
    }

    #[test]
    fn delete_removes_only_active_graph_from_graph_and_samples_focus() {
        for focus in [FocusedPanel::DetailsGraph, FocusedPanel::DetailsSamples] {
            let mut app = make_test_app(1, 10);
            let first = add_test_graph(&mut app, 0);
            let second = add_test_graph(&mut app, 1);
            app.focused_panel = focus;

            app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
                .unwrap();

            assert_eq!(app.graph_entries.len(), 1);
            assert_eq!(app.active_graph_id, Some(first));
            assert!(app.graph_entry_by_id(second).is_none());
        }
    }

    #[test]
    fn graph_enter_opens_info_for_graphed_process_without_changing_selection() {
        let mut app = make_test_app(3, 10);
        let selected_identity = app.selected_visible_process_identity().unwrap();
        app.open_selected_process_info_dialog().unwrap();
        app.activate_process_info_tab(app::ProcessInfoTab::Image)
            .unwrap();
        app.close_process_info_dialog();
        let graph_identity = ProcessIdentity::from_row(&app.snapshot.processes[2]);
        app.add_or_reveal_graph_source(
            GraphSlot::process(graph_identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.filter_text = "proc-0".to_string();
        app.rebuild_visible_process_cache();

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_process_info_dialog);
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Image);
        assert_eq!(
            ProcessIdentity::from_row(app.process_info_target_process().unwrap()),
            graph_identity
        );
        assert_eq!(
            app.selected_visible_process_identity(),
            Some(selected_identity)
        );
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.status, "Process Info: proc-2");
    }

    #[test]
    fn files_tab_from_graph_uses_fixed_graph_target() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            3,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        let selected_identity = app.selected_visible_process_identity().unwrap();
        let graph_identity = ProcessIdentity::from_row(&app.snapshot.processes[2]);
        app.add_or_reveal_graph_source(
            GraphSlot::process(graph_identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Files);
        match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect { identity, .. } => assert_eq!(identity, graph_identity),
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        }
        assert_eq!(
            app.selected_visible_process_identity(),
            Some(selected_identity)
        );
    }

    #[test]
    fn graph_enter_rejects_system_graphs() {
        let mut app = make_test_app(1, 10);
        app.add_or_reveal_graph_source(
            GraphSlot::system(SystemMetric::PhysicalMemory),
            FocusedPanel::System,
        );
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_process_info_dialog);
        assert_eq!(
            app.status,
            "Process Info is available only for process Graphs"
        );
    }

    #[test]
    fn graph_pan_skips_empty_time_ranges() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(180),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 120);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 0);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 120);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 128);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 136);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 144);
    }

    #[test]
    fn graph_wheel_scrolls_workspace_rows_without_zooming() {
        let mut app = make_test_app(8, 10);
        let ids = (0..8)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.show_samples_panel = false;
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.graph_time_span_seconds = 120;
        let screen = Rect::new(0, 0, 100, 45);
        app::sync_layout_state(&mut app, screen);
        assert!(app.set_active_graph(ids[0]));
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let viewport = ui::layout::graph_workspace_layout(details, &app).graph_viewport;

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: viewport.x,
                row: viewport.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.graph_time_span_seconds, 120);
        assert_eq!(app.graph_scroll_row, 1);
    }

    #[test]
    fn shift_wheel_encodings_change_span_with_graph_focus_outside_graph_viewport() {
        let mut app = make_test_app(8, 10);
        let ids = (0..8)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.show_samples_panel = false;
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.graph_time_span_seconds = 120;
        let screen = Rect::new(0, 0, 100, 45);
        app::sync_layout_state(&mut app, screen);
        assert!(app.set_active_graph(ids[0]));
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::SHIFT,
            },
            screen,
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.graph_time_span_seconds, 60);
        assert_eq!(app.graph_scroll_row, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::SHIFT,
            },
            screen,
        );

        assert_eq!(app.graph_time_span_seconds, 120);
        assert_eq!(app.graph_scroll_row, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollLeft,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::SHIFT,
            },
            screen,
        );

        assert_eq!(app.graph_time_span_seconds, 60);
        assert_eq!(app.graph_scroll_row, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollRight,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::SHIFT,
            },
            screen,
        );

        assert_eq!(app.graph_time_span_seconds, 120);
        assert_eq!(app.graph_scroll_row, 0);
    }

    #[test]
    fn graph_right_button_drag_pans_visible_range() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        for offset in [0, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let start_x = graph.x.saturating_add(graph.width / 2);
        let y = graph.y.saturating_add(5);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: start_x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Right),
                column: start_x.saturating_add(400),
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                column: start_x.saturating_add(400),
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.graph_time_span_seconds, 60);
        assert!(app.graph_time_offset_seconds > 0);
        assert!(app.graph_pan_drag.is_none());
    }

    #[test]
    fn graph_drag_clamps_to_range_with_visible_sample() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        for offset in [0, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let start_x = graph.x.saturating_add(20);
        let y = graph.y.saturating_add(5);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: start_x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Right),
                column: start_x.saturating_add(400),
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(
            (180..=240).contains(&app.graph_time_offset_seconds),
            "{}",
            app.graph_time_offset_seconds
        );
    }

    #[test]
    fn graph_right_click_without_drag_resets_to_live_edge() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.graph_time_offset_seconds = 60;
        app.details_live = false;
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let x = graph.x.saturating_add(30);
        let y = graph.y.saturating_add(5);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.graph_time_offset_seconds, 0);
        assert_eq!(app.status, "Graph right edge: 0s");
    }

    #[test]
    fn graph_ctrl_left_drag_pans_without_selecting_sample() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.graph_time_offset_seconds = 60;
        app.details_live = false;
        for offset in [0, 30, 60, 90, 120, 150, 180, 210, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.set_details_sample_selected_manual(5);
        let selected = app.details_sample_selected;
        assert_eq!(app.graph_time_offset_seconds, 60);
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let start_x = graph.x.saturating_add(30);
        let y = graph.y.saturating_add(5);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: start_x,
                row: y,
                modifiers: KeyModifiers::CONTROL,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: start_x.saturating_sub(30),
                row: y,
                modifiers: KeyModifiers::CONTROL,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: start_x.saturating_sub(30),
                row: y,
                modifiers: KeyModifiers::CONTROL,
            },
            screen,
        );

        assert_eq!(app.details_sample_selected, selected);
        assert!(app.graph_time_offset_seconds < 60);
        assert!(app.graph_pan_drag.is_none());
    }

    #[test]
    fn graph_stops_live_scroll_when_latest_sample_is_outside_visible_range() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.details_live = true;
        app.graph_time_offset_seconds = 60;
        app.sampling_in_progress = true;
        app.process_history.record_snapshot(
            app.snapshot.captured_at - chrono::Duration::seconds(60),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        let mut snapshot = test_snapshot(1);
        snapshot.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);

        result_tx
            .send(CollectSnapshotResult {
                snapshot,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert!(!app.details_live);
        assert_eq!(app.graph_time_offset_seconds, 61);

        app.sampling_in_progress = true;
        let mut snapshot = test_snapshot(1);
        snapshot.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);
        result_tx
            .send(CollectSnapshotResult {
                snapshot,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert!(!app.details_live);
        assert_eq!(app.graph_time_offset_seconds, 62);
    }

    #[test]
    fn frozen_graph_window_uses_rounded_subsecond_sample_intervals() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        assign_private_graph(&mut app);
        let latest = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap()
            + chrono::Duration::milliseconds(900);
        app.snapshot.captured_at = latest;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.details_live = false;
        app.graph_time_offset_seconds = 60;
        app.graph_time_window_right_at = Some(latest - chrono::Duration::seconds(60));
        app.sampling_in_progress = true;
        let mut snapshot = test_snapshot(1);
        snapshot.captured_at = latest + chrono::Duration::milliseconds(950);

        result_tx
            .send(CollectSnapshotResult {
                snapshot,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.graph_time_offset_seconds, 61);
    }

    #[test]
    fn graph_cursor_movement_does_not_stop_graph_live_scroll() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.details_live = true;
        app.process_history.record_snapshot(
            app.snapshot.captured_at - chrono::Duration::seconds(1),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.select_details_sample_latest();

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.details_live);
        assert_eq!(app.graph_time_offset_seconds, 0);
        assert!(app.graph_time_window_right_at.is_none());

        app.sampling_in_progress = true;
        let mut snapshot = test_snapshot(1);
        snapshot.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);
        result_tx
            .send(CollectSnapshotResult {
                snapshot,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.graph_time_offset_seconds, 0);
        assert!(app.graph_time_window_right_at.is_none());
    }

    #[test]
    fn setting_ab_point_does_not_stop_graph_live_scroll() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.details_live = true;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.details_live);
        assert!(app.graph_time_window_right_at.is_none());
        assert!(app.ab_comparison.as_ref().and_then(|ab| ab.a).is_some());

        app.sampling_in_progress = true;
        let mut snapshot = test_snapshot(1);
        snapshot.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);
        result_tx
            .send(CollectSnapshotResult {
                snapshot,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.graph_time_offset_seconds, 0);
        assert!(app.graph_time_window_right_at.is_none());
    }

    #[test]
    fn graph_drag_does_not_clear_fit_all_samples() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.toggle_graph_all_samples();
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let start_x = graph.x.saturating_add(40);
        let y = graph.y.saturating_add(5);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: start_x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Right),
                column: start_x.saturating_add(20),
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                column: start_x.saturating_add(20),
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(app.graph_show_all_samples);
        assert_eq!(app.graph_time_offset_seconds, 0);
    }

    #[test]
    fn graph_all_samples_checkbox_uses_full_sample_span() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        let screen = Rect::new(0, 0, 120, 45);
        let controls = details_shared_controls_area_for_app(screen, &app).unwrap();
        let x = controls
            .right()
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_sub(GRAPH_ALL_SAMPLES_TOGGLE_WIDTH)
            .saturating_add(1);
        let y = controls.y;

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(app.graph_show_all_samples);
        assert_eq!(app.effective_graph_time_span_seconds(), 240);

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("☑  f: Fit all"), "{rendered}");
    }

    #[test]
    fn graph_f_key_toggles_fit_all_samples() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_show_all_samples);
        assert_eq!(app.effective_graph_time_span_seconds(), 240);
        assert_eq!(app.status, "Graph span: fit all (240s)");

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.graph_show_all_samples);
        assert_eq!(app.effective_graph_time_span_seconds(), 60);
    }

    #[test]
    fn graph_shared_keys_work_when_samples_are_focused() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsSamples;
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.graph_show_all_samples);
        assert_eq!(app.effective_graph_time_span_seconds(), 240);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);

        app.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.graph_y_axis_zero_min);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
    }

    #[test]
    fn log_view_all_samples_span_can_exceed_live_history_cap() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.log_view_path = Some(std::path::PathBuf::from("long.log"));
        app.process_history = ProcessHistory::default();
        for offset in [0, 7_201] {
            app.process_history.record_snapshot_unbounded(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
            );
        }

        app.toggle_graph_all_samples();

        assert!(app.graph_show_all_samples);
        assert_eq!(app.effective_graph_time_span_seconds(), 7_201);
    }

    #[test]
    fn log_view_process_title_omits_history_counts() {
        let mut app = make_test_app(1, 10);
        app.log_view_path = Some(std::path::PathBuf::from("long.log"));
        app.process_history = ProcessHistory::default();
        app.system_history = SystemHistory::default();
        for offset in 0..=7_200 {
            app.snapshot.captured_at =
                app.snapshot.captured_at + chrono::Duration::seconds(i64::from(offset));
            app.process_history
                .record_snapshot_unbounded(app.snapshot.captured_at, &app.snapshot.processes);
            app.system_history.record_snapshot_unbounded(&app.snapshot);
        }

        let rendered = render_app_to_text(&app, 120, 30);

        assert!(rendered.contains("[Samples: 7,201]"), "{rendered}");
        assert!(
            rendered.contains("PROCESSES · 1 visible · ☐ Tracked-only(Shift+T)"),
            "{rendered}"
        );
        assert!(!rendered.contains("Samples: tracked"), "{rendered}");
        assert!(
            !rendered.contains("[Max samples: normal 120 / tracked 7200]"),
            "{rendered}"
        );
        assert!(!rendered.contains("[Max samples: 7200]"), "{rendered}");
    }

    #[test]
    fn graph_y_axis_checkbox_click_toggles_scale_mode() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        assert!(app.graph_y_axis_zero_min);

        let screen = Rect::new(0, 0, 120, 45);
        let controls = details_shared_controls_area_for_app(screen, &app).unwrap();
        let x = controls
            .right()
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_add(1);
        let y = controls.y;

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(!app.graph_y_axis_zero_min);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(app.graph_y_axis_zero_min);
    }

    #[test]
    fn graph_checkboxes_work_when_samples_panel_is_hidden() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.show_samples_panel = false;
        assert!(!app.graph_show_all_samples);
        assert!(app.graph_y_axis_zero_min);

        let screen = Rect::new(0, 0, 120, 45);
        let controls = details_shared_controls_area_for_app(screen, &app).unwrap();
        let y = controls.y;
        let all_samples_x = controls
            .right()
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_sub(GRAPH_ALL_SAMPLES_TOGGLE_WIDTH)
            .saturating_add(1);
        let y_axis_x = controls
            .right()
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_add(1);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: all_samples_x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.graph_show_all_samples);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: y_axis_x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(!app.graph_y_axis_zero_min);
    }

    #[test]
    fn graph_mouse_selection_uses_full_width_when_samples_panel_is_hidden() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.show_samples_panel = false;
        for offset in [0, 30, 60] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.details_sample_selected = 0;

        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_app(screen, &app).expect("graph plot");
        let x = graph.right().saturating_sub(2);
        let y = graph.y.saturating_add(4);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.details_sample_selected, 2);
    }

    #[test]
    fn graph_z_key_toggles_y_axis_scale_mode() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.graph_y_axis_zero_min);

        app.on_key(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT))
            .unwrap();

        assert!(app.graph_y_axis_zero_min);
    }

    #[test]
    fn graph_layout_shortcuts_preserve_explicit_samples_preference() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        add_test_graph(&mut app, 1);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.last_screen_area = Rect::new(0, 0, 120, 60);

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::OneColumn);
        assert!(app.show_samples_panel);

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
        assert!(app.show_samples_panel);
        let rendered = render_app_to_text(&app, 120, 60);
        assert!(rendered.contains("☑  v: Samples"), "{rendered}");
        assert!(rendered.contains("l: 2 cols"), "{rendered}");

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::Auto);
        assert!(app.show_samples_panel);

        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_sample_delta);
        app.on_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_samples_panel);
    }

    #[test]
    fn auto_graph_layout_uses_width_and_keeps_every_graph_registered() {
        let mut app = make_test_app(30, 10);
        for index in 0..8 {
            add_test_graph(&mut app, index);
        }
        app.graph_slot_layout = GraphSlotLayout::Auto;
        app.show_samples_panel = false;

        let wide = Rect::new(0, 0, 140, 80);
        app::sync_layout_state(&mut app, wide);
        let details = main_panel_areas_for_app(wide, &app).details.unwrap();
        assert_eq!(ui::layout::graph_workspace_layout(details, &app).columns, 2);

        let narrow = Rect::new(0, 0, 80, 80);
        app::sync_layout_state(&mut app, narrow);
        let details = main_panel_areas_for_app(narrow, &app).details.unwrap();
        assert_eq!(ui::layout::graph_workspace_layout(details, &app).columns, 1);
        assert_eq!(app.graph_entries.len(), 8);
    }

    #[test]
    fn graph_workspace_layout_reaches_required_counts_in_every_column_mode() {
        let screen = Rect::new(0, 0, 220, 110);
        for count in [1, 2, 3, 4, 5, 8, app::GRAPH_LIMIT] {
            for mode in [
                GraphSlotLayout::OneColumn,
                GraphSlotLayout::TwoColumns,
                GraphSlotLayout::Auto,
            ] {
                let mut app = make_test_app(1, 10);
                for index in 0..count {
                    add_test_graph(&mut app, index);
                }
                app.graph_slot_layout = mode;
                app.show_samples_panel = false;
                app::sync_layout_state(&mut app, screen);
                let details = main_panel_areas_for_app(screen, &app).details.unwrap();
                let layout = ui::layout::graph_workspace_layout(details, &app);
                let expected_columns = if mode == GraphSlotLayout::OneColumn || count == 1 {
                    1
                } else {
                    2
                };
                assert_eq!(
                    layout.columns, expected_columns,
                    "count={count}, mode={mode:?}"
                );
                assert_eq!(
                    layout.total_rows,
                    count.div_ceil(expected_columns),
                    "count={count}, mode={mode:?}"
                );

                let expected_ids = app
                    .graph_entries
                    .iter()
                    .map(|entry| entry.id)
                    .collect::<std::collections::HashSet<_>>();
                let mut reached = std::collections::HashSet::new();
                for row in 0..=layout.max_scroll_row {
                    app.graph_scroll_row = row;
                    let row_layout = ui::layout::graph_workspace_layout(details, &app);
                    for card in &row_layout.graph_cards {
                        assert_eq!(app.graph_entries[card.ordinal].id, card.id);
                        reached.insert(card.id);
                    }
                }
                assert_eq!(reached, expected_ids, "count={count}, mode={mode:?}");

                let rendered = render_app_to_text(&app, screen.width, screen.height);
                let slot_label = if count == 1 { "Slot" } else { "Slots" };
                assert!(
                    rendered.contains(&format!("GRAPHS · {count} {slot_label} · Span 60s")),
                    "count={count}, mode={mode:?}\n{rendered}"
                );
                assert!(rendered.contains(&format!("Slot#{count}")), "{rendered}");
                assert!(rendered.contains("[x]"), "{rendered}");
            }
        }
    }

    #[test]
    fn two_column_workspace_scrolls_by_rows_in_row_major_order() {
        let mut app = make_test_app(1, 10);
        let ids = (0..8)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = false;
        let screen = Rect::new(0, 0, 160, 45);
        app::sync_layout_state(&mut app, screen);
        assert!(app.set_active_graph(ids[0]));
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let before = ui::layout::graph_workspace_layout(details, &app);
        assert_eq!(before.columns, 2);
        assert_eq!(
            before
                .graph_cards
                .iter()
                .map(|card| card.id)
                .collect::<Vec<_>>(),
            ids[..before.graph_cards.len()]
        );

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: before.graph_viewport.x,
                row: before.graph_viewport.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        let after = ui::layout::graph_workspace_layout(details, &app);
        assert_eq!(app.graph_scroll_row, 1);
        assert_eq!(after.graph_cards[0].id, ids[2]);
        assert_eq!(after.graph_cards[1].id, ids[3]);
    }

    #[test]
    fn samples_inspector_uses_right_bottom_and_temporary_collapse_placements() {
        let mut app = make_test_app(30, 10);
        for index in 0..4 {
            add_test_graph(&mut app, index);
        }
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = true;

        let wide = Rect::new(0, 0, 200, 80);
        app::sync_layout_state(&mut app, wide);
        let details = main_panel_areas_for_app(wide, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        assert_eq!(
            layout.samples_placement,
            Some(ui::layout::SamplesPlacement::Right)
        );
        assert_eq!(layout.columns, 2);
        assert!(!app.samples_temporarily_collapsed);

        let narrow_tall = Rect::new(0, 0, 70, 100);
        app::sync_layout_state(&mut app, narrow_tall);
        let details = main_panel_areas_for_app(narrow_tall, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        assert_eq!(
            layout.samples_placement,
            Some(ui::layout::SamplesPlacement::Bottom)
        );
        assert!(!app.samples_temporarily_collapsed);

        let narrow_short = Rect::new(0, 0, 70, 40);
        app::sync_layout_state(&mut app, narrow_short);
        let details = main_panel_areas_for_app(narrow_short, &app)
            .details
            .unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        assert!(layout.samples.is_none());
        assert!(app.samples_temporarily_collapsed);
        assert!(app.show_samples_panel);

        app::sync_layout_state(&mut app, wide);
        assert!(app.effective_show_samples_panel());
        app.toggle_samples_panel();
        app::sync_layout_state(&mut app, narrow_short);
        app::sync_layout_state(&mut app, wide);
        assert!(!app.show_samples_panel);
        assert!(!app.effective_show_samples_panel());
    }

    #[test]
    fn panel_focus_and_active_graph_use_distinct_frames_in_both_themes() {
        let screen = Rect::new(0, 0, 180, 60);
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(3, 10);
            app.theme_index = theme_index;
            assign_private_graph(&mut app);
            app.show_samples_panel = true;
            app::sync_layout_state(&mut app, screen);
            let details = main_panel_areas_for_app(screen, &app).details.unwrap();
            let layout = ui::layout::graph_workspace_layout(details, &app);
            let samples = layout
                .samples
                .expect("Samples should fit beside Graph Slots");
            let card = layout.graph_cards.first().expect("active graph card").area;

            app.focused_panel = FocusedPanel::DetailsGraph;
            let graph_focused = render_app_to_buffer(&app, screen.width, screen.height);
            assert_eq!(
                graph_focused[(
                    layout.graph_slots.right().saturating_sub(1),
                    layout.graph_slots.y
                )]
                    .symbol(),
                "━"
            );
            assert_eq!(
                graph_focused[(
                    layout.graph_slots.right().saturating_sub(1),
                    layout.graph_slots.y
                )]
                    .fg,
                theme.focus_border
            );
            assert_eq!(
                graph_focused[(layout.graph_slots.x, layout.graph_slots.y + 1)].symbol(),
                " "
            );
            assert_eq!(
                graph_focused[(
                    layout.graph_slots.x,
                    layout.graph_slots.bottom().saturating_sub(1)
                )]
                    .symbol(),
                " "
            );
            assert_eq!(graph_focused[(card.x, card.y)].symbol(), "╭");
            assert_eq!(graph_focused[(card.x, card.y)].fg, theme.active_series);
            assert!(
                graph_focused[(card.x, card.y)]
                    .modifier
                    .contains(Modifier::BOLD)
            );
            assert_eq!(graph_focused[(samples.x, samples.y)].symbol(), "╭");
            assert_eq!(graph_focused[(samples.x, samples.y)].fg, theme.border);

            app.focused_panel = FocusedPanel::DetailsSamples;
            let samples_focused = render_app_to_buffer(&app, screen.width, screen.height);
            assert_eq!(
                samples_focused[(
                    layout.graph_slots.right().saturating_sub(1),
                    layout.graph_slots.y
                )]
                    .symbol(),
                "─"
            );
            assert_eq!(
                samples_focused[(
                    layout.graph_slots.right().saturating_sub(1),
                    layout.graph_slots.y
                )]
                    .fg,
                theme.border
            );
            assert_eq!(samples_focused[(card.x, card.y)].symbol(), "╭");
            assert_eq!(samples_focused[(card.x, card.y)].fg, theme.active_series);
            assert!(
                samples_focused[(card.x, card.y)]
                    .modifier
                    .contains(Modifier::BOLD)
            );
            assert_eq!(samples_focused[(samples.x, samples.y)].symbol(), "┏");
            assert_eq!(
                samples_focused[(samples.x, samples.y)].fg,
                theme.focus_border
            );

            app.focused_panel = FocusedPanel::Processes;
            let processes_focused = render_app_to_buffer(&app, screen.width, screen.height);
            assert_eq!(
                processes_focused[(
                    layout.graph_slots.right().saturating_sub(1),
                    layout.graph_slots.y
                )]
                    .symbol(),
                "─"
            );
            assert_eq!(processes_focused[(card.x, card.y)].symbol(), "╭");
            assert_eq!(processes_focused[(card.x, card.y)].fg, theme.active_series);
            assert!(
                processes_focused[(card.x, card.y)]
                    .modifier
                    .contains(Modifier::BOLD)
            );
            assert_eq!(processes_focused[(samples.x, samples.y)].symbol(), "╭");
        }
    }

    #[test]
    fn graph_series_stay_grayscale_and_samples_stay_neutral_in_both_themes() {
        let screen = Rect::new(0, 0, 180, 70);
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme_index;
            let identity = app.selected_visible_process_identity().unwrap();
            app.add_or_reveal_graph_source(
                GraphSlot::process(identity.clone(), DetailsMetric::Private),
                FocusedPanel::Processes,
            );
            app.add_or_reveal_graph_source(
                GraphSlot::process(identity, DetailsMetric::Workset),
                FocusedPanel::Processes,
            );
            let active_id = app.active_graph_id.unwrap();
            let first_time = app.snapshot.captured_at;
            app.snapshot.processes[0].private_bytes = Some(1_000);
            app.snapshot.processes[0].workset_bytes = Some(2_000);
            app.process_history.record_snapshot(
                first_time,
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
            app.snapshot.captured_at = first_time + chrono::Duration::seconds(1);
            app.snapshot.processes[0].private_bytes = Some(1_100);
            app.snapshot.processes[0].workset_bytes = Some(2_200);
            app.process_history.record_snapshot(
                app.snapshot.captured_at,
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
            app.select_details_sample_latest();
            app.show_samples_panel = true;
            app::sync_layout_state(&mut app, screen);

            let details = main_panel_areas_for_app(screen, &app).details.unwrap();
            let layout = ui::layout::graph_workspace_layout(details, &app);
            let active_plot = layout
                .graph_cards
                .iter()
                .find(|card| card.id == active_id)
                .expect("active graph card")
                .plot;
            let inactive_plot = layout
                .graph_cards
                .iter()
                .find(|card| card.id != active_id)
                .expect("inactive graph card")
                .plot;
            let samples = layout.samples.expect("Samples should render");
            let buffer = render_app_to_buffer(&app, screen.width, screen.height);

            assert!(
                area_contains_foreground(&buffer, active_plot, theme.graph_line),
                "theme={theme_index}: active series should stay monochrome"
            );
            assert!(
                !area_contains_foreground(&buffer, active_plot, theme.active_series),
                "theme={theme_index}: active series should not carry the selection color"
            );
            assert!(
                area_contains_foreground(&buffer, inactive_plot, theme.graph_line),
                "theme={theme_index}: inactive series should stay monochrome"
            );
            let (value_x, value_y) = find_text_position_in_area(&buffer, samples, "2,200")
                .expect("active Samples metric value should render");
            assert_eq!(buffer[(value_x, value_y)].fg, theme.text);
            let (history_x, history_y) = find_text_position_in_area(&buffer, samples, "2,000")
                .expect("non-selected Samples metric value should render");
            assert_eq!(buffer[(history_x, history_y)].fg, theme.text);
            let (header_x, header_y) = find_text_position_in_area(&buffer, samples, "WS")
                .expect("active Samples metric header should render");
            assert_eq!(buffer[(header_x, header_y)].fg, theme.accent);
            assert!(
                buffer[(header_x, header_y)]
                    .modifier
                    .contains(Modifier::BOLD)
            );
            let (slot_x, slot_y) = find_text_position_in_area(&buffer, samples, "Slot#2")
                .expect("compact Samples slot title should render");
            assert_eq!(buffer[(slot_x, slot_y)].fg, theme.muted);
            assert!(!buffer[(slot_x, slot_y)].modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn exactly_one_top_level_panel_uses_high_contrast_focus_chrome() {
        let screen = Rect::new(0, 0, 180, 60);
        for theme_index in 0..ui::THEMES.len() {
            let mut app = make_test_app(3, 10);
            app.theme_index = theme_index;
            assign_private_graph(&mut app);
            app.show_samples_panel = true;
            app::sync_layout_state(&mut app, screen);
            let details = main_panel_areas_for_app(screen, &app)
                .details
                .expect("Graph Workspace should render");
            let graph_layout = ui::layout::graph_workspace_layout(details, &app);

            for focused_panel in [
                FocusedPanel::System,
                FocusedPanel::SystemActivity,
                FocusedPanel::Cpu,
                FocusedPanel::Processes,
                FocusedPanel::DetailsGraph,
                FocusedPanel::DetailsSamples,
            ] {
                app.focused_panel = focused_panel;
                let buffer = render_app_to_buffer(&app, screen.width, screen.height);
                let focused_top_left_corners = buffer
                    .content()
                    .iter()
                    .filter(|cell| cell.symbol() == "┏" && cell.fg == app.theme().focus_border)
                    .count();
                let graph_has_focused_top_rule =
                    (graph_layout.graph_slots.x..graph_layout.graph_slots.right()).any(|x| {
                        let cell = &buffer[(x, graph_layout.graph_slots.y)];
                        cell.symbol() == "━" && cell.fg == app.theme().focus_border
                    });
                assert_eq!(
                    focused_top_left_corners + usize::from(graph_has_focused_top_rule),
                    1,
                    "theme={theme_index}, focus={focused_panel:?}"
                );
            }
        }
    }

    #[test]
    fn top_level_panel_titles_follow_their_border_color_in_both_themes() {
        let screen = Rect::new(0, 0, 180, 60);
        for theme_index in 0..ui::THEMES.len() {
            let mut app = make_test_app(3, 10);
            app.theme_index = theme_index;
            assign_private_graph(&mut app);
            app.show_samples_panel = true;
            app::sync_layout_state(&mut app, screen);

            let main_panels = main_panel_areas_for_app(screen, &app);
            let graph_layout = ui::layout::graph_workspace_layout(
                main_panels.details.expect("Graph Workspace should render"),
                &app,
            );
            let titled_panels = [
                (
                    FocusedPanel::System,
                    ui::ram_vram_panel_area_for_screen(screen, &app),
                    "RAM/VRAM",
                ),
                (
                    FocusedPanel::SystemActivity,
                    ui::system_activity_panel_area_for_screen(screen, &app),
                    "NW/DISK",
                ),
                (
                    FocusedPanel::Cpu,
                    ui::cpu_panel_area_for_screen(screen, &app),
                    "CPUS",
                ),
                (
                    FocusedPanel::Processes,
                    main_panels.processes.area,
                    "PROCESSES",
                ),
                (
                    FocusedPanel::DetailsGraph,
                    graph_layout.graph_slots,
                    "GRAPHS",
                ),
                (
                    FocusedPanel::DetailsSamples,
                    graph_layout.samples.expect("Samples should render"),
                    "SAMPLES",
                ),
            ];

            for (focused_panel, _, _) in titled_panels {
                app.focused_panel = focused_panel;
                let buffer = render_app_to_buffer(&app, screen.width, screen.height);
                for (panel, area, title) in titled_panels {
                    let (x, y) = find_text_position_in_area(&buffer, area, title)
                        .unwrap_or_else(|| panic!("missing title {title}"));
                    let expected = if panel == focused_panel {
                        app.theme().focus_border
                    } else {
                        app.theme().border
                    };
                    assert_eq!(
                        buffer[(x, y)].fg,
                        expected,
                        "theme={theme_index}, focus={focused_panel:?}, title={title}"
                    );
                    let should_be_bold =
                        panel != FocusedPanel::DetailsGraph || panel == focused_panel;
                    assert_eq!(
                        buffer[(x, y)].modifier.contains(Modifier::BOLD),
                        should_be_bold,
                        "unexpected title weight: theme={theme_index}, focus={focused_panel:?}, title={title}"
                    );
                }
            }
        }
    }

    #[test]
    fn panel_and_dialog_title_names_are_uppercase_and_bold_in_both_themes() {
        let screen = Rect::new(0, 0, 180, 60);
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(3, 10);
            app.theme_index = theme_index;

            app.show_help = true;
            let help = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&help, "HELP", theme);
            app.show_help = false;

            app.show_column_picker = true;
            let columns = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&columns, "COLUMNS", theme);
            app.show_column_picker = false;

            app.show_system_info_dialog = true;
            let system_info = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&system_info, "SYSTEM INFO", theme);
            app.show_system_info_dialog = false;

            app.show_log_dir_dialog = true;
            let log_directory = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&log_directory, "LOG DIRECTORY", theme);
            app.show_log_dir_dialog = false;

            app.open_selected_process_info_dialog().unwrap();
            let process_info = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&process_info, "PROCESS INFO", theme);
            let (metadata_x, metadata_y) = find_text_position(&process_info, "proc-0 · PID 0")
                .expect("Process Info target metadata should render");
            assert_eq!(process_info[(metadata_x, metadata_y)].fg, theme.muted);
            assert!(
                !process_info[(metadata_x, metadata_y)]
                    .modifier
                    .contains(Modifier::BOLD),
                "Process Info target metadata should use normal weight"
            );
            app.close_process_info_dialog();

            app.show_recording_path_dialog = true;
            let recording = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&recording, "RECORDING", theme);
            app.show_recording_path_dialog = false;

            app.show_log_list = true;
            let logs = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&logs, "LOGS", theme);
            app.show_log_list = false;

            app.open_tracked_lists();
            let tracked_lists = render_app_to_buffer(&app, screen.width, screen.height);
            assert_dialog_title_style(&tracked_lists, "TRACKED LISTS", theme);
            assert_title_style(&tracked_lists, "LOAD TRACKED LIST", theme.border);
            assert_title_style(&tracked_lists, "SAVE CURRENT TRACKED LIST", theme.border);
            app.close_tracked_lists();

            app.show_display_area_warning = true;
            let warning = render_app_to_buffer(&app, screen.width, screen.height);
            assert_title_style(&warning, "WARNING", theme.warning);
            app.show_display_area_warning = false;

            app.show_quit_confirmation = true;
            let confirm = render_app_to_buffer(&app, screen.width, screen.height);
            assert_title_style(&confirm, "CONFIRM", theme.warning);
        }
    }

    #[test]
    fn clicking_graph_workspace_top_rule_moves_focus_to_graphs() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::Processes;
        let screen = Rect::new(0, 0, 180, 60);
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);

        app.on_mouse(
            left_click(layout.graph_slots.right() - 1, layout.graph_slots.y),
            screen,
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.status, "Focus: Graph 1/1");
    }

    #[test]
    fn compact_workspace_keeps_process_row_panel_title_remove_and_resize_message() {
        let mut app = make_test_app(3, 10);
        add_test_graph(&mut app, 0);
        app.show_samples_panel = false;
        let screen = Rect::new(0, 0, 120, 25);
        app::sync_layout_state(&mut app, screen);
        let panels = main_panel_areas_for_app(screen, &app);
        let layout = ui::layout::graph_workspace_layout(panels.details.unwrap(), &app);

        assert!(layout.compact);
        assert_eq!(layout.graph_cards.len(), 1);
        assert!(layout.graph_cards[0].title.height > 0);
        assert!(layout.graph_cards[0].remove.width > 0);
        assert!(panels.processes.page_size >= 1);

        let rendered = render_app_to_text(&app, screen.width, screen.height);
        assert!(
            rendered.contains("GRAPHS · 1 Slot · Span 60s"),
            "{rendered}"
        );
        assert!(rendered.contains("Slot#1"), "{rendered}");
        assert!(rendered.contains("[x]"), "{rendered}");
        assert!(
            rendered.contains("Resize terminal to view Graph"),
            "{rendered}"
        );
    }

    #[test]
    fn long_graph_target_keeps_remove_button_visible_at_narrow_width() {
        let mut app = make_test_app(1, 10);
        let mut row = app.snapshot.processes[0].clone();
        row.name = "a-very-long-process-name-that-must-be-truncated.exe".to_string();
        app.add_or_reveal_graph_source(
            GraphSlot::process(
                ProcessIdentity::from_row(&row),
                DetailsMetric::WorksetPrivate,
            ),
            FocusedPanel::Processes,
        );
        app.show_samples_panel = false;
        let screen = Rect::new(0, 0, 60, 45);
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);

        assert_eq!(layout.graph_cards.len(), 1);
        assert_eq!(layout.graph_cards[0].remove.width, 5);
        assert_eq!(layout.graph_cards[0].remove_label, "[x]");
        let rendered = render_app_to_text(&app, screen.width, screen.height);
        assert!(rendered.contains("[x]"), "{rendered}");
    }

    #[test]
    fn ctrl_o_is_unassigned() {
        let mut app = make_test_app(1, 10);
        let status = app.status.clone();

        app.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.status, status);
        assert!(!app.has_modal_focus());
    }

    #[test]
    fn changing_layout_never_hides_or_removes_registered_graphs() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        for index in 1..5 {
            add_test_graph(&mut app, index);
        }
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.last_screen_area = Rect::new(0, 0, 99, 60);
        let ids = app
            .graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
        assert_eq!(
            app.graph_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert!(!app.show_display_area_warning);
    }

    #[test]
    fn samples_temporary_collapse_is_distinct_from_user_preference() {
        let mut app = make_test_app(30, 10);
        for index in 0..3 {
            add_test_graph(&mut app, index);
        }
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = true;
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.set_screen_area(Rect::new(0, 0, 70, 40));
        app.sync_graph_layout_visibility();
        assert!(app.show_samples_panel);
        assert!(app.samples_temporarily_collapsed);
        assert!(!app.effective_show_samples_panel());

        app.set_screen_area(Rect::new(0, 0, 180, 80));
        app.sync_graph_layout_visibility();
        assert!(app.show_samples_panel);
        assert!(!app.samples_temporarily_collapsed);
        assert!(app.effective_show_samples_panel());

        app.toggle_samples_panel();
        app.set_screen_area(Rect::new(0, 0, 220, 100));
        app.sync_graph_layout_visibility();
        assert!(!app.show_samples_panel);
        assert!(!app.effective_show_samples_panel());
    }

    #[test]
    fn graph_shared_samples_and_layout_checkboxes_work_with_mouse() {
        let screen = Rect::new(0, 0, 120, 60);
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        add_test_graph(&mut app, 1);
        app.last_screen_area = screen;
        let initial = render_app_to_buffer(&app, screen.width, screen.height);
        let (layout_x, layout_y) =
            find_text_position(&initial, "l: Auto").expect("layout control should render");

        app.on_mouse(left_click(layout_x, layout_y), screen);

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::OneColumn);
        assert!(app.show_samples_panel);
        let one_column = render_app_to_buffer(&app, screen.width, screen.height);
        let (layout_x, layout_y) =
            find_text_position(&one_column, "l: 1 col").expect("layout control should render");

        app.on_mouse(left_click(layout_x, layout_y), screen);

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
        assert!(app.show_samples_panel);
        let two_columns = render_app_to_buffer(&app, screen.width, screen.height);
        let (samples_x, samples_y) =
            find_text_position(&two_columns, "v: Samples").expect("Samples checkbox should render");

        app.on_mouse(left_click(samples_x, samples_y), screen);

        assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
        assert!(!app.show_samples_panel);
    }

    #[test]
    fn graph_y_axis_checkbox_uses_box_symbols() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("☑  z: Min 0"), "{rendered}");

        app.graph_y_axis_zero_min = false;
        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("☐  z: Min 0"), "{rendered}");
    }

    #[test]
    fn clicking_graph_card_selects_graph_without_changing_process_row() {
        let mut app = make_test_app(4, 10);
        let target = ProcessIdentity::from_row(&app.snapshot.processes[2]);
        app.add_or_reveal_graph_source(
            GraphSlot::process(target, DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        let target_id = app.graph_entries[0].id;
        add_test_graph(&mut app, 1);
        app.show_samples_panel = false;
        app.select_process_index(0);

        let screen = Rect::new(0, 0, 140, 80);
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let card = layout
            .graph_cards
            .iter()
            .find(|card| card.id == target_id)
            .unwrap();
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: card.title.x,
                row: card.title.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.active_graph_id, Some(target_id));
        assert_eq!(app.process_table_state.selected(), Some(0));
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
    }

    #[test]
    fn process_metric_double_click_adds_then_reveals_without_removing_graph() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![MetricColumn::HandleCount];
        app.selected_process_column_index = 2;
        let screen = Rect::new(0, 0, 140, 60);
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, _) = find_text_position(&buffer, "Hndl").expect("handle column should render");
        let y = main_panel_areas_for_app(screen, &app).processes.area.y + 2;
        let source = app.selected_process_graph_source().unwrap();

        app.on_mouse(left_click(x, y), screen);
        assert!(app.graph_entries.is_empty());
        app.on_mouse(left_click(x, y), screen);

        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(app.graph_entries[0].source, source);
        let source_id = app.graph_entries[0].id;
        add_test_graph(&mut app, 8);
        assert_ne!(app.active_graph_id, Some(source_id));

        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);

        assert_eq!(app.graph_entries.len(), 2);
        assert_eq!(app.active_graph_id, Some(source_id));
    }

    #[test]
    fn process_metric_double_click_requires_the_same_visible_process_identity() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![MetricColumn::HandleCount];
        app.sort = SortSpec {
            column: SortColumn::Pid,
            direction: SortDirection::Asc,
        };
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();
        let screen = Rect::new(0, 0, 140, 60);
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, _) = find_text_position(&buffer, "Hndl").expect("handle column should render");
        let y = main_panel_areas_for_app(screen, &app).processes.area.y + 2;

        app.on_mouse(left_click(x, y), screen);
        app.snapshot.processes[0].pid = 500;
        app.snapshot.processes[0].name = "reordered.exe".to_string();
        app.snapshot.processes[0].start_time = Some(1_900_000_000);
        app.rebuild_visible_process_cache();
        app.clamp_process_table_state();
        let visible_identity = ProcessIdentity::from_row(app.visible_process_at(0).unwrap());

        app.on_mouse(left_click(x, y), screen);
        assert!(app.graph_entries.is_empty());
        app.on_mouse(left_click(x, y), screen);

        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(
            app.graph_entries[0].source,
            GraphSlot::process(visible_identity, DetailsMetric::HandleCount)
        );
    }

    #[test]
    fn process_non_metric_double_click_does_not_add_a_graph() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![MetricColumn::HandleCount];
        let screen = Rect::new(0, 0, 140, 60);
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, _) = find_text_position(&buffer, "PID").expect("PID column should render");
        let y = main_panel_areas_for_app(screen, &app).processes.area.y + 2;

        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);

        assert!(app.graph_entries.is_empty());
    }

    #[test]
    fn semantic_double_click_rejects_changed_identity_metric_and_timeout() {
        let mut app = make_test_app(1, 10);
        let first = test_graph_source(&app, 0);
        let second = test_graph_source(&app, 1);
        let start = Instant::now();

        app.register_graph_source_click(first.clone(), start, FocusedPanel::Processes);
        app.register_graph_source_click(
            second.clone(),
            start + Duration::from_millis(100),
            FocusedPanel::Processes,
        );
        assert!(app.graph_entries.is_empty());

        app.register_graph_source_click(
            second.clone(),
            start + Duration::from_millis(200),
            FocusedPanel::Processes,
        );
        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(app.graph_entries[0].source, second);

        let third = test_graph_source(&app, 2);
        app.register_graph_source_click(
            third.clone(),
            start + Duration::from_millis(300),
            FocusedPanel::Processes,
        );
        app.register_graph_source_click(
            third,
            start + Duration::from_millis(801),
            FocusedPanel::Processes,
        );
        assert_eq!(app.graph_entries.len(), 1);

        let different_metric = match first {
            GraphSlot::Process { identity, .. } => {
                GraphSlot::process(identity, DetailsMetric::WorksetPrivate)
            }
            GraphSlot::System { .. } => unreachable!(),
        };
        app.register_graph_source_click(
            test_graph_source(&app, 3),
            start + Duration::from_millis(900),
            FocusedPanel::Processes,
        );
        app.register_graph_source_click(
            different_metric,
            start + Duration::from_millis(950),
            FocusedPanel::Processes,
        );
        assert_eq!(app.graph_entries.len(), 1);
    }

    #[test]
    fn system_panel_double_clicks_add_ram_activity_and_cpu_graphs() {
        let mut app = make_test_app(2, 10);
        let screen = Rect::new(0, 0, 180, 70);
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let points = [
            (
                find_text_position(&buffer, "Physical Memory").unwrap(),
                GraphSlot::system(SystemMetric::PhysicalMemory),
            ),
            (
                find_text_position(&buffer, "Net Rx").unwrap(),
                GraphSlot::system(SystemMetric::NetworkReceived),
            ),
            (
                find_text_position(&buffer, "CPU Usage").unwrap(),
                GraphSlot::system(SystemMetric::CpuAverage),
            ),
        ];

        for ((x, y), expected) in points {
            app.on_mouse(left_click(x, y), screen);
            app.on_mouse(left_click(x, y), screen);
            assert!(app.graph_id_for_source(&expected).is_some());
        }
        assert_eq!(app.graph_entries.len(), 3);
    }

    #[test]
    fn graph_panel_title_omits_the_verbose_slot_list() {
        let mut app = make_test_app(1, 10);
        for index in 0..8 {
            add_test_graph(&mut app, index);
        }
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.show_samples_panel = false;
        let selected_at = app.snapshot.captured_at;
        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint {
                captured_at: selected_at - chrono::Duration::seconds(10),
            }),
            b: Some(app::AbComparisonPoint {
                captured_at: selected_at,
            }),
        });
        let screen = Rect::new(0, 0, 120, 48);
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let title_row = (layout.graph_slots.x..layout.graph_slots.right())
            .map(|x| buffer[(x, layout.graph_slots.y)].symbol())
            .collect::<String>();

        assert!(
            title_row.contains("GRAPHS · 8 Slots · Span 60s"),
            "{title_row}"
        );
        assert!(!title_row.contains("graph-"), "{title_row}");
        assert!(!title_row.contains("Cursor"), "{title_row}");
        assert!(!title_row.contains("· A "), "{title_row}");
        assert!(!title_row.contains("· B "), "{title_row}");
        let (metadata_x, metadata_y) =
            find_text_position_in_area(&buffer, layout.graph_slots, "8 Slots")
                .expect("Graph slot metadata should render in the panel title");
        assert_eq!(buffer[(metadata_x, metadata_y)].fg, app.theme().border);
        assert!(
            !buffer[(metadata_x, metadata_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn graph_remove_button_has_priority_and_preserves_non_active_selection() {
        let mut app = make_test_app(1, 10);
        let ids = (0..3)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.show_samples_panel = false;
        let active = ids[2];
        let screen = Rect::new(0, 0, 160, 80);
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let remove = layout
            .graph_cards
            .iter()
            .find(|card| card.id == ids[0])
            .unwrap()
            .remove;

        app.on_mouse(left_click(remove.x, remove.y), screen);

        assert!(app.graph_entry_by_id(ids[0]).is_none());
        assert_eq!(app.active_graph_id, Some(active));
        assert_eq!(app.graph_entries.len(), 2);
    }

    #[test]
    fn graph_scrollbar_track_click_and_thumb_drag_reach_later_rows() {
        let mut app = make_test_app(1, 10);
        let ids = (0..8)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.show_samples_panel = false;
        let screen = Rect::new(0, 0, 100, 48);
        app::sync_layout_state(&mut app, screen);
        assert!(app.set_active_graph(ids[0]));
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let scrollbar = layout.graph_scrollbar.expect("graph scrollbar");
        let bottom = scrollbar.bottom().saturating_sub(2);

        app.on_mouse(left_click(scrollbar.x, bottom), screen);
        assert!(app.graph_scroll_row > 0);
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar.x,
                row: bottom,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        app.set_graph_scroll_row(0);
        app.on_mouse(
            left_click(scrollbar.x, scrollbar.y.saturating_add(1)),
            screen,
        );
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar.x,
                row: bottom,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert_eq!(app.graph_scroll_row, layout.max_scroll_row);
    }

    #[test]
    fn samples_wheel_does_not_scroll_graph_workspace_rows() {
        let mut app = make_test_app(1, 10);
        let ids = (0..8)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.show_samples_panel = true;
        let screen = Rect::new(0, 0, 180, 60);
        app::sync_layout_state(&mut app, screen);
        assert!(app.set_active_graph(ids[0]));
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let samples = ui::layout::graph_workspace_layout(details, &app)
            .samples
            .expect("Samples inspector");
        assert_eq!(app.graph_scroll_row, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: samples.x.saturating_add(1),
                row: samples.y.saturating_add(1),
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.graph_scroll_row, 0);
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
    }

    #[test]
    fn tracked_lists_close_button_closes_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(2, 10);
        app.open_tracked_lists();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ Close ]")
            .expect("Tracked Lists close button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(app.tracked_lists_dialog.is_none());
    }

    #[test]
    fn tracked_lists_separates_loading_and_saving() {
        let mut app = make_test_app(2, 10);
        app.watch_list = vec!["chrome.exe".to_string()];
        app.runtime.active_tracked_list = Some("Browser".to_string());
        app.open_tracked_lists();

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(rendered.contains("LOAD TRACKED LIST"), "{rendered}");
        assert!(
            rendered
                .contains("Up/Down selects · Enter loads · Click Empty loads · F2/Del saved only"),
            "{rendered}"
        );
        assert!(rendered.contains("Empty (default)"), "{rendered}");
        assert!(rendered.contains("SAVE CURRENT TRACKED LIST"), "{rendered}");
        assert!(rendered.contains("Current: Browser"), "{rendered}");
        assert!(rendered.contains("List name:  Browser"), "{rendered}");
        assert!(rendered.contains("Tracked List startup"), "{rendered}");
        assert!(rendered.contains("< Resume last >"), "{rendered}");
        assert!(!rendered.contains("[ Rename ]"), "{rendered}");
        assert!(!rendered.contains("[ Delete ]"), "{rendered}");
    }

    #[test]
    fn tracked_lists_rows_preview_process_names_instead_of_counts() {
        let mut app = make_test_app(1, 10);
        app.runtime.active_tracked_list = Some("Browser".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "Browser".to_string(),
            processes: vec!["chrome.exe".to_string(), "node.exe".to_string()],
        }];
        app.open_tracked_lists();

        let rendered = render_app_to_text(&app, 120, 45);
        let row = rendered
            .lines()
            .find(|line| line.contains("Browser (*)"))
            .expect("saved Tracked List row should render");

        assert!(row.contains("chrome.exe, node.exe"), "{row}");
        assert!(!row.contains("2 processes"), "{row}");
    }

    #[test]
    fn tracked_lists_enter_loads_selected_saved_list() {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.watch_list, vec!["api.exe"]);
        assert!(app.tracked_lists_dialog.is_none());
    }

    #[test]
    fn tracked_lists_builtin_empty_is_virtual_and_active_only_for_empty_working_list() {
        let mut app = make_test_app(1, 10);
        app.open_tracked_lists();

        let rendered = render_app_to_text(&app, 120, 45);
        let empty_row = rendered
            .lines()
            .find(|line| line.contains("Empty (default)"))
            .expect("built-in empty row should render");
        assert!(empty_row.contains("Empty (default) (*)"), "{empty_row}");
        assert!(rendered.contains("Enter/Click Load Empty"), "{rendered}");
        assert!(!rendered.contains("New Empty"), "{rendered}");
        assert!(app.runtime.saved_tracked_lists.is_empty());

        app.watch_list = vec!["worker.exe".to_string()];
        app.normalized_watch_names = ["worker.exe".to_string()].into_iter().collect();
        let rendered = render_app_to_text(&app, 120, 45);
        let empty_row = rendered
            .lines()
            .find(|line| line.contains("Empty (default)"))
            .expect("built-in empty row should render");
        assert!(!empty_row.contains("(*)"), "{empty_row}");

        app.watch_list.clear();
        app.normalized_watch_names.clear();
        app.runtime.active_tracked_list = Some("Saved empty".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "Saved empty".to_string(),
            processes: Vec::new(),
        }];
        let rendered = render_app_to_text(&app, 120, 45);
        let empty_row = rendered
            .lines()
            .find(|line| line.contains("Empty (default)"))
            .expect("built-in empty row should render");
        assert!(!empty_row.contains("(*)"), "{empty_row}");
        assert!(rendered.contains("Saved empty (*)"), "{rendered}");
    }

    #[test]
    fn tracked_lists_builtin_empty_loads_with_enter_and_preserves_tracked_only() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["api.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string()].into_iter().collect();
        app.watch_enabled = true;
        app.runtime.active_tracked_list = Some("API".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_home();

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(app.watch_list.is_empty());
        assert!(app.watch_enabled);
        assert_eq!(app.runtime.active_tracked_list, None);
        assert_eq!(app.runtime.saved_tracked_lists.len(), 1);
        assert_eq!(app.runtime.saved_tracked_lists[0].name, "API");
        assert!(app.tracked_lists_dialog.is_none());
    }

    #[test]
    fn tracked_lists_builtin_empty_loads_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["api.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string()].into_iter().collect();
        app.open_tracked_lists();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "Empty (default)")
            .expect("built-in empty row should render");

        app.on_mouse(left_click(x + 1, y), screen);

        assert!(app.watch_list.is_empty());
        assert!(app.tracked_lists_dialog.is_none());
    }

    #[test]
    fn tracked_lists_builtin_empty_cannot_be_renamed_deleted_or_overwritten() {
        let mut app = make_test_app(1, 10);
        app.open_tracked_lists();

        app.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::Browse)
        ));
        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::Browse)
        ));

        app.focus_tracked_lists_save_name();
        for ch in "Empty (default)".chars() {
            app.push_tracked_list_save_name_char(ch);
        }
        app.save_current_tracked_list();

        let (_, _, error) = app
            .tracked_lists_save_name()
            .expect("save-name input should remain available");
        assert_eq!(
            error,
            Some("Empty (default) is built in and cannot be overwritten.")
        );
        assert!(app.runtime.saved_tracked_lists.is_empty());
    }

    #[test]
    fn tracked_lists_named_list_cannot_be_renamed_to_builtin_empty_name() {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);
        app.begin_tracked_list_rename();
        for _ in 0.."API".len() {
            app.pop_tracked_list_name_char();
        }
        for ch in "Empty (default)".chars() {
            app.push_tracked_list_name_char(ch);
        }

        app.commit_tracked_list_name_input();

        assert_eq!(app.runtime.saved_tracked_lists[0].name, "API");
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::NameInput { error: Some(error), .. })
                if error.contains("cannot be overwritten")
        ));
    }

    #[test]
    fn tracked_lists_plain_n_no_longer_starts_empty() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["api.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string()].into_iter().collect();
        app.open_tracked_lists();

        app.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.watch_list, vec!["api.exe"]);
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::Browse)
        ));
    }

    #[test]
    fn tracked_lists_tab_cycles_list_and_every_button() {
        let mut app = make_test_app(1, 10);
        app.open_tracked_lists();
        let expected = [
            (None, true, false),
            (Some(app::TrackedListsButton::Save), false, false),
            (None, false, true),
            (Some(app::TrackedListsButton::Close), false, false),
            (None, false, false),
        ];

        for (focused, save_name_focused, startup_focused) in expected {
            app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .unwrap();
            assert_eq!(app.tracked_lists_focused_button(), focused);
            assert_eq!(app.tracked_lists_save_name_focused(), save_name_focused);
            assert_eq!(app.tracked_lists_startup_focused(), startup_focused);
        }

        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.tracked_lists_focused_button(),
            Some(app::TrackedListsButton::Close)
        );
    }

    #[test]
    fn tracked_lists_f2_opens_rename_and_plain_r_d_do_nothing() {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);

        app.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::NameInput { .. })
        ));

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::Browse)
        ));
    }

    #[test]
    fn tracked_lists_delete_key_opens_delete_confirmation() {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);

        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();

        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::ConfirmDelete { name, .. }) if name == "API"
        ));
    }

    #[test]
    fn tracked_lists_mouse_hover_highlights_button() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.open_tracked_lists();
        let initial = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&initial, "[ Save ]").expect("Save button should render");
        assert_eq!(initial[(x + 2, y)].bg, ui::THEMES[0].panel_alt);

        app.on_mouse(mouse_move(x + 2, y), screen);

        assert_eq!(
            app.tracked_lists_hovered_button(),
            Some(app::TrackedListsButton::Save)
        );
        let hovered = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(hovered[(x + 2, y)].bg, ui::THEMES[0].focus_surface);
        assert!(hovered[(x + 2, y)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tracked_lists_save_name_accepts_keyboard_input_and_enter() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["api.exe".to_string()];
        app.open_tracked_lists();
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert!(app.tracked_lists_save_name_focused());

        for ch in "API".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .unwrap();
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.runtime.active_tracked_list.as_deref(), Some("API"));
        assert_eq!(app.runtime.saved_tracked_lists.len(), 1);
        assert_eq!(
            app.runtime.saved_tracked_lists[0].processes,
            vec!["api.exe"]
        );
    }

    #[test]
    fn tracked_lists_save_name_focuses_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.runtime.active_tracked_list = Some("API".to_string());
        app.open_tracked_lists();
        let input = ui::tracked_list_save_name_area_for_screen(screen)
            .expect("save-name input should have an area");

        app.on_mouse(left_click(input.x + 1, input.y), screen);

        assert!(app.tracked_lists_save_name_focused());
    }

    #[test]
    fn tracked_list_startup_changes_with_keyboard_and_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.open_tracked_lists();
        for _ in 0..3 {
            app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .unwrap();
        }
        assert!(app.tracked_lists_startup_focused());

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.runtime.tracked_list_startup,
            config::TrackedListStartup::ChooseList
        );

        let startup = ui::tracked_list_startup_area_for_screen(screen)
            .expect("startup control should render");
        app.on_mouse(left_click(startup.x + 1, startup.y), screen);
        assert_eq!(
            app.runtime.tracked_list_startup,
            config::TrackedListStartup::StartEmpty
        );
        assert!(app.tracked_lists_startup_focused());
    }

    #[test]
    fn tracked_list_delete_confirmation_applies_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);
        app.request_delete_selected_tracked_list();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ Delete ]")
            .expect("Tracked List delete button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(app.runtime.saved_tracked_lists.is_empty());
        assert!(matches!(
            app.tracked_lists_view(),
            Some(app::TrackedListsView::Browse)
        ));
    }

    #[test]
    fn graph_current_line_label_draws_selected_value_in_accent() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].private_bytes = Some(424_242);
        assign_private_graph(&mut app);
        app.show_samples_panel = false;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.select_details_sample_latest();

        let screen = Rect::new(0, 0, 120, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let mut found_accent_value = false;
        for y in 0..screen.height {
            for x in 0..screen.width {
                let row = (x..screen.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>();
                if row.starts_with("424,242") && buffer[(x, y)].fg == ui::THEMES[0].accent {
                    found_accent_value = true;
                }
            }
        }
        assert!(found_accent_value, "current value label should use accent");
    }

    #[test]
    fn graph_ab_labels_render_on_x_axis_not_cursor_value_row() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.show_sample_delta = true;
        let base = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap();
        for (seconds, value) in [(0, 100), (30, 200), (60, 424_242)] {
            app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
            app.snapshot.processes[0].private_bytes = Some(value);
            app.process_history.record_snapshot(
                app.snapshot.captured_at,
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.select_details_sample_latest();
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();
        app.select_details_sample_oldest();
        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();
        app.select_details_sample_latest();

        let screen = Rect::new(0, 0, 120, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let graph = details_graph_area_for_app(screen, &app).unwrap();
        let (_, value_y) = find_text_position_in_area(&buffer, graph, "424,242")
            .expect("selected graph value should render in graph");
        let a_labels = find_styled_symbol_positions_in_area(&buffer, graph, "A", THEMES[0].warning);
        let b_labels = find_styled_symbol_positions_in_area(&buffer, graph, "B", THEMES[0].warning);
        let graph_rows = ui::layout::details_graph_rows(graph);
        let expected_label_y = graph_rows[1].bottom().saturating_sub(1);

        assert_eq!(a_labels.len(), 1, "A label should render once in Graph");
        assert_eq!(b_labels.len(), 1, "B label should render once in Graph");
        assert_eq!(a_labels[0].1, expected_label_y);
        assert_eq!(b_labels[0].1, expected_label_y);
        assert!(a_labels[0].1 > value_y);
        assert!(b_labels[0].1 > value_y);
    }

    #[test]
    fn details_rendering_shows_workspace_title_and_active_samples() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(
            rendered.contains("GRAPHS · 1 Slot · Span 60s"),
            "{rendered}"
        );
        assert!(!rendered.contains("Slot#1/"), "{rendered}");
        assert!(
            rendered.contains("Slot#1 · Private · proc-0 · B-A: --"),
            "{rendered}"
        );
        assert!(rendered.contains("SAMPLES · Slot#1"), "{rendered}");
        assert!(!rendered.contains("SAMPLES · Slot#1/"), "{rendered}");
        assert!(rendered.contains("A/B Time      Private"), "{rendered}");
        assert!(rendered.contains("MA5:"), "{rendered}");
        assert!(!rendered.contains("Details"), "{rendered}");
        assert!(!rendered.contains("A/B not set"), "{rendered}");
    }

    #[test]
    fn multi_graph_rendering_uses_one_shared_samples_inspector() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 140, 80));
        let identity = app.selected_visible_process_identity().unwrap();
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Workset),
            FocusedPanel::Processes,
        );
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        let rendered = render_app_to_text(&app, 140, 80);

        assert!(rendered.contains("Slot#1 · Private · proc-0"), "{rendered}");
        assert!(rendered.contains("Slot#2 · WS · proc-0"), "{rendered}");
        assert_eq!(rendered.matches("B-A: --").count(), 2, "{rendered}");
        assert_eq!(rendered.matches("f: Fit all").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("z: Min 0").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("v: Samples").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("d: Delta").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("l: Auto").count(), 1, "{rendered}");
        assert_eq!(
            rendered.matches("SAMPLES · Slot#2").count(),
            1,
            "{rendered}"
        );

        let buffer = render_app_to_buffer(&app, 140, 80);
        let (active_x, active_y) =
            find_text_position(&buffer, "Slot#2 · WS").expect("active Graph title should render");
        let (inactive_x, inactive_y) = find_text_position(&buffer, "Slot#1 · Private")
            .expect("inactive Graph title should render");
        assert_eq!(buffer[(active_x + 9, active_y)].fg, THEMES[0].text);
        assert_eq!(buffer[(inactive_x + 9, inactive_y)].fg, THEMES[0].muted);
    }

    #[test]
    fn four_graphs_render_in_a_two_by_two_row_major_grid() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 140, 80));
        let identity = app.selected_visible_process_identity().unwrap();
        for metric in [
            DetailsMetric::Private,
            DetailsMetric::Workset,
            DetailsMetric::CpuPercent,
            DetailsMetric::IoRead,
        ] {
            app.add_or_reveal_graph_source(
                GraphSlot::process(identity.clone(), metric),
                FocusedPanel::Processes,
            );
        }
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = false;
        let details = main_panel_areas_for_app(Rect::new(0, 0, 140, 80), &app)
            .details
            .unwrap();
        let cards = ui::layout::graph_workspace_layout(details, &app).graph_cards;

        assert_eq!(cards.len(), 4);
        assert!(cards[0].area.x < cards[1].area.x);
        assert_eq!(cards[0].area.y, cards[1].area.y);
        assert_eq!(cards[0].area.x, cards[2].area.x);
        assert!(cards[0].area.y < cards[2].area.y);
        assert_eq!(cards[2].area.y, cards[3].area.y);
    }

    #[test]
    fn one_column_graphs_share_compact_y_axis_width_and_keep_samples_exact() {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, 140, 80);
        app.set_screen_area(screen);
        let identity = app.selected_visible_process_identity().unwrap();
        app.snapshot.processes[0].private_bytes = Some(5_900_000);
        app.snapshot.processes[0].handle_count = Some(42);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::HandleCount),
            FocusedPanel::Processes,
        );
        app.graph_slot_layout = GraphSlotLayout::OneColumn;
        app.show_samples_panel = true;

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let rendered = buffer_to_text(&buffer);
        assert!(rendered.contains("Slot#1 · Private"), "{rendered}");
        assert!(rendered.contains("Slot#2 · Hndl"), "{rendered}");
        assert!(rendered.contains("5.9 MB"), "{rendered}");
        assert!(rendered.contains("5,900,000"), "{rendered}");

        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        for card in layout.graph_cards {
            let chart = ui::layout::details_graph_rows(card.plot)[1];
            assert!(buffer[(chart.x, chart.y)].symbol() == " " || chart.width > 0);
        }
    }

    #[test]
    fn two_column_graphs_share_compact_y_axis_width() {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, 140, 80);
        app.set_screen_area(screen);
        let identity = app.selected_visible_process_identity().unwrap();
        app.snapshot.processes[0].private_bytes = Some(5_900_000);
        app.snapshot.processes[0].handle_count = Some(42);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::HandleCount),
            FocusedPanel::Processes,
        );
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = false;

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let rendered = buffer_to_text(&buffer);
        assert!(rendered.contains("Slot#1 · Private"), "{rendered}");
        assert!(rendered.contains("Slot#2 · Hndl"), "{rendered}");
        assert!(rendered.contains("5.9 MB"), "{rendered}");

        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        assert_eq!(
            ui::layout::graph_workspace_layout(details, &app)
                .graph_cards
                .len(),
            2
        );
    }

    #[test]
    fn process_selection_tracks_identity_after_rows_reorder() {
        let mut app = make_test_app(4, 10);
        app.select_process_index(2);

        app.snapshot.processes.reverse();
        app.clamp_process_table_state();

        let selected = app
            .selected_visible_process()
            .expect("selected process should remain visible");
        assert_eq!(selected.name, "proc-2");
    }

    #[test]
    fn left_right_selects_process_metric_column_when_processes_are_focused() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::WorksetPrivateBytes)
        );
        assert_eq!(app.details_metric, DetailsMetric::Private);
    }

    #[test]
    fn left_right_selects_pid_and_process_columns() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.selected_process_column_index = 2;

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_process_column(), SortColumn::ProcessName);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_process_column(), SortColumn::Pid);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_process_column(), SortColumn::ProcessName);
    }

    #[test]
    fn shift_left_right_reorders_selected_metric_column() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = vec![
            MetricColumn::CpuPercent,
            MetricColumn::PrivateBytes,
            MetricColumn::HandleCount,
        ];
        app.selected_process_column_index = 3;

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(
            app.process_columns,
            vec![
                MetricColumn::PrivateBytes,
                MetricColumn::CpuPercent,
                MetricColumn::HandleCount,
            ]
        );
        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::PrivateBytes)
        );
        assert_eq!(app.selected_process_column_index, 2);
        assert_eq!(app.column_preset, ColumnPreset::Custom);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(
            app.process_columns,
            vec![
                MetricColumn::CpuPercent,
                MetricColumn::PrivateBytes,
                MetricColumn::HandleCount,
            ]
        );
        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::PrivateBytes)
        );
        assert_eq!(app.selected_process_column_index, 3);
    }

    #[test]
    fn w_and_shift_w_adjust_selected_fixed_process_column_widths() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;

        app.selected_process_column_index = 0;
        app.process_metric_column_offset = usize::MAX;
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::Pid),
            SortColumn::Pid.default_width() + 1
        );
        assert_eq!(
            app.process_metric_column_offset,
            app.process_columns.len() - 1
        );

        app.selected_process_column_index = 1;
        app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::ProcessName),
            SortColumn::ProcessName.default_width() - 1
        );

        app.process_column_widths.set(
            SortColumn::ProcessName,
            SortColumn::ProcessName.default_width(),
        );
        app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::ProcessName),
            SortColumn::ProcessName.default_width() - 1
        );

        app.process_column_widths.set(
            SortColumn::ProcessName,
            SortColumn::ProcessName.default_width(),
        );
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::ProcessName),
            SortColumn::ProcessName.default_width() - 1
        );
    }

    #[test]
    fn process_column_width_shortcuts_respect_limits_modifiers_focus_and_filter() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.selected_process_column_index = 0;

        app.process_column_widths.set(
            SortColumn::Pid,
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX,
        );
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::Pid),
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX
        );
        assert!(app.status.starts_with("Column width limit: PID"));

        app.process_column_widths
            .set(SortColumn::Pid, SortColumn::Pid.min_width());
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::Pid),
            SortColumn::Pid.min_width()
        );

        let width = app.process_column_widths.resolved(SortColumn::Pid);
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT))
            .unwrap();
        assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

        app.focused_panel = FocusedPanel::System;
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

        app.focused_panel = FocusedPanel::Processes;
        app.begin_filter_edit();
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.filter_draft, "wW");
        assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_help);
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);
    }

    #[test]
    fn process_column_widths_follow_identity_without_changing_table_state() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = vec![
            MetricColumn::PrivateBytes,
            MetricColumn::FullPath,
            MetricColumn::HandleCount,
        ];
        app.selected_process_column_index = 2;
        let original_sort = app.sort;
        let original_details_metric = app.details_metric;
        let original_show_details = app.show_details;
        let original_preset = app.column_preset;
        let original_order = app.process_columns.clone();

        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            MetricColumn::PrivateBytes.width() + 1
        );
        assert_eq!(app.sort, original_sort);
        assert_eq!(app.details_metric, original_details_metric);
        assert_eq!(app.show_details, original_show_details);
        assert_eq!(app.column_preset, original_preset);
        assert_eq!(app.process_columns, original_order);

        app.selected_process_column_index = 3;
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::FullPath)),
            MetricColumn::FullPath.width() + 1
        );
        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            MetricColumn::PrivateBytes.width() + 1
        );

        app.selected_process_column_index = 2;
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.process_columns[1], MetricColumn::PrivateBytes);
        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            MetricColumn::PrivateBytes.width() + 1
        );

        app.process_columns
            .retain(|column| *column != MetricColumn::FullPath);
        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::FullPath)),
            MetricColumn::FullPath.width() + 1
        );
        app.process_columns.push(MetricColumn::FullPath);
        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::FullPath)),
            MetricColumn::FullPath.width() + 1
        );
    }

    #[test]
    fn shift_left_right_do_not_reorder_fixed_process_columns() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = vec![MetricColumn::PrivateBytes, MetricColumn::HandleCount];
        app.selected_process_column_index = 1;

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(
            app.process_columns,
            vec![MetricColumn::PrivateBytes, MetricColumn::HandleCount]
        );
        assert_eq!(app.selected_process_column(), SortColumn::ProcessName);
        assert_eq!(app.status, "Only metric columns can be reordered");
    }

    #[test]
    fn process_metric_columns_scroll_only_when_selection_leaves_visible_range() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = MetricColumn::ALL.to_vec();
        app.show_details = false;
        let screen = Rect::new(0, 0, 72, 45);
        app.set_screen_area(screen);
        let area = main_panel_areas_for_app(screen, &app).processes.area;
        let visible_count = process_table_visible_column_count(
            area.width,
            &app.process_columns,
            0,
            &app.process_column_widths,
        );
        assert!(visible_count < 2 + app.process_columns.len());

        app.selected_process_column_index = visible_count - 1;
        app.process_metric_column_offset = 0;
        let before = render_app_to_text(&app, screen.width, screen.height);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();

        let after_left = render_app_to_text(&app, screen.width, screen.height);
        let content_height = screen_layout(screen)[2].y as usize;
        assert_eq!(
            before.lines().take(content_height).collect::<Vec<_>>(),
            after_left.lines().take(content_height).collect::<Vec<_>>()
        );
        assert_eq!(app.selected_process_column_index, visible_count - 2);
        assert_eq!(app.process_metric_column_offset, 0);
        assert!(
            app.status.starts_with("Selected column: "),
            "{}",
            app.status
        );

        app.selected_process_column_index = visible_count - 1;
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.selected_process_column_index, visible_count);
        assert!(app.process_metric_column_offset > 0);
        let visible_range = ui::process_table_visible_metric_range(
            area.width,
            &app.process_columns,
            app.process_metric_column_offset,
            &app.process_column_widths,
        );
        assert!(visible_range.contains(&(visible_count - 2)));
    }

    #[test]
    fn left_right_does_not_select_process_metric_outside_processes() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::DetailsSamples;

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::PrivateBytes)
        );
        assert_eq!(app.details_metric, DetailsMetric::Private);
    }

    #[test]
    fn process_table_unifies_row_column_and_header_selection_surfaces() {
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(2, 10);
            app.theme_index = theme_index;
            app.snapshot.processes[0].private_bytes = Some(987_654_321);
            app.snapshot.processes[1].private_bytes = Some(123_456_789);

            let buffer = render_app_to_buffer(&app, 100, 30);
            let (row_x, row_y) = find_text_position(&buffer, "proc-0")
                .expect("selected process row should be rendered");
            let (column_x, column_y) = find_text_position(&buffer, "123.5 MB")
                .expect("selected process column should be rendered");
            let (intersection_x, intersection_y) = find_text_position(&buffer, "987.7 MB")
                .expect("selected row and column intersection should be rendered");
            let (header_x, header_y) =
                find_text_position(&buffer, "Private").expect("selected header should render");

            assert_eq!(buffer[(row_x, row_y)].bg, theme.table_selection_surface);
            assert_eq!(
                buffer[(column_x, column_y)].bg,
                theme.table_selection_surface
            );
            assert_eq!(
                buffer[(header_x, header_y)].bg,
                theme.table_selection_surface
            );
            assert_eq!(
                buffer[(intersection_x, intersection_y)].bg,
                theme.table_intersection_surface
            );
            assert_ne!(
                theme.table_intersection_surface,
                theme.table_selection_surface
            );
        }
    }

    #[test]
    fn process_table_underlines_header_name_without_underlining_sort_arrow() {
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme_index;
            app.sort = SortSpec {
                column: SortColumn::Metric(MetricColumn::PrivateBytes),
                direction: SortDirection::Desc,
            };
            let buffer = render_app_to_buffer(&app, 100, 30);
            let (x, y) = find_text_position(&buffer, "Private ↓")
                .expect("sorted process header should be rendered");
            let (pid_x, pid_y) =
                find_text_position(&buffer, "PID").expect("ordinary process header should render");

            assert_eq!(buffer[(pid_x, pid_y)].bg, theme.panel);
            assert_eq!(buffer[(x, y)].bg, theme.table_selection_surface);

            for offset in 0.."Private".len() as u16 {
                assert!(
                    buffer[(x + offset, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::UNDERLINED)
                );
            }
            assert!(
                !buffer[(x + "Private ".len() as u16, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
            );
        }
    }

    #[test]
    fn process_table_dotnet_header_keeps_sort_arrow_at_compact_width() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![MetricColumn::DotNetHeapBytes];
        app.sort = SortSpec {
            column: SortColumn::Metric(MetricColumn::DotNetHeapBytes),
            direction: SortDirection::Desc,
        };

        let buffer = render_app_to_buffer(&app, 100, 30);

        assert!(find_text_position(&buffer, ".NET Heap ↓").is_some());
    }

    #[test]
    fn neutral_first_header_and_footer_roles_apply_to_both_themes() {
        for theme_index in 0..ui::THEMES.len() {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme_index;
            let theme = ui::THEMES[theme_index];
            let buffer = render_app_to_buffer(&app, 100, 30);

            let product_and_version = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));
            let (product_x, product_y) = find_text_position(&buffer, &product_and_version)
                .expect("product and version should render when the header has room");
            assert_eq!(
                product_x + product_and_version.len() as u16,
                buffer.area.width
            );
            assert_eq!(product_y, 0);
            assert_eq!(buffer[(product_x, product_y)].fg, theme.muted);
            assert_eq!(buffer[(product_x, product_y)].bg, theme.panel);

            let (live_x, live_y) =
                find_text_position(&buffer, "LIVE").expect("live badge should be rendered");
            assert_eq!(live_y, 0);
            assert_eq!(buffer[(live_x, live_y)].fg, theme.background);
            assert_eq!(buffer[(live_x, live_y)].bg, theme.success);

            let (shortcut_x, shortcut_y) = find_text_position(&buffer, "c Columns")
                .expect("process shortcut should be rendered");
            assert_eq!(buffer[(shortcut_x, shortcut_y)].fg, theme.muted);
        }
    }

    #[test]
    fn process_table_renders_live_metric_values_neutrally() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].private_bytes = Some(987_654_321);
        app.process_table_state.select(None);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) =
            find_text_position(&buffer, "987.7 MB").expect("private bytes should be rendered");

        assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].text);
        assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn process_table_highlights_graph_metric_cell_and_keeps_tracked_name_plain() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[0].private_bytes = Some(107_374_182_400);
        app.selected_process_column_index = 1;
        app.process_table_state.select(None);
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = std::collections::HashSet::from(["target.exe".to_string()]);
        app.rebuild_visible_process_cache();
        app.add_or_reveal_graph_source(
            GraphSlot::process(
                ProcessIdentity::from_row(&app.snapshot.processes[0]),
                DetailsMetric::Private,
            ),
            FocusedPanel::Processes,
        );
        app.show_details = false;

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (value_x, value_y) = find_text_position(&buffer, "107.4GB")
            .expect("graphed private bytes should be rendered");
        let (name_x, name_y) =
            find_text_position(&buffer, "target.exe").expect("tracked name should be rendered");
        let tracked_x = (0..name_x)
            .find(|&x| buffer[(x, name_y)].symbol() == "T")
            .expect("tracked marker should be rendered beside the process name");
        let graph_slot_number_x = (1..MetricColumn::PrivateBytes.width())
            .find_map(|offset| {
                let x = value_x.checked_sub(offset)?;
                (buffer[(x, value_y)].symbol() == "1").then_some(x)
            })
            .expect("Graph slot number should be rendered in the metric cell");
        let graph_slot_number_cell = &buffer[(graph_slot_number_x, value_y)];
        let value_cell = &buffer[(value_x, value_y)];
        let tracked_cell = &buffer[(tracked_x, name_y)];

        assert_eq!(graph_slot_number_cell.symbol(), "1");
        assert_eq!(graph_slot_number_cell.fg, ui::THEMES[0].active_series);
        assert_ne!(graph_slot_number_cell.bg, ui::THEMES[0].active_series);
        assert!(graph_slot_number_cell.modifier.contains(Modifier::BOLD));
        assert_eq!(value_cell.fg, ui::THEMES[0].text);
        assert_ne!(value_cell.bg, ui::THEMES[0].warning);
        assert_eq!(buffer[(name_x, name_y)].fg, ui::THEMES[0].text);
        assert_eq!(tracked_cell.fg, ui::THEMES[0].background);
        assert_eq!(tracked_cell.bg, ui::THEMES[0].tracked);
        assert!(!tracked_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn process_table_keeps_tracked_badge_visible_on_the_selected_row() {
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme_index;
            app.snapshot.processes[0].name = "target.exe".to_string();
            track_process_name(&mut app, "target.exe");

            let buffer = render_app_to_buffer(&app, 100, 30);
            let (name_x, name_y) =
                find_text_position(&buffer, "target.exe").expect("tracked name should render");
            let marker_x = (0..name_x)
                .find(|&x| buffer[(x, name_y)].symbol() == "T")
                .expect("tracked badge should render on the selected row");
            let marker = &buffer[(marker_x, name_y)];

            assert_eq!(marker.fg, theme.background);
            assert_eq!(marker.bg, theme.tracked);
            assert!(!marker.modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn process_table_current_row_uses_background_without_cursor_symbol() {
        let app = make_test_app(1, 10);
        let buffer = render_app_to_buffer(&app, 100, 30);
        let rendered = buffer_to_text(&buffer);
        let (name_x, name_y) =
            find_text_position(&buffer, "proc-0").expect("current process row should render");

        assert!(!rendered.contains(">>"), "{rendered}");
        assert_eq!(
            buffer[(name_x, name_y)].bg,
            ui::THEMES[0].table_selection_surface
        );
    }

    #[test]
    fn process_table_marks_only_names_that_are_actually_truncated() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![MetricColumn::FullPath];
        app.process_column_widths.set(SortColumn::ProcessName, 8);
        app.snapshot.processes[0].name = "process-name.exe".to_string();
        app.snapshot.processes[0].executable_path = Some(r"C:\app.exe".to_string());

        let truncated = render_app_to_text(&app, 100, 30);

        assert!(truncated.contains("process⋯"), "{truncated}");

        app.snapshot.processes[0].name = "app.exe".to_string();
        let complete = render_app_to_text(&app, 100, 30);

        assert!(complete.contains("app.exe"), "{complete}");
        assert!(!complete.contains('⋯'), "{complete}");
    }

    #[test]
    fn process_name_width_shortcuts_change_the_rendered_width_immediately() {
        let screen = Rect::new(0, 0, 100, 30);
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = vec![MetricColumn::PrivateBytes];
        app.process_column_widths.set(SortColumn::ProcessName, 8);
        app.selected_process_column_index = 1;
        app.snapshot.processes[0].name = "process-name.exe".to_string();
        app.set_screen_area(screen);

        let initial = render_app_to_text(&app, screen.width, screen.height);
        assert!(initial.contains("process⋯"), "{initial}");

        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
            .unwrap();
        let widened = render_app_to_text(&app, screen.width, screen.height);
        assert!(widened.contains("process-⋯"), "{widened}");
        assert!(!widened.contains("process⋯"), "{widened}");

        app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
            .unwrap();
        let narrowed = render_app_to_text(&app, screen.width, screen.height);
        assert!(narrowed.contains("process⋯"), "{narrowed}");
    }

    #[test]
    fn process_table_overflow_indicator_tracks_offsets_and_custom_widths() {
        let screen = Rect::new(0, 0, 72, 30);
        let mut app = make_test_app(1, 10);
        app.process_columns = MetricColumn::ALL.to_vec();
        app.show_details = false;

        let area = ui::main_panel_areas_for_app(screen, &app).processes.area;
        let leading_range = ui::process_table_visible_metric_range(
            area.width,
            &app.process_columns,
            app.process_metric_column_offset,
            &app.process_column_widths,
        );
        let leading_indicator = format!(
            "‹ {}–{}/{} ›",
            leading_range.start + 1,
            leading_range.end,
            app.process_columns.len()
        );
        let leading = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, _) = find_text_position(&leading, &leading_indicator)
            .expect("the leading visible metric range should render");
        assert_eq!(
            x + leading_indicator.chars().count() as u16,
            area.right().saturating_sub(1)
        );

        app.process_metric_column_offset = 10;
        let offset_range = ui::process_table_visible_metric_range(
            area.width,
            &app.process_columns,
            app.process_metric_column_offset,
            &app.process_column_widths,
        );
        let offset_indicator = format!(
            "‹ {}–{}/{} ›",
            offset_range.start + 1,
            offset_range.end,
            app.process_columns.len()
        );
        let offset = render_app_to_text(&app, screen.width, screen.height);
        assert!(offset.contains(&offset_indicator), "{offset}");

        app.process_metric_column_offset = 0;
        app.process_column_widths.set(SortColumn::ProcessName, 40);
        let custom_range = ui::process_table_visible_metric_range(
            area.width,
            &app.process_columns,
            app.process_metric_column_offset,
            &app.process_column_widths,
        );
        let custom_indicator = format!(
            "‹ {}–{}/{} ›",
            custom_range.start + 1,
            custom_range.end,
            app.process_columns.len()
        );
        let custom = render_app_to_text(&app, screen.width, screen.height);
        assert!(custom.contains(&custom_indicator), "{custom}");
        assert!(custom_range.end < leading_range.end);
    }

    #[test]
    fn process_table_overflow_indicator_handles_zero_or_no_hidden_metrics() {
        let mut app = make_test_app(1, 10);
        app.process_columns = MetricColumn::ALL.to_vec();

        let narrow_buffer = render_app_to_buffer(&app, 35, 20);
        let narrow = buffer_to_text(&narrow_buffer);
        assert!(narrow.contains("‹ 0/15 ›"), "{narrow}");
        let (indicator_x, indicator_y) = find_text_position(&narrow_buffer, "‹ 0/15 ›")
            .expect("the zero-column indicator should render");
        app.on_mouse(
            left_click(indicator_x, indicator_y),
            Rect::new(0, 0, 35, 20),
        );
        assert!(!app.watch_enabled);

        let wide = render_app_to_text(&app, 400, 20);
        assert!(!wide.contains("‹ 1–15/15 ›"), "{wide}");
        assert!(!wide.contains("‹ 0/15 ›"), "{wide}");
    }

    #[test]
    fn process_table_mouse_click_selects_row_and_metric_column() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![
            MetricColumn::PrivateBytes,
            MetricColumn::ThreadCount,
            MetricColumn::HandleCount,
        ];
        app.process_column_widths.set(SortColumn::Pid, 8);
        app.process_column_widths.set(SortColumn::ProcessName, 27);
        app.process_column_widths.set(
            SortColumn::Metric(MetricColumn::ThreadCount),
            MetricColumn::ThreadCount.width() + 4,
        );
        app.selected_process_column_index = 2;
        app.snapshot.processes[1].thread_count = Some(77);
        app.snapshot.processes[1].handle_count = Some(888);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, _) =
            find_text_position(&buffer, "Hndl").expect("target column header should be rendered");
        let (_, y) =
            find_text_position(&buffer, "888").expect("target handle count should be rendered");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 100, 30),
        );

        assert_eq!(app.process_table_state.selected(), Some(1));
        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::HandleCount)
        );
        assert_eq!(app.details_metric, DetailsMetric::Private);
    }

    #[test]
    fn process_table_resize_preserves_custom_column_widths() {
        let mut app = make_test_app(2, 10);
        app.process_column_widths.set(
            SortColumn::Metric(MetricColumn::PrivateBytes),
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX,
        );

        let _ = render_app_to_buffer(&app, 40, 20);
        let _ = render_app_to_buffer(&app, 160, 40);

        assert_eq!(
            app.process_column_widths
                .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX
        );
    }

    #[test]
    fn process_table_tracked_only_title_checkbox_click_toggles_filter() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        let screen = Rect::new(0, 0, 120, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "☐ Tracked-only(Shift+T)")
            .expect("unchecked tracked-only control should render in the process title");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(app.watch_enabled);
        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(app.visible_process_at(0).unwrap().name, "target.exe");
        assert_eq!(
            app.tracked_total_visible_row().unwrap().process.name,
            "Tracked Total"
        );

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "☑ Tracked-only(Shift+T)")
            .expect("checked tracked-only control should render in the process title");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(!app.watch_enabled);
        assert_eq!(app.visible_process_count(), 2);
    }

    #[test]
    fn ram_vram_enter_does_not_assign_graph_metric() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_system_metric(), SystemMetric::Committed);
        assert_eq!(app.details_target, DetailsTarget::Process);

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.details_target, DetailsTarget::Process);
        assert!(!app.show_details);
        assert!(app.status.contains("Committed"));
    }

    #[test]
    fn ram_vram_up_down_only_selects_system_metric() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.selected_system_metric(), SystemMetric::Committed);
        assert_eq!(app.details_target, DetailsTarget::Process);
        assert!(!app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.selected_system_metric(), SystemMetric::PhysicalMemory);
        assert_eq!(app.details_target, DetailsTarget::Process);
    }

    #[test]
    fn ram_vram_space_toggles_selected_graph() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.watch_list.is_empty());
        assert_eq!(app.graph_entries.len(), 1);
        assert_eq!(
            app.active_graph_slot(),
            Some(&GraphSlot::system(SystemMetric::Committed))
        );
        assert!(app.show_details);
        assert_eq!(app.focused_panel, FocusedPanel::System);
    }

    #[test]
    fn ram_vram_graph_number_uses_slot_ordinal_and_active_series_color() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.active_graph_slot(),
            Some(&GraphSlot::system(SystemMetric::Committed))
        );
        assert!(app.show_details);

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (x, y) = find_text_position(&buffer, "1  Committed")
            .expect("RAM Graph slot number should render");
        let marker = &buffer[(x, y)];
        assert_eq!(marker.fg, ui::THEMES[0].active_series);
        assert_ne!(marker.bg, ui::THEMES[0].active_series);
        assert!(marker.modifier.contains(Modifier::BOLD));
        let rendered = buffer_to_text(&buffer);
        assert!(
            rendered.contains("Slot#1 · Committed · SYSTEM"),
            "{rendered}"
        );
    }

    #[test]
    fn ram_vram_inactive_two_digit_graph_number_matches_process_style() {
        let mut app = make_test_app(3, 10);
        let ids = (0..9)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::system(SystemMetric::GpuDedicated),
            FocusedPanel::System,
        ));
        assert!(app.set_active_graph(ids[0]));

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (x, y) = find_text_position(&buffer, "10 GPU Dedicated")
            .expect("two-digit RAM Graph slot number should keep a label gap");
        let marker = &buffer[(x, y)];

        assert_eq!(marker.fg, ui::THEMES[0].graph_line);
        assert!(marker.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn ram_vram_panel_excludes_cache_standby_and_separates_gpu_rows() {
        let app = make_test_app(3, 10);

        let rendered = render_app_to_text(&app, 120, 30);

        assert!(rendered.contains("RAM/VRAM"), "{rendered}");
        assert!(!rendered.contains("[Max samples: 7200]"), "{rendered}");
        assert!(rendered.contains("Physical Memory"), "{rendered}");
        assert!(rendered.contains("Committed"), "{rendered}");
        assert!(rendered.contains("GPU Dedicated"), "{rendered}");
        assert!(rendered.contains("GPU Shared"), "{rendered}");
        assert!(rendered.contains("────────"), "{rendered}");
        assert!(!rendered.contains("Standby"), "{rendered}");
    }

    #[test]
    fn system_activity_panel_shows_network_disk_and_queue_metrics() {
        let mut app = make_test_app(3, 10);
        app.snapshot.network_received_bytes_per_sec = Some(30_000_000);
        app.snapshot.network_sent_bytes_per_sec = Some(40_000_000);
        app.snapshot.disk_read_bytes_per_sec = Some(10_000_000);
        app.snapshot.disk_write_bytes_per_sec = Some(20_000_000);
        app.snapshot.disk_queue_length = Some(1.5);

        let rendered = render_app_to_text(&app, 120, 30);

        assert!(rendered.contains("NW/DISK"), "{rendered}");
        assert!(
            rendered.find("RAM/VRAM").unwrap() < rendered.find("NW/DISK").unwrap(),
            "{rendered}"
        );
        assert!(
            rendered.find("NW/DISK").unwrap() < rendered.find("CPUS").unwrap(),
            "{rendered}"
        );
        assert!(rendered.contains("Net Rx   240 Mbps"), "{rendered}");
        assert!(rendered.contains("Net Tx   320 Mbps"), "{rendered}");
        assert!(rendered.contains("Disk R    10 MB/s"), "{rendered}");
        assert!(rendered.contains("Disk W    20 MB/s"), "{rendered}");
        assert!(rendered.contains("Disk Q     2"), "{rendered}");
    }

    #[test]
    fn system_activity_space_assigns_graph_and_shows_slot_number() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::SystemActivity;
        app.snapshot.disk_queue_length = Some(1.5);
        app.system_history.record_snapshot(&app.snapshot);

        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.selected_system_activity_metric(),
            SystemMetric::DiskQueueLength
        );

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.active_graph_slot(),
            Some(&GraphSlot::system(SystemMetric::DiskQueueLength))
        );
        assert!(app.show_details);
        assert_eq!(
            app.active_graph_slot().map(GraphSlot::value_format),
            Some(GraphValueFormat::QueueLength)
        );
        assert_eq!(
            app.graph_slot_samples(app.active_graph_slot().unwrap())
                .last()
                .and_then(|sample| sample.value),
            Some(1.5)
        );

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("1  Disk Q"), "{rendered}");
        assert!(rendered.contains("Slot#1 · Disk Q · SYSTEM"), "{rendered}");

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (x, y) = find_text_position(&buffer, "1  Disk Q")
            .expect("NW/DISK Graph slot number should render");
        let marker = &buffer[(x, y)];
        assert_eq!(marker.fg, ui::THEMES[0].active_series);
        assert_ne!(marker.bg, ui::THEMES[0].active_series);
        assert!(marker.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn process_enter_does_not_assign_graph_metric() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = ColumnPreset::Resources.columns().to_vec();
        app.selected_process_column_index = 4;
        app.select_process_index(2);
        app.details_target = DetailsTarget::System(SystemMetric::Committed);

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.details_target,
            DetailsTarget::System(SystemMetric::Committed)
        );
        assert_eq!(app.details_metric, DetailsMetric::Private);
        assert!(!app.show_details);
        assert_eq!(
            app.selected_process_identity
                .as_ref()
                .map(|identity| identity.name.as_str()),
            Some("proc-2")
        );
        assert!(app.show_process_info_dialog);
        assert!(app.pending_process_info.is_some());

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_process_info_dialog);
    }

    #[test]
    fn selected_process_metric_column_updates_details_metric() {
        let mut app = make_test_app(3, 10);
        app.process_columns = ColumnPreset::Resources.columns().to_vec();
        app.selected_process_column_index = 3;

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.selected_process_column(),
            SortColumn::Metric(MetricColumn::ThreadCount)
        );
        assert_eq!(app.details_metric, DetailsMetric::Private);
    }

    #[test]
    fn full_path_column_is_not_graphable() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.process_columns = vec![MetricColumn::FullPath];
        app.selected_process_column_index = 2;
        app.select_process_index(0);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_entries.is_empty());
        assert_eq!(app.details_metric, DetailsMetric::Private);
        assert_eq!(app.status, "Select a graphable metric cell");
    }

    #[test]
    fn sort_uses_selected_process_metric_column() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].private_bytes = Some(10);
        app.snapshot.processes[1].private_bytes = Some(30);
        app.snapshot.processes[2].private_bytes = Some(20);
        app.selected_process_column_index = 2;

        app.cycle_sort_column();

        assert_eq!(
            app.sort.column,
            SortColumn::Metric(MetricColumn::PrivateBytes)
        );
        assert_eq!(app.snapshot.processes[0].private_bytes, Some(30));
        assert!(!app.is_display_paused());
    }

    #[test]
    fn sort_uses_selected_pid_column() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].pid = 30;
        app.snapshot.processes[1].pid = 10;
        app.snapshot.processes[2].pid = 20;
        app.selected_process_column_index = 0;

        app.cycle_sort_column();

        assert_eq!(app.sort.column, SortColumn::Pid);
        assert_eq!(app.sort.direction, SortDirection::Asc);
        assert_eq!(app.snapshot.processes[0].pid, 10);
        assert!(!app.is_display_paused());
    }

    #[test]
    fn sort_uses_selected_process_name_column() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "zeta.exe".to_string();
        app.snapshot.processes[1].name = "alpha.exe".to_string();
        app.snapshot.processes[2].name = "mid.exe".to_string();
        app.selected_process_column_index = 1;

        app.cycle_sort_column();

        assert_eq!(app.sort.column, SortColumn::ProcessName);
        assert_eq!(app.sort.direction, SortDirection::Asc);
        assert_eq!(app.snapshot.processes[0].name, "alpha.exe");
        assert!(!app.is_display_paused());
    }

    #[test]
    fn sample_refresh_resorts_process_rows_when_order_is_unlocked() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(3, 10, sampling_worker);
        app.snapshot.processes[0].private_bytes = Some(10);
        app.snapshot.processes[1].private_bytes = Some(30);
        app.snapshot.processes[2].private_bytes = Some(20);
        app.selected_process_column_index = 2;
        app.cycle_sort_column();
        let sorted_pids = app
            .snapshot
            .processes
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        assert_eq!(sorted_pids, vec![1, 2, 0]);

        let mut next = test_snapshot(3);
        next.processes[0].private_bytes = Some(100);
        next.processes[1].private_bytes = Some(30);
        next.processes[2].private_bytes = Some(20);
        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();

        app.poll_sample_results().unwrap();

        let refreshed_pids = app
            .snapshot
            .processes
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        assert_eq!(refreshed_pids, vec![0, 1, 2]);
    }

    #[test]
    fn sample_refresh_keeps_process_order_while_navigation_is_active() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(3, 10, sampling_worker);
        app.snapshot.processes[0].private_bytes = Some(10);
        app.snapshot.processes[1].private_bytes = Some(30);
        app.snapshot.processes[2].private_bytes = Some(20);
        app.selected_process_column_index = 2;
        app.cycle_sort_column();
        app.select_first_row();
        app.move_selection_down(1);
        assert_eq!(app.process_table_state.selected(), Some(1));
        let sorted_pids = app
            .snapshot
            .processes
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        assert_eq!(sorted_pids, vec![1, 2, 0]);

        let mut next = test_snapshot(3);
        next.processes[0].private_bytes = Some(100);
        next.processes[1].private_bytes = Some(30);
        next.processes[2].private_bytes = Some(20);
        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();

        app.poll_sample_results().unwrap();

        let refreshed_pids = app
            .snapshot
            .processes
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        assert_eq!(refreshed_pids, vec![1, 2, 0]);
        assert_eq!(app.process_table_state.selected(), Some(1));
    }

    #[test]
    fn paused_display_freezes_visible_metrics_while_histories_continue() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(3, 10, sampling_worker);
        app.snapshot.used_memory = 10;
        app.snapshot.processes[0].private_bytes = Some(10);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
        app.rebuild_visible_process_cache();
        let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);

        app.toggle_display_pause();
        let mut next = test_snapshot(3);
        next.used_memory = 99;
        next.processes[0].private_bytes = Some(99);
        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();

        assert!(!app.poll_sample_results().unwrap());

        assert_eq!(app.snapshot.used_memory, 99);
        assert_eq!(app.snapshot.processes[0].private_bytes, Some(99));
        assert_eq!(app.display_snapshot().used_memory, 10);
        assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(10));
        assert_eq!(app.process_history.sample_count_for(&identity), 2);
        assert_eq!(app.display_process_history().sample_count_for(&identity), 1);
        assert_eq!(app.system_history.len(), 2);
        assert_eq!(app.display_system_history().len(), 1);
    }

    #[test]
    fn unpausing_display_resumes_latest_snapshot() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(3, 10, sampling_worker);
        app.snapshot.processes[0].private_bytes = Some(10);
        app.rebuild_visible_process_cache();
        app.toggle_display_pause();

        let mut next = test_snapshot(3);
        next.processes[0].private_bytes = Some(99);
        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();
        assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(10));

        app.toggle_display_pause();

        assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(99));
        assert!(!app.is_display_paused());
        assert_eq!(app.status, "Display resumed");
    }

    #[test]
    fn ctrl_p_toggles_display_pause_from_any_panel() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.is_display_paused());
        assert_eq!(app.status, "Display paused");

        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.is_display_paused());
        assert_eq!(app.status, "Display resumed");
    }

    #[test]
    fn l_does_not_toggle_display_pause() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.is_display_paused());
    }

    #[test]
    fn ab_keys_set_points_instead_of_starting_filter() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.ab_comparison.as_ref().and_then(|ab| ab.b).is_some());
        assert!(!app.filter_editing);
    }

    #[test]
    fn ab_clear_key_clears_comparison() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.ab_comparison.is_none());
        assert!(app.status.contains("cleared"));
    }

    #[test]
    fn ab_keys_keep_current_focus() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::Processes;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.focused_panel, FocusedPanel::Processes);
    }

    #[test]
    fn shifted_ab_keys_jump_selection_to_points() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        let base = Local::now();
        for (seconds, value) in [(0, 10), (1, 20), (2, 30)] {
            app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
            app.snapshot.processes[0].private_bytes = Some(value);
            app.process_history.record_snapshot(
                app.snapshot.captured_at,
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        app.set_details_sample_selected(0);
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();
        app.set_details_sample_selected(2);
        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();
        app.set_details_sample_selected(1);

        app.on_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.details_sample_selected, 0);

        app.on_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.details_sample_selected, 2);
    }

    #[test]
    fn ab_key_does_not_open_details_panel() {
        let mut app = make_test_app(1, 10);
        let status = app.status.clone();

        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.ab_comparison.is_none());
        assert!(!app.show_details);
        assert_eq!(app.status, status);
    }

    #[test]
    fn help_closes_with_escape_enter_or_question_mark() {
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        ] {
            let mut app = make_test_app(1, 10);
            app.show_help = true;

            app.on_key(key).unwrap();

            assert!(!app.show_help);
            assert!(!app.show_quit_confirmation);
        }
    }

    #[test]
    fn help_blocks_normal_shortcuts_while_open() {
        let mut app = make_test_app(1, 10);
        app.show_help = true;

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_help);
        assert!(!app.show_quit_confirmation);
    }

    #[test]
    fn help_dialog_buffer_shows_two_column_layout() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;

        let rendered = render_app_to_text(&app, 150, 70);
        let rendered_lower = rendered.to_ascii_lowercase();

        assert!(
            rendered.contains(&format!(
                "winproc-tui {} · Keyboard shortcuts",
                env!("CARGO_PKG_VERSION")
            )),
            "{rendered}"
        );
        assert!(rendered.contains("Keyboard shortcuts"), "{rendered}");
        assert!(
            rendered.contains("History: 120/7,200 normal/tracked"),
            "{rendered}"
        );
        assert!(rendered.contains("Global  (any focus)"), "{rendered}");
        assert!(rendered.contains("Processes"), "{rendered}");
        assert!(rendered.contains("RAM/VRAM"), "{rendered}");
        assert!(rendered.contains("NW/DISK"), "{rendered}");
        assert!(
            rendered.contains("Graph Workspace  (Graph focus)"),
            "{rendered}"
        );
        assert!(rendered.contains("Samples"), "{rendered}");
        assert!(
            rendered.contains("Tracking  (Processes focus)"),
            "{rendered}"
        );
        assert!(
            rendered.contains("A/B comparison  (Graph or Samples)"),
            "{rendered}"
        );
        assert!(rendered.contains("Mouse"), "{rendered}");
        assert!(!rendered.contains("▋"), "{rendered}");

        assert!(rendered.contains("Set A at sample"), "{rendered}");
        assert!(rendered.contains("Set B at sample"), "{rendered}");
        assert!(rendered.contains("Jump to A or B"), "{rendered}");
        assert!(rendered.contains("Clear A/B comparison"), "{rendered}");

        assert!(rendered.contains("Pan time range"), "{rendered}");
        assert!(
            rendered.contains("Select previous Graph slot"),
            "{rendered}"
        );
        assert!(rendered.contains("Select next Graph slot"), "{rendered}");
        assert!(rendered.contains("Select older sample"), "{rendered}");
        assert!(rendered.contains("Select newer sample"), "{rendered}");
        assert!(rendered.contains("Remove active Graph"), "{rendered}");
        assert!(
            rendered.contains("f/z") && rendered.contains("Fit all / compact Min0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("v/d/l") && rendered.contains("Samples / Delta / layout"),
            "{rendered}"
        );
        assert!(rendered.contains("Toggle recording"), "{rendered}");
        assert!(rendered.contains("Pause / Resume"), "{rendered}");
        assert!(rendered.contains("Copy selected row"), "{rendered}");
        assert!(!rendered.contains("Open Settings"), "{rendered}");

        assert!(rendered.contains("Select row range"), "{rendered}");
        assert!(rendered.contains("Toggle row selection"), "{rendered}");
        assert!(
            rendered.contains("Kill selected live process"),
            "{rendered}"
        );
        assert!(rendered.contains("Info/detail / Files"), "{rendered}");
        assert!(rendered.contains("Switch Info tabs"), "{rendered}");
        assert!(rendered.contains("Refresh Info tab"), "{rendered}");

        assert!(rendered.contains("Click panel"), "{rendered}");
        assert!(rendered.contains("Samples auto-scroll"), "{rendered}");
        assert!(rendered.contains("Shift+Wheel"), "{rendered}");
        assert!(rendered.contains("Change Graph time span"), "{rendered}");

        assert!(!rendered.contains("Details panel"), "{rendered}");
        assert!(!rendered.contains("Dialogs"), "{rendered}");
        assert!(!rendered.contains("Recording path"), "{rendered}");
        assert!(!rendered.contains("Sampling interval"), "{rendered}");
        assert!(
            !rendered.contains("Esc / Enter closes this help dialog."),
            "{rendered}"
        );
        assert!(!rendered.contains("F6"), "{rendered}");
        assert!(rendered.contains("[ Close ]"), "{rendered}");
        assert!(rendered.contains("Footer: focused actions."), "{rendered}");
        assert!(
            rendered.contains("Neutral selects; amber marks."),
            "{rendered}"
        );
        assert!(!rendered_lower.contains("baseline"), "{rendered}");
    }

    #[test]
    fn help_dialog_header_and_shortcuts_use_footer_like_styles() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;

        let buffer = render_app_to_buffer(&app, 100, 45);
        let theme = ui::THEMES[0];

        let title = format!(
            "winproc-tui {} · Keyboard shortcuts",
            env!("CARGO_PKG_VERSION")
        );
        let (title_x, title_y) =
            find_text_position(&buffer, &title).expect("help dialog title should be rendered");
        assert_eq!(title_x, help_area(Rect::new(0, 0, 100, 45)).x + 2);
        let title_cell = &buffer[(title_x, title_y)];
        assert_eq!(title_cell.fg, theme.text);
        assert_ne!(title_cell.fg, theme.accent);
        assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));

        let (group_x, group_y) =
            find_text_position(&buffer, "Global").expect("group title should be rendered");
        let group_cell = &buffer[(group_x, group_y)];
        assert_eq!(group_cell.symbol(), "G");
        assert_eq!(group_cell.fg, theme.accent);
        assert!(group_cell.modifier.contains(ratatui::style::Modifier::BOLD));
        assert!(
            !group_cell
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );

        let (key_x, key_y) =
            find_text_position(&buffer, "Ctrl+F").expect("shortcut key should be rendered");
        let key_cell = &buffer[(key_x, key_y)];
        assert_eq!(key_cell.fg, theme.text);
        assert_eq!(key_cell.bg, theme.panel_alt);
        assert!(key_cell.modifier.contains(ratatui::style::Modifier::BOLD));

        let label_cell = &buffer[(key_x + "Ctrl+F ".len() as u16, key_y)];
        assert_eq!(label_cell.fg, theme.text);
    }

    #[test]
    fn help_dialog_panel_fits_rendered_content() {
        let screen = Rect::new(0, 0, 120, 50);
        let popup = help_area(screen);

        assert!(popup.width <= screen.width);
        assert!(popup.height <= screen.height);
        assert!(popup.width >= 50, "popup too narrow: {popup:?}");
        assert!(popup.height >= 25, "popup too short: {popup:?}");
    }

    #[test]
    fn help_dialog_scrolls_when_content_overflows() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;
        let screen = Rect::new(0, 0, 100, 20);

        let top_rendered = render_app_to_text(&app, screen.width, screen.height);
        let top_buffer = render_app_to_buffer(&app, screen.width, screen.height);

        assert!(
            top_rendered.contains("Keyboard shortcuts"),
            "{top_rendered}"
        );
        assert!(top_rendered.contains("Global"), "{top_rendered}");
        assert!(
            find_symbol_position(&top_buffer, "█").is_some(),
            "{top_rendered}"
        );

        app.set_help_page_size(ui::help_page_size_for_screen(screen));
        app.scroll_help_end();
        let bottom_rendered = render_app_to_text(&app, screen.width, screen.height);

        assert!(bottom_rendered.contains("[ Close ]"), "{bottom_rendered}");
        assert!(
            !bottom_rendered.contains("Esc / Enter closes this help dialog."),
            "{bottom_rendered}"
        );
    }

    #[test]
    fn help_dialog_keyboard_scroll_updates_offset() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;
        app.set_help_page_size(ui::help_page_size_for_screen(Rect::new(0, 0, 100, 20)));

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.help_scroll.offset, 1);

        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();
        assert!(app.help_scroll.offset > 1);

        app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.help_scroll.offset, 0);
    }

    #[test]
    fn help_dialog_scrollbar_drag_scrolls_content() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;
        let screen = Rect::new(0, 0, 100, 20);
        app.set_help_page_size(ui::help_page_size_for_screen(screen));
        let scrollbar = help_scrollbar_area(screen, app.help_scroll.page_size)
            .expect("small help dialog should have a scrollbar");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.help_scroll.dragging);
        assert_eq!(app.help_scroll.offset, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.bottom().saturating_sub(1),
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.help_scroll.offset > 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.bottom().saturating_sub(1),
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(!app.help_scroll.dragging);
    }

    #[test]
    fn footer_shows_process_context_on_one_row() {
        let mut app = make_test_app(3, 10);
        app.status = "Copied row: proc-0".to_string();

        let rendered = render_app_to_text(&app, 260, 30);

        assert!(rendered.contains("PROCESSES"), "{rendered}");
        assert!(rendered.contains("Ctrl+P Pause"), "{rendered}");
        assert!(rendered.contains("Ctrl+T Lists"), "{rendered}");
        assert!(rendered.contains("c Columns"), "{rendered}");
        assert!(rendered.contains("s Sort"), "{rendered}");
        assert!(rendered.contains("g Graphs"), "{rendered}");
        assert!(rendered.contains("Ctrl+I Jump"), "{rendered}");
        assert!(!rendered.contains("Shift+←/→ Move column"), "{rendered}");
        assert!(rendered.contains("Space Graph"), "{rendered}");
        assert!(rendered.contains("Enter/f Info/Files"), "{rendered}");
        assert!(rendered.contains("t Track"), "{rendered}");
        assert!(rendered.contains("Shift+T Tracked-only"), "{rendered}");
        assert!(rendered.contains("d Kill"), "{rendered}");
        assert!(rendered.contains("Ctrl+F Filter"), "{rendered}");
        assert!(rendered.contains("Esc Quit"), "{rendered}");
        assert!(!rendered.contains("Tab Focus"), "{rendered}");
        assert!(rendered.contains("? Help"), "{rendered}");
        assert!(!rendered.contains("Status  "), "{rendered}");
        assert!(!rendered.contains("Copied row: proc-0"), "{rendered}");
        assert!(!rendered.contains("Up/Down Row"), "{rendered}");
        assert!(!rendered.contains("Left/Right Column"), "{rendered}");
        assert!(!rendered.contains("Ctrl+R Record"), "{rendered}");
        assert!(!rendered.contains("Ctrl+O Settings"), "{rendered}");
    }

    #[test]
    fn footer_keeps_primary_action_visible_at_narrow_width() {
        let app = make_test_app(3, 10);
        let buffer = render_app_to_buffer(&app, 30, 24);
        let footer = Rect::new(0, 23, 30, 1);
        let (action_x, action_y) = find_text_position_in_area(&buffer, footer, "Space Graph")
            .expect("primary action should remain visible");

        assert_eq!(action_x, 0);
        assert_eq!(action_y, footer.y);
        assert!(find_text_position_in_area(&buffer, footer, "PROCESSES").is_none());
        assert!(find_text_position(&buffer, "Ctrl+T Lists").is_none());
    }

    #[test]
    fn footer_shortcuts_follow_the_focused_panel() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;
        let system = render_app_to_text(&app, 170, 30);
        assert!(system.contains("i System info"), "{system}");
        assert!(!system.contains("Up/Down Metric"), "{system}");
        assert!(!system.contains("Left/Right Column"), "{system}");

        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;
        let graph = render_app_to_text(&app, 260, 45);
        assert!(graph.contains("↑ Prev Slot"), "{graph}");
        assert!(graph.contains("↓ Next Slot"), "{graph}");
        assert!(graph.contains("Del Remove Graph"), "{graph}");
        assert!(graph.contains("← Older"), "{graph}");
        assert!(graph.contains("→ Newer"), "{graph}");
        assert!(graph.contains("Enter Info"), "{graph}");
        assert!(graph.contains("Ctrl+←/→ Pan"), "{graph}");
        assert!(graph.contains("PgUp/PgDn/Shift+Wheel Span"), "{graph}");
        assert!(graph.contains("f/z Fit/Min 0"), "{graph}");
        assert!(graph.contains("a/b Set A/B"), "{graph}");
        assert!(graph.contains("Shift+A/B Jump A/B"), "{graph}");

        app.focused_panel = FocusedPanel::DetailsSamples;
        let samples = render_app_to_text(&app, 260, 45);
        assert!(samples.contains("↑/← Older"), "{samples}");
        assert!(samples.contains("↓/→ Newer"), "{samples}");
        assert!(samples.contains("Del Remove Graph"), "{samples}");
        assert!(samples.contains("PgUp/PgDn Scroll"), "{samples}");
        assert!(samples.contains("Home/End Edge"), "{samples}");
        assert!(samples.contains("f/z Fit/Min 0"), "{samples}");
        assert!(samples.contains("Shift+A/B Jump A/B"), "{samples}");
        assert!(samples.contains("x Clear A/B"), "{samples}");
    }

    #[test]
    fn footer_shows_pause_and_omits_tab_for_every_focused_panel() {
        let mut app = make_test_app(3, 10);

        for focused_panel in [
            FocusedPanel::System,
            FocusedPanel::SystemActivity,
            FocusedPanel::Cpu,
            FocusedPanel::Processes,
            FocusedPanel::DetailsGraph,
            FocusedPanel::DetailsSamples,
        ] {
            app.focused_panel = focused_panel;
            let rendered = render_app_to_text(&app, 420, 45);
            assert!(
                rendered.contains("Ctrl+P Pause"),
                "{focused_panel:?}: {rendered}"
            );
            assert!(
                !rendered.contains("Tab Focus"),
                "{focused_panel:?}: {rendered}"
            );
        }
    }

    #[test]
    fn cpu_panel_renders_average_frequency_and_core_cells() {
        let mut app = make_test_app(3, 10);
        app.snapshot.cpu_total_usage_percent = Some(42);
        app.snapshot.cpu_p_core_frequency_mhz = Some(3_200);
        app.snapshot.cpu_e_core_frequency_mhz = Some(1_800);
        app.snapshot.cpu_logical_processors = vec![
            CpuLogicalProcessorSample {
                usage_percent: 1,
                kind: Some(CpuCoreKind::Performance),
            },
            CpuLogicalProcessorSample {
                usage_percent: 22,
                kind: Some(CpuCoreKind::Performance),
            },
            CpuLogicalProcessorSample {
                usage_percent: 99,
                kind: Some(CpuCoreKind::Efficiency),
            },
        ];

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(rendered.contains("CPUS"), "{rendered}");
        assert!(rendered.contains("CPU Usage ["), "{rendered}");
        assert!(rendered.contains("]  42%"), "{rendered}");
        assert!(rendered.contains("P-core 3200 MHz"), "{rendered}");
        assert!(rendered.contains("E-core 1800 MHz"), "{rendered}");
        assert!(rendered.contains("Per-core Usage (P/E)"), "{rendered}");
        assert!(rendered.contains("P ▁▂  E █"), "{rendered}");
    }

    #[test]
    fn cpu_panel_space_assigns_cpu_average_graph() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Cpu;
        app.snapshot.cpu_total_usage_percent = Some(42);
        app.system_history.record_snapshot(&app.snapshot);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.active_graph_slot(),
            Some(&GraphSlot::system(SystemMetric::CpuAverage))
        );
        assert!(app.show_details);
        assert_eq!(
            app.active_graph_slot().map(GraphSlot::value_format),
            Some(GraphValueFormat::Percent)
        );
        assert_eq!(
            app.graph_slot_samples(app.active_graph_slot().unwrap())[0].value,
            Some(42.0)
        );

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("1  CPU Usage ["), "{rendered}");
        assert!(rendered.contains("]  42%"), "{rendered}");
        assert!(
            rendered.contains("Slot#1 · CPU Usage · SYSTEM"),
            "{rendered}"
        );

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (x, y) = find_text_position(&buffer, "1  CPU Usage [")
            .expect("CPU Graph slot number should render");
        let marker = &buffer[(x, y)];
        assert_eq!(marker.fg, ui::THEMES[0].active_series);
        assert_ne!(marker.bg, ui::THEMES[0].active_series);
        assert!(marker.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn cpu_inactive_two_digit_graph_number_matches_process_style() {
        let mut app = make_test_app(3, 10);
        let ids = (0..9)
            .map(|index| add_test_graph(&mut app, index))
            .collect::<Vec<_>>();
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::system(SystemMetric::CpuAverage),
            FocusedPanel::Cpu,
        ));
        assert!(app.set_active_graph(ids[0]));

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (x, y) = find_text_position(&buffer, "10 CPU Usage [")
            .expect("two-digit CPU Graph slot number should keep a label gap");
        let marker = &buffer[(x, y)];

        assert_eq!(marker.fg, ui::THEMES[0].graph_line);
        assert!(marker.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn clicking_cpu_panel_moves_focus_to_cpus() {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, 120, 45);
        let area = ui::cpu_panel_area_for_screen(screen, &app);

        app.on_mouse(left_click(area.x + 1, area.y + 1), screen);

        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        assert_eq!(app.status, "Focus: CPUs");
    }

    #[test]
    fn clicking_system_activity_panel_moves_focus_to_system_activity() {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, 120, 45);
        let area = ui::system_activity_panel_area_for_screen(screen, &app);

        app.on_mouse(left_click(area.x + 1, area.y + 1), screen);

        assert_eq!(app.focused_panel, FocusedPanel::SystemActivity);
        assert_eq!(app.status, "NW/DISK row: Net Rx");
    }

    #[test]
    fn help_dialog_takes_focus_border_from_previous_panel() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.show_help = true;

        let screen = Rect::new(0, 0, 140, 70);
        let popup = help_area(screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let process_table = main_panel_areas_for_app(screen, &app).processes.area;
        assert_eq!(buffer[(popup.x, popup.y)].fg, app.theme().focus_border);
        assert_eq!(
            buffer[(process_table.x, process_table.y)].fg,
            app.theme().border
        );
    }

    #[test]
    fn quit_key_opens_confirmation_before_exiting() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_quit_confirmation);
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Cancel);
        assert!(!app.should_quit);

        app.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_quit_confirmation);
        assert!(!app.should_quit);
    }

    #[test]
    fn quit_confirmation_dialog_uses_clear_copy_without_extra_key_help() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(rendered.contains("Quit winproc-tui?"), "{rendered}");
        assert!(
            !rendered.contains("Close winproc-tui and return to terminal."),
            "{rendered}"
        );
        assert!(rendered.contains("[ Quit ]"), "{rendered}");
        assert!(rendered.contains("[ Cancel ]"), "{rendered}");
        assert!(
            !rendered.contains("Confirm before closing the monitor"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Enter selects / Esc cancels / q quits"),
            "{rendered}"
        );
    }

    #[test]
    fn quit_confirmation_dialog_keeps_buttons_on_one_row_on_narrow_screens() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 40, 24);
        assert!(rendered.contains("Quit winproc-tui?"), "{rendered}");
        assert!(
            !rendered.contains("Close winproc-tui and return to terminal."),
            "{rendered}"
        );
        assert!(rendered.contains("[ Quit ]   [ Cancel ]"), "{rendered}");
        assert!(
            !rendered.contains("Enter selects / Esc cancels / q quits"),
            "{rendered}"
        );
    }

    #[test]
    fn quit_confirmation_dialog_mentions_recording_flush_when_active() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        let path = unique_recording_path("quit");
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(rendered.contains("Stop recording and quit?"), "{rendered}");
        assert!(
            rendered.contains("The log will be flushed before exit."),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Recording is active. The log will be flushed first."),
            "{rendered}"
        );

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quit_confirmation_enter_activates_default_cancel() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_quit_confirmation);
        assert!(!app.should_quit);
    }

    #[test]
    fn quit_confirmation_enter_confirms_when_quit_is_selected() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Quit);

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_quit_confirmation);
        assert!(app.should_quit);
    }

    #[test]
    fn quit_confirmation_switches_buttons_with_tab_and_arrows() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Cancel);

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Quit);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Cancel);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.quit_confirm_selection, QuitConfirmSelection::Quit);
    }

    #[test]
    fn quit_confirmation_q_confirms_and_esc_cancels() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_quit_confirmation);
        assert!(!app.should_quit);

        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_quit_confirmation);
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_r_requires_tracked_processes_before_opening_recording_dialog() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.show_recording_no_tracked_warning);
        assert!(!app.show_recording_path_dialog);
        assert_eq!(app.status, "No tracked processes to record");

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(rendered.contains("No tracked processes"), "{rendered}");
        assert!(
            rendered.contains("Track a process before starting recording."),
            "{rendered}"
        );
        assert!(rendered.contains("[ OK ]"), "{rendered}");
        assert!(
            !rendered.contains("Press Enter or Esc to close."),
            "{rendered}"
        );
    }

    #[test]
    fn recording_no_tracked_warning_closes_with_escape_or_enter() {
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let mut app = make_test_app(1, 10);
            app.show_recording_no_tracked_warning = true;

            app.on_key(key).unwrap();

            assert!(!app.show_recording_no_tracked_warning);
            assert_eq!(app.status, "Recording canceled");
        }
    }

    #[test]
    fn warning_ok_buttons_close_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.show_no_graph_metrics_warning = true;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ OK ]").expect("OK button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_no_graph_metrics_warning);
    }

    #[test]
    fn quit_confirmation_buttons_activate_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.request_quit_confirmation();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ Quit ]").expect("Quit button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_quit_confirmation);
        assert!(app.should_quit);
    }

    #[test]
    fn recording_no_tracked_ok_button_closes_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.show_recording_no_tracked_warning = true;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ OK ]").expect("OK button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_recording_no_tracked_warning);
        assert_eq!(app.status, "Recording canceled");
    }

    #[test]
    fn recording_overwrite_buttons_are_clickable_over_path_dialog() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.show_recording_overwrite_confirmation = true;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        assert!(
            find_text_position(&buffer, "Enter Select  Esc Cancel  y Overwrite").is_some(),
            "overwrite shortcuts should follow footer formatting"
        );
        let (x, y) =
            find_text_position(&buffer, "[ Cancel ]").expect("Cancel button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(app.show_recording_path_dialog);
        assert!(!app.show_recording_overwrite_confirmation);
        assert_eq!(app.status, "Overwrite canceled");
    }

    #[test]
    fn recording_path_cancel_button_closes_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.recording_path_draft = "C:/logs/example.log".to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) =
            find_text_position(&buffer, "[ Cancel ]").expect("Cancel button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_recording_path_dialog);
        assert_eq!(app.status, "Recording canceled");
    }

    #[test]
    fn recording_path_start_button_starts_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let path = unique_recording_path("mouse-start");
        let _ = std::fs::remove_file(&path);
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.show_recording_path_dialog = true;
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ Start ]").expect("Start button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_recording_path_dialog);
        assert!(app.recording_session.is_some());
        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn recording_path_dialog_tab_moves_focus_to_start_button() {
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.recording_path_draft = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("definitely-no-such-prefix")
            .join("example.log")
            .display()
            .to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Path
        );

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Start
        );
        assert!(app.status.is_empty());

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Cancel
        );
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Start
        );

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(rendered.contains("[ Start ]   [ Cancel ]"), "{rendered}");
    }

    #[test]
    fn recording_path_dialog_keeps_arrows_for_path_cursor() {
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.recording_path_draft = "C:/logs/example.log".to_string();
        app.recording_path_cursor = app.recording_path_draft.len();

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Path
        );
        assert!(app.recording_path_cursor < app.recording_path_draft.len());
    }

    #[test]
    fn recording_path_backspace_handles_key_repeat_and_ignores_release() {
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.recording_path_draft = "C:/logs/example.log".to_string();
        app.recording_path_cursor = app.recording_path_draft.len();

        app.on_key(KeyEvent::new_with_kind(
            KeyCode::Backspace,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ))
        .unwrap();
        app.on_key(KeyEvent::new_with_kind(
            KeyCode::Backspace,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ))
        .unwrap();

        assert_eq!(app.recording_path_draft, "C:/logs/example.lo");
        assert_eq!(app.recording_path_cursor, app.recording_path_draft.len());
    }

    #[test]
    fn ctrl_space_completes_recording_path_directory() {
        let root = unique_recording_dir("recording-path-complete");
        let target = root.join("alpha");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        let head = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
        app.recording_path_draft = format!("{head}{}capture.log", std::path::MAIN_SEPARATOR);
        app.recording_path_cursor = head.len();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();

        let expected = format!(
            "{}{}alpha{}capture.log",
            root.display(),
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        assert_eq!(app.recording_path_draft, expected);
        assert_eq!(
            app.recording_path_cursor,
            format!("{}{}alpha", root.display(), std::path::MAIN_SEPARATOR).len()
        );
        assert_eq!(app.status, "Completed directory");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tracked_remove_cancel_button_closes_with_mouse() {
        let screen = Rect::new(0, 0, 100, 45);
        let mut app = make_test_app(1, 10);
        app.show_tracked_remove_confirmation = true;
        app.tracked_remove_name = "proc-0".to_string();
        app.tracked_remove_total_samples = 7_200;
        app.tracked_remove_discarded_samples = 7_080;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) =
            find_text_position(&buffer, "[ Cancel ]").expect("Cancel button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_tracked_remove_confirmation);
        assert_eq!(app.status, "Tracked removal canceled");
    }

    #[test]
    fn tracked_remove_confirmation_is_compact_left_aligned_and_uses_footer_shortcuts() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.show_tracked_remove_confirmation = true;
        app.tracked_remove_name = "target.exe".to_string();
        app.tracked_remove_total_samples = 143;
        app.tracked_remove_discarded_samples = 23;

        let popup = tracked_remove_dialog_area(screen);
        assert_eq!(popup.width, 74);
        assert_eq!(popup.height, 10);

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let message = "target.exe has 143 in-memory samples.";
        let (message_x, _) =
            find_text_position(&buffer, message).expect("confirmation message should render");
        assert_eq!(
            message_x,
            popup.x + 1,
            "message body should be left aligned"
        );

        let shortcut = "Enter Select  Esc Cancel  y Remove";
        assert!(find_text_position(&buffer, shortcut).is_some());
        assert!(find_text_position(&buffer, "Enter selects / Esc cancels / y removes").is_none());

        let (enter_x, enter_y) =
            find_text_position(&buffer, shortcut).expect("shortcut line should render");
        assert_eq!(buffer[(enter_x, enter_y)].fg, app.theme().muted);
        assert_eq!(buffer[(enter_x + 6, enter_y)].fg, app.theme().text);
    }

    #[test]
    fn closing_modal_restores_visible_panel_focus() {
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::DetailsGraph;
        app.show_details = false;
        app.show_help = true;

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_help);
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        assert!(app.panel_has_focus(FocusedPanel::Processes));

        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::DetailsSamples;
        app.show_details = false;
        app.show_recording_no_tracked_warning = true;

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_recording_no_tracked_warning);
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        assert!(app.panel_has_focus(FocusedPanel::Processes));
    }

    #[test]
    fn recording_no_tracked_warning_takes_focus_border_from_previous_panel() {
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.show_recording_no_tracked_warning = true;

        assert_modal_focus_border(&app, 52, 16);
    }

    #[test]
    fn ctrl_r_opens_recording_path_dialog_with_last_dir_default() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        let last_dir = std::path::PathBuf::from("C:/logs");
        app.recording_last_dir = Some(last_dir.clone());

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.show_recording_path_dialog);
        assert!(
            app.recording_path_draft.starts_with("C:/logs")
                || app.recording_path_draft.starts_with("C:\\logs")
        );
        assert!(app.recording_path_draft.contains("winproc-tui-"));
        assert!(app.recording_path_draft.ends_with(".log"));
        assert_eq!(app.recording_path_cursor, app.recording_path_draft.len());
    }

    #[test]
    fn recording_path_dialog_takes_focus_border_from_previous_panel() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.focused_panel = FocusedPanel::Processes;

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();

        assert_modal_rect_focus_border(&app, Rect::new(11, 18, 78, 8));
    }

    #[test]
    fn recording_path_dialog_uses_terminal_cursor_without_inline_marker() {
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        app.recording_path_draft = "C:/logs/example.log".to_string();
        app.recording_path_cursor = "C:/logs/".len();
        let screen = Rect::new(0, 0, 100, 45);
        let popup = Rect::new(11, 18, 78, 8);
        let expected_cursor =
            Position::new(popup.x + 1 + app.recording_path_cursor as u16, popup.y + 2);

        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("test render should succeed");
        terminal
            .backend_mut()
            .assert_cursor_position(expected_cursor);
        let rendered = buffer_to_text(terminal.backend().buffer());

        assert!(rendered.contains("C:/logs/example.log"), "{rendered}");
        assert!(rendered.contains("Path"), "{rendered}");
        assert!(
            rendered.contains("Enter a log file path. Missing parent directories will be created."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Enter activate · Esc close · Tab focus · Ctrl+Space complete"),
            "{rendered}"
        );
        assert!(rendered.contains("[ Start ]   [ Cancel ]"), "{rendered}");
        assert!(!rendered.contains("Log file path"), "{rendered}");
        assert!(
            !rendered.contains("Specify the log file path."),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Enter starts recording / Esc cancels"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Press Enter to start recording. Press Esc to cancel."),
            "{rendered}"
        );
        assert!(!rendered.contains("C:/logs/|example.log"), "{rendered}");
    }

    #[test]
    fn recording_dialog_actions_use_semantic_roles_in_both_themes() {
        for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme_index;
            app.show_recording_path_dialog = true;
            app.recording_path_draft = "C:/logs/example.log".to_string();
            app.recording_path_cursor = app.recording_path_draft.len();

            let hint_buffer = render_app_to_buffer(&app, 100, 45);
            let (enter_x, hint_y) = find_text_position(&hint_buffer, "Enter activate")
                .expect("recording shortcut should render");
            let (activate_x, _) =
                find_text_position(&hint_buffer, "activate").expect("shortcut label should render");
            assert_eq!(hint_buffer[(enter_x, hint_y)].fg, theme.muted);
            assert_eq!(hint_buffer[(activate_x, hint_y)].fg, theme.text);

            app.recording_path_selection = app::RecordingPathSelection::Start;

            let path_buffer = render_app_to_buffer(&app, 100, 45);
            let (start_x, start_y) =
                find_text_position(&path_buffer, "[ Start ]").expect("Start button should render");
            let (cancel_x, _) = find_text_position(&path_buffer, "[ Cancel ]")
                .expect("Cancel button should render");
            let button_midpoint = (start_x as i32 + cancel_x as i32 + 10) / 2;
            assert!((button_midpoint - 50).abs() <= 1);
            assert_eq!(path_buffer[(start_x, start_y)].bg, theme.accent);
            assert_ne!(path_buffer[(start_x, start_y)].bg, theme.warning);

            app.show_recording_overwrite_confirmation = true;
            app.recording_overwrite_selection = app::RecordingOverwriteSelection::Overwrite;
            let overwrite_buffer = render_app_to_buffer(&app, 100, 45);
            let (overwrite_x, overwrite_y) = find_text_position(&overwrite_buffer, "[ Overwrite ]")
                .expect("Overwrite button should render");
            assert_eq!(
                overwrite_buffer[(overwrite_x, overwrite_y)].bg,
                theme.warning
            );
            assert_ne!(
                overwrite_buffer[(overwrite_x, overwrite_y)].bg,
                theme.accent
            );
        }
    }

    #[test]
    fn recording_creates_missing_parent_directories() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        let root = unique_recording_dir("mkdir");
        let path = root.join("nested").join("capture.log");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;

        app.confirm_recording_path().unwrap();

        assert!(path.parent().unwrap().is_dir());
        assert!(path.is_file());
        assert!(!app.show_recording_path_dialog);
        assert!(app.recording_session.is_some());

        app.stop_recording().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_directory_path_is_rejected() {
        let directory = unique_recording_dir("directory-path");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.show_recording_path_dialog = true;
        app.recording_path_draft = directory.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.recording_path_selection = app::RecordingPathSelection::Start;

        app.confirm_recording_path().unwrap();

        assert!(app.show_recording_path_dialog);
        assert!(!app.show_recording_overwrite_confirmation);
        assert!(app.recording_session.is_none());
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Path
        );
        assert_eq!(app.status, "Recording path must be a file, not a directory");
        assert!(directory.is_dir());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recording_overwrite_rechecks_directory_path() {
        let directory = unique_recording_dir("overwrite-directory-path");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.show_recording_path_dialog = true;
        app.show_recording_overwrite_confirmation = true;
        app.recording_overwrite_selection = app::RecordingOverwriteSelection::Overwrite;
        app.recording_path_draft = directory.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();

        app.confirm_recording_overwrite().unwrap();

        assert!(app.show_recording_path_dialog);
        assert!(!app.show_recording_overwrite_confirmation);
        assert!(app.recording_session.is_none());
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Path
        );
        assert_eq!(app.status, "Recording path must be a file, not a directory");
        assert!(directory.is_dir());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn existing_recording_path_opens_overwrite_confirmation() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        let path = unique_recording_path("existing");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old").unwrap();
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_recording_path_dialog);
        assert!(app.show_recording_overwrite_confirmation);
        assert_eq!(
            app.recording_overwrite_selection,
            app::RecordingOverwriteSelection::Cancel
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn overwrite_cancel_returns_to_recording_path_dialog() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.show_recording_path_dialog = true;
        app.show_recording_overwrite_confirmation = true;

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_recording_path_dialog);
        assert!(!app.show_recording_overwrite_confirmation);
        assert_eq!(app.status, "Overwrite canceled");
    }

    #[test]
    fn live_header_omits_freshness_when_current() {
        let app = make_test_app(1, 10);

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(rendered.contains("LIVE"), "{rendered}");
        assert!(!rendered.contains("fresh"), "{rendered}");
        assert!(!rendered.contains("STALE"), "{rendered}");
    }

    #[test]
    fn live_header_hides_product_and_version_when_the_row_is_too_narrow() {
        let app = make_test_app(1, 10);
        let product_and_version = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));

        let rendered = render_app_to_text(&app, 24, 20);

        assert!(rendered.contains("LIVE"), "{rendered}");
        assert!(!rendered.contains(&product_and_version), "{rendered}");
    }

    #[test]
    fn live_header_shows_visible_stale_state() {
        let mut app = make_test_app(1, 10);
        app.snapshot.captured_at =
            Local::now() - chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64 + 2);

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(rendered.contains("LIVE"), "{rendered}");
        assert!(rendered.contains("STALE "), "{rendered}");
        assert!(!rendered.contains("fresh"), "{rendered}");
    }

    #[test]
    fn recording_header_shows_rec_spinner_and_path() {
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        let path = unique_recording_path("header");
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("REC"), "{rendered}");
        assert!(!rendered.contains("fresh"), "{rendered}");
        assert!(!rendered.contains("STALE"), "{rendered}");
        assert!(rendered.contains("winproc-tui-test-header"), "{rendered}");

        app.toggle_display_pause();
        let paused = render_app_to_text(&app, 120, 45);
        assert!(paused.contains("REC"), "{paused}");
        assert!(paused.contains("DISPLAY PAUSED"), "{paused}");
        assert!(paused.contains("winproc-tui-test-header"), "{paused}");

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn column_picker_toggles_visible_columns() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_column_picker);

        app.column_picker_index = 0;
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.process_columns.contains(&MetricColumn::CpuPercent));
        assert_eq!(app.column_preset, ColumnPreset::Custom);

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_column_picker);
    }

    #[test]
    fn column_picker_mouse_click_toggles_clicked_column() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![MetricColumn::PrivateBytes];
        app.show_column_picker = true;

        let buffer = render_app_to_buffer(&app, 100, 45);
        let (x, y) = find_text_position(&buffer, "CPU%")
            .expect("CPU column row should be rendered in the picker");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 100, 45),
        );

        assert_eq!(app.column_picker_index, 0);
        assert!(app.process_columns.contains(&MetricColumn::CpuPercent));
        assert_eq!(app.column_preset, ColumnPreset::Custom);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 100, 45),
        );

        assert!(!app.process_columns.contains(&MetricColumn::CpuPercent));
        assert_eq!(app.process_columns, vec![MetricColumn::PrivateBytes]);
    }

    #[test]
    fn column_picker_scrollbar_drag_scrolls_content() {
        let mut app = make_test_app(1, 10);
        app.show_column_picker = true;
        let screen = Rect::new(0, 0, 100, 10);
        app.set_column_picker_page_size(ui::column_picker_page_size_for_screen(screen));
        let scrollbar = column_picker_scrollbar_area(screen, app.column_picker_scroll.page_size)
            .expect("small column picker should have a scrollbar");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.column_picker_scroll.dragging);
        assert_eq!(app.column_picker_scroll.offset, 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.bottom().saturating_sub(1),
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(app.column_picker_scroll.offset > 0);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: scrollbar.x,
                row: scrollbar.bottom().saturating_sub(1),
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert!(!app.column_picker_scroll.dragging);
    }

    #[test]
    fn column_picker_panel_fits_rendered_content_height() {
        let popup = column_picker_area(Rect::new(0, 0, 100, 45));

        assert_eq!(popup.height, MetricColumn::ALL.len() as u16 + 8);
    }

    #[test]
    fn column_picker_close_button_click_closes_dialog() {
        let mut app = make_test_app(1, 10);
        app.show_column_picker = true;
        let screen = Rect::new(0, 0, 100, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ Close ]")
            .expect("column picker close button should be rendered");

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert!(!app.show_column_picker);
    }

    #[test]
    fn column_picker_header_uses_footer_like_shortcut_styles() {
        let mut app = make_test_app(1, 10);
        app.show_column_picker = true;

        let buffer = render_app_to_buffer(&app, 100, 45);
        let rendered = buffer_to_text(&buffer);
        let theme = ui::THEMES[0];

        assert!(!rendered.contains("Descriptions are concise"), "{rendered}");
        assert!(
            rendered.contains("Up/Down select  Space toggle  Enter/Esc close"),
            "{rendered}"
        );
        assert!(rendered.contains("[ Close ]"), "{rendered}");

        let (title_x, title_y) = find_text_position(&buffer, "Select process columns")
            .expect("column picker title should be rendered");
        assert_eq!(title_x, column_picker_area(Rect::new(0, 0, 100, 45)).x + 2);
        let title_cell = &buffer[(title_x, title_y)];
        assert_eq!(title_cell.fg, theme.text);
        assert_ne!(title_cell.fg, theme.accent);
        assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));

        let (key_x, key_y) =
            find_text_position(&buffer, "Up/Down").expect("shortcut key should be rendered");
        let key_cell = &buffer[(key_x, key_y)];
        assert_eq!(key_cell.fg, theme.accent);
        assert!(key_cell.modifier.contains(ratatui::style::Modifier::BOLD));

        let label_cell = &buffer[(key_x + "Up/Down ".len() as u16, key_y)];
        assert_eq!(label_cell.fg, theme.text);
    }

    #[test]
    fn column_picker_takes_focus_border_from_previous_panel() {
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::Processes;

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            .unwrap();

        assert_modal_rect_focus_border(&app, column_picker_area(Rect::new(0, 0, 100, 45)));
    }

    #[test]
    fn number_keys_do_not_switch_column_presets() {
        let mut app = make_test_app(1, 10);
        let columns_before = app.process_columns.clone();

        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.column_preset, ColumnPreset::Default);
        assert_eq!(app.process_columns, columns_before);
    }

    #[test]
    fn ctrl_c_copies_selected_process_row_text() {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![
            MetricColumn::PrivateBytes,
            MetricColumn::WorksetPrivateBytes,
        ];
        app.snapshot.processes[0].private_bytes = Some(388_067_328);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.show_column_picker);
        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("0\tproc-0\t388,067,328\t--")
        );
        assert_eq!(app.status, "Copied row: proc-0");
    }

    #[test]
    fn ctrl_c_copies_selected_ram_vram_row_text() {
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::System;
        app.ram_vram_selected_index = 1;
        app.snapshot.committed_memory = Some(9_000_000_000);
        app.snapshot.commit_limit = Some(18_000_000_000);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("Committed\t9,000 MB / 18,000 MB ( 50%)")
        );
        assert_eq!(app.status, "Copied row: Committed");
    }

    #[test]
    fn ctrl_c_copies_cpu_average_row_text() {
        let mut app = make_test_app(1, 10);
        app.focused_panel = FocusedPanel::Cpu;
        app.snapshot.cpu_total_usage_percent = Some(37);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("CPU Usage\t37%")
        );
        assert_eq!(app.status, "Copied row: CPU Usage");
    }

    #[test]
    fn ctrl_c_copies_selected_sample_row_text_when_samples_are_focused() {
        let mut app = make_test_app(1, 10);
        let first = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
        let second = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 1).unwrap();
        let tracked = app.normalized_watch_names.clone();
        app.snapshot.captured_at = first;
        app.snapshot.processes[0].private_bytes = Some(100);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &tracked,
        );
        app.snapshot.captured_at = second;
        app.snapshot.processes[0].private_bytes = Some(1_234);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &tracked,
        );
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsSamples;
        app.details_sample_selected = 1;

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("10:00:01\t1,234\t+1,134")
        );
        assert_eq!(app.status, "Copied row: 10:00:01 Private=1,234");
    }

    #[test]
    fn plain_c_opens_column_picker() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_column_picker);
    }

    #[test]
    fn plain_i_opens_system_info_dialog() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_system_info_dialog);
        assert_eq!(app.process_info_cache.len(), 0);
        assert!(app.pending_process_info.is_none());

        let rendered = render_app_to_text(&app, 100, 20);
        assert_eq!(rendered.matches("SYSTEM INFO").count(), 1, "{rendered}");
        assert!(!rendered.contains("System Activity"), "{rendered}");
        assert!(!rendered.contains("CPUS"), "{rendered}");
        assert!(!rendered.contains("CPU Usage ["), "{rendered}");
        assert!(!rendered.contains("Net Rx"), "{rendered}");
        assert!(!rendered.contains("Disk Q"), "{rendered}");

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_system_info_dialog);
        assert!(app.pending_process_info.is_none());
    }

    #[test]
    fn ctrl_i_opens_process_jump_instead_of_info_panel() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.show_system_info_dialog);
        assert!(app.jump_editing);
    }

    #[test]
    fn process_info_request_keeps_the_process_selected_when_dialog_opened() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, request_rx, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            3,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_selected_process_info_dialog().unwrap();

        app.move_selection_down(1);
        app.move_selection_down(1);

        assert_eq!(app.selected_visible_process().unwrap().name, "proc-2");
        assert!(app.pending_process_info.is_some());
        assert!(!app.request_due_process_info().unwrap());
        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));

        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        assert!(!app.request_due_process_info().unwrap());

        match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect { identity, .. } => {
                assert_eq!(identity.name, "proc-0");
            }
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        }
        assert!(app.pending_process_info.is_none());
        assert_eq!(app.process_info_in_flight.as_ref().unwrap().name, "proc-0");
    }

    #[test]
    fn process_info_result_updates_cache_for_current_selection() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_selected_process_info_dialog().unwrap();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let (generation, identity) = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect {
                generation,
                identity,
                ..
            } => (generation, identity),
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(ProcessInfoResult {
                generation,
                identity: identity.clone(),
                info: test_process_info(&identity.name, identity.pid),
            })
            .unwrap();

        assert!(app.poll_process_info_results().unwrap());
        assert!(app.process_info_cache.contains_key(&identity));
        assert_eq!(app.process_info_display_identity, Some(identity));
        assert!(app.process_info_in_flight.is_none());
    }

    #[test]
    fn process_info_result_applies_to_fixed_dialog_target_after_selection_changes() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_selected_process_info_dialog().unwrap();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let (generation, old_identity) = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect {
                generation,
                identity,
                ..
            } => (generation, identity),
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        app.move_selection_down(1);
        result_tx
            .send(ProcessInfoResult {
                generation,
                identity: old_identity.clone(),
                info: test_process_info(&old_identity.name, old_identity.pid),
            })
            .unwrap();

        assert!(app.poll_process_info_results().unwrap());
        assert!(app.process_info_cache.contains_key(&old_identity));
        assert!(app.process_info_in_flight.is_none());
        assert!(app.pending_process_info.is_none());
    }

    #[test]
    fn stale_process_info_result_cannot_replace_reopened_dialog_request() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );

        app.open_selected_process_info_dialog().unwrap();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let (old_generation, identity) = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect {
                generation,
                identity,
                ..
            } => (generation, identity),
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        app.close_process_info_dialog();
        app.open_selected_process_info_dialog().unwrap();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let new_generation = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect { generation, .. } => generation,
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(ProcessInfoResult {
                generation: old_generation,
                identity: identity.clone(),
                info: test_process_info("old.exe", identity.pid),
            })
            .unwrap();
        assert!(!app.poll_process_info_results().unwrap());
        assert_eq!(app.process_info_in_flight_generation, Some(new_generation));
        assert!(!app.process_info_cache.contains_key(&identity));

        result_tx
            .send(ProcessInfoResult {
                generation: new_generation,
                identity: identity.clone(),
                info: test_process_info("new.exe", identity.pid),
            })
            .unwrap();
        assert!(app.poll_process_info_results().unwrap());
        assert_eq!(
            app.process_info_cache.get(&identity).unwrap().name,
            "new.exe"
        );
    }

    #[test]
    fn process_info_metrics_follow_current_a_and_b_rules_with_missing_values() {
        let mut app = make_test_app(1, 10);
        let current_at = app.snapshot.captured_at;
        let a_at = current_at - chrono::Duration::seconds(2);
        let b_at = current_at - chrono::Duration::seconds(1);
        let mut a = app.snapshot.processes[0].clone();
        a.cpu_percent = Some(10.8);
        a.private_bytes = Some(375_800_000);
        a.thread_count = Some(1_036);
        a.handle_count = Some(20);
        a.gdi_object_count = Some(5);
        a.io_read_bytes_per_sec = Some(50_000);
        let mut b = a.clone();
        b.cpu_percent = Some(11.7);
        b.private_bytes = Some(384_400_000);
        b.thread_count = Some(1_030);
        b.handle_count = Some(20);
        b.io_read_bytes_per_sec = Some(75_000);
        let mut current = a.clone();
        current.cpu_percent = Some(12.3);
        current.private_bytes = Some(388_100_000);
        current.thread_count = Some(1_024);
        current.handle_count = Some(20);
        current.gdi_object_count = None;
        current.io_read_bytes_per_sec = Some(100_000);
        app.snapshot.processes[0] = current.clone();
        app.process_history
            .record_snapshot_unbounded(a_at, &[a.clone()]);
        app.process_history
            .record_snapshot_unbounded(b_at, &[b.clone()]);
        app.process_history
            .record_snapshot_unbounded(current_at, &[current]);
        app.open_selected_process_info_dialog().unwrap();

        let current_view = app.process_info_metrics_view().unwrap();
        assert_eq!(current_view.value_heading, "Current");
        assert_eq!(current_view.delta_heading, None);
        assert!(current_view.rows.iter().all(|row| row.delta.is_none()));
        assert_eq!(
            current_view
                .rows
                .iter()
                .map(|row| row.label)
                .collect::<Vec<_>>(),
            vec![
                "CPU Usage",
                "Private Bytes",
                "Working Set",
                "Working Set - Private",
                "Threads",
                "Handles",
                "USER Objects",
                "GDI Objects",
                "GPU Usage",
                ".NET Heap",
                "GPU Dedicated Memory",
                "GPU Shared Memory",
                "I/O Read Throughput",
                "I/O Write Throughput",
            ]
        );
        assert_eq!(
            current_view
                .rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .value,
            "388.1 MB"
        );
        assert_eq!(
            current_view
                .rows
                .iter()
                .find(|row| row.label == "I/O Read Throughput")
                .unwrap()
                .value,
            "800 Kbps"
        );
        let compact = render_app_to_text(&app, 60, 40);
        for label in [
            "Private Bytes",
            "Working Set - Private",
            "Handles",
            "GPU Dedicated Memory",
            "I/O Write Throughput",
        ] {
            assert!(compact.contains(label), "missing {label}: {compact}");
        }

        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint { captured_at: a_at }),
            b: None,
        });
        let a_view = app.process_info_metrics_view().unwrap();
        assert_eq!(a_view.delta_heading, Some("Delta from A"));
        assert_eq!(
            a_view
                .rows
                .iter()
                .find(|row| row.label == "CPU Usage")
                .unwrap()
                .delta
                .as_deref(),
            Some("+1.5%")
        );
        assert_eq!(
            a_view
                .rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .delta
                .as_deref(),
            Some("+12.3 MB")
        );
        assert_eq!(
            a_view
                .rows
                .iter()
                .find(|row| row.label == "Threads")
                .unwrap()
                .delta
                .as_deref(),
            Some("-12")
        );
        assert_eq!(
            a_view
                .rows
                .iter()
                .find(|row| row.label == "Handles")
                .unwrap()
                .delta
                .as_deref(),
            Some("+0")
        );
        assert_eq!(
            a_view
                .rows
                .iter()
                .find(|row| row.label == "I/O Read Throughput")
                .unwrap()
                .delta
                .as_deref(),
            Some("+400 Kbps")
        );
        let missing = a_view
            .rows
            .iter()
            .find(|row| row.label == "GDI Objects")
            .unwrap();
        assert_eq!(missing.value, "--");
        assert_eq!(missing.delta.as_deref(), Some("--"));

        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint { captured_at: a_at }),
            b: Some(app::AbComparisonPoint { captured_at: b_at }),
        });
        let ab_view = app.process_info_metrics_view().unwrap();
        assert_eq!(ab_view.value_heading, "At B");
        assert_eq!(ab_view.delta_heading, Some("B-A"));
        assert_eq!(
            ab_view
                .rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .value,
            "384.4 MB"
        );
        let io_read = ab_view
            .rows
            .iter()
            .find(|row| row.label == "I/O Read Throughput")
            .unwrap();
        assert_eq!(io_read.value, "600 Kbps");
        assert_eq!(io_read.delta.as_deref(), Some("+200 Kbps"));

        app.ab_comparison = Some(app::AbComparison {
            a: None,
            b: Some(app::AbComparisonPoint { captured_at: b_at }),
        });
        let b_only_view = app.process_info_metrics_view().unwrap();
        assert_eq!(b_only_view.value_heading, "Current");
        assert_eq!(b_only_view.delta_heading, None);
    }

    #[test]
    fn process_info_renders_range_above_underlined_metric_headers() {
        let mut app = make_test_app(1, 10);
        let current_at = app.snapshot.captured_at;
        let a_at = current_at - chrono::Duration::seconds(2);
        let b_at = current_at - chrono::Duration::seconds(1);
        let process = app.snapshot.processes[0].clone();
        app.process_history
            .record_snapshot_unbounded(a_at, std::slice::from_ref(&process));
        app.process_history
            .record_snapshot_unbounded(b_at, std::slice::from_ref(&process));
        app.process_history
            .record_snapshot_unbounded(current_at, std::slice::from_ref(&process));
        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint { captured_at: a_at }),
            b: Some(app::AbComparisonPoint { captured_at: b_at }),
        });
        app.open_selected_process_info_dialog().unwrap();
        let range = app.process_info_metrics_view().unwrap().range;

        let buffer = render_app_to_buffer(&app, 120, 40);
        let (_, range_y) =
            find_text_position(&buffer, &range).expect("comparison range should render");
        let (_, header_y) =
            find_text_position(&buffer, "At B").expect("metric header should render");
        let metrics_x = ui::process_info_content_area_for_screen(Rect::new(0, 0, 120, 40)).x;

        assert!(range_y < header_y);
        for heading in ["Metrics", "At B", "B-A"] {
            let (x, y) = if heading == "Metrics" {
                (metrics_x, header_y)
            } else {
                find_text_position(&buffer, heading).expect("column heading should render")
            };
            for offset in 0..heading.len() as u16 {
                assert!(
                    buffer[(x + offset, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::UNDERLINED),
                    "{heading} should be underlined"
                );
            }
        }
        assert!(
            !buffer[(metrics_x + "Metrics".len() as u16, header_y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED),
            "spacing after a column heading should not be underlined"
        );
    }

    #[test]
    fn process_info_current_delta_updates_while_dialog_remains_open() {
        let mut app = make_test_app(1, 10);
        let a_at = app.snapshot.captured_at;
        let mut a = app.snapshot.processes[0].clone();
        a.private_bytes = Some(100_000_000);
        app.snapshot.processes[0] = a.clone();
        app.process_history
            .record_snapshot_unbounded(a_at, &[a.clone()]);
        app.open_selected_process_info_dialog().unwrap();
        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint { captured_at: a_at }),
            b: None,
        });

        let later = a_at + chrono::Duration::seconds(1);
        let mut current = a;
        current.private_bytes = Some(125_000_000);
        app.snapshot.captured_at = later;
        app.snapshot.processes[0] = current.clone();
        app.process_history
            .record_snapshot_unbounded(later, &[current]);

        let view = app.process_info_metrics_view().unwrap();
        assert!(view.range.contains("Current"));
        assert_eq!(
            view.rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .delta
                .as_deref(),
            Some("+25.0 MB")
        );
    }

    #[test]
    fn process_info_uses_paused_history_and_log_view_starts_no_live_worker() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, request_rx, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        let paused_at = app.snapshot.captured_at - chrono::Duration::seconds(5);
        let mut paused_snapshot = app.snapshot.clone();
        paused_snapshot.captured_at = paused_at;
        paused_snapshot.processes[0].private_bytes = Some(42_000_000);
        paused_snapshot.processes[0].executable_path = Some(r"C:\recorded\proc-0.exe".to_string());
        let mut paused_history = ProcessHistory::default();
        paused_history.record_snapshot_unbounded(paused_at, &paused_snapshot.processes);
        app.paused_display = Some(app::state::PausedDisplay {
            snapshot: paused_snapshot,
            exited_tracked_rows: std::collections::HashMap::new(),
            process_history: paused_history,
            system_history: SystemHistory::default(),
            process_info_cache: std::collections::HashMap::new(),
            process_info_display_identity: None,
        });

        app.open_selected_process_info_dialog().unwrap();
        assert_eq!(
            app.process_info_metrics_view()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .value,
            "42.0 MB"
        );
        app.close_process_info_dialog();
        app.log_view_display = app.paused_display.take();
        app.log_view_path = Some(std::path::PathBuf::from("recording.jsonl"));
        app.open_selected_process_info_dialog().unwrap();

        assert!(app.pending_process_info.is_none());
        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(
            app.process_info_target_process()
                .unwrap()
                .executable_path
                .as_deref(),
            Some(r"C:\recorded\proc-0.exe")
        );
        assert_eq!(
            app.process_info_metrics_view()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.label == "Private Bytes")
                .unwrap()
                .value,
            "42.0 MB"
        );
    }

    #[test]
    fn f_requests_open_files_for_selected_process() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.process_info_tab = app::ProcessInfoTab::Environment;

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_process_info_dialog);
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Files);
        assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
        match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect {
                identity, process, ..
            } => {
                assert_eq!(identity.name, "proc-0");
                assert_eq!(process.name, "proc-0");
            }
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        }
    }

    #[test]
    fn process_info_resets_tab_filters_when_opened_again() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_files_filter = ".mxf .mp4".to_string();
        app.open_files_filter_cursor = app.open_files_filter.len();
        app.process_modules_filter = "microsoft".to_string();
        app.process_modules_filter_cursor = app.process_modules_filter.len();
        app.process_environment_filter = "path".to_string();
        app.process_environment_filter_cursor = app.process_environment_filter.len();

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.open_files_filter.is_empty());
        assert_eq!(app.open_files_filter_cursor, 0);
        assert!(app.process_modules_filter.is_empty());
        assert_eq!(app.process_modules_filter_cursor, 0);
        assert!(app.process_environment_filter.is_empty());
        assert_eq!(app.process_environment_filter_cursor, 0);
    }

    #[test]
    fn f_does_not_open_files_outside_processes_focus() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_process_info_dialog);
        assert!(request_rx.try_recv().is_err());
    }

    #[test]
    fn ctrl_u_refreshes_open_files_for_selected_process() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            2,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Files;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 1,
            file_handles: 1,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![OpenFileEntry {
                path: r"C:\tmp\a.log".to_string(),
                handle_count: 1,
            }],
            error: None,
        });
        app.open_files_result_identity = Some(identity);

        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.open_files_result.is_some());
        assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
        match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect {
                identity, process, ..
            } => {
                assert_eq!(identity.name, "proc-0");
                assert_eq!(process.name, "proc-0");
            }
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        }
    }

    #[test]
    fn open_files_result_updates_modal_state() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, result_tx) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.open_selected_process_files().unwrap();
        let (generation, identity) = match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect {
                generation,
                identity,
                ..
            } => (generation, identity),
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(OpenFilesResult {
                generation,
                identity: identity.clone(),
                report: OpenFilesReport {
                    pid: 0,
                    process_name: "proc-0".to_string(),
                    total_handles: 3,
                    file_handles: 2,
                    inaccessible_handles: 1,
                    unnamed_file_handles: 0,
                    entries: vec![OpenFileEntry {
                        path: r"C:\tmp\a.log".to_string(),
                        handle_count: 2,
                    }],
                    error: None,
                },
            })
            .unwrap();

        assert!(app.poll_open_files_results().unwrap());
        assert!(app.open_files_in_flight.is_none());
        assert_eq!(app.open_files_result.as_ref().unwrap().entries.len(), 1);
        assert!(app.status.contains("Loaded 1 open file paths"));
    }

    #[test]
    fn open_files_clipboard_is_raw_paths_without_header() {
        let mut app = make_test_app(1, 10);
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 2,
            file_handles: 2,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![
                OpenFileEntry {
                    path: r"C:\tmp\a.log".to_string(),
                    handle_count: 1,
                },
                OpenFileEntry {
                    path: r"C:\tmp\b.log".to_string(),
                    handle_count: 2,
                },
            ],
            error: None,
        });

        app.copy_open_files_to_clipboard().unwrap();

        assert_eq!(
            crate::app::clipboard::last_copied_text().unwrap(),
            "C:\\tmp\\a.log\nC:\\tmp\\b.log\t2"
        );
    }

    #[test]
    fn open_files_clipboard_filter_matches_full_paths() {
        let mut app = make_test_app(1, 10);
        app.open_files_filter = "exports".to_string();
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 3,
            file_handles: 3,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![
                OpenFileEntry {
                    path: r"C:\tmp\a.wav".to_string(),
                    handle_count: 1,
                },
                OpenFileEntry {
                    path: r"C:\exports\b.MXF".to_string(),
                    handle_count: 2,
                },
                OpenFileEntry {
                    path: r"C:\media\c.mp4".to_string(),
                    handle_count: 1,
                },
            ],
            error: None,
        });

        app.copy_open_files_to_clipboard().unwrap();

        assert_eq!(
            crate::app::clipboard::last_copied_text().unwrap(),
            "C:\\exports\\b.MXF\t2"
        );
    }

    #[test]
    fn open_files_filter_cursor_moves_and_inserts_at_cursor() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_filter = ".mp4".to_string();
        app.open_files_filter_cursor = app.open_files_filter.len();

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.open_files_filter, ".mpx4");
        assert_eq!(app.open_files_filter_cursor, ".mpx".len());

        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.open_files_filter, ".mp4");
        assert_eq!(app.open_files_filter_cursor, app.open_files_filter.len());
    }

    #[test]
    fn open_files_filter_delete_removes_character_at_cursor() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_filter = ".mxpf".to_string();
        app.open_files_filter_cursor = ".mx".len();

        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.open_files_filter, ".mxf");
        assert_eq!(app.open_files_filter_cursor, ".mx".len());
    }

    #[test]
    fn open_files_filter_shows_colon_and_terminal_cursor() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_filter = ".mp4".to_string();
        app.open_files_filter_cursor = ".m".len();
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 1,
            file_handles: 1,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![OpenFileEntry {
                path: r"C:\tmp\a.mp4".to_string(),
                handle_count: 1,
            }],
            error: None,
        });
        let screen = Rect::new(0, 0, 160, 45);
        let content = ui::process_info_content_area_for_screen(screen);
        let expected_cursor = Position::new(content.x + 10, content.y + 1);

        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("test render should succeed");
        terminal
            .backend_mut()
            .assert_cursor_position(expected_cursor);
        let rendered = buffer_to_text(terminal.backend().buffer());

        assert!(rendered.contains("Filter: .mp4"), "{rendered}");
    }

    #[test]
    fn open_files_modal_size_stays_fixed_while_filtering() {
        let mut app = make_test_app(1, 10);
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 3,
            file_handles: 3,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![
                OpenFileEntry {
                    path: r"C:\tmp\a.log".to_string(),
                    handle_count: 1,
                },
                OpenFileEntry {
                    path: r"C:\tmp\b.log".to_string(),
                    handle_count: 1,
                },
                OpenFileEntry {
                    path: r"C:\tmp\c.log".to_string(),
                    handle_count: 1,
                },
            ],
            error: None,
        });
        let screen = Rect::new(0, 0, 160, 45);
        show_process_info_files_tab(&mut app);
        let before = ui::process_info_page_size_for_screen(screen);

        app.open_files_filter = "b.log".to_string();
        let after = ui::process_info_page_size_for_screen(screen);

        assert_eq!(before, after);
    }

    #[test]
    fn open_files_modal_renders_table_columns() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 1,
            file_handles: 1,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![OpenFileEntry {
                path: r"C:\tmp\a.log".to_string(),
                handle_count: 1,
            }],
            error: None,
        });

        let rendered = render_app_to_text(&app, 160, 45);

        assert!(rendered.contains("Count File"), "{rendered}");
        assert!(rendered.contains("a.log"), "{rendered}");
        assert!(rendered.contains(r"C:\tmp"), "{rendered}");
    }

    #[test]
    fn open_files_filter_matches_directory_and_shows_filtered_total() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_filter = "fonts".to_string();
        app.open_files_filter_cursor = app.open_files_filter.len();
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 2,
            file_handles: 2,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![
                OpenFileEntry {
                    path: r"C:\Windows\Fonts\a.ttf".to_string(),
                    handle_count: 1,
                },
                OpenFileEntry {
                    path: r"C:\tmp\b.log".to_string(),
                    handle_count: 1,
                },
            ],
            error: None,
        });

        let rendered = render_app_to_text(&app, 120, 30);

        assert!(rendered.contains("shown 1/2"), "{rendered}");
        assert!(rendered.contains("a.ttf"), "{rendered}");
        assert!(!rendered.contains("b.log"), "{rendered}");
    }

    #[test]
    fn open_files_table_column_names_are_underlined() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 1,
            file_handles: 1,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![OpenFileEntry {
                path: r"C:\tmp\a.log".to_string(),
                handle_count: 1,
            }],
            error: None,
        });

        let buffer = render_app_to_buffer(&app, 160, 45);
        let (x, y) = find_text_position(&buffer, "Count").expect("header should render");
        let cell = &buffer[(x, y)];

        assert!(cell.modifier.contains(ratatui::style::Modifier::UNDERLINED));
        assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
    }

    #[test]
    fn open_files_scroll_offset_changes_rendered_rows() {
        let mut app = make_test_app(1, 10);
        show_process_info_files_tab(&mut app);
        app.open_files_result = Some(OpenFilesReport {
            pid: 0,
            process_name: "proc-0".to_string(),
            total_handles: 30,
            file_handles: 30,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: (0..30)
                .map(|index| OpenFileEntry {
                    path: format!(r"C:\tmp\file-{index:02}.log"),
                    handle_count: 1,
                })
                .collect(),
            error: None,
        });
        let screen = Rect::new(0, 0, 160, 45);
        app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));
        app.scroll_process_info_end();

        let rendered = render_app_to_text(&app, screen.width, screen.height);

        assert!(!rendered.contains("file-00.log"), "{rendered}");
        assert!(rendered.contains("file-29.log"), "{rendered}");
    }

    #[test]
    fn cached_process_info_is_reused_without_worker_request() {
        let mut app = make_test_app(2, 10);
        let identity = app.selected_visible_process_identity().unwrap();
        app.process_info_cache.insert(
            identity.clone(),
            test_process_info(&identity.name, identity.pid),
        );

        app.open_selected_process_info_dialog().unwrap();

        assert!(app.pending_process_info.is_none());
        assert_eq!(app.process_info_display_identity, Some(identity));
    }

    #[test]
    fn process_info_dialog_keeps_the_process_selected_when_opened() {
        let mut app = make_test_app(2, 10);
        let identity = app.selected_visible_process_identity().unwrap();
        app.process_info_cache.insert(
            identity.clone(),
            test_process_info(&identity.name, identity.pid),
        );
        app.process_info_display_identity = Some(identity);
        app.open_selected_process_info_dialog().unwrap();

        app.move_selection_down(1);

        assert_eq!(app.selected_visible_process().unwrap().name, "proc-1");
        assert_eq!(app.process_info_for_selected().unwrap().name, "proc-0");
        assert!(app.pending_process_info.is_none());
    }

    #[test]
    fn process_info_dialog_reopens_on_last_active_tab() {
        let mut app = make_test_app(2, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.activate_process_info_tab(app::ProcessInfoTab::Image)
            .unwrap();
        app.close_process_info_dialog();
        app.move_selection_down(1);

        app.open_selected_process_info_dialog().unwrap();

        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Image);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);
        assert_eq!(app.process_info_target_process().unwrap().name, "proc-1");
    }

    #[test]
    fn process_info_small_dialog_scrolls_without_overwriting_close_button() {
        let mut app = make_test_app(1, 10);
        let captured_at = app.snapshot.captured_at;
        let process = app.snapshot.processes[0].clone();
        app.process_history
            .record_snapshot_unbounded(captured_at, &[process]);
        app.open_selected_process_info_dialog().unwrap();
        let screen = Rect::new(0, 0, 60, 12);
        app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));
        let content = ui::process_info_content_area_for_screen(screen);

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: content.x,
                row: content.y,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert_eq!(app.process_info_scroll.offset, 1);
        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, screen.width, screen.height);
        assert!(rendered.contains("I/O Write Throughput"), "{rendered}");
        assert!(rendered.contains("[ Close ]"), "{rendered}");
    }

    #[test]
    fn process_info_ctrl_left_right_cycles_while_tab_moves_control_focus() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _open_files_request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        let target = app.selected_visible_process_identity().unwrap();
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_scroll.offset = 4;

        let screen = Rect::new(0, 0, 120, 40);
        let initial = render_app_to_buffer(&app, screen.width, screen.height);
        let (close_x, close_y) = find_text_position(&initial, "[ Close ]").unwrap();
        assert_eq!(initial[(close_x, close_y)].bg, app.theme().panel_alt);

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Metrics);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Close);
        let focused = render_app_to_buffer(&app, screen.width, screen.height);
        let (close_x, close_y) = find_text_position(&focused, "[ Close ]").unwrap();
        assert_eq!(focused[(close_x, close_y)].bg, app.theme().accent);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Metrics);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Metrics);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);

        for expected in [
            app::ProcessInfoTab::Image,
            app::ProcessInfoTab::Files,
            app::ProcessInfoTab::Dlls,
            app::ProcessInfoTab::Environment,
            app::ProcessInfoTab::Metrics,
        ] {
            app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
                .unwrap();
            assert_eq!(app.process_info_tab, expected);
            assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);
        }

        assert_eq!(app.process_info_scroll.offset, 4);
        assert_eq!(
            app.process_info_target
                .as_ref()
                .map(|target| &target.identity),
            Some(&target)
        );
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Environment);
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Dlls);
    }

    #[test]
    fn process_info_mouse_tabs_close_and_outside_click_are_modal() {
        let mut app = make_test_app(2, 10);
        app.open_selected_process_info_dialog().unwrap();
        let screen = Rect::new(0, 0, 200, 60);
        let layout = ui::process_info_dialog::process_info_dialog_layout_for_screen(screen);
        let image_point = (layout.tabs.y..layout.tabs.bottom())
            .flat_map(|y| (layout.tabs.x..layout.tabs.right()).map(move |x| (x, y)))
            .find(|(x, y)| {
                ui::process_info_tab_at(screen, *x, *y) == Some(app::ProcessInfoTab::Image)
            })
            .expect("Image tab should have a hit area");

        app.on_mouse(left_click(image_point.0, image_point.1), screen);
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Image);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Close);

        let selected = app.selected_visible_process_identity();
        let focused = app.focused_panel;
        app.on_mouse(left_click(0, 10), screen);
        assert!(app.show_process_info_dialog);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Close);
        assert_eq!(app.selected_visible_process_identity(), selected);
        assert_eq!(app.focused_panel, focused);

        let close = ui::process_info_close_button_area_for_screen(screen).unwrap();
        app.on_mouse(left_click(close.x, close.y), screen);
        assert!(!app.show_process_info_dialog);
    }

    #[test]
    fn process_info_image_shows_extended_fields_and_scrolls_long_values() {
        let mut app = make_test_app(1, 10);
        let identity = app.selected_visible_process_identity().unwrap();
        let mut info = test_process_info(&identity.name, identity.pid);
        info.command_line = InfoValue::Value(format!("{}COMMAND-END", "argument ".repeat(80)));
        app.process_info_cache.insert(identity.clone(), info);
        app.process_info_display_identity = Some(identity);
        app.open_selected_process_info_dialog().unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        let screen = Rect::new(0, 0, 70, 18);
        app.set_screen_area(screen);
        app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));

        let first_page = render_app_to_text(&app, screen.width, screen.height);
        assert!(first_page.contains("User"), "{first_page}");
        assert!(first_page.contains("Architecture"), "{first_page}");
        assert!(first_page.contains("Command line"), "{first_page}");

        app.scroll_process_info_end();
        let last_page = render_app_to_text(&app, screen.width, screen.height);
        assert!(last_page.contains("COMMAND-END"), "{last_page}");
        assert!(last_page.contains("Company"), "{last_page}");
        assert!(last_page.contains("File version"), "{last_page}");
    }

    #[test]
    fn files_tab_in_log_view_does_not_request_live_collection() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, process_request_rx, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, open_files_request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        app.log_view_path = Some(std::path::PathBuf::from("recording.log"));

        app.open_selected_process_info_dialog().unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();

        assert!(matches!(
            process_request_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
        assert!(matches!(
            open_files_request_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
        let rendered = render_app_to_text(&app, 120, 40);
        assert!(rendered.contains("Not recorded in Log view."), "{rendered}");
    }

    #[test]
    fn files_tab_does_not_query_after_the_fixed_target_exits() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, open_files_request_rx, _) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );

        app.open_selected_process_info_dialog().unwrap();
        app.snapshot.processes.clear();
        app.activate_process_info_tab(app::ProcessInfoTab::Files)
            .unwrap();

        assert!(matches!(
            open_files_request_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
        assert_eq!(
            app.open_files_result
                .as_ref()
                .and_then(|report| report.error.as_ref()),
            Some(&OpenFilesError::ProcessExited)
        );
        assert_eq!(app.status, "Process has exited");
    }

    #[test]
    fn stale_open_files_result_cannot_replace_reopened_dialog_request() {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, request_rx, result_tx) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );

        app.open_selected_process_files().unwrap();
        let (old_generation, identity) = match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect {
                generation,
                identity,
                ..
            } => (generation, identity),
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        };
        app.close_process_info_dialog();
        app.open_selected_process_files().unwrap();
        let new_generation = match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect { generation, .. } => generation,
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(OpenFilesResult {
                generation: old_generation,
                identity: identity.clone(),
                report: test_open_files_report(&identity.name, identity.pid, "old.log"),
            })
            .unwrap();
        assert!(!app.poll_open_files_results().unwrap());
        assert_eq!(app.open_files_in_flight_generation, Some(new_generation));
        assert!(app.open_files_result.is_none());

        result_tx
            .send(OpenFilesResult {
                generation: new_generation,
                identity: identity.clone(),
                report: test_open_files_report(&identity.name, identity.pid, "new.log"),
            })
            .unwrap();
        assert!(app.poll_open_files_results().unwrap());
        assert!(
            app.open_files_result.as_ref().unwrap().entries[0]
                .path
                .ends_with("new.log")
        );
    }

    #[test]
    fn dlls_tab_lazy_loads_once_for_the_fixed_dialog_target() {
        let (worker, request_rx, _) = ProcessModulesWorker::test_pair();
        let mut app = make_test_app(2, 10);
        app.process_modules_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        let target = app.process_info_target.as_ref().unwrap().identity.clone();

        activate_process_modules_tab(&mut app);
        match request_rx.try_recv().unwrap() {
            ProcessModulesRequest::Collect { identity, .. } => assert_eq!(identity, target),
            ProcessModulesRequest::Stop => panic!("unexpected stop request"),
        }
        app.move_selection_down(1);
        app.activate_process_info_tab(app::ProcessInfoTab::Dlls)
            .unwrap();

        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(
            app.process_modules_in_flight.as_ref(),
            Some(&target),
            "DLL collection must remain bound to the dialog target"
        );
    }

    #[test]
    fn dlls_tab_refresh_preserves_snapshot_path_filter_and_copies_selected_path() {
        let (worker, request_rx, result_tx) = ProcessModulesWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_modules_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        activate_process_modules_tab(&mut app);
        let (generation, request_id, identity) = match request_rx.try_recv().unwrap() {
            ProcessModulesRequest::Collect {
                generation,
                request_id,
                identity,
                ..
            } => (generation, request_id, identity),
            ProcessModulesRequest::Stop => panic!("unexpected stop request"),
        };
        let first = test_process_module_entry("first.dll", "First Company");
        let second = test_process_module_entry("second.dll", "Second Company");
        result_tx
            .send(ProcessModulesResult {
                generation,
                request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_modules_report(
                    &identity.name,
                    identity.pid,
                    vec![first, second.clone()],
                )),
            })
            .unwrap();
        assert!(app.poll_process_modules_results().unwrap());

        for ch in "second.dll".chars() {
            app.push_process_modules_filter_char(ch);
        }
        assert_eq!(ui::process_modules::filtered_entries(&app).len(), 1);
        let filtered = render_app_to_text(&app, 100, 30);
        assert!(filtered.contains("shown 1/2"), "{filtered}");
        app.copy_selected_process_module_to_clipboard().unwrap();
        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some(second.path.as_str())
        );

        app.refresh_process_modules().unwrap();
        let refresh = match request_rx.try_recv().unwrap() {
            ProcessModulesRequest::Collect {
                generation,
                request_id,
                ..
            } => (generation, request_id),
            ProcessModulesRequest::Stop => panic!("unexpected stop request"),
        };
        assert!(app.process_modules_result.is_some());
        app.refresh_process_modules().unwrap();
        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(app.status, "DLL refresh already in progress");

        result_tx
            .send(ProcessModulesResult {
                generation: refresh.0,
                request_id: refresh.1,
                identity,
                outcome: Err(ProcessModulesError::AccessDenied),
            })
            .unwrap();
        assert!(app.poll_process_modules_results().unwrap());
        assert!(app.process_modules_result.is_some());
        assert_eq!(
            app.process_modules_error,
            Some(ProcessModulesError::AccessDenied)
        );
    }

    #[test]
    fn dlls_tab_lists_full_paths_and_enter_opens_selected_detail() {
        let mut app = make_test_app(1, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Dlls;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        let mut entry = test_process_module_entry(
            "a-very-long-module-name-that-does-not-fit.dll",
            "A Company With A Long Name",
        );
        entry.product_version = InfoValue::NotAvailable;
        app.process_modules_result_identity = Some(identity.clone());
        app.process_modules_result = Some(test_process_modules_report(
            &identity.name,
            identity.pid,
            vec![entry],
        ));

        let list = render_app_to_text(&app, 68, 26);
        assert!(list.contains("DLL path"), "{list}");
        assert!(list.contains(r"C:\Program Files\Test"), "{list}");
        assert!(!list.contains("Product Version"), "{list}");

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.process_modules_show_detail);
        let detail = render_app_to_text(&app, 68, 26);
        assert!(detail.contains("DLL details"), "{detail}");
        assert!(detail.contains("DLL file"), "{detail}");
        assert!(detail.contains("Product Version"), "{detail}");
        assert!(detail.contains("Directory"), "{detail}");
        assert!(detail.contains("<not available>"), "{detail}");
        assert!(detail.contains("Esc/Enter back"), "{detail}");

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_process_info_dialog);
        assert!(!app.process_modules_show_detail);

        app.snapshot.processes.clear();
        let exited = render_app_to_text(&app, 100, 30);
        assert!(exited.contains("process exited"), "{exited}");
    }

    #[test]
    fn dlls_tab_arrow_selection_controls_enter_detail_target() {
        let mut app = make_test_app(1, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Dlls;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        app.process_modules_result_identity = Some(identity.clone());
        app.process_modules_result = Some(test_process_modules_report(
            &identity.name,
            identity.pid,
            vec![
                test_process_module_entry("first.dll", "First Company"),
                test_process_module_entry("second.dll", "Second Company"),
            ],
        ));

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.process_modules_selected, 1);
        assert!(app.process_modules_show_detail);
        let detail = render_app_to_text(&app, 80, 26);
        assert!(detail.contains("second.dll"), "{detail}");
        assert!(detail.contains("Second Company"), "{detail}");
        assert!(!detail.contains("First Company"), "{detail}");
    }

    #[test]
    fn dlls_tab_in_log_view_starts_no_worker() {
        let (worker, request_rx, _) = ProcessModulesWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_modules_worker = worker;
        app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
        app.open_selected_process_info_dialog().unwrap();
        activate_process_modules_tab(&mut app);

        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        let rendered = render_app_to_text(&app, 120, 40);
        assert!(rendered.contains("Not recorded in Log view."), "{rendered}");
    }

    #[test]
    fn stale_dll_result_cannot_replace_reopened_dialog_request() {
        let (worker, request_rx, result_tx) = ProcessModulesWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_modules_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        activate_process_modules_tab(&mut app);
        let (old_generation, old_request_id, identity) = match request_rx.try_recv().unwrap() {
            ProcessModulesRequest::Collect {
                generation,
                request_id,
                identity,
                ..
            } => (generation, request_id, identity),
            ProcessModulesRequest::Stop => panic!("unexpected stop request"),
        };

        app.close_process_info_dialog();
        app.open_selected_process_info_dialog().unwrap();
        activate_process_modules_tab(&mut app);
        let (new_generation, new_request_id) = match request_rx.try_recv().unwrap() {
            ProcessModulesRequest::Collect {
                generation,
                request_id,
                ..
            } => (generation, request_id),
            ProcessModulesRequest::Stop => panic!("unexpected stop request"),
        };
        result_tx
            .send(ProcessModulesResult {
                generation: old_generation,
                request_id: old_request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_modules_report(
                    &identity.name,
                    identity.pid,
                    vec![test_process_module_entry("old.dll", "Old")],
                )),
            })
            .unwrap();
        assert!(!app.poll_process_modules_results().unwrap());
        assert_eq!(
            app.process_modules_in_flight_request_id,
            Some(new_request_id)
        );
        assert!(app.process_modules_result.is_none());

        result_tx
            .send(ProcessModulesResult {
                generation: new_generation,
                request_id: new_request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_modules_report(
                    &identity.name,
                    identity.pid,
                    vec![test_process_module_entry("new.dll", "New")],
                )),
            })
            .unwrap();
        assert!(app.poll_process_modules_results().unwrap());
        assert_eq!(
            app.process_modules_result.as_ref().unwrap().entries[0].dll_name,
            "new.dll"
        );
    }

    #[test]
    fn environment_tab_lazy_loads_once_for_the_fixed_dialog_target() {
        let (worker, request_rx, _) = ProcessEnvironmentWorker::test_pair();
        let mut app = make_test_app(2, 10);
        app.process_environment_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        let target = app.process_info_target.as_ref().unwrap().identity.clone();

        activate_process_environment_tab(&mut app);
        match request_rx.try_recv().unwrap() {
            ProcessEnvironmentRequest::Collect { identity, .. } => assert_eq!(identity, target),
            ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
        }
        app.move_selection_down(1);
        app.activate_process_info_tab(app::ProcessInfoTab::Environment)
            .unwrap();

        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(app.process_environment_in_flight.as_ref(), Some(&target));
    }

    #[test]
    fn environment_refresh_preserves_snapshot_filters_values_and_copies_one_entry() {
        let (worker, request_rx, result_tx) = ProcessEnvironmentWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_environment_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        activate_process_environment_tab(&mut app);
        let (generation, request_id, identity) = match request_rx.try_recv().unwrap() {
            ProcessEnvironmentRequest::Collect {
                generation,
                request_id,
                identity,
                ..
            } => (generation, request_id, identity),
            ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
        };
        let secret = "sensitive-value-for-filter-test";
        result_tx
            .send(ProcessEnvironmentResult {
                generation,
                request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_environment_report(
                    &identity.name,
                    identity.pid,
                    vec![
                        ProcessEnvironmentEntry {
                            name: "EMPTY".to_string(),
                            value: String::new(),
                        },
                        ProcessEnvironmentEntry {
                            name: "TOKEN".to_string(),
                            value: secret.to_string(),
                        },
                    ],
                )),
            })
            .unwrap();
        assert!(app.poll_process_environment_results().unwrap());

        for ch in "value-for-filter".chars() {
            app.push_process_environment_filter_char(ch);
        }
        for ch in " missing-term".chars() {
            app.push_process_environment_filter_char(ch);
        }
        assert_eq!(ui::process_environment::filtered_entries(&app).len(), 1);
        app.copy_selected_process_environment_to_clipboard()
            .unwrap();
        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("TOKEN=sensitive-value-for-filter-test")
        );

        app.refresh_process_environment().unwrap();
        let refresh = match request_rx.try_recv().unwrap() {
            ProcessEnvironmentRequest::Collect {
                generation,
                request_id,
                ..
            } => (generation, request_id),
            ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
        };
        assert!(app.process_environment_result.is_some());
        app.refresh_process_environment().unwrap();
        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(app.status, "Environment refresh already in progress");

        result_tx
            .send(ProcessEnvironmentResult {
                generation: refresh.0,
                request_id: refresh.1,
                identity,
                outcome: Err(ProcessEnvironmentError::AccessDenied),
            })
            .unwrap();
        assert!(app.poll_process_environment_results().unwrap());
        assert!(app.process_environment_result.is_some());
        assert_eq!(
            app.process_environment_error,
            Some(ProcessEnvironmentError::AccessDenied)
        );
        assert!(!app.status.contains(secret));
        app.close_process_info_dialog();
        assert!(app.process_environment_result.is_none());
    }

    #[test]
    fn environment_tab_enter_opens_long_selected_value_detail() {
        let mut app = make_test_app(1, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Environment;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        let long_value = "C:\\one;C:\\two;C:\\three;C:\\four;C:\\five;C:\\six";
        let mut report = test_process_environment_report(
            &identity.name,
            identity.pid,
            vec![ProcessEnvironmentEntry {
                name: "PATH".to_string(),
                value: long_value.to_string(),
            }],
        );
        report.malformed_entries = 2;
        app.process_environment_result_identity = Some(identity);
        app.process_environment_result = Some(report);

        let list = render_app_to_text(&app, 60, 24);
        assert!(list.contains("Name"), "{list}");
        assert!(list.contains("Value"), "{list}");
        assert!(!list.contains("Environment may contain secrets"), "{list}");
        assert!(list.contains("2 malformed entries skipped"), "{list}");

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.process_environment_show_detail);
        let detail = render_app_to_text(&app, 60, 24);
        assert!(detail.contains("Environment variable details"), "{detail}");
        assert!(detail.contains("C:\\one;"), "{detail}");
        assert!(detail.contains("Esc/Enter back"), "{detail}");
    }

    #[test]
    fn environment_filter_cursor_and_mouse_rows_match_rendered_layout() {
        let mut app = make_test_app(1, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Environment;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        app.process_environment_result_identity = Some(identity.clone());
        app.process_environment_result = Some(test_process_environment_report(
            &identity.name,
            identity.pid,
            vec![
                ProcessEnvironmentEntry {
                    name: "FIRST".to_string(),
                    value: "one".to_string(),
                },
                ProcessEnvironmentEntry {
                    name: "SECOND".to_string(),
                    value: "two".to_string(),
                },
            ],
        ));
        app.process_environment_filter = "o".to_string();
        app.process_environment_filter_cursor = 1;
        let screen = Rect::new(0, 0, 160, 45);
        let content = ui::process_info_content_area_for_screen(screen);
        let expected_cursor = Position::new(content.x + "Filter: ".len() as u16 + 1, content.y + 1);

        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("test render should succeed");
        terminal
            .backend_mut()
            .assert_cursor_position(expected_cursor);
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_text(&buffer);
        let (second_x, second_y) =
            find_text_position(&buffer, "SECOND").expect("second environment row should render");

        assert!(
            !rendered.contains("Environment may contain secrets"),
            "{rendered}"
        );
        assert_eq!(second_y, content.y + 4);
        app.on_mouse(left_click(second_x, second_y), screen);
        assert_eq!(app.process_environment_selected, 1);
    }

    #[test]
    fn environment_detail_is_keyboard_scrollable_on_short_screens() {
        let mut app = make_test_app(1, 10);
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Environment;
        let identity = app.process_info_target.as_ref().unwrap().identity.clone();
        let long_value = format!("{}VALUE-END", "abcdefghij".repeat(20));
        app.process_environment_result_identity = Some(identity.clone());
        app.process_environment_result = Some(test_process_environment_report(
            &identity.name,
            identity.pid,
            vec![ProcessEnvironmentEntry {
                name: "LONG_VALUE".to_string(),
                value: long_value,
            }],
        ));
        let screen = Rect::new(0, 0, 50, 12);
        app.set_screen_area(screen);
        app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();

        let detail = render_app_to_text(&app, screen.width, screen.height);
        assert!(detail.contains("VALUE-END"), "{detail}");
        assert!(detail.contains("[ Close ]"), "{detail}");
    }

    #[test]
    fn environment_tab_in_log_view_starts_no_worker() {
        let (worker, request_rx, _) = ProcessEnvironmentWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_environment_worker = worker;
        app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
        app.open_selected_process_info_dialog().unwrap();
        activate_process_environment_tab(&mut app);

        assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
        let rendered = render_app_to_text(&app, 120, 40);
        assert!(rendered.contains("Not recorded in Log view."), "{rendered}");
    }

    #[test]
    fn stale_environment_result_cannot_replace_reopened_dialog_request() {
        let (worker, request_rx, result_tx) = ProcessEnvironmentWorker::test_pair();
        let mut app = make_test_app(1, 10);
        app.process_environment_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        activate_process_environment_tab(&mut app);
        let (old_generation, old_request_id, identity) = match request_rx.try_recv().unwrap() {
            ProcessEnvironmentRequest::Collect {
                generation,
                request_id,
                identity,
                ..
            } => (generation, request_id, identity),
            ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
        };
        app.close_process_info_dialog();
        app.open_selected_process_info_dialog().unwrap();
        assert_eq!(app.process_info_tab, app::ProcessInfoTab::Environment);
        let (new_generation, new_request_id) = match request_rx.try_recv().unwrap() {
            ProcessEnvironmentRequest::Collect {
                generation,
                request_id,
                ..
            } => (generation, request_id),
            ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(ProcessEnvironmentResult {
                generation: old_generation,
                request_id: old_request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_environment_report(
                    &identity.name,
                    identity.pid,
                    vec![ProcessEnvironmentEntry {
                        name: "OLD".to_string(),
                        value: "old".to_string(),
                    }],
                )),
            })
            .unwrap();
        assert!(!app.poll_process_environment_results().unwrap());
        assert_eq!(
            app.process_environment_in_flight_request_id,
            Some(new_request_id)
        );
        assert!(app.process_environment_result.is_none());

        result_tx
            .send(ProcessEnvironmentResult {
                generation: new_generation,
                request_id: new_request_id,
                identity: identity.clone(),
                outcome: Ok(test_process_environment_report(
                    &identity.name,
                    identity.pid,
                    vec![ProcessEnvironmentEntry {
                        name: "NEW".to_string(),
                        value: "new".to_string(),
                    }],
                )),
            })
            .unwrap();
        assert!(app.poll_process_environment_results().unwrap());
        assert_eq!(
            app.process_environment_result.as_ref().unwrap().entries[0].name,
            "NEW"
        );
    }

    #[test]
    fn tab_cycles_focus_through_visible_panels() {
        let mut app = make_test_app(1, 10);

        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::System);

        let identity = app.selected_visible_process_identity().unwrap();
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Workset),
            FocusedPanel::Processes,
        );
        let active_id = app.active_graph_id;
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::SystemActivity);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_id, active_id);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::System);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_id, active_id);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::SystemActivity);
    }

    #[test]
    fn tab_leaves_graph_workspace_when_samples_are_hidden() {
        let mut app = make_test_app(1, 10);
        let identity = app.selected_visible_process_identity().unwrap();
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Workset),
            FocusedPanel::Processes,
        );
        app.show_samples_panel = false;
        app.focused_panel = FocusedPanel::DetailsGraph;
        let active_id = app.active_graph_id;

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.focused_panel, FocusedPanel::System);
        assert_eq!(app.active_graph_id, active_id);
    }

    #[test]
    fn process_navigation_only_runs_when_processes_are_focused() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.process_table_state.selected(), Some(0));
    }

    #[test]
    fn watch_list_filters_processes_by_exact_name() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.snapshot.processes[2].name = "cargo-watch.exe".to_string();
        app.watch_list = vec!["CARGO.EXE".to_string()];
        app.normalized_watch_names = ["cargo.exe".to_string()].into_iter().collect();
        app.watch_enabled = true;
        app.rebuild_visible_process_cache();

        let visible = app
            .visible_processes()
            .into_iter()
            .map(|process| process.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec!["cargo.exe"]);
        assert_eq!(
            app.tracked_total_visible_row().unwrap().process.name,
            "Tracked Total"
        );
    }

    #[test]
    fn selected_process_can_be_added_to_watch_list() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.move_selection_down(1);

        app.add_selected_process_to_watch_list();

        assert!(!app.watch_enabled);
        assert_eq!(app.watch_list, vec!["winproc-tui.exe"]);
        assert_eq!(app.visible_process_count(), 3);
    }

    #[test]
    fn t_toggles_selected_process_in_tracked_list() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.move_selection_down(1);

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
        assert_eq!(app.watch_list, vec!["winproc-tui.exe"]);
        assert_eq!(app.visible_process_count(), 3);

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
        assert!(app.watch_list.is_empty());
        assert_eq!(app.visible_process_count(), 3);
    }

    #[test]
    fn f4_does_not_add_selected_process_to_tracked_list() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE))
            .unwrap();

        assert!(app.watch_list.is_empty());
        assert!(!app.watch_enabled);
    }

    #[test]
    fn f5_does_not_remove_selected_process_from_tracked_list() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "winproc-tui.exe".to_string();
        app.watch_list = vec!["winproc-tui.exe".to_string()];
        app.normalized_watch_names = ["winproc-tui.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.watch_list, vec!["winproc-tui.exe"]);
    }

    #[test]
    fn ctrl_t_opens_tracked_lists_without_toggling_tracked_only() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["proc-0".to_string()];
        app.normalized_watch_names = ["proc-0".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.tracked_lists_dialog.is_some());
        assert!(!app.watch_enabled);
    }

    #[test]
    fn save_current_tracked_list_creates_named_list_without_changing_t_semantics() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["proc-0".to_string(), "worker.exe".to_string()];
        app.normalized_watch_names = ["proc-0".to_string(), "worker.exe".to_string()]
            .into_iter()
            .collect();
        app.open_tracked_lists();
        app.focus_tracked_lists_save_name();
        for ch in "API debug".chars() {
            app.push_tracked_list_save_name_char(ch);
        }

        app.save_current_tracked_list();

        assert_eq!(
            app.runtime.active_tracked_list.as_deref(),
            Some("API debug")
        );
        assert_eq!(app.runtime.saved_tracked_lists.len(), 1);
        assert_eq!(
            app.runtime.saved_tracked_lists[0].processes,
            vec!["proc-0", "worker.exe"]
        );
        assert!(!app.active_tracked_list_dirty());

        app.close_tracked_lists();
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.active_tracked_list_dirty());
    }

    #[test]
    fn save_current_tracked_list_persists_immediately() {
        let mut app = make_test_app(1, 10);
        let path = unique_config_path("tracked-list-save-as");
        let _ = std::fs::remove_file(&path);
        app.runtime.config_path = Some(path.clone());
        app.watch_list = vec!["api.exe".to_string(), "worker.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string(), "worker.exe".to_string()]
            .into_iter()
            .collect();
        app.open_tracked_lists();
        app.focus_tracked_lists_save_name();
        for ch in "API".chars() {
            app.push_tracked_list_save_name_char(ch);
        }

        app.save_current_tracked_list();

        let saved: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.tracking.active_list.as_deref(), Some("API"));
        assert_eq!(saved.tracked_lists.len(), 1);
        assert_eq!(
            saved.tracked_lists[0].processes,
            vec!["api.exe", "worker.exe"]
        );
    }

    #[test]
    fn save_current_tracked_list_defaults_to_active_name_and_updates_it() {
        let mut app = make_test_app(1, 10);
        app.runtime.active_tracked_list = Some("API".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["old.exe".to_string()],
        }];
        app.watch_list = vec!["api.exe".to_string(), "worker.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string(), "worker.exe".to_string()]
            .into_iter()
            .collect();
        app.open_tracked_lists();

        let (draft, cursor, error) = app
            .tracked_lists_save_name()
            .expect("save-name input should be available");
        assert_eq!(draft, "API");
        assert_eq!(cursor, 3);
        assert_eq!(error, None);

        app.save_current_tracked_list();

        assert_eq!(app.runtime.saved_tracked_lists.len(), 1);
        assert_eq!(
            app.runtime.saved_tracked_lists[0].processes,
            vec!["api.exe", "worker.exe"]
        );
        assert_eq!(app.runtime.active_tracked_list.as_deref(), Some("API"));
        assert!(!app.active_tracked_list_dirty());
        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("Saved: API · 2 processes"), "{rendered}");
    }

    #[test]
    fn loading_named_tracked_list_replaces_active_working_copy() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["old.exe".to_string()];
        app.normalized_watch_names = ["old.exe".to_string()].into_iter().collect();
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string(), "worker.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);

        app.load_selected_tracked_list();

        assert_eq!(app.watch_list, vec!["api.exe", "worker.exe"]);
        assert_eq!(app.runtime.active_tracked_list.as_deref(), Some("API"));
        assert!(app.tracked_lists_dialog.is_none());
        assert!(!app.active_tracked_list_dirty());
    }

    #[test]
    fn loading_named_tracked_list_confirms_before_discarding_older_history() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "old.exe".to_string();
        track_process_name(&mut app, "old.exe");
        record_tracked_process_history_samples(&mut app, "old.exe", 121);
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.move_tracked_list_selection_down(1);

        app.load_selected_tracked_list();

        let Some(app::TrackedListsView::ConfirmSwitch { pending, selection }) =
            app.tracked_lists_view()
        else {
            panic!("expected tracked-list switch confirmation");
        };
        assert_eq!(*selection, app::TrackedListConfirmSelection::Cancel);
        assert_eq!(pending.removed_name_count, 1);
        assert_eq!(pending.affected_name_count, 1);
        assert_eq!(pending.discarded_sample_count, 1);
        assert_eq!(app.watch_list, vec!["old.exe"]);
        assert_eq!(selected_process_history_sample_count(&app, "old.exe"), 121);

        app.set_tracked_list_confirmation_selection(app::TrackedListConfirmSelection::Apply);
        app.activate_tracked_list_confirmation();

        assert_eq!(app.watch_list, vec!["api.exe"]);
        assert_eq!(app.runtime.active_tracked_list.as_deref(), Some("API"));
        assert_eq!(selected_process_history_sample_count(&app, "old.exe"), 120);
        assert!(app.tracked_lists_dialog.is_none());
    }

    #[test]
    fn loading_builtin_empty_confirms_before_discarding_older_history() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "old.exe".to_string();
        track_process_name(&mut app, "old.exe");
        record_tracked_process_history_samples(&mut app, "old.exe", 121);
        app.watch_enabled = true;
        app.open_tracked_lists();

        app.load_selected_tracked_list();

        let Some(app::TrackedListsView::ConfirmSwitch { pending, selection }) =
            app.tracked_lists_view()
        else {
            panic!("expected built-in empty switch confirmation");
        };
        assert_eq!(*selection, app::TrackedListConfirmSelection::Cancel);
        assert_eq!(pending.target_name, None);
        assert!(pending.target_processes.is_empty());
        assert_eq!(pending.discarded_sample_count, 1);
        assert_eq!(app.watch_list, vec!["old.exe"]);

        app.set_tracked_list_confirmation_selection(app::TrackedListConfirmSelection::Apply);
        app.activate_tracked_list_confirmation();

        assert!(app.watch_list.is_empty());
        assert!(app.watch_enabled);
        assert_eq!(app.runtime.active_tracked_list, None);
        assert_eq!(selected_process_history_sample_count(&app, "old.exe"), 120);
    }

    #[test]
    fn deleting_active_saved_list_keeps_working_copy_unsaved() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["api.exe".to_string()];
        app.normalized_watch_names = ["api.exe".to_string()].into_iter().collect();
        app.runtime.active_tracked_list = Some("API".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["api.exe".to_string()],
        }];
        app.open_tracked_lists();
        app.request_delete_selected_tracked_list();
        app.set_tracked_list_confirmation_selection(app::TrackedListConfirmSelection::Apply);

        app.activate_tracked_list_confirmation();

        assert!(app.runtime.saved_tracked_lists.is_empty());
        assert_eq!(app.runtime.active_tracked_list, None);
        assert_eq!(app.watch_list, vec!["api.exe"]);
        assert!(app.active_tracked_list_dirty());
    }

    #[test]
    fn shift_t_toggles_tracked_only_when_processes_are_focused() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
            .unwrap();

        assert!(app.watch_enabled);
        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(app.visible_process_at(0).unwrap().name, "target.exe");
        assert_eq!(
            app.tracked_total_visible_row().unwrap().process.name,
            "Tracked Total"
        );

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SHIFT))
            .unwrap();

        assert!(!app.watch_enabled);
        assert_eq!(app.visible_process_count(), 2);
    }

    #[test]
    fn tracked_only_preserves_graph_order_active_graph_and_valid_scroll() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();
        for index in 0..16 {
            add_test_graph(&mut app, index);
        }
        app.show_samples_panel = false;
        let screen = Rect::new(0, 0, 100, 30);
        app::sync_layout_state(&mut app, screen);
        app.set_graph_scroll_row(1);
        let entries = app.graph_entries.clone();
        let active = app.active_graph_id;

        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
            .unwrap();
        app::sync_layout_state(&mut app, screen);

        assert!(app.watch_enabled);
        assert_eq!(app.graph_entries, entries);
        assert_eq!(app.active_graph_id, active);
        assert_eq!(app.graph_scroll_row, 1);
    }

    #[test]
    fn tracked_only_adds_active_total_row() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[0].private_bytes = Some(10);
        app.snapshot.processes[0].cpu_percent = Some(12.5);
        app.snapshot.processes[1].name = "target.exe".to_string();
        app.snapshot.processes[1].private_bytes = Some(25);
        app.snapshot.processes[1].cpu_percent = Some(7.5);
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
            .unwrap();

        let total = app.tracked_total_visible_row().unwrap().process;
        assert_eq!(total.name, "Tracked Total");
        assert_eq!(total.private_bytes, Some(35));
        assert_eq!(total.cpu_percent, Some(20.0));
        assert_eq!(app.process_table_state.selected(), Some(0));
    }

    #[test]
    fn tracked_total_renders_immediately_after_visible_process_rows() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[0].private_bytes = Some(10);
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
            .unwrap();

        let screen = Rect::new(0, 0, 100, 30);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let process_area = main_panel_areas_for_app(screen, &app).processes.area;
        let (_, process_y) =
            find_text_position(&buffer, "target.exe").expect("tracked process should be rendered");
        let (_, total_y) =
            find_text_position(&buffer, "Tracked Total").expect("tracked total should be rendered");

        assert_eq!(total_y, process_y + 1);
        assert!(total_y < process_area.bottom().saturating_sub(2));
    }

    #[test]
    fn tracked_only_count_reports_visible_rows_not_stored_names() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["missing-a.exe".to_string(), "missing-b.exe".to_string()];
        app.normalized_watch_names = ["missing-a.exe".to_string(), "missing-b.exe".to_string()]
            .into_iter()
            .collect();

        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
            .unwrap();

        let rendered = render_app_to_text(&app, 100, 30);
        assert!(app.watch_enabled);
        assert_eq!(app.visible_process_count(), 0);
        assert_eq!(app.visible_tracked_process_count(), 0);
        assert!(app.status.contains("0 visible"));
        assert!(
            rendered.contains("PROCESSES · 0 visible · ☑ Tracked-only(Shift+T)"),
            "{rendered}"
        );
    }

    #[test]
    fn process_table_title_shows_concise_active_view_state() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.snapshot.processes[2].name = "target-helper.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();
        app.watch_enabled = true;
        app.filter_text = "target".to_string();
        app.column_preset = ColumnPreset::Custom;
        app.rebuild_visible_process_cache();

        let buffer = render_app_to_buffer(&app, 130, 30);
        let rendered = buffer_to_text(&buffer);

        assert!(
            rendered
                .contains("PROCESSES · 1 visible · ☑ Tracked-only(Shift+T) · Filter \"target\""),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Filter \"target\" · WS Priv"),
            "{rendered}"
        );
        assert!(!rendered.contains("Max samples: normal"), "{rendered}");
        assert!(!rendered.contains("[x]"), "{rendered}");
        assert!(!rendered.contains("Custom"), "{rendered}");

        let (state_x, state_y) = find_text_position(&buffer, "☑ Tracked-only(Shift+T)")
            .expect("tracked-only state should be rendered");
        let state_cell = &buffer[(state_x, state_y)];
        assert_eq!(state_cell.fg, ui::THEMES[0].tracked);
        assert_ne!(state_cell.fg, ui::THEMES[0].warning);
        assert_eq!(state_cell.bg, ui::THEMES[0].panel);
        assert!(!state_cell.modifier.contains(Modifier::BOLD));

        let (filter_x, filter_y) = find_text_position(&buffer, "Filter \"target\"")
            .expect("filter state should be rendered");
        let filter_cell = &buffer[(filter_x, filter_y)];
        assert_eq!(filter_cell.fg, ui::THEMES[0].warning);
        assert_ne!(filter_cell.fg, ui::THEMES[0].tracked);
    }

    #[test]
    fn process_table_title_omits_named_list_and_unsaved_marker() {
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["proc-0".to_string()];
        app.normalized_watch_names = ["proc-0".to_string()].into_iter().collect();
        app.runtime.active_tracked_list = Some("API".to_string());
        app.runtime.saved_tracked_lists = vec![config::SavedTrackedList {
            name: "API".to_string(),
            processes: vec!["proc-0".to_string()],
        }];

        let saved = render_app_to_text(&app, 120, 30);
        assert!(
            saved.contains("PROCESSES · 1 visible · ☐ Tracked-only(Shift+T)"),
            "{saved}"
        );
        assert!(!saved.contains("List \"API\""), "{saved}");

        app.watch_list.push("worker.exe".to_string());
        app.normalized_watch_names.insert("worker.exe".to_string());
        let dirty = render_app_to_text(&app, 120, 30);
        assert!(
            dirty.contains("PROCESSES · 1 visible · ☐ Tracked-only(Shift+T)"),
            "{dirty}"
        );
        assert!(!dirty.contains("List \"API*\""), "{dirty}");
    }

    #[test]
    fn process_table_filter_editing_shows_prominent_title_input() {
        let mut app = make_test_app(3, 10);
        app.begin_filter_edit();
        app.push_filter_char('t');
        app.push_filter_char('a');
        let buffer = render_app_to_buffer(&app, 130, 30);
        let rendered = buffer_to_text(&buffer);
        let (label_x, label_y) =
            find_text_position(&buffer, "Filter").expect("filter input label should be rendered");
        let (x, y) =
            find_text_position(&buffer, "ta_").expect("filter input text should be rendered");
        let label_cell = &buffer[(label_x, label_y)];
        let cell = &buffer[(x, y)];
        let cursor_cell = &buffer[(x + 2, y)];

        assert!(!rendered.contains("[Editing filter:"), "{rendered}");
        assert!(
            !rendered.contains("[Max samples: normal 120 / tracked 7200]"),
            "{rendered}"
        );
        assert_eq!(label_cell.fg, ui::THEMES[0].background);
        assert_eq!(label_cell.bg, ui::THEMES[0].warning);
        assert_eq!(cell.fg, ui::THEMES[0].warning);
        assert_eq!(cell.bg, ui::THEMES[0].panel_alt);
        assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
        assert_eq!(cursor_cell.fg, ui::THEMES[0].background);
        assert_eq!(cursor_cell.bg, ui::THEMES[0].warning);
        assert!(
            cursor_cell
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }

    #[test]
    fn t_does_not_toggle_tracked_only_when_graph_is_focused() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
        assert_eq!(app.visible_process_count(), 2);
    }

    #[test]
    fn f3_does_not_toggle_tracked_only() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
    }

    #[test]
    fn selected_process_can_be_removed_from_watch_list() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.watch_list = vec!["cargo.exe".to_string()];
        app.watch_enabled = true;

        app.remove_selected_process_from_watch_list();

        assert!(!app.watch_enabled);
        assert!(app.watch_list.is_empty());
    }

    #[test]
    fn removing_tracked_process_with_short_history_does_not_confirm() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        track_process_name(&mut app, "target.exe");
        record_tracked_process_history_samples(&mut app, "target.exe", 120);

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_tracked_remove_confirmation);
        assert!(app.watch_list.is_empty());
        assert_eq!(
            selected_process_history_sample_count(&app, "target.exe"),
            120
        );
    }

    #[test]
    fn removing_tracked_process_with_long_history_opens_confirm() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        track_process_name(&mut app, "target.exe");
        record_tracked_process_history_samples(&mut app, "target.exe", 121);

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_tracked_remove_confirmation);
        assert_eq!(app.tracked_remove_selection, TrackedRemoveSelection::Cancel);
        assert_eq!(app.tracked_remove_name, "target.exe");
        assert_eq!(app.tracked_remove_total_samples, 121);
        assert_eq!(app.tracked_remove_discarded_samples, 1);
        assert_eq!(app.watch_list, vec!["target.exe"]);
        assert_eq!(
            selected_process_history_sample_count(&app, "target.exe"),
            121
        );

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("Remove from Tracked List?"), "{rendered}");
        assert!(
            rendered.contains("target.exe has 121 in-memory samples."),
            "{rendered}"
        );
        assert!(
            rendered.contains("This will keep the latest 120 samples and discard 1 older samples."),
            "{rendered}"
        );
        assert!(rendered.contains("Continue?"), "{rendered}");
        assert!(
            rendered.contains("Enter Select  Esc Cancel  y Remove"),
            "{rendered}"
        );
    }

    #[test]
    fn tracked_remove_confirm_cancels_without_pruning() {
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let mut app = make_test_app(1, 10);
            app.snapshot.processes[0].name = "target.exe".to_string();
            track_process_name(&mut app, "target.exe");
            record_tracked_process_history_samples(&mut app, "target.exe", 121);
            app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
                .unwrap();

            app.on_key(key).unwrap();

            assert!(!app.show_tracked_remove_confirmation);
            assert_eq!(app.watch_list, vec!["target.exe"]);
            assert_eq!(
                selected_process_history_sample_count(&app, "target.exe"),
                121
            );
            assert_eq!(app.status, "Tracked removal canceled");
        }
    }

    #[test]
    fn tracked_remove_confirm_with_y_removes_and_prunes_history() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        track_process_name(&mut app, "target.exe");
        record_tracked_process_history_samples(&mut app, "target.exe", 121);
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_tracked_remove_confirmation);
        assert!(app.watch_list.is_empty());
        assert_eq!(
            selected_process_history_sample_count(&app, "target.exe"),
            120
        );
        assert!(app.status.contains("discarded 1 older samples"));
    }

    #[test]
    fn tracked_remove_confirm_with_remove_selection_removes_and_prunes_history() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        track_process_name(&mut app, "target.exe");
        record_tracked_process_history_samples(&mut app, "target.exe", 121);
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.tracked_remove_selection, TrackedRemoveSelection::Remove);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_tracked_remove_confirmation);
        assert!(app.watch_list.is_empty());
        assert_eq!(
            selected_process_history_sample_count(&app, "target.exe"),
            120
        );
    }

    #[test]
    fn tracked_process_exit_adds_ghost_row() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.sampling_in_progress = true;

        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(0),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(app.visible_process_at(0).unwrap().name, "target.exe");
        assert_eq!(app.exited_tracked_rows.len(), 1);
    }

    #[test]
    fn exited_process_name_shows_close_time() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.snapshot.captured_at = Local.with_ymd_and_hms(2026, 5, 9, 12, 34, 56).unwrap();
        app.sampling_in_progress = true;

        let mut next = test_snapshot(0);
        next.captured_at = Local.with_ymd_and_hms(2026, 5, 9, 12, 34, 56).unwrap();
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("target.⋯(12:34:56)"), "{rendered}");
    }

    #[test]
    fn tracked_only_includes_live_and_ghost_rows_with_live_first() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(2, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.toggle_watch_list();
        app.sampling_in_progress = true;

        let mut next = test_snapshot(1);
        next.processes[0].name = "target.exe".to_string();
        next.processes[0].start_time = Some(1_800_000_000);
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.visible_process_count(), 2);
        assert!(matches!(
            app.visible_process_entries[0],
            VisibleProcessEntry::Live(_)
        ));
        assert!(matches!(
            app.visible_process_entries[1],
            VisibleProcessEntry::Ghost(_)
        ));
        assert!(app.tracked_total_visible_row().is_some());
    }

    #[test]
    fn exited_tracked_rows_stay_below_live_rows_in_full_process_list() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(2, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.sampling_in_progress = true;

        let mut next = test_snapshot(1);
        next.processes[0].pid = 1;
        next.processes[0].name = "other.exe".to_string();
        next.processes[0].start_time = Some(1_700_000_001);
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.visible_process_count(), 2);
        assert_eq!(app.visible_process_at(0).unwrap().name, "other.exe");
        assert_eq!(app.visible_process_at(1).unwrap().name, "target.exe");
        assert!(matches!(
            app.visible_process_entries[0],
            VisibleProcessEntry::Live(_)
        ));
        assert!(matches!(
            app.visible_process_entries[1],
            VisibleProcessEntry::Ghost(_)
        ));
    }

    #[test]
    fn delete_hides_selected_ghost_row_when_processes_are_focused() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(2, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.toggle_watch_list();
        app.sampling_in_progress = true;

        let mut next = test_snapshot(1);
        next.processes[0].name = "target.exe".to_string();
        next.processes[0].start_time = Some(1_800_000_000);
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();
        app.select_process_index(1);

        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.visible_process_count(), 1);
        assert!(app.exited_tracked_rows.is_empty());
        assert!(matches!(
            app.visible_process_entries[0],
            VisibleProcessEntry::Live(_)
        ));
        assert!(app.tracked_total_visible_row().is_some());
    }

    #[test]
    fn latest_same_name_ghost_is_the_only_visible_ghost() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.sampling_in_progress = true;

        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(0),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        app.sampling_in_progress = true;
        let mut restarted = test_snapshot(1);
        restarted.processes[0].name = "target.exe".to_string();
        restarted.processes[0].pid = 42;
        restarted.processes[0].start_time = Some(1_800_000_000);
        result_tx
            .send(CollectSnapshotResult {
                snapshot: restarted,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(0),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        let ghost_count = app
            .visible_process_entries
            .iter()
            .filter(|entry| matches!(entry, VisibleProcessEntry::Ghost(_)))
            .count();
        assert_eq!(app.exited_tracked_rows.len(), 2);
        assert_eq!(ghost_count, 1);
        assert_eq!(app.visible_process_at(0).unwrap().pid, 42);
    }

    #[test]
    fn removing_tracked_name_hides_ghost_row() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.add_selected_process_to_watch_list();
        app.sampling_in_progress = true;

        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(0),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();
        app.remove_selected_process_from_watch_list();

        assert_eq!(app.visible_process_count(), 0);
        assert!(app.watch_list.is_empty());
    }

    #[test]
    fn sampling_request_is_not_sent_while_in_progress() {
        let (sampling_worker, request_rx, _result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(3, 10, sampling_worker);
        app.status = "Copied row: proc-0".to_string();

        app.request_sample().unwrap();
        assert!(app.sampling_in_progress);
        assert_eq!(request_rx.try_recv(), Ok(SampleRequest::Sample));
        assert_eq!(app.status, "Copied row: proc-0");

        app.request_sample().unwrap();
        assert_eq!(request_rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(app.status, "Copied row: proc-0");
    }

    #[test]
    fn sampling_result_updates_snapshot_and_clamps_selection() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(5, 10, sampling_worker);
        app.select_last_row();
        app.sampling_in_progress = true;
        app.status = "Selected column: Private".to_string();

        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(2),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert!(!app.sampling_in_progress);
        assert_eq!(app.snapshot.process_count, 2);
        assert_eq!(app.visible_process_count(), 2);
        assert_eq!(app.process_table_state.selected(), Some(1));
        assert_eq!(app.status, "Selected column: Private");
        assert_eq!(app.process_history.len(), 2);
    }

    #[test]
    fn successful_sample_returns_fresh_after_stale_state() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(1, 10, sampling_worker);
        app.snapshot.captured_at =
            Local::now() - chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64 + 2);
        assert!(matches!(
            app.sample_freshness(),
            Some(SampleFreshness::Stale { .. })
        ));

        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: test_snapshot(1),
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();

        assert_eq!(app.sample_freshness(), Some(SampleFreshness::Fresh));
    }

    #[test]
    fn sampling_worker_disconnect_keeps_existing_snapshot() {
        let (request_tx, _request_rx) = mpsc::channel::<SampleRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CollectSnapshotResult>();
        drop(result_tx);
        let sampling_worker = SamplingWorker {
            request_tx,
            result_rx,
            join_handle: None,
        };
        let mut app = make_test_app_with_worker(4, 10, sampling_worker);
        app.sampling_in_progress = true;

        app.poll_sample_results().unwrap();

        assert!(!app.sampling_in_progress);
        assert_eq!(app.snapshot.process_count, 4);
        assert!(app.status.contains("sampling worker stopped"));
    }

    #[test]
    fn gpu_usage_prefers_adapter_totals_and_falls_back_per_field() {
        let merged = GpuUsageSample {
            dedicated: Some(128),
            shared: None,
        }
        .merge(GpuUsageSample {
            dedicated: Some(64),
            shared: Some(32),
        });

        assert_eq!(
            merged,
            GpuUsageSample {
                dedicated: Some(128),
                shared: Some(32),
            }
        );
    }

    #[test]
    fn process_counter_instances_map_to_pids() {
        let process_ids = [
            ("chrome".to_string(), 4100),
            ("chrome#1".to_string(), 4120),
            ("_Total".to_string(), 999_999),
            ("Idle".to_string(), 0),
        ]
        .into_iter()
        .collect::<Vec<_>>();
        let handle_counts = [
            ("chrome".to_string(), 1200),
            ("chrome#1".to_string(), 800),
            ("_Total".to_string(), 2000),
        ]
        .into_iter()
        .collect::<Vec<_>>();

        let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

        assert_eq!(mapped.get(&4100), Some(&1200));
        assert_eq!(mapped.get(&4120), Some(&800));
        assert!(!mapped.contains_key(&0));
        assert_eq!(mapped.len(), 2);
    }

    #[test]
    fn process_counter_instances_skip_missing_values() {
        let process_ids = [("app".to_string(), 1234), ("app#1".to_string(), 1235)]
            .into_iter()
            .collect::<Vec<_>>();
        let handle_counts = [("app".to_string(), 77)].into_iter().collect::<Vec<_>>();

        let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

        assert_eq!(mapped.get(&1234), Some(&77));
        assert!(!mapped.contains_key(&1235));
    }

    #[test]
    fn process_counter_instances_keep_duplicate_names_by_occurrence_order() {
        let process_ids = [
            ("svchost".to_string(), 3144),
            ("svchost".to_string(), 3068),
            ("svchost".to_string(), 2568),
        ]
        .into_iter()
        .collect::<Vec<_>>();
        let handle_counts = [
            ("svchost".to_string(), 274),
            ("svchost".to_string(), 400),
            ("svchost".to_string(), 156),
        ]
        .into_iter()
        .collect::<Vec<_>>();

        let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

        assert_eq!(mapped.get(&3144), Some(&274));
        assert_eq!(mapped.get(&3068), Some(&400));
        assert_eq!(mapped.get(&2568), Some(&156));
    }

    #[test]
    fn process_counter_instances_map_double_values_to_pids() {
        let process_ids = [("app".to_string(), 1000), ("app#1".to_string(), 1001)]
            .into_iter()
            .collect::<Vec<_>>();
        let cpu_values = [("app".to_string(), 12.5), ("app#1".to_string(), 25.0)]
            .into_iter()
            .collect::<Vec<_>>();

        let mapped = map_process_counter_instances_to_pids(process_ids, cpu_values);

        assert_eq!(mapped.get(&1000), Some(&12.5));
        assert_eq!(mapped.get(&1001), Some(&25.0));
    }

    #[test]
    fn normalize_process_cpu_percent_scales_uncapped_pdh_percent_to_total_capacity() {
        assert_eq!(normalize_process_cpu_percent(100.0, 20), Some(5.0));
        assert_eq!(normalize_process_cpu_percent(400.0, 8), Some(50.0));
        assert_eq!(normalize_process_cpu_percent(2_000.0, 20), Some(100.0));
        assert_eq!(normalize_process_cpu_percent(2_500.0, 20), Some(100.0));
        assert_eq!(normalize_process_cpu_percent(-1.0, 8), None);
    }

    #[test]
    fn standby_cache_sum_uses_available_counters() {
        assert_eq!(sum_optional_values([Some(10), None, Some(25)]), Some(35));
        assert_eq!(sum_optional_values([None, None, None]), None);
    }

    #[test]
    fn working_set_flag_helpers_match_share_bits() {
        let shareable_shared = (1usize << 8) | (2usize << 5);
        let shareable_not_shared = 1usize << 8;

        assert!(working_set_page_is_shareable(shareable_shared));
        assert!(working_set_page_is_shared(shareable_shared));
        assert!(working_set_page_is_shareable(shareable_not_shared));
        assert!(!working_set_page_is_shared(shareable_not_shared));
    }

    #[test]
    fn filtered_dxgi_adapters_are_skipped() {
        assert!(is_filtered_dxgi_adapter(DXGI_ADAPTER_FLAG_SOFTWARE as u32));
        assert!(is_filtered_dxgi_adapter(DXGI_ADAPTER_FLAG_REMOTE as u32));
        assert!(!is_filtered_dxgi_adapter(0));
    }

    fn make_test_app(row_count: usize, page_size: usize) -> App {
        let (sampling_worker, _, _) = SamplingWorker::test_pair();
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        make_test_app_with_workers(
            row_count,
            page_size,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        )
    }

    fn assign_private_graph(app: &mut App) {
        let identity = app
            .selected_visible_process_identity()
            .expect("selected process identity");
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Private),
            FocusedPanel::Processes,
        ));
    }

    fn test_graph_source(app: &App, index: usize) -> GraphSlot {
        let mut row = app.snapshot.processes[0].clone();
        row.pid = 10_000 + index as u32;
        row.start_time = Some(1_800_000_000 + index as u64);
        row.name = format!("graph-{index}.exe");
        GraphSlot::process(ProcessIdentity::from_row(&row), DetailsMetric::Private)
    }

    fn add_test_graph(app: &mut App, index: usize) -> app::GraphId {
        let source = test_graph_source(app, index);
        assert!(app.add_or_reveal_graph_source(source, FocusedPanel::Processes));
        app.graph_entries.last().unwrap().id
    }

    fn track_process_name(app: &mut App, name: &str) {
        app.watch_list = vec![name.to_string()];
        app.normalized_watch_names = std::collections::HashSet::from([name.to_ascii_lowercase()]);
        app.watch_enabled = true;
        app.rebuild_visible_process_cache();
    }

    fn record_tracked_process_history_samples(app: &mut App, name: &str, count: usize) {
        let mut process = app.snapshot.processes[0].clone();
        process.name = name.to_string();
        process.pid = 42;
        process.start_time = Some(1_700_000_042);
        let tracked_names = std::collections::HashSet::from([name.to_ascii_lowercase()]);
        let now = Local.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).unwrap();
        app.process_history = ProcessHistory::default();

        for offset in 0..count {
            process.private_bytes = Some(offset as u64);
            app.process_history.record_snapshot(
                now + chrono::Duration::seconds(offset as i64),
                &[process.clone()],
                &tracked_names,
            );
        }
    }

    fn selected_process_history_sample_count(app: &App, name: &str) -> usize {
        app.process_history.sample_count_for(&ProcessIdentity {
            pid: 42,
            name: name.to_string(),
            start_time: Some(1_700_000_042),
        })
    }

    fn test_process_info(name: &str, pid: u32) -> ProcessInfo {
        ProcessInfo {
            name: name.to_string(),
            pid,
            start_time: Some(1_700_000_000 + u64::from(pid)),
            ppid: InfoValue::Value("1".to_string()),
            parent_process: InfoValue::Value("parent.exe / PID 1".to_string()),
            arch: InfoValue::Value("x64".to_string()),
            user: InfoValue::Value("test-user".to_string()),
            executable: InfoValue::Value(format!("C:/test/{name}")),
            command_line: InfoValue::Value(name.to_string()),
            file_modified: InfoValue::Value("2026-05-06 00:00:00".to_string()),
            file_size: InfoValue::Value("1,024".to_string()),
            company_name: InfoValue::Value("Test Company".to_string()),
            product_name: InfoValue::Value("Test Product".to_string()),
            product_version: InfoValue::Value("1.0.0".to_string()),
            file_version: InfoValue::Value("1.0.0.1".to_string()),
            workset_bytes: InfoValue::Value("1,024".to_string()),
            workset_private_bytes: InfoValue::Value("512".to_string()),
            ws_shareable_bytes: InfoValue::Value("256".to_string()),
            ws_shared_bytes: InfoValue::Value("128".to_string()),
        }
    }

    fn show_process_info_files_tab(app: &mut App) {
        app.open_selected_process_info_dialog().unwrap();
        app.process_info_tab = app::ProcessInfoTab::Files;
    }

    fn test_open_files_report(name: &str, pid: u32, file_name: &str) -> OpenFilesReport {
        OpenFilesReport {
            pid,
            process_name: name.to_string(),
            total_handles: 1,
            file_handles: 1,
            inaccessible_handles: 0,
            unnamed_file_handles: 0,
            entries: vec![OpenFileEntry {
                path: format!(r"C:\tmp\{file_name}"),
                handle_count: 1,
            }],
            error: None,
        }
    }

    fn test_process_module_entry(file_name: &str, company: &str) -> ProcessModuleEntry {
        ProcessModuleEntry {
            path: format!(r"C:\Program Files\Test\{file_name}"),
            dll_name: file_name.to_string(),
            directory: r"C:\Program Files\Test".to_string(),
            company_name: InfoValue::Value(company.to_string()),
            product_version: InfoValue::Value("2.0.0".to_string()),
            file_version: InfoValue::Value("2.0.0.1".to_string()),
            modified: InfoValue::Value("2026-08-04 12:34:56".to_string()),
        }
    }

    fn test_process_modules_report(
        name: &str,
        pid: u32,
        entries: Vec<ProcessModuleEntry>,
    ) -> ProcessModulesReport {
        ProcessModulesReport {
            pid,
            process_name: name.to_string(),
            captured_at: Local::now(),
            entries,
        }
    }

    fn activate_process_modules_tab(app: &mut App) {
        app.activate_process_info_tab(app::ProcessInfoTab::Dlls)
            .unwrap();
    }

    fn test_process_environment_report(
        name: &str,
        pid: u32,
        entries: Vec<ProcessEnvironmentEntry>,
    ) -> ProcessEnvironmentReport {
        ProcessEnvironmentReport {
            pid,
            process_name: name.to_string(),
            captured_at: Local::now(),
            entries,
            malformed_entries: 0,
        }
    }

    fn activate_process_environment_tab(app: &mut App) {
        app.activate_process_info_tab(app::ProcessInfoTab::Environment)
            .unwrap();
    }

    fn unique_recording_path(label: &str) -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "winproc-tui-test-{label}-{}.log",
                std::process::id()
            ))
    }

    fn unique_config_path(label: &str) -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "winproc-tui-test-{label}-{}.toml",
                std::process::id()
            ))
    }

    fn unique_recording_dir(label: &str) -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("winproc-tui-test-{label}-{}", std::process::id()))
    }

    fn render_app_to_text(app: &App, width: u16, height: u16) -> String {
        buffer_to_text(&render_app_to_buffer(app, width, height))
    }

    fn render_app_to_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, app))
            .expect("test render should succeed");
        terminal.backend().buffer().clone()
    }

    fn left_click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn mouse_move(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn assert_modal_focus_border(app: &App, popup_percent_x: u16, popup_percent_y: u16) {
        let screen = Rect::new(0, 0, 100, 45);
        let popup = centered_rect(popup_percent_x, popup_percent_y, screen);
        assert_modal_rect_focus_border(app, popup);
    }

    fn assert_modal_rect_focus_border(app: &App, popup: Rect) {
        let screen = Rect::new(0, 0, 100, 45);
        let buffer = render_app_to_buffer(app, screen.width, screen.height);
        let process_table = main_panel_areas_for_app(screen, app).processes.area;
        let theme = app.theme();

        assert_eq!(
            buffer[(popup.x, popup.y)].fg,
            theme.focus_border,
            "modal border should use the high-contrast neutral focus color"
        );
        assert_eq!(
            buffer[(process_table.x, process_table.y)].fg,
            theme.border,
            "underlying process table should not stay focused while a modal is open"
        );
    }

    #[test]
    fn ctrl_l_opens_log_list() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.show_log_list);
        assert!(app.log_list_worker.is_some());
        assert_eq!(app.log_list_dir, Some(std::env::current_dir().unwrap()));
    }

    #[test]
    fn log_list_renders_session_rows() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_list_dir = Some(std::path::PathBuf::from("C:/logs"));
        let started_at = Local.with_ymd_and_hms(2026, 5, 14, 7, 43, 22).unwrap();
        let ended_at = Local.with_ymd_and_hms(2026, 5, 14, 7, 45, 27).unwrap();
        app.log_summaries = vec![app::logs::LogSummary {
            path: std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"),
            schema_version: Some(2),
            session_id: Some("demo".to_string()),
            started_at: Some(started_at),
            ended_at: Some(ended_at),
            host: Some("PC".to_string()),
            tracked_names: vec!["app.exe".to_string()],
            frame_count: 12,
            error: None,
        }];

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(!rendered.contains("Log sessions"), "{rendered}");
        assert!(rendered.contains("Dir C:/logs"), "{rendered}");
        assert!(rendered.contains("d change dir"), "{rendered}");
        assert!(rendered.contains("00:02:05"), "{rendered}");
        assert!(!rendered.contains("app.exe"), "{rendered}");
        assert!(rendered.contains("winproc-tui-demo.log"), "{rendered}");
        assert!(
            !rendered.contains("C:/logs/winproc-tui-demo.log"),
            "{rendered}"
        );
        assert!(rendered.contains("[ Open ]"), "{rendered}");
        assert!(rendered.contains("[ Directory ]"), "{rendered}");
        assert!(rendered.contains("[ Refresh ]"), "{rendered}");
        assert!(rendered.contains("[ Close ]"), "{rendered}");
    }

    #[test]
    fn logs_dialog_matches_recording_dialog_width() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        let logs = render_app_to_buffer(&app, screen.width, screen.height);
        let (logs_x, logs_y) = find_text_position(&logs, "LOGS").expect("Logs title should render");

        app.show_log_list = false;
        app.show_recording_path_dialog = true;
        let recording = render_app_to_buffer(&app, screen.width, screen.height);
        let (recording_x, recording_y) =
            find_text_position(&recording, "RECORDING").expect("Recording title should render");

        assert_eq!(logs_x, recording_x);
        assert_eq!(logs[(logs_x - 1, logs_y)].symbol(), "┏");
        assert_eq!(recording[(recording_x - 1, recording_y)].symbol(), "┏");
        assert_eq!(logs[(logs_x + 76, logs_y)].symbol(), "┓");
        assert_eq!(recording[(recording_x + 76, recording_y)].symbol(), "┓");
    }

    #[test]
    fn log_list_tab_cycles_list_and_buttons() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.log_list_focus, app::LogListFocus::Open);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.log_list_focus, app::LogListFocus::Directory);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .unwrap();
        assert_eq!(app.log_list_focus, app::LogListFocus::Open);
    }

    #[test]
    fn log_list_buttons_activate_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_list_dir = Some(std::env::current_dir().unwrap());
        app.log_summaries = vec![app::logs::LogSummary {
            path: std::path::PathBuf::from("missing.log"),
            schema_version: Some(2),
            session_id: None,
            started_at: Some(Local::now()),
            ended_at: None,
            host: None,
            tracked_names: Vec::new(),
            frame_count: 0,
            error: None,
        }];

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (open_x, open_y) = find_text_position(&buffer, "[ Open ]").unwrap();
        app.on_mouse(left_click(open_x + 2, open_y), screen);
        assert!(app.log_load_worker.is_some());
        app.log_load_worker = None;

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (directory_x, directory_y) = find_text_position(&buffer, "[ Directory ]").unwrap();
        app.on_mouse(left_click(directory_x + 2, directory_y), screen);
        assert!(app.show_log_dir_dialog);
        app.cancel_log_dir_dialog();

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (refresh_x, refresh_y) = find_text_position(&buffer, "[ Refresh ]").unwrap();
        app.on_mouse(left_click(refresh_x + 2, refresh_y), screen);
        assert!(app.log_list_worker.is_some());
        app.log_list_worker = None;

        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (close_x, close_y) = find_text_position(&buffer, "[ Close ]").unwrap();
        app.on_mouse(left_click(close_x + 2, close_y), screen);
        assert!(!app.show_log_list);
    }

    #[test]
    fn ctrl_l_uses_previous_recording_dir_as_default_log_dir() {
        let dir = unique_recording_dir("log-default");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = make_test_app(1, 10);
        app.recording_last_dir = Some(dir.clone());

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.log_list_dir, Some(dir.clone()));
        assert_eq!(app.recording_last_dir, Some(dir.clone()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn log_dir_dialog_changes_active_dir_without_recording_last_dir() {
        let recording_dir = unique_recording_dir("log-recording");
        let selected_dir = unique_recording_dir("log-selected");
        std::fs::create_dir_all(&recording_dir).unwrap();
        std::fs::create_dir_all(&selected_dir).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.recording_last_dir = Some(recording_dir.clone());
        app.log_list_dir = Some(recording_dir.clone());

        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_log_dir_dialog);
        app.log_dir_draft = selected_dir.display().to_string();
        app.log_dir_cursor = app.log_dir_draft.len();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_log_dir_dialog);
        assert_eq!(app.log_list_dir, Some(selected_dir.clone()));
        assert_eq!(app.recording_last_dir, Some(recording_dir.clone()));
        assert!(app.log_list_worker.is_some());
        let _ = std::fs::remove_dir_all(recording_dir);
        let _ = std::fs::remove_dir_all(selected_dir);
    }

    #[test]
    fn log_dir_dialog_scans_selected_directory() {
        let selected_dir = unique_recording_dir("log-scan-selected");
        std::fs::create_dir_all(&selected_dir).unwrap();
        let log_path = selected_dir.join("chosen.log");
        std::fs::write(
            &log_path,
            r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["chosen.exe"]}"#,
        )
        .unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_list_dir = Some(std::env::current_dir().unwrap());
        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = selected_dir.display().to_string();
        app.log_dir_cursor = app.log_dir_draft.len();

        app.confirm_log_dir().unwrap();
        for _ in 0..100 {
            if app.poll_log_workers() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert_eq!(app.log_summaries.len(), 1);
        assert_eq!(app.log_summaries[0].path, log_path);
        let _ = std::fs::remove_dir_all(selected_dir);
    }

    #[test]
    fn log_dir_dialog_rejects_missing_directory() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_list_dir = Some(std::env::current_dir().unwrap());

        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = unique_recording_dir("missing-log-dir")
            .display()
            .to_string();
        app.log_dir_cursor = app.log_dir_draft.len();
        app.confirm_log_dir().unwrap();

        assert!(app.show_log_dir_dialog);
        assert_eq!(
            app.log_dir_error.as_deref(),
            Some("Directory does not exist.")
        );
        assert!(app.status.starts_with("Log directory does not exist:"));
        assert!(app.log_list_worker.is_none());
        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("Directory does not exist."), "{rendered}");
    }

    #[test]
    fn log_dir_dialog_rejects_empty_directory() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;

        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft.clear();
        app.log_dir_cursor = 0;
        app.confirm_log_dir().unwrap();

        assert!(app.show_log_dir_dialog);
        assert_eq!(app.log_dir_error.as_deref(), Some("Directory is empty."));
        assert!(app.log_list_worker.is_none());
    }

    #[test]
    fn log_dir_dialog_rejects_file_path() {
        let path = unique_recording_dir("log-dir-file");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not a directory").unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;

        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = path.display().to_string();
        app.log_dir_cursor = app.log_dir_draft.len();
        app.confirm_log_dir().unwrap();

        assert!(app.show_log_dir_dialog);
        assert_eq!(
            app.log_dir_error.as_deref(),
            Some("Path is not a directory.")
        );
        assert!(app.log_list_worker.is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn log_dir_dialog_shows_shortcuts_above_directory_input() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.open_log_dir_dialog().unwrap();
        let buffer = render_app_to_buffer(&app, 120, 45);
        let (_, shortcut_y) =
            find_text_position(&buffer, "Tab focus").expect("focus shortcut should render");
        assert!(find_text_position(&buffer, "Ctrl+Space complete").is_some());
        let (_, label_y) =
            find_text_position(&buffer, "Directory").expect("directory label should render");

        assert!(shortcut_y < label_y);
    }

    #[test]
    fn ctrl_space_completes_log_dir_dialog_directory() {
        let root = unique_recording_dir("log-dir-complete");
        let target = root.join("alpha");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
        app.log_dir_cursor = app.log_dir_draft.len();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
            .unwrap();

        let expected = format!(
            "{}{}alpha{}",
            root.display(),
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        assert_eq!(app.log_dir_draft, expected);
        assert_eq!(app.log_dir_cursor, app.log_dir_draft.len());
        assert_eq!(app.status, "Completed directory");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn log_dir_backspace_handles_key_repeat_and_ignores_release() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = "C:/logs/example".to_string();
        app.log_dir_cursor = app.log_dir_draft.len();

        app.on_key(KeyEvent::new_with_kind(
            KeyCode::Backspace,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ))
        .unwrap();
        app.on_key(KeyEvent::new_with_kind(
            KeyCode::Backspace,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ))
        .unwrap();

        assert_eq!(app.log_dir_draft, "C:/logs/exampl");
        assert_eq!(app.log_dir_cursor, app.log_dir_draft.len());
    }

    #[test]
    fn log_list_refresh_uses_active_manual_dir() {
        let recording_dir = unique_recording_dir("log-refresh-recording");
        let selected_dir = unique_recording_dir("log-refresh-selected");
        std::fs::create_dir_all(&recording_dir).unwrap();
        std::fs::create_dir_all(&selected_dir).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.recording_last_dir = Some(recording_dir.clone());
        app.log_list_dir = Some(selected_dir.clone());

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.log_list_dir, Some(selected_dir.clone()));
        assert_eq!(app.recording_last_dir, Some(recording_dir.clone()));
        assert!(app.status.contains(&selected_dir.display().to_string()));
        let _ = std::fs::remove_dir_all(recording_dir);
        let _ = std::fs::remove_dir_all(selected_dir);
    }

    #[test]
    fn log_dir_cancel_button_closes_dialog() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_list_dir = Some(std::env::current_dir().unwrap());
        app.open_log_dir_dialog().unwrap();
        let screen = Rect::new(0, 0, 120, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, " Cancel ").expect("cancel button should render");

        app.on_mouse(left_click(x, y), screen);

        assert!(!app.show_log_dir_dialog);
    }

    #[test]
    fn log_list_click_selects_row() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_summaries = vec![
            app::logs::LogSummary {
                path: std::path::PathBuf::from("C:/logs/first.log"),
                schema_version: Some(2),
                session_id: None,
                started_at: Some(Local::now()),
                ended_at: None,
                host: None,
                tracked_names: vec!["first.exe".to_string()],
                frame_count: 0,
                error: None,
            },
            app::logs::LogSummary {
                path: std::path::PathBuf::from("C:/logs/second.log"),
                schema_version: Some(2),
                session_id: None,
                started_at: Some(Local::now()),
                ended_at: None,
                host: None,
                tracked_names: vec!["second.exe".to_string()],
                frame_count: 0,
                error: None,
            },
        ];
        app.log_list_index = 0;
        let screen = Rect::new(0, 0, 140, 45);
        app.set_log_list_page_size(ui::log_list_page_size_for_screen(screen));
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) =
            find_text_position(&buffer, "second.log").expect("second log row should be rendered");

        app.on_mouse(left_click(x, y), screen);

        assert_eq!(app.log_list_index, 1);
        assert!(app.log_load_worker.is_none());
    }

    #[test]
    fn log_list_double_click_opens_row() {
        let path = std::env::temp_dir().join(format!(
            "winproc-tui-log-double-click-test-{}-{}.log",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(
            &path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.log_summaries = vec![app::logs::LogSummary {
            path: path.clone(),
            schema_version: Some(2),
            session_id: Some("s1".to_string()),
            started_at: Some(Local::now()),
            ended_at: None,
            host: Some("PC".to_string()),
            tracked_names: vec!["app.exe".to_string()],
            frame_count: 0,
            error: None,
        }];
        let screen = Rect::new(0, 0, 180, 45);
        app.set_log_list_page_size(ui::log_list_page_size_for_screen(screen));
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "> v2").expect("log row should be rendered");

        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);

        assert!(app.log_load_worker.is_some());
        assert!(app.status.starts_with("Opening log:"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn log_view_header_shows_log_badge_and_path_without_freshness() {
        let mut app = make_test_app(1, 10);
        app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        let rendered = render_app_to_text(&app, 100, 20);
        let buffer = render_app_to_buffer(&app, 100, 20);
        let (_, log_y) = find_text_position(&buffer, "LOG").expect("log badge should be rendered");

        assert!(rendered.contains("LOG"), "{rendered}");
        assert_eq!(log_y, 0);
        assert!(!rendered.contains("fresh"), "{rendered}");
        assert!(!rendered.contains("STALE"), "{rendered}");
        assert!(rendered.contains("winproc-tui-demo.log"), "{rendered}");
        assert!(
            rendered.contains(&format!("winproc-tui {}", env!("CARGO_PKG_VERSION"))),
            "{rendered}"
        );
    }

    #[test]
    fn log_view_header_keeps_the_path_and_hides_product_at_narrow_width() {
        let mut app = make_test_app(1, 10);
        let path = "C:/logs/winproc-tui-demo.log";
        let product_and_version = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));
        app.log_view_path = Some(std::path::PathBuf::from(path));

        let rendered = render_app_to_text(&app, 40, 20);

        assert!(rendered.contains("LOG"), "{rendered}");
        assert!(rendered.contains(path), "{rendered}");
        assert!(!rendered.contains(&product_and_version), "{rendered}");
    }

    #[test]
    fn display_pause_is_unavailable_in_log_view() {
        let mut app = make_test_app(1, 10);
        app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        let rendered = render_app_to_text(&app, 240, 30);
        assert!(!rendered.contains("Ctrl+P Pause"), "{rendered}");
        assert!(rendered.contains("Esc Live"), "{rendered}");

        app.toggle_display_pause();

        assert!(!app.is_display_paused());
        assert_eq!(app.status, "Display pause is unavailable in Log view");
    }

    #[test]
    fn log_view_esc_returns_to_live_without_quit_confirmation() {
        let mut app = make_test_app(1, 10);
        app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.activity(), AppActivity::Live);
        assert!(app.log_view_path.is_none());
        assert!(!app.show_quit_confirmation);
        assert_eq!(app.status, "Log view closed");
    }

    #[test]
    fn ctrl_r_is_rejected_in_log_view() {
        let mut app = make_test_app(1, 10);
        app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.activity(), AppActivity::LogView);
        assert_eq!(app.status, "Recording is unavailable in Log view");
    }

    #[test]
    fn ctrl_l_is_rejected_during_recording() {
        let path = unique_recording_path("deny-log-view");
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;
        app.confirm_recording_path().unwrap();

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.activity(), AppActivity::Recording);
        assert!(!app.show_log_list);
        assert_eq!(app.status, "Log view is unavailable during recording");

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loaded_log_is_ignored_if_recording_started_before_worker_returns() {
        let log_view_path = std::env::temp_dir().join(format!(
            "winproc-tui-log-view-race-test-{}-{}.log",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(
            &log_view_path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let loaded = app::logs::load_log(&log_view_path, SortSpec::default()).unwrap();
        let recording_path = unique_recording_path("deny-loaded-log-view");
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.recording_path_draft = recording_path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;
        app.confirm_recording_path().unwrap();

        app.apply_loaded_log(loaded);

        assert_eq!(app.activity(), AppActivity::Recording);
        assert!(app.log_view_path.is_none());
        assert_eq!(app.status, "Log view is unavailable during recording");

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(recording_path);
        let _ = std::fs::remove_file(log_view_path);
    }

    #[test]
    fn loaded_log_feeds_graph_samples_without_turning_missing_values_to_zero() {
        let path = std::env::temp_dir().join(format!(
            "winproc-tui-log-view-test-{}-{}.log",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(
            &path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":100,"total_memory_bytes":1000},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":null}}]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:13+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":200,"total_memory_bytes":1000},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let loaded = app::logs::load_log(&path, SortSpec::default()).unwrap();
        let mut app = make_test_app(1, 10);

        app.apply_loaded_log(loaded);
        let identity = app.visible_process_identity_at(0).unwrap();
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.focused_panel = FocusedPanel::DetailsSamples;
        let samples = app.graph_slot_samples(app.graph_slot(0).unwrap());

        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].value, None);
        assert_eq!(samples[1].value, Some(1024.0));

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(
            rendered.contains("Slot#1 · Private · app.exe"),
            "{rendered}"
        );
        assert!(rendered.contains("A/B Time      Private"), "{rendered}");
        assert!(rendered.contains("1,024"), "{rendered}");
    }

    #[test]
    fn recording_writes_v2_session_frame_and_end_records() {
        let path = unique_recording_path("v2-session");
        let mut app = make_test_app(1, 10);
        app.watch_list = vec!["proc-0".to_string()];
        app.normalized_watch_names = std::collections::HashSet::from(["proc-0".to_string()]);
        app.recording_path_draft = path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;

        app.confirm_recording_path().unwrap();
        app.stop_recording().unwrap();

        let lines = std::fs::read_to_string(&path).unwrap();
        let records = lines
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records[0]["schema_version"], 2);
        assert_eq!(records[0]["record_type"], "session");
        assert_eq!(records[1]["record_type"], "frame");
        assert_eq!(records[1]["system_metrics"]["physical_memory_bytes"], 0);
        assert_eq!(records[2]["record_type"], "end");
    }

    fn buffer_to_text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer
            .content()
            .chunks(buffer.area().width as usize)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_dialog_title_style(buffer: &ratatui::buffer::Buffer, title: &str, theme: ui::Theme) {
        assert_title_style(buffer, title, theme.focus_border);
    }

    fn assert_title_style(
        buffer: &ratatui::buffer::Buffer,
        title: &str,
        expected_color: ratatui::style::Color,
    ) {
        let (x, y) = find_text_position(buffer, title)
            .unwrap_or_else(|| panic!("dialog title should render: {title}"));
        let cell = &buffer[(x, y)];
        assert_eq!(cell.fg, expected_color, "dialog title: {title}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "dialog title should be bold: {title}"
        );
    }

    fn find_text_position(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
        let width = buffer.area().width;
        let height = buffer.area().height;
        for y in 0..height {
            let row = (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(x) = row.find(needle) {
                return Some((row[..x].chars().count() as u16, y));
            }
        }
        None
    }

    fn find_text_position_in_area(
        buffer: &ratatui::buffer::Buffer,
        area: Rect,
        needle: &str,
    ) -> Option<(u16, u16)> {
        let right = area.right().min(buffer.area().right());
        let bottom = area.bottom().min(buffer.area().bottom());
        for y in area.y..bottom {
            let row = (area.x..right)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(x) = row.find(needle) {
                return Some((area.x + row[..x].chars().count() as u16, y));
            }
        }
        None
    }

    fn area_contains_foreground(
        buffer: &ratatui::buffer::Buffer,
        area: Rect,
        foreground: ratatui::style::Color,
    ) -> bool {
        let right = area.right().min(buffer.area().right());
        let bottom = area.bottom().min(buffer.area().bottom());
        (area.y..bottom).any(|y| (area.x..right).any(|x| buffer[(x, y)].fg == foreground))
    }

    fn find_styled_symbol_positions_in_area(
        buffer: &ratatui::buffer::Buffer,
        area: Rect,
        symbol: &str,
        fg: ratatui::style::Color,
    ) -> Vec<(u16, u16)> {
        let right = area.right().min(buffer.area().right());
        let bottom = area.bottom().min(buffer.area().bottom());
        let mut positions = Vec::new();
        for y in area.y..bottom {
            for x in area.x..right {
                let cell = &buffer[(x, y)];
                if cell.symbol() == symbol && cell.fg == fg {
                    positions.push((x, y));
                }
            }
        }
        positions
    }

    fn find_symbol_position(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
        let width = buffer.area().width;
        let height = buffer.area().height;
        for y in 0..height {
            for x in 0..width {
                if buffer[(x, y)].symbol() == needle {
                    return Some((x, y));
                }
            }
        }
        None
    }

    fn make_test_app_with_worker(
        row_count: usize,
        page_size: usize,
        sampling_worker: SamplingWorker,
    ) -> App {
        let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
        let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
        make_test_app_with_workers(
            row_count,
            page_size,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        )
    }

    fn make_test_app_with_workers(
        row_count: usize,
        page_size: usize,
        sampling_worker: SamplingWorker,
        process_info_worker: ProcessInfoWorker,
        open_files_worker: OpenFilesWorker,
    ) -> App {
        let process_modules_worker = ProcessModulesWorker::test_noop();
        let process_environment_worker = ProcessEnvironmentWorker::test_noop();
        let mut table_state = TableState::default();
        if row_count > 0 {
            table_state.select(Some(0));
        }

        let snapshot = test_snapshot(row_count);
        let selected_process_identity = table_state
            .selected()
            .and_then(|index| snapshot.processes.get(index))
            .map(model::ProcessIdentity::from_row);

        App {
            runtime: RuntimeConfig {
                mouse: true,
                config_path: None,
                recording_last_dir: None,
                initial_theme: "Dark".to_string(),
                initial_graph_slot_layout: GraphSlotLayout::Auto,
                initial_show_samples_panel: true,
                initial_show_sample_delta: true,
                column_preset: ColumnPreset::Default,
                process_columns: vec![
                    MetricColumn::PrivateBytes,
                    MetricColumn::WorksetPrivateBytes,
                ],
                process_column_widths: ProcessColumnWidths::default(),
                sort: SortSpec::default(),
                initial_tracked_only: false,
                process_filters: Vec::new(),
                tracked_list_startup: config::TrackedListStartup::ResumeLast,
                active_tracked_list: None,
                saved_tracked_lists: Vec::new(),
                sampling_options: samplers::SamplingOptions::default(),
            },
            sampling_worker,
            process_info_worker,
            open_files_worker,
            process_modules_worker,
            process_environment_worker,
            sampling_in_progress: false,
            snapshot,
            process_table_state: table_state,
            process_page_size: page_size,
            selected_process_identity,
            process_selection_anchor: None,
            selected_process_identities: std::collections::HashSet::new(),
            selected_process_column_index: 2,
            process_metric_column_offset: 0,
            process_order_hold_until: None,
            show_help: false,
            help_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            show_column_picker: false,
            tracked_lists_dialog: None,
            show_quit_confirmation: false,
            quit_confirm_selection: QuitConfirmSelection::Cancel,
            show_recording_no_tracked_warning: false,
            show_recording_path_dialog: false,
            recording_path_draft: String::new(),
            recording_path_cursor: 0,
            recording_path_completion: app::path_completion::PathCompletionState::default(),
            recording_path_selection: app::RecordingPathSelection::Path,
            show_recording_overwrite_confirmation: false,
            recording_overwrite_selection: app::RecordingOverwriteSelection::Cancel,
            show_tracked_remove_confirmation: false,
            tracked_remove_selection: TrackedRemoveSelection::Cancel,
            tracked_remove_name: String::new(),
            tracked_remove_total_samples: 0,
            tracked_remove_discarded_samples: 0,
            show_process_kill_confirmation: false,
            process_kill_selection: app::ProcessKillSelection::Cancel,
            process_kill_targets: Vec::new(),
            show_display_area_warning: false,
            show_metric_column_warning: false,
            show_no_graph_metrics_warning: false,
            recording_session: None,
            recording_last_dir: None,
            recording_spinner_index: 0,
            log_view_path: None,
            should_quit: false,
            column_picker_index: 0,
            column_picker_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            show_log_list: false,
            log_list_focus: app::LogListFocus::List,
            log_list_index: 0,
            log_list_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            show_log_dir_dialog: false,
            log_dir_draft: String::new(),
            log_dir_cursor: 0,
            log_dir_completion: app::path_completion::PathCompletionState::default(),
            log_dir_selection: app::LogDirSelection::Path,
            log_dir_error: None,
            open_files_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
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
            process_info_tab: app::ProcessInfoTab::Metrics,
            process_info_focus: app::ProcessInfoFocus::Content,
            process_info_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            process_info_image_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            process_info_dlls_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            process_info_environment_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
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
            log_view_normalized_watch_names: std::collections::HashSet::new(),
            focused_panel: FocusedPanel::Processes,
            show_details: false,
            graph_entries: Vec::new(),
            active_graph_id: None,
            next_graph_id: 0,
            graph_scroll_row: 0,
            graph_scrollbar_dragging: false,
            graph_scrollbar_grab_offset: 0,
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
            graph_slot_layout: GraphSlotLayout::Auto,
            show_samples_panel: true,
            samples_temporarily_collapsed: false,
            show_sample_delta: true,
            details_live: true,
            column_preset: ColumnPreset::Default,
            process_columns: vec![
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
            ],
            process_column_widths: ProcessColumnWidths::default(),
            sort: SortSpec::default(),
            paused_display: None,
            log_view_display: None,
            filter_text: String::new(),
            filter_draft: String::new(),
            filter_editing: false,
            jump_draft: String::new(),
            jump_editing: false,
            watch_list: Vec::new(),
            normalized_watch_names: std::collections::HashSet::new(),
            watch_enabled: false,
            visible_process_entries: (0..row_count).map(VisibleProcessEntry::Live).collect(),
            tracked_total_row: None,
            exited_tracked_rows: std::collections::HashMap::new(),
            last_tracked_live_identities: std::collections::HashSet::new(),
            process_history: ProcessHistory::default(),
            system_history: SystemHistory::default(),
            ram_vram_selected_index: 0,
            system_activity_selected_index: 0,
            process_info_cache: std::collections::HashMap::new(),
            process_info_display_identity: None,
            pending_process_info: None,
            process_info_in_flight: None,
            process_info_in_flight_generation: None,
            ab_comparison: None,
            last_screen_area: ratatui::layout::Rect::new(0, 0, 100, 45),
            theme_index: 0,
            status: String::new(),
        }
    }

    fn test_snapshot(row_count: usize) -> Snapshot {
        let processes = (0..row_count)
            .map(|index| ProcessRow {
                pid: index as u32,
                name: format!("proc-{index}"),
                executable_path: None,
                start_time: Some(1_700_000_000 + index as u64),
                cpu_percent: None,
                private_bytes: Some(index as u64),
                workset_bytes: Some(index as u64),
                workset_private_bytes: None,
                workset_shareable_bytes: None,
                workset_shared_bytes: None,
                thread_count: None,
                handle_count: None,
                user_object_count: None,
                gdi_object_count: None,
                gpu_percent: None,
                gpu_dedicated_bytes: None,
                gpu_shared_bytes: None,
                dotnet_heap_bytes: None,
                io_read_bytes_per_sec: None,
                io_write_bytes_per_sec: None,
            })
            .collect::<Vec<_>>();

        Snapshot {
            captured_at: Local::now(),
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
            cpu_total_usage_percent: None,
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_name: None,
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            process_count: row_count,
            processes,
        }
    }
}
