use super::support::{
    buffer_to_text, find_text_position, make_test_app, make_test_app_with_workers,
    render_app_to_buffer, render_app_to_text, show_process_info_files_tab, test_open_files_report,
};
use crate::app;
use crate::app::FocusedPanel;
use crate::model::open_files::{FileAttribute, FileAttributeError, OpenFileHandle};
use crate::samplers::SamplingWorker;
use crate::samplers::open_files::{
    OpenFileEntry, OpenFilesError, OpenFilesReport, OpenFilesRequest, OpenFilesResult,
    OpenFilesWorker,
};
use crate::samplers::process_info::ProcessInfoWorker;
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use std::sync::mpsc::TryRecvError;

fn mode_entry(
    path: &str,
    value: usize,
    access: Result<u32, FileAttributeError>,
    mode: Result<u32, FileAttributeError>,
) -> OpenFileEntry {
    OpenFileEntry {
        path: path.into(),
        handle: OpenFileHandle {
            value,
            access,
            mode,
        },
    }
}

#[test]
fn open_file_modes_keep_each_handle_and_each_unknown_attribute_independent() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    let mut report = test_open_files_report("proc-0", 0, "same.bin");
    report.entries = vec![
        mode_entry(r"C:\media\same.bin", 0x40, Ok(1), Ok(0x20)),
        mode_entry(r"C:\media\same.bin", 0x44, Ok(6), Ok(0x0a)),
        mode_entry(
            r"C:\media\same.bin",
            0x48,
            Ok(0x80),
            Err(FileAttributeError::NtStatus(0xc0000022u32 as i32)),
        ),
        mode_entry(
            r"C:\media\same.bin",
            0x4c,
            Err(FileAttributeError::ShortReply),
            Ok(0),
        ),
    ];
    report.file_handles = report.entries.len();
    report.total_handles = report.entries.len();
    app.open_files_result = Some(report);
    let entries = ui::open_files::filtered_entries(&app);
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].attribute(FileAttribute::Cache), "Y");
    assert_eq!(entries[0].attribute(FileAttribute::IoMode), "N");
    assert_eq!(entries[0].attribute(FileAttribute::WriteThrough), "N");
    assert_eq!(entries[1].attribute(FileAttribute::Cache), "N");
    assert_eq!(entries[1].attribute(FileAttribute::WriteThrough), "Y");
    assert_eq!(entries[1].attribute(FileAttribute::IoMode), "Y");
    assert_eq!(entries[1].attribute(FileAttribute::Access), "WA");
    assert_eq!(entries[2].attribute(FileAttribute::Access), "-");
    assert_eq!(entries[2].attribute(FileAttribute::Cache), "--");
    assert_eq!(entries[2].attribute(FileAttribute::IoMode), "--");
    assert_eq!(entries[2].attribute(FileAttribute::WriteThrough), "--");
    assert_eq!(entries[3].attribute(FileAttribute::Cache), "Y");
    assert_eq!(entries[3].attribute(FileAttribute::Access), "--");
    let screen = Rect::new(0, 0, 160, 35);
    app::sync_layout_state(&mut app, screen);
    let rendered = render_app_to_text(&app, screen.width, screen.height);
    assert_eq!(rendered.matches("same.bin").count(), 4);
    assert!(!rendered.contains("Mixed"));
    for value in ["0x40", "0x44", "0x48", "0x4C", "WA", "--"] {
        assert!(rendered.contains(value), "{value}: {rendered}");
    }
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.open_files_show_detail);
    assert!(render_app_to_text(&app, 160, 35).contains("0xC0000022"));
    app.copy_open_files_to_clipboard().unwrap();
    let copy = crate::app::clipboard::last_copied_text().unwrap();
    assert_eq!(copy.lines().count(), 1);
    assert!(copy.contains("\t0x48\t--\t--\t-\t--\t0x00000080\t-- (NTSTATUS 0xC0000022)"));
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.open_files_selected, 2);
    assert!(app.show_process_info_dialog);
    app.copy_open_files_to_clipboard().unwrap();
    assert_eq!(
        crate::app::clipboard::last_copied_text()
            .unwrap()
            .lines()
            .count(),
        4
    );
}

