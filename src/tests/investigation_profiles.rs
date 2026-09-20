use super::support::{
    assign_private_graph, find_text_position, make_test_app,
    record_tracked_process_history_samples, render_app_to_buffer, render_app_to_text,
    track_process_name, unique_config_path, unique_recording_path,
};
use crate::{
    app::{GraphDisplayMode, GraphSlotLayout, InvestigationProfilesView, ProcessViewMode},
    config::{InvestigationStartup, InvestigationStateConfig, SavedInvestigationProfile},
    model::{MetricColumn, ProcessIdentity, SortColumn, SortDirection},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

fn profile(name: &str) -> SavedInvestigationProfile {
    SavedInvestigationProfile {
        name: name.to_string(),
        investigation: InvestigationStateConfig {
            tracked_names: vec!["proc-0".to_string()],
            ..InvestigationStateConfig::default()
        },
    }
}

#[test]
fn save_as_captures_only_tracked_process_names() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].name = "api.exe".to_string();
    app.snapshot.processes[0].executable_path = Some(r"C:\apps\api.exe".to_string());
    app.selected_process_identity = Some(ProcessIdentity::from_row(&app.snapshot.processes[0]));
    app.watch_list = vec!["api.exe".to_string()];
    app.watch_enabled = true;
    app.process_view_mode = ProcessViewMode::Tree;
    app.process_columns = vec![MetricColumn::CpuPercent, MetricColumn::PrivateBytes];
    app.sort.column = SortColumn::Metric(MetricColumn::CpuPercent);
    app.sort.direction = SortDirection::Asc;
    assign_private_graph(&mut app);
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.graph_time_span_seconds = 300;
    app.show_samples_panel = false;
    app.show_sample_delta = false;
    app.graph_y_axis_zero_min = false;
    app.recording_interval_index = 2;

    app.begin_save_investigation_profile_as();
    for ch in "API check".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.investigation_profiles_dialog.is_none());
    assert_eq!(app.runtime.saved_investigation_profiles.len(), 1);
    assert_eq!(
        app.active_investigation_profile.as_deref(),
        Some("API check")
    );
    let saved = &app.runtime.saved_investigation_profiles[0];
    assert_eq!(saved.tracked_names, ["api.exe"]);
    assert_eq!(
        saved.investigation,
        InvestigationStateConfig {
            tracked_names: vec!["api.exe".to_string()],
            ..InvestigationStateConfig::default()
        }
    );
    let rendered = toml::to_string(saved).unwrap();
    let value = toml::from_str::<toml::Value>(&rendered).unwrap();
    let fields = value
        .as_table()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(fields, ["name", "tracked_names"]);
}

#[test]
fn ctrl_s_without_a_bound_profile_opens_only_save_as() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::NameInput { .. })
    ));
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(
        rendered.contains("SAVE INVESTIGATION PROFILE AS"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("OPEN INVESTIGATION PROFILE"),
        "{rendered}"
    );

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.investigation_profiles_dialog.is_none());
}

#[test]
fn ctrl_s_overwrites_the_active_profile_tracking_list() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("Bound")];
    app.active_investigation_profile = Some("Bound".to_string());
    app.watch_list = vec!["api.exe".to_string(), "worker.exe".to_string()];

    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(app.runtime.saved_investigation_profiles.len(), 1);
    assert_eq!(
        app.runtime.saved_investigation_profiles[0].tracked_names,
        ["api.exe", "worker.exe"]
    );
    assert_eq!(app.active_investigation_profile.as_deref(), Some("Bound"));
    assert!(!app.active_investigation_profile_dirty());
    assert_eq!(app.status, "Saved Investigation Profile: Bound");
}

#[test]
fn profile_open_dialog_ignores_removed_management_keys() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("First")];
    app.open_investigation_profiles();

    for key in [
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
    ] {
        app.on_key(key).unwrap();
        assert!(matches!(
            app.investigation_profiles_view(),
            Some(InvestigationProfilesView::Browse)
        ));
    }
    assert_eq!(app.runtime.saved_investigation_profiles.len(), 1);
}

