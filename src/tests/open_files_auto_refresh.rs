use super::support::{make_test_app_with_workers, render_app_to_text, test_open_files_report};
use crate::{
    app::{App, ProcessInfoTab},
    samplers::{
        SamplingWorker,
        open_files::{
            OpenFilesError, OpenFilesReport, OpenFilesRequest, OpenFilesResult, OpenFilesWorker,
        },
        process_info::ProcessInfoWorker,
    },
    ui,
};
use std::{
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant},
};

struct Fixture {
    app: App,
    requests: Receiver<OpenFilesRequest>,
    results: Sender<OpenFilesResult>,
}

impl Fixture {
    fn new() -> Self {
        let (sampling, _, _) = SamplingWorker::test_pair();
        let (info, _, _) = ProcessInfoWorker::test_pair();
        let (files, requests, results) = OpenFilesWorker::test_pair();
        let mut app = make_test_app_with_workers(2, 10, sampling, info, files);
        app.open_selected_process_files().unwrap();
        Self {
            app,
            requests,
            results,
        }
    }

    fn deliver(&mut self, elapsed_ms: u64, report: OpenFilesReport) -> bool {
        let OpenFilesRequest::Collect {
            generation,
            identity,
            ..
        } = self.requests.try_recv().unwrap()
        else {
            panic!("expected collection")
        };
        self.results
            .send(OpenFilesResult {
                generation,
                identity,
                report,
                elapsed: Duration::from_millis(elapsed_ms),
            })
            .unwrap();
        self.app.poll_open_files_results().unwrap()
    }

    fn due(&mut self) {
        let due = self.app.open_files_refresh.next_due.unwrap();
        assert!(self.app.request_due_open_files_at(due).unwrap());
    }
}

#[test]
fn files_handoff_reveals_offscreen_target_once_without_stealing_focus() {
    let mut f = Fixture::new();
    let mut report = test_open_files_report("proc-0", 0, "base.txt");
    let template = report.entries[0].clone();
    report.entries = (0..60)
        .map(|i| {
            let mut entry = template.clone();
            entry.path = format!(r"C:\files\file-{i:02}.txt");
            entry.handle.value = i;
            entry
        })
        .collect();
    f.app.open_files_match_path = Some(report.entries[50].path.clone());
    let screen = ratatui::layout::Rect::new(0, 0, 100, 24);
    crate::app::sync_layout_state(&mut f.app, screen);
    f.app.process_info_focus = crate::app::ProcessInfoFocus::Tabs;
    assert!(f.deliver(10, report.clone()));
    assert!(f.app.open_files_filter.is_empty());
    assert_eq!(ui::open_files::filtered_entries(&f.app).len(), 60);
    assert_eq!(f.app.open_files_selected, 50);
    assert!(f.app.open_files_scroll.offset > 0);
    assert!(render_app_to_text(&f.app, 100, 24).contains("file-50.txt"));
    assert_eq!(f.app.process_info_focus, crate::app::ProcessInfoFocus::Tabs);
    f.app.select_open_file(3);
    let offset = f.app.open_files_scroll.offset;
    f.due();
    assert!(f.deliver(10, report));
    assert_eq!(f.app.open_files_selected, 3);
    assert_eq!(f.app.open_files_scroll.offset, offset);
}

#[test]
fn files_missing_handoff_keeps_other_rows_and_reports_failure() {
    for failed in [false, true] {
        let mut f = Fixture::new();
        f.app.open_files_match_path = Some(r"C:\absent.txt".into());
        let mut report = test_open_files_report("proc-0", 0, "other.txt");
        if failed {
            report.error = Some(OpenFilesError::AccessDenied);
        }
        assert!(f.deliver(10, report));
        assert!(f.app.open_files_match_unavailable);
        assert!(render_app_to_text(&f.app, 120, 35).contains("Original match unavailable"));
        if !failed {
            assert_eq!(ui::open_files::filtered_entries(&f.app).len(), 1);
            assert!(render_app_to_text(&f.app, 120, 35).contains("other.txt"));
        }
    }
}

#[test]
fn files_filter_edit_before_capture_cancels_initial_reveal() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut f = Fixture::new();
    f.app.open_files_match_path = Some(r"C:\absent.txt".into());
    f.app
        .on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
        .unwrap();
    f.app
        .on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE))
        .unwrap();
    assert!(f.app.open_files_match_path.is_none());
    assert!(f.deliver(10, test_open_files_report("proc-0", 0, "other.txt")));
    assert_eq!(f.app.open_files_filter, "o");
    assert!(!f.app.open_files_match_unavailable);
    assert!(f.app.process_info_filter_editing);
}

