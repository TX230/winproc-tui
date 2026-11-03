use super::support::*;
use crate::{
    app::{
        AbComparison, AbComparisonPoint, App, FocusedPanel, GraphDisplayMode, graph_csv::CsvRange,
    },
    mcp::operations::{GraphTargets, Operations},
    model::{ProcessHistory, ProcessIdentity, SystemHistory},
};
use chrono::{Local, TimeZone};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn setup() -> App {
    let mut app = make_test_app(1, 10);
    app.mcp_started = true;
    app.mcp_connections_disabled = false;
    app.process_history = ProcessHistory::default();
    app.system_history = SystemHistory::default();
    app.snapshot.captured_at = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
    app.snapshot.processes[0].start_time = Some(12);
    for value in [Some(9_007_199_254_740_993), None, Some(3)] {
        app.snapshot.processes[0].private_bytes = value;
        app.snapshot.committed_memory = Some(9_007_199_254_740_993);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
        app.snapshot.captured_at += chrono::Duration::seconds(1);
    }
    app.snapshot.captured_at -= chrono::Duration::seconds(1);
    app.poll_mcp();
    app
}
fn submit(app: &mut App, method: &str, mut args: Value, targets: GraphTargets) -> String {
    if args.get("operation_id").is_none() {
        args["operation_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    let id = args["operation_id"].as_str().unwrap().to_owned();
    app.mcp_state
        .operations
        .lock()
        .unwrap()
        .submit_graph("test", method, &args, targets);
    app.poll_mcp();
    id
}
fn finish(app: &mut App, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.poll_mcp();
        let result = app.mcp_state.operations.lock().unwrap().get(id);
        if matches!(result["state"].as_str(), Some("succeeded" | "failed")) {
            return result;
        }
        assert!(Instant::now() < deadline, "{result}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn action(app: &mut App, method: &str, args: Value) -> Value {
    let id = submit(app, method, args, GraphTargets::default());
    finish(app, &id)
}
fn targets(app: &App, scoped: bool) -> GraphTargets {
    GraphTargets {
        identities: std::collections::HashMap::from([(
            "process".into(),
            ProcessIdentity::from_row(&app.snapshot.processes[0]),
        )]),
        revision: scoped.then(|| app.mcp_test_revision()),
    }
}
struct Output(PathBuf);
impl Output {
    fn new() -> Self {
        Self(
            std::env::temp_dir()
                .join(format!("winproc-mcp-graphs-{}", uuid::Uuid::new_v4()))
                .join("missing")
                .join("nested"),
        )
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let root = self.0.parent().unwrap().parent().unwrap();
        if root.exists() {
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn mcp_graph_append_preserves_workspace_and_rejects_stale_revisions() {
    let mut app = setup();
    assign_private_graph(&mut app);
    app.poll_mcp();
    app.focused_panel = FocusedPanel::Processes;
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    app.graph_time_offset_seconds = 1;
    app.graph_time_span_seconds = 30;
    let original = app.graph_entries[0].clone();
    let active = app.active_graph_id;
    let show = app.show_details;
    let result = action(
        &mut app,
        "winproc_add_graphs",
        json!({"operation_id":"append","metrics":[{"metric_id":"committed_bytes"},{"metric_id":"committed_bytes"},{"metric_id":"cpu_average_percent"}]}),
    );
    assert_eq!(result["state"], "succeeded", "{result}");
    assert_eq!(app.graph_entries.len(), 3);
    assert_eq!(app.graph_entries[0].id, original.id);
    assert_eq!(app.graph_entries[0].source, original.source);
    assert_eq!(app.graph_entries[0].display_mode, original.display_mode);
    assert_eq!(app.active_graph_id, active);
    assert_eq!(app.focused_panel, FocusedPanel::Processes);
    assert_eq!(app.show_details, show);
    assert_eq!(app.graph_time_offset_seconds, 1);
    assert_eq!(app.graph_time_span_seconds, 30);
    let old_id = result["result"]["graphs"][0]["graph_id"].clone();
    let stale = app.mcp_test_revision();
    add_test_graph(&mut app, 1);
    app.poll_mcp();
    let conflict = action(
        &mut app,
        "winproc_add_graphs",
        json!({"expected_revision":stale,"metrics":[{"metric_id":"process_count"}]}),
    );
    assert_eq!(conflict["result"]["error"]["code"], "STATE_CONFLICT");
    assert_eq!(app.graph_entries.len(), 4);
    assert_eq!(
        app.mcp_test_view().control["graphs"]["graphs"][0]["graph_id"],
        old_id
    );
    assert!(app.recording_session.is_none());
}
#[test]
fn mcp_graph_batch_validation_is_atomic_and_capacity_counts_unique_sources() {
    let mut app = setup();
    let invalid = action(
        &mut app,
        "winproc_add_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"},{"metric_id":"private_bytes"}]}),
    );
    assert_eq!(invalid["result"]["error"]["code"], "INVALID_ARGUMENT");
    assert!(app.graph_entries.is_empty());
    for i in 0..15 {
        add_test_graph(&mut app, i);
    }
    let overflow = action(
        &mut app,
        "winproc_add_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"},{"metric_id":"cpu_average_percent"}]}),
    );
    assert_eq!(overflow["result"]["error"]["code"], "RESOURCE_LIMIT");
    assert_eq!(app.graph_entries.len(), 15);
    let last = action(
        &mut app,
        "winproc_add_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"},{"metric_id":"committed_bytes"}]}),
    );
    assert_eq!(last["state"], "succeeded");
    assert_eq!(app.graph_entries.len(), 16);
    let duplicate = action(
        &mut app,
        "winproc_add_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"}]}),
    );
    assert_eq!(duplicate["state"], "succeeded");
}
#[test]
fn mcp_graph_process_targets_bind_lifetime_and_display_revision() {
    let mut app = setup();
    let mut unknown = targets(&app, false);
    unknown.identities.get_mut("process").unwrap().start_time = None;
    let spec = json!({"metrics":[{"metric_id":"private_bytes","process_ref":"process"}]});
    let id = submit(&mut app, "winproc_add_graphs", spec.clone(), unknown);
    assert_eq!(
        finish(&mut app, &id)["result"]["error"]["code"],
        "IDENTITY_UNVERIFIABLE"
    );
    let mut reused = targets(&app, false);
    reused.identities.get_mut("process").unwrap().start_time = Some(13);
    let id = submit(&mut app, "winproc_add_graphs", spec.clone(), reused);
    assert_eq!(
        finish(&mut app, &id)["result"]["error"]["code"],
        "REFERENCE_INVALID"
    );
    let scoped = targets(&app, true);
    app.toggle_display_pause();
    app.poll_mcp();
    let id = submit(&mut app, "winproc_add_graphs", spec.clone(), scoped);
    assert_eq!(
        finish(&mut app, &id)["result"]["error"]["code"],
        "STATE_CONFLICT"
    );
    let scoped = targets(&app, true);
    app.snapshot.processes[0].start_time = Some(99);
    let id = submit(&mut app, "winproc_add_graphs", spec, scoped);
    assert_eq!(finish(&mut app, &id)["state"], "succeeded");
    assert_eq!(
        app.graph_entries[0]
            .source
            .process_identity()
            .unwrap()
            .start_time,
        Some(12)
    );
}
#[test]
fn mcp_graph_export_combines_addition_with_exact_tui_csv_and_is_idempotent() {
    let mut app = setup();
    assign_private_graph(&mut app);
    app.graph_entries[0].display_mode = GraphDisplayMode::MovingAverage5;
    let output = Output::new();
    let args = json!({"operation_id":"export-once","metrics":[{"metric_id":"committed_bytes"}],"output_directory":output.0,"filename":"exact.csv"});
    let result = action(&mut app, "winproc_export_graphs", args.clone());
    assert_eq!(result["state"], "succeeded", "{result}");
    assert_eq!(result["result"]["rows"], 3);
    let path = PathBuf::from(result["result"]["output_path"].as_str().unwrap());
    assert_eq!(path, output.0.join("exact.csv"));
    let bytes = std::fs::read(&path).unwrap();
    let mut expected = Vec::new();
    app.csv_snapshot(CsvRange::All)
        .unwrap()
        .write_to(&mut expected)
        .unwrap();
    assert_eq!(bytes, expected);
    assert!(
        String::from_utf8(bytes.clone())
            .unwrap()
            .contains("9007199254740993")
    );
    assert_eq!(action(&mut app, "winproc_export_graphs", args), result);
    assert_eq!(app.graph_entries.len(), 2);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let collision = action(
        &mut app,
        "winproc_export_graphs",
        json!({"output_directory":output.0,"filename":"exact.csv"}),
    );
    assert_eq!(collision["result"]["error"]["code"], "EXPORT_FAILED");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn mcp_graph_export_snapshots_paused_and_log_data_with_inclusive_ranges() {
    let mut app = setup();
    assign_private_graph(&mut app);
    app.toggle_display_pause();
    let output = Output::new();
    let final_time = app.display_snapshot().captured_at;
    app.ab_comparison = Some(AbComparison {
        a: Some(AbComparisonPoint {
            captured_at: final_time,
        }),
        b: Some(AbComparisonPoint {
            captured_at: final_time - chrono::Duration::seconds(1),
        }),
    });
    app.snapshot.processes[0].private_bytes = Some(1);
    app.process_history.record_snapshot(
        final_time + chrono::Duration::seconds(1),
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    for range in ["all", "ab", "visible"] {
        let expected_range = match range {
            "ab" => CsvRange::Ab,
            "visible" => CsvRange::Visible,
            _ => CsvRange::All,
        };
        let mut expected = Vec::new();
        let count = app
            .csv_snapshot(expected_range)
            .unwrap()
            .write_to(&mut expected)
            .unwrap();
        let result = action(
            &mut app,
            "winproc_export_graphs",
            json!({"output_directory":output.0,"range":range}),
        );
        assert_eq!(result["state"], "succeeded", "{result}");
        assert_eq!(result["result"]["rows"], count);
        assert_eq!(
            std::fs::read(result["result"]["output_path"].as_str().unwrap()).unwrap(),
            expected
        );
    }
    app.log_view_display = app.paused_display.take();
    app.log_view_path = Some("fixture.jsonl".into());
    app.poll_mcp();
    assert_eq!(app.mcp_test_view().control["graphs"]["source"], "log");
    let spec = json!({"metrics":[{"metric_id":"cpu_percent","process_ref":"process"}]});
    let live = targets(&app, false);
    let id = submit(&mut app, "winproc_add_graphs", spec.clone(), live);
    assert_eq!(
        finish(&mut app, &id)["result"]["error"]["code"],
        "ACTIVITY_CONFLICT"
    );
    let scoped = GraphTargets {
        identities: std::collections::HashMap::from([(
            "process".into(),
            ProcessIdentity::from_row(&app.display_snapshot().processes[0]),
        )]),
        revision: Some(app.mcp_test_revision()),
    };
    let id = submit(&mut app, "winproc_add_graphs", spec, scoped);
    assert_eq!(finish(&mut app, &id)["state"], "succeeded");
    let mut expected = Vec::new();
    app.csv_snapshot(CsvRange::All)
        .unwrap()
        .write_to(&mut expected)
        .unwrap();
    let result = action(
        &mut app,
        "winproc_export_graphs",
        json!({"output_directory":output.0}),
    );
    assert_eq!(result["result"]["workspace"]["source"], "log");
    assert_eq!(
        std::fs::read(result["result"]["output_path"].as_str().unwrap()).unwrap(),
        expected
    );
}
#[test]
fn mcp_graph_export_validation_rolls_back_additions_and_write_failure_keeps_them() {
    let mut app = setup();
    let output = Output::new();
    for filename in [
        "../escape.csv",
        "CON.csv",
        "LPT1.csv",
        "wrong.txt",
        "bad:stream.csv",
    ] {
        let result = action(
            &mut app,
            "winproc_export_graphs",
            json!({"metrics":[{"metric_id":"committed_bytes"}],"output_directory":output.0,"filename":filename}),
        );
        assert_eq!(
            result["result"]["error"]["code"], "INVALID_ARGUMENT",
            "{filename}: {result}"
        );
        assert!(app.graph_entries.is_empty());
    }
    let unset = action(
        &mut app,
        "winproc_export_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"}],"output_directory":output.0,"range":"ab"}),
    );
    assert_eq!(unset["state"], "failed");
    assert!(app.graph_entries.is_empty());
    assert!(app.active_graph_id.is_none());
    std::fs::create_dir_all(&output.0).unwrap();
    let file = output.0.join("file");
    std::fs::write(&file, "keep").unwrap();
    let failure = action(
        &mut app,
        "winproc_export_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"}],"output_directory":file.join("directory")}),
    );
    assert_eq!(failure["result"]["error"]["code"], "EXPORT_FAILED");
    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "keep");
}
#[test]
fn mcp_graph_accepted_export_survives_disconnect_and_rejects_parallel_mutation() {
    let mut app = setup();
    let output = Output::new();
    let id = submit(
        &mut app,
        "winproc_export_graphs",
        json!({"metrics":[{"metric_id":"committed_bytes"}],"output_directory":output.0}),
        GraphTargets::default(),
    );
    {
        let mut ops = app.mcp_state.operations.lock().unwrap();
        assert_eq!(ops.get(&id)["state"], "running");
        ops.cancel_connection(Some("test"));
        assert_eq!(ops.get(&id)["state"], "running");
        assert_eq!(
            ops.submit(
                "other",
                "winproc_add_graphs",
                &json!({"operation_id":"parallel","metrics":[{"metric_id":"cpu_average_percent"}]}),
                None
            )["error"]["code"],
            "BUSY"
        );
    }
    app.mcp_connections_disabled = true;
    assert_eq!(finish(&mut app, &id)["state"], "succeeded");
    let mut ops = Operations::default();
    ops.submit(
        "test",
        "winproc_export_graphs",
        &json!({"operation_id":"pending"}),
        None,
    );
    ops.cancel_connection(None);
    assert_eq!(ops.get("pending")["result"]["error"]["code"], "CANCELLED");
}
