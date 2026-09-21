use super::support::{
    add_test_graph, area_contains_foreground, assign_private_graph, make_test_app,
    make_test_app_with_worker, make_test_app_with_workers, render_app_to_buffer,
    render_app_to_text, test_snapshot,
};
use crate::app;
use crate::app::{DetailsMetric, FocusedPanel, GraphSlot, GraphSlotLayout};
use crate::model::{ProcessHistory, ProcessIdentity, SystemHistory, SystemMetric};
use crate::samplers::open_files::{OpenFilesRequest, OpenFilesWorker};
use crate::samplers::process_info::ProcessInfoWorker;
use crate::samplers::{CollectSnapshotResult, SamplingWorker};
use crate::ui;
use crate::ui::{
    GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, details_graph_area_for_app,
    details_samples_area_for_app, details_shared_controls_area_for_app, main_panel_areas_for_app,
};
use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

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
fn ab_summary_and_resize_keep_selected_sample_visible_and_clickable() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    for offset in 0..60 {
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    app.focused_panel = FocusedPanel::DetailsGraph;
    let screen = Rect::new(0, 0, 160, 48);
    app::sync_layout_state(&mut app, screen);
    app.select_details_sample_oldest();
    app.set_ab_point_a();
    app::sync_layout_state(&mut app, screen);
    app.select_details_sample_latest();
    let selected_time = app.selected_details_sample_time().unwrap();
    let old_capacity = app.details_sample_page_size;
    app.set_ab_point_b();
    app::sync_layout_state(&mut app, screen);
    assert!(app.details_sample_page_size < old_capacity);

    for screen in [screen, Rect::new(0, 0, 160, 36), Rect::new(0, 0, 160, 55)] {
        app::sync_layout_state(&mut app, screen);
        assert!(app.details_sample_selected >= app.details_sample_offset);
        assert!(
            app.details_sample_selected < app.details_sample_offset + app.details_sample_page_size
        );
        let area = details_samples_area_for_app(screen, &app).unwrap();
        let row = area.y + 1 + (app.details_sample_selected - app.details_sample_offset) as u16;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let time = selected_time.format("%H:%M:%S").to_string();
        assert!(
            super::support::find_text_position_in_area(
                &buffer,
                Rect::new(area.x, row, area.width, 1),
                &time
            )
            .is_some()
        );
        app.on_mouse(super::support::left_click(area.x + 5, row), screen);
        assert_eq!(app.selected_details_sample_time(), Some(selected_time));
    }
    app.clear_ab_comparison_with_status();
    app::sync_layout_state(&mut app, screen);
    assert_eq!(app.selected_details_sample_time(), Some(selected_time));
    assert!(app.details_sample_selected < app.details_sample_offset + app.details_sample_page_size);
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
fn graph_and_samples_scrollbar_thumbs_follow_their_content_focus() {
    let screen = Rect::new(0, 0, 160, 48);
    let mut graph_app = make_test_app(1, 10);
    for index in 0..8 {
        add_test_graph(&mut graph_app, index);
    }
    graph_app.graph_slot_layout = GraphSlotLayout::OneColumn;
    graph_app.show_samples_panel = false;
    graph_app.focused_panel = FocusedPanel::DetailsGraph;
    app::sync_layout_state(&mut graph_app, screen);
    let details = main_panel_areas_for_app(screen, &graph_app)
        .details
        .unwrap();
    let graph_scrollbar = ui::layout::graph_workspace_layout(details, &graph_app)
        .graph_scrollbar
        .expect("graph scrollbar");
    let focused_graph = render_app_to_buffer(&graph_app, screen.width, screen.height);
    assert!(area_contains_foreground(
        &focused_graph,
        graph_scrollbar,
        graph_app.theme().focus_border
    ));

    graph_app.focused_panel = FocusedPanel::Processes;
    let inactive_graph = render_app_to_buffer(&graph_app, screen.width, screen.height);
    assert!(!area_contains_foreground(
        &inactive_graph,
        graph_scrollbar,
        graph_app.theme().focus_border
    ));
    assert!(area_contains_foreground(
        &inactive_graph,
        graph_scrollbar,
        graph_app.theme().muted
    ));

    let mut samples_app = make_test_app(1, 10);
    assign_private_graph(&mut samples_app);
    let tracked_names = ["proc-0".to_string()].into_iter().collect();
    for offset in 0..100 {
        samples_app.process_history.record_snapshot(
            samples_app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &samples_app.snapshot.processes,
            &tracked_names,
        );
    }
    samples_app.show_samples_panel = true;
    samples_app.focused_panel = FocusedPanel::DetailsSamples;
    samples_app.select_details_sample_latest();
    app::sync_layout_state(&mut samples_app, screen);
    let details = main_panel_areas_for_app(screen, &samples_app)
        .details
        .unwrap();
    let samples = ui::layout::graph_workspace_layout(details, &samples_app)
        .samples
        .expect("Samples inspector");
    let samples_content = samples.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    let focused_samples = render_app_to_buffer(&samples_app, screen.width, screen.height);
    assert!(area_contains_foreground(
        &focused_samples,
        samples_content,
        samples_app.theme().focus_border
    ));

    samples_app.focused_panel = FocusedPanel::DetailsGraph;
    let inactive_samples = render_app_to_buffer(&samples_app, screen.width, screen.height);
    assert!(!area_contains_foreground(
        &inactive_samples,
        samples_content,
        samples_app.theme().focus_border
    ));
    assert!(area_contains_foreground(
        &inactive_samples,
        samples_content,
        samples_app.theme().muted
    ));
}

#[test]
fn samples_scrollbar_keeps_one_column_gap_after_values() {
    let screen = Rect::new(0, 0, 160, 48);
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    let tracked_names = ["proc-0".to_string()].into_iter().collect();
    for offset in 0..100 {
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &tracked_names,
        );
    }
    app.show_samples_panel = true;
    app.focused_panel = FocusedPanel::DetailsSamples;
    app.select_details_sample_latest();

    for show_delta in [false, true] {
        app.show_sample_delta = show_delta;
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let samples = ui::layout::graph_workspace_layout(details, &app)
            .samples
            .expect("Samples inspector");
        let content = samples.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let scrollbar_x = content.right().saturating_sub(1);
        let sample_row_y = content.y.saturating_add(1);

        assert_ne!(
            buffer[(scrollbar_x.saturating_sub(2), sample_row_y)].symbol(),
            " "
        );
        assert_eq!(
            buffer[(scrollbar_x.saturating_sub(1), sample_row_y)].symbol(),
            " "
        );
        assert_ne!(buffer[(scrollbar_x, sample_row_y)].symbol(), " ");
    }
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

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 8);

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 0);

    app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.graph_time_span_seconds, 120);
}

