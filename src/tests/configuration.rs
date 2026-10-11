use super::support::{assign_private_graph, make_test_app, unique_config_path};
use crate::app::{
    GraphDisplayMode, GraphSlotLayout, ProcessPanelHeight, ProcessViewMode,
    SAMPLE_STALE_AFTER_SECONDS, SampleFreshness,
};
use crate::cli::Cli;
use crate::config;
use crate::config::{
    AppConfig, ConfigPaths, build_runtime_config, load_config, migrate_legacy_config,
    write_app_config,
};
use crate::model::{ColumnPreset, MetricColumn, SortColumn, SortDirection, SortSpec};
use crate::with_terminal_session;
use clap::Parser;
use std::time::Duration;

#[test]
fn config_path_uses_real_executable_directory() {
    let launched_dir = std::path::Path::new(r"C:\Users\user\AppData\Local\Microsoft\WinGet\Links");
    let real_dir = std::path::Path::new(
        r"C:\Users\user\AppData\Local\Microsoft\WinGet\Packages\TX230.winproc-tui\",
    );

    let paths = config::config_paths_from_resolved_dirs(launched_dir, launched_dir, real_dir);

    assert_eq!(paths.active, real_dir.join("winproc-tui.toml"));
    assert_eq!(paths.legacy, Some(launched_dir.join("winproc-tui.toml")));
}

#[test]
fn config_path_has_no_legacy_location_for_direct_executable() {
    let real_dir = std::path::Path::new(r"C:\tools\winproc-tui");

    let paths = config::config_paths_from_resolved_dirs(real_dir, real_dir, real_dir);

    assert_eq!(paths.active, real_dir.join("winproc-tui.toml"));
    assert_eq!(paths.legacy, None);
}

#[test]
fn config_path_follows_executable_symbolic_link() {
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!(
            "winproc-tui-test-config-symlink-{}",
            std::process::id()
        ));
    let launcher_dir = root.join("launcher");
    let real_dir = root.join("real");
    let launcher_exe = launcher_dir.join("winproc-tui.exe");
    let real_exe = real_dir.join("winproc-tui.exe");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&launcher_dir).unwrap();
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::write(&real_exe, b"test executable").unwrap();
    if let Err(error) = std::os::windows::fs::symlink_file(&real_exe, &launcher_exe) {
        std::fs::remove_dir_all(root).unwrap();
        if error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("failed to create executable symlink: {error}");
    }

    let paths = config::resolve_config_paths_from_executable(&launcher_exe).unwrap();
    let canonical_real_dir = std::fs::canonicalize(&real_dir).unwrap();

    assert_eq!(paths.active, canonical_real_dir.join("winproc-tui.toml"));
    assert_eq!(paths.legacy, Some(launcher_dir.join("winproc-tui.toml")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_launcher_config_moves_to_real_executable_directory() {
    let legacy = unique_config_path("launcher-config");
    let active = unique_config_path("real-executable-config");
    let content = "[general]\nmouse = false\n";
    std::fs::write(&legacy, content).unwrap();
    let _ = std::fs::remove_file(&active);
    let paths = ConfigPaths {
        active: active.clone(),
        legacy: Some(legacy.clone()),
    };

    migrate_legacy_config(&paths).unwrap();

    assert_eq!(std::fs::read_to_string(&active).unwrap(), content);
    assert!(!legacy.exists());
    std::fs::remove_file(active).unwrap();
}

#[test]
fn real_executable_config_wins_when_both_locations_exist() {
    let legacy = unique_config_path("launcher-config-existing-target");
    let active = unique_config_path("real-config-existing-target");
    std::fs::write(&legacy, "legacy").unwrap();
    std::fs::write(&active, "active").unwrap();
    let paths = ConfigPaths {
        active: active.clone(),
        legacy: Some(legacy.clone()),
    };

    migrate_legacy_config(&paths).unwrap();

    assert_eq!(std::fs::read_to_string(&active).unwrap(), "active");
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "legacy");
    std::fs::remove_file(active).unwrap();
    std::fs::remove_file(legacy).unwrap();
}

#[test]
fn terminal_session_does_not_restore_between_startup_and_main() {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let setup_events = std::rc::Rc::clone(&events);
    let operation_events = std::rc::Rc::clone(&events);
    let restore_events = std::rc::Rc::clone(&events);

    with_terminal_session(
        || {
            setup_events.borrow_mut().push("setup");
            Ok(())
        },
        |_| {
            let mut events = operation_events.borrow_mut();
            events.push("startup choice");
            events.push("initial sample");
            events.push("main loop");
            Ok(())
        },
        |_| {
            restore_events.borrow_mut().push("restore");
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "setup",
            "startup choice",
            "initial sample",
            "main loop",
            "restore"
        ]
    );
}

#[test]
fn terminal_session_restores_after_startup_failure() {
    let restored = std::cell::Cell::new(false);

    let result = with_terminal_session(
        || Ok(()),
        |_| Err::<(), _>(anyhow::anyhow!("startup failed")),
        |_| {
            restored.set(true);
            Ok(())
        },
    );

    assert!(result.is_err());
    assert!(restored.get());
}

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
    config.process_table.preset = Some("Custom".to_string());
    config.process_table.columns = Some(vec!["CPU %".to_string(), "Private".to_string()]);
    config.process_table.sort_by = Some("CPU %".to_string());
    config.process_table.sort_order = Some("asc".to_string());
    config.process_table.tracked_only = Some(true);
    config.process_table.view = Some("Tree".to_string());

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
    assert_eq!(runtime.initial_process_view_mode, ProcessViewMode::Tree);
}

