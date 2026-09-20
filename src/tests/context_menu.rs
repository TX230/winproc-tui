use super::support::{
    add_test_graph, assign_private_graph, find_text_position, make_test_app, render_app_to_buffer,
};
use crate::{
    app::{self, FocusedPanel},
    model::ProcessIdentity,
    ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

fn menu_key(app: &mut crate::App) {
    app.on_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT))
        .unwrap();
}
fn enter(app: &mut crate::App) {
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

#[test]
fn show_graph_captures_process_lifetime_and_reveals_existing_source() {
    let mut app = make_test_app(2, 10);
    let screen = Rect::new(0, 0, 180, 60);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (x, _) = find_text_position(&buffer, "PrivBytes").unwrap();
    let table = ui::main_panel_areas_for_app(screen, &app).processes.area;
    let identity = app.visible_process_identity_at(0).unwrap();
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: x,
            row: table.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    let menu = app.context_menu.clone().expect("menu");
    assert_eq!(menu.items[0].label, "Show graph");
    assert!(app.graph_entries.is_empty());
    assert!(app.watch_list.is_empty());
    app.snapshot.processes[0].start_time = Some(99999);
    enter(&mut app);
    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(
        app.graph_entries[0].source.process_identity(),
        Some(&identity)
    );
    let id = app.graph_entries[0].id;
    app.show_details = false;
    app.open_context_menu(menu);
    enter(&mut app);
    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(app.active_graph_id, Some(id));
    assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
    assert!(app.show_details);
}

#[test]
fn context_menu_limits_and_process_info_keep_fixed_targets() {
    let mut app = make_test_app(2, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.selected_process_column_index = 1;
    let identity = ProcessIdentity::from_row(app.selected_visible_process().unwrap());
    menu_key(&mut app);
    app.snapshot.processes[0].start_time = Some(123456);
    app.process_table_state.select(Some(1));
    enter(&mut app);
    assert_eq!(app.process_info_target.as_ref().unwrap().identity, identity);
    app.close_process_info_dialog();
    for i in 0..16 {
        add_test_graph(&mut app, i);
    }
    app.focused_panel = FocusedPanel::Processes;
    app.selected_process_column_index = 2;
    menu_key(&mut app);
    assert!(!app.context_menu.as_ref().unwrap().items[0].enabled);
    enter(&mut app);
    assert_eq!(app.graph_entries.len(), 16);
    assert!(app.context_menu.is_some());
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.context_menu.is_none());
    app.log_view_path = Some("recorded.log".into());
    app.selected_process_column_index = 1;
    menu_key(&mut app);
    let menu = app.context_menu.as_ref().unwrap();
    assert!(
        menu.items
            .iter()
            .any(|i| i.label == "Track process name" && !i.enabled)
    );
    assert!(menu.items.iter().any(|i| i.label == "Files" && !i.enabled));
}

#[test]
fn graph_menu_preserves_time_until_action_and_uses_captured_sample() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    for offset in [0, 30, 60] {
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    app.set_details_sample_selected_manual(0);
    app.toggle_graph_all_samples();
    let time = app.selected_details_sample_time().unwrap();
    app.focused_panel = FocusedPanel::DetailsSamples;
    menu_key(&mut app);
    assert!(app.ab_comparison.is_none());
    assert!(app.graph_show_all_samples);
    assert_eq!(app.selected_details_sample_time(), Some(time));
    app.set_details_sample_selected_manual(2);
    enter(&mut app);
    assert_eq!(
        app.ab_comparison.as_ref().unwrap().a.unwrap().captured_at,
        time
    );
    let sample = app.graph_slot_sample_at(app.active_graph_slot().unwrap(), 0);
    let mut menu = app
        .graph_context_menu(app.active_graph_id.unwrap(), sample, Position::new(179, 59))
        .unwrap();
    menu.selected = menu.items.len() - 1;
    let screen = Rect::new(0, 0, 180, 60);
    let area = ui::context_menu::layout(screen, &menu).area;
    assert!(area.right() <= 180 && area.bottom() <= 60);
    app.open_context_menu(menu);
    app.on_mouse(super::support::left_click(0, 0), screen);
    assert!(app.context_menu.is_none());
    assert_eq!(app.graph_entries.len(), 1);
}
