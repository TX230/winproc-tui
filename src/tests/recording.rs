use super::support::{
    AlwaysFailWriter, assert_blank_row_above_text, assert_modal_rect_focus_border,
    assert_title_style, buffer_to_text, find_text_position, make_test_app,
    make_test_app_with_worker, render_app_to_buffer, render_app_to_text, track_process_name,
    unique_recording_dir, unique_recording_path,
};
use crate::app;
use crate::app::export::MAX_RECORDING_DURATION;
use crate::app::{AppActivity, FocusedPanel, SAMPLE_STALE_AFTER_SECONDS};
use crate::samplers::{CollectSnapshotResult, SamplingWorker};
use crate::ui;
use crate::ui::tracked_remove_dialog_area;
use chrono::Local;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};

#[test]
fn ctrl_r_requires_tracked_processes_before_opening_recording_dialog() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.show_recording_no_tracked_warning);
    assert!(!app.show_recording_path_dialog);
    assert_eq!(app.status, "No tracked processes to record");

    let rendered = render_app_to_text(&app, 100, 45);
    assert!(
        rendered.contains("The tracking list is empty."),
        "{rendered}"
    );
    assert!(
        rendered.contains("In Processes, focus a process and press t to track its name."),
        "{rendered}"
    );
    assert!(!rendered.contains("[ OK ]"), "{rendered}");
    assert!(rendered.contains("Enter/Esc Close"), "{rendered}");
}

#[test]
fn recording_start_dialog_discloses_the_fixed_tracking_scope() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.open_recording_path_dialog().unwrap();

    let rendered = render_app_to_text(&app, 100, 45);

    assert!(
        rendered.contains("Confirm the log file and interval, then press Enter to start."),
        "{rendered}"
    );
    assert!(
        rendered.contains("Tracking List  1 entry (fixed while recording)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Format         JSON Lines (.log)"),
        "{rendered}"
    );
    assert!(rendered.contains("Max duration   24 hours"), "{rendered}");
    assert!(!rendered.contains("Ctrl+L"), "{rendered}");
    assert!(!rendered.contains("WARNING"), "{rendered}");
}

#[test]
fn recording_automatically_stops_at_24_hour_limit() {
    let path = unique_recording_path("duration-limit");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    app.request_recording_stop();
    app.open_main_menu();

    assert!(app.enforce_recording_duration_limit_for_test(MAX_RECORDING_DURATION));
    assert_eq!(app.activity(), AppActivity::Live);
    assert!(!app.show_recording_stop_confirmation);
    assert!(!app.is_main_menu_open());
    assert_eq!(
        app.status,
        format!(
            "24-hour recording limit reached; saved log to: {}",
            path.display()
        )
    );

    let contents = std::fs::read_to_string(&path).unwrap();
    let last_record = serde_json::from_str::<app::log_format::V3Record>(
        contents
            .lines()
            .last()
            .expect("recording must have an end record"),
    )
    .unwrap();
    let app::log_format::V3Record::End(app::log_format::V3EndRecord(_, reason)) = last_record
    else {
        panic!("recording must end with an end record");
    };
    assert_eq!(reason, "duration_limit");

    let _ = std::fs::remove_file(path);
}