#[test]
fn process_view_mode_defaults_to_flat_when_missing_or_invalid() {
    let missing: AppConfig = toml::from_str("[process_table]\npreset = \"Default\"\n").unwrap();
    assert_eq!(
        build_runtime_config(missing)
            .unwrap()
            .initial_process_view_mode,
        ProcessViewMode::Flat
    );

    let invalid: AppConfig = toml::from_str("[process_table]\nview = \"invalid\"\n").unwrap();
    assert_eq!(
        build_runtime_config(invalid)
            .unwrap()
            .initial_process_view_mode,
        ProcessViewMode::Flat
    );
}

#[test]
fn process_view_mode_round_trips_with_session_state() {
    let path = unique_config_path("process-view-mode");
    let mut app = make_test_app(3, 10);
    app.process_view_mode = ProcessViewMode::Tree;

    write_app_config(&path, &app).unwrap();
    let rendered = std::fs::read_to_string(&path).unwrap();
    let runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("view = \"Tree\""), "{rendered}");
    assert_eq!(runtime.initial_process_view_mode, ProcessViewMode::Tree);
}

#[test]
fn process_panel_height_defaults_to_auto_when_the_setting_is_missing() {
    let config: AppConfig = toml::from_str(
        r#"
[process_table]
preset = "Default"
"#,
    )
    .unwrap();

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(
        runtime.initial_process_panel_height,
        ProcessPanelHeight::Auto
    );
}

#[test]
fn process_panel_height_loads_manual_rows_and_rejects_invalid_values() {
    let manual: AppConfig = toml::from_str(
        r#"
[process_table]
body_rows = 14
"#,
    )
    .unwrap();
    assert_eq!(
        build_runtime_config(manual)
            .unwrap()
            .initial_process_panel_height,
        ProcessPanelHeight::Manual(14)
    );

    for value in ["0", "-1", "65536", "\"invalid\""] {
        let config: AppConfig =
            toml::from_str(&format!("[process_table]\nbody_rows = {value}\n")).unwrap();
        assert_eq!(
            build_runtime_config(config)
                .unwrap()
                .initial_process_panel_height,
            ProcessPanelHeight::Auto,
            "value={value}"
        );
    }
}

#[test]
fn process_panel_height_round_trips_manual_and_auto_settings() {
    let path = unique_config_path("process-panel-height");
    let mut app = make_test_app(20, 10);
    app.process_panel_height = ProcessPanelHeight::Manual(14);

    write_app_config(&path, &app).unwrap();
    let manual_rendered = std::fs::read_to_string(&path).unwrap();
    let manual_runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    assert!(
        manual_rendered.contains("body_rows = 14"),
        "{manual_rendered}"
    );
    assert_eq!(
        manual_runtime.initial_process_panel_height,
        ProcessPanelHeight::Manual(14)
    );

    app.process_panel_height = ProcessPanelHeight::Auto;
    write_app_config(&path, &app).unwrap();
    let auto_rendered = std::fs::read_to_string(&path).unwrap();
    let auto_runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(
        auto_rendered.contains("body_rows = \"auto\""),
        "{auto_rendered}"
    );
    assert_eq!(
        auto_runtime.initial_process_panel_height,
        ProcessPanelHeight::Auto
    );
}

