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
mod ui;

pub(crate) use app::App;
use app::run_tui;
#[cfg(test)]
use app::{
    AppActivity, DetailsMetric, DetailsTarget, FocusedPanel, GraphSlot, InfoPanelMode,
    PROCESS_INFO_DEBOUNCE, QuitConfirmSelection, SettingsSelection, TrackedRemoveSelection,
    VisibleProcessEntry,
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
    ProcessIdentity, ProcessInfo, ProcessRow, SortColumn, SortDirection, SortSpec,
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
    OpenFileEntry, OpenFilesReport, OpenFilesRequest, OpenFilesResult, OpenFilesWorker,
};
#[cfg(test)]
use samplers::pdh::map_process_counter_instances_to_pids;
#[cfg(test)]
use samplers::pdh::{normalize_process_cpu_percent, sum_optional_values};
#[cfg(test)]
use samplers::process::{working_set_page_is_shareable, working_set_page_is_shared};
#[cfg(test)]
use samplers::process_info::{ProcessInfoRequest, ProcessInfoResult, ProcessInfoWorker};
#[cfg(test)]
use samplers::{CollectSnapshotResult, SamplingWorker};
#[cfg(test)]
use std::sync::mpsc::{self, TryRecvError};
#[cfg(test)]
use ui::layout::centered_rect;
#[cfg(test)]
use ui::{
    GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, THEMES,
    details_graph_area_for_screen, details_samples_area_for_screen, details_slot_areas_for_screen,
    process_kill_button_at, process_kill_dialog_area, process_table_area_for_screen,
    process_table_page_size, process_table_visible_column_count,
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
        let config = load_config(&config_path)?;
        let runtime = build_runtime_config(config)?;
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
    fn cli_rejects_export_dir_option() {
        let error = Cli::try_parse_from(["winproc-tui", "--export-dir", "C:/logs"]).unwrap_err();

        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn sampling_interval_is_fixed_to_one_second() {
        let mut config = AppConfig::default();
        config.general.interval_seconds = 30;

        let runtime = build_runtime_config(config).unwrap();

        assert_eq!(runtime.interval_seconds, 1);
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
        let (
            available,
            committed,
            limit,
            cache,
            standby,
            disk_read,
            disk_write,
            net_recv,
            net_sent,
            warning,
        ) = map_memory_counters(
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
                network_received_bytes_per_sec: Some(5_000),
                network_sent_bytes_per_sec: Some(6_000),
                cpu_frequencies_mhz: Vec::new(),
            })),
        );

        assert_eq!(available, 10_000);
        assert_eq!(committed, Some(9_000));
        assert_eq!(limit, Some(24_000));
        assert_eq!(cache, Some(1_000));
        assert_eq!(standby, Some(2_000));
        assert_eq!(disk_read, Some(3_000));
        assert_eq!(disk_write, Some(4_000));
        assert_eq!(net_recv, Some(5_000));
        assert_eq!(net_sent, Some(6_000));
        assert_eq!(warning, None);
    }

    #[test]
    fn map_memory_counters_drops_commit_fields_on_failure() {
        let (
            available,
            committed,
            limit,
            cache,
            standby,
            disk_read,
            disk_write,
            net_recv,
            net_sent,
            warning,
        ) = map_memory_counters(32_000, 12_000, Err(anyhow::anyhow!("pdh failed")));

        assert_eq!(available, 12_000);
        assert_eq!(committed, None);
        assert_eq!(limit, None);
        assert_eq!(cache, None);
        assert_eq!(standby, None);
        assert_eq!(disk_read, None);
        assert_eq!(disk_write, None);
        assert_eq!(net_recv, None);
        assert_eq!(net_sent, None);
        assert!(warning.unwrap().contains("commit counters unavailable"));
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
    fn system_history_tracks_real_commit_samples_only() {
        assert_eq!(process_table_page_size(Rect::new(0, 0, 80, 13)), 10);
        assert_eq!(
            process_table_page_size(process_table_area_for_screen(
                Rect::new(0, 0, 120, 40),
                false,
            )),
            23
        );
        assert_eq!(
            process_table_page_size(process_table_area_for_screen(
                Rect::new(0, 0, 120, 60),
                true,
            )),
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
        let page_size = process_table_page_size(process_table_area_for_screen(screen, false));

        for row_count in [120usize, 1_000usize] {
            let mut app = make_test_app(row_count, page_size);
            app.focused_panel = FocusedPanel::Processes;
            app.set_screen_area(screen);
            app.previous_snapshot = Some(app.snapshot.clone());
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
            app.previous_snapshot = Some(app.snapshot.clone());
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
    fn filter_editing_space_tracks_selected_process_without_editing_draft() {
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
        assert_eq!(app.filter_draft, "b");
        assert_eq!(app.watch_list, vec!["beta.exe"]);
        assert_eq!(app.status, "Added to Tracked List: beta.exe");
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
        assert_eq!(app.info_panel_mode, InfoPanelMode::System);
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
    fn details_toggle_changes_visibility() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_details);
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
            rendered.contains("Select a metric, then press 1-4 to show it in Graph#1-#4."),
            "{rendered}"
        );

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_no_graph_metrics_warning);
    }

    #[test]
    fn number_keys_assign_replace_and_clear_graph_slots() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.graph_slots[0]
                .as_ref()
                .and_then(GraphSlot::process_metric),
            Some(DetailsMetric::Private)
        );
        assert!(app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.graph_slots[0]
                .as_ref()
                .and_then(GraphSlot::process_metric),
            Some(DetailsMetric::WorksetPrivate)
        );

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.graph_slots[0].is_none());
        assert!(!app.show_details);
    }

    #[test]
    fn zero_key_clears_graph_slots_and_closes_graphs_in_processes_panel() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots[0].is_some());
        assert!(app.graph_slots[1].is_some());
        assert!(app.show_details);

        app.on_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots.iter().all(Option::is_none));
        assert!(!app.show_details);
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        assert_eq!(app.status, "Graph metrics cleared");
    }

    #[test]
    fn number_keys_move_same_graph_metric_between_slots() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots[0].is_none());
        assert_eq!(
            app.graph_slots[1]
                .as_ref()
                .and_then(GraphSlot::process_metric),
            Some(DetailsMetric::Private)
        );
        assert_eq!(app.active_graph_slot_index, 1);
        assert_eq!(app.status, "Graph metric moved to Graph#2");

        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.graph_slots[1]
                .as_ref()
                .and_then(GraphSlot::process_metric),
            Some(DetailsMetric::WorksetPrivate)
        );
    }

    #[test]
    fn delete_on_live_process_opens_kill_confirm_before_graph_clear() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 80));

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_process_kill_confirmation);
        assert!(app.graph_slots[0].is_some());
        assert_eq!(app.process_kill_targets.len(), 1);
    }

    #[test]
    fn number_key_on_non_metric_process_column_shows_warning_dialog() {
        let mut app = make_test_app(3, 10);
        app.selected_process_column_index = 1;

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots[0].is_none());
        assert!(!app.show_details);
        assert!(app.show_metric_column_warning);
        assert_eq!(app.status, "Select a metric cell before pressing 1-4");

        let rendered = render_app_to_text(&app, 100, 45);
        assert!(
            !rendered.contains("Select a metric column first."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Move to a metric cell before pressing 1-4."),
            "{rendered}"
        );

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_metric_column_warning);
    }

    #[test]
    fn adding_graph_slot_requires_minimum_display_area() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 120, 45));

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots[1].is_none());
        assert!(app.show_display_area_warning);
        assert_eq!(app.status, "Not enough display area.");
    }

    #[test]
    fn resizing_closes_high_numbered_graph_slots_that_no_longer_fit() {
        let mut app = make_test_app(3, 10);
        let identity = app.selected_visible_process_identity().unwrap();
        app.graph_slots[0] = Some(GraphSlot::process(identity.clone(), DetailsMetric::Private));
        app.graph_slots[1] = Some(GraphSlot::process(identity.clone(), DetailsMetric::Workset));
        app.graph_slots[2] = Some(GraphSlot::process(identity, DetailsMetric::HandleCount));
        app.active_graph_slot_index = 2;
        app.show_details = true;

        app.set_screen_area(Rect::new(0, 0, 120, 58));
        app.close_graph_slots_that_do_not_fit();

        assert!(app.graph_slots[0].is_some());
        assert!(app.graph_slots[1].is_some());
        assert!(app.graph_slots[2].is_none());
        assert_eq!(app.active_graph_slot_index, 0);
        assert!(app.show_details);
    }

    #[test]
    fn d_does_not_toggle_details() {
        let mut app = make_test_app(3, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.show_details);
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
        let samples = details_samples_area_for_screen(screen, app.show_details).unwrap();
        let scrollbar_x = samples.right().saturating_sub(2);
        let scrollbar_top = samples.y.saturating_add(1);
        let scrollbar_bottom = samples.bottom().saturating_sub(2);

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
    fn graph_wheel_zooms_when_graph_is_focused() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.focused_panel = FocusedPanel::DetailsGraph;

        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 30,
                row: 20,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 100, 30),
        );

        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.graph_time_span_seconds, 60);
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
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
        let start_x = graph.x.saturating_add(70);
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
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
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
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
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
        app.select_details_sample_oldest();
        let selected = app.details_sample_selected;
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
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
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.toggle_graph_all_samples();
        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
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
        for offset in [0, 120, 240] {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }

        let screen = Rect::new(0, 0, 120, 45);
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
        let x = graph
            .right()
            .saturating_sub(1)
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_sub(GRAPH_ALL_SAMPLES_TOGGLE_WIDTH)
            .saturating_add(1);
        let y = graph.y.saturating_add(1);

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
    fn playback_all_samples_span_can_exceed_live_history_cap() {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        app.playback_path = Some(std::path::PathBuf::from("long.log"));
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
    fn playback_titles_show_loaded_sample_counts_instead_of_live_caps() {
        let mut app = make_test_app(1, 10);
        app.playback_path = Some(std::path::PathBuf::from("long.log"));
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

        assert!(rendered.contains("[Samples: tracked 7,201]"), "{rendered}");
        assert!(rendered.contains("[Samples: 7,201]"), "{rendered}");
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
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
        let x = graph
            .right()
            .saturating_sub(1)
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_add(1);
        let y = graph.y.saturating_add(1);

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
        let graph = details_slot_areas_for_screen(screen, app.show_details, 1)
            .into_iter()
            .next()
            .expect("graph slot");
        let y = graph.y.saturating_add(1);
        let all_samples_x = graph
            .right()
            .saturating_sub(1)
            .saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH)
            .saturating_sub(GRAPH_ALL_SAMPLES_TOGGLE_WIDTH)
            .saturating_add(1);
        let y_axis_x = graph
            .right()
            .saturating_sub(1)
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
        let graph = details_slot_areas_for_screen(screen, app.show_details, 1)
            .into_iter()
            .next()
            .expect("graph slot");
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
    fn clicking_graph_item_selects_matching_process_row() {
        let mut app = make_test_app(4, 10);
        let target = ProcessIdentity::from_row(&app.snapshot.processes[2]);
        app.graph_slots[0] = Some(GraphSlot::process(target, DetailsMetric::Private));
        app.active_graph_slot_index = 0;
        app.show_details = true;
        app.select_process_index(0);

        let screen = Rect::new(0, 0, 120, 60);
        let graph = details_graph_area_for_screen(screen, true).unwrap();
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: graph.x + 3,
                row: graph.y + 1,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );

        assert_eq!(app.process_table_state.selected(), Some(2));
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
    }

    #[test]
    fn settings_dialog_toggles_samples_panel_and_delta() {
        let mut app = make_test_app(2, 10);
        assign_private_graph(&mut app);

        app.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
            .unwrap();
        assert!(app.show_settings_dialog);
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_samples_panel);

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("Settings"), "{rendered}");
        assert!(!rendered.contains("Samples#1"), "{rendered}");

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_sample_delta);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_settings_dialog);
    }

    #[test]
    fn settings_ok_button_closes_with_mouse() {
        let screen = Rect::new(0, 0, 120, 45);
        let mut app = make_test_app(2, 10);
        app.show_settings_dialog = true;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ OK ]").expect("OK button should render");

        app.on_mouse(left_click(x + 2, y), screen);

        assert!(!app.show_settings_dialog);
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
        let graph = details_graph_area_for_screen(screen, app.show_details).unwrap();
        let (_, value_y) = find_text_position_in_area(&buffer, graph, "424,242")
            .expect("selected graph value should render in graph");
        let a_labels = find_styled_symbol_positions_in_area(&buffer, graph, "A", THEMES[0].warning);
        let b_labels = find_styled_symbol_positions_in_area(&buffer, graph, "B", THEMES[0].warning);
        let graph_inner = graph.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        let expected_label_y = graph_inner.bottom().saturating_sub(2);

        assert_eq!(a_labels.len(), 1, "A label should render once in Graph");
        assert_eq!(b_labels.len(), 1, "B label should render once in Graph");
        assert_eq!(a_labels[0].1, expected_label_y);
        assert_eq!(b_labels[0].1, expected_label_y);
        assert!(a_labels[0].1 > value_y);
        assert!(b_labels[0].1 > value_y);
    }

    #[test]
    fn details_rendering_keeps_only_graph_and_samples_frames() {
        let mut app = make_test_app(3, 10);
        assign_private_graph(&mut app);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        let rendered = render_app_to_text(&app, 120, 45);

        assert!(rendered.contains("Graph#1"), "{rendered}");
        assert!(rendered.contains("Samples#1"), "{rendered}");
        assert!(rendered.contains("MA5:"), "{rendered}");
        assert!(!rendered.contains("Details"), "{rendered}");
        assert!(!rendered.contains("A/B not set"), "{rendered}");
    }

    #[test]
    fn multi_graph_rendering_numbers_panels_and_hides_base_sample_summary() {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 140, 80));
        app.graph_slots[0] = Some(GraphSlot::process(
            app.selected_visible_process_identity().unwrap(),
            DetailsMetric::Private,
        ));
        app.graph_slots[1] = Some(GraphSlot::process(
            app.selected_visible_process_identity().unwrap(),
            DetailsMetric::Workset,
        ));
        app.show_details = true;
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );

        let rendered = render_app_to_text(&app, 140, 80);

        assert!(rendered.contains("Graph#1"), "{rendered}");
        assert!(rendered.contains("Samples#1"), "{rendered}");
        assert!(rendered.contains("Graph#2"), "{rendered}");
        assert!(rendered.contains("Samples#2"), "{rendered}");
        assert!(!rendered.contains("Max:"), "{rendered}");
        assert!(!rendered.contains("MA5:"), "{rendered}");
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
        let area = process_table_area_for_screen(screen, app.show_details);
        let visible_count = process_table_visible_column_count(area.width, &app.process_columns, 0);
        assert!(visible_count < 2 + app.process_columns.len());

        app.selected_process_column_index = visible_count - 1;
        app.process_metric_column_offset = 0;
        let before = render_app_to_text(&app, screen.width, screen.height);

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();

        let after_left = render_app_to_text(&app, screen.width, screen.height);
        assert_eq!(before, after_left);
        assert_eq!(app.selected_process_column_index, visible_count - 2);
        assert_eq!(app.process_metric_column_offset, 0);

        app.selected_process_column_index = visible_count - 1;
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.selected_process_column_index, visible_count);
        assert_eq!(app.process_metric_column_offset, 1);
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
    fn process_table_highlights_selected_metric_cell() {
        let mut app = make_test_app(1, 10);
        app.snapshot.processes[0].private_bytes = Some(987_654_321);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) = find_text_position(&buffer, "987,654,321")
            .expect("selected private bytes should be rendered");

        assert_eq!(buffer[(x, y)].bg, ui::THEMES[0].accent_alt);
    }

    #[test]
    fn process_table_colors_metric_value_increased_from_previous_sample() {
        let mut app = make_test_app(1, 10);
        let mut previous = app.snapshot.clone();
        previous.processes[0].private_bytes = Some(987_654_320);
        app.previous_snapshot = Some(previous);
        app.snapshot.processes[0].private_bytes = Some(987_654_321);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) = find_text_position(&buffer, "987,654,321")
            .expect("changed private bytes should be rendered");

        assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].success);
    }

    #[test]
    fn process_table_colors_metric_value_decreased_from_previous_sample() {
        let mut app = make_test_app(1, 10);
        let mut previous = app.snapshot.clone();
        previous.processes[0].private_bytes = Some(987_654_322);
        app.previous_snapshot = Some(previous);
        app.snapshot.processes[0].private_bytes = Some(987_654_321);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) = find_text_position(&buffer, "987,654,321")
            .expect("decreased private bytes should be rendered");

        assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].danger);
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
        app.graph_slots[0] = Some(GraphSlot::process(
            ProcessIdentity::from_row(&app.snapshot.processes[0]),
            DetailsMetric::Private,
        ));
        app.show_details = false;

        let buffer = render_app_to_buffer(&app, 120, 45);
        let (value_x, value_y) = find_text_position(&buffer, "107,374,182,400")
            .expect("graphed private bytes should be rendered");
        let (name_x, name_y) =
            find_text_position(&buffer, "target.exe").expect("tracked name should be rendered");
        let graph_number_cell = &buffer[(value_x - 1, value_y)];
        let value_cell = &buffer[(value_x, value_y)];

        assert_eq!(graph_number_cell.symbol(), "1");
        assert_eq!(graph_number_cell.bg, ui::THEMES[0].warning);
        assert_eq!(value_cell.fg, ui::THEMES[0].warning);
        assert_ne!(value_cell.bg, ui::THEMES[0].warning);
        assert_eq!(buffer[(name_x, name_y)].fg, ui::THEMES[0].text);
    }

    #[test]
    fn process_table_mouse_click_selects_row_and_metric_column() {
        let mut app = make_test_app(2, 10);
        app.process_columns = vec![
            MetricColumn::PrivateBytes,
            MetricColumn::ThreadCount,
            MetricColumn::HandleCount,
        ];
        app.selected_process_column_index = 2;
        app.snapshot.processes[1].thread_count = Some(77);
        app.snapshot.processes[1].handle_count = Some(888);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) =
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
    fn process_table_tracked_only_title_checkbox_click_toggles_filter() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        let screen = Rect::new(0, 0, 120, 45);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, "[ ] Tracked-only")
            .expect("tracked-only checkbox should be rendered in the process title");

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
        let (x, y) = find_text_position(&buffer, "[x] Tracked-only")
            .expect("checked tracked-only checkbox should be rendered in the process title");

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
    fn ram_vram_space_reports_metrics_are_retained_automatically() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(app.watch_list.is_empty());
        assert_eq!(app.details_target, DetailsTarget::Process);
        assert_eq!(
            app.status,
            "RAM/VRAM metrics keep 7200 samples automatically"
        );
    }

    #[test]
    fn ram_vram_number_keys_assign_graph_slot_and_show_slot_number() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.graph_slots[0]
                .as_ref()
                .and_then(GraphSlot::system_metric),
            Some(SystemMetric::Committed)
        );
        assert!(app.show_details);

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("1 Committed"), "{rendered}");
        assert!(!rendered.contains("★"), "{rendered}");
    }

    #[test]
    fn ram_vram_panel_excludes_cache_standby_and_separates_gpu_rows() {
        let mut app = make_test_app(3, 10);
        app.info_panel_mode = InfoPanelMode::Process;

        let rendered = render_app_to_text(&app, 120, 30);

        assert!(rendered.contains("RAM/VRAM"), "{rendered}");
        assert!(rendered.contains("[Max samples: 7200]"), "{rendered}");
        assert!(rendered.contains("Physical Memory"), "{rendered}");
        assert!(rendered.contains("Committed"), "{rendered}");
        assert!(rendered.contains("GPU Dedicated"), "{rendered}");
        assert!(rendered.contains("GPU Shared"), "{rendered}");
        assert!(rendered.contains("────────"), "{rendered}");
        assert!(!rendered.contains("Standby"), "{rendered}");
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
        assert!(app.status.contains("Use 1-4"));
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

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.graph_slots[0].is_none());
        assert_eq!(app.details_metric, DetailsMetric::Private);
        assert_eq!(app.status, "Full Path cannot be graphed");
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
        assert_eq!(app.status, "Screen resumed");
    }

    #[test]
    fn ctrl_p_toggles_display_pause_from_any_panel() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::System;

        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.is_display_paused());
        assert_eq!(app.status, "Screen paused");

        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.is_display_paused());
        assert_eq!(app.status, "Screen resumed");
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

        app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.ab_comparison.is_none());
        assert!(!app.show_details);
        assert!(app.status.contains("Details"));
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

        let rendered = render_app_to_text(&app, 120, 50);
        let rendered_lower = rendered.to_ascii_lowercase();

        assert!(rendered.contains("Keyboard shortcuts"), "{rendered}");
        assert!(rendered.contains("Global  (any focus)"), "{rendered}");
        assert!(rendered.contains("Processes"), "{rendered}");
        assert!(rendered.contains("RAM/VRAM"), "{rendered}");
        assert!(rendered.contains("Graph"), "{rendered}");
        assert!(rendered.contains("Samples"), "{rendered}");
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

        assert!(rendered.contains("Toggle Y-axis Min 0"), "{rendered}");
        assert!(rendered.contains("Pan time range"), "{rendered}");
        assert!(rendered.contains("Fit all samples"), "{rendered}");

        assert!(rendered.contains("Toggle recording"), "{rendered}");
        assert!(rendered.contains("Pause / Resume"), "{rendered}");
        assert!(rendered.contains("Copy selected row"), "{rendered}");

        assert!(rendered.contains("Select row range"), "{rendered}");
        assert!(rendered.contains("Toggle row selection"), "{rendered}");
        assert!(
            rendered.contains("Kill selected live process"),
            "{rendered}"
        );
        assert!(rendered.contains("Refresh open-files list"), "{rendered}");

        assert!(rendered.contains("Click panel"), "{rendered}");
        assert!(rendered.contains("Samples auto-scroll"), "{rendered}");

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
        assert!(!rendered_lower.contains("baseline"), "{rendered}");
    }

    #[test]
    fn help_dialog_header_and_shortcuts_use_footer_like_styles() {
        let mut app = make_test_app(3, 10);
        app.show_help = true;

        let buffer = render_app_to_buffer(&app, 100, 45);
        let theme = ui::THEMES[0];

        let (title_x, title_y) = find_text_position(&buffer, "Keyboard shortcuts")
            .expect("help dialog title should be rendered");
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
        assert!(
            popup.height < screen.height,
            "two-column help should not need the full screen height: {popup:?}"
        );
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
    fn footer_shows_global_and_context_shortcuts_on_two_rows() {
        let app = make_test_app(3, 10);

        let rendered = render_app_to_text(&app, 170, 30);

        assert!(rendered.contains("Tab Focus panel"), "{rendered}");
        assert!(rendered.contains("1-4 Show in graph"), "{rendered}");
        assert!(rendered.contains("c Pick columns"), "{rendered}");
        assert!(rendered.contains("t Tracked only"), "{rendered}");
        assert!(rendered.contains("g Toggle graphs"), "{rendered}");
        assert!(rendered.contains("s Sort rows"), "{rendered}");
        assert!(rendered.contains("f Open files"), "{rendered}");
        assert!(rendered.contains("Ctrl+O Settings"), "{rendered}");
        assert!(rendered.contains("Ctrl+P Pause"), "{rendered}");
        assert!(rendered.contains("Space Track"), "{rendered}");
        assert!(rendered.contains("Ctrl+F Filter"), "{rendered}");
        assert!(rendered.contains("Ctrl+R Record"), "{rendered}");
        assert!(rendered.contains("a/b Set A/B"), "{rendered}");
        assert!(rendered.contains("Shift+A/B Jump A/B"), "{rendered}");
        assert!(rendered.contains("x Clear A/B"), "{rendered}");
        assert!(rendered.contains("q Quit"), "{rendered}");
        assert!(rendered.contains("? Help"), "{rendered}");
        assert!(!rendered.contains("Left/Right Select column"), "{rendered}");
        assert!(
            !rendered.contains("Wheel/PgUp/PgDn Time span"),
            "{rendered}"
        );
        assert!(!rendered.contains("z Toggle Min 0"), "{rendered}");
        assert!(!rendered.contains("Ready"), "{rendered}");
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

        assert!(rendered.contains("CPUs"), "{rendered}");
        assert!(rendered.contains("Avg 42%"), "{rendered}");
        assert!(rendered.contains("P-core 3.20 GHz"), "{rendered}");
        assert!(rendered.contains("E-core 1.80 GHz"), "{rendered}");
        assert!(rendered.contains("(P) ▁▂ (E) █"), "{rendered}");
    }

    #[test]
    fn cpu_panel_number_keys_assign_cpu_average_to_graph_slot() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Cpu;
        app.snapshot.cpu_total_usage_percent = Some(42);
        app.system_history.record_snapshot(&app.snapshot);

        app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(
            app.graph_slots[0]
                .as_ref()
                .and_then(GraphSlot::system_metric),
            Some(SystemMetric::CpuAverage)
        );
        assert!(app.show_details);
        assert_eq!(
            app.active_graph_slot().map(GraphSlot::value_format_metric),
            Some(DetailsMetric::CpuPercent)
        );
        assert_eq!(
            app.graph_slot_samples(app.active_graph_slot().unwrap())[0].value,
            Some(42.0)
        );

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("1 Avg 42%"), "{rendered}");
        assert!(rendered.contains("CPUs - CPU Avg"), "{rendered}");
    }

    #[test]
    fn clicking_cpu_panel_moves_focus_to_cpus() {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, 120, 45);
        let area = ui::layout::cpu_panel_area_for_screen(screen);

        app.on_mouse(left_click(area.x + 1, area.y + 1), screen);

        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        assert_eq!(app.status, "Focus: CPUs");
    }

    #[test]
    fn help_dialog_takes_focus_border_from_previous_panel() {
        let mut app = make_test_app(3, 10);
        app.focused_panel = FocusedPanel::Processes;
        app.show_help = true;

        assert_modal_rect_focus_border(&app, help_area(Rect::new(0, 0, 100, 45)));
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
    fn recording_path_dialog_tab_tries_completion_without_switching_buttons() {
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
            app::RecordingPathSelection::Start
        );

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.recording_path_selection,
            app::RecordingPathSelection::Start
        );
        assert_eq!(app.status, "No directory completion match");

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
            app::RecordingPathSelection::Start
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
    fn tab_completes_recording_path_directory() {
        let root = unique_recording_dir("recording-path-complete");
        let target = root.join("alpha");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_recording_path_dialog = true;
        let head = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
        app.recording_path_draft = format!("{head}{}capture.log", std::path::MAIN_SEPARATOR);
        app.recording_path_cursor = head.len();

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
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
            rendered.contains("Missing directories will be created automatically."),
            "{rendered}"
        );
        assert!(rendered.contains("Tab completes."), "{rendered}");
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
        assert!(rendered.contains("winproc-tui-test-header"), "{rendered}");

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
            rendered.contains("Up/Down move  Space toggle  Enter/Esc close"),
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

        app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.column_preset, ColumnPreset::Default);
        assert_eq!(
            app.process_columns,
            ColumnPreset::Default.columns().to_vec()
        );
    }

    #[test]
    fn ctrl_c_copies_selected_process_row_text() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(!app.show_column_picker);
        assert_eq!(
            app::clipboard::last_copied_text().as_deref(),
            Some("0\tproc-0\t0\t--")
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
            Some("CPU Avg\t37%")
        );
        assert_eq!(app.status, "Copied row: CPU Avg");
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
    fn plain_i_toggles_process_info_panel() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.info_panel_mode, InfoPanelMode::Process);
        assert_eq!(app.process_info_cache.len(), 0);
        assert!(app.pending_process_info.is_some());

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.info_panel_mode, InfoPanelMode::System);
        assert!(app.pending_process_info.is_none());
    }

    #[test]
    fn ctrl_i_opens_process_jump_instead_of_info_panel() {
        let mut app = make_test_app(1, 10);

        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.info_panel_mode, InfoPanelMode::System);
        assert!(app.jump_editing);
    }

    #[test]
    fn process_info_request_is_debounced_on_selection_change() {
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
        app.info_panel_mode = InfoPanelMode::Process;
        app.ensure_selected_process_info();

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
                assert_eq!(identity.name, "proc-2");
            }
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        }
        assert!(app.pending_process_info.is_none());
        assert_eq!(app.process_info_in_flight.as_ref().unwrap().name, "proc-2");
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
        app.info_panel_mode = InfoPanelMode::Process;
        app.ensure_selected_process_info();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let identity = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect { identity, .. } => identity,
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        result_tx
            .send(ProcessInfoResult {
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
    fn stale_process_info_result_is_ignored_after_selection_changes() {
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
        app.info_panel_mode = InfoPanelMode::Process;
        app.ensure_selected_process_info();
        app.pending_process_info.as_mut().unwrap().changed_at =
            std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
        app.request_due_process_info().unwrap();
        let old_identity = match request_rx.try_recv().unwrap() {
            ProcessInfoRequest::Collect { identity, .. } => identity,
            ProcessInfoRequest::Stop => panic!("unexpected stop request"),
        };

        app.move_selection_down(1);
        result_tx
            .send(ProcessInfoResult {
                identity: old_identity.clone(),
                info: test_process_info(&old_identity.name, old_identity.pid),
            })
            .unwrap();

        assert!(!app.poll_process_info_results().unwrap());
        assert!(!app.process_info_cache.contains_key(&old_identity));
        assert!(app.process_info_in_flight.is_none());
        assert_eq!(
            app.pending_process_info.as_ref().unwrap().identity.name,
            "proc-1"
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

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.show_open_files);
        assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
        match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect { identity, process } => {
                assert_eq!(identity.name, "proc-0");
                assert_eq!(process.name, "proc-0");
            }
            OpenFilesRequest::Stop => panic!("unexpected stop request"),
        }
    }

    #[test]
    fn open_files_reuses_existing_filter_when_opened_again() {
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

        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.open_files_filter, ".mxf .mp4");
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

        assert!(!app.show_open_files);
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
        app.show_open_files = true;
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

        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
            .unwrap();

        assert!(app.open_files_result.is_some());
        assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
        match request_rx.try_recv().unwrap() {
            OpenFilesRequest::Collect { identity, process } => {
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
        let (open_files_worker, _, result_tx) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(
            1,
            10,
            sampling_worker,
            process_info_worker,
            open_files_worker,
        );
        let identity = app.selected_visible_process_identity().unwrap();
        app.open_files_in_flight = Some(identity.clone());
        app.show_open_files = true;

        result_tx
            .send(OpenFilesResult {
                identity,
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
    fn open_files_clipboard_uses_file_name_filter() {
        let mut app = make_test_app(1, 10);
        app.open_files_filter = ".mxf .MP4".to_string();
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
                    path: r"C:\tmp\b.MXF".to_string(),
                    handle_count: 2,
                },
                OpenFileEntry {
                    path: r"C:\tmp\c.mp4".to_string(),
                    handle_count: 1,
                },
            ],
            error: None,
        });

        app.copy_open_files_to_clipboard().unwrap();

        assert_eq!(
            crate::app::clipboard::last_copied_text().unwrap(),
            "C:\\tmp\\b.MXF\t2\nC:\\tmp\\c.mp4"
        );
    }

    #[test]
    fn open_files_filter_cursor_moves_and_inserts_at_cursor() {
        let mut app = make_test_app(1, 10);
        app.show_open_files = true;
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
        app.show_open_files = true;
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
        app.show_open_files = true;
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
        let expected_cursor = Position::new(20, 13);

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
        let before = ui::open_files::open_files_page_size_for_screen(screen, &app);

        app.open_files_filter = "b.log".to_string();
        let after = ui::open_files::open_files_page_size_for_screen(screen, &app);

        assert_eq!(before, after);
    }

    #[test]
    fn open_files_modal_renders_table_columns() {
        let mut app = make_test_app(1, 10);
        app.show_open_files = true;
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
    fn open_files_table_column_names_are_underlined() {
        let mut app = make_test_app(1, 10);
        app.show_open_files = true;
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
        app.show_open_files = true;
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
        app.set_open_files_page_size(ui::open_files::open_files_page_size_for_screen(
            screen, &app,
        ));
        app.scroll_open_files_end();

        let rendered = render_app_to_text(&app, screen.width, screen.height);

        assert!(!rendered.contains("file-00.log"), "{rendered}");
        assert!(rendered.contains("file-29.log"), "{rendered}");
    }

    #[test]
    fn cached_process_info_is_reused_without_worker_request() {
        let mut app = make_test_app(2, 10);
        app.info_panel_mode = InfoPanelMode::Process;
        let identity = app.selected_visible_process_identity().unwrap();
        app.process_info_cache.insert(
            identity.clone(),
            test_process_info(&identity.name, identity.pid),
        );

        app.ensure_selected_process_info();

        assert!(app.pending_process_info.is_none());
        assert_eq!(app.process_info_display_identity, Some(identity));
    }

    #[test]
    fn process_info_panel_keeps_previous_info_while_selected_row_is_pending() {
        let mut app = make_test_app(2, 10);
        app.info_panel_mode = InfoPanelMode::Process;
        let identity = app.selected_visible_process_identity().unwrap();
        app.process_info_cache.insert(
            identity.clone(),
            test_process_info(&identity.name, identity.pid),
        );
        app.process_info_display_identity = Some(identity);

        app.move_selection_down(1);

        assert_eq!(app.selected_visible_process().unwrap().name, "proc-1");
        assert_eq!(app.process_info_for_selected().unwrap().name, "proc-0");
        assert!(app.pending_process_info.is_some());
    }

    #[test]
    fn tab_cycles_focus_through_visible_panels() {
        let mut app = make_test_app(1, 10);

        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::System);

        app.graph_slots[0] = Some(GraphSlot::process(
            app.selected_visible_process_identity().unwrap(),
            DetailsMetric::Private,
        ));
        app.graph_slots[1] = Some(GraphSlot::process(
            app.selected_visible_process_identity().unwrap(),
            DetailsMetric::Workset,
        ));
        app.show_details = true;
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_slot_index, 0);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.active_graph_slot_index, 0);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_slot_index, 1);
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.active_graph_slot_index, 1);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_slot_index, 1);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
        assert_eq!(app.active_graph_slot_index, 0);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
        assert_eq!(app.active_graph_slot_index, 0);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
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
    fn space_toggles_selected_process_in_tracked_list() {
        let mut app = make_test_app(3, 10);
        app.snapshot.processes[0].name = "cargo.exe".to_string();
        app.snapshot.processes[1].name = "winproc-tui.exe".to_string();
        app.move_selection_down(1);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
        assert_eq!(app.watch_list, vec!["winproc-tui.exe"]);
        assert_eq!(app.visible_process_count(), 3);

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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
    fn t_toggles_tracked_only_when_processes_are_focused() {
        let mut app = make_test_app(2, 10);
        app.snapshot.processes[0].name = "target.exe".to_string();
        app.snapshot.processes[1].name = "other.exe".to_string();
        app.watch_list = vec!["target.exe".to_string()];
        app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.watch_enabled);
        assert_eq!(app.visible_process_count(), 1);
        assert_eq!(app.visible_process_at(0).unwrap().name, "target.exe");
        assert_eq!(
            app.tracked_total_visible_row().unwrap().process.name,
            "Tracked Total"
        );

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        assert!(!app.watch_enabled);
        assert_eq!(app.visible_process_count(), 2);
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

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
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

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        let screen = Rect::new(0, 0, 100, 30);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let process_area = process_table_area_for_screen(screen, app.show_details);
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

        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
            .unwrap();

        let rendered = render_app_to_text(&app, 100, 30);
        assert!(app.watch_enabled);
        assert_eq!(app.visible_process_count(), 0);
        assert_eq!(app.visible_tracked_process_count(), 0);
        assert!(app.status.contains("0 visible"));
        assert!(
            rendered.contains("[x] Tracked-only: 0 visible"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("[x] Tracked-only: 2 visible"),
            "{rendered}"
        );
    }

    #[test]
    fn process_table_title_shows_active_view_badges() {
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

        let rendered = render_app_to_text(&app, 130, 30);

        assert!(
            rendered.contains("[Max samples: normal 120 / tracked 7200]"),
            "{rendered}"
        );
        assert!(
            rendered.contains("[x] Tracked-only: 1 visible"),
            "{rendered}"
        );
        assert!(rendered.contains("[Filter: \"target\"]"), "{rendered}");
        assert!(!rendered.contains("Custom"), "{rendered}");
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

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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

        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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
            rendered.contains("Enter selects / Esc cancels / y removes"),
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
            app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
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
        assert!(rendered.contains("target.exe(12:34:56)"), "{rendered}");
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

        app.request_sample().unwrap();
        assert!(app.sampling_in_progress);
        assert_eq!(request_rx.try_recv(), Ok(SampleRequest::Sample));

        app.request_sample().unwrap();
        assert_eq!(request_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn sampling_result_updates_snapshot_and_clamps_selection() {
        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(5, 10, sampling_worker);
        app.select_last_row();
        app.sampling_in_progress = true;

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
        assert!(app.status.contains("Updated 2 process rows"));
        assert_eq!(app.process_history.len(), 2);
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
        app.graph_slots[0] = Some(GraphSlot::process(identity, DetailsMetric::Private));
        app.active_graph_slot_index = 0;
        app.show_details = true;
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
            product_version: InfoValue::Value("1.0.0".to_string()),
            workset_bytes: InfoValue::Value("1,024".to_string()),
            workset_private_bytes: InfoValue::Value("512".to_string()),
            ws_shareable_bytes: InfoValue::Value("256".to_string()),
            ws_shared_bytes: InfoValue::Value("128".to_string()),
        }
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

    fn assert_modal_focus_border(app: &App, popup_percent_x: u16, popup_percent_y: u16) {
        let screen = Rect::new(0, 0, 100, 45);
        let popup = centered_rect(popup_percent_x, popup_percent_y, screen);
        assert_modal_rect_focus_border(app, popup);
    }

    fn assert_modal_rect_focus_border(app: &App, popup: Rect) {
        let screen = Rect::new(0, 0, 100, 45);
        let buffer = render_app_to_buffer(app, screen.width, screen.height);
        let process_table = process_table_area_for_screen(screen, app.show_details);
        let theme = app.theme();

        assert_eq!(
            buffer[(popup.x, popup.y)].fg,
            theme.accent,
            "modal border should use the focused accent"
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
        assert!(
            rendered.contains("C:/logs/winproc-tui-demo.log"),
            "{rendered}"
        );
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
    fn log_dir_dialog_shows_instruction_above_directory_input() {
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.open_log_dir_dialog().unwrap();
        let buffer = render_app_to_buffer(&app, 120, 45);
        let (_, instruction_y) = find_text_position(
            &buffer,
            "Enter a directory containing winproc-tui log files.",
        )
        .expect("instruction should render");
        assert!(
            find_text_position(&buffer, "Tab completes.").is_some(),
            "{}",
            buffer_to_text(&buffer)
        );
        let (_, label_y) =
            find_text_position(&buffer, "Directory").expect("directory label should render");

        assert!(instruction_y < label_y);
    }

    #[test]
    fn tab_completes_log_dir_dialog_directory() {
        let root = unique_recording_dir("log-dir-complete");
        let target = root.join("alpha");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        let mut app = make_test_app(1, 10);
        app.show_log_list = true;
        app.open_log_dir_dialog().unwrap();
        app.log_dir_draft = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
        app.log_dir_cursor = app.log_dir_draft.len();

        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
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
        let (x, y) = find_text_position(&buffer, "C:/logs/second.log")
            .expect("second log row should be rendered");

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
        let path_text = path.display().to_string();
        let (x, y) =
            find_text_position(&buffer, &path_text).expect("log path row should be rendered");

        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);

        assert!(app.log_load_worker.is_some());
        assert!(app.status.starts_with("Opening log:"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn playback_header_shows_play_badge_and_path() {
        let mut app = make_test_app(1, 10);
        app.playback_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        let rendered = render_app_to_text(&app, 100, 20);

        assert!(rendered.contains("PLAY"), "{rendered}");
        assert!(rendered.contains("winproc-tui-demo.log"), "{rendered}");
    }

    #[test]
    fn playback_esc_returns_to_live_without_quit_confirmation() {
        let mut app = make_test_app(1, 10);
        app.playback_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(app.activity(), AppActivity::Live);
        assert!(app.playback_path.is_none());
        assert!(!app.show_quit_confirmation);
        assert_eq!(app.status, "Playback closed");
    }

    #[test]
    fn ctrl_r_is_rejected_during_playback() {
        let mut app = make_test_app(1, 10);
        app.playback_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap();

        assert_eq!(app.activity(), AppActivity::Playback);
        assert_eq!(app.status, "Recording is unavailable during playback");
    }

    #[test]
    fn ctrl_l_is_rejected_during_recording() {
        let path = unique_recording_path("deny-playback");
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
        assert_eq!(app.status, "Playback is unavailable during recording");

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loaded_log_is_ignored_if_recording_started_before_worker_returns() {
        let replay_path = std::env::temp_dir().join(format!(
            "winproc-tui-replay-race-test-{}-{}.log",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(
            &replay_path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let loaded = app::logs::load_log(&replay_path, SortSpec::default()).unwrap();
        let recording_path = unique_recording_path("deny-loaded-playback");
        let mut app = make_test_app(1, 10);
        track_process_name(&mut app, "proc-0");
        app.recording_path_draft = recording_path.display().to_string();
        app.recording_path_cursor = app.recording_path_draft.len();
        app.show_recording_path_dialog = true;
        app.confirm_recording_path().unwrap();

        app.apply_loaded_log(loaded);

        assert_eq!(app.activity(), AppActivity::Recording);
        assert!(app.playback_path.is_none());
        assert_eq!(app.status, "Playback is unavailable during recording");

        app.stop_recording().unwrap();
        let _ = std::fs::remove_file(recording_path);
        let _ = std::fs::remove_file(replay_path);
    }

    #[test]
    fn replay_log_feeds_graph_samples_without_turning_missing_values_to_zero() {
        let path = std::env::temp_dir().join(format!(
            "winproc-tui-replay-test-{}-{}.log",
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
        app.graph_slots[0] = Some(GraphSlot::process(identity, DetailsMetric::Private));
        app.show_details = true;
        app.focused_panel = FocusedPanel::DetailsSamples;
        let samples = app.graph_slot_samples(app.graph_slot(0).unwrap());

        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].value, None);
        assert_eq!(samples[1].value, Some(1024.0));

        let rendered = render_app_to_text(&app, 120, 45);
        assert!(rendered.contains("Samples#1"), "{rendered}");
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
                interval_seconds: 1,
                mouse: true,
                recording_last_dir: None,
                initial_theme: "Dark".to_string(),
                column_preset: ColumnPreset::Default,
                process_columns: ColumnPreset::Default.columns().to_vec(),
                sort: SortSpec::default(),
                initial_tracked_only: false,
                process_filters: Vec::new(),
                sampling_options: samplers::SamplingOptions::default(),
            },
            sampling_worker,
            process_info_worker,
            open_files_worker,
            sampling_in_progress: false,
            snapshot,
            previous_snapshot: None,
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
            show_settings_dialog: false,
            settings_selection: SettingsSelection::SamplesPanel,
            show_quit_confirmation: false,
            quit_confirm_selection: QuitConfirmSelection::Cancel,
            show_recording_no_tracked_warning: false,
            show_recording_path_dialog: false,
            recording_path_draft: String::new(),
            recording_path_cursor: 0,
            recording_path_completion: app::path_completion::PathCompletionState::default(),
            recording_path_selection: app::RecordingPathSelection::Start,
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
            playback_path: None,
            should_quit: false,
            column_picker_index: 0,
            column_picker_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            show_log_list: false,
            log_list_index: 0,
            log_list_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            show_log_dir_dialog: false,
            log_dir_draft: String::new(),
            log_dir_cursor: 0,
            log_dir_completion: app::path_completion::PathCompletionState::default(),
            log_dir_selection: app::LogDirSelection::Apply,
            log_dir_error: None,
            show_open_files: false,
            open_files_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
                page_size: 1,
                ..ui::widgets::scrollable_modal::ScrollableModalState::default()
            },
            open_files_result: None,
            open_files_in_flight: None,
            open_files_filter: String::new(),
            open_files_filter_cursor: 0,
            log_summaries: Vec::new(),
            log_list_dir: None,
            log_list_worker: None,
            log_list_last_click: None,
            log_load_worker: None,
            playback_watch_list: Vec::new(),
            playback_normalized_watch_names: std::collections::HashSet::new(),
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
            column_preset: ColumnPreset::Default,
            process_columns: ColumnPreset::Default.columns().to_vec(),
            sort: SortSpec::default(),
            paused_display: None,
            playback_display: None,
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
            info_panel_mode: InfoPanelMode::System,
            process_info_cache: std::collections::HashMap::new(),
            process_info_display_identity: None,
            pending_process_info: None,
            process_info_in_flight: None,
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
            process_count: row_count,
            processes,
        }
    }
}
