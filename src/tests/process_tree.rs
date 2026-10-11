use std::{collections::HashSet, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, style::Modifier};

use super::support::{
    find_text_position, left_click, make_test_app, make_test_app_with_worker, mouse_move,
    render_app_to_buffer, render_app_to_text, unique_recording_path,
};
use crate::{
    app::state::{ExitedTrackedRow, PausedDisplay},
    app::{self, App, AppActivity, FocusedPanel, ProcessLifecycle, ProcessViewMode},
    model::{
        ProcessHistory, ProcessIdentity, SortColumn, SortDirection, SortSpec, SystemHistory,
        sort_process_rows,
    },
    samplers::{CollectSnapshotResult, SamplingWorker},
};

fn make_tree_app() -> App {
    let mut app = make_test_app(5, 10);
    let rows = &mut app.snapshot.processes;
    rows[0].pid = 30;
    rows[0].parent_pid = Some(20);
    rows[0].name = "grandchild.exe".to_string();
    rows[1].pid = 10;
    rows[1].parent_pid = None;
    rows[1].name = "root.exe".to_string();
    rows[2].pid = 20;
    rows[2].parent_pid = Some(10);
    rows[2].name = "child.exe".to_string();
    rows[3].pid = 40;
    rows[3].parent_pid = None;
    rows[3].name = "other-root.exe".to_string();
    rows[4].pid = 50;
    rows[4].parent_pid = Some(40);
    rows[4].name = "other-child.exe".to_string();
    app.sort = SortSpec {
        column: SortColumn::Pid,
        direction: SortDirection::Asc,
    };
    sort_process_rows(&mut app.snapshot.processes, app.sort);
    app.process_view_mode = ProcessViewMode::Tree;
    app.selected_process_identity = None;
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    app
}

fn visible_names_and_depths(app: &App) -> Vec<(String, usize)> {
    app.visible_process_row_window(0, app.visible_process_count())
        .into_iter()
        .map(|row| (row.process.name.clone(), row.tree_depth))
        .collect()
}

#[test]
fn app_tree_renders_complete_subtrees_in_parent_first_order() {
    let app = make_tree_app();

    assert_eq!(
        visible_names_and_depths(&app),
        vec![
            ("root.exe".to_string(), 0),
            ("child.exe".to_string(), 1),
            ("grandchild.exe".to_string(), 2),
            ("other-root.exe".to_string(), 0),
            ("other-child.exe".to_string(), 1),
        ]
    );
}

#[test]
fn tree_buffer_indents_descendants_and_draws_disclosures() {
    let mut app = make_tree_app();
    app.process_column_widths.set(SortColumn::ProcessName, 24);
    let buffer = render_app_to_buffer(&app, 120, 45);
    let (root_x, root_y) = find_text_position(&buffer, "root.exe").unwrap();
    let (child_x, child_y) = find_text_position(&buffer, "child.exe").unwrap();
    let (grandchild_x, grandchild_y) = find_text_position(&buffer, "grandchild.exe").unwrap();

    assert!(root_y < child_y && child_y < grandchild_y);
    assert_eq!(child_x, root_x + 2);
    assert_eq!(grandchild_x, child_x + 2);
    assert_eq!(buffer[(root_x - 2, root_y)].symbol(), "▾");
    assert_eq!(buffer[(child_x - 2, child_y)].symbol(), "▾");
}

#[test]
fn tracked_only_excludes_untracked_ancestors_and_combined_filter_stays_in_subset() {
    let mut app = make_tree_app();
    app.watch_list = vec!["child.exe".to_string(), "grandchild.exe".to_string()];
    app.normalized_watch_names =
        HashSet::from(["child.exe".to_string(), "grandchild.exe".to_string()]);
    app.watch_enabled = true;
    app.rebuild_visible_process_cache();

    assert_eq!(
        visible_names_and_depths(&app),
        vec![
            ("child.exe".to_string(), 0),
            ("grandchild.exe".to_string(), 1),
        ]
    );

    app.filter_text = "grandchild".to_string();
    app.rebuild_visible_process_cache();
    let rows = app.visible_process_row_window(0, app.visible_process_count());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].process.name, "child.exe");
    assert!(rows[0].filter_context);
    assert_eq!(rows[1].process.name, "grandchild.exe");
    assert!(!rows[1].filter_context);
    assert_eq!(app.visible_process_match_count(), 1);
}

