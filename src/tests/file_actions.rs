use super::support::{
    find_text_position, make_test_app, render_app_to_buffer, render_app_to_text,
    show_process_info_files_tab, test_open_files_report,
};
use crate::app::{self, ProcessInfoFocus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

fn key(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers)).unwrap();
}
#[test]
fn file_actions_f4_opens_modal_directly_and_preserves_editor_priority() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report(
        "proc-0",
        0,
        r"fixture space\日本語.txt",
    ));
    let path = crate::ui::open_files::selected_entry(&app)
        .unwrap()
        .path
        .clone();
    key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
    assert!(!app.open_files_show_detail);
    assert!(app.file_action.is_some());
    assert_eq!(
        app.file_action.as_ref().unwrap().path.to_str().unwrap(),
        path
    );
    app.open_files_result = Some(test_open_files_report("proc-0", 0, r"other.txt"));
    assert_eq!(
        app.file_action.as_ref().unwrap().path.to_str().unwrap(),
        path
    );
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_action.is_none());
    key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
    key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(app.file_action.is_none());
    assert!(app.process_info_filter_editing);
    assert!(!render_app_to_text(&app, 100, 30).contains("F4 actions"));
}
#[test]
fn file_actions_narrow_footer_and_mouse_share_action_path_and_errors() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report(
        "proc-0",
        0,
        r"missing-issue167-367489\日本語.txt",
    ));
    let screen = Rect::new(0, 0, 60, 24);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, 60, 24);
    let (x, y) = find_text_position(&buffer, "F4 actions").unwrap();
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.file_action.is_some());
    let text = render_app_to_text(&app, 60, 24);
    assert!(text.contains("Open containing folder"), "{text}");
    assert!(text.contains("Open with default app"), "{text}");
    let buffer = render_app_to_buffer(&app, 60, 24);
    let (x, y) = find_text_position(&buffer, "Open with default app").unwrap();
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.poll_file_action() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.file_action.as_ref().unwrap().failed);
    assert!(
        app.file_action
            .as_ref()
            .unwrap()
            .message
            .contains("Cannot access current path")
    );
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_action.is_none());
    assert_eq!(app.process_info_focus, ProcessInfoFocus::Content);
}

#[test]
fn file_actions_pending_can_dismiss_without_duplicate_worker_or_late_view_update() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report("proc-0", 0, "fixture.txt"));
    app.open_file_actions();
    let tx = app.inject_pending_file_action();
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_action.is_none());
    assert!(app.file_action_worker.is_some());
    app.open_file_actions();
    app.launch_file_action(1);
    assert!(app.file_action_worker.is_some());
    assert!(
        app.file_action
            .as_ref()
            .unwrap()
            .message
            .contains("previous file action")
    );
    tx.send(Err("injected delayed error".into())).unwrap();
    assert!(app.poll_file_action());
    assert!(app.file_action_worker.is_none());
    assert!(
        !app.file_action
            .as_ref()
            .unwrap()
            .message
            .contains("injected delayed error")
    );
    assert!(
        app.status
            .contains("Earlier file action: Request failed: injected delayed error")
    );
    let tx = app.inject_pending_file_action();
    drop(tx);
    assert!(app.poll_file_action());
    assert!(app.file_action.as_ref().unwrap().failed);
    assert!(
        app.file_action
            .as_ref()
            .unwrap()
            .message
            .contains("disconnected")
    );
}
#[test]
fn file_actions_log_view_rejects_actions_and_hides_shortcut() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report("proc-0", 0, "fixture.txt"));
    app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
    key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
    assert!(app.file_action.is_none());
    assert!(app.file_action_worker.is_none());
    assert!(!render_app_to_text(&app, 100, 30).contains("F4 actions"));
}

#[test]
fn file_actions_modal_preserves_parent_scroll_and_blocks_background_mouse() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    let mut report = test_open_files_report("proc-0", 0, "fixture.txt");
    let entry = report.entries[0].clone();
    report.entries = (0..50).map(|_| entry.clone()).collect();
    app.open_files_result = Some(report);
    for (width, height) in [(120, 48), (60, 24)] {
        let screen = Rect::new(0, 0, width, height);
        app::sync_layout_state(&mut app, screen);
        app.select_open_file(49);
        let offset = app.open_files_scroll.offset;
        assert!(offset > 0);
        let parent = crate::ui::process_info_dialog::process_info_dialog_layout_for_screen(
            screen,
            app.process_info_height(),
        )
        .area;
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        let modal = crate::ui::file_actions::layout(screen);
        assert!(modal.area.height < parent.height);
        assert!(!app.open_files_show_detail);
        app.file_action.as_mut().unwrap().message =
            "Long failure details ".repeat(150) + "END OF ERROR";
        let footer = crate::ui::file_actions::layout(screen).footer;
        let buffer = render_app_to_buffer(&app, width, height);
        assert!(super::support::find_text_position_in_area(&buffer, footer, "PgUp").is_none());
        for code in [KeyCode::PageDown, KeyCode::PageUp] {
            key(&mut app, code, KeyModifiers::NONE);
            assert_eq!(app.file_action.as_ref().unwrap().scroll.offset, 0);
            assert_eq!(app.file_action.as_ref().unwrap().selected, 0);
        }
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert!(render_app_to_text(&app, width, height).contains("END OF ERROR"));
        assert_eq!(app.open_files_scroll.offset, offset);
        app.on_mouse(
            super::support::left_click(parent.x, parent.bottom() - 1),
            screen,
        );
        assert!(app.process_info_resize_drag.is_none());
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.file_action.as_ref().unwrap().selected, 1);
        assert_eq!(app.file_action.as_ref().unwrap().scroll.offset, 0);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.open_files_selected, 49);
        assert_eq!(app.open_files_scroll.offset, offset);
        assert!(app.show_process_info_dialog);
    }
}

#[test]
fn direct_file_shortcuts_freeze_targets_and_respect_pending_and_editor() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(test_open_files_report("proc-0", 0, "one.txt"));
    app.open_file_actions();
    let _pending = app.inject_pending_file_action();
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(
        app.file_action
            .as_ref()
            .unwrap()
            .message
            .contains("previous file action")
    );
    let captured = app.file_action.as_ref().unwrap().path.clone();
    app.open_files_result = Some(test_open_files_report("proc-0", 0, "two.txt"));
    assert_eq!(app.file_action.as_ref().unwrap().path, captured);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.process_info_tab = crate::app::ProcessInfoTab::Dlls;
    app.process_modules_result = Some(super::support::test_process_modules_report(
        "proc-0",
        0,
        vec![super::support::test_process_module_entry(
            "test.dll", "test",
        )],
    ));
    key(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(
        app.file_action
            .as_ref()
            .unwrap()
            .path
            .to_string_lossy()
            .contains("test.dll")
    );
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.process_info_filter_editing = true;
    key(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(app.file_action.is_none());
}