#[test]
fn default_runtime_config_selects_all_process_columns() {
    let runtime = build_runtime_config(AppConfig::default()).unwrap();

    assert_eq!(
        runtime.process_columns,
        crate::model::ColumnPreset::Default.columns()
    );
    assert_eq!(
        runtime.process_column_widths.resolved(SortColumn::Pid),
        SortColumn::Pid.default_width()
    );
}

#[test]
fn removed_gc_rate_columns_are_ignored_in_saved_config() {
    let mut config = AppConfig::default();
    config.process_table.preset = Some("Custom".to_string());
    config.process_table.columns = Some(vec![
        ".NET GC0/s".to_string(),
        "CPU%".to_string(),
        ".NET GC2/s".to_string(),
    ]);
    config.process_table.sort_by = Some(".NET GC1/s".to_string());

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.process_columns, vec![MetricColumn::CpuPercent]);
    assert_eq!(
        runtime.sort.column,
        SortColumn::Metric(MetricColumn::WorksetPrivateBytes)
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
        40
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
    config.process_table.preset = Some("Custom".to_string());
    config.process_table.columns = Some(Vec::new());

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.column_preset, ColumnPreset::Default);
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
    config.graphs.columns = Some(2);
    config.graphs.samples = Some(false);
    config.graphs.delta = Some(false);

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
    config.graphs.columns = Some(4);

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.initial_graph_slot_layout, GraphSlotLayout::Auto);
}

#[test]
fn build_runtime_config_restores_three_column_graph_layout() {
    let mut config = AppConfig::default();
    config.graphs.columns = Some(3);

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(
        runtime.initial_graph_slot_layout,
        GraphSlotLayout::ThreeColumns
    );
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
fn app_config_does_not_serialize_legacy_tracked_entries() {
    let mut config = AppConfig::default();
    config.tracked.push(config::TrackedConfig {
        name: "app.exe".to_string(),
    });

    let rendered = toml::to_string(&config).unwrap();

    assert!(!rendered.contains("[[tracked]]"));
    assert!(!rendered.contains("[[watch]]"));
}

#[test]
fn legacy_named_tracking_lists_migrate_to_profiles_and_startup_mode() {
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
    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(
        runtime.investigation_startup,
        config::InvestigationStartup::ChooseProfile
    );
    assert_eq!(runtime.active_investigation_profile.as_deref(), Some("API"));
    assert_eq!(runtime.saved_investigation_profiles[0].name, "API");
    assert_eq!(
        runtime.saved_investigation_profiles[0].tracked_names,
        vec!["api.exe", "worker.exe"]
    );
}

#[test]
fn start_empty_runtime_ignores_last_working_tracked_list() {
    let mut config = AppConfig::default();
    config.general.mouse = false;
    config.general.theme = "Cyan".to_string();
    config.process_table.body_rows = config::ProcessPanelHeightConfig::Rows(14);
    config.tracking.startup = config::TrackedListStartup::StartEmpty;
    config.tracking.active_list = Some("API".to_string());
    config.tracked.push(config::TrackedConfig {
        name: "api.exe".to_string(),
    });

    let runtime = build_runtime_config(config).unwrap();

    assert!(runtime.process_filters.is_empty());
    assert_eq!(runtime.active_investigation_profile, None);
    assert!(!runtime.mouse);
    assert_eq!(runtime.initial_theme, "Cyan");
    assert_eq!(
        runtime.initial_process_panel_height,
        ProcessPanelHeight::Manual(14)
    );
}

#[test]
fn resume_last_runtime_restores_state_without_binding_a_profile() {
    let mut config = AppConfig::default();
    config.investigation = Some(config::InvestigationConfig {
        startup: config::InvestigationStartup::ResumeLast,
        active_profile: Some("API".to_string()),
        last: config::InvestigationStateConfig {
            tracked_names: vec!["api.exe".to_string()],
            ..config::InvestigationStateConfig::default()
        },
    });
    config.investigation_profiles = vec![config::SavedInvestigationProfile {
        name: "API".to_string(),
        ..config::SavedInvestigationProfile::default()
    }];

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.process_filters, ["api.exe"]);
    assert_eq!(runtime.active_investigation_profile, None);
}

