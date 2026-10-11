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
    std::fs::remove_file(&path).unwrap();
    assert!(export.write_file(&path.join("missing.csv")).is_err());
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
    for (w, h) in [(60, 18), (80, 24), (160, 40)] {
        let text = render_app_to_text(&app, w, h);
        assert!(text.contains("Export Graphs to CSV"));
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
        "winproc-csv-worker-{}-{}.csv",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap()
    ));
    for attempt in 0..2 {
        app.open_graph_csv();
        app.graph_csv_dialog.as_mut().unwrap().path = path.to_string_lossy().into_owned();
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
}