#[test]
fn alt_arrows_pan_the_shared_range_from_graph_and_samples() {
    for focus in [FocusedPanel::DetailsGraph, FocusedPanel::DetailsSamples] {
        let mut app = make_test_app(1, 10);
        assign_private_graph(&mut app);
        for offset in 0..=120 {
            app.process_history.record_snapshot(
                app.snapshot.captured_at + chrono::Duration::seconds(offset),
                &app.snapshot.processes,
                &app.normalized_watch_names,
            );
        }
        app.graph_time_span_seconds = 60;
        app.select_details_sample_latest();
        app.focused_panel = focus;
        let selected = app.details_sample_selected;
        let selected_time = app.selected_details_sample_time();
        let active = app.active_graph_id;
        let latest = app.snapshot.captured_at + chrono::Duration::seconds(120);
        app.ab_comparison = Some(app::AbComparison {
            a: Some(app::AbComparisonPoint {
                captured_at: latest - chrono::Duration::seconds(60),
            }),
            b: Some(app::AbComparisonPoint {
                captured_at: latest,
            }),
        });
        let ab = app.ab_comparison.clone();
        let modifiers = KeyModifiers::ALT;

        app.on_key(KeyEvent::new(KeyCode::Left, modifiers)).unwrap();
        assert_eq!(app.graph_time_offset_seconds, 8, "{focus:?}");
        assert_eq!(
            app.graph_time_window_right_at,
            Some(latest - chrono::Duration::seconds(8))
        );
        assert!(!app.details_live);
        assert_eq!(app.graph_time_span_seconds, 60);
        assert_eq!(app.details_sample_selected, selected);
        assert_eq!(app.selected_details_sample_time(), selected_time);
        assert_eq!(app.active_graph_id, active);
        assert_eq!(app.ab_comparison, ab);
        assert_eq!(app.focused_panel, focus);

        app.on_key(KeyEvent::new(KeyCode::Right, modifiers))
            .unwrap();
        assert_eq!(app.graph_time_offset_seconds, 0);
        assert!(app.graph_time_window_right_at.is_none());
        assert!(app.details_live);
        assert_eq!(app.graph_time_span_seconds, 60);
        assert_eq!(app.details_sample_selected, selected);
        assert_eq!(app.selected_details_sample_time(), selected_time);
        assert_eq!(app.active_graph_id, active);
        assert_eq!(app.ab_comparison, ab);
        assert_eq!(app.focused_panel, focus);
    }
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
fn shift_up_down_reorders_active_graph_without_changing_shared_state() {
    let mut app = make_test_app(1, 10);
    let ids = (0..4)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.set_active_graph(ids[1]));
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.details_sample_selected = 7;
    app.details_sample_offset = 3;
    app.graph_time_span_seconds = 300;
    app.graph_time_offset_seconds = 42;
    app.graph_time_window_right_at = Some(Local::now());
    app.ab_comparison = Some(app::AbComparison { a: None, b: None });
    let window_right = app.graph_time_window_right_at;
    let comparison = app.ab_comparison.clone();

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(
        app.graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [ids[0], ids[2], ids[1], ids[3]]
    );
    assert_eq!(app.active_graph_id, Some(ids[1]));
    assert_eq!(app.details_sample_selected, 7);
    assert_eq!(app.details_sample_offset, 3);
    assert_eq!(app.graph_time_span_seconds, 300);
    assert_eq!(app.graph_time_offset_seconds, 42);
    assert_eq!(app.graph_time_window_right_at, window_right);
    assert_eq!(app.ab_comparison, comparison);

    app.focused_panel = FocusedPanel::DetailsSamples;
    app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        app.graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(app.active_graph_id, Some(ids[1]));
}