#[test]
fn header_shows_profile_binding_and_modified_marker() {
    fn assert_profile_badge(app: &crate::app::App, label: &str) {
        let buffer = render_app_to_buffer(app, 120, 45);
        let (x, y) = find_text_position(&buffer, label).expect("profile badge should render");
        for offset in 0..label.chars().count() as u16 {
            assert_eq!(buffer[(x + offset, y)].fg, Color::Black);
            assert_eq!(buffer[(x + offset, y)].bg, app.theme().muted);
        }
    }

    let mut app = make_test_app(1, 10);
    let initial = render_app_to_text(&app, 120, 45);
    assert!(initial.contains("PF: none"), "{initial}");
    assert_profile_badge(&app, "PF: none");

    app.begin_save_investigation_profile_as();
    for ch in "myapp".chars() {
        app.push_investigation_profile_name_char(ch);
    }
    app.commit_investigation_profile_name_input();

    let clean = render_app_to_text(&app, 120, 45);
    assert!(clean.contains("PF: myapp"), "{clean}");
    assert!(!clean.contains("PF: myapp*"), "{clean}");
    assert_profile_badge(&app, "PF: myapp");

    app.watch_enabled = true;
    app.process_view_mode = ProcessViewMode::Tree;
    app.process_columns = vec![MetricColumn::CpuPercent];
    app.sort.column = SortColumn::Metric(MetricColumn::CpuPercent);
    app.sort.direction = SortDirection::Asc;
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.graph_time_span_seconds = app.graph_time_span_seconds.saturating_add(1);
    app.show_samples_panel = false;
    app.show_sample_delta = false;
    app.graph_y_axis_zero_min = false;
    app.recording_interval_index = 2;
    let still_clean = render_app_to_text(&app, 120, 45);
    assert!(still_clean.contains("PF: myapp"), "{still_clean}");
    assert!(!still_clean.contains("PF: myapp*"), "{still_clean}");

    app.watch_list.push("worker.exe".to_string());
    let modified = render_app_to_text(&app, 120, 45);
    assert!(modified.contains("PF: myapp*"), "{modified}");
    assert_profile_badge(&app, "PF: myapp*");

    app.save_active_investigation_profile();
    let saved = render_app_to_text(&app, 120, 45);
    assert!(saved.contains("PF: myapp"), "{saved}");
    assert!(!saved.contains("PF: myapp*"), "{saved}");
    assert_profile_badge(&app, "PF: myapp");
}

#[test]
fn save_delete_and_duplicate_names_are_explicit() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("First")];
    app.active_investigation_profile = Some("First".to_string());
    app.watch_list = vec!["updated.exe".to_string()];
    app.save_active_investigation_profile();
    assert_eq!(
        app.runtime.saved_investigation_profiles[0].tracked_names,
        ["updated.exe"]
    );

    app.begin_save_investigation_profile_as();
    for ch in "first".chars() {
        app.push_investigation_profile_name_char(ch);
    }
    app.commit_investigation_profile_name_input();
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::NameInput {
            error: Some(error),
            ..
        }) if error.contains("already exists")
    ));

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.investigation_profiles_dialog.is_none());
    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.runtime.saved_investigation_profiles.is_empty());
    assert_eq!(app.active_investigation_profile, None);
}

#[test]
fn profile_delete_confirmation_uses_enter_and_keeps_current_investigation() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("Delete me"), profile("Keep bound")];
    app.active_investigation_profile = Some("Delete me".to_string());
    app.watch_list = vec!["current.exe".to_string()];
    app.watch_enabled = true;
    app.process_view_mode = ProcessViewMode::Tree;
    app.graph_time_span_seconds = 600;

    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::ConfirmDelete { name }) if name == "Delete me"
    ));

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.runtime
            .saved_investigation_profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        ["Keep bound"]
    );
    assert_eq!(app.active_investigation_profile, None);
    assert_eq!(app.watch_list, ["current.exe"]);
    assert!(app.watch_enabled);
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);
    assert_eq!(app.graph_time_span_seconds, 600);

    app.runtime
        .saved_investigation_profiles
        .insert(0, profile("Delete other"));
    app.active_investigation_profile = Some("Keep bound".to_string());
    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.runtime
            .saved_investigation_profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        ["Keep bound"]
    );
    assert_eq!(
        app.active_investigation_profile.as_deref(),
        Some("Keep bound")
    );
    assert_eq!(app.watch_list, ["current.exe"]);
    assert!(app.watch_enabled);
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);
    assert_eq!(app.graph_time_span_seconds, 600);
}

