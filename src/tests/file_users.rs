use super::support::{left_click, make_test_app, render_app_to_buffer, render_app_to_text};
use crate::{
    App,
    app::{self, ProcessInfoTab, file_users::FileUsersFocus},
    model::file_users::*,
    samplers::file_users::FileUsersWorker,
    ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc::Receiver},
};

type Updates = Arc<Mutex<Option<FileUsersUpdate>>>;
fn setup() -> (App, Receiver<(u64, FileUsersRequest)>, Updates) {
    let mut app = make_test_app(2, 10);
    let (worker, requests, updates) = FileUsersWorker::test_pair();
    app.file_users_worker = worker;
    (app, requests, updates)
}
fn press(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
}
fn deliver(
    app: &mut App,
    updates: &Updates,
    id: u64,
    report: FileSearchReport,
    owner: Option<FileUserOwner>,
) {
    *updates.lock().unwrap() = Some(FileUsersUpdate { id, report, owner });
    assert!(app.poll_file_users_results());
}
fn entry(index: u32) -> FileUserMatch {
    FileUserMatch {
        path: format!(r"C:\media\映像-{index}.mxf"),
        owner: FileUserOwner {
            pid: index + 1234,
            name: format!("player-{index}.exe"),
            executable_path: format!(r"C:\tools\player-{index}.exe"),
            creation_time: 116_444_736_120_000_000,
        },
        handle_count: 2,
    }
}

#[test]
fn file_users_matching_handles_unicode_unc_and_extended_paths() {
    let query = |text: &str, mode| FileSearchQuery {
        text: text.into(),
        mode,
    };
    assert!(query("映像.MXF", FileSearchMode::FileName).matches(r"C:\media\映像.mxf"));
    assert!(!query("media", FileSearchMode::FileName).matches(r"C:\media\映像.mxf"));
    assert!(query("media/映像", FileSearchMode::Path).matches(r"C:\MEDIA\映像.mxf"));
    assert!(
        query("bad\nquery", FileSearchMode::ExactPath)
            .validate()
            .is_err()
    );
    assert!(
        query(r"\\server\share\映像.mxf", FileSearchMode::ExactPath)
            .matches(r"\\?\UNC\SERVER\share\映像.mxf")
    );
    assert!(
        query(r"C:/media/映像.mxf", FileSearchMode::ExactPath).matches(r"\\?\C:\media\映像.mxf")
    );
    assert!(!query(r"C:\other\映像.mxf", FileSearchMode::ExactPath).matches(r"C:\media\映像.mxf"));
    assert!(
        query("映像.mxf", FileSearchMode::ExactPath)
            .validate()
            .is_err()
    );
    assert!(query("", FileSearchMode::FileName).validate().is_err());
    assert!(
        query(r"C:\media\.\take\..\映像.mxf", FileSearchMode::ExactPath)
            .matches(r"C:\media\映像.mxf")
    );
    assert!(
        query(&"a".repeat(4097), FileSearchMode::Path)
            .validate()
            .is_err()
    );
    let long = format!(r"C:\{}\映像.mxf", "long-path".repeat(60));
    assert!(query(&long, FileSearchMode::ExactPath).matches(&format!(r"\\?\{long}")));
}

#[test]
fn file_users_search_is_explicit_and_independent_of_process_filters() {
    let (mut app, requests, updates) = setup();
    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
        .unwrap();
    for ch in "not-the-file-owner".chars() {
        press(&mut app, KeyCode::Char(ch));
    }
    press(&mut app, KeyCode::Enter);
    app.toggle_watch_list();
    app.open_file_users();
    assert!(requests.try_recv().is_err());
    for ch in "映像 / test.mxf".chars() {
        press(&mut app, KeyCode::Char(ch));
    }
    assert_eq!(app.file_users.draft, "映像 / test.mxf");
    assert!(requests.try_recv().is_err());
    press(&mut app, KeyCode::Enter);
    let (id, request) = requests.try_recv().unwrap();
    let FileUsersRequest::Search(query) = request else {
        panic!("search expected")
    };
    assert_eq!(query.text, "映像 / test.mxf");
    app.start_file_search();
    assert!(requests.try_recv().is_err());
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            matches: vec![entry(0)],
            end: Some(FileSearchEnd::Complete),
            ..FileSearchReport::default()
        },
        None,
    );
    assert_eq!(app.file_users.report.matches.len(), 1);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.file_users.searched.as_ref().unwrap().text, query.text);
    assert!(requests.try_recv().is_err());
}

