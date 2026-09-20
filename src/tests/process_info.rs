use super::support::{
    area_contains_foreground, buffer_to_text, find_text_position, find_text_position_in_area,
    left_click, make_test_app, make_test_app_with_workers, render_app_to_buffer,
    render_app_to_text, test_process_info, test_process_module_entry, test_process_modules_report,
};
use crate::app;
use crate::app::PROCESS_INFO_DEBOUNCE;
use crate::model::{InfoValue, ProcessHistory, SystemHistory};
use crate::samplers::SamplingWorker;
use crate::samplers::open_files::OpenFilesWorker;
use crate::samplers::process_info::{ProcessInfoRequest, ProcessInfoResult, ProcessInfoWorker};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::sync::mpsc::TryRecvError;

#[test]
fn process_info_result_updates_cache_for_current_selection() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_selected_process_info_dialog().unwrap();
    app.pending_process_info.as_mut().unwrap().changed_at =
        std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
    app.request_due_process_info().unwrap();
    let (generation, identity) = match request_rx.try_recv().unwrap() {
        ProcessInfoRequest::Collect {
            generation,
            identity,
            ..
        } => (generation, identity),
        ProcessInfoRequest::Stop => panic!("unexpected stop request"),
    };

    result_tx
        .send(ProcessInfoResult {
            generation,
            identity: identity.clone(),
            info: test_process_info(&identity.name, identity.pid),
        })
        .unwrap();

    assert!(app.poll_process_info_results().unwrap());
    assert!(app.process_info_cache.contains_key(&identity));
    assert_eq!(app.process_info_display_identity, Some(identity));
    assert!(app.process_info_in_flight.is_none());
}

#[test]
fn process_info_result_applies_to_fixed_dialog_target_after_selection_changes() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_selected_process_info_dialog().unwrap();
    app.pending_process_info.as_mut().unwrap().changed_at =
        std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
    app.request_due_process_info().unwrap();
    let (generation, old_identity) = match request_rx.try_recv().unwrap() {
        ProcessInfoRequest::Collect {
            generation,
            identity,
            ..
        } => (generation, identity),
        ProcessInfoRequest::Stop => panic!("unexpected stop request"),
    };

    app.move_selection_down(1);
    result_tx
        .send(ProcessInfoResult {
            generation,
            identity: old_identity.clone(),
            info: test_process_info(&old_identity.name, old_identity.pid),
        })
        .unwrap();

    assert!(app.poll_process_info_results().unwrap());
    assert!(app.process_info_cache.contains_key(&old_identity));
    assert!(app.process_info_in_flight.is_none());
    assert!(app.pending_process_info.is_none());
}

#[test]
fn stale_process_info_result_cannot_replace_reopened_dialog_request() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, request_rx, result_tx) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );

    app.open_selected_process_info_dialog().unwrap();
    app.pending_process_info.as_mut().unwrap().changed_at =
        std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
    app.request_due_process_info().unwrap();
    let (old_generation, identity) = match request_rx.try_recv().unwrap() {
        ProcessInfoRequest::Collect {
            generation,
            identity,
            ..
        } => (generation, identity),
        ProcessInfoRequest::Stop => panic!("unexpected stop request"),
    };

    app.close_process_info_dialog();
    app.open_selected_process_info_dialog().unwrap();
    app.pending_process_info.as_mut().unwrap().changed_at =
        std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
    app.request_due_process_info().unwrap();
    let new_generation = match request_rx.try_recv().unwrap() {
        ProcessInfoRequest::Collect { generation, .. } => generation,
        ProcessInfoRequest::Stop => panic!("unexpected stop request"),
    };

    result_tx
        .send(ProcessInfoResult {
            generation: old_generation,
            identity: identity.clone(),
            info: test_process_info("old.exe", identity.pid),
        })
        .unwrap();
    assert!(!app.poll_process_info_results().unwrap());
    assert_eq!(app.process_info_in_flight_generation, Some(new_generation));
    assert!(!app.process_info_cache.contains_key(&identity));

    result_tx
        .send(ProcessInfoResult {
            generation: new_generation,
            identity: identity.clone(),
            info: test_process_info("new.exe", identity.pid),
        })
        .unwrap();
    assert!(app.poll_process_info_results().unwrap());
    assert_eq!(
        app.process_info_cache.get(&identity).unwrap().name,
        "new.exe"
    );
}

