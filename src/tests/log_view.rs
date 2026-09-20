use super::support::{
    find_text_position, left_click, make_test_app, render_app_to_buffer, render_app_to_text,
    track_process_name, unique_recording_dir, unique_recording_path,
};
use crate::app;
use crate::app::{AppActivity, DetailsMetric, FocusedPanel, GraphSlot};
use crate::model::SortSpec;
use crate::ui;
use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;

#[test]
fn ctrl_l_opens_log_list() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.show_log_list);
    assert!(app.log_list_worker.is_some());
    assert_eq!(app.log_list_dir, Some(std::env::current_dir().unwrap()));
}

#[test]
fn log_list_renders_session_rows() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::path::PathBuf::from("C:/logs"));
    let started_at = Local.with_ymd_and_hms(2026, 5, 14, 7, 43, 22).unwrap();
    let ended_at = Local.with_ymd_and_hms(2026, 5, 14, 7, 45, 27).unwrap();
    app.log_summaries = vec![app::logs::LogSummary {
        path: std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"),
        schema_version: Some(2),
        session_id: Some("demo".to_string()),
        started_at: Some(started_at),
        ended_at: Some(ended_at),
        host: Some("PC".to_string()),
        tracked_names: vec!["app.exe".to_string()],
        interval_seconds: Some(5),
        frame_count: 12,
        error: None,
    }];

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(!rendered.contains("Log sessions"), "{rendered}");
    assert!(
        rendered.contains("Select a log file and press Enter."),
        "{rendered}"
    );
    assert!(rendered.contains("Dir C:/logs"), "{rendered}");
    assert!(rendered.contains("d change dir"), "{rendered}");
    assert!(rendered.contains("00:02:05"), "{rendered}");
    assert!(rendered.contains("app.exe"), "{rendered}");
    assert!(rendered.contains("Interval 5s"), "{rendered}");
    assert!(
        rendered.contains("Tracked process names (1):"),
        "{rendered}"
    );
    assert!(rendered.contains("winproc-tui-demo.log"), "{rendered}");
    assert!(
        !rendered.contains("C:/logs/winproc-tui-demo.log"),
        "{rendered}"
    );
    for button in ["[ Open ]", "[ Directory ]", "[ Refresh ]", "[ Close ]"] {
        assert!(!rendered.contains(button), "{rendered}");
    }
    assert!(
        rendered.contains("↑/↓ select  Enter open  d change dir  r refresh  Esc close"),
        "{rendered}"
    );
}

#[test]
fn log_list_shows_the_log_being_opened() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::path::PathBuf::from("C:/logs"));
    app.log_summaries = vec![app::logs::LogSummary {
        path: std::path::PathBuf::from("C:/logs/large-session.log"),
        schema_version: Some(2),
        session_id: Some("large".to_string()),
        started_at: Some(Local::now()),
        ended_at: None,
        host: Some("PC".to_string()),
        tracked_names: vec!["app.exe".to_string()],
        interval_seconds: Some(5),
        frame_count: 0,
        error: None,
    }];

    app.load_selected_log();
    let rendered = render_app_to_text(&app, 120, 45);

    assert!(
        rendered.contains("Opening large-session.log..."),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Select a log file and press Enter."),
        "{rendered}"
    );
}

#[test]
fn log_list_ignores_another_open_while_loading() {
    let first_path = std::path::PathBuf::from("C:/logs/first.log");
    let second_path = std::path::PathBuf::from("C:/logs/second.log");
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_summaries = [first_path.clone(), second_path]
        .into_iter()
        .map(|path| app::logs::LogSummary {
            path,
            schema_version: Some(2),
            session_id: None,
            started_at: Some(Local::now()),
            ended_at: None,
            host: None,
            tracked_names: Vec::new(),
            interval_seconds: Some(5),
            frame_count: 0,
            error: None,
        })
        .collect();

    app.load_selected_log();
    app.log_list_index = 1;
    app.load_selected_log();

    let worker = app
        .log_load_worker
        .as_ref()
        .expect("first load stays active");
    assert_eq!(worker.path(), first_path.as_path());
    assert_eq!(app.status, format!("Opening log: {}", first_path.display()));
}