#[test]
fn file_users_cancellation_retains_partial_results_and_rejects_stale_updates() {
    let (mut app, requests, updates) = setup();
    app.open_file_users();
    app.file_users.draft = "mxf".into();
    app.file_users.cursor = 3;
    app.start_file_search();
    let (first, _) = requests.try_recv().unwrap();
    deliver(
        &mut app,
        &updates,
        first,
        FileSearchReport {
            matches: vec![entry(1)],
            ..FileSearchReport::default()
        },
        None,
    );
    press(&mut app, KeyCode::Esc);
    assert!(app.file_users.visible);
    assert_eq!(app.file_users.pending, Some(first));
    deliver(
        &mut app,
        &updates,
        first,
        FileSearchReport {
            matches: vec![entry(1)],
            end: Some(FileSearchEnd::Cancelled),
            ..FileSearchReport::default()
        },
        None,
    );
    assert_eq!(app.file_users.report.matches.len(), 1);
    app.start_file_search();
    let (second, _) = requests.try_recv().unwrap();
    assert_ne!(first, second);
    *updates.lock().unwrap() = Some(FileUsersUpdate {
        id: first,
        report: FileSearchReport::default(),
        owner: None,
    });
    assert!(!app.poll_file_users_results());
    app.close_file_users();
    *updates.lock().unwrap() = Some(FileUsersUpdate {
        id: second,
        report: FileSearchReport::default(),
        owner: Some(entry(0).owner),
    });
    assert!(!app.poll_file_users_results());
    assert!(!app.show_process_info_dialog);
}

#[test]
fn file_users_partial_zero_results_show_coverage_and_full_diagnostics() {
    let (mut app, _, _) = setup();
    app.open_file_users();
    app.file_users.searched = Some(FileSearchQuery {
        text: "missing.mxf".into(),
        mode: FileSearchMode::FileName,
    });
    app.file_users.report = FileSearchReport {
        progress: FileSearchProgress {
            total_processes: 10,
            denied_processes: 3,
            unreadable_handles: 5,
            ..FileSearchProgress::default()
        },
        end: Some(FileSearchEnd::TimedOut),
        ..FileSearchReport::default()
    };
    let text = render_app_to_text(&app, 120, 35);
    assert!(text.contains("Timed out (partial results)"));
    assert!(text.contains("No matches found in the inspected scope."));
    app.file_users.focus = FileUsersFocus::Results;
    press(&mut app, KeyCode::Char(' '));
    let details = ui::file_users::detail_lines(&app.file_users, 120).join("\n");
    assert!(details.contains("Access denied for 3 processes"));
    assert!(details.contains("Handles unreadable: 5"));
    assert!(details.contains("Memory-mapped-only"));
}

#[test]
fn file_users_selection_details_and_mouse_share_responsive_geometry() {
    let (mut app, _, _) = setup();
    app.open_file_users();
    app.file_users.focus = FileUsersFocus::Results;
    app.file_users.report.matches = (0..40).map(entry).collect();
    for (width, height) in [(160, 35), (80, 24), (60, 18), (160, 35)] {
        let screen = Rect::new(0, 0, width, height);
        app::sync_layout_state(&mut app, screen);
        press(&mut app, KeyCode::End);
        assert_eq!(app.file_users.selected, 39);
        let rows =
            ui::file_users::content_layout(ui::file_users::browser_layout(screen).content).rows;
        let buffer = render_app_to_buffer(&app, width, height);
        let (x, y) = super::support::find_text_position_in_area(&buffer, rows, "1273")
            .expect("last PID visible");
        assert_eq!(buffer[(x, y)].bg, app.theme().focus_surface);
        app.on_mouse(left_click(x, y), screen);
        assert_eq!(app.file_users.selected, 39);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.file_users.detail);
        press(&mut app, KeyCode::Esc);
        assert!(!app.file_users.detail);
    }
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().unwrap(),
        entry(39).plain_text()
    );
}

