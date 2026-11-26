use super::support::{add_test_graph, make_test_app, test_graph_source};
use crate::app;
use crate::app::{DetailsMetric, FocusedPanel, GraphDisplayMode, GraphSlot, GraphSlotLayout};
use crate::model::{ProcessIdentity, SystemMetric};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

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
fn graph_display_modes_are_per_entry_and_follow_stable_graph_ids() {
    let mut app = make_test_app(1, 10);
    let ids = (0..3)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.set_active_graph(ids[0]));
    app.details_sample_selected = 7;
    app.graph_time_offset_seconds = 30;
    app.ab_comparison = Some(app::AbComparison { a: None, b: None });

    assert!(app.toggle_graph_display_mode(ids[1]));

    assert_eq!(app.active_graph_id, Some(ids[0]));
    assert_eq!(app.details_sample_selected, 7);
    assert_eq!(app.graph_time_offset_seconds, 30);
    assert_eq!(
        app.graph_entry_by_id(ids[1]).unwrap().display_mode,
        GraphDisplayMode::MovingAverage5
    );
    assert_eq!(
        app.graph_entry_by_id(ids[0]).unwrap().display_mode,
        GraphDisplayMode::Raw
    );
    assert_eq!(
        app.graph_entry_by_id(ids[2]).unwrap().display_mode,
        GraphDisplayMode::Raw
    );

    assert!(app.set_active_graph(ids[1]));
    assert!(app.move_active_graph_earlier());
    app.set_screen_area(Rect::new(0, 0, 80, 40));
    assert_eq!(
        app.graph_entry_by_id(ids[1]).unwrap().display_mode,
        GraphDisplayMode::MovingAverage5
    );

    assert!(app.remove_graph(ids[1]));
    assert!(app.graph_entry_by_id(ids[1]).is_none());
    assert!(
        app.graph_entries
            .iter()
            .all(|entry| entry.display_mode == GraphDisplayMode::Raw)
    );
}

#[test]
fn keyboard_toggles_only_the_active_graph_display_mode() {
    let mut app = make_test_app(1, 10);
    let ids = (0..2)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.set_active_graph(ids[0]));
    app.focused_panel = FocusedPanel::DetailsGraph;

    app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.graph_entry_by_id(ids[0]).unwrap().display_mode,
        GraphDisplayMode::MovingAverage5
    );
    assert_eq!(
        app.graph_entry_by_id(ids[1]).unwrap().display_mode,
        GraphDisplayMode::Raw
    );
}

#[test]
fn display_mode_toggle_does_not_change_raw_history_or_comparison_state() {
    let mut app = make_test_app(1, 10);
    let tracked = std::collections::HashSet::new();
    for offset in 0..6 {
        let captured_at = app.snapshot.captured_at + jiff::SignedDuration::from_secs(offset);
        app.process_history
            .record_snapshot(captured_at, &app.snapshot.processes, &tracked);
    }
    let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let source = GraphSlot::process(identity, DetailsMetric::Private);
    assert!(app.add_or_reveal_graph_source(source.clone(), FocusedPanel::Processes));
    let id = app.active_graph_id.unwrap();
    let before = app.graph_slot_samples(&source);
    app.ab_comparison = Some(app::AbComparison { a: None, b: None });

    assert!(app.toggle_graph_display_mode(id));

    assert_eq!(app.graph_slot_samples(&source), before);
    assert_eq!(
        app.ab_comparison,
        Some(app::AbComparison { a: None, b: None })
    );
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
            .expect("registered source should have display state");
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
    for (active_index, remove_index, expected_active_index) in [(0, 0, 1), (2, 2, 3), (4, 4, 3)] {
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