#[test]
fn empty_log_list_explains_how_to_record_or_change_directory() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::path::PathBuf::from("C:/logs"));

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(
        rendered.contains("No .log files. Press d to change directory; Esc then Ctrl+R to record."),
        "{rendered}"
    );
}

#[test]
fn logs_dialog_matches_recording_dialog_width() {
    let screen = Rect::new(0, 0, 120, 45);
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    let logs = render_app_to_buffer(&app, screen.width, screen.height);
    let (logs_x, logs_y) = find_text_position(&logs, "LOGS").expect("Logs title should render");

    app.show_log_list = false;
    app.show_recording_path_dialog = true;
    let recording = render_app_to_buffer(&app, screen.width, screen.height);
    let (recording_x, recording_y) =
        find_text_position(&recording, "RECORDING").expect("Recording title should render");

    assert_eq!(logs_x, recording_x);
    assert_eq!(logs[(logs_x - 2, logs_y)].symbol(), "┏");
    assert_eq!(recording[(recording_x - 2, recording_y)].symbol(), "┏");
    assert_eq!(logs[(logs_x + 75, logs_y)].symbol(), "┓");
    assert_eq!(recording[(recording_x + 75, recording_y)].symbol(), "┓");
}

#[test]
fn ctrl_l_uses_previous_recording_dir_as_default_log_dir() {
    let dir = unique_recording_dir("log-default");
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = make_test_app(1, 10);
    app.recording_last_dir = Some(dir.clone());

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(app.log_list_dir, Some(dir.clone()));
    assert_eq!(app.recording_last_dir, Some(dir.clone()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn log_dir_dialog_changes_active_dir_without_recording_last_dir() {
    let recording_dir = unique_recording_dir("log-recording");
    let selected_dir = unique_recording_dir("log-selected");
    std::fs::create_dir_all(&recording_dir).unwrap();
    std::fs::create_dir_all(&selected_dir).unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.recording_last_dir = Some(recording_dir.clone());
    app.log_list_dir = Some(recording_dir.clone());

    app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_log_dir_dialog);
    app.log_dir_draft = selected_dir.display().to_string();
    app.log_dir_cursor = app.log_dir_draft.len();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_log_dir_dialog);
    assert_eq!(app.log_list_dir, Some(selected_dir.clone()));
    assert_eq!(app.recording_last_dir, Some(recording_dir.clone()));
    assert!(app.log_list_worker.is_some());
    let _ = std::fs::remove_dir_all(recording_dir);
    let _ = std::fs::remove_dir_all(selected_dir);
}

#[test]
fn log_dir_dialog_scans_selected_directory() {
    let selected_dir = unique_recording_dir("log-scan-selected");
    std::fs::create_dir_all(&selected_dir).unwrap();
    let log_path = selected_dir.join("chosen.log");
    std::fs::write(
            &log_path,
            r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["chosen.exe"]}"#,
        )
        .unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::env::current_dir().unwrap());
    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft = selected_dir.display().to_string();
    app.log_dir_cursor = app.log_dir_draft.len();

    app.confirm_log_dir().unwrap();
    for _ in 0..100 {
        if app.poll_log_workers() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    assert_eq!(app.log_summaries.len(), 1);
    assert_eq!(app.log_summaries[0].path, log_path);
    let _ = std::fs::remove_dir_all(selected_dir);
}

#[test]
fn log_dir_dialog_rejects_missing_directory() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::env::current_dir().unwrap());

    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft = unique_recording_dir("missing-log-dir")
        .display()
        .to_string();
    app.log_dir_cursor = app.log_dir_draft.len();
    app.confirm_log_dir().unwrap();

    assert!(app.show_log_dir_dialog);
    assert_eq!(
        app.log_dir_error.as_deref(),
        Some("Directory does not exist.")
    );
    assert!(app.status.starts_with("Log directory does not exist:"));
    assert!(app.log_list_worker.is_none());
    let rendered = render_app_to_text(&app, 120, 45);
    assert!(rendered.contains("Directory does not exist."), "{rendered}");
}