#[test]
fn recording_duration_limit_write_failure_shows_error() {
    let path = unique_recording_path("duration-limit-error");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    app.replace_recording_writer_for_test(Box::new(AlwaysFailWriter));

    assert!(app.enforce_recording_duration_limit_for_test(MAX_RECORDING_DURATION));
    assert_eq!(app.activity(), AppActivity::Live);
    assert!(app.recording_session.is_none());
    assert_eq!(
        app.recording_error
            .as_ref()
            .expect("duration-limit write error should be visible")
            .kind,
        app::state::RecordingErrorKind::Stopped
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn recording_rejects_tracking_list_changes_but_allows_tracked_only() {
    let path = unique_recording_path("fixed-tracking-controls");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app(2, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();

    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_recording_tracking_fixed);
    assert_eq!(app.watch_list, vec!["proc-0"]);
    let rendered = render_app_to_text(&app, 260, 45);
    assert!(
        rendered.contains("Tracking List is fixed while recording."),
        "{rendered}"
    );
    assert!(
        rendered.contains("Stop recording before changing it."),
        "{rendered}"
    );
    assert!(rendered.contains("Enter/Esc Close"), "{rendered}");
    assert!(!rendered.contains("Ctrl+R Stop"), "{rendered}");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    app.selected_process_column_index = 0;
    let footer = render_app_to_text(&app, 260, 45);
    assert!(!footer.contains("Space Track"), "{footer}");
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_recording_tracking_fixed);
    assert_eq!(app.watch_list, vec!["proc-0"]);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(!app.show_recording_tracking_fixed);
    assert!(app.investigation_profiles_dialog.is_some());

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    let tracked_only_before = app.watch_enabled;
    app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT))
        .unwrap();
    assert_ne!(app.watch_enabled, tracked_only_before);

    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn ctrl_r_confirms_stop_and_defaults_to_continue() {
    let path = unique_recording_path("confirm-stop");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.show_recording_stop_confirmation);
    assert_eq!(app.activity(), AppActivity::Recording);
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(rendered.contains("Stop recording?"), "{rendered}");
    assert!(
        rendered.contains("Recording continues until Stop is confirmed."),
        "{rendered}"
    );
    assert!(!rendered.contains("[ Stop ]"), "{rendered}");
    assert!(!rendered.contains("[ Continue ]"), "{rendered}");
    assert!(
        rendered.contains("Enter/Esc/n Continue  y Stop"),
        "{rendered}"
    );
    let buffer = render_app_to_buffer(&app, 100, 45);
    for shortcut in ["Enter/Esc/n Continue", "y Stop"] {
        let (key_x, key_y) = find_text_position(&buffer, shortcut)
            .unwrap_or_else(|| panic!("{shortcut} should render"));
        assert_eq!(buffer[(key_x, key_y)].fg, app.theme().warning);
        assert!(buffer[(key_x, key_y)].modifier.contains(Modifier::BOLD));
    }

    app.write_current_recording_frame().unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.show_recording_stop_confirmation);
    assert_eq!(app.activity(), AppActivity::Recording);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_recording_stop_confirmation);
    assert_eq!(app.activity(), AppActivity::Recording);

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.activity(), AppActivity::Live);
    assert!(!app.show_recording_stop_confirmation);

    let contents = std::fs::read_to_string(&path).unwrap();
    let records = contents
        .lines()
        .map(|line| serde_json::from_str::<app::log_format::V3Record>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, app::log_format::V3Record::Frame(_)))
            .count(),
        2
    );
    assert!(
        records
            .iter()
            .last()
            .is_some_and(|record| matches!(record, app::log_format::V3Record::End(_)))
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn recording_no_tracked_warning_closes_with_escape_or_enter() {
    for key in [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        let mut app = make_test_app(1, 10);
        app.show_recording_no_tracked_warning = true;

        app.on_key(key).unwrap();

        assert!(!app.show_recording_no_tracked_warning);
        assert_eq!(app.status, "Recording canceled");
    }
}

#[test]
fn warning_dialogs_group_close_keys_and_color_all_keys_like_the_border() {
    let mut display = make_test_app(1, 10);
    display.show_display_area_warning = true;

    let mut metric = make_test_app(1, 10);
    metric.show_metric_column_warning = true;

    let mut graph = make_test_app(1, 10);
    graph.show_no_graph_metrics_warning = true;

    let mut recording = make_test_app(1, 10);
    recording.show_recording_no_tracked_warning = true;

    for (app, name) in [
        (display, "display-area"),
        (metric, "metric"),
        (graph, "graph"),
        (recording, "recording"),
    ] {
        let buffer = render_app_to_buffer(&app, 100, 45);
        let (key_x, key_y) = find_text_position(&buffer, "Enter/Esc Close")
            .unwrap_or_else(|| panic!("{name} warning should show grouped close shortcuts"));
        assert_eq!(buffer[(key_x, key_y)].fg, app.theme().warning, "{name}");
        assert!(
            buffer[(key_x, key_y)].modifier.contains(Modifier::BOLD),
            "{name}"
        );
        assert_blank_row_above_text(&buffer, "Enter/Esc Close");
    }
}

#[test]
fn recording_path_dialog_cycles_path_and_interval_controls_without_buttons() {
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("definitely-no-such-prefix")
        .join("example.log")
        .display()
        .to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    let before = app.recording_path_draft.clone();

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.recording_path_draft, before);
    assert!(app.recording_interval_focused());
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_recording_interval_seconds(), 2);
    app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.recording_path_focused());

    let rendered = render_app_to_text(&app, 100, 45);
    assert!(!rendered.contains("[ Start ]"), "{rendered}");
    assert!(!rendered.contains("[ Cancel ]"), "{rendered}");
    assert!(rendered.contains("(*) 2s"), "{rendered}");
    assert!(rendered.contains("Tab focus"), "{rendered}");
    assert!(rendered.contains("←/→ value"), "{rendered}");
}

