use super::support::{
    activate_process_modules_tab, make_test_app, render_app_to_text, test_process_module_entry,
    test_process_modules_report,
};
use crate::app;
use crate::model::{InfoValue, ProcessModulesError};
use crate::samplers::process_modules::{
    ProcessModulesRequest, ProcessModulesResult, ProcessModulesWorker,
};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc::TryRecvError;

#[test]
fn dlls_tab_lazy_loads_once_for_the_fixed_dialog_target() {
    let (worker, request_rx, _) = ProcessModulesWorker::test_pair();
    let mut app = make_test_app(2, 10);
    app.process_modules_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    let target = app.process_info_target.as_ref().unwrap().identity.clone();

    activate_process_modules_tab(&mut app);
    match request_rx.try_recv().unwrap() {
        ProcessModulesRequest::Collect { identity, .. } => assert_eq!(identity, target),
        ProcessModulesRequest::Stop => panic!("unexpected stop request"),
    }
    app.move_selection_down(1);
    app.activate_process_info_tab(app::ProcessInfoTab::Dlls)
        .unwrap();

    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        app.process_modules_in_flight.as_ref(),
        Some(&target),
        "DLL collection must remain bound to the dialog target"
    );
}

#[test]
fn dlls_tab_refresh_preserves_snapshot_path_filter_and_copies_selected_path() {
    let (worker, request_rx, result_tx) = ProcessModulesWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_modules_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    activate_process_modules_tab(&mut app);
    let (generation, request_id, identity) = match request_rx.try_recv().unwrap() {
        ProcessModulesRequest::Collect {
            generation,
            request_id,
            identity,
            ..
        } => (generation, request_id, identity),
        ProcessModulesRequest::Stop => panic!("unexpected stop request"),
    };
    let first = test_process_module_entry("first.dll", "First Company");
    let second = test_process_module_entry("second.dll", "Second Company");
    result_tx
        .send(ProcessModulesResult {
            generation,
            request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_modules_report(
                &identity.name,
                identity.pid,
                vec![first, second.clone()],
            )),
        })
        .unwrap();
    assert!(app.poll_process_modules_results().unwrap());

    for ch in "second.dll".chars() {
        app.push_process_modules_filter_char(ch);
    }
    assert_eq!(ui::process_modules::filtered_entries(&app).len(), 1);
    let filtered = render_app_to_text(&app, 100, 30);
    assert!(filtered.contains("shown 1/2"), "{filtered}");
    app.copy_selected_process_module_to_clipboard().unwrap();
    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some(second.path.as_str())
    );

    app.refresh_process_modules().unwrap();
    let refresh = match request_rx.try_recv().unwrap() {
        ProcessModulesRequest::Collect {
            generation,
            request_id,
            ..
        } => (generation, request_id),
        ProcessModulesRequest::Stop => panic!("unexpected stop request"),
    };
    assert!(app.process_modules_result.is_some());
    app.refresh_process_modules().unwrap();
    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(app.status, "DLL refresh already in progress");

    result_tx
        .send(ProcessModulesResult {
            generation: refresh.0,
            request_id: refresh.1,
            identity,
            outcome: Err(ProcessModulesError::AccessDenied),
        })
        .unwrap();
    assert!(app.poll_process_modules_results().unwrap());
    assert!(app.process_modules_result.is_some());
    assert_eq!(
        app.process_modules_error,
        Some(ProcessModulesError::AccessDenied)
    );
}

