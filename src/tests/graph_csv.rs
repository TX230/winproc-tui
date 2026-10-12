use crate::App;
use crate::app::graph_csv::*;
use crate::app::{DetailsMetric, GraphSlot};
use crate::model::history::SystemMetric;
use crate::{
    app::{AbComparison, AbComparisonPoint, FocusedPanel, GraphDisplayMode},
    model::{GpuAdapterId, GpuAdapterSample, history::ProcessIdentity},
    tests::support::{assign_private_graph, make_test_app, render_app_to_text},
};
use chrono::Local;
use chrono::TimeZone;
use crossterm::event::{KeyCode, KeyEvent};
use std::io::Write;
fn fixture() -> App {
    let mut app = make_test_app(1, 10);
    app.snapshot.captured_at = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
    app.snapshot.processes[0].name = "a,\"b.exe".into();
    app.snapshot.processes[0].start_time = Some(12);
    app.snapshot.processes[0].private_bytes = Some(9_007_199_254_740_993);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    assign_private_graph(&mut app);
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    app
}
#[test]
fn csv_exact_integer_union_gaps_order_and_lifetimes() {
    let mut app = fixture();
    let time = app.snapshot.captured_at;
    app.snapshot.captured_at += chrono::Duration::milliseconds(1);
    app.snapshot.gpu_adapters = vec![GpuAdapterSample {
        id: GpuAdapterId { high: 1, low: 2 },
        dedicated_used: Some(99),
        ..Default::default()
    }];
    app.system_history.record_snapshot(&app.snapshot);
    let mut export = app.csv_snapshot(CsvRange::All).unwrap();
    export.sources.insert(
        0,
        GraphSlot::gpu(
            GpuAdapterId { high: 1, low: 2 },
            "gpu",
            SystemMetric::GpuDedicated,
        ),
    );
    let mut other = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    other.start_time = Some(13);
    export
        .sources
        .push(GraphSlot::process(other, DetailsMetric::Private));
    let rows = export.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[&time], ["", "9007199254740993", ""]);
    assert_eq!(rows[&app.snapshot.captured_at], ["99", "", ""]);
    let mut bytes = vec![];
    export.write_to(&mut bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("a,\"\"b.exe"));
    assert!(text.contains("start_unix=12"));
    assert!(text.contains("start_unix=13"));
    assert!(text.contains("adapter=00000001:00000002"));
    assert!(text.contains(".001000000+"));
    assert!(!text.contains('\r'));
}
#[test]
fn csv_snapshot_is_immutable_and_paused_history_is_used() {
    let mut app = fixture();
    let time = app.snapshot.captured_at;
    app.toggle_display_pause();
    app.snapshot.processes[0].private_bytes = Some(1);
    app.process_history.record_snapshot(
        time + chrono::Duration::seconds(1),
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    let frozen = app.csv_snapshot(CsvRange::All).unwrap();
    assert_eq!(frozen.rows().len(), 1);
    app.toggle_display_pause();
    assert_eq!(app.csv_snapshot(CsvRange::All).unwrap().rows().len(), 2);
    assert_eq!(frozen.rows().len(), 1);
}
#[test]
fn csv_ranges_are_inclusive_and_unset_ab_is_rejected() {
    let mut app = fixture();
    let time = app.snapshot.captured_at;
    for offset in [5, 10, 15] {
        app.process_history.record_snapshot(
            time + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    assert!(app.csv_snapshot(CsvRange::Ab).is_err());
    app.ab_comparison = Some(AbComparison {
        a: Some(AbComparisonPoint {
            captured_at: time + chrono::Duration::seconds(10),
        }),
        b: Some(AbComparisonPoint {
            captured_at: time + chrono::Duration::seconds(5),
        }),
    });
    assert_eq!(app.csv_snapshot(CsvRange::Ab).unwrap().rows().len(), 2);
    app.graph_show_all_samples = false;
    app.graph_time_offset_seconds = 5;
    app.graph_time_span_seconds = 5;
    assert_eq!(app.csv_snapshot(CsvRange::Visible).unwrap().rows().len(), 2);
}
#[test]
fn csv_does_not_overwrite_and_reports_write_failures() {
    let app = fixture();
    let export = app.csv_snapshot(CsvRange::All).unwrap();
    let path = std::env::temp_dir().join(format!(
        "winproc-csv-{}-{}.csv",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap()
    ));
    assert_eq!(export.write_file(&path).unwrap(), 1);
    let original = std::fs::read(&path).unwrap();
    assert!(export.write_file(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(export.write_file(&path.join("missing.csv")).is_err());
    std::fs::remove_file(&path).unwrap();
    struct Fail;
    impl Write for Fail {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(export.write_to(&mut Fail).is_err());
}
#[test]
fn csv_dialog_keyboard_and_rendering() {
    let mut app = fixture();
    app.focused_panel = FocusedPanel::DetailsSamples;
    app.on_key(KeyEvent::new(
        KeyCode::Char('e'),
        crossterm::event::KeyModifiers::CONTROL,
    ))
    .unwrap();
    assert!(app.has_modal_focus());
    let destination = std::path::PathBuf::from(&app.graph_csv_dialog.as_ref().unwrap().path);
    assert!(destination.is_absolute());
    assert_eq!(
        destination.parent().unwrap(),
        std::env::current_dir().unwrap()
    );
    for (w, h) in [(60, 18), (80, 24), (160, 40)] {
        let text = render_app_to_text(&app, w, h);
        assert!(text.contains("Export Graphs to CSV"));
        assert!(text.contains("Export time series from all Graph slots to one CSV file."));
        assert!(text.contains("Destination file (full path):"));
        assert!(text.contains("All retained history"));
        assert!(text.contains("Enter Export"));
        assert!(text.contains("Esc Cancel"));
    }
    app.graph_csv_key(KeyEvent::new(
        KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.graph_csv_key(KeyEvent::new(
        KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(
        app.graph_csv_dialog.as_ref().unwrap().range,
        CsvRange::Visible
    );
    app.graph_csv_key(KeyEvent::new(
        KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.graph_csv_dialog.as_ref().unwrap().range, CsvRange::All);
    app.graph_csv_key(KeyEvent::new(
        KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.graph_csv_dialog.is_none());
}

#[test]
fn csv_worker_reports_success_and_existing_destination_in_dialog() {
    let mut app = fixture();
    let path = std::env::temp_dir().join(format!(
        "winproc-csv-worker-{}-{}",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap()
    ));
    let path = path.join("日本語 folder").join("graphs.csv");
    for attempt in 0..2 {
        app.open_graph_csv();
        app.graph_csv_key(KeyEvent::new(
            KeyCode::Char('u'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in path.to_string_lossy().chars() {
            app.graph_csv_key(KeyEvent::new(
                KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        app.graph_csv_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.poll_graph_csv() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if attempt == 0 {
            assert!(app.graph_csv_dialog.is_none());
            assert!(app.status.starts_with("Exported 1 CSV rows:"));
        } else {
            assert!(!app.graph_csv_dialog.as_ref().unwrap().busy());
            assert!(
                app.graph_csv_dialog
                    .as_ref()
                    .unwrap()
                    .error
                    .contains("File exists")
            );
        }
    }
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("9007199254740993")
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(path.parent().unwrap()).unwrap();
    std::fs::remove_dir(path.parent().unwrap().parent().unwrap()).unwrap();
}

#[test]
fn csv_dialog_mouse_targets_follow_intro_rows() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = fixture();
    app.open_graph_csv();
    for (width, height) in [(60, 18), (80, 24), (160, 40)] {
        let text = render_app_to_text(&app, width, height);
        let lines: Vec<_> = text.lines().collect();
        let range_row = lines
            .iter()
            .position(|line| line.contains("Range:"))
            .unwrap();
        let path_row = lines
            .iter()
            .position(|line| line.contains("Destination file"))
            .unwrap()
            + 1;
        let column = lines[range_row].chars().position(|ch| ch == 'R').unwrap() as u16;
        for (row, focused) in [(range_row, true), (path_row, false)] {
            crate::ui::graph_csv::mouse(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row: row as u16,
                    modifiers: KeyModifiers::NONE,
                },
                ratatui::layout::Rect::new(0, 0, width, height),
            );
            assert_eq!(
                app.graph_csv_dialog.as_ref().unwrap().range_focused,
                focused
            );
        }
    }
}

#[test]
fn csv_path_cursor_keys_match_recording_without_inserting_characters() {
    use crossterm::event::KeyModifiers;
    let mut csv = fixture();
    csv.open_graph_csv();
    let path = r"C:\日本語 folder\graphs.csv";
    let d = csv.graph_csv_dialog.as_mut().unwrap();
    d.path = path.into();
    d.cursor = path.len();
    let mut recording = fixture();
    recording.show_recording_path_dialog = true;
    recording.recording_path_draft = path.into();
    recording.recording_path_cursor = path.len();
    for code in [
        KeyCode::Home,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Char('X'),
        KeyCode::Delete,
        KeyCode::Backspace,
        KeyCode::End,
        KeyCode::Right,
        KeyCode::Left,
    ] {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        let previous_path = csv.graph_csv_dialog.as_ref().unwrap().path.clone();
        csv.on_key(key).unwrap();
        recording.on_key(key).unwrap();
        let d = csv.graph_csv_dialog.as_ref().unwrap();
        assert_eq!(d.path, recording.recording_path_draft);
        assert_eq!(d.cursor, recording.recording_path_cursor);
        if matches!(
            code,
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
        ) {
            assert_eq!(d.path, previous_path);
        }
    }
}

#[test]
fn csv_ctrl_space_matches_recording_and_preserves_filename() {
    use crossterm::event::KeyModifiers;
    let root = std::env::temp_dir().join(format!(
        "winproc-csv-completion-{}-{}",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap()
    ));
    for name in ["日本語 one", "日本語 two"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let prefix = root.join("日本").display().to_string();
    let path = format!(r"{prefix}\graphs.csv");
    let mut csv = fixture();
    csv.open_graph_csv();
    let d = csv.graph_csv_dialog.as_mut().unwrap();
    d.path = path.clone();
    d.cursor = prefix.len();
    let mut recording = fixture();
    recording.show_recording_path_dialog = true;
    recording.recording_path_draft = path;
    recording.recording_path_cursor = prefix.len();
    let complete = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
    let mut results = Vec::new();
    for _ in 0..3 {
        csv.on_key(complete).unwrap();
        recording.on_key(complete).unwrap();
        let d = csv.graph_csv_dialog.as_ref().unwrap();
        assert_eq!(d.path, recording.recording_path_draft);
        assert_eq!(d.cursor, recording.recording_path_cursor);
        assert!(d.path.ends_with(r"\graphs.csv"));
        results.push(d.path.clone());
    }
    assert_ne!(results[0], results[1]);
    assert_eq!(results[0], results[2]);
    csv.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    csv.on_key(complete).unwrap();
    assert_eq!(csv.graph_csv_dialog.as_ref().unwrap().range, CsvRange::All);
    for name in ["日本語 one", "日本語 two"] {
        std::fs::remove_dir(root.join(name)).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn csv_path_draws_terminal_cursor_without_underscore() {
    use ratatui::{
        Terminal,
        backend::{Backend, TestBackend},
    };
    let mut app = fixture();
    app.open_graph_csv();
    let d = app.graph_csv_dialog.as_mut().unwrap();
    d.path = r"C:\日本語\graphs.csv".into();
    d.cursor = r"C:\日".len();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| crate::ui::draw(frame, &app)).unwrap();
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(cursor.x, cursor.y)].symbol(), "本");
    let text = render_app_to_text(&app, 80, 24);
    assert!(text.contains("graphs.csv"));
    assert!(!text.lines().nth(cursor.y as usize).unwrap().contains('_'));
    assert!(text.contains("Ctrl+Space Complete path"));
    assert!(text.contains("←/→ Cursor"));
}

#[test]
fn csv_ctrl_e_defaults_to_all_regardless_of_ab_and_rechecks_on_reopen() {
    use crossterm::event::KeyModifiers;
    let mut app = fixture();
    app.focused_panel = FocusedPanel::DetailsSamples;
    let time = app.snapshot.captured_at;
    let a = AbComparisonPoint { captured_at: time };
    let b = AbComparisonPoint {
        captured_at: time - chrono::Duration::seconds(5),
    };
    for (comparison, expected) in [
        (None, CsvRange::All),
        (
            Some(AbComparison {
                a: Some(a),
                b: None,
            }),
            CsvRange::All,
        ),
        (
            Some(AbComparison {
                a: None,
                b: Some(b),
            }),
            CsvRange::All,
        ),
        (
            Some(AbComparison {
                a: Some(a),
                b: Some(b),
            }),
            CsvRange::All,
        ),
        (
            Some(AbComparison {
                a: Some(b),
                b: Some(a),
            }),
            CsvRange::All,
        ),
        (None, CsvRange::All),
    ] {
        app.ab_comparison = comparison;
        app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(app.graph_csv_dialog.as_ref().unwrap().range, expected);
        let text = render_app_to_text(&app, 80, 24);
        assert!(text.contains(&format!("Range: {}", expected.label())));
        app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_ne!(app.graph_csv_dialog.as_ref().unwrap().range, expected);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
    }
}

#[test]
fn csv_opens_from_every_main_panel_and_empty_graphs_cannot_export() {
    use crossterm::event::KeyModifiers;
    for empty in [false, true] {
        for panel in [
            FocusedPanel::System,
            FocusedPanel::SystemActivity,
            FocusedPanel::Cpu,
            FocusedPanel::Processes,
            FocusedPanel::DetailsGraph,
            FocusedPanel::DetailsSamples,
        ] {
            let mut app = fixture();
            if empty {
                app.graph_entries.clear();
                app.active_graph_id = None;
            }
            app.focused_panel = panel;
            app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
                .unwrap();
            assert!(app.graph_csv_dialog.is_some(), "{panel:?}");
            for (width, height) in [(60, 18), (80, 24), (160, 40)] {
                let text = render_app_to_text(&app, width, height);
                if empty {
                    assert!(
                        text.replace(' ', "")
                            .contains("PleaseaddmetricstoGraphslots.")
                    );
                    for hidden in [
                        "Export time series",
                        "Destination file",
                        "Range:",
                        "A-B unavailable",
                        "Enter Export",
                        "Esc Cancel",
                        "Tab Field",
                        "Ctrl+Space",
                        "graphs-",
                    ] {
                        assert!(!text.contains(hidden), "{hidden}");
                    }
                } else {
                    assert!(text.contains("Export time series"));
                    assert!(text.contains("Destination file"));
                }
            }
            if empty {
                let path = app.graph_csv_dialog.as_ref().unwrap().path.clone();
                for code in [KeyCode::Tab, KeyCode::Right, KeyCode::Char('x')] {
                    app.on_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
                }
                let d = app.graph_csv_dialog.as_ref().unwrap();
                assert_eq!(d.path, path);
                assert!(!d.range_focused);
                assert!(!d.busy());
                app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                    .unwrap();
                assert!(app.graph_csv_dialog.is_none());
                app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
                    .unwrap();
                app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                    .unwrap();
                assert!(app.graph_csv_dialog.is_none());
            }
        }
    }
}

#[test]
fn csv_wider_dialog_displays_long_path_and_still_fits_small_screens() {
    let mut app = fixture();
    app.open_graph_csv();
    let path = format!("C:/exports/{}/graphs.csv", "a".repeat(60));
    let d = app.graph_csv_dialog.as_mut().unwrap();
    d.path = path.clone();
    d.cursor = path.len();
    assert!(path.len() > 72);
    assert!(render_app_to_text(&app, 160, 40).contains(&path));
    let small = render_app_to_text(&app, 60, 18);
    assert!(small.contains("graphs.csv"));
    assert!(small.contains("Ctrl+Space Complete path"));
}

#[test]
fn csv_range_ignores_vertical_arrows_but_keeps_horizontal_navigation() {
    use crossterm::event::KeyModifiers;
    let mut app = fixture();
    let point = AbComparisonPoint {
        captured_at: app.snapshot.captured_at,
    };
    app.ab_comparison = Some(AbComparison {
        a: Some(point),
        b: Some(point),
    });
    app.open_graph_csv();
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    for range in [CsvRange::All, CsvRange::Visible, CsvRange::Ab] {
        app.graph_csv_dialog.as_mut().unwrap().range = range;
        for key in [KeyCode::Up, KeyCode::Down] {
            app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();
            let d = app.graph_csv_dialog.as_ref().unwrap();
            assert_eq!(d.range, range);
            assert!(d.range_focused);
        }
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
            .unwrap();
        assert_ne!(app.graph_csv_dialog.as_ref().unwrap().range, range);
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.graph_csv_dialog.as_ref().unwrap().range, range);
    }
}