#[test]
fn profile_delete_confirmation_only_esc_cancels() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("First")];
    app.active_investigation_profile = Some("First".to_string());
    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();

    for ch in ['y', 'Y', 'n', 'N'] {
        app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.investigation_profiles_view(),
            Some(InvestigationProfilesView::ConfirmDelete { name }) if name == "First"
        ));
        assert_eq!(app.runtime.saved_investigation_profiles.len(), 1);
        assert_eq!(app.active_investigation_profile.as_deref(), Some("First"));
    }

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Browse)
    ));
    assert_eq!(app.runtime.saved_investigation_profiles.len(), 1);
    assert_eq!(app.active_investigation_profile.as_deref(), Some("First"));
}

#[test]
fn profile_delete_confirmation_renders_exact_warning_shortcuts() {
    let mut app = make_test_app(1, 10);
    app.runtime.saved_investigation_profiles = vec![profile("First")];
    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();

    let buffer = render_app_to_buffer(&app, 100, 45);
    let rendered = super::support::buffer_to_text(&buffer);
    let shortcut = "Enter Delete  Esc Cancel";
    let (enter_x, shortcut_y) =
        find_text_position(&buffer, shortcut).expect("delete shortcuts should render exactly");

    assert!(
        rendered.contains("DELETE INVESTIGATION PROFILE?"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Delete \"First\"? The current setup is kept."),
        "{rendered}"
    );
    assert!(rendered.contains("This cannot be undone."), "{rendered}");
    assert!(!rendered.contains("Enter/Esc/n Cancel"), "{rendered}");
    assert!(!rendered.contains("y Delete"), "{rendered}");

    assert_eq!(buffer[(enter_x, shortcut_y)].fg, app.theme().warning);
    assert!(
        buffer[(enter_x, shortcut_y)]
            .modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(buffer[(enter_x + 6, shortcut_y)].fg, app.theme().text);
    let esc_x = enter_x + "Enter Delete  ".chars().count() as u16;
    assert_eq!(buffer[(esc_x, shortcut_y)].fg, app.theme().warning);
    assert!(
        buffer[(esc_x, shortcut_y)]
            .modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(buffer[(esc_x + 4, shortcut_y)].fg, app.theme().text);
}

#[test]
fn loading_a_profile_changes_only_the_tracking_list() {
    let mut app = make_test_app(1, 10);
    app.watch_list = vec!["old.exe".to_string()];
    app.watch_enabled = true;
    app.process_view_mode = ProcessViewMode::Tree;
    app.process_columns = vec![MetricColumn::CpuPercent, MetricColumn::PrivateBytes];
    app.sort.column = SortColumn::Metric(MetricColumn::CpuPercent);
    app.sort.direction = SortDirection::Asc;
    assign_private_graph(&mut app);
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.graph_time_span_seconds = 300;
    app.show_samples_panel = false;
    app.show_sample_delta = false;
    app.graph_y_axis_zero_min = false;
    app.recording_interval_index = 2;
    app.ab_comparison = Some(crate::app::AbComparison { a: None, b: None });
    app.graph_time_offset_seconds = 120;
    app.graph_show_all_samples = true;
    let graph_entries = app.graph_entries.clone();

    let mut saved = profile("API");
    saved.tracked_names = vec!["api.exe".to_string()];
    app.runtime.saved_investigation_profiles = vec![saved];

    app.open_investigation_profiles();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.investigation_profiles_dialog.is_none());
    assert_eq!(app.active_investigation_profile.as_deref(), Some("API"));
    assert_eq!(app.watch_list, ["api.exe"]);
    assert!(app.watch_enabled);
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);
    assert_eq!(
        app.process_columns,
        [MetricColumn::CpuPercent, MetricColumn::PrivateBytes]
    );
    assert_eq!(
        app.sort.column,
        SortColumn::Metric(MetricColumn::CpuPercent)
    );
    assert_eq!(app.sort.direction, SortDirection::Asc);
    assert_eq!(app.graph_entries, graph_entries);
    assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
    assert_eq!(app.graph_time_span_seconds, 300);
    assert!(!app.show_samples_panel);
    assert!(!app.show_sample_delta);
    assert!(!app.graph_y_axis_zero_min);
    assert_eq!(app.selected_recording_interval_seconds(), 5);
    assert_eq!(
        app.ab_comparison,
        Some(crate::app::AbComparison { a: None, b: None })
    );
    assert_eq!(app.graph_time_offset_seconds, 120);
    assert!(app.graph_show_all_samples);
    assert!(!app.active_investigation_profile_dirty());
}