#[test]
fn filter_context_is_muted_and_jump_skips_context_only_rows() {
    let mut app = make_tree_app();
    app.filter_text = "grandchild".to_string();
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();

    let buffer = render_app_to_buffer(&app, 120, 45);
    let (root_x, root_y) = find_text_position(&buffer, "root.exe").unwrap();
    assert_eq!(buffer[(root_x, root_y)].fg, app.theme().muted);
    let rendered = render_app_to_text(&app, 120, 45);
    assert!(rendered.contains("1 match · 3 visible"), "{rendered}");

    app.begin_process_jump_edit();
    for ch in "root".chars() {
        app.push_process_jump_char(ch);
    }
    assert_eq!(app.status, "No matching process: root");
    assert_ne!(
        app.selected_visible_process().map(|row| row.name.as_str()),
        Some("root.exe")
    );
}

#[test]
fn filter_forced_paths_disable_expansion_and_restore_prior_collapsed_state() {
    let mut app = make_tree_app();
    let root = app.visible_process_identity_at(0).unwrap();
    app.toggle_process_expansion_at(0);
    assert!(app.collapsed_process_identities.contains(&root));

    app.filter_text = "grandchild".to_string();
    app.rebuild_visible_process_cache();
    assert_eq!(
        visible_names_and_depths(&app),
        vec![
            ("root.exe".to_string(), 0),
            ("child.exe".to_string(), 1),
            ("grandchild.exe".to_string(), 2),
        ]
    );

    app.focused_panel = FocusedPanel::Processes;
    app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.collapsed_process_identities.contains(&root));
    assert_eq!(
        app.status,
        "Clear the process filter to expand or collapse Tree rows"
    );

    app.filter_text.clear();
    app.rebuild_visible_process_cache();
    assert_eq!(
        visible_names_and_depths(&app),
        vec![
            ("root.exe".to_string(), 0),
            ("other-root.exe".to_string(), 0),
            ("other-child.exe".to_string(), 1),
        ]
    );
}