#[test]
fn graph_reorder_dialog_applies_draft_and_escape_discards_it() {
    let mut app = make_test_app(1, 10);
    let ids = (0..4)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.set_active_graph(ids[2]));
    app.focused_panel = FocusedPanel::DetailsSamples;
    let sort = app.sort;

    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.graph_reorder_dialog.is_some());
    assert_eq!(app.sort, sort);
    app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        app.graph_reorder_dialog.as_ref().unwrap().order,
        [ids[0], ids[2], ids[1], ids[3]]
    );
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.graph_reorder_dialog.is_none());
    assert_eq!(
        app.graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );

    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.graph_reorder_dialog.is_none());
    assert_eq!(
        app.graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [ids[0], ids[2], ids[1], ids[3]]
    );
    assert_eq!(app.active_graph_id, Some(ids[2]));
    assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
}

#[test]
fn graph_reorder_dialog_scrolls_to_selected_row_on_short_screens() {
    let mut app = make_test_app(1, 10);
    for index in 0..app::GRAPH_LIMIT {
        add_test_graph(&mut app, index);
    }
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.open_graph_reorder_dialog();
    let screen = Rect::new(0, 0, 90, 10);

    app::sync_layout_state(&mut app, screen);
    let rendered = render_app_to_text(&app, screen.width, screen.height);

    assert!(app.graph_reorder_dialog.as_ref().unwrap().scroll.offset > 0);
    assert!(rendered.contains("REORDER GRAPHS"), "{rendered}");
    assert!(rendered.contains("graph-15.exe"), "{rendered}");
    assert!(rendered.contains("Shift+↑/↓ Move"), "{rendered}");
    assert!(rendered.contains('█'), "{rendered}");
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

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 120);

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 0);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 120);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 128);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.graph_time_offset_seconds, 136);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
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
    assert!(app.context_menu.is_none());
}