#[test]
fn open_file_modes_selection_filter_details_and_resize_share_row_geometry() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    let mut report = test_open_files_report("proc-0", 0, "same.bin");
    report.entries = (0..40)
        .map(|index| mode_entry(r"C:\media\同じ映像.bin", 0x40 + index * 4, Ok(7), Ok(0xa)))
        .collect();
    app.open_files_result = Some(report);
    for (width, height) in [(160, 35), (80, 24), (60, 18), (160, 35)] {
        let screen = Rect::new(0, 0, width, height);
        app::sync_layout_state(&mut app, screen);
        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.open_files_selected, 39);
        let area = ui::process_info_content_area_for_screen(screen);
        let buffer = render_app_to_buffer(&app, width, height);
        let (x, y) = super::support::find_text_position_in_area(&buffer, area, "0xDC")
            .expect("last handle visible");
        assert_eq!(buffer[(x, y)].bg, app.theme().focus_surface);
        if width == 60 {
            let rendered = buffer_to_text(&buffer);
            assert!(rendered.contains("Cached: N  Async: Y  Access: RWA  W-Thru: Y"));
            app.on_mouse(super::support::left_click(x, y + 1), screen);
            assert_eq!(app.open_files_selected, 39);
        }
        app.on_mouse(super::support::left_click(x, y), screen);
        assert_eq!(app.open_files_selected, 39);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        app::sync_layout_state(&mut app, screen);
        assert!(app.open_files_show_detail);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.open_files_filter.is_empty());
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.open_files_show_detail);
        assert_eq!(app.open_files_selected, 39);
    }
    for ch in "映像/ r a".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }
    assert_eq!(app.open_files_filter, "映像/ r a");
    assert_eq!(app.open_files_selected, 0);
}

#[test]
fn open_file_modes_unicode_names_do_not_shift_attribute_columns() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    let mut report = test_open_files_report("proc-0", 0, "file.bin");
    report.entries = vec![mode_entry(
        r"C:\日本語の長いディレクトリ\全角ファイル名の長い映像ファイル.bin",
        0x40,
        Ok(7),
        Ok(0xa),
    )];
    app.open_files_result = Some(report);
    for width in [80, 112, 160] {
        let screen = Rect::new(0, 0, width, 35);
        app::sync_layout_state(&mut app, screen);
        let area = ui::process_info_content_area_for_screen(screen);
        let buffer = render_app_to_buffer(&app, width, 35);
        let mut previous_x = area.x;
        for (heading, value) in [
            ("Cached", "N"),
            ("Async", "Y"),
            ("Access", "RWA"),
            ("W-Thru", "Y"),
        ] {
            let (x, y) = super::support::find_text_position_in_area(&buffer, area, heading)
                .unwrap_or_else(|| panic!("{heading} at width {width}"));
            assert!(x > previous_x, "column order at width {width}");
            previous_x = x;
            for (offset, ch) in value.chars().enumerate() {
                assert_eq!(buffer[(x + offset as u16, y + 1)].symbol(), ch.to_string());
            }
            assert_eq!(buffer[(x, y + 1)].bg, app.theme().focus_surface);
        }
        let (directory_x, header_y) =
            super::support::find_text_position_in_area(&buffer, area, "Directory").unwrap();
        assert!(
            directory_x > previous_x,
            "Directory is last at width {width}"
        );
        let mut directory_text = String::new();
        let mut x = directory_x;
        while x < area.right().saturating_sub(1) {
            let symbol = buffer[(x, header_y + 1)].symbol();
            directory_text.push_str(symbol);
            // Skip the trailing buffer cell occupied by each full-width character.
            x += ratatui::text::Span::raw(symbol).width().max(1) as u16;
        }
        assert!(
            directory_text.trim_end().ends_with("ディレクトリ"),
            "Directory at width {width}: {directory_text}"
        );
    }
}

