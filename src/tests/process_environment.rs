use super::support::{
    activate_process_environment_tab, buffer_to_text, find_text_position, left_click,
    make_test_app, render_app_to_text, test_process_environment_report,
};
use crate::app;
use crate::model::{ProcessEnvironmentEntry, ProcessEnvironmentError};
use crate::samplers::process_environment::{
    ProcessEnvironmentRequest, ProcessEnvironmentResult, ProcessEnvironmentWorker,
};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use std::sync::mpsc::TryRecvError;

#[test]
fn environment_tab_lazy_loads_once_for_the_fixed_dialog_target() {
    let (worker, request_rx, _) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(2, 10);
    app.process_environment_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    let target = app.process_info_target.as_ref().unwrap().identity.clone();

    activate_process_environment_tab(&mut app);
    match request_rx.try_recv().unwrap() {
        ProcessEnvironmentRequest::Collect { identity, .. } => assert_eq!(identity, target),
        ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
    }
    app.move_selection_down(1);
    app.activate_process_info_tab(app::ProcessInfoTab::Environment)
        .unwrap();

    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(app.process_environment_in_flight.as_ref(), Some(&target));
}

#[test]
fn environment_refresh_preserves_snapshot_filters_values_and_copies_one_entry() {
    let (worker, request_rx, result_tx) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_environment_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    activate_process_environment_tab(&mut app);
    let (generation, request_id, identity) = match request_rx.try_recv().unwrap() {
        ProcessEnvironmentRequest::Collect {
            generation,
            request_id,
            identity,
            ..
        } => (generation, request_id, identity),
        ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
    };
    let secret = "sensitive-value-for-filter-test";
    result_tx
        .send(ProcessEnvironmentResult {
            generation,
            request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_environment_report(
                &identity.name,
                identity.pid,
                vec![
                    ProcessEnvironmentEntry {
                        name: "EMPTY".to_string(),
                        value: String::new(),
                    },
                    ProcessEnvironmentEntry {
                        name: "TOKEN".to_string(),
                        value: secret.to_string(),
                    },
                ],
            )),
        })
        .unwrap();
    assert!(app.poll_process_environment_results().unwrap());

    for ch in "value-for-filter".chars() {
        app.push_process_environment_filter_char(ch);
    }
    for ch in " missing-term".chars() {
        app.push_process_environment_filter_char(ch);
    }
    assert!(ui::process_environment::filtered_entries(&app).is_empty());
    app.process_environment_filter = "TOKEN value-for-filter".into();
    assert_eq!(ui::process_environment::filtered_entries(&app).len(), 1);
    app.copy_selected_process_environment_to_clipboard()
        .unwrap();
    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("TOKEN=sensitive-value-for-filter-test")
    );

    app.refresh_process_environment().unwrap();
    let refresh = match request_rx.try_recv().unwrap() {
        ProcessEnvironmentRequest::Collect {
            generation,
            request_id,
            ..
        } => (generation, request_id),
        ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
    };
    assert!(app.process_environment_result.is_some());
    app.refresh_process_environment().unwrap();
    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(app.status, "Environment refresh already in progress");

    result_tx
        .send(ProcessEnvironmentResult {
            generation: refresh.0,
            request_id: refresh.1,
            identity,
            outcome: Err(ProcessEnvironmentError::AccessDenied),
        })
        .unwrap();
    assert!(app.poll_process_environment_results().unwrap());
    assert!(app.process_environment_result.is_some());
    assert_eq!(
        app.process_environment_error,
        Some(ProcessEnvironmentError::AccessDenied)
    );
    assert!(!app.status.contains(secret));
    app.close_process_info_dialog();
    assert!(app.process_environment_result.is_none());
}

#[test]
fn environment_tab_enter_opens_long_selected_value_detail() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Environment;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    let long_value = "C:\\one;C:\\two;C:\\three;C:\\four;C:\\five;C:\\six";
    let mut report = test_process_environment_report(
        &identity.name,
        identity.pid,
        vec![ProcessEnvironmentEntry {
            name: "PATH".to_string(),
            value: long_value.to_string(),
        }],
    );
    report.malformed_entries = 2;
    app.process_environment_result_identity = Some(identity);
    app.process_environment_result = Some(report);

    let list = render_app_to_text(&app, 60, 24);
    assert!(list.contains("Name"), "{list}");
    assert!(list.contains("Value"), "{list}");
    assert!(!list.contains("Environment may contain secrets"), "{list}");
    assert!(list.contains("2 malformed entries skipped"), "{list}");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.process_environment_show_detail);
    let detail = render_app_to_text(&app, 60, 24);
    assert!(detail.contains("Environment variable details"), "{detail}");
    assert!(detail.contains("C:\\one;"), "{detail}");
    assert!(detail.contains("Esc/Enter back"), "{detail}");
}