#[test]
fn process_info_metrics_follow_current_a_and_b_rules_with_missing_values() {
    let mut app = make_test_app(1, 10);
    let current_at = app.snapshot.captured_at;
    let a_at = current_at - chrono::Duration::seconds(2);
    let b_at = current_at - chrono::Duration::seconds(1);
    let mut a = app.snapshot.processes[0].clone();
    a.cpu_percent = Some(10.8);
    a.private_bytes = Some(375_800_000);
    a.thread_count = Some(1_036);
    a.handle_count = Some(20);
    a.gdi_object_count = Some(5);
    a.io_read_bytes_per_sec = Some(50_000);
    let mut b = a.clone();
    b.cpu_percent = Some(11.7);
    b.private_bytes = Some(384_400_000);
    b.thread_count = Some(1_030);
    b.handle_count = Some(20);
    b.io_read_bytes_per_sec = Some(75_000);
    let mut current = a.clone();
    current.cpu_percent = Some(12.3);
    current.private_bytes = Some(388_100_000);
    current.thread_count = Some(1_024);
    current.handle_count = Some(20);
    current.gdi_object_count = None;
    current.io_read_bytes_per_sec = Some(100_000);
    app.snapshot.processes[0] = current.clone();
    app.process_history
        .record_snapshot_unbounded(a_at, &[a.clone()]);
    app.process_history
        .record_snapshot_unbounded(b_at, &[b.clone()]);
    app.process_history
        .record_snapshot_unbounded(current_at, &[current]);
    app.open_selected_process_info_dialog().unwrap();

    let current_view = app.process_info_metrics_view().unwrap();
    assert_eq!(current_view.value_heading, "Current");
    assert_eq!(current_view.delta_heading, None);
    assert!(current_view.rows.iter().all(|row| row.delta.is_none()));
    assert_eq!(
        current_view
            .rows
            .iter()
            .map(|row| row.label)
            .collect::<Vec<_>>(),
        vec![
            "CPU Usage",
            "Private Bytes",
            "Working Set",
            "Working Set - Private",
            "Working Set - Shareable",
            "Threads",
            "Handles",
            "USER Objects",
            "GDI Objects",
            "GPU Usage",
            "GPU Dedicated Memory",
            "GPU Shared Memory",
            "I/O Read Throughput",
            "I/O Write Throughput",
            ".NET Heap",
            ".NET Gen 0 Heap",
            ".NET Gen 1 Heap",
            ".NET Gen 2 Heap",
            ".NET Large Object Heap",
            ".NET Pinned Object Heap",
            ".NET GC Committed",
            ".NET GC Fragmentation",
            ".NET Allocation Rate",
        ]
    );
    assert_eq!(
        current_view
            .rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .value,
        "388.1 MB"
    );
    assert_eq!(
        current_view
            .rows
            .iter()
            .find(|row| row.label == "I/O Read Throughput")
            .unwrap()
            .value,
        "800 Kbps"
    );
    let compact = render_app_to_text(&app, 60, 50);
    for label in [
        "Private Bytes",
        "Working Set - Private",
        "Handles",
        "GPU Dedicated Memory",
    ] {
        assert!(compact.contains(label), "missing {label}: {compact}");
    }
    app.scroll_process_info_end();
    let compact_end = render_app_to_text(&app, 60, 50);
    for label in [".NET Pinned Object Heap", ".NET Allocation Rate"] {
        assert!(
            compact_end.contains(label),
            "missing {label}: {compact_end}"
        );
    }

    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: a_at }),
        b: None,
    });
    let a_view = app.process_info_metrics_view().unwrap();
    assert_eq!(a_view.delta_heading, Some("Delta from A"));
    assert_eq!(
        a_view
            .rows
            .iter()
            .find(|row| row.label == "CPU Usage")
            .unwrap()
            .delta
            .as_deref(),
        Some("+1.5%")
    );
    assert_eq!(
        a_view
            .rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .delta
            .as_deref(),
        Some("+12.3 MB")
    );
    assert_eq!(
        a_view
            .rows
            .iter()
            .find(|row| row.label == "Threads")
            .unwrap()
            .delta
            .as_deref(),
        Some("-12")
    );
    assert_eq!(
        a_view
            .rows
            .iter()
            .find(|row| row.label == "Handles")
            .unwrap()
            .delta
            .as_deref(),
        Some("+0")
    );
    assert_eq!(
        a_view
            .rows
            .iter()
            .find(|row| row.label == "I/O Read Throughput")
            .unwrap()
            .delta
            .as_deref(),
        Some("+400 Kbps")
    );
    let missing = a_view
        .rows
        .iter()
        .find(|row| row.label == "GDI Objects")
        .unwrap();
    assert_eq!(missing.value, "--");
    assert_eq!(missing.delta.as_deref(), Some("--"));

    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: a_at }),
        b: Some(app::AbComparisonPoint { captured_at: b_at }),
    });
    let ab_view = app.process_info_metrics_view().unwrap();
    assert_eq!(ab_view.value_heading, "At B");
    assert_eq!(ab_view.delta_heading, Some("B-A"));
    assert_eq!(
        ab_view
            .rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .value,
        "384.4 MB"
    );
    let io_read = ab_view
        .rows
        .iter()
        .find(|row| row.label == "I/O Read Throughput")
        .unwrap();
    assert_eq!(io_read.value, "600 Kbps");
    assert_eq!(io_read.delta.as_deref(), Some("+200 Kbps"));

    app.ab_comparison = Some(app::AbComparison {
        a: None,
        b: Some(app::AbComparisonPoint { captured_at: b_at }),
    });
    let b_only_view = app.process_info_metrics_view().unwrap();
    assert_eq!(b_only_view.value_heading, "Current");
    assert_eq!(b_only_view.delta_heading, None);
}