#[test]
fn legacy_tracking_list_name_collision_preserves_both_profiles() {
    let mut config = AppConfig::default();
    config.tracking.startup = config::TrackedListStartup::ChooseList;
    config.tracking.active_list = Some("API".to_string());
    config.investigation_profiles = vec![config::SavedInvestigationProfile {
        name: "API".to_string(),
        ..config::SavedInvestigationProfile::default()
    }];
    config.tracked_lists = vec![config::SavedTrackedList {
        name: "API".to_string(),
        processes: vec!["api.exe".to_string(), "API.EXE".to_string()],
    }];

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.saved_investigation_profiles.len(), 2);
    assert_eq!(runtime.saved_investigation_profiles[0].name, "API");
    assert_eq!(
        runtime.saved_investigation_profiles[1].name,
        "API (Tracking List)"
    );
    assert_eq!(
        runtime.saved_investigation_profiles[1].tracked_names,
        vec!["api.exe"]
    );
    assert_eq!(
        runtime.active_investigation_profile.as_deref(),
        Some("API (Tracking List)")
    );
}

#[test]
fn app_config_writes_only_the_unified_investigation_format() {
    let mut app = make_test_app(1, 10);
    app.runtime.investigation_startup = config::InvestigationStartup::ChooseProfile;
    app.active_investigation_profile = Some("API".to_string());
    app.watch_list = vec!["api.exe".to_string()];
    app.runtime.saved_investigation_profiles = vec![config::SavedInvestigationProfile {
        name: "API".to_string(),
        investigation: config::InvestigationStateConfig {
            tracked_names: vec!["api.exe".to_string()],
            ..config::InvestigationStateConfig::default()
        },
    }];
    let path = unique_config_path("unified-investigations");

    write_app_config(&path, &app).unwrap();
    let rendered = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("[investigation]"), "{rendered}");
    assert!(
        rendered.contains("startup = \"choose_profile\""),
        "{rendered}"
    );
    assert!(rendered.contains("active_profile = \"API\""), "{rendered}");
    assert!(
        rendered.contains("tracked_names = [\"api.exe\"]"),
        "{rendered}"
    );
    assert!(!rendered.contains("[[tracked_lists]]"), "{rendered}");
    assert!(!rendered.contains("[tracking]"), "{rendered}");
}

#[test]
fn investigation_profiles_round_trip_through_config() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![config::SavedInvestigationProfile {
        name: "API investigation".to_string(),
        investigation: config::InvestigationStateConfig {
            tracked_names: vec!["api.exe".to_string()],
            ..config::InvestigationStateConfig::default()
        },
    }];
    let path = unique_config_path("investigation-profiles");

    write_app_config(&path, &app).unwrap();
    let rendered = std::fs::read_to_string(&path).unwrap();
    let runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(
        rendered.contains("[[investigation_profiles]]"),
        "{rendered}"
    );
    let value = toml::from_str::<toml::Value>(&rendered).unwrap();
    let profile = value["investigation_profiles"]
        .as_array()
        .unwrap()
        .first()
        .unwrap()
        .as_table()
        .unwrap();
    assert_eq!(
        profile.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["name", "tracked_names"]
    );
    assert_eq!(runtime.saved_investigation_profiles.len(), 1);
    assert_eq!(
        runtime.saved_investigation_profiles[0],
        app.runtime.saved_investigation_profiles[0]
    );
}