#[test]
fn tracked_only_remains_enabled_when_a_profile_has_no_tracked_names() {
    let mut app = make_test_app(1, 10);
    app.watch_enabled = true;
    let mut saved = profile("Empty tracked-only");
    saved.tracked_names.clear();
    app.runtime.saved_investigation_profiles = vec![saved];

    app.open_investigation_profiles();
    app.load_selected_investigation_profile();

    assert!(app.watch_list.is_empty());
    assert!(app.watch_enabled);
    assert_eq!(app.visible_process_count(), 0);
}

#[test]
fn profile_dialog_changes_the_unified_startup_mode() {
    let mut app = make_test_app(1, 10);
    app.open_investigation_startup();
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Startup {
            selected: InvestigationStartup::ChooseProfile
        })
    ));
    assert_eq!(
        app.runtime.investigation_startup,
        InvestigationStartup::ResumeLast
    );
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(rendered.contains("STARTUP BEHAVIOR"), "{rendered}");
    assert!(rendered.contains("> Ask at startup"), "{rendered}");
    assert!(
        rendered.contains("Choose a tracking list at startup."),
        "{rendered}"
    );

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.runtime.investigation_startup,
        InvestigationStartup::ChooseProfile
    );
    assert!(app.investigation_profiles_dialog.is_none());
}

#[test]
fn profile_open_dialog_is_direct_and_has_no_management_shortcuts() {
    let mut app = make_test_app(1, 10);
    let mut saved = profile("monitor-winproc-tui");
    saved.tracked_names = vec![
        "winproc-tui.exe".to_string(),
        "memory-eater.exe".to_string(),
    ];
    app.runtime.saved_investigation_profiles = vec![saved];
    app.open_investigation_profiles();

    let buffer = render_app_to_buffer(&app, 76, 35);
    let rendered = super::support::buffer_to_text(&buffer);
    assert!(
        rendered.contains("OPEN INVESTIGATION PROFILE"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Profiles save tracking lists only; current Graphs stay unchanged."),
        "{rendered}"
    );
    assert!(rendered.contains("SAVED PROFILES"), "{rendered}");
    assert!(
        rendered.contains("SELECTED PROFILE · monitor-winproc-tui"),
        "{rendered}"
    );
    assert!(!rendered.contains("CURRENT INVESTIGATION"), "{rendered}");
    assert!(!rendered.contains("s Save"), "{rendered}");
    assert!(!rendered.contains("S Save New"), "{rendered}");
    assert!(!rendered.contains("u Startup"), "{rendered}");
    assert!(!rendered.contains("F2 Rename"), "{rendered}");
    assert!(rendered.contains("Delete Delete"), "{rendered}");
    assert!(!rendered.contains("Current: Unsaved"), "{rendered}");
    assert!(!rendered.contains("(*)"), "{rendered}");
    assert!(rendered.contains("winproc-tui.exe"), "{rendered}");
    assert!(rendered.contains("memory-eater.exe"), "{rendered}");
    assert!(rendered.contains("2 process names"), "{rendered}");
    for removed in [
        "┃Processes",
        "┃Tracked-only",
        "Inspector",
        "Recording",
        "┃Sort",
    ] {
        assert!(
            !rendered.contains(removed),
            "unexpected {removed}: {rendered}"
        );
    }

    let (_, profile_row) =
        find_text_position(&buffer, "> monitor-winproc-tui").expect("profile row should render");
    let (_, selected_heading_row) = find_text_position(&buffer, "SELECTED PROFILE")
        .expect("selected profile heading should render");
    assert_eq!(
        selected_heading_row,
        profile_row + 2,
        "selected profile details should follow a one-row profile list without a fixed empty gap"
    );
}

#[test]
fn empty_profile_detail_shows_only_the_empty_tracking_message() {
    let mut app = make_test_app(1, 10);
    let mut saved = profile("Empty");
    saved.tracked_names.clear();
    app.runtime.saved_investigation_profiles = vec![saved];
    app.open_investigation_profiles();

    let rendered = render_app_to_text(&app, 76, 35);
    assert!(rendered.contains("SELECTED PROFILE · Empty"), "{rendered}");
    assert!(rendered.contains("(Empty tracking list)"), "{rendered}");
    assert!(rendered.contains("0 process names"), "{rendered}");
}

#[test]
fn profile_dialog_startup_mode_has_mouse_parity() {
    let mut app = make_test_app(1, 10);
    app.open_investigation_startup();
    let screen = Rect::new(0, 0, 100, 45);
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Startup { .. })
    ));

    let mut hit = None;
    for y in 0..screen.height {
        for x in 0..screen.width {
            if crate::ui::investigation_profile_startup_at_for_screen(screen, x, y)
                == Some(InvestigationStartup::ChooseProfile)
            {
                hit = Some((x, y));
                break;
            }
        }
        if hit.is_some() {
            break;
        }
    }
    let (column, row) = hit.expect("Ask at startup should have a hit region");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert_eq!(
        app.runtime.investigation_startup,
        InvestigationStartup::ChooseProfile
    );
    assert!(app.investigation_profiles_dialog.is_none());
}