#[test]
fn process_info_renders_range_above_underlined_metric_headers() {
    let mut app = make_test_app(1, 10);
    let current_at = app.snapshot.captured_at;
    let a_at = current_at - chrono::Duration::seconds(2);
    let b_at = current_at - chrono::Duration::seconds(1);
    let process = app.snapshot.processes[0].clone();
    app.process_history
        .record_snapshot_unbounded(a_at, std::slice::from_ref(&process));
    app.process_history
        .record_snapshot_unbounded(b_at, std::slice::from_ref(&process));
    app.process_history
        .record_snapshot_unbounded(current_at, std::slice::from_ref(&process));
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: a_at }),
        b: Some(app::AbComparisonPoint { captured_at: b_at }),
    });
    app.open_selected_process_info_dialog().unwrap();
    let range = app.process_info_metrics_view().unwrap().range;

    let buffer = render_app_to_buffer(&app, 120, 40);
    let (_, range_y) = find_text_position(&buffer, &range).expect("comparison range should render");
    let (_, header_y) = find_text_position(&buffer, "At B").expect("metric header should render");
    let metrics_x = ui::process_info_content_area_for_screen(Rect::new(0, 0, 120, 40)).x;

    assert!(range_y < header_y);
    for heading in ["Metrics", "At B", "B-A"] {
        let (x, y) = if heading == "Metrics" {
            (metrics_x, header_y)
        } else {
            find_text_position(&buffer, heading).expect("column heading should render")
        };
        for offset in 0..heading.len() as u16 {
            assert!(
                buffer[(x + offset, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::UNDERLINED),
                "{heading} should be underlined"
            );
        }
    }
    assert!(
        !buffer[(metrics_x + "Metrics".len() as u16, header_y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED),
        "spacing after a column heading should not be underlined"
    );
}

#[test]
fn process_info_current_delta_updates_while_dialog_remains_open() {
    let mut app = make_test_app(1, 10);
    let a_at = app.snapshot.captured_at;
    let mut a = app.snapshot.processes[0].clone();
    a.private_bytes = Some(100_000_000);
    app.snapshot.processes[0] = a.clone();
    app.process_history
        .record_snapshot_unbounded(a_at, &[a.clone()]);
    app.open_selected_process_info_dialog().unwrap();
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: a_at }),
        b: None,
    });

    let later = a_at + chrono::Duration::seconds(1);
    let mut current = a;
    current.private_bytes = Some(125_000_000);
    app.snapshot.captured_at = later;
    app.snapshot.processes[0] = current.clone();
    app.process_history
        .record_snapshot_unbounded(later, &[current]);

    let view = app.process_info_metrics_view().unwrap();
    assert!(view.range.contains("Current"));
    assert_eq!(
        view.rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .delta
            .as_deref(),
        Some("+25.0 MB")
    );
}