#[test]
fn file_users_verified_navigation_retains_search_and_opens_files() {
    let (mut app, requests, updates) = setup();
    app.open_file_users();
    app.file_users.focus = FileUsersFocus::Results;
    let entry = entry(0);
    let (files_worker, files_requests, _) =
        crate::samplers::open_files::OpenFilesWorker::test_pair();
    app.open_files_worker = files_worker;
    app.file_users.report.matches = vec![entry.clone()];
    press(&mut app, KeyCode::Enter);
    let (id, request) = requests.try_recv().unwrap();
    assert!(matches!(request, FileUsersRequest::Verify(ref owner) if owner == &entry.owner));
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            end: Some(FileSearchEnd::Complete),
            ..FileSearchReport::default()
        },
        Some(entry.owner.clone()),
    );
    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_tab, ProcessInfoTab::Files);
    assert!(
        files_requests.try_recv().is_ok(),
        "a newly verified owner can precede the next process snapshot"
    );
    assert!(app.process_info_target_is_currently_live());
    app.snapshot.captured_at += chrono::Duration::seconds(1);
    assert!(
        !app.process_info_target_is_currently_live(),
        "verification allowance expires with the next snapshot"
    );
    assert_eq!(
        app.process_info_target.as_ref().unwrap().identity,
        entry.owner.identity()
    );
    app.close_process_info_dialog();
    assert!(app.process_info_verified_snapshot_at.is_none());
    assert!(app.file_users.visible);
    assert_eq!(app.file_users.report.matches[0].path, entry.path);
    press(&mut app, KeyCode::Enter);
    let (id, _) = requests.try_recv().unwrap();
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            end: Some(FileSearchEnd::Failed("Process identity changed".into())),
            ..FileSearchReport::default()
        },
        None,
    );
    assert!(!app.show_process_info_dialog);
    assert!(
        app.file_users
            .notice
            .as_deref()
            .unwrap()
            .contains("identity changed")
    );
}

#[test]
fn file_users_menu_is_live_only_and_does_not_start_a_scan() {
    let (mut app, requests, _) = setup();
    app.activate_header_action(ui::header::HeaderAction::FileUsers);
    assert!(app.file_users.visible);
    assert!(requests.try_recv().is_err());
    app.close_file_users();
    app.log_view_path = Some(PathBuf::from("example.jsonl"));
    app.open_file_users();
    assert!(!app.file_users.visible);
    assert!(requests.try_recv().is_err());
}

#[test]
fn file_users_queries_and_results_stay_outside_recordings() {
    let (mut app, requests, updates) = setup();
    let path = super::support::unique_recording_path("file-users");
    super::support::track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.to_string_lossy().into_owned();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    app.open_file_users();
    app.file_users.draft = "private-search-query.mxf".into();
    app.start_file_search();
    let (id, _) = requests.try_recv().unwrap();
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            matches: vec![entry(123)],
            end: Some(FileSearchEnd::Complete),
            ..FileSearchReport::default()
        },
        None,
    );
    app.stop_recording().unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    for absent in [
        "private-search-query",
        "player-123",
        "映像-123",
        "file_users",
    ] {
        assert!(!saved.contains(absent));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn file_search_reopens_at_query_and_slash_focuses_from_mode_or_results() {
    let (mut app, requests, _) = setup();
    app.file_users.draft = "映像".into();
    app.file_users.report.matches = vec![entry(0), entry(1)];
    app.file_users.selected = 1;
    app.file_users.focus = FileUsersFocus::Results;
    app.file_users.detail = true;
    press(&mut app, KeyCode::F(4));
    assert_eq!(app.file_users.focus, FileUsersFocus::Query);
    assert!(!app.file_users.detail);
    assert_eq!(app.file_users.cursor, "映像".len());
    press(&mut app, KeyCode::Char('/'));
    assert_eq!(app.file_users.draft, "映像/");
    for focus in [FileUsersFocus::Mode, FileUsersFocus::Results] {
        app.file_users.focus = focus;
        let buffer = render_app_to_buffer(&app, 120, 60);
        let (x, y) = super::support::find_text_position(&buffer, "/ query").unwrap();
        app.on_mouse(left_click(x, y), Rect::new(0, 0, 120, 60));
        assert_eq!(app.file_users.focus, FileUsersFocus::Query);
        assert_eq!(app.file_users.draft, "映像/");
        assert_eq!(app.file_users.cursor, "映像/".len());
    }
    app.file_users.focus = FileUsersFocus::Results;
    app.file_users.detail = true;
    press(&mut app, KeyCode::Char('/'));
    assert!(app.file_users.detail);
    assert_eq!(app.file_users.focus, FileUsersFocus::Results);
    press(&mut app, KeyCode::F(2));
    press(&mut app, KeyCode::F(4));
    assert_eq!(app.file_users.focus, FileUsersFocus::Query);
    assert_eq!(app.file_users.report.matches.len(), 2);
    assert_eq!(app.file_users.selected, 1);
    assert!(requests.try_recv().is_err());
}