#[test]
fn files_auto_refresh_is_serial_and_preserves_filter_selection_and_status() {
    let mut f = Fixture::new();
    let mut report = test_open_files_report("proc-0", 0, "keep.log");
    let mut other = report.entries[0].clone();
    other.path = other.path.replace("keep.log", "earlier.log");
    other.handle.value += 4;
    report.entries.insert(0, other);
    assert!(f.deliver(35, report.clone()));
    f.app.open_files_filter = ".log".into();
    f.app.open_files_filter_cursor = 2;
    f.app.select_open_file(1);
    let selected = ui::open_files::selected_entry(&f.app).unwrap().handle.value;
    f.app.status = "Copied file rows".into();
    let due = f.app.open_files_refresh.next_due.unwrap();
    assert!(
        !f.app
            .request_due_open_files_at(due - Duration::from_millis(1))
            .unwrap()
    );
    f.due();
    assert_eq!(f.app.status, "Copied file rows");
    for _ in 0..3 {
        assert!(
            !f.app
                .request_due_open_files_at(due + Duration::from_secs(30))
                .unwrap()
        );
    }
    f.app.refresh_open_files().unwrap();
    f.app.status = "Copied file rows".into();
    report.entries.remove(0);
    assert!(f.deliver(45, report));
    assert!(f.requests.try_recv().is_err());
    assert_eq!(f.app.status, "Copied file rows");
    assert_eq!(f.app.open_files_filter, ".log");
    assert_eq!(f.app.open_files_filter_cursor, 2);
    assert_eq!(f.app.open_files_selected, 0);
    assert_eq!(
        ui::open_files::selected_entry(&f.app).unwrap().handle.value,
        selected
    );
    for (width, height) in [(60, 18), (80, 24), (160, 35)] {
        crate::app::sync_layout_state(&mut f.app, ratatui::layout::Rect::new(0, 0, width, height));
        let text = render_app_to_text(&f.app, width, height);
        assert!(text.contains("Auto 2s · 45 ms"), "{text}");
        assert!(text.contains("Filter: .log"), "{text}");
    }
}

#[test]
fn files_auto_refresh_adapts_to_cost_and_manual_retry_resumes_after_slow_or_error() {
    let mut f = Fixture::new();
    let report = test_open_files_report("proc-0", 0, "keep.log");
    assert!(f.deliver(30, report.clone()));
    for (elapsed, seconds) in [
        (200, Some(2)),
        (201, Some(3)),
        (800, Some(8)),
        (1000, Some(10)),
        (1001, None),
    ] {
        f.due();
        assert!(f.deliver(elapsed, report.clone()));
        assert_eq!(
            f.app.open_files_refresh.interval,
            seconds.map(Duration::from_secs)
        );
    }
    assert!(render_app_to_text(&f.app, 80, 24).contains("Auto off (slow: 1001 ms)"));
    assert!(
        !f.app
            .request_due_open_files_at(Instant::now() + Duration::from_secs(100))
            .unwrap()
    );
    f.app.refresh_open_files().unwrap();
    assert!(f.deliver(20, report.clone()));
    assert_eq!(
        f.app.open_files_refresh.interval,
        Some(Duration::from_secs(2))
    );
    f.due();
    let mut failure = report.clone();
    failure.error = Some(OpenFilesError::AccessDenied);
    assert!(f.deliver(20, failure));
    assert!(f.app.open_files_refresh.next_due.is_none());
    assert!(render_app_to_text(&f.app, 160, 35).contains("Auto off; Ctrl+U retry"));
    f.app.refresh_open_files().unwrap();
    assert!(f.deliver(20, report));
    assert!(f.app.open_files_refresh.next_due.is_some());
}

#[test]
fn files_auto_refresh_stops_when_hidden_exited_or_in_log_view() {
    let mut f = Fixture::new();
    assert!(f.deliver(30, test_open_files_report("proc-0", 0, "keep.log")));
    let due = f.app.open_files_refresh.next_due.unwrap();
    f.app
        .activate_process_info_tab(ProcessInfoTab::Metrics)
        .unwrap();
    assert!(!f.app.request_due_open_files_at(due).unwrap());
    f.app
        .activate_process_info_tab(ProcessInfoTab::Files)
        .unwrap();
    assert!(f.requests.try_recv().is_err());
    f.app.log_view_path = Some("recording.log".into());
    assert!(!f.app.request_due_open_files_at(due).unwrap());
    f.app.log_view_path = None;
    let target = f.app.snapshot.processes.remove(0);
    assert!(!f.app.request_due_open_files_at(due).unwrap());
    assert!(render_app_to_text(&f.app, 80, 24).contains("Process exited"));
    f.app.snapshot.processes.insert(0, target);
    f.due();
    f.app.close_process_info_dialog();
    assert!(f.app.open_files_refresh.next_due.is_none());
    assert!(
        !f.app
            .request_due_open_files_at(due + Duration::from_secs(50))
            .unwrap()
    );
    // A completed request for the closed session must not restart the timer.
    assert!(!f.deliver(20, test_open_files_report("proc-0", 0, "late.log")));
    assert!(f.app.open_files_refresh.next_due.is_none());
}

#[test]
fn files_auto_refresh_during_recording_does_not_record_file_data() {
    let mut f = Fixture::new();
    assert!(f.deliver(30, test_open_files_report("proc-0", 0, "private-file.bin")));
    let path = super::support::unique_recording_path("files-auto-refresh");
    super::support::track_process_name(&mut f.app, "proc-0");
    f.app.recording_path_draft = path.to_string_lossy().into_owned();
    f.app.recording_path_cursor = f.app.recording_path_draft.len();
    f.app.show_recording_path_dialog = true;
    f.app.confirm_recording_path().unwrap();
    f.due();
    assert!(f.deliver(25, test_open_files_report("proc-0", 0, "private-file.bin")));
    f.app.stop_recording().unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    for absent in ["private-file.bin", "open_files", "Auto 2s"] {
        assert!(!saved.contains(absent));
    }
    std::fs::remove_file(path).unwrap();
}