#[test]
fn filtered_disclosures_are_muted_and_absorb_mouse_input_without_tracking() {
    let mut app = make_tree_app();
    app.filter_text = "grandchild".to_string();
    app.rebuild_visible_process_cache();
    let screen = Rect::new(0, 0, 120, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (glyph_x, glyph_y) = super::support::find_text_position_in_area(
        &buffer,
        crate::ui::main_panel_areas_for_app(screen, &app)
            .processes
            .area,
        "▾",
    )
    .unwrap();
    assert_eq!(buffer[(glyph_x, glyph_y)].fg, app.theme().muted);

    app.on_mouse(mouse_move(glyph_x, glyph_y), screen);
    assert!(app.process_disclosure_hovered.is_none());

    app.on_mouse(left_click(glyph_x, glyph_y), screen);
    app.on_mouse(left_click(glyph_x, glyph_y), screen);
    assert!(app.watch_list.is_empty());
    assert!(app.collapsed_process_identities.is_empty());
    assert_eq!(
        app.status,
        "Clear the process filter to expand or collapse Tree rows"
    );
}

#[test]
fn collapse_moves_hidden_focus_to_parent_and_prunes_hidden_multi_selection() {
    let mut app = make_tree_app();
    let child = app.visible_process_identity_at(1).unwrap();
    let grandchild = app.visible_process_identity_at(2).unwrap();
    app.select_process_index(2);
    app.selected_process_identities = HashSet::from([child, grandchild]);

    app.toggle_process_expansion_at(0);

    assert_eq!(
        app.selected_visible_process().map(|row| row.name.as_str()),
        Some("root.exe")
    );
    assert_eq!(app.selected_process_identities_count(), 0);
    assert_eq!(
        visible_names_and_depths(&app),
        vec![
            ("root.exe".to_string(), 0),
            ("other-root.exe".to_string(), 0),
            ("other-child.exe".to_string(), 1),
        ]
    );
}

#[test]
fn collapsed_state_uses_full_identity_and_is_not_inherited_after_pid_reuse() {
    let mut app = make_tree_app();
    let old_root = app.visible_process_identity_at(0).unwrap();
    app.toggle_process_expansion_at(0);
    assert!(app.collapsed_process_identities.contains(&old_root));

    let root_index = app
        .snapshot
        .processes
        .iter()
        .position(|row| row.pid == 10)
        .unwrap();
    app.snapshot.processes[root_index].start_time = Some(1_900_000_000);
    app.rebuild_visible_process_cache();

    let new_root = app
        .snapshot
        .processes
        .iter()
        .find(|row| row.pid == 10)
        .map(ProcessIdentity::from_row)
        .unwrap();
    assert_ne!(old_root, new_root);
    assert!(!app.collapsed_process_identities.contains(&old_root));
    assert!(!app.collapsed_process_identities.contains(&new_root));
    assert!(
        visible_names_and_depths(&app)
            .iter()
            .any(|(name, depth)| name == "child.exe" && *depth == 1)
    );
}

#[test]
fn normal_sample_refresh_keeps_focus_by_identity_after_tree_reorders() {
    let configured = make_tree_app();
    let (worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(5, 10, worker);
    app.snapshot = configured.snapshot.clone();
    app.sort = configured.sort;
    app.process_view_mode = ProcessViewMode::Tree;
    app.selected_process_identity = None;
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    app.select_process_index(2);
    let selected = app.selected_visible_process_identity().unwrap();

    let mut next = app.snapshot.clone();
    next.captured_at += chrono::Duration::seconds(1);
    next.processes.reverse();
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();
    app.sampling_in_progress = true;
    app.poll_sample_results().unwrap();

    assert_eq!(app.selected_visible_process_identity(), Some(selected));
    assert_eq!(
        app.selected_visible_process().map(|row| row.name.as_str()),
        Some("grandchild.exe")
    );
    assert_eq!(
        app.visible_process_tree_state_at(2).map(|state| state.0),
        Some(2)
    );
}

#[test]
fn tree_range_and_multi_selection_survive_sorting_and_filter_context() {
    let mut app = make_tree_app();
    app.select_process_index(1);
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
        .unwrap();
    let child = ProcessIdentity::from_row(
        app.snapshot
            .processes
            .iter()
            .find(|row| row.name == "child.exe")
            .unwrap(),
    );
    let grandchild = ProcessIdentity::from_row(
        app.snapshot
            .processes
            .iter()
            .find(|row| row.name == "grandchild.exe")
            .unwrap(),
    );
    assert_eq!(app.selected_process_identities_count(), 2);
    assert!(app.selected_process_identities.contains(&child));
    assert!(app.selected_process_identities.contains(&grandchild));

    app.sort = SortSpec {
        column: SortColumn::ProcessName,
        direction: SortDirection::Desc,
    };
    sort_process_rows(&mut app.snapshot.processes, app.sort);
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    assert_eq!(
        app.selected_visible_process_identity(),
        Some(grandchild.clone())
    );

    app.filter_text = "grandchild".to_string();
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    assert_eq!(
        app.selected_visible_process_identity(),
        Some(grandchild.clone())
    );
    assert_eq!(app.selected_process_identities_count(), 2);
    assert!(app.selected_process_identities.contains(&child));
    assert!(app.selected_process_identities.contains(&grandchild));
}

#[test]
fn parent_exit_promotes_a_remaining_child_without_losing_its_focus() {
    let mut app = make_tree_app();
    app.select_process_index(1);
    let child = app.selected_visible_process_identity().unwrap();
    let old_root = app.visible_process_identity_at(0).unwrap();
    app.collapsed_process_identities.insert(old_root.clone());

    app.snapshot.processes.retain(|row| row.pid != old_root.pid);
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();

    assert_eq!(app.selected_visible_process_identity(), Some(child.clone()));
    let child_index = app.visible_process_position(&child).unwrap();
    assert_eq!(
        app.visible_process_tree_state_at(child_index)
            .map(|state| state.0),
        Some(0)
    );
    assert!(!app.collapsed_process_identities.contains(&old_root));
}

#[test]
fn ghost_rows_are_non_expandable_roots_after_the_live_forest() {
    let mut app = make_tree_app();
    let mut ghost = app.snapshot.processes[0].clone();
    ghost.pid = 99;
    ghost.parent_pid = Some(10);
    ghost.name = "ghost.exe".to_string();
    let identity = ProcessIdentity::from_row(&ghost);
    app.watch_list.push("ghost.exe".to_string());
    app.normalized_watch_names.insert("ghost.exe".to_string());
    app.exited_tracked_rows.insert(
        identity,
        ExitedTrackedRow {
            process: ghost,
            exited_at: app.snapshot.captured_at,
        },
    );
    app.rebuild_visible_process_cache();

    let rows = app.visible_process_row_window(0, app.visible_process_count());
    let last = rows.last().unwrap();
    assert_eq!(last.process.name, "ghost.exe");
    assert_eq!(last.tree_depth, 0);
    assert!(!last.tree_has_children);
    assert!(matches!(last.lifecycle, ProcessLifecycle::Exited { .. }));
}

#[test]
fn paused_display_keeps_its_tree_while_live_relationships_change() {
    let mut app = make_tree_app();
    app.toggle_display_pause();
    for row in &mut app.snapshot.processes {
        row.parent_pid = None;
    }
    app.rebuild_visible_process_cache();
    assert!(
        visible_names_and_depths(&app)
            .iter()
            .any(|(name, depth)| name == "grandchild.exe" && *depth == 2)
    );

    app.toggle_display_pause();
    assert!(
        visible_names_and_depths(&app)
            .iter()
            .all(|(_, depth)| *depth == 0)
    );
}

#[test]
fn recording_keeps_tree_available_and_log_view_forces_flat() {
    let path = unique_recording_path("process-tree-recording");
    let mut app = make_tree_app();
    app.watch_list = vec!["root.exe".to_string()];
    app.normalized_watch_names = HashSet::from(["root.exe".to_string()]);
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();

    assert_eq!(app.activity(), AppActivity::Recording);
    assert_eq!(app.effective_process_view_mode(), ProcessViewMode::Tree);
    assert!(
        visible_names_and_depths(&app)
            .iter()
            .any(|(_, depth)| *depth > 0)
    );
    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);

    app.log_view_path = Some(PathBuf::from("test.log"));
    app.log_view_display = Some(PausedDisplay {
        snapshot: app.snapshot.clone(),
        exited_tracked_rows: Default::default(),
        process_history: ProcessHistory::default(),
        system_history: SystemHistory::default(),
        process_info_cache: Default::default(),
        process_info_display_identity: None,
    });
    app.rebuild_visible_process_cache();
    assert_eq!(app.effective_process_view_mode(), ProcessViewMode::Flat);
    assert!(
        visible_names_and_depths(&app)
            .iter()
            .all(|(_, depth)| *depth == 0)
    );
    let rendered = render_app_to_text(&app, 140, 45);
    assert!(
        rendered.contains("Flat (Tree unavailable in LOG)"),
        "{rendered}"
    );

    app.toggle_process_view_mode();
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);
    assert_eq!(app.status, "Tree view is unavailable in Log view");
}