#[test]
fn process_info_uses_paused_history_and_log_view_starts_no_live_worker() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, request_rx, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    let paused_at = app.snapshot.captured_at - chrono::Duration::seconds(5);
    let mut paused_snapshot = app.snapshot.clone();
    paused_snapshot.captured_at = paused_at;
    paused_snapshot.processes[0].private_bytes = Some(42_000_000);
    paused_snapshot.processes[0].executable_path = Some(r"C:\recorded\proc-0.exe".to_string());
    let mut paused_history = ProcessHistory::default();
    paused_history.record_snapshot_unbounded(paused_at, &paused_snapshot.processes);
    app.paused_display = Some(app::state::PausedDisplay {
        snapshot: paused_snapshot,
        exited_tracked_rows: std::collections::HashMap::new(),
        process_history: paused_history,
        system_history: SystemHistory::default(),
        process_info_cache: std::collections::HashMap::new(),
        process_info_display_identity: None,
    });

    app.open_selected_process_info_dialog().unwrap();
    assert_eq!(
        app.process_info_metrics_view()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .value,
        "42.0 MB"
    );
    app.close_process_info_dialog();
    app.log_view_display = app.paused_display.take();
    app.log_view_path = Some(std::path::PathBuf::from("recording.jsonl"));
    app.open_selected_process_info_dialog().unwrap();

    assert!(app.pending_process_info.is_none());
    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        app.process_info_target_process()
            .unwrap()
            .executable_path
            .as_deref(),
        Some(r"C:\recorded\proc-0.exe")
    );
    assert_eq!(
        app.process_info_metrics_view()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.label == "Private Bytes")
            .unwrap()
            .value,
        "42.0 MB"
    );
}

#[test]
fn process_info_resets_tab_filters_when_opened_again() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_files_filter = ".mxf .mp4".to_string();
    app.open_files_filter_cursor = app.open_files_filter.len();
    app.process_modules_filter = "microsoft".to_string();
    app.process_modules_filter_cursor = app.process_modules_filter.len();
    app.process_environment_filter = "path".to_string();
    app.process_environment_filter_cursor = app.process_environment_filter.len();

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.open_files_filter.is_empty());
    assert_eq!(app.open_files_filter_cursor, 0);
    assert!(app.process_modules_filter.is_empty());
    assert_eq!(app.process_modules_filter_cursor, 0);
    assert!(app.process_environment_filter.is_empty());
    assert_eq!(app.process_environment_filter_cursor, 0);
}

#[test]
fn cached_process_info_is_reused_without_worker_request() {
    let mut app = make_test_app(2, 10);
    let identity = app.selected_visible_process_identity().unwrap();
    app.process_info_cache.insert(
        identity.clone(),
        test_process_info(&identity.name, identity.pid),
    );

    app.open_selected_process_info_dialog().unwrap();

    assert!(app.pending_process_info.is_none());
    assert_eq!(app.process_info_display_identity, Some(identity));
}

#[test]
fn process_info_dialog_keeps_the_process_selected_when_opened() {
    let mut app = make_test_app(2, 10);
    let identity = app.selected_visible_process_identity().unwrap();
    app.process_info_cache.insert(
        identity.clone(),
        test_process_info(&identity.name, identity.pid),
    );
    app.process_info_display_identity = Some(identity);
    app.open_selected_process_info_dialog().unwrap();

    app.move_selection_down(1);

    assert_eq!(app.selected_visible_process().unwrap().name, "proc-1");
    assert_eq!(app.process_info_for_selected().unwrap().name, "proc-0");
    assert!(app.pending_process_info.is_none());
}

#[test]
fn process_info_dialog_reopens_on_last_active_tab() {
    let mut app = make_test_app(2, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.activate_process_info_tab(app::ProcessInfoTab::Image)
        .unwrap();
    app.close_process_info_dialog();
    app.move_selection_down(1);

    app.open_selected_process_info_dialog().unwrap();

    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Image);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    assert_eq!(app.process_info_target_process().unwrap().name, "proc-1");
}