#[test]
fn profile_load_retained_history_confirmation_uses_enter_and_escape() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "old.exe");
    record_tracked_process_history_samples(&mut app, "old.exe", 180);
    let mut saved = profile("Next");
    saved.tracked_names = vec!["new.exe".to_string()];
    app.runtime.saved_investigation_profiles = vec![saved];

    app.open_investigation_profiles();
    app.load_selected_investigation_profile();

    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::ConfirmLoad { pending })
            if pending.tracking_switch.discarded_sample_count > 0
    ));
    assert_eq!(app.watch_list, ["old.exe"]);

    let rendered = render_app_to_text(&app, 100, 45);
    assert!(rendered.contains("Enter Open  Esc Cancel"), "{rendered}");

    {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.on_key(key).unwrap();
        assert!(matches!(
            app.investigation_profiles_view(),
            Some(InvestigationProfilesView::Browse)
        ));
        assert_eq!(app.watch_list, ["old.exe"]);
        app.load_selected_investigation_profile();
        assert!(matches!(
            app.investigation_profiles_view(),
            Some(InvestigationProfilesView::ConfirmLoad { .. })
        ));
    }

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.watch_list, ["new.exe"]);
}

#[test]
fn profile_load_persists_the_active_profile_and_current_investigation_immediately() {
    let mut app = make_test_app(1, 10);
    let mut saved = profile("Persisted");
    saved.tracked_names = vec!["persisted.exe".to_string()];
    app.runtime.saved_investigation_profiles = vec![saved];
    let path = unique_config_path("profile-load");
    app.runtime.config_path = Some(path.clone());

    app.open_investigation_profiles();
    app.load_selected_investigation_profile();

    let loaded = crate::config::load_config(&path).unwrap();
    assert_eq!(
        loaded
            .investigation
            .as_ref()
            .and_then(|investigation| investigation.active_profile.as_deref()),
        Some("Persisted")
    );
    let runtime = crate::config::build_runtime_config(loaded).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(runtime.active_investigation_profile, None);
    assert_eq!(runtime.process_filters, ["persisted.exe"]);
}

#[test]
fn ctrl_t_opens_profiles_and_save_load_are_rejected_outside_live() {
    let mut app = make_test_app(1, 10);
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Browse)
    ));
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(
        rendered.contains("OPEN INVESTIGATION PROFILE"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Save the current tracking list as your first profile."),
        "{rendered}"
    );
    assert!(!rendered.contains("S Save New"), "{rendered}");
    assert!(!rendered.contains("u Startup"), "{rendered}");

    app.close_investigation_profiles();
    app.log_view_path = Some("loaded.log".into());
    app.open_investigation_profiles();
    app.runtime.saved_investigation_profiles = vec![profile("Blocked")];
    app.load_selected_investigation_profile();
    assert_eq!(app.active_investigation_profile, None);
    assert!(app.status.contains("Log view"));
    app.begin_save_investigation_profile_as();
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Browse)
    ));
    assert!(app.status.contains("Log view"));
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.status.contains("Log view"));

    app.log_view_path = None;
    let path = unique_recording_path("profiles-rejected");
    let _ = std::fs::remove_file(&path);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    app.load_selected_investigation_profile();
    assert_eq!(app.active_investigation_profile, None);
    assert!(app.status.contains("Recording"));
    app.begin_save_investigation_profile_as();
    assert!(app.status.contains("Recording"));
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.status.contains("Recording"));
    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn reopening_a_modified_profile_confirms_and_cancel_preserves_changes() {
    for history in [0, 191] {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_investigation_profiles = vec![profile("Saved")];
        app.active_investigation_profile = Some("Saved".to_string());
        app.watch_list = vec!["proc-0".to_string(), "worker.exe".to_string()];
        record_tracked_process_history_samples(&mut app, "worker.exe", history);
        app.open_investigation_profiles();
        app.load_selected_investigation_profile();
        assert!(matches!(
            app.investigation_profiles_view(),
            Some(InvestigationProfilesView::ConfirmLoad { .. })
        ));
        let text = render_app_to_text(&app, 120, 60);
        assert!(text.contains("Discard unsaved tracking changes and open this profile?"));
        assert!(text.contains("worker.exe"));
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.watch_list, ["proc-0", "worker.exe"]);
        app.load_selected_investigation_profile();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.watch_list, ["proc-0"]);
        assert!(app.investigation_profiles_dialog.is_none());
    }
}

