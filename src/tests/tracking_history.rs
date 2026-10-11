use super::support::{
    add_test_graph, buffer_to_text, find_text_position, make_test_app, make_test_app_with_worker,
    record_tracked_process_history_samples, render_app_to_buffer, render_app_to_text,
    selected_process_history_sample_count, test_snapshot, track_process_name,
    unique_recording_path,
};
use crate::app;
use crate::app::{
    AppActivity, DetailsMetric, FocusedPanel, GraphSlot, ProcessViewMode, ResourcePanel,
    VisibleProcessEntry,
};
use crate::model;
use crate::model::{ColumnPreset, ProcessIdentity};
use crate::samplers::{CollectSnapshotResult, SamplingWorker};
use crate::ui;
use crate::ui::main_panel_areas_for_app;
use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

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
fn ctrl_t_opens_profiles_without_toggling_tracked_only() {
    let mut app = make_test_app(1, 10);
    let tracked_only = app.watch_enabled;

    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.investigation_profiles_dialog.is_some());
    assert_eq!(app.watch_enabled, tracked_only);
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
fn shift_t_works_in_every_main_panel_and_activity_without_changing_tracking_scope() {
    for activity in [
        AppActivity::Live,
        AppActivity::Recording,
        AppActivity::LogView,
    ] {
        let mut app = make_test_app(2, 10);
        track_process_name(&mut app, "proc-0");
        add_test_graph(&mut app, 0);
        let path = unique_recording_path("global-tracked-only");
        if activity == AppActivity::Recording {
            app.recording_path_draft = path.display().to_string();
            app.show_recording_path_dialog = true;
            app.confirm_recording_path().unwrap();
        } else if activity == AppActivity::LogView {
            app.log_view_path = Some(path.clone());
        }
        let graphs = app.graph_entries.clone();
        let active = app.active_graph_id;
        for (panel, resource) in [
            (FocusedPanel::System, ResourcePanel::Memory),
            (FocusedPanel::System, ResourcePanel::Gpu),
            (FocusedPanel::SystemActivity, ResourcePanel::Memory),
            (FocusedPanel::Cpu, ResourcePanel::Memory),
            (FocusedPanel::Processes, ResourcePanel::Memory),
            (FocusedPanel::DetailsGraph, ResourcePanel::Memory),
            (FocusedPanel::DetailsSamples, ResourcePanel::Memory),
        ] {
            app.focused_panel = panel;
            app.resource_panel = resource;
            for key in [
                KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE),
            ] {
                let before = app.watch_enabled;
                app.on_key(key).unwrap();
                assert_ne!(
                    app.watch_enabled, before,
                    "{activity:?} {panel:?} {resource:?}"
                );
                assert_eq!(app.watch_list, vec!["proc-0"]);
                assert_eq!(app.focused_panel, panel);
                assert_eq!(app.resource_panel, resource);
                assert_eq!(app.graph_entries, graphs);
                assert_eq!(app.active_graph_id, active);
                assert_eq!(app.activity(), activity);
                let rendered = render_app_to_text(&app, 500, 60);
                assert!(
                    rendered
                        .lines()
                        .last()
                        .unwrap()
                        .contains("Shift+T Tracked-only")
                );
            }
        }
        if activity == AppActivity::Recording {
            app.stop_recording().unwrap();
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn shift_t_preserves_modal_and_text_input_ownership() {
    let mut app = make_test_app(2, 10);
    app.show_help = true;
    app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.show_help);
    assert!(!app.watch_enabled);
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.is_filter_editing());
    assert_eq!(app.filter_draft, "T");
    assert!(!app.watch_enabled);
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.is_main_menu_open());
    assert!(!app.watch_enabled);
}