#[test]
fn recording_path_dialog_keeps_arrows_for_path_cursor() {
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = app.recording_path_draft.len();

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();

    assert!(app.recording_path_cursor < app.recording_path_draft.len());
}

#[test]
fn recording_interval_control_supports_direct_mouse_selection() {
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    let buffer = render_app_to_buffer(&app, 100, 45);
    let (x, y) = find_text_position(&buffer, "( ) 10s")
        .expect("10-second interval option should be rendered");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 100, 45),
    );

    assert!(app.recording_interval_focused());
    assert_eq!(app.selected_recording_interval_seconds(), 10);
}

#[test]
fn recording_path_backspace_handles_key_repeat_and_ignores_release() {
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = app.recording_path_draft.len();

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

    assert_eq!(app.recording_path_draft, "C:/logs/example.lo");
    assert_eq!(app.recording_path_cursor, app.recording_path_draft.len());
}

#[test]
fn ctrl_space_completes_recording_path_directory() {
    let root = unique_recording_dir("recording-path-complete");
    let target = root.join("alpha");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&target).unwrap();
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    let head = format!("{}{}al", root.display(), std::path::MAIN_SEPARATOR);
    app.recording_path_draft = format!("{head}{}capture.log", std::path::MAIN_SEPARATOR);
    app.recording_path_cursor = head.len();

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
        .unwrap();

    let expected = format!(
        "{}{}alpha{}capture.log",
        root.display(),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    assert_eq!(app.recording_path_draft, expected);
    assert_eq!(
        app.recording_path_cursor,
        format!("{}{}alpha", root.display(), std::path::MAIN_SEPARATOR).len()
    );
    assert_eq!(app.status, "Completed directory");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tracked_remove_confirmation_is_compact_left_aligned_and_uses_footer_shortcuts() {
    let screen = Rect::new(0, 0, 120, 45);
    let mut app = make_test_app(1, 10);
    app.show_tracked_remove_confirmation = true;
    app.tracked_remove_name = "target.exe".to_string();
    app.tracked_remove_total_samples = 143;
    app.tracked_remove_discarded_samples = 23;

    let popup = tracked_remove_dialog_area(screen);
    assert_eq!(popup.width, 74);
    assert_eq!(popup.height, 9);

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let message = "target.exe has 143 in-memory samples.";
    let (message_x, _) =
        find_text_position(&buffer, message).expect("confirmation message should render");
    assert_eq!(
        message_x,
        popup.x + 1,
        "message body should be left aligned"
    );

    assert_eq!(buffer[(popup.x, popup.y)].fg, app.theme().warning);

    let shortcut = "Enter Remove  Esc Cancel";
    assert!(find_text_position(&buffer, shortcut).is_some());
    assert!(find_text_position(&buffer, "Enter removes / Esc cancels").is_none());

    let (enter_x, enter_y) =
        find_text_position(&buffer, shortcut).expect("shortcut line should render");
    assert_eq!(buffer[(enter_x, enter_y)].fg, app.theme().warning);
    assert!(buffer[(enter_x, enter_y)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(enter_x + 6, enter_y)].fg, app.theme().text);
    let esc_x = enter_x + "Enter Remove  ".chars().count() as u16;
    assert_eq!(buffer[(esc_x, enter_y)].fg, app.theme().warning);
    assert!(buffer[(esc_x, enter_y)].modifier.contains(Modifier::BOLD));
}

#[test]
fn closing_modal_restores_visible_panel_focus() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.show_details = false;
    app.show_help = true;

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_help);
    assert_eq!(app.focused_panel, FocusedPanel::Processes);
    assert!(app.panel_has_focus(FocusedPanel::Processes));

    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::DetailsSamples;
    app.show_details = false;
    app.show_recording_no_tracked_warning = true;

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_recording_no_tracked_warning);
    assert_eq!(app.focused_panel, FocusedPanel::Processes);
    assert!(app.panel_has_focus(FocusedPanel::Processes));
}