#[test]
fn graph_right_click_after_drag_preserves_panned_range() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    for offset in [0, 30, 60, 90, 120, 150, 180, 210, 240] {
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    app.select_details_sample_latest();
    let screen = Rect::new(0, 0, 120, 45);
    let graph = details_graph_area_for_app(screen, &app).unwrap();
    let start_x = graph.x.saturating_add(20);
    let end_x = start_x.saturating_add(40);
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
            column: end_x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            column: end_x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    let panned_offset = app.graph_time_offset_seconds;
    assert!(panned_offset > 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: end_x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            column: end_x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert_eq!(app.graph_time_offset_seconds, panned_offset);
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
fn graph_right_click_without_drag_preserves_fit_all_samples() {
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

    assert!(app.graph_show_all_samples);
    assert_eq!(app.effective_graph_time_span_seconds(), 240);
    assert!(app.graph_pan_drag.is_none());
    assert!(app.context_menu.is_some());
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
fn graph_fit_all_uses_the_time_range_across_every_graph() {
    let mut app = make_test_app(1, 10);
    let base = app.snapshot.captured_at;
    app.process_history = ProcessHistory::default();
    app.system_history = SystemHistory::default();

    for offset in [120, 240] {
        app.process_history.record_snapshot_unbounded(
            base + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
        );
    }
    for offset in [0, 360] {
        let mut snapshot = app.snapshot.clone();
        snapshot.captured_at = base + chrono::Duration::seconds(offset);
        snapshot.committed_memory = Some(1_000 + offset as u64);
        app.system_history.record_snapshot_unbounded(&snapshot);
    }

    assign_private_graph(&mut app);
    let process_graph = app.active_graph_id.unwrap();
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::system(SystemMetric::Committed),
        FocusedPanel::System,
    ));
    app.toggle_graph_all_samples();

    assert_eq!(app.effective_graph_time_span_seconds(), 360);
    assert_eq!(
        app.graph_time_reference_at(),
        Some(base + chrono::Duration::seconds(360))
    );

    assert!(app.set_active_graph(process_graph));
    assert_eq!(app.effective_graph_time_span_seconds(), 360);
    assert_eq!(
        app.graph_time_reference_at(),
        Some(base + chrono::Duration::seconds(360))
    );
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
fn log_view_panel_titles_omit_history_counts() {
    let mut app = make_test_app(1, 10);
    app.log_view_path = Some(std::path::PathBuf::from("long.log"));
    app.process_history = ProcessHistory::default();
    app.system_history = SystemHistory::default();
    for offset in 0..=7_200 {
        app.snapshot.captured_at += chrono::Duration::seconds(i64::from(offset));
        app.process_history
            .record_snapshot_unbounded(app.snapshot.captured_at, &app.snapshot.processes);
        app.system_history.record_snapshot_unbounded(&app.snapshot);
    }

    let rendered = render_app_to_text(&app, 120, 30);

    assert!(!rendered.contains("[Samples:"), "{rendered}");
    assert!(
        rendered.contains(
            "PROCESSES  · 1 visible · Flat (Tree unavailable in LOG) · ☐ Tracked-only(Shift+T)"
        ),
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
    assert!(rendered.contains("l: 1/2 cols"), "{rendered}");

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.graph_slot_layout, GraphSlotLayout::ThreeColumns);
    assert!(app.show_samples_panel);
    let rendered = render_app_to_text(&app, 180, 60);
    assert!(rendered.contains("l: 2/3 cols"), "{rendered}");

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

    let extra_wide = Rect::new(0, 0, 220, 80);
    app::sync_layout_state(&mut app, extra_wide);
    let details = main_panel_areas_for_app(extra_wide, &app).details.unwrap();
    assert_eq!(ui::layout::graph_workspace_layout(details, &app).columns, 3);

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
            GraphSlotLayout::ThreeColumns,
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
            let expected_columns = match mode {
                GraphSlotLayout::OneColumn => 1,
                GraphSlotLayout::TwoColumns => count.min(2),
                GraphSlotLayout::ThreeColumns | GraphSlotLayout::Auto => count.min(3),
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
                rendered.contains(&format!("GRAPHS  · {count} {slot_label} · Span 60s")),
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
fn ctrl_arrows_follow_visible_panels_without_mutating_content() {
    let mut app = make_test_app(30, 10);
    let first = add_test_graph(&mut app, 0);
    add_test_graph(&mut app, 1);
    app.select_graph(first);
    let selected = app.process_table_state.selected();
    let offset = app.graph_time_offset_seconds;
    for screen in [
        Rect::new(0, 0, 180, 60),
        Rect::new(0, 0, 90, 60),
        Rect::new(0, 0, 180, 60),
    ] {
        for show_samples in [true, false] {
            app.show_samples_panel = show_samples;
            app.focused_panel = FocusedPanel::Processes;
            app::sync_layout_state(&mut app, screen);
            app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
                .unwrap();
            assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
            let details = ui::main_panel_areas_for_app(screen, &app).details.unwrap();
            let layout = ui::layout::graph_workspace_layout(details, &app);
            if let Some(samples) = layout.samples {
                let key = if samples.x >= layout.graph_slots.right() {
                    KeyCode::Right
                } else {
                    KeyCode::Down
                };
                app.on_key(KeyEvent::new(key, KeyModifiers::CONTROL))
                    .unwrap();
                assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
                let back = if key == KeyCode::Right {
                    KeyCode::Left
                } else {
                    KeyCode::Up
                };
                app.on_key(KeyEvent::new(back, KeyModifiers::CONTROL))
                    .unwrap();
                assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
            }
            app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
                .unwrap();
            assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
            app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL))
                .unwrap();
            assert_eq!(app.focused_panel, FocusedPanel::Processes);
            assert_eq!(app.active_graph_id, Some(first));
            assert_eq!(app.process_table_state.selected(), selected);
            assert_eq!(app.graph_time_offset_seconds, offset);
        }
    }
    app.show_details = false;
    app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 60));
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.focused_panel, FocusedPanel::Processes);
    app.focused_panel = FocusedPanel::System;
    app.select_resource_panel(app::ResourcePanel::Memory);
    for (key, panel, resource) in [
        (
            KeyCode::Left,
            FocusedPanel::System,
            app::ResourcePanel::Memory,
        ),
        (
            KeyCode::Right,
            FocusedPanel::System,
            app::ResourcePanel::Gpu,
        ),
        (
            KeyCode::Right,
            FocusedPanel::SystemActivity,
            app::ResourcePanel::Gpu,
        ),
        (KeyCode::Right, FocusedPanel::Cpu, app::ResourcePanel::Gpu),
        (KeyCode::Up, FocusedPanel::Cpu, app::ResourcePanel::Gpu),
        (
            KeyCode::Down,
            FocusedPanel::Processes,
            app::ResourcePanel::Gpu,
        ),
    ] {
        app.on_key(KeyEvent::new(key, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.focused_panel, panel);
        assert_eq!(app.resource_panel, resource);
    }
}