#[test]
fn open_file_modes_remain_outside_recordings() {
    let mut app = make_test_app(1, 10);
    let path = super::support::unique_recording_path("file-handle-modes");
    super::support::track_process_name(&mut app, "proc-0");
    app.recording_path_draft = path.to_string_lossy().into_owned();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    let mut report = test_open_files_report("proc-0", 0, "private-file.bin");
    report.entries = vec![mode_entry(r"C:\private-file.bin", 0xabcdef, Ok(7), Ok(0xa))];
    app.open_files_result = Some(report);
    app.stop_recording().unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    for absent in ["private-file.bin", "W-Thru", "ABCDEF", "open_files"] {
        assert!(!saved.contains(absent));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn open_file_modes_refresh_retains_the_selected_handle_among_duplicate_paths() {
    let mut app = make_test_app(1, 10);
    let (worker, requests, results) = OpenFilesWorker::test_pair();
    app.open_files_worker = worker;
    app.open_selected_process_files().unwrap();
    let OpenFilesRequest::Collect {
        generation,
        identity,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!("expected collect");
    };
    let mut report = test_open_files_report(&identity.name, identity.pid, "same.bin");
    report.entries = vec![
        mode_entry(r"C:\same.bin", 0x40, Ok(1), Ok(0x20)),
        mode_entry(r"C:\same.bin", 0x44, Ok(2), Ok(0xa)),
    ];
    results
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation,
            identity: identity.clone(),
            report: report.clone(),
        })
        .unwrap();
    app.poll_open_files_results().unwrap();
    app::sync_layout_state(&mut app, Rect::new(0, 0, 80, 24));
    app.select_open_file(1);
    app.open_selected_process_info_detail();
    app.refresh_open_files().unwrap();
    let _ = requests.try_recv().unwrap();
    report.entries.remove(0);
    results
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation,
            identity: identity.clone(),
            report: report.clone(),
        })
        .unwrap();
    app.poll_open_files_results().unwrap();
    assert!(app.open_files_show_detail);
    assert_eq!(app.open_files_selected, 0);
    assert_eq!(
        ui::open_files::selected_entry(&app).unwrap().handle.value,
        0x44
    );
    app.refresh_open_files().unwrap();
    let _ = requests.try_recv().unwrap();
    report.entries[0].handle.value = 0x48;
    results
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation,
            identity,
            report,
        })
        .unwrap();
    app.poll_open_files_results().unwrap();
    assert!(!app.open_files_show_detail);
    assert_eq!(
        ui::open_files::selected_entry(&app).unwrap().handle.value,
        0x48
    );
}