#[test]
fn process_info_small_dialog_scrolls_without_overwriting_footer_shortcuts() {
    let mut app = make_test_app(1, 10);
    let captured_at = app.snapshot.captured_at;
    let process = app.snapshot.processes[0].clone();
    app.process_history
        .record_snapshot_unbounded(captured_at, &[process]);
    app.open_selected_process_info_dialog().unwrap();
    let screen = Rect::new(0, 0, 60, 12);
    app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));
    let content = ui::process_info_content_area_for_screen(screen);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: content.x,
            row: content.y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(app.process_info_scroll.offset, 1);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    let rendered = render_app_to_text(&app, screen.width, screen.height);
    assert!(rendered.contains(".NET Allocation Rate"), "{rendered}");
    assert!(!rendered.contains("[ Close ]"), "{rendered}");
    assert!(rendered.contains("Esc close"), "{rendered}");
}

#[test]
fn process_info_scrollbar_thumb_follows_content_focus() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Dlls;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    app.process_modules_result_identity = Some(identity.clone());
    app.process_modules_result = Some(test_process_modules_report(
        &identity.name,
        identity.pid,
        (0..20)
            .map(|index| test_process_module_entry(&format!("module-{index}.dll"), "Test"))
            .collect(),
    ));
    let screen = Rect::new(0, 0, 60, 16);
    app.set_screen_area(screen);
    app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));
    let scrollbar =
        ui::process_info_scrollbar_area_for_screen(screen, &app).expect("Process Info scrollbar");

    let tabs_focused = render_app_to_buffer(&app, screen.width, screen.height);
    assert!(!area_contains_foreground(
        &tabs_focused,
        scrollbar,
        app.theme().focus_border
    ));
    assert!(area_contains_foreground(
        &tabs_focused,
        scrollbar,
        app.theme().muted
    ));

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);
    let content_focused = render_app_to_buffer(&app, screen.width, screen.height);
    assert!(area_contains_foreground(
        &content_focused,
        scrollbar,
        app.theme().focus_border
    ));

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    let inactive = render_app_to_buffer(&app, screen.width, screen.height);
    assert!(!area_contains_foreground(
        &inactive,
        scrollbar,
        app.theme().focus_border
    ));
    assert!(area_contains_foreground(
        &inactive,
        scrollbar,
        app.theme().muted
    ));
}

#[test]
fn narrow_process_info_footer_keeps_dynamic_tab_primary_actions() {
    let mut app = make_test_app(1, 10);
    app.open_selected_process_info_dialog().unwrap();
    let screen = Rect::new(0, 0, 60, 12);
    app.set_screen_area(screen);
    app.process_info_focus = app::ProcessInfoFocus::Content;

    app.process_info_tab = app::ProcessInfoTab::Dlls;
    let dlls = render_app_to_text(&app, screen.width, screen.height);
    assert!(dlls.contains("Enter details"), "{dlls}");
    assert!(dlls.contains("Ctrl+U refresh"), "{dlls}");
    assert!(dlls.contains("Ctrl+C copy path"), "{dlls}");

    app.process_info_tab = app::ProcessInfoTab::Environment;
    let environment = render_app_to_text(&app, screen.width, screen.height);
    assert!(environment.contains("/ filter"), "{environment}");
    assert!(environment.contains("Enter details"), "{environment}");
    assert!(environment.contains("Ctrl+U refresh"), "{environment}");
    assert!(environment.contains("Ctrl+C copy"), "{environment}");
}