#[test]
fn disclosure_mouse_region_is_distinct_from_tracking_and_uses_hover_style() {
    let mut app = make_tree_app();
    let screen = Rect::new(0, 0, 120, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (glyph_x, glyph_y) = super::support::find_text_position_in_area(
        &buffer,
        crate::ui::main_panel_areas_for_app(screen, &app)
            .processes
            .area,
        "▾",
    )
    .unwrap();

    app.on_mouse(mouse_move(glyph_x, glyph_y), screen);
    let hovered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(hovered[(glyph_x, glyph_y)].bg, app.theme().focus_surface);
    assert!(
        hovered[(glyph_x, glyph_y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    app.on_mouse(left_click(glyph_x, glyph_y), screen);
    assert!(app.watch_list.is_empty());
    assert_eq!(app.visible_process_count(), 3);

    let name_x = glyph_x.saturating_add(2);
    app.on_mouse(left_click(name_x, glyph_y), screen);
    app.on_mouse(left_click(name_x, glyph_y), screen);
    assert!(app.watch_list.is_empty());
    assert!(app.show_process_info_dialog);
    assert_eq!(
        app.process_info_target.as_ref().unwrap().identity.name,
        "root.exe"
    );
}

#[test]
fn mode_toggle_is_keyboard_and_mouse_accessible_and_tree_clipboard_stays_raw() {
    let mut app = make_tree_app();
    app.focused_panel = FocusedPanel::Processes;
    app.on_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_view_mode, ProcessViewMode::Flat);

    let screen = Rect::new(0, 0, 140, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (mode_x, mode_y) = find_text_position(&buffer, "Flat(v)").unwrap();
    app.on_mouse(mouse_move(mode_x, mode_y), screen);
    let hovered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(hovered[(mode_x, mode_y)].bg, app.theme().focus_surface);
    assert!(hovered[(mode_x, mode_y)].modifier.contains(Modifier::BOLD));
    app.on_mouse(left_click(mode_x, mode_y), screen);
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);

    app.select_process_index(1);
    app.copy_selected_process_row_to_clipboard().unwrap();
    let copied = app::clipboard::last_copied_text().unwrap();
    assert!(copied.starts_with("20\tchild.exe\t"), "{copied}");
    assert!(!copied.contains('▾'));
    assert!(!copied.contains('▸'));
}

#[test]
fn narrow_process_column_keeps_disclosure_draw_and_hit_test_aligned() {
    let mut app = make_tree_app();
    app.process_column_widths
        .set(SortColumn::ProcessName, SortColumn::ProcessName.min_width());
    app.rebuild_visible_process_cache();
    let screen = Rect::new(0, 0, 100, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (glyph_x, glyph_y) = super::support::find_text_position_in_area(
        &buffer,
        crate::ui::main_panel_areas_for_app(screen, &app)
            .processes
            .area,
        "▾",
    )
    .unwrap();

    app.on_mouse(left_click(glyph_x, glyph_y), screen);

    assert_eq!(app.visible_process_count(), 3);
    assert!(app.watch_list.is_empty());
}

#[test]
fn deep_tree_preserves_name_width_and_disclosure_target() {
    let mut app = make_test_app(15, 30);
    for (index, process) in app.snapshot.processes.iter_mut().enumerate() {
        process.pid = index as u32 + 100;
        process.parent_pid = (index > 0).then_some(index as u32 + 99);
        process.name = format!("branch-{index:02}.exe");
    }
    app.process_view_mode = ProcessViewMode::Tree;
    app.process_column_widths.set(SortColumn::ProcessName, 24);
    app.rebuild_visible_process_cache();
    let screen = Rect::new(0, 0, 120, 60);
    let buffer = render_app_to_buffer(&app, 120, 60);
    let (name_x, name_y) = find_text_position(&buffer, "branch-12.exe").unwrap();
    assert_eq!(buffer[(name_x - 3, name_y)].symbol(), "…");
    assert_eq!(buffer[(name_x - 2, name_y)].symbol(), "▾");
    app.on_mouse(left_click(name_x - 2, name_y), screen);
    assert_eq!(app.visible_process_count(), 13);
    assert!(app.watch_list.is_empty());
    app.begin_filter_edit();
    for ch in "branch-14".chars() {
        app.push_filter_char(ch);
    }
    assert_eq!(app.visible_process_count(), 15);
    assert!(render_app_to_text(&app, 120, 60).contains("branch-14.exe"));
}

#[test]
fn tracked_only_ctrl_a_excludes_collapsed_descendants_and_ghost_rows() {
    let mut app = make_tree_app();
    app.watch_list = app
        .snapshot
        .processes
        .iter()
        .map(|row| row.name.clone())
        .collect();
    app.normalized_watch_names = app.watch_list.iter().cloned().collect();
    app.watch_enabled = true;
    let root = app
        .snapshot
        .processes
        .iter()
        .find(|row| row.pid == 10)
        .unwrap()
        .clone();
    app.collapsed_process_identities
        .insert(ProcessIdentity::from_row(&root));
    let mut ghost = root;
    ghost.pid = 99;
    app.exited_tracked_rows.insert(
        ProcessIdentity::from_row(&ghost),
        ExitedTrackedRow {
            process: ghost,
            exited_at: app.snapshot.captured_at,
        },
    );
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    assert_eq!(app.visible_process_count(), 4);

    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(
        app.selected_process_identities
            .iter()
            .map(|identity| identity.pid)
            .collect::<HashSet<_>>(),
        HashSet::from([10, 40, 50])
    );
    let rendered = render_app_to_text(&app, 120, 60);
    assert!(rendered.contains("3 selected (*)"), "{rendered}");
    assert!(rendered.contains("Tracked Total"), "{rendered}");
}

#[test]
fn copy_table_respects_tree_depth_collapse_and_filter() {
    let mut app = make_tree_app();
    let table = app.process_table_clipboard_text();
    assert!(table.contains("\tgrandchild.exe\tlive\t2\t"));
    let root = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    app.collapsed_process_identities.insert(root);
    app.rebuild_visible_process_cache();
    let table = app.process_table_clipboard_text();
    assert!(!table.contains("grandchild.exe"));
    assert!(!table.contains("\tchild.exe\t"));
    assert!(table.contains("root.exe"));
    app.filter_text = "other-child".into();
    app.rebuild_visible_process_cache();
    let table = app.process_table_clipboard_text();
    assert!(table.contains("other-child.exe"));
    assert!(!table.contains("\troot.exe\t"));
}