#[test]
fn open_file_mode_fixture_child() {
    use std::{
        fs::OpenOptions,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    let Some(directory) = std::env::var_os("WINPROC_FILE_MODE_FIXTURE") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let path = directory.join("same.bin");
    std::fs::write(&path, b"fixture").unwrap();
    let variants = [
        (0, 1),
        (0x20000000, 1),
        (0x80000000, 2),
        (0x40000000, 3),
        (0xe0000000, 3),
        (0, 4),
        (0, 0x80),
        (0x40000000, 0),
    ];
    let mut files = Vec::new();
    let mut expected = Vec::new();
    for (flags, access) in variants {
        let file = OpenOptions::new()
            .access_mode(access)
            .share_mode(7)
            .custom_flags(flags)
            .open(&path)
            .unwrap();
        expected.push((file.as_raw_handle() as usize, flags, access));
        files.push(file);
    }
    std::fs::write(
        directory.join("ready.json"),
        serde_json::to_vec(&expected).unwrap(),
    )
    .unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    drop(files);
}

#[test]
fn open_file_modes_collect_original_handles_from_dedicated_process() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    let directory = std::env::temp_dir().join(format!(
        "winproc-file-mode-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::open_files::open_file_mode_fixture_child",
            "--nocapture",
        ])
        .env("WINPROC_FILE_MODE_FIXTURE", &directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    struct Fixture {
        child: std::process::Child,
        directory: std::path::PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            for name in ["same.bin", "ready.json"] {
                let _ = std::fs::remove_file(self.directory.join(name));
            }
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
    let mut fixture = Fixture { child, directory };
    let started = Instant::now();
    let expected: Vec<(usize, u32, u32)> = loop {
        if let Ok(bytes) = std::fs::read(fixture.directory.join("ready.json"))
            && let Ok(values) = serde_json::from_slice(&bytes)
        {
            break values;
        }
        assert!(
            fixture.child.try_wait().unwrap().is_none(),
            "fixture exited before ready"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "fixture startup timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut process = make_test_app(1, 1).snapshot.processes[0].clone();
    process.pid = fixture.child.id();
    let report = crate::samplers::open_files::collect_open_files_for_process(&process);
    assert!(report.error.is_none(), "{:?}", report.error);
    let entries = report
        .entries
        .iter()
        .filter(|entry| entry.path.ends_with("same.bin"))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), expected.len(), "{entries:?}");
    for (handle, flags, access) in expected {
        let entry = entries
            .iter()
            .find(|entry| entry.handle.value == handle)
            .unwrap();
        assert_eq!(
            entry.handle.access.as_ref().unwrap() & 7,
            access & 7,
            "{entry:?}"
        );
        let expected_access = match access {
            0 | 0x80 => "-",
            1 => "R",
            2 => "W",
            3 => "RW",
            4 => "A",
            _ => panic!("unexpected fixture access: {access}"),
        };
        assert_eq!(entry.attribute(FileAttribute::Access), expected_access);
        assert_eq!(
            entry.attribute(FileAttribute::Cache),
            if flags & 0x20000000 != 0 { "N" } else { "Y" },
            "{entry:?}"
        );
        assert_eq!(
            entry.attribute(FileAttribute::WriteThrough),
            if flags & 0x80000000 != 0 { "Y" } else { "N" },
            "{entry:?}"
        );
        assert_eq!(
            entry.attribute(FileAttribute::IoMode),
            if flags & 0x40000000 != 0 { "Y" } else { "N" },
            "{entry:?}"
        );
        let copied = entry.plain_text();
        let fields = copied.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[2], if flags & 0x20000000 != 0 { "N" } else { "Y" });
        assert_eq!(fields[3], if flags & 0x40000000 != 0 { "Y" } else { "N" });
        assert_eq!(fields[4], expected_access);
        assert_eq!(fields[5], if flags & 0x80000000 != 0 { "Y" } else { "N" });
    }
}

#[test]
fn f_requests_open_files_for_selected_process() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.process_info_tab = app::ProcessInfoTab::Environment;

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_tab, app::ProcessInfoTab::Files);
    assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
    match request_rx.try_recv().unwrap() {
        OpenFilesRequest::Collect {
            identity, process, ..
        } => {
            assert_eq!(identity.name, "proc-0");
            assert_eq!(process.name, "proc-0");
        }
        OpenFilesRequest::Stop => panic!("unexpected stop request"),
    }
}

#[test]
fn f_does_not_open_files_outside_processes_focus() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.focused_panel = FocusedPanel::System;

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_process_info_dialog);
    assert!(request_rx.try_recv().is_err());
}