#[test]
fn equivalent_tracking_names_do_not_prompt_when_opening_a_profile() {
    let mut app = make_test_app(1, 10);
    app.watch_list = vec!["PROC-0".to_string()];
    app.runtime.saved_investigation_profiles = vec![profile("Saved")];
    app.open_investigation_profiles();
    app.load_selected_investigation_profile();
    assert!(app.investigation_profiles_dialog.is_none());
}

#[test]
fn empty_profile_browser_has_a_save_route_and_explicit_scope() {
    let mut app = make_test_app(1, 10);
    app.open_investigation_profiles();
    let rendered = render_app_to_text(&app, 120, 60);
    assert!(rendered.contains("Ctrl+S Save As"));
    assert!(rendered.contains("Profiles do not include app settings or Graphs"));
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::NameInput { .. })
    ));
    let rendered = render_app_to_text(&app, 120, 60);
    assert!(rendered.contains("Save the current tracking list as a profile."));
}

#[test]
fn profile_open_warning_keeps_scope_and_clickable_actions_at_compact_widths() {
    for width in [80, 120, 180] {
        let mut app = make_test_app(1, 10);
        app.runtime.saved_investigation_profiles = vec![profile("Saved")];
        app.active_investigation_profile = Some("Saved".to_string());
        app.watch_list = vec!["proc-0".to_string(), "worker.exe".to_string()];
        record_tracked_process_history_samples(&mut app, "worker.exe", 191);
        app.open_investigation_profiles();
        app.load_selected_investigation_profile();
        let Some(InvestigationProfilesView::ConfirmLoad { pending }) =
            app.investigation_profiles_view()
        else {
            panic!("expected unsaved changes and history confirmation");
        };
        let discarded = pending.tracking_switch.discarded_sample_count;
        assert!(discarded > 0);
        let screen = Rect::new(0, 0, width, 24);
        let buffer = render_app_to_buffer(&app, width, 24);
        let text = super::support::buffer_to_text(&buffer);
        for expected in [
            "Discard unsaved tracking changes and open this profile?".to_string(),
            "Removes 1 entry from the tracking list.".to_string(),
            format!("{discarded} older samples for 1 process name will be discarded."),
            "Removed: worker.exe".to_string(),
        ] {
            assert!(text.contains(&expected), "{expected} at {width}: {text}");
        }
        let (x, y) = find_text_position(&buffer, "Enter Open").unwrap();
        assert_eq!(buffer[(x, y - 1)].symbol(), " ");
        app.on_mouse(super::support::left_click(x + 9, y), screen);
        assert!(app.investigation_profiles_dialog.is_none());
        assert_eq!(app.watch_list, ["proc-0"]);
    }
}

#[test]
fn long_profile_names_keep_process_name_counts_visible() {
    for width in [80, 120, 180] {
        for name in ["Long profile ".repeat(20), "調査プロファイル".repeat(20)] {
            let mut app = make_test_app(1, 10);
            app.runtime.saved_investigation_profiles = vec![profile(&name)];
            app.active_investigation_profile = Some(name);
            app.open_investigation_profiles();
            let text = render_app_to_text(&app, width, 24);
            assert!(text.contains("1 process name  [current]"), "{text}");
            assert!(text.contains("..."));
        }
    }
}