#[test]
fn broad_profile_config_migrates_global_settings_from_last_investigation_only() {
    let mut config = AppConfig::default();
    config.investigation = Some(config::InvestigationConfig {
        startup: config::InvestigationStartup::ResumeLast,
        active_profile: None,
        last: config::InvestigationStateConfig {
            tracked_names: vec![" current.exe ".to_string()],
            tracked_only: Some(true),
            process_view: Some("Tree".to_string()),
            process_columns: Some(vec!["CPU%".to_string(), "PrivBytes".to_string()]),
            sort_by: Some("CPU%".to_string()),
            sort_order: Some("asc".to_string()),
            graph_columns: Some(2),
            graph_time_span_seconds: Some(300),
            samples: Some(false),
            delta: Some(false),
            y_axis_zero_min: Some(false),
            recording_interval_seconds: Some(5),
            ..config::InvestigationStateConfig::default()
        },
    });
    config.investigation_profiles = vec![
        config::SavedInvestigationProfile {
            name: " Profile ".to_string(),
            investigation: config::InvestigationStateConfig {
                tracked_names: vec![" app.exe ".to_string(), "APP.EXE".to_string()],
                process_columns: Some(vec!["unknown".to_string()]),
                graphs: vec![config::InvestigationGraphConfig {
                    kind: " PROCESS ".to_string(),
                    metric: " PRIVATE_BYTES ".to_string(),
                    display_mode: " MA ".to_string(),
                    process_name: Some(" app.exe ".to_string()),
                    executable_path: Some(" ".to_string()),
                    gpu_adapter_name: None,
                }],
                graph_columns: Some(9),
                graph_time_span_seconds: Some(1),
                samples: Some(true),
                delta: Some(true),
                y_axis_zero_min: Some(true),
                recording_interval_seconds: Some(10),
                ..config::InvestigationStateConfig::default()
            },
        },
        config::SavedInvestigationProfile {
            name: "profile".to_string(),
            ..config::SavedInvestigationProfile::default()
        },
        config::SavedInvestigationProfile::default(),
    ];

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.saved_investigation_profiles.len(), 1);
    let profile = &runtime.saved_investigation_profiles[0];
    assert_eq!(profile.name, "Profile");
    assert_eq!(profile.tracked_names, ["app.exe"]);
    assert_eq!(
        profile.investigation,
        config::InvestigationStateConfig {
            tracked_names: vec!["app.exe".to_string()],
            ..config::InvestigationStateConfig::default()
        }
    );
    assert_eq!(runtime.process_filters, ["current.exe"]);
    assert!(runtime.initial_tracked_only);
    assert_eq!(runtime.initial_process_view_mode, ProcessViewMode::Tree);
    assert_eq!(
        runtime.process_columns,
        [MetricColumn::CpuPercent, MetricColumn::PrivateBytes]
    );
    assert_eq!(
        runtime.initial_graph_slot_layout,
        GraphSlotLayout::TwoColumns
    );
    assert_eq!(runtime.initial_graph_time_span_seconds, 300);
    assert!(!runtime.initial_show_samples_panel);
    assert!(!runtime.initial_show_sample_delta);
    assert!(!runtime.initial_graph_y_axis_zero_min);
    assert_eq!(runtime.initial_recording_interval_seconds, 5);
}

#[test]
fn explicit_global_preferences_win_over_legacy_investigation_values() {
    let mut config = AppConfig::default();
    config.process_table.view = Some("Flat".to_string());
    config.process_table.tracked_only = Some(false);
    config.graphs.time_span_seconds = Some(600);
    config.recording.interval_seconds = Some(10);
    config.investigation = Some(config::InvestigationConfig {
        startup: config::InvestigationStartup::ResumeLast,
        active_profile: None,
        last: config::InvestigationStateConfig {
            process_view: Some("Tree".to_string()),
            tracked_only: Some(true),
            graph_time_span_seconds: Some(300),
            recording_interval_seconds: Some(5),
            ..config::InvestigationStateConfig::default()
        },
    });

    let runtime = build_runtime_config(config).unwrap();

    assert_eq!(runtime.initial_process_view_mode, ProcessViewMode::Flat);
    assert!(!runtime.initial_tracked_only);
    assert_eq!(runtime.initial_graph_time_span_seconds, 600);
    assert_eq!(runtime.initial_recording_interval_seconds, 10);
}