#[test]
fn log_view_tracking_inputs_and_pending_changes_preserve_live_history() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    record_tracked_process_history_samples(&mut app, "proc-0", 191);
    let pending = app.prepare_tracked_list_switch(Vec::new());
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_tracked_remove_confirmation);

    app.log_view_path = Some(std::path::PathBuf::from("recorded.log"));
    app.confirm_tracked_remove();
    assert!(!app.show_tracked_remove_confirmation);
    app.apply_tracked_list_switch(pending);
    app.selected_process_column_index = 0;
    for key in [KeyCode::Char('t'), KeyCode::Char(' ')] {
        app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();
        assert!(!app.show_tracked_remove_confirmation);
    }
    let identity = app.visible_process_identity_at(0).unwrap();
    let now = std::time::Instant::now();
    app.register_process_tracking_cell_click(identity.clone(), model::SortColumn::ProcessName, now);
    app.register_process_tracking_cell_click(identity, model::SortColumn::ProcessName, now);
    assert!(app.show_process_info_dialog);
    app.close_process_info_dialog();

    assert_eq!(app.watch_list, vec!["proc-0"]);
    assert_eq!(selected_process_history_sample_count(&app, "proc-0"), 191);
    assert!(!app.show_tracked_remove_confirmation);
    assert!(!app.show_recording_tracking_fixed);
    let rendered = render_app_to_text(&app, 260, 60);
    let footer = rendered.lines().last().unwrap();
    assert!(!footer.contains("t Track"));
    assert!(!footer.contains("Space Track"));
    assert!(footer.contains("Shift+T Tracked-only"));
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
        rendered.contains("PROCESSES  · 0 visible · Flat(v) · ☑ Tracked-only(Shift+T)"),
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
        rendered.contains(
            "PROCESSES  · 1 visible · Flat(v) · ☑ Tracked-only(Shift+T) · [x] Show exited(Shift+E) · Filter \"target\""
        ),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Filter \"target\" · WS Priv"),
        "{rendered}"
    );
    assert!(!rendered.contains("Max samples: normal"), "{rendered}");
    assert!(rendered.contains("[x] Show exited"), "{rendered}");
    assert!(!rendered.contains("Custom"), "{rendered}");

    let (state_x, state_y) = find_text_position(&buffer, "☑ Tracked-only(Shift+T)")
        .expect("tracked-only state should be rendered");
    let state_cell = &buffer[(state_x, state_y)];
    assert_eq!(state_cell.fg, ui::THEMES[0].tracked);
    assert_ne!(state_cell.fg, ui::THEMES[0].warning);
    assert_eq!(state_cell.bg, ui::THEMES[0].panel);
    assert!(!state_cell.modifier.contains(Modifier::BOLD));

    let (label_x, label_y) =
        find_text_position(&buffer, "Tracked-only").expect("tracked-only label should be rendered");
    assert_eq!(label_y, state_y);
    assert_eq!(buffer[(label_x, label_y)].fg, ui::THEMES[0].text);
    assert!(!buffer[(label_x, label_y)].modifier.contains(Modifier::BOLD));

    let (shortcut_x, shortcut_y) =
        find_text_position(&buffer, "(Shift+T)").expect("tracked-only shortcut should be rendered");
    assert_eq!(shortcut_y, state_y);
    assert_eq!(buffer[(shortcut_x, shortcut_y)].fg, ui::THEMES[0].muted);
    assert!(
        !buffer[(shortcut_x, shortcut_y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    let (filter_x, filter_y) =
        find_text_position(&buffer, "Filter \"target\"").expect("filter state should be rendered");
    let filter_cell = &buffer[(filter_x, filter_y)];
    assert_eq!(filter_cell.fg, ui::THEMES[0].warning);
    assert_ne!(filter_cell.fg, ui::THEMES[0].tracked);
}

#[test]
fn process_table_view_mode_stays_muted_until_hovered() {
    let mut app = make_test_app(3, 10);
    app.process_view_mode = ProcessViewMode::Tree;
    app.rebuild_visible_process_cache();

    let buffer = render_app_to_buffer(&app, 100, 30);
    let (x, y) = find_text_position(&buffer, "Tree(v)").expect("Tree view state");
    assert_eq!(buffer[(x, y)].fg, app.theme().muted);
    assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
}

#[test]
fn process_table_title_omits_named_list_and_unsaved_marker() {
    let mut app = make_test_app(1, 10);
    app.watch_list = vec!["proc-0".to_string()];
    app.normalized_watch_names = ["proc-0".to_string()].into_iter().collect();

    let saved = render_app_to_text(&app, 120, 30);
    assert!(
        saved.contains("PROCESSES  · 1 visible · Flat(v) · ☐ Tracked-only(Shift+T)"),
        "{saved}"
    );
    assert!(!saved.contains("List \"API\""), "{saved}");

    app.watch_list.push("worker.exe".to_string());
    app.normalized_watch_names.insert("worker.exe".to_string());
    let dirty = render_app_to_text(&app, 120, 30);
    assert!(
        dirty.contains("PROCESSES  · 1 visible · Flat(v) · ☐ Tracked-only(Shift+T)"),
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
    let (x, y) = find_text_position(&buffer, "ta_").expect("filter input text should be rendered");
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

    assert!(app.watch_enabled);
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
    app.selected_process_column_index = 1;

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_tracked_remove_confirmation);
    assert_eq!(app.tracked_remove_name, "target.exe");
    assert_eq!(app.tracked_remove_total_samples, 121);
    assert_eq!(app.tracked_remove_discarded_samples, 1);
    assert_eq!(app.watch_list, vec!["target.exe"]);
    assert_eq!(
        selected_process_history_sample_count(&app, "target.exe"),
        121
    );

    let rendered = render_app_to_text(&app, 120, 45);
    assert!(
        rendered.contains("Remove from Tracking List?"),
        "{rendered}"
    );
    assert!(
        rendered.contains("target.exe has 121 in-memory samples."),
        "{rendered}"
    );
    assert!(
        rendered.contains("This will keep the latest 120 samples and discard 1 older samples."),
        "{rendered}"
    );
    assert!(rendered.contains("Continue?"), "{rendered}");
    assert!(rendered.contains("Enter Remove  Esc Cancel"), "{rendered}");
}

#[test]
fn tracked_remove_confirm_cancels_without_pruning() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].name = "target.exe".to_string();
    track_process_name(&mut app, "target.exe");
    record_tracked_process_history_samples(&mut app, "target.exe", 121);
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_tracked_remove_confirmation);
    assert_eq!(app.watch_list, vec!["target.exe"]);
    assert_eq!(
        selected_process_history_sample_count(&app, "target.exe"),
        121
    );
    assert_eq!(app.status, "Tracked removal canceled");
}