#[test]
fn process_info_tabs_and_content_cycle_without_changing_the_fixed_target() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _open_files_request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    let target = app.selected_visible_process_identity().unwrap();
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_scroll.offset = 4;

    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Metrics);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    app.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    let screen = Rect::new(0, 0, 120, 40);
    let tabs_focused = render_app_to_buffer(&app, screen.width, screen.height);
    let tabs_area = ui::process_info_dialog::process_info_dialog_layout_for_screen(screen).tabs;
    let (tab_x, tab_y) = find_text_position_in_area(&tabs_focused, tabs_area, "Metrics")
        .expect("active Process Info tab should render");
    assert_eq!(
        tabs_focused[(tab_x, tab_y)].fg,
        ui::theme::contrasting_foreground(app.theme().focus_border, app.theme())
    );
    assert_eq!(tabs_focused[(tab_x, tab_y)].bg, app.theme().focus_border);
    assert!(
        tabs_focused[(tab_x, tab_y)]
            .modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED)
    );
    assert!(buffer_to_text(&tabs_focused).contains("←/→ tabs  ↑/↓ scroll  Esc close"));
    let (hint_x, hint_y) =
        find_text_position(&tabs_focused, "←/→ tabs").expect("tab-focus shortcut should render");
    assert_eq!(tabs_focused[(hint_x, hint_y)].fg, app.theme().key_hint);
    assert!(
        !tabs_focused[(hint_x, hint_y)]
            .modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(
        tabs_focused[(hint_x + "←/→ ".chars().count() as u16, hint_y)].fg,
        app.theme().text
    );

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Metrics);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    for expected in [
        app::ProcessInfoTab::Image,
        app::ProcessInfoTab::Files,
        app::ProcessInfoTab::Dlls,
        app::ProcessInfoTab::Environment,
        app::ProcessInfoTab::Network,
        app::ProcessInfoTab::Scheduling,
        app::ProcessInfoTab::Metrics,
    ] {
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.process_info_tab, expected);
        assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
        if matches!(
            expected,
            app::ProcessInfoTab::Metrics | app::ProcessInfoTab::Image
        ) {
            app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .unwrap();
            assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
        }
    }
    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Scheduling);
    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Network);
    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Environment);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    let filter_before = app.process_environment_filter.clone();
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    assert_eq!(app.process_environment_filter, filter_before);

    for expected in [
        app::ProcessInfoTab::Network,
        app::ProcessInfoTab::Scheduling,
        app::ProcessInfoTab::Metrics,
        app::ProcessInfoTab::Image,
        app::ProcessInfoTab::Files,
        app::ProcessInfoTab::Dlls,
        app::ProcessInfoTab::Environment,
    ] {
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.process_info_tab, expected);
        let expected_focus = if matches!(
            expected,
            app::ProcessInfoTab::Metrics | app::ProcessInfoTab::Image
        ) {
            app::ProcessInfoFocus::Tabs
        } else {
            app::ProcessInfoFocus::Content
        };
        assert_eq!(app.process_info_focus, expected_focus);
    }

    assert_eq!(app.process_info_scroll.offset, 4);
    assert_eq!(
        app.process_info_target
            .as_ref()
            .map(|target| &target.identity),
        Some(&target)
    );
    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Dlls);

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    assert!(app.show_process_info_dialog);
}

#[test]
fn process_info_mouse_tabs_content_and_outside_click_are_modal() {
    let mut app = make_test_app(2, 10);
    app.open_selected_process_info_dialog().unwrap();
    let screen = Rect::new(0, 0, 200, 60);
    let layout = ui::process_info_dialog::process_info_dialog_layout_for_screen(screen);
    let image_point = (layout.tabs.y..layout.tabs.bottom())
        .flat_map(|y| (layout.tabs.x..layout.tabs.right()).map(move |x| (x, y)))
        .find(|(x, y)| ui::process_info_tab_at(screen, *x, *y) == Some(app::ProcessInfoTab::Image))
        .expect("Image tab should have a hit area");

    app.on_mouse(left_click(image_point.0, image_point.1), screen);
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Image);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);

    let selected = app.selected_visible_process_identity();
    let focused = app.focused_panel;
    app.on_mouse(left_click(0, 10), screen);
    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    assert_eq!(app.selected_visible_process_identity(), selected);
    assert_eq!(app.focused_panel, focused);

    app.on_mouse(left_click(layout.content.x, layout.content.y), screen);
    assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
    assert!(app.show_process_info_dialog);
}