#[test]
fn legacy_broad_profile_toml_is_rewritten_as_tracked_names_only() {
    let mut config: AppConfig = toml::from_str(
        r#"
[investigation]
startup = "resume_last"
tracked_names = ["current.exe"]
tracked_only = true
process_view = "Tree"
process_columns = ["CPU%", "PrivBytes"]
sort_by = "CPU%"
sort_order = "Asc"
graph_columns = 2
graph_time_span_seconds = 300
samples = false
delta = false
y_axis_zero_min = false
recording_interval_seconds = 5

[[investigation.graphs]]
kind = "process"
metric = "private_bytes"
display_mode = "ma5"
process_name = "current.exe"

[[investigation_profiles]]
name = "Legacy API"
tracked_names = ["api.exe"]
tracked_only = false
process_view = "Flat"
graph_columns = 3

[[investigation_profiles.graphs]]
kind = "process"
metric = "private_bytes"
display_mode = "raw"
process_name = "api.exe"
"#,
    )
    .unwrap();

    config::prepare_app_config(&mut config);
    let rendered = toml::to_string_pretty(&config).unwrap();
    let value = toml::from_str::<toml::Value>(&rendered).unwrap();
    let profile = value["investigation_profiles"]
        .as_array()
        .unwrap()
        .first()
        .unwrap()
        .as_table()
        .unwrap();

    assert_eq!(
        profile.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["name", "tracked_names"]
    );
    assert_eq!(config.graphs.columns, Some(2));
    assert_eq!(config.graphs.time_span_seconds, Some(300));
    assert_eq!(config.recording.interval_seconds, Some(5));
    assert!(!rendered.contains("[[investigation.graphs]]"), "{rendered}");
    assert!(
        !rendered.contains("[[investigation_profiles.graphs]]"),
        "{rendered}"
    );
}

#[test]
fn config_without_investigation_profiles_remains_compatible() {
    let config: AppConfig = toml::from_str(
        r#"
[general]
mouse = true
theme = "Green"
"#,
    )
    .unwrap();

    let runtime = build_runtime_config(config).unwrap();

    assert!(runtime.saved_investigation_profiles.is_empty());
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
fn app_config_saves_selected_color_scheme() {
    let mut app = make_test_app(3, 10);
    app.theme_index = 3;
    app.high_contrast = true;
    let path = unique_config_path("color-scheme");

    write_app_config(&path, &app).unwrap();
    let rendered = std::fs::read_to_string(&path).unwrap();
    let loaded = load_config(&path).unwrap();
    let runtime = build_runtime_config(loaded).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("theme = \"Cyan\""), "{rendered}");
    assert_eq!(runtime.initial_theme, "Cyan");
    assert!(runtime.initial_high_contrast);
}

#[test]
fn app_config_saves_global_graph_preferences_without_graph_registrations() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.show_samples_panel = false;
    app.show_sample_delta = false;
    let path = unique_config_path("graph-layout");

    write_app_config(&path, &app).unwrap();
    let rendered = std::fs::read_to_string(&path).unwrap();
    let runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("[graphs]"), "{rendered}");
    assert!(rendered.contains("columns = 2"), "{rendered}");
    assert!(rendered.contains("samples = false"), "{rendered}");
    assert!(rendered.contains("delta = false"), "{rendered}");
    assert!(!rendered.contains("display_mode = \"ma5\""), "{rendered}");
    assert_eq!(
        runtime.initial_graph_slot_layout,
        GraphSlotLayout::TwoColumns
    );
    assert!(!runtime.initial_show_samples_panel);
    assert!(!runtime.initial_show_sample_delta);
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
    assert!(rendered.contains("PrivBytes = 14"), "{rendered}");
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
fn show_exited_defaults_on_and_restores_false() {
    assert!(
        build_runtime_config(AppConfig::default())
            .unwrap()
            .initial_show_exited
    );
    let config: AppConfig = toml::from_str("[process_table]\nshow_exited = false\n").unwrap();
    assert!(!build_runtime_config(config).unwrap().initial_show_exited);
}

#[test]
fn show_exited_round_trips_with_session_state() {
    let path = unique_config_path("show-exited");
    let mut app = make_test_app(1, 10);
    app.show_exited = false;
    write_app_config(&path, &app).unwrap();
    let runtime = build_runtime_config(load_config(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(path);
    assert!(!runtime.initial_show_exited);
}