#[test]
fn ctrl_u_refreshes_open_files_for_selected_process() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        2,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = app::ProcessInfoTab::Files;
    app.process_info_focus = app::ProcessInfoFocus::Content;
    let identity = app.process_info_target.as_ref().unwrap().identity.clone();
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 1,
        file_handles: 1,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![OpenFileEntry {
            path: r"C:\tmp\a.log".to_string(),
            handle: crate::model::open_files::OpenFileHandle {
                value: 1,
                access: Ok(1),
                mode: Ok(0x20),
            },
        }],
        error: None,
    });
    app.open_files_result_identity = Some(identity);

    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.open_files_result.is_some());
    assert_eq!(app.open_files_in_flight.as_ref().unwrap().name, "proc-0");
    match request_rx.try_recv().unwrap() {
        OpenFilesRequest::Collect {
            identity, process, ..
        } => {
            assert_eq!(identity.name, "proc-0");
            assert_eq!(process.name, "proc-0");
        }
        OpenFilesRequest::Stop => panic!("unexpected stop request"),
    }
}

#[test]
fn open_files_result_updates_modal_state() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, request_rx, result_tx) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.open_selected_process_files().unwrap();
    let (generation, identity) = match request_rx.try_recv().unwrap() {
        OpenFilesRequest::Collect {
            generation,
            identity,
            ..
        } => (generation, identity),
        OpenFilesRequest::Stop => panic!("unexpected stop request"),
    };

    result_tx
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation,
            identity: identity.clone(),
            report: OpenFilesReport {
                pid: 0,
                process_name: "proc-0".to_string(),
                total_handles: 3,
                file_handles: 2,
                inaccessible_handles: 1,
                unnamed_file_handles: 0,
                entries: vec![OpenFileEntry {
                    path: r"C:\tmp\a.log".to_string(),
                    handle: crate::model::open_files::OpenFileHandle {
                        value: 2,
                        access: Ok(1),
                        mode: Ok(0x20),
                    },
                }],
                error: None,
            },
        })
        .unwrap();

    assert!(app.poll_open_files_results().unwrap());
    assert!(app.open_files_in_flight.is_none());
    assert_eq!(app.open_files_result.as_ref().unwrap().entries.len(), 1);
    assert!(app.status.contains("Loaded 1 named file handles"));
}

#[test]
fn open_files_clipboard_is_raw_handle_rows_without_header() {
    let mut app = make_test_app(1, 10);
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 2,
        file_handles: 2,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![
            OpenFileEntry {
                path: r"C:\tmp\a.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\tmp\b.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 2,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
        ],
        error: None,
    });

    app.copy_open_files_to_clipboard().unwrap();

    assert_eq!(
        crate::app::clipboard::last_copied_text().unwrap(),
        "C:\\tmp\\a.log\t0x1\tY\tN\tR\tN\t0x00000001\t0x00000020\nC:\\tmp\\b.log\t0x2\tY\tN\tR\tN\t0x00000001\t0x00000020"
    );
}

#[test]
fn open_files_clipboard_filter_matches_full_paths() {
    let mut app = make_test_app(1, 10);
    app.open_files_filter = "exports".to_string();
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 3,
        file_handles: 3,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![
            OpenFileEntry {
                path: r"C:\tmp\a.wav".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\exports\b.MXF".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 2,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\media\c.mp4".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
        ],
        error: None,
    });

    app.copy_open_files_to_clipboard().unwrap();

    assert_eq!(
        crate::app::clipboard::last_copied_text().unwrap(),
        "C:\\exports\\b.MXF\t0x2\tY\tN\tR\tN\t0x00000001\t0x00000020"
    );
}

#[test]
fn open_files_filter_cursor_moves_and_inserts_at_cursor() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_filter = ".mp4".to_string();
    app.open_files_filter_cursor = app.open_files_filter.len();

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.open_files_filter, ".mpx4");
    assert_eq!(app.open_files_filter_cursor, ".mpx".len());

    app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.open_files_filter, ".mp4");
    assert_eq!(app.open_files_filter_cursor, app.open_files_filter.len());
}

#[test]
fn open_files_filter_delete_removes_character_at_cursor() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_filter = ".mxpf".to_string();
    app.open_files_filter_cursor = ".mx".len();

    app.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.open_files_filter, ".mxf");
    assert_eq!(app.open_files_filter_cursor, ".mx".len());
}