#[test]
fn environment_filter_cursor_and_mouse_rows_match_rendered_layout() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Environment;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    app.process_environment_result_identity = Some(identity.clone());
    app.process_environment_result = Some(test_process_environment_report(
        &identity.name,
        identity.pid,
        vec![
            ProcessEnvironmentEntry {
                name: "FIRST".to_string(),
                value: "one".to_string(),
            },
            ProcessEnvironmentEntry {
                name: "SECOND".to_string(),
                value: "two".to_string(),
            },
        ],
    ));
    app.process_environment_filter = "o".to_string();
    app.process_environment_filter_cursor = 1;
    app.process_info_filter_editing = true;
    let screen = Rect::new(0, 0, 160, 45);
    let content = ui::process_info_content_area_for_screen(screen, app.process_info_height());
    let expected_cursor = Position::new(content.x + "Filter: ".len() as u16 + 1, content.y + 1);

    let backend = TestBackend::new(screen.width, screen.height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");
    terminal
        .draw(|frame| ui::draw(frame, &app))
        .expect("test render should succeed");
    terminal
        .backend_mut()
        .assert_cursor_position(expected_cursor);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_text(&buffer);
    let (second_x, second_y) =
        find_text_position(&buffer, "SECOND").expect("second environment row should render");

    assert!(
        !rendered.contains("Environment may contain secrets"),
        "{rendered}"
    );
    assert_eq!(second_y, content.y + 4);
    app.on_mouse(left_click(second_x, second_y), screen);
    assert_eq!(app.process_environment_selected, 1);
}

#[test]
fn environment_detail_is_keyboard_scrollable_on_short_screens() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Environment;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    let long_value = format!("{}VALUE-END", "abcdefghij".repeat(20));
    app.process_environment_result_identity = Some(identity.clone());
    app.process_environment_result = Some(test_process_environment_report(
        &identity.name,
        identity.pid,
        vec![ProcessEnvironmentEntry {
            name: "LONG_VALUE".to_string(),
            value: long_value,
        }],
    ));
    let screen = Rect::new(0, 0, 50, 12);
    app.set_screen_area(screen);
    app.set_process_info_page_size(ui::process_info_page_size_for_screen(
        screen,
        app.process_info_height(),
    ));

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();

    let detail = render_app_to_text(&app, screen.width, screen.height);
    assert!(detail.contains("VALUE-END"), "{detail}");
    assert!(!detail.contains("[ Close ]"), "{detail}");
    assert!(detail.contains("Esc/Enter back"), "{detail}");
}

#[test]
fn environment_tab_in_log_view_starts_no_worker() {
    let (worker, request_rx, _) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_environment_worker = worker;
    app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
    app.open_selected_process_info_dialog().unwrap();
    activate_process_environment_tab(&mut app);

    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    let rendered = render_app_to_text(&app, 120, 40);
    assert!(
        rendered.contains("This information is not included in recording logs."),
        "{rendered}"
    );
}

#[test]
fn stale_environment_result_cannot_replace_reopened_dialog_request() {
    let (worker, request_rx, result_tx) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_environment_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    activate_process_environment_tab(&mut app);
    let (old_generation, old_request_id, identity) = match request_rx.try_recv().unwrap() {
        ProcessEnvironmentRequest::Collect {
            generation,
            request_id,
            identity,
            ..
        } => (generation, request_id, identity),
        ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
    };
    app.close_process_info_dialog();
    app.open_selected_process_info_dialog().unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Environment);
    let (new_generation, new_request_id) = match request_rx.try_recv().unwrap() {
        ProcessEnvironmentRequest::Collect {
            generation,
            request_id,
            ..
        } => (generation, request_id),
        ProcessEnvironmentRequest::Stop => panic!("unexpected stop request"),
    };

    result_tx
        .send(ProcessEnvironmentResult {
            generation: old_generation,
            request_id: old_request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_environment_report(
                &identity.name,
                identity.pid,
                vec![ProcessEnvironmentEntry {
                    name: "OLD".to_string(),
                    value: "old".to_string(),
                }],
            )),
        })
        .unwrap();
    assert!(!app.poll_process_environment_results().unwrap());
    assert_eq!(
        app.process_environment_in_flight_request_id,
        Some(new_request_id)
    );
    assert!(app.process_environment_result.is_none());

    result_tx
        .send(ProcessEnvironmentResult {
            generation: new_generation,
            request_id: new_request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_environment_report(
                &identity.name,
                identity.pid,
                vec![ProcessEnvironmentEntry {
                    name: "NEW".to_string(),
                    value: "new".to_string(),
                }],
            )),
        })
        .unwrap();
    assert!(app.poll_process_environment_results().unwrap());
    assert_eq!(
        app.process_environment_result.as_ref().unwrap().entries[0].name,
        "NEW"
    );
}