#[test]
fn recording_no_tracked_warning_uses_warning_title_and_border() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.show_recording_no_tracked_warning = true;

    let buffer = render_app_to_buffer(&app, 100, 45);
    assert_title_style(
        &buffer,
        "WARNING",
        app.theme().warning,
        ui::theme::contrasting_foreground(app.theme().warning, app.theme()),
    );
}

#[test]
fn ctrl_r_opens_recording_path_dialog_with_last_dir_default() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    let last_dir = std::path::PathBuf::from("C:/logs");
    app.recording_last_dir = Some(last_dir.clone());

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.show_recording_path_dialog);
    assert!(
        app.recording_path_draft.starts_with("C:/logs")
            || app.recording_path_draft.starts_with("C:\\logs")
    );
    assert!(app.recording_path_draft.contains("winproc-tui-"));
    assert!(app.recording_path_draft.ends_with(".log"));
    assert_eq!(app.recording_path_cursor, app.recording_path_draft.len());
}

#[test]
fn recording_path_dialog_takes_focus_border_from_previous_panel() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.focused_panel = FocusedPanel::Processes;

    app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();

    assert_modal_rect_focus_border(&app, Rect::new(11, 18, 78, 8));
}

#[test]
fn recording_path_dialog_uses_terminal_cursor_without_inline_marker() {
    let mut app = make_test_app(1, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = "C:/logs/".len();
    let screen = Rect::new(0, 0, 100, 45);
    let input_area = ui::recording_path_input_area(screen);
    let expected_cursor = Position::new(
        input_area.x + app.recording_path_cursor as u16,
        input_area.y,
    );

    let backend = TestBackend::new(screen.width, screen.height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");
    terminal
        .draw(|frame| ui::draw(frame, &app))
        .expect("test render should succeed");
    terminal
        .backend_mut()
        .assert_cursor_position(expected_cursor);
    let rendered = buffer_to_text(terminal.backend().buffer());

    assert!(rendered.contains("C:/logs/example.log"), "{rendered}");
    assert!(rendered.contains("Log file"), "{rendered}");
    assert!(
        rendered.contains("Confirm the log file and interval, then press Enter to start."),
        "{rendered}"
    );
    assert!(
        rendered
            .contains("Enter start  Esc cancel  Tab focus  ←/→ value  Ctrl+Space Complete path"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Tracking List  0 entries (fixed while recording)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Format         JSON Lines (.log)"),
        "{rendered}"
    );
    assert!(rendered.contains("Max duration   24 hours"), "{rendered}");
    assert!(!rendered.contains("Ctrl+L"), "{rendered}");
    assert!(!rendered.contains("[ Start ]"), "{rendered}");
    assert!(!rendered.contains("[ Cancel ]"), "{rendered}");
    assert!(!rendered.contains("Log file path"), "{rendered}");
    assert!(
        !rendered.contains("Specify the log file path."),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Enter starts recording / Esc cancels"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Press Enter to start recording. Press Esc to cancel."),
        "{rendered}"
    );
    assert!(!rendered.contains("C:/logs/|example.log"), "{rendered}");
}

#[test]
fn recording_dialog_shortcuts_use_footer_roles_in_all_color_schemes() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(1, 10);
        app.theme_index = theme_index;
        app.show_recording_path_dialog = true;
        app.recording_path_draft = "C:/logs/example.log".to_string();
        app.recording_path_cursor = app.recording_path_draft.len();

        let hint_buffer = render_app_to_buffer(&app, 100, 45);
        let (enter_x, hint_y) = find_text_position(&hint_buffer, "Enter start")
            .expect("recording shortcut should render");
        let start_x = enter_x + "Enter ".chars().count() as u16;
        assert_eq!(hint_buffer[(enter_x, hint_y)].fg, theme.key_hint);
        assert_eq!(hint_buffer[(start_x, hint_y)].fg, theme.text);

        app.show_recording_overwrite_confirmation = true;
        let overwrite_buffer = render_app_to_buffer(&app, 100, 45);
        let (cancel_x, overwrite_y) = find_text_position(&overwrite_buffer, "Enter/Esc/n Cancel")
            .expect("overwrite cancel shortcut should render");
        assert_eq!(overwrite_buffer[(cancel_x, overwrite_y)].fg, theme.warning);
        assert!(
            overwrite_buffer[(cancel_x, overwrite_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        let (y_x, overwrite_y) = find_text_position(&overwrite_buffer, "y Overwrite")
            .expect("overwrite shortcut should render");
        assert_eq!(overwrite_buffer[(y_x, overwrite_y)].fg, theme.warning);
        assert!(
            overwrite_buffer[(y_x, overwrite_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(overwrite_buffer[(y_x + 2, overwrite_y)].fg, theme.text);
    }
}

#[test]
fn recording_creates_missing_parent_directories() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    let root = unique_recording_dir("mkdir");
    let path = root.join("nested").join("capture.log");
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;

    app.confirm_recording_path().unwrap();

    assert!(path.parent().unwrap().is_dir());
    assert!(path.is_file());
    assert!(!app.show_recording_path_dialog);
    assert!(app.recording_session.is_some());

    app.stop_recording().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn recording_open_failure_uses_a_visible_error_and_returns_to_path_input() {
    let root = unique_recording_dir("open-error");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let parent_file = root.join("not-a-directory");
    std::fs::write(&parent_file, "blocker").unwrap();
    let path = parent_file.join("capture.log");
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();

    app.confirm_recording_path().unwrap();

    assert!(app.recording_session.is_none());
    assert!(app.recording_error.is_some());
    assert!(app.show_recording_path_dialog);
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(rendered.contains("RECORDING ERROR"), "{rendered}");
    assert!(
        rendered.contains("Recording could not start."),
        "{rendered}"
    );
    assert!(rendered.contains("Log:"), "{rendered}");
    assert!(rendered.contains("Error:"), "{rendered}");
    assert!(rendered.contains("Enter/Esc Close"), "{rendered}");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.recording_error.is_none());
    assert!(app.show_recording_path_dialog);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn periodic_recording_write_failure_stops_recording_and_shows_error() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let path = unique_recording_path("periodic-write-error");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.confirm_recording_path().unwrap();
    app.replace_recording_writer_for_test(Box::new(AlwaysFailWriter));
    app.open_main_menu();
    let snapshot = app.snapshot.clone();
    result_tx
        .send(CollectSnapshotResult {
            snapshot,
            warning: None,
        })
        .unwrap();

    app.poll_sample_results().unwrap();

    assert_eq!(app.activity(), AppActivity::Live);
    assert!(app.recording_session.is_none());
    assert!(!app.is_main_menu_open());
    let error = app
        .recording_error
        .as_ref()
        .expect("error should be visible");
    assert_eq!(error.kind, app::state::RecordingErrorKind::Stopped);
    assert!(path.exists(), "partial log should be retained");
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(
        rendered.contains("Recording stopped because the log could not be written."),
        "{rendered}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn quit_is_canceled_when_recording_flush_fails() {
    let path = unique_recording_path("quit-write-error");
    let _ = std::fs::remove_file(&path);
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.confirm_recording_path().unwrap();
    app.replace_recording_writer_for_test(Box::new(AlwaysFailWriter));
    app.request_quit_confirmation();

    app.confirm_quit().unwrap();

    assert!(!app.should_quit);
    assert!(!app.show_quit_confirmation);
    assert_eq!(app.activity(), AppActivity::Live);
    assert!(app.recording_error.is_some());
    let rendered = render_app_to_text(&app, 100, 45);
    assert!(rendered.contains("RECORDING ERROR"), "{rendered}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn recording_directory_path_is_rejected() {
    let directory = unique_recording_dir("directory-path");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.recording_path_draft = directory.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.confirm_recording_path().unwrap();

    assert!(app.show_recording_path_dialog);
    assert!(!app.show_recording_overwrite_confirmation);
    assert!(app.recording_session.is_none());
    assert_eq!(app.status, "Recording path must be a file, not a directory");
    assert!(directory.is_dir());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recording_overwrite_rechecks_directory_path() {
    let directory = unique_recording_dir("overwrite-directory-path");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.show_recording_overwrite_confirmation = true;
    app.recording_path_draft = directory.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();

    app.confirm_recording_overwrite().unwrap();

    assert!(app.show_recording_path_dialog);
    assert!(!app.show_recording_overwrite_confirmation);
    assert!(app.recording_session.is_none());
    assert_eq!(app.status, "Recording path must be a file, not a directory");
    assert!(directory.is_dir());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn existing_recording_path_opens_overwrite_confirmation() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    let path = unique_recording_path("existing");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old").unwrap();
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_recording_path_dialog);
    assert!(app.show_recording_overwrite_confirmation);

    let _ = std::fs::remove_file(path);
}

#[test]
fn overwrite_cancel_returns_to_recording_path_dialog() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    app.show_recording_path_dialog = true;
    app.show_recording_overwrite_confirmation = true;

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_recording_path_dialog);
    assert!(!app.show_recording_overwrite_confirmation);
    assert_eq!(app.status, "Overwrite canceled");
}

#[test]
fn live_header_omits_freshness_when_current() {
    let app = make_test_app(1, 10);

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(rendered.contains("LIVE"), "{rendered}");
    assert!(rendered.contains("Session ▾"), "{rendered}");
    assert!(rendered.contains("PF: none"), "{rendered}");
    assert!(!rendered.contains("fresh"), "{rendered}");
    assert!(!rendered.contains("STALE"), "{rendered}");
}

#[test]
fn live_header_hides_product_and_version_when_the_row_is_too_narrow() {
    let app = make_test_app(1, 10);
    let product_and_version = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));

    let rendered = render_app_to_text(&app, 24, 20);

    assert!(rendered.contains("LIVE"), "{rendered}");
    assert!(rendered.contains("Session ▾"), "{rendered}");
    assert!(!rendered.contains(&product_and_version), "{rendered}");
}

#[test]
fn live_header_shows_visible_stale_state() {
    let mut app = make_test_app(1, 10);
    app.snapshot.captured_at =
        Local::now() - chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64 + 2);

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(rendered.contains("LIVE"), "{rendered}");
    assert!(rendered.contains("STALE "), "{rendered}");
    assert!(!rendered.contains("fresh"), "{rendered}");
}

#[test]
fn recording_header_shows_rec_spinner_and_path() {
    let mut app = make_test_app(1, 10);
    track_process_name(&mut app, "proc-0");
    let path = unique_recording_path("header");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    let rendered = render_app_to_text(&app, 120, 45);
    let header = rendered.lines().next().expect("header row");
    assert!(rendered.contains("REC"), "{rendered}");
    assert!(!rendered.contains("fresh"), "{rendered}");
    assert!(!rendered.contains("STALE"), "{rendered}");
    assert!(
        header.contains("winproc-") && header.contains(".log"),
        "{header}"
    );
    assert!(!header.contains(&path.display().to_string()), "{header}");
    assert!(header.contains("PF: none"), "{header}");
    assert!(rendered.contains("Session ▾"), "{rendered}");
    let buffer = render_app_to_buffer(&app, 120, 45);
    let (profile_x, profile_y) =
        find_text_position(&buffer, "PF: none").expect("profile badge should render");
    assert_eq!(buffer[(profile_x, profile_y)].fg, Color::Black);
    assert_eq!(buffer[(profile_x, profile_y)].bg, app.theme().muted);

    app.toggle_display_pause();
    let paused = render_app_to_text(&app, 120, 45);
    let paused_header = paused.lines().next().expect("paused header row");
    app.recording_spinner_index += 1;
    let advanced = render_app_to_text(&app, 120, 45);
    assert_ne!(paused_header, advanced.lines().next().unwrap());
    assert!(paused.contains("REC"), "{paused}");
    assert!(paused.contains("DISPLAY PAUSED"), "{paused}");
    assert!(paused_header.contains(".log"), "{paused_header}");

    let narrow = render_app_to_text(&app, 80, 45);
    let footer = narrow.lines().last().unwrap();
    for shortcut in [
        "ESC Menu",
        "F1/? Help",
        "Ctrl+R Stop",
        "Ctrl+P Resume",
        "Tab Focus",
    ] {
        assert!(footer.contains(shortcut), "{footer}");
    }

    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}