#[test]
fn tracked_remove_confirm_with_enter_removes_and_prunes_history() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].name = "target.exe".to_string();
    track_process_name(&mut app, "target.exe");
    record_tracked_process_history_samples(&mut app, "target.exe", 121);
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
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
    app.process_column_widths
        .set(crate::model::SortColumn::ProcessName, 18);
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
        VisibleProcessEntry::Live { .. }
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
        VisibleProcessEntry::Live { .. }
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
        VisibleProcessEntry::Live { .. }
    ));
    assert!(app.tracked_total_visible_row().is_some());
}

#[test]
fn latest_same_name_ghost_is_the_only_visible_ghost() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    app.snapshot.processes[0].name = "target.exe".to_string();
    app.add_selected_process_to_watch_list();
    let first_identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let captured_at = app.snapshot.captured_at;
    app.process_history.record_snapshot(
        captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.sampling_in_progress = true;

    let mut first_exit = test_snapshot(0);
    first_exit.captured_at = captured_at + chrono::Duration::seconds(1);
    result_tx
        .send(CollectSnapshotResult {
            snapshot: first_exit,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    app.sampling_in_progress = true;
    let mut restarted = test_snapshot(1);
    restarted.captured_at = captured_at + chrono::Duration::seconds(2);
    restarted.processes[0].name = "target.exe".to_string();
    restarted.processes[0].pid = 42;
    restarted.processes[0].start_time = Some(1_800_000_000);
    let restarted_identity = ProcessIdentity::from_row(&restarted.processes[0]);
    result_tx
        .send(CollectSnapshotResult {
            snapshot: restarted,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    app.sampling_in_progress = true;
    let mut second_exit = test_snapshot(0);
    second_exit.captured_at = captured_at + chrono::Duration::seconds(3);
    result_tx
        .send(CollectSnapshotResult {
            snapshot: second_exit,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    app.sampling_in_progress = true;
    let mut restarted_again = test_snapshot(1);
    restarted_again.captured_at = captured_at + chrono::Duration::seconds(4);
    restarted_again.processes[0].name = "target.exe".to_string();
    restarted_again.processes[0].pid = 43;
    restarted_again.processes[0].start_time = Some(1_800_000_001);
    let latest_identity = ProcessIdentity::from_row(&restarted_again.processes[0]);
    result_tx
        .send(CollectSnapshotResult {
            snapshot: restarted_again,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    app.sampling_in_progress = true;
    let mut third_exit = test_snapshot(0);
    third_exit.captured_at = captured_at + chrono::Duration::seconds(5);
    result_tx
        .send(CollectSnapshotResult {
            snapshot: third_exit,
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
    assert_eq!(app.process_history.identity_count(), 2);
    assert_eq!(app.process_history.peak_count(), 2);
    assert_eq!(ghost_count, 1);
    assert_eq!(app.visible_process_at(0).unwrap().pid, 43);
    assert_eq!(app.process_history.sample_count_for(&first_identity), 0);
    assert_eq!(app.process_history.sample_count_for(&restarted_identity), 1);
    assert_eq!(app.process_history.sample_count_for(&latest_identity), 1);
}

#[test]
fn registered_graph_retains_older_tracked_identity_after_restart() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    app.snapshot.processes[0].name = "target.exe".to_string();
    app.add_selected_process_to_watch_list();
    let graph_identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(graph_identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    ));

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
    let restarted_identity = ProcessIdentity::from_row(&restarted.processes[0]);
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

    assert_eq!(app.exited_tracked_rows.len(), 2);
    assert_eq!(app.process_history.identity_count(), 2);
    assert_eq!(app.process_history.peak_count(), 2);
    assert_eq!(app.process_history.sample_count_for(&graph_identity), 1);
    assert_eq!(app.process_history.sample_count_for(&restarted_identity), 1);
    assert!(app.process_history.peak_for(&graph_identity).is_some());
    assert!(app.process_history.peak_for(&restarted_identity).is_some());
}

#[test]
fn paused_process_identity_remains_available_for_a_later_graph() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let captured_at = app.snapshot.captured_at;
    app.process_history.record_snapshot(
        captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.toggle_display_pause();

    let mut exited = test_snapshot(0);
    exited.captured_at = captured_at
        + chrono::Duration::seconds(model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as i64 + 1);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: exited,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert_eq!(app.process_history.sample_count_for(&identity), 1);
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
    app.toggle_display_pause();

    let mut later = test_snapshot(0);
    later.captured_at = captured_at
        + chrono::Duration::seconds(model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as i64 * 2);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: later,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert_eq!(app.process_history.sample_count_for(&identity), 1);
    assert!(app.process_history.peak_for(&identity).is_some());
}

#[test]
fn paused_ghost_identity_remains_available_for_a_later_graph() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    app.snapshot.processes[0].name = "target.exe".to_string();
    app.add_selected_process_to_watch_list();
    let old_identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let captured_at = app.snapshot.captured_at;
    app.process_history.record_snapshot(
        captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    let mut first_exit = test_snapshot(0);
    first_exit.captured_at = captured_at + chrono::Duration::seconds(1);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: first_exit,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();
    assert!(app.exited_tracked_rows.contains_key(&old_identity));
    app.toggle_display_pause();

    let mut restarted = test_snapshot(1);
    restarted.captured_at = captured_at + chrono::Duration::seconds(2);
    restarted.processes[0].name = "target.exe".to_string();
    restarted.processes[0].pid = 42;
    restarted.processes[0].start_time = Some(1_800_000_000);
    let restarted_identity = ProcessIdentity::from_row(&restarted.processes[0]);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: restarted,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    let mut second_exit = test_snapshot(0);
    second_exit.captured_at = captured_at + chrono::Duration::seconds(3);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: second_exit,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    let mut expired = test_snapshot(0);
    expired.captured_at = captured_at
        + chrono::Duration::seconds(model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as i64 * 2);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: expired,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert!(app.exited_tracked_rows.contains_key(&old_identity));
    assert!(app.exited_tracked_rows.contains_key(&restarted_identity));
    assert_eq!(app.process_history.sample_count_for(&old_identity), 1);
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(old_identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
    app.toggle_display_pause();

    let mut later = test_snapshot(0);
    later.captured_at = captured_at
        + chrono::Duration::seconds(model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as i64 * 3);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: later,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert_eq!(app.process_history.sample_count_for(&old_identity), 1);
    assert!(app.process_history.peak_for(&old_identity).is_some());
}

#[test]
fn live_process_churn_prunes_stale_histories_and_peaks() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    let captured_at = app.snapshot.captured_at;
    let mut first_identity = None;
    let mut previous_identity = None;
    let mut latest_identity = None;

    for identity_index in 0..256_u32 {
        let mut next = test_snapshot(1);
        next.captured_at = captured_at + chrono::Duration::seconds(i64::from(identity_index) + 1);
        next.processes[0].pid = 10_000 + identity_index;
        next.processes[0].start_time = Some(1_800_000_000 + u64::from(identity_index));
        next.processes[0].private_bytes = Some(u64::from(identity_index));
        let identity = ProcessIdentity::from_row(&next.processes[0]);
        first_identity.get_or_insert_with(|| identity.clone());
        previous_identity = latest_identity.replace(identity);
        app.sampling_in_progress = true;
        result_tx
            .send(CollectSnapshotResult {
                snapshot: next,
                warning: None,
            })
            .unwrap();
        app.poll_sample_results().unwrap();
    }

    let first_identity = first_identity.unwrap();
    let previous_identity = previous_identity.unwrap();
    let latest_identity = latest_identity.unwrap();
    assert_eq!(app.process_history.identity_count(), 2);
    assert_eq!(app.process_history.peak_count(), 2);
    assert_eq!(app.process_history.len(), 2);
    assert_eq!(app.process_history.sample_count_for(&first_identity), 0);
    assert!(app.process_history.peak_for(&first_identity).is_none());
    assert_eq!(app.process_history.sample_count_for(&previous_identity), 1);
    assert!(app.process_history.peak_for(&previous_identity).is_some());
    assert_eq!(app.process_history.sample_count_for(&latest_identity), 1);
    assert!(app.process_history.peak_for(&latest_identity).is_some());
}

#[test]
fn concurrent_same_name_live_processes_are_all_retained() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    let mut next = test_snapshot(3);
    next.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);
    for (index, process) in next.processes.iter_mut().enumerate() {
        process.name = "worker.exe".to_string();
        process.pid = 1_000 + index as u32;
        process.start_time = Some(1_800_000_000 + index as u64);
    }
    let identities = next
        .processes
        .iter()
        .map(ProcessIdentity::from_row)
        .collect::<Vec<_>>();

    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert_eq!(app.process_history.identity_count(), 3);
    assert_eq!(app.process_history.peak_count(), 3);
    assert!(
        identities
            .iter()
            .all(|identity| app.process_history.sample_count_for(identity) == 1)
    );
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
fn show_exited_filters_without_discarding_retained_rows() {
    let (worker, _requests, results) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, worker);
    app.add_selected_process_to_watch_list();
    app.sampling_in_progress = true;
    results
        .send(CollectSnapshotResult {
            snapshot: test_snapshot(0),
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();
    assert_eq!(app.visible_process_count(), 1);
    app.focused_panel = FocusedPanel::Processes;
    app.on_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.visible_process_count(), 0);
    assert_eq!(app.exited_tracked_rows.len(), 1);
    assert!(!app.watch_enabled);
    app.toggle_show_exited();
    assert_eq!(app.visible_process_count(), 1);
    app.process_view_mode = ProcessViewMode::Tree;
    app.toggle_show_exited();
    assert_eq!(app.visible_process_count(), 0);
    app.toggle_show_exited();
    assert_eq!(app.visible_process_count(), 1);
}

#[test]
fn show_exited_mouse_matches_header_and_updates_paused_hover() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 150, 40);
    app.toggle_display_pause();
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position(&buffer, "[x] Show exited").unwrap();
    let outcome = app::handle_mouse_event(&mut app, super::support::mouse_move(x, y), screen);
    assert!(outcome.dirty);
    assert!(app.process_show_exited_hovered);
    app.on_mouse(super::support::left_click(x, y), screen);
    assert!(!app.show_exited);
}
