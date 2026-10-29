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
// Establish an empty completed capture for tests of post-capture interactions.
fn open_ready(app: &mut App, requests: &Receiver<(u64, FileUsersRequest)>) {
    app.open_file_users();
    if let Ok((_, request)) = requests.try_recv() {
        assert!(matches!(request, FileUsersRequest::CaptureAll));
        app.file_users.pending = None;
        app.file_users.report.end = Some(FileSearchEnd::Complete);
    }
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
fn file_users_collects_on_open_and_filters_without_new_requests() {
    let (mut app, requests, updates) = setup();
    app.filter_text = "not-the-file-owner".into();
    app.open_file_users();
    let (id, request) = requests.try_recv().unwrap();
    assert!(matches!(request, FileUsersRequest::CaptureAll));
    assert!(render_app_to_text(&app, 120, 35).contains("Collecting file list..."));
    for ch in "MEDIA 映像-1".chars() {
        press(&mut app, KeyCode::Char(ch));
    }
    app.start_file_search();
    assert!(requests.try_recv().is_err());
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            matches: vec![entry(0), entry(1), entry(2)],
            end: Some(FileSearchEnd::Complete),
            ..Default::default()
        },
        None,
    );
    assert_eq!(app.file_users.filtered_count(), 1);
    assert_eq!(app.file_users.selected_entry().unwrap().path, entry(1).path);
    assert_eq!(app.file_users.focus, FileUsersFocus::Query);
    assert!(render_app_to_text(&app, 120, 35).contains("1/3 matches"));
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.file_users.filtered_count(), 3);
    assert_eq!(app.file_users.selected_entry().unwrap().path, entry(1).path);
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.file_users.draft.is_empty());
    assert_eq!(app.file_users.filtered_count(), 3);
    assert!(requests.try_recv().is_err());
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.file_users.focus, FileUsersFocus::Results);
    assert!(requests.try_recv().is_err());
    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(matches!(
        requests.try_recv().unwrap().1,
        FileUsersRequest::CaptureAll
    ));
    app.close_file_users();
    app.open_file_users();
    assert!(matches!(
        requests.try_recv().unwrap().1,
        FileUsersRequest::CaptureAll
    ));
}

#[test]
fn file_users_cancellation_retains_partial_results_and_rejects_stale_updates() {
    let (mut app, requests, updates) = setup();
    open_ready(&mut app, &requests);
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
fn file_users_filtered_rows_drive_mouse_copy_and_partial_empty_feedback() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
    app.file_users.report.matches = (0..40).map(entry).collect();
    app.file_users.report.end = Some(FileSearchEnd::ResultLimit);
    for ch in "MEDIA 映像-2".chars() {
        press(&mut app, KeyCode::Char(ch));
    }
    assert_eq!(app.file_users.filtered_count(), 11);
    let screen = Rect::new(0, 0, 120, 35);
    let buffer = render_app_to_buffer(&app, 120, 35);
    let rows = ui::file_users::content_layout(ui::file_users::browser_layout(screen).content).rows;
    let (x, y) = super::support::find_text_position_in_area(&buffer, rows, "1254").unwrap();
    app.on_mouse(left_click(x, y), screen);
    assert_eq!(
        app.file_users.selected_entry().unwrap().path,
        entry(20).path
    );
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().unwrap(),
        entry(20).plain_text()
    );
    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Char('x'));
    assert!(app.file_users.selected_entry().is_none());
    let text = render_app_to_text(&app, 120, 35);
    assert!(text.contains("Result limit reached (partial results)"));
    assert!(text.contains("0/40 matches"));
    assert!(text.contains("No matches for Query"));
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.file_users.filtered_count(), 40);
    assert_eq!(app.file_users.selected, 0);
    assert!(requests.try_recv().is_err());
}