#[test]
fn dlls_tab_lists_full_paths_and_enter_opens_selected_detail() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Dlls;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    let mut entry = test_process_module_entry(
        "a-very-long-module-name-that-does-not-fit.dll",
        "A Company With A Long Name",
    );
    entry.product_version = InfoValue::NotAvailable;
    app.process_modules_result_identity = Some(identity.clone());
    app.process_modules_result = Some(test_process_modules_report(
        &identity.name,
        identity.pid,
        vec![entry],
    ));

    let list = render_app_to_text(&app, 68, 26);
    assert!(list.contains("DLL path"), "{list}");
    assert!(list.contains(r"C:\Program Files\Test"), "{list}");
    assert!(!list.contains("Product Version"), "{list}");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.process_modules_show_detail);
    let detail = render_app_to_text(&app, 68, 26);
    assert!(detail.contains("DLL details"), "{detail}");
    assert!(detail.contains("DLL file"), "{detail}");
    assert!(detail.contains("Product Version"), "{detail}");
    assert!(detail.contains("Directory"), "{detail}");
    assert!(detail.contains("<not available>"), "{detail}");
    assert!(detail.contains("Esc/Enter back"), "{detail}");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_process_info_dialog);
    assert!(!app.process_modules_show_detail);

    app.snapshot.processes.clear();
    let exited = render_app_to_text(&app, 100, 30);
    assert!(exited.contains("process exited"), "{exited}");
}

#[test]
fn dlls_tab_arrow_selection_controls_enter_detail_target() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Dlls;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    app.process_modules_result_identity = Some(identity.clone());
    app.process_modules_result = Some(test_process_modules_report(
        &identity.name,
        identity.pid,
        vec![
            test_process_module_entry("first.dll", "First Company"),
            test_process_module_entry("second.dll", "Second Company"),
        ],
    ));

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.process_modules_selected, 1);
    assert!(app.process_modules_show_detail);
    let detail = render_app_to_text(&app, 80, 26);
    assert!(detail.contains("second.dll"), "{detail}");
    assert!(detail.contains("Second Company"), "{detail}");
    assert!(!detail.contains("First Company"), "{detail}");
}

#[test]
fn dlls_tab_in_log_view_starts_no_worker() {
    let (worker, request_rx, _) = ProcessModulesWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_modules_worker = worker;
    app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
    app.open_selected_process_info_dialog().unwrap();
    activate_process_modules_tab(&mut app);

    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    let rendered = render_app_to_text(&app, 120, 40);
    assert!(
        rendered.contains("This information is not included in recording logs."),
        "{rendered}"
    );
}

#[test]
fn stale_dll_result_cannot_replace_reopened_dialog_request() {
    let (worker, request_rx, result_tx) = ProcessModulesWorker::test_pair();
    let mut app = make_test_app(1, 10);
    app.process_modules_worker = worker;
    app.open_selected_process_info_dialog().unwrap();
    activate_process_modules_tab(&mut app);
    let (old_generation, old_request_id, identity) = match request_rx.try_recv().unwrap() {
        ProcessModulesRequest::Collect {
            generation,
            request_id,
            identity,
            ..
        } => (generation, request_id, identity),
        ProcessModulesRequest::Stop => panic!("unexpected stop request"),
    };

    app.close_process_info_dialog();
    app.open_selected_process_info_dialog().unwrap();
    activate_process_modules_tab(&mut app);
    let (new_generation, new_request_id) = match request_rx.try_recv().unwrap() {
        ProcessModulesRequest::Collect {
            generation,
            request_id,
            ..
        } => (generation, request_id),
        ProcessModulesRequest::Stop => panic!("unexpected stop request"),
    };
    result_tx
        .send(ProcessModulesResult {
            generation: old_generation,
            request_id: old_request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_modules_report(
                &identity.name,
                identity.pid,
                vec![test_process_module_entry("old.dll", "Old")],
            )),
        })
        .unwrap();
    assert!(!app.poll_process_modules_results().unwrap());
    assert_eq!(
        app.process_modules_in_flight_request_id,
        Some(new_request_id)
    );
    assert!(app.process_modules_result.is_none());

    result_tx
        .send(ProcessModulesResult {
            generation: new_generation,
            request_id: new_request_id,
            identity: identity.clone(),
            outcome: Ok(test_process_modules_report(
                &identity.name,
                identity.pid,
                vec![test_process_module_entry("new.dll", "New")],
            )),
        })
        .unwrap();
    assert!(app.poll_process_modules_results().unwrap());
    assert_eq!(
        app.process_modules_result.as_ref().unwrap().entries[0].dll_name,
        "new.dll"
    );
}
