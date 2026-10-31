use super::support::{
    find_text_position, left_click, make_test_app, render_app_to_buffer, render_app_to_text,
    show_process_info_files_tab, test_open_files_report,
};
use crate::app::{self, ProcessInfoFocus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::path::PathBuf;

fn setup() -> crate::App {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report(
        "proc-0",
        0,
        r"missing-direct-action-367489\日本語 space.txt",
    ));
    app
}
fn key(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers)).unwrap();
}
#[test]
fn direct_actions_capture_target_without_dialog_or_focus_change() {
    for code in ['o', 'e'] {
        let mut app = setup();
        let path = crate::ui::open_files::selected_entry(&app)
            .unwrap()
            .path
            .clone();
        key(&mut app, KeyCode::Char(code), KeyModifiers::CONTROL);
        assert_eq!(
            app.file_action_worker.as_ref().unwrap().path,
            PathBuf::from(path.clone())
        );
        assert!(!render_app_to_text(&app, 100, 30).contains("FILE ACTIONS"));
        assert_eq!(app.process_info_focus, ProcessInfoFocus::Content);
        app.open_files_result = Some(test_open_files_report("proc-0", 0, "other.txt"));
        assert_eq!(
            app.file_action_worker.as_ref().unwrap().path,
            PathBuf::from(path)
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.poll_file_action() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            app.status.contains("Cannot access current path"),
            "{}",
            app.status
        );
        assert!(!app.open_files_show_detail);
        assert!(app.show_process_info_dialog);
    }
}
#[test]
fn direct_actions_pending_keeps_navigation_and_reports_original_target() {
    let mut app = setup();
    let tx = app.inject_pending_file_action(PathBuf::from("original.txt"));
    key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(app.status.contains("still pending"));
    assert_eq!(
        app.file_action_worker.as_ref().unwrap().path,
        PathBuf::from("original.txt")
    );
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.open_files_show_detail);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.open_files_show_detail);
    tx.send(Ok(())).unwrap();
    assert!(app.poll_file_action());
    assert!(app.status.contains("Windows accepted"));
    assert!(app.status.contains("original.txt"));
    assert_eq!(app.process_info_focus, ProcessInfoFocus::Content);
    let tx = app.inject_pending_file_action(PathBuf::from("earlier.txt"));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.show_process_info_dialog);
    drop(tx);
    assert!(app.poll_file_action());
    assert!(app.status.contains("disconnected"));
    assert!(app.status.contains("earlier.txt"));
    assert!(!app.show_process_info_dialog);
}
#[test]
fn direct_actions_respect_editor_empty_selection_and_log_view() {
    let mut app = setup();
    key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
    assert!(app.file_action_worker.is_none());
    key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
    for code in ['o', 'e'] {
        key(&mut app, KeyCode::Char(code), KeyModifiers::CONTROL);
    }
    assert!(app.file_action_worker.is_none());
    assert!(app.process_info_filter_editing);
    app.process_info_filter_editing = false;
    app.open_files_filter = "no-match".into();
    key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(app.file_action_worker.is_none());
    app.open_files_filter.clear();
    app.log_view_path = Some(PathBuf::from("recording.log"));
    for code in ['o', 'e'] {
        key(&mut app, KeyCode::Char(code), KeyModifiers::CONTROL);
    }
    assert!(app.file_action_worker.is_none());
    assert!(!render_app_to_text(&app, 100, 30).contains("Ctrl+O open"));
}
#[test]
fn direct_actions_footer_mouse_uses_same_path_without_modal() {
    for width in [80, 120] {
        let mut app = setup();
        let screen = Rect::new(0, 0, width, 30);
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, width, 30);
        assert!(!super::support::buffer_to_text(&buffer).contains("F4 actions"));
        let (x, y) = find_text_position(&buffer, "Ctrl+O open").unwrap();
        app.on_mouse(left_click(x, y), screen);
        assert!(app.file_action_worker.is_some());
        assert_eq!(app.process_info_focus, ProcessInfoFocus::Content);
        assert!(!render_app_to_text(&app, width, 30).contains("FILE ACTIONS"));
    }
}
#[test]
fn dll_folder_action_captures_module_and_keeps_detail() {
    let mut app = setup();
    app.process_info_tab = app::ProcessInfoTab::Dlls;
    let mut entry = super::support::test_process_module_entry("missing.dll", "Test");
    entry.path = r"C:\missing-direct-action-367489\missing.dll".into();
    app.process_modules_result = Some(super::support::test_process_modules_report(
        "proc-0",
        0,
        vec![entry.clone()],
    ));
    key(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert_eq!(
        app.file_action_worker.as_ref().unwrap().path,
        PathBuf::from(entry.path)
    );
    assert!(!render_app_to_text(&app, 100, 30).contains("FILE ACTIONS"));
}