#[test]
fn open_files_filter_shows_colon_and_terminal_cursor() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_filter = ".mp4".to_string();
    app.open_files_filter_cursor = ".m".len();
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 1,
        file_handles: 1,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![OpenFileEntry {
            path: r"C:\tmp\a.mp4".to_string(),
            handle: crate::model::open_files::OpenFileHandle {
                value: 1,
                access: Ok(1),
                mode: Ok(0x20),
            },
        }],
        error: None,
    });
    let screen = Rect::new(0, 0, 160, 45);
    let content = ui::process_info_content_area_for_screen(screen);
    let expected_cursor = Position::new(content.x + 10, content.y + 1);

    let backend = TestBackend::new(screen.width, screen.height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");
    terminal
        .draw(|frame| ui::draw(frame, &app))
        .expect("test render should succeed");
    terminal
        .backend_mut()
        .assert_cursor_position(expected_cursor);
    let rendered = buffer_to_text(terminal.backend().buffer());

    assert!(rendered.contains("Filter: .mp4"), "{rendered}");
}

#[test]
fn open_files_modal_size_stays_fixed_while_filtering() {
    let mut app = make_test_app(1, 10);
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 3,
        file_handles: 3,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![
            OpenFileEntry {
                path: r"C:\tmp\a.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\tmp\b.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\tmp\c.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
        ],
        error: None,
    });
    let screen = Rect::new(0, 0, 160, 45);
    show_process_info_files_tab(&mut app);
    let before = ui::process_info_page_size_for_screen(screen);

    app.open_files_filter = "b.log".to_string();
    let after = ui::process_info_page_size_for_screen(screen);

    assert_eq!(before, after);
}

#[test]
fn open_files_modal_renders_table_columns() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 1,
        file_handles: 1,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![OpenFileEntry {
            path: r"C:\tmp\a.log".to_string(),
            handle: crate::model::open_files::OpenFileHandle {
                value: 1,
                access: Ok(1),
                mode: Ok(0x20),
            },
        }],
        error: None,
    });

    let rendered = render_app_to_text(&app, 160, 45);

    for heading in [
        "Handle",
        "File",
        "Cached",
        "Async",
        "Access",
        "W-Thru",
        "Directory",
    ] {
        assert!(rendered.contains(heading), "{heading}: {rendered}");
    }
    assert!(rendered.contains("a.log"), "{rendered}");
    assert!(rendered.contains(r"C:\tmp"), "{rendered}");
}

#[test]
fn open_files_filter_matches_directory_and_shows_filtered_total() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_filter = "fonts".to_string();
    app.open_files_filter_cursor = app.open_files_filter.len();
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 2,
        file_handles: 2,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![
            OpenFileEntry {
                path: r"C:\Windows\Fonts\a.ttf".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
            OpenFileEntry {
                path: r"C:\tmp\b.log".to_string(),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            },
        ],
        error: None,
    });

    let rendered = render_app_to_text(&app, 120, 30);

    assert!(rendered.contains("shown 1/2"), "{rendered}");
    assert!(rendered.contains("a.ttf"), "{rendered}");
    assert!(!rendered.contains("b.log"), "{rendered}");
}