#[test]
fn process_info_image_shows_extended_fields_and_scrolls_long_values() {
    let mut app = make_test_app(1, 10);
    let identity = app.selected_visible_process_identity().unwrap();
    let mut info = test_process_info(&identity.name, identity.pid);
    info.command_line = InfoValue::Value(format!("{}COMMAND-END", "argument ".repeat(80)));
    app.process_info_cache.insert(identity.clone(), info);
    app.process_info_display_identity = Some(identity);
    app.open_selected_process_info_dialog().unwrap();
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
        .unwrap();
    let screen = Rect::new(0, 0, 70, 18);
    app.set_screen_area(screen);
    app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));

    let first_page = render_app_to_text(&app, screen.width, screen.height);
    assert!(first_page.contains("User"), "{first_page}");
    assert!(first_page.contains("Architecture"), "{first_page}");
    assert!(first_page.contains(".NET version"), "{first_page}");
    assert!(first_page.contains("Command line"), "{first_page}");

    app.scroll_process_info_end();
    let last_page = render_app_to_text(&app, screen.width, screen.height);
    assert!(last_page.contains("COMMAND-END"), "{last_page}");
    assert!(last_page.contains("Company"), "{last_page}");
    assert!(last_page.contains("File version"), "{last_page}");
}

#[test]
fn ctrl_i_opens_process_jump_instead_of_info_panel() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(!app.show_system_info_dialog);
    assert!(app.jump_editing);
}

#[test]
fn process_info_request_keeps_the_process_selected_when_dialog_opened() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, request_rx, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        3,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_selected_process_info_dialog().unwrap();

    app.move_selection_down(1);
    app.move_selection_down(1);

    assert_eq!(app.selected_visible_process().unwrap().name, "proc-2");
    assert!(app.pending_process_info.is_some());
    assert!(!app.request_due_process_info().unwrap());
    assert!(matches!(request_rx.try_recv(), Err(TryRecvError::Empty)));

    app.pending_process_info.as_mut().unwrap().changed_at =
        std::time::Instant::now() - PROCESS_INFO_DEBOUNCE;
    assert!(!app.request_due_process_info().unwrap());

    match request_rx.try_recv().unwrap() {
        ProcessInfoRequest::Collect { identity, .. } => {
            assert_eq!(identity.name, "proc-0");
        }
        ProcessInfoRequest::Stop => panic!("unexpected stop request"),
    }
    assert!(app.pending_process_info.is_none());
    assert_eq!(app.process_info_in_flight.as_ref().unwrap().name, "proc-0");
}

#[test]
fn process_metrics_show_gpu_and_io_before_optional_runtime_rows_at_120x60() {
    let mut app = make_test_app(1, 10);
    app.process_info_tab = crate::app::ProcessInfoTab::Metrics;
    app.open_selected_process_info_dialog().unwrap();
    let text = render_app_to_text(&app, 120, 60);
    assert!(text.contains("GPU Dedicated Memory"), "{text}");
    assert!(text.contains("I/O Write Throughput"), "{text}");
    let view = app.process_info_metrics_view().unwrap();
    let labels: Vec<_> = view.rows.iter().map(|row| row.label).collect();
    assert!(
        labels
            .iter()
            .position(|label| *label == "I/O Write Throughput")
            < labels.iter().position(|label| *label == ".NET Heap")
    );
}

#[test]
fn inspection_filter_keeps_text_when_switching_tabs_or_escaping() {
    for tab in [
        app::ProcessInfoTab::Files,
        app::ProcessInfoTab::Dlls,
        app::ProcessInfoTab::Environment,
        app::ProcessInfoTab::Network,
    ] {
        for direction in [KeyCode::Left, KeyCode::Right] {
            let mut app = make_test_app(1, 10);
            let (files, _files_requests, _files_results) = OpenFilesWorker::test_pair();
            app.open_files_worker = files;
            let (network, _network_requests, _network_results) =
                crate::samplers::network::NetworkWorker::test_pair();
            app.network_worker = network;
            let (scheduling, _scheduling_requests, _scheduling_results) =
                crate::samplers::scheduling::SchedulingWorker::test_pair();
            app.scheduling_worker = scheduling;
            app.open_selected_process_info_dialog().unwrap();
            app.process_info_tab = tab;
            app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
                .unwrap();
            for ch in "日a".chars() {
                app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                    .unwrap();
            }
            app.on_key(KeyEvent::new(direction, KeyModifiers::CONTROL))
                .unwrap();
            assert_eq!(
                app.process_info_tab,
                if direction == KeyCode::Left {
                    tab.previous()
                } else {
                    tab.next()
                }
            );
            assert!(!app.process_info_filter_editing);
            assert!(!app.process_network.editing);
            app.process_info_tab = tab;
            app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
                .unwrap();
            app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(app.show_process_info_dialog);
            assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
            assert!(!app.process_info_filter_editing);
            let text = match tab {
                app::ProcessInfoTab::Files => &app.open_files_filter,
                app::ProcessInfoTab::Dlls => &app.process_modules_filter,
                app::ProcessInfoTab::Environment => &app.process_environment_filter,
                _ => &app.process_network.filter,
            };
            assert_eq!(text, "日a");
            app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(!app.show_process_info_dialog);
        }
    }
}