#[test]
fn log_dir_dialog_rejects_empty_directory() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;

    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft.clear();
    app.log_dir_cursor = 0;
    app.confirm_log_dir().unwrap();

    assert!(app.show_log_dir_dialog);
    assert_eq!(app.log_dir_error.as_deref(), Some("Directory is empty."));
    assert!(app.log_list_worker.is_none());
}

#[test]
fn log_dir_dialog_rejects_file_path() {
    let path = unique_recording_dir("log-dir-file");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not a directory").unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;

    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft = path.display().to_string();
    app.log_dir_cursor = app.log_dir_draft.len();
    app.confirm_log_dir().unwrap();

    assert!(app.show_log_dir_dialog);
    assert_eq!(
        app.log_dir_error.as_deref(),
        Some("Path is not a directory.")
    );
    assert!(app.log_list_worker.is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn log_dir_dialog_shows_shortcuts_below_directory_input() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.open_log_dir_dialog().unwrap();
    let buffer = render_app_to_buffer(&app, 120, 45);
    let (_, shortcut_y) =
        find_text_position(&buffer, "Enter apply  Esc cancel  Ctrl+Space Complete path")
            .expect("directory shortcuts should render");
    assert!(find_text_position(&buffer, "Ctrl+Space Complete path").is_some());
    let (_, label_y) =
        find_text_position(&buffer, "Directory").expect("directory label should render");

    assert!(shortcut_y > label_y);
}

#[test]
fn ctrl_space_completes_log_dir_dialog_directory() {
    let root = unique_recording_dir("log-dir-complete");
    let target = root.join("alpha");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&target).unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
    app.log_dir_cursor = app.log_dir_draft.len();

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
        .unwrap();

    let expected = format!(
        "{}{}alpha{}",
        root.display(),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    assert_eq!(app.log_dir_draft, expected);
    assert_eq!(app.log_dir_cursor, app.log_dir_draft.len());
    assert_eq!(app.status, "Completed directory");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn log_dir_backspace_handles_key_repeat_and_ignores_release() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.open_log_dir_dialog().unwrap();
    app.log_dir_draft = "C:/logs/example".to_string();
    app.log_dir_cursor = app.log_dir_draft.len();

    app.on_key(KeyEvent::new_with_kind(
        KeyCode::Backspace,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ))
    .unwrap();
    app.on_key(KeyEvent::new_with_kind(
        KeyCode::Backspace,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ))
    .unwrap();

    assert_eq!(app.log_dir_draft, "C:/logs/exampl");
    assert_eq!(app.log_dir_cursor, app.log_dir_draft.len());
}

#[test]
fn log_list_refresh_uses_active_manual_dir() {
    let recording_dir = unique_recording_dir("log-refresh-recording");
    let selected_dir = unique_recording_dir("log-refresh-selected");
    std::fs::create_dir_all(&recording_dir).unwrap();
    std::fs::create_dir_all(&selected_dir).unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.recording_last_dir = Some(recording_dir.clone());
    app.log_list_dir = Some(selected_dir.clone());

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.log_list_dir, Some(selected_dir.clone()));
    assert_eq!(app.recording_last_dir, Some(recording_dir.clone()));
    assert!(app.status.contains(&selected_dir.display().to_string()));
    let _ = std::fs::remove_dir_all(recording_dir);
    let _ = std::fs::remove_dir_all(selected_dir);
}

#[test]
fn log_dir_escape_closes_dialog() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_list_dir = Some(std::env::current_dir().unwrap());
    app.open_log_dir_dialog().unwrap();
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_log_dir_dialog);
}