#[test]
fn file_users_partial_zero_results_show_coverage_and_full_diagnostics() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
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
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
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
        assert_eq!(buffer[(x, y)].bg, app.theme().table_selection_surface);
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
    open_ready(&mut app, &requests);
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
fn file_users_menu_collects_only_in_live_mode() {
    let (mut app, requests, _) = setup();
    app.activate_header_action(ui::header::HeaderAction::FileUsers);
    assert!(app.file_users.visible);
    assert!(matches!(
        requests.try_recv().unwrap().1,
        FileUsersRequest::CaptureAll
    ));
    app.close_file_users();
    app.log_view_path = Some(PathBuf::from("example.jsonl"));
    open_ready(&mut app, &requests);
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
    open_ready(&mut app, &requests);
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
    open_ready(&mut app, &requests);
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
    {
        app.file_users.focus = FileUsersFocus::Results;
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
    assert!(matches!(
        requests.try_recv().unwrap().1,
        FileUsersRequest::CaptureAll
    ));
    assert!(app.file_users.pending.is_some());
}

#[test]
fn file_users_handoff_reveals_path_without_filtering_duplicate_handles() {
    use crate::samplers::open_files::{OpenFilesRequest, OpenFilesResult, OpenFilesWorker};
    let (mut app, requests, updates) = setup();
    let (worker, captures, results) = OpenFilesWorker::test_pair();
    app.open_files_worker = worker;
    open_ready(&mut app, &requests);
    app.file_users.draft = "media".into();
    app.file_users.focus = FileUsersFocus::Results;
    app.open_files_filter = "old filter".into();
    let original = entry(0);
    app.file_users.report.matches = vec![original.clone(), entry(1)];
    app.file_users.report.progress.denied_processes = 3;
    press(&mut app, KeyCode::Enter);
    let (id, _) = requests.try_recv().unwrap();
    press(&mut app, KeyCode::Down);
    deliver(
        &mut app,
        &updates,
        id,
        FileSearchReport {
            end: Some(FileSearchEnd::Complete),
            ..Default::default()
        },
        Some(original.owner.clone()),
    );
    assert!(app.open_files_filter.is_empty());
    assert_eq!(app.file_users.selected, 1);
    let OpenFilesRequest::Collect {
        generation,
        identity,
        ..
    } = captures.try_recv().unwrap()
    else {
        panic!()
    };
    let mut report =
        super::support::test_open_files_report("player-0.exe", original.owner.pid, "unrelated.dll");
    let mut same_name = report.entries[0].clone();
    same_name.path = original.path.replace("media", "other");
    report.entries.push(same_name);
    let mut matched = report.entries[0].clone();
    matched.path = original.path.clone();
    matched.handle.value = 100;
    report.entries.push(matched.clone());
    matched.handle.value = 104;
    report.entries.push(matched);
    let send = |report, generation, identity| {
        results
            .send(OpenFilesResult {
                report,
                generation,
                identity,
                elapsed: std::time::Duration::from_millis(10),
            })
            .unwrap()
    };
    send(report.clone(), generation + 1, identity.clone());
    assert!(!app.poll_open_files_results().unwrap());
    send(report.clone(), generation, identity.clone());
    assert!(app.poll_open_files_results().unwrap());
    assert_eq!(ui::open_files::filtered_entries(&app).len(), 4);
    assert_eq!(
        app.process_info_focus,
        crate::app::ProcessInfoFocus::Content
    );
    let rendered = render_app_to_text(&app, 180, 42);
    assert!(rendered.contains("Filter: "));
    assert!(!rendered.contains("Exact:"));
    assert_eq!(
        ui::open_files::selected_entry(&app).unwrap().path,
        original.path
    );
    assert_eq!(app.open_files_selected, 2);
    assert!(app.open_files_match_path.is_none());
    assert!(!render_app_to_text(&app, 180, 42).contains("matched handles"));
    app.select_open_file(0);
    app.refresh_open_files().unwrap();
    let OpenFilesRequest::Collect {
        generation,
        identity,
        ..
    } = captures.try_recv().unwrap()
    else {
        panic!()
    };
    report.entries.truncate(1);
    send(report, generation, identity);
    assert!(app.poll_open_files_results().unwrap());
    assert_eq!(app.open_files_selected, 0);
    assert!(ui::open_files::selected_entry(&app).is_some());
    press(&mut app, KeyCode::Char('/'));
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
    press(&mut app, KeyCode::Enter);
    assert!(app.open_files_filter.is_empty());
    assert_eq!(ui::open_files::filtered_entries(&app).len(), 1);
    app.close_process_info_dialog();
    assert_eq!(app.file_users.draft, "media");
    assert_eq!(app.file_users.selected, 1);
    assert_eq!(app.file_users.report.progress.denied_processes, 3);
}

#[test]
fn file_users_has_only_query_and_results_controls() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
    for key in [KeyCode::Tab, KeyCode::BackTab] {
        press(&mut app, key);
        assert_eq!(app.file_users.focus, FileUsersFocus::Results);
        press(&mut app, key);
        assert_eq!(app.file_users.focus, FileUsersFocus::Query);
    }
    let buffer = render_app_to_buffer(&app, 120, 48);
    assert!(super::support::find_text_position(&buffer, "Mode:").is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn file_user_query_and_results_use_matching_focus_colors() {
    use ratatui::style::Modifier;
    for width in [65, 140] {
        let mut app = make_test_app(1, 10);
        app.file_users.visible = true;
        for focus in [FileUsersFocus::Query, FileUsersFocus::Results] {
            app.file_users.focus = focus;
            let buffer = render_app_to_buffer(&app, width, 40);
            for (label, active) in [
                ("Query:", focus == FileUsersFocus::Query),
                ("PID", focus == FileUsersFocus::Results),
            ] {
                let xy = super::support::find_text_position(&buffer, label).unwrap();
                let cell = &buffer[xy];
                assert_eq!(
                    cell.fg,
                    if active {
                        app.theme().focus_border
                    } else {
                        app.theme().text
                    }
                );
                assert_eq!(cell.modifier.contains(Modifier::BOLD), active);
            }
        }
    }
}

#[test]
fn file_users_and_terms_and_quoted_paths() {
    let query = |text: &str| FileSearchQuery {
        text: text.into(),
        mode: FileSearchMode::Path,
    };
    let path = r"C:\Program Files\映像 player\clip.mxf";
    assert!(query(r#""Program Files" 映像 .MXF"#).matches(path));
    assert!(!query("Program missing").matches(path));
    assert!(!query(r#""Files Program""#).matches(path));
    assert!(query(r#""C:\Program Files\映像 player\clip.mxf""#).matches(path));
    assert!(query(r#"  ""  "#).validate().is_err());
}

#[test]
fn file_users_direct_actions_preserve_query_priority_and_parent() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
    app.file_users.report.matches = vec![entry(0)];
    app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.file_action.is_none());
    app.open_path_actions(PathBuf::from("previous"));
    let _pending = app.inject_pending_file_action();
    app.file_action = None;
    app.file_users.focus = FileUsersFocus::Results;
    app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        app.file_action.as_ref().unwrap().path,
        PathBuf::from(entry(0).path)
    );
    assert!(render_app_to_text(&app, 160, 48).contains("FILE ACTIONS"));
    press(&mut app, KeyCode::Esc);
    assert!(app.file_action.is_none());
    assert!(app.file_users.visible);
    assert_eq!(app.file_users.selected, 0);
}

#[test]
fn query_contains_only_editable_text_during_draft_changes_and_notices() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
    app.file_users.draft = "new".into();
    app.file_users.cursor = 3;
    app.file_users.report.matches = vec![entry(0)];
    app.file_users.report.end = Some(FileSearchEnd::Complete);
    app.file_users.focus = FileUsersFocus::Results;
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    for notice in ["Copied file user", "Clipboard failed", "Cancelling..."] {
        app.file_users.notice = Some(notice.into());
        for width in [40, 60, 160] {
            let text = render_app_to_text(&app, width, 24);
            assert!(text.contains("Query: new"), "{text}");
            assert!(!text.contains("[unsearched]"), "{text}");
            assert!(text.contains(notice), "{text}");
        }
    }
    assert!(requests.try_recv().is_err());
    app.file_users.focus = FileUsersFocus::Query;
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    assert!(app.file_users.draft.is_empty());
    assert!(!render_app_to_text(&app, 60, 24).contains("[unsearched]"));
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.file_users.draft, "x");
    assert!(render_app_to_text(&app, 60, 24).contains("Query: x"));
    app.file_users.draft = "old".into();
    assert!(!render_app_to_text(&app, 60, 24).contains("[unsearched]"));
    app.file_users.draft = "new".into();
    app.start_file_search();
    assert!(requests.try_recv().is_ok());
    assert!(!render_app_to_text(&app, 60, 24).contains("[unsearched]"));
}

#[test]
fn empty_query_never_shows_change_guidance_before_or_after_search() {
    let (mut app, requests, _) = setup();
    open_ready(&mut app, &requests);
    let guidance = "Query changed; start a search to update results";
    assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
    press(&mut app, KeyCode::Char('x'));
    assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
    app.file_users.report.end = Some(FileSearchEnd::Complete);
    assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
    press(&mut app, KeyCode::Char('y'));
    assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    assert!(app.file_users.draft.is_empty());
    for draft in ["", "   ", "　"] {
        app.file_users.draft = draft.into();
        app.file_users.cursor = draft.len();
        assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
        assert!(
            !ui::file_users::detail_lines(&app.file_users, 120)
                .iter()
                .any(|line| line.contains("Draft query not searched"))
        );
    }
    app.close_file_users();
    open_ready(&mut app, &requests);
    assert!(!render_app_to_text(&app, 120, 24).contains(guidance));
    assert!(requests.try_recv().is_err());
}