#[test]
fn environment_value_copy_preserves_raw_and_empty_values_without_status_leak() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    activate_process_environment_tab(&mut app);
    app.process_info_focus = app::ProcessInfoFocus::Content;
    app.process_environment_result = Some(test_process_environment_report(
        "proc-0",
        0,
        vec![
            ProcessEnvironmentEntry {
                name: "EMPTY".into(),
                value: String::new(),
            },
            ProcessEnvironmentEntry {
                name: "DUMMY".into(),
                value: "日本語=value\nnext".into(),
            },
        ],
    ));
    app.process_environment_selected = 1;
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().as_deref(),
        Some("日本語=value\nnext")
    );
    assert_eq!(app.status, "Copied environment value");
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().as_deref(),
        Some("DUMMY=日本語=value\nnext")
    );
    app.process_environment_selected = 0;
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().as_deref(),
        Some("")
    );
    app.process_info_filter_editing = true;
    app.process_environment_selected = 1;
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT))
        .unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text().as_deref(),
        Some("")
    );
}

#[test]
fn refresh_retains_environment_identity_across_insertion_reorder_and_removal() {
    let (worker, requests, results) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_environment_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    activate_process_environment_tab(&mut app);
    let deliver = |app: &mut crate::App, names: &[&str]| {
        let ProcessEnvironmentRequest::Collect {
            generation,
            request_id,
            identity,
            ..
        } = requests.try_recv().unwrap()
        else {
            panic!()
        };
        let entries = names
            .iter()
            .map(|&name| ProcessEnvironmentEntry {
                name: name.into(),
                value: "dummy".repeat(100),
            })
            .collect();
        let report = test_process_environment_report(&identity.name, identity.pid, entries);
        results
            .send(ProcessEnvironmentResult {
                generation,
                request_id,
                identity,
                outcome: Ok(report),
            })
            .unwrap();
        assert!(app.poll_process_environment_results().unwrap());
    };
    deliver(&mut app, &["BETA", "DELTA"]);
    app.process_environment_selected = 1;
    app.open_selected_process_info_detail();
    let original = ui::process_environment::selected_entry(&app)
        .unwrap()
        .name
        .clone();
    app.refresh_process_environment().unwrap();
    deliver(&mut app, &["ALPHA", "BETA", "DELTA"]);
    assert_eq!(app.process_environment_selected, 2);
    assert!(app.process_environment_show_detail);
    assert_eq!(
        ui::process_environment::selected_entry(&app).unwrap().name,
        original
    );
    app.refresh_process_environment().unwrap();
    deliver(&mut app, &["DELTA", "BETA", "ALPHA"]);
    assert_eq!(app.process_environment_selected, 0);
    assert!(app.process_environment_show_detail);
    app.refresh_process_environment().unwrap();
    deliver(&mut app, &["BETA", "ALPHA"]);
    assert!(!app.process_environment_show_detail);
    app.process_environment_filter = "BETA".into();
    app.process_environment_selected = 0;
    app.open_selected_process_info_detail();
    app.refresh_process_environment().unwrap();
    deliver(&mut app, &["DELTA"]);
    assert!(!app.process_environment_show_detail);
    assert!(ui::process_environment::selected_entry(&app).is_none());
}

#[test]
fn environment_detail_footer_refresh_is_clickable_and_filter_clear_stays_local() {
    let (worker, requests, _) = ProcessEnvironmentWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_environment_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Environment;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    app.process_environment_result = Some(test_process_environment_report(
        "proc-0",
        0,
        vec![ProcessEnvironmentEntry {
            name: "DUMMY".into(),
            value: "dummy".into(),
        }],
    ));
    app.open_selected_process_info_detail();
    for width in [40, 60, 160] {
        let text = render_app_to_text(&app, width, 24);
        assert!(text.contains("Esc/Enter back"), "{text}");
        assert!(text.contains("Ctrl+U refresh"), "{text}");
    }
    let screen = Rect::new(0, 0, 60, 24);
    app::sync_layout_state(&mut app, screen);
    let buffer = super::support::render_app_to_buffer(&app, 60, 24);
    let (x, y) = find_text_position(&buffer, "Ctrl+U refresh").unwrap();
    app.on_mouse(left_click(x, y), screen);
    assert!(matches!(
        requests.try_recv(),
        Ok(ProcessEnvironmentRequest::Collect { .. })
    ));
    assert!(!render_app_to_text(&app, 60, 24).contains("Ctrl+U refresh"));
    app.close_process_info_detail();
    app.process_environment_filter = "DUMMY".into();
    app.process_environment_filter_cursor = 5;
    app.process_info_filter_editing = true;
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.process_environment_filter.is_empty());
    assert!(requests.try_recv().is_err());
}