#[test]
fn log_list_click_selects_row() {
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_summaries = vec![
        app::logs::LogSummary {
            path: std::path::PathBuf::from("C:/logs/first.log"),
            schema_version: Some(2),
            session_id: None,
            started_at: Some(Local::now()),
            ended_at: None,
            host: None,
            tracked_names: vec!["first.exe".to_string()],
            interval_seconds: Some(5),
            frame_count: 0,
            error: None,
        },
        app::logs::LogSummary {
            path: std::path::PathBuf::from("C:/logs/second.log"),
            schema_version: Some(2),
            session_id: None,
            started_at: Some(Local::now()),
            ended_at: None,
            host: None,
            tracked_names: vec!["second.exe".to_string()],
            interval_seconds: Some(5),
            frame_count: 0,
            error: None,
        },
    ];
    app.log_list_index = 0;
    let screen = Rect::new(0, 0, 140, 45);
    app.set_log_list_page_size(ui::log_list_page_size_for_screen(screen));
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) =
        find_text_position(&buffer, "second.log").expect("second log row should be rendered");

    app.on_mouse(left_click(x, y), screen);

    assert_eq!(app.log_list_index, 1);
    assert!(app.log_load_worker.is_none());
}

#[test]
fn log_list_double_click_opens_row() {
    let path = std::env::temp_dir().join(format!(
        "winproc-tui-log-double-click-test-{}-{}.log",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::write(
            &path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
    let mut app = make_test_app(1, 10);
    app.show_log_list = true;
    app.log_summaries = vec![app::logs::LogSummary {
        path: path.clone(),
        schema_version: Some(2),
        session_id: Some("s1".to_string()),
        started_at: Some(Local::now()),
        ended_at: None,
        host: Some("PC".to_string()),
        tracked_names: vec!["app.exe".to_string()],
        interval_seconds: Some(5),
        frame_count: 0,
        error: None,
    }];
    let screen = Rect::new(0, 0, 180, 45);
    app.set_log_list_page_size(ui::log_list_page_size_for_screen(screen));
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position(&buffer, "> v2").expect("log row should be rendered");

    app.on_mouse(left_click(x, y), screen);
    app.on_mouse(left_click(x, y), screen);

    assert!(app.log_load_worker.is_some());
    assert!(app.status.starts_with("Opening log:"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn log_view_header_shows_log_badge_and_path_without_freshness() {
    let mut app = make_test_app(1, 10);
    app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

    let rendered = render_app_to_text(&app, 100, 20);
    let buffer = render_app_to_buffer(&app, 100, 20);
    let (_, log_y) = find_text_position(&buffer, "LOG").expect("log badge should be rendered");

    assert!(rendered.contains("LOG"), "{rendered}");
    assert!(rendered.contains("Session ▾"), "{rendered}");
    assert!(rendered.contains("PF: --"), "{rendered}");
    let (profile_x, profile_y) =
        find_text_position(&buffer, "PF: --").expect("profile badge should render");
    assert_eq!(buffer[(profile_x, profile_y)].fg, Color::Black);
    assert_eq!(buffer[(profile_x, profile_y)].bg, app.theme().muted);
    assert_eq!(log_y, 0);
    assert!(!rendered.contains("fresh"), "{rendered}");
    assert!(!rendered.contains("STALE"), "{rendered}");
    assert!(
        rendered.contains("winproc-t") && rendered.contains("-demo.log"),
        "{rendered}"
    );
    assert!(
        !rendered.contains(&format!("winproc-tui {}", env!("CARGO_PKG_VERSION"))),
        "{rendered}"
    );
}

#[test]
fn log_view_header_keeps_the_filename_suffix_and_hides_product_at_narrow_width() {
    let mut app = make_test_app(1, 10);
    let path = "C:/logs/winproc-tui-demo.log";
    let product_and_version = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));
    app.log_view_path = Some(std::path::PathBuf::from(path));

    let rendered = render_app_to_text(&app, 80, 20);
    let header = rendered.lines().next().expect("header row");

    assert!(rendered.contains("LOG"), "{rendered}");
    assert!(rendered.contains("Session ▾"), "{rendered}");
    assert!(!rendered.contains(path), "{rendered}");
    assert!(header.contains(".log"), "{header}");
    assert!(!rendered.contains(&product_and_version), "{rendered}");
}

#[test]
fn display_pause_is_unavailable_in_log_view() {
    let mut app = make_test_app(1, 10);
    app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

    let rendered = render_app_to_text(&app, 240, 30);
    assert!(!rendered.contains("Ctrl+P Pause"), "{rendered}");
    assert!(rendered.contains("ESC Menu"), "{rendered}");

    app.toggle_display_pause();

    assert!(!app.is_display_paused());
    assert_eq!(app.status, "Display pause is unavailable in Log view");
}

#[test]
fn log_view_esc_opens_menu_without_returning_to_live_or_requesting_quit() {
    let mut app = make_test_app(1, 10);
    app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.activity(), AppActivity::LogView);
    assert!(app.log_view_path.is_some());
    assert!(app.is_main_menu_open());
    assert!(!app.show_quit_confirmation);
}

#[test]
fn ctrl_r_is_rejected_in_log_view() {
    let mut app = make_test_app(1, 10);
    app.log_view_path = Some(std::path::PathBuf::from("C:/logs/winproc-tui-demo.log"));

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(app.activity(), AppActivity::LogView);
    assert_eq!(app.status, "Recording is unavailable in Log view");
}

#[test]
fn ctrl_l_is_rejected_during_recording() {
    let path = unique_recording_path("deny-log-view");
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(app.activity(), AppActivity::Recording);
    assert!(!app.show_log_list);
    assert_eq!(app.status, "Log view is unavailable during recording");

    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn loaded_log_is_ignored_if_recording_started_before_worker_returns() {
    let log_view_path = std::env::temp_dir().join(format!(
        "winproc-tui-log-view-race-test-{}-{}.log",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::write(
            &log_view_path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
    let loaded = app::logs::load_log(&log_view_path, SortSpec::default()).unwrap();
    let recording_path = unique_recording_path("deny-loaded-log-view");
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = recording_path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();

    app.apply_loaded_log(loaded);

    assert_eq!(app.activity(), AppActivity::Recording);
    assert!(app.log_view_path.is_none());
    assert_eq!(app.status, "Log view is unavailable during recording");

    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(recording_path);
    let _ = std::fs::remove_file(log_view_path);
}

#[test]
fn loaded_log_feeds_graph_samples_without_turning_missing_values_to_zero() {
    let path = std::env::temp_dir().join(format!(
        "winproc-tui-log-view-test-{}-{}.log",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::write(
            &path,
            [
                r#"{"schema_version":2,"record_type":"session","session_id":"s1","host":"PC","started_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:12+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":100,"total_memory_bytes":1000},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":null}}]}"#,
                r#"{"schema_version":2,"record_type":"frame","session_id":"s1","captured_at":"2026-05-04T14:30:13+09:00","tracked_names":["app.exe"],"system_metrics":{"physical_memory_bytes":200,"total_memory_bytes":1000},"processes":[{"pid":1,"name":"app.exe","start_time":100,"metrics":{"private_bytes":1024}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
    let loaded = app::logs::load_log(&path, SortSpec::default()).unwrap();
    let mut app = make_test_app(1, 10);

    app.apply_loaded_log(loaded);
    let identity = app.visible_process_identity_at(0).unwrap();
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::Private),
        FocusedPanel::Processes,
    );
    app.focused_panel = FocusedPanel::DetailsSamples;
    let samples = app.graph_slot_samples(app.graph_slot(0).unwrap());

    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].value, None);
    assert_eq!(samples[1].value, Some(1024.0));

    let rendered = render_app_to_text(&app, 120, 45);
    assert!(
        rendered.contains("Slot#1 · PrivBytes · PID 1 app.exe"),
        "{rendered}"
    );
    assert!(rendered.contains("A/B Time      PrivBytes"), "{rendered}");
    assert!(rendered.contains("1,024"), "{rendered}");
}

#[test]
fn recording_writes_v3_session_definitions_frames_and_end_records() {
    let path = unique_recording_path("v3-session");
    let mut app = make_test_app(1, 10);
    app.watch_list = vec!["proc-0".to_string()];
    app.normalized_watch_names = std::collections::HashSet::from(["proc-0".to_string()]);
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;

    app.confirm_recording_path().unwrap();
    app.watch_list = vec!["other.exe".to_string()];
    app.normalized_watch_names = std::collections::HashSet::from(["other.exe".to_string()]);
    let initial_captured_at = app.snapshot.captured_at;
    for generation in 1..=2_u32 {
        app.snapshot.captured_at =
            initial_captured_at + chrono::Duration::seconds(i64::from(generation));
        app.snapshot.processes[0].pid = 100 + generation;
        app.snapshot.processes[0].start_time = Some(1_800_000_000 + u64::from(generation));
        app.write_current_recording_frame().unwrap();
    }
    app.stop_recording().unwrap();

    let lines = std::fs::read_to_string(&path).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<app::log_format::V3Record>(line).unwrap())
        .collect::<Vec<_>>();
    let app::log_format::V3Record::Session(session) = &records[0] else {
        panic!("first record must be a schema v3 session");
    };
    assert_eq!(session.schema_version, 3);
    assert_eq!(session.tracked_names, ["proc-0"]);

    let definitions = records
        .iter()
        .filter_map(|record| match record {
            app::log_format::V3Record::Process(definition) => Some(definition),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 3);
    assert_eq!(definitions[0].2, "proc-0");
    assert_eq!(definitions[1].1, 101);
    assert_eq!(definitions[2].1, 102);

    let frames = records
        .iter()
        .filter_map(|record| match record {
            app::log_format::V3Record::Frame(frame) => Some(frame),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 3);
    assert_eq!(
        frames[0].1.0[app::log_format::system_u64::PHYSICAL_MEMORY],
        Some(0)
    );
    assert_eq!(frames[0].2[0].0, definitions[0].0);
    assert_eq!(frames[1].2[0].0, definitions[1].0);
    assert_eq!(frames[2].2[0].0, definitions[2].0);
    assert!(matches!(
        records.last(),
        Some(app::log_format::V3Record::End(_))
    ));
    let loaded = app::logs::load_log(&path, SortSpec::default()).unwrap();
    assert_eq!(loaded.process_history.identity_count(), 3);
    let _ = std::fs::remove_file(path);
}

#[test]
fn recording_interval_is_written_and_partial_window_is_flushed() {
    let path = unique_recording_path("10s-partial-window");
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.recording_interval_index = 3;
    app.show_recording_path_dialog = true;

    app.confirm_recording_path().unwrap();

    assert_eq!(app.active_recording_interval_seconds(), Some(10));
    let recording_header = render_app_to_text(&app, 120, 45);
    assert!(recording_header.contains("REC"), "{recording_header}");
    assert!(recording_header.contains("10s AVG"), "{recording_header}");
    app.stop_recording().unwrap();

    let records = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<app::log_format::V3Record>(line).unwrap())
        .collect::<Vec<_>>();
    let app::log_format::V3Record::Session(session) = &records[0] else {
        panic!("first record must be a schema v3 session");
    };
    assert_eq!(session.interval_seconds, 10);
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, app::log_format::V3Record::Frame(_)))
            .count(),
        1
    );

    let loaded = app::logs::load_log(&path, SortSpec::default()).unwrap();
    assert_eq!(loaded.interval_seconds, 10);
    assert_eq!(loaded.frame_times.len(), 1);
    app.apply_loaded_log(loaded);
    let log_header = render_app_to_text(&app, 120, 45);
    assert!(log_header.contains("LOG"), "{log_header}");
    assert!(log_header.contains("10s AVG"), "{log_header}");

    let captured_at = app.log_view_frame_times[0];
    let identity = app.visible_process_identity_at(0).unwrap();
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at }),
        b: Some(app::AbComparisonPoint { captured_at }),
    });
    let range_summary = render_app_to_text(&app, 180, 55);
    assert!(
        range_summary.contains("A-B min (10s avg):"),
        "{range_summary}"
    );
    assert!(
        range_summary.contains("A-B samples: 1/1  Missing: 0"),
        "{range_summary}"
    );
    let _ = std::fs::remove_file(path);
}