#[test]
fn open_files_table_column_names_are_underlined() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 1,
        file_handles: 1,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: vec![OpenFileEntry {
            path: r"C:\tmp\a.log".to_string(),
            handle: crate::model::open_files::OpenFileHandle {
                value: 1,
                access: Ok(1),
                mode: Ok(0x20),
            },
        }],
        error: None,
    });

    let buffer = render_app_to_buffer(&app, 160, 45);
    let (x, y) = find_text_position(&buffer, "Handle").expect("header should render");
    let cell = &buffer[(x, y)];

    assert!(cell.modifier.contains(ratatui::style::Modifier::UNDERLINED));
    assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn open_files_scroll_offset_changes_rendered_rows() {
    let mut app = make_test_app(1, 10);
    show_process_info_files_tab(&mut app);
    app.open_files_result = Some(OpenFilesReport {
        pid: 0,
        process_name: "proc-0".to_string(),
        total_handles: 30,
        file_handles: 30,
        inaccessible_handles: 0,
        unnamed_file_handles: 0,
        entries: (0..30)
            .map(|index| OpenFileEntry {
                path: format!(r"C:\tmp\file-{index:02}.log"),
                handle: crate::model::open_files::OpenFileHandle {
                    value: 1,
                    access: Ok(1),
                    mode: Ok(0x20),
                },
            })
            .collect(),
        error: None,
    });
    let screen = Rect::new(0, 0, 160, 45);
    app.set_process_info_page_size(ui::process_info_page_size_for_screen(screen));
    app.scroll_process_info_end();

    let rendered = render_app_to_text(&app, screen.width, screen.height);

    assert!(!rendered.contains("file-00.log"), "{rendered}");
    assert!(rendered.contains("file-29.log"), "{rendered}");
}

#[test]
fn files_tab_in_log_view_does_not_request_live_collection() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, process_request_rx, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, open_files_request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );
    app.log_view_path = Some(std::path::PathBuf::from("recording.log"));

    app.open_selected_process_info_dialog().unwrap();
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
        .unwrap();

    assert!(matches!(
        process_request_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
    assert!(matches!(
        open_files_request_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
    let rendered = render_app_to_text(&app, 120, 40);
    assert!(
        rendered.contains("This information is not included in recording logs."),
        "{rendered}"
    );
}

#[test]
fn files_tab_does_not_query_after_the_fixed_target_exits() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, open_files_request_rx, _) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );

    app.open_selected_process_info_dialog().unwrap();
    app.snapshot.processes.clear();
    app.activate_process_info_tab(app::ProcessInfoTab::Files)
        .unwrap();

    assert!(matches!(
        open_files_request_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
    assert_eq!(
        app.open_files_result
            .as_ref()
            .and_then(|report| report.error.as_ref()),
        Some(&OpenFilesError::ProcessExited)
    );
    assert_eq!(app.status, "Process has exited");
}

#[test]
fn stale_open_files_result_cannot_replace_reopened_dialog_request() {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, request_rx, result_tx) = OpenFilesWorker::test_pair();
    let mut app = make_test_app_with_workers(
        1,
        10,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    );

    app.open_selected_process_files().unwrap();
    let (old_generation, identity) = match request_rx.try_recv().unwrap() {
        OpenFilesRequest::Collect {
            generation,
            identity,
            ..
        } => (generation, identity),
        OpenFilesRequest::Stop => panic!("unexpected stop request"),
    };
    app.close_process_info_dialog();
    app.open_selected_process_files().unwrap();
    let new_generation = match request_rx.try_recv().unwrap() {
        OpenFilesRequest::Collect { generation, .. } => generation,
        OpenFilesRequest::Stop => panic!("unexpected stop request"),
    };

    result_tx
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation: old_generation,
            identity: identity.clone(),
            report: test_open_files_report(&identity.name, identity.pid, "old.log"),
        })
        .unwrap();
    assert!(!app.poll_open_files_results().unwrap());
    assert_eq!(app.open_files_in_flight_generation, Some(new_generation));
    assert!(app.open_files_result.is_none());
    assert!(app.open_files_refresh.next_due.is_none());

    result_tx
        .send(OpenFilesResult {
            elapsed: std::time::Duration::from_millis(30),
            generation: new_generation,
            identity: identity.clone(),
            report: test_open_files_report(&identity.name, identity.pid, "new.log"),
        })
        .unwrap();
    assert!(app.poll_open_files_results().unwrap());
    assert!(
        app.open_files_result.as_ref().unwrap().entries[0]
            .path
            .ends_with("new.log")
    );
}