#[test]
fn inspection_tables_pin_headers_and_keep_scrolled_rows_clickable() {
    use super::support::{test_open_files_report, test_process_environment_report};
    use crate::model::ProcessEnvironmentEntry;
    for tab in [
        app::ProcessInfoTab::Files,
        app::ProcessInfoTab::Dlls,
        app::ProcessInfoTab::Environment,
    ] {
        for (width, height) in [(60, 24), (120, 30)] {
            let mut app = make_test_app(1, 10);
            app.open_selected_process_info_dialog().unwrap();
            app.process_info_tab = tab;
            app.process_info_focus = app::ProcessInfoFocus::Content;
            let mut files = test_open_files_report("proc-0", 0, "row-0.bin");
            files.entries = (0..50)
                .map(|i| {
                    let mut entry = files.entries[0].clone();
                    entry.path = format!("C:/row-{i}.bin");
                    entry.handle.value = i;
                    entry
                })
                .collect();
            files.inaccessible_handles = 2;
            app.open_files_result = Some(files);
            app.process_modules_result = Some(test_process_modules_report(
                "proc-0",
                0,
                (0..50)
                    .map(|i| test_process_module_entry(&format!("row-{i}.dll"), "Test"))
                    .collect(),
            ));
            app.process_environment_result = Some(test_process_environment_report(
                "proc-0",
                0,
                (0..50)
                    .map(|i| ProcessEnvironmentEntry {
                        name: format!("ROW_{i}"),
                        value: format!("value-{i}"),
                    })
                    .collect(),
            ));
            let screen = Rect::new(0, 0, width, height);
            app::sync_layout_state(&mut app, screen);
            let content = ui::process_info_content_area_for_screen(screen);
            let (header, body) = ui::process_info_dialog::inspection_table_areas(
                content,
                ui::process_info_dialog::inspection_header_rows(&app),
            );
            let first = render_app_to_buffer(&app, width, height);
            if tab != app::ProcessInfoTab::Files {
                app.on_mouse(
                    MouseEvent {
                        kind: MouseEventKind::ScrollDown,
                        column: body.x,
                        row: body.y,
                        modifiers: KeyModifiers::NONE,
                    },
                    screen,
                );
                let offset = app.process_info_scroll_offset();
                assert!(offset > 0);
                app::sync_layout_state(&mut app, screen);
                assert_eq!(app.process_info_scroll_offset(), offset);
            }
            app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
                .unwrap();
            let last = render_app_to_buffer(&app, width, height);
            assert!(app.process_info_scroll_offset() > 0);
            for y in header.y..header.bottom() {
                for x in header.x..header.right() {
                    assert_eq!(first[(x, y)], last[(x, y)], "header changed for {tab:?}");
                }
            }
            let at = |app: &crate::App, x, y| match tab {
                app::ProcessInfoTab::Files => ui::open_files::index_at(content, app, x, y),
                app::ProcessInfoTab::Dlls => {
                    ui::process_modules::process_module_index_at(content, app, x, y)
                }
                _ => ui::process_environment::process_environment_index_at(content, app, x, y),
            };
            assert_eq!(at(&app, header.x, header.y + 1), None);
            let index = at(&app, body.x, body.y).expect("visible row");
            assert!(index > 0);
            app.on_mouse(left_click(body.x, body.y), screen);
            let selected = match tab {
                app::ProcessInfoTab::Files => app.open_files_selected,
                app::ProcessInfoTab::Dlls => app.process_modules_selected,
                _ => app.process_environment_selected,
            };
            assert_eq!(selected, index);
            app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .unwrap();
            assert!(app.process_info_detail_is_open());
            app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(!app.process_info_detail_is_open());
            assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Content);
            app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(app.show_process_info_dialog);
            assert_eq!(app.process_info_focus, app::ProcessInfoFocus::Tabs);
        }
    }
}
