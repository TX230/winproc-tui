use super::support::{
    find_text_position, make_test_app_with_worker, render_app_to_buffer, test_snapshot,
    unique_recording_path,
};
use crate::{
    app,
    model::{ProcessIdentity, ProcessRow},
    samplers::{CollectSnapshotResult, SamplingWorker},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn rows() -> Vec<ProcessRow> {
    [
        (1, None, "root.exe"),
        (2, Some(1), "worker.exe"),
        (3, Some(2), "helper.exe"),
        (4, None, "worker.exe"),
    ]
    .into_iter()
    .map(|(pid, parent_pid, name)| ProcessRow {
        pid,
        parent_pid,
        name: name.into(),
        start_time: Some(pid as u64),
        private_bytes: Some(pid as u64 * 10),
        ..Default::default()
    })
    .collect()
}
fn setup() -> (app::App, std::sync::mpsc::Sender<CollectSnapshotResult>) {
    let (worker, _, sender) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(0, 10, worker);
    app.snapshot.processes = rows();
    let pending = app.prepare_tracked_list_switch(vec!["root.exe".into()]);
    app.apply_tracked_list_switch(pending);
    app.watch_enabled = true;
    app.rebuild_visible_process_cache();
    (app, sender)
}
fn sample(
    app: &mut app::App,
    sender: &std::sync::mpsc::Sender<CollectSnapshotResult>,
    rows: Vec<ProcessRow>,
) {
    let mut snapshot = test_snapshot(0);
    snapshot.captured_at = app.snapshot.captured_at + chrono::Duration::seconds(1);
    snapshot.processes = rows;
    app.sampling_in_progress = true;
    sender
        .send(CollectSnapshotResult {
            snapshot,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();
}
#[test]
fn descendants_share_filter_total_markers_and_mcp_scope() {
    let (mut app, _) = setup();
    assert_eq!(app.visible_process_count(), 3);
    assert_eq!(
        app.tracked_total_row.as_ref().unwrap().private_bytes,
        Some(60)
    );
    assert_eq!(app.watch_list, ["root.exe"]);
    let view = app.mcp_test_view();
    assert_eq!(view.tracked_identities.len(), 3);
    let buffer = render_app_to_buffer(&app, 120, 40);
    let (_, root_y) = find_text_position(&buffer, "root.exe").unwrap();
    let (_, child_y) = find_text_position(&buffer, "worker.exe").unwrap();
    let marker_x = (0..buffer.area.width)
        .find(|x| buffer[(*x, root_y)].symbol() == "T")
        .unwrap();
    assert_eq!(buffer[(marker_x, child_y)].symbol(), "T");
    assert_eq!(buffer[(marker_x, child_y)].bg, ratatui::style::Color::Gray);
    assert_eq!(buffer[(marker_x, child_y)].fg, ratatui::style::Color::Black);
    assert_eq!(buffer[(marker_x, root_y)].bg, app.theme().tracked);
    let (total_x, total_y) = find_text_position(&buffer, "Tracked Total").unwrap();
    for x in total_x..buffer.area.width - 1 {
        let cell = &buffer[(x, total_y)];
        if !cell.symbol().trim().is_empty() {
            assert_eq!(cell.fg, ratatui::style::Color::Yellow);
            assert!(!cell.modifier.contains(ratatui::style::Modifier::BOLD));
        }
    }
    app.filter_text = "root".into();
    app.rebuild_visible_process_cache();
    assert_eq!(app.visible_process_count(), 1);
    assert_eq!(
        app.tracked_total_row.as_ref().unwrap().private_bytes,
        Some(60)
    );
}
#[test]
fn parent_exit_keeps_children_history_and_new_descendants_while_paused() {
    let (mut app, sender) = setup();
    let original = rows();
    let child = ProcessIdentity::from_row(&original[1]);
    app.toggle_display_pause();
    for _ in 0..125 {
        sample(&mut app, &sender, original[1..].to_vec());
    }
    assert_eq!(app.process_history.sample_count_for(&child), 125);
    let mut current = original[1..].to_vec();
    current.push(ProcessRow {
        pid: 5,
        parent_pid: Some(2),
        name: "later.exe".into(),
        start_time: Some(5),
        ..Default::default()
    });
    sample(&mut app, &sender, current);
    assert_eq!(app.recording_matching_process_count(), 3);
    assert_eq!(app.visible_process_count(), 3); // frozen root + child + grandchild
    app.toggle_display_pause();
    assert!(
        app.visible_processes()
            .iter()
            .any(|p| p.name == "later.exe")
    );
    sample(&mut app, &sender, vec![]);
    assert!(app.exited_tracked_rows.contains_key(&child));
}
#[test]
fn removing_root_confirms_descendant_history_and_preserves_other_roots() {
    let (mut app, sender) = setup();
    let original = rows();
    let child = ProcessIdentity::from_row(&original[1]);
    for _ in 0..125 {
        sample(&mut app, &sender, original.clone());
    }
    let pending = app.prepare_tracked_list_switch(vec![]);
    assert_eq!(pending.discarded_sample_count, 15);
    let pending = app.prepare_tracked_list_switch(vec!["worker.exe".into()]);
    assert_eq!(pending.discarded_sample_count, 5);
    app.apply_tracked_list_switch(pending);
    assert_eq!(app.process_history.sample_count_for(&child), 125);
    assert_eq!(
        app.tracked_total_row.as_ref().unwrap().private_bytes,
        Some(90)
    );
}
#[test]
fn inherited_row_toggle_changes_explicit_name_without_excluding_descendants() {
    let (mut app, _) = setup();
    let child = ProcessIdentity::from_row(&rows()[1]);
    let index = app.visible_process_position(&child).unwrap();
    app.process_table_state.select(Some(index));
    app.selected_process_identity = Some(child);
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.watch_list.contains(&"worker.exe".to_string()));
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.watch_list, ["root.exe"]);
    assert_eq!(app.visible_process_count(), 3);
}
#[test]
fn recording_seeds_orphans_and_round_trips_descendants_with_fixed_roots() {
    let (mut app, sender) = setup();
    sample(&mut app, &sender, rows()[1..].to_vec());
    app.toggle_display_pause();
    let path = unique_recording_path("descendants");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    let mut current = rows()[1..].to_vec();
    current.push(ProcessRow {
        pid: 5,
        parent_pid: Some(2),
        name: "later.exe".into(),
        start_time: Some(5),
        ..Default::default()
    });
    sample(&mut app, &sender, current);
    app.stop_recording().unwrap();
    let loaded = app::logs::load_log(&path, app.sort).unwrap();
    assert_eq!(loaded.summary.tracked_names, ["root.exe"]);
    assert_eq!(loaded.snapshot.processes.len(), 3);
    assert!(!loaded.snapshot.processes.iter().any(|p| p.pid == 4));
    app.apply_loaded_log(loaded);
    assert_eq!(app.visible_process_count(), 3);
    assert_eq!(
        app.tracked_total_row.as_ref().unwrap().private_bytes,
        Some(50)
    );
    std::fs::remove_file(path).unwrap();
}
