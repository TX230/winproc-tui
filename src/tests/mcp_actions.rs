use super::support::*;
use crate::{app::App, mcp::operations};
use serde_json::{Value, json};
fn setup() -> App {
    let mut app = make_test_app(2, 10);
    app.mcp_started = true;
    app.mcp_connections_disabled = false;
    app.poll_mcp();
    app
}
fn revision(app: &App) -> String {
    app.mcp_state.operations.lock().unwrap().get("never");
    app.mcp_test_revision()
}
fn action(app: &mut App, method: &str, mut args: Value) -> Value {
    if args.get("operation_id").is_none() {
        args["operation_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    if method != "winproc_stop_recording" && args.get("expected_revision").is_none() {
        args["expected_revision"] = json!(revision(app));
    }
    let id = args["operation_id"].as_str().unwrap().to_owned();
    app.mcp_state
        .operations
        .lock()
        .unwrap()
        .submit("test", method, &args, None);
    app.poll_mcp();
    app.mcp_state.operations.lock().unwrap().get(&id)
}
#[test]
fn tracking_actions_are_idempotent_and_detect_tui_changes() {
    let mut app = setup();
    let before = app.snapshot.processes.len();
    let args = json!({"action":"add","process_name":"future.exe","operation_id":"one","expected_revision":revision(&app)});
    let first = action(&mut app, "winproc_update_tracking", args.clone());
    assert_eq!(first["state"], "succeeded");
    let second = action(&mut app, "winproc_update_tracking", args);
    assert_eq!(first, second);
    assert_eq!(
        app.watch_list.iter().filter(|n| *n == "future.exe").count(),
        1
    );
    assert_eq!(app.snapshot.processes.len(), before);
    let stale = revision(&app);
    app.watch_list.push("local.exe".into());
    app.poll_mcp();
    let result = action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"bad.exe","expected_revision":stale}),
    );
    assert_eq!(result["result"]["error"]["code"], "STATE_CONFLICT");
    assert!(!app.watch_list.iter().any(|n| n == "bad.exe"));
}
#[test]
fn profiles_save_rename_confirmation_cancel_and_delete() {
    let mut app = setup();
    let saved = action(
        &mut app,
        "winproc_update_profile",
        json!({"action":"save_as","name":"test-profile"}),
    );
    assert_eq!(saved["state"], "succeeded");
    let profile = saved["result"]["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "test-profile")
        .unwrap();
    let id = profile["profile_id"].clone();
    let rev = revision(&app);
    let renamed = action(
        &mut app,
        "winproc_update_profile",
        json!({"action":"rename","profile_id":id,"expected_profile_revision":rev,"name":"renamed"}),
    );
    assert_eq!(renamed["state"], "succeeded");
    let profile = renamed["result"]["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "renamed")
        .unwrap();
    let id = profile["profile_id"].clone();
    let rev = revision(&app);
    let pending = action(
        &mut app,
        "winproc_update_profile",
        json!({"action":"delete","profile_id":id,"expected_profile_revision":rev}),
    );
    assert_eq!(pending["state"], "pending_confirmation");
    app.mcp_state
        .operations
        .lock()
        .unwrap()
        .cancel_connection(Some("test"));
    app.poll_mcp();
    assert!(app.mcp_state.confirmation.is_none());
    assert!(
        app.runtime
            .saved_investigation_profiles
            .iter()
            .any(|p| p.name == "renamed")
    );
    let rev = revision(&app);
    let pending = action(
        &mut app,
        "winproc_update_profile",
        json!({"action":"delete","profile_id":id,"expected_profile_revision":rev}),
    );
    assert_eq!(pending["state"], "pending_confirmation");
    app.resolve_mcp_confirmation(true);
    assert!(
        !app.runtime
            .saved_investigation_profiles
            .iter()
            .any(|p| p.name == "renamed")
    );
}
#[test]
fn mcp_recording_stop_flushes_without_confirmation_and_replays_safely() {
    let mut app = setup();
    let dir = unique_recording_dir("mcp-session");
    std::fs::create_dir_all(&dir).unwrap();
    app.recording_last_dir = Some(dir.clone());
    app.snapshot.committed_memory = Some(100);
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"absent.exe"}),
    );
    let result = action(
        &mut app,
        "winproc_start_recording",
        json!({"interval_seconds":10}),
    );
    assert_eq!(result["state"], "succeeded");
    let session = result["result"]["recording_session_id"].clone();
    let path = app.recording_session.as_ref().unwrap().path.clone();
    app.mcp_state
        .operations
        .lock()
        .unwrap()
        .cancel_connection(Some("test"));
    assert!(app.recording_session.is_some());
    let rejected = action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"bad.exe"}),
    );
    assert_eq!(rejected["result"]["error"]["code"], "ACTIVITY_CONFLICT");
    app.snapshot.captured_at += chrono::Duration::seconds(1);
    app.snapshot.committed_memory = Some(300);
    app.write_current_recording_frame().unwrap();
    let args = json!({"recording_session_id":session,"operation_id":"stop-once"});
    let stopped = action(&mut app, "winproc_stop_recording", args.clone());
    assert_eq!(stopped["state"], "succeeded");
    assert_eq!(stopped["result"]["stopped"], true);
    assert!(app.recording_session.is_none());
    assert!(app.mcp_state.confirmation.is_none());
    assert!(!app.show_recording_stop_confirmation);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(action(&mut app, "winproc_stop_recording", args), stopped);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let loaded = crate::app::logs::load_log(&path, Default::default()).unwrap();
    assert_eq!(loaded.interval_seconds, 10);
    assert_eq!(loaded.summary.frame_count, 1);
    assert_eq!(loaded.snapshot.committed_memory, Some(200));
    assert_eq!(
        loaded.snapshot.captured_at.timestamp_millis(),
        app.snapshot.captured_at.timestamp_millis()
    );
    assert!(loaded.snapshot.processes.is_empty());
    let last: crate::app::log_format::V3Record = serde_json::from_slice(
        bytes
            .split(|b| *b == b'\n')
            .rfind(|line| !line.is_empty())
            .unwrap(),
    )
    .unwrap();
    let crate::app::log_format::V3Record::End(crate::app::log_format::V3EndRecord(_, reason)) =
        last
    else {
        panic!("Recording must finish with a clean end record");
    };
    assert_eq!(reason, "stopped");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn mcp_recording_stop_never_targets_a_replacement_session() {
    let mut app = setup();
    let dir = unique_recording_dir("mcp-stale-stop");
    std::fs::create_dir_all(&dir).unwrap();
    app.recording_last_dir = Some(dir.clone());
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"absent.exe"}),
    );
    let first = action(
        &mut app,
        "winproc_start_recording",
        json!({"interval_seconds":10}),
    );
    let old = first["result"]["recording_session_id"].clone();
    let args = json!({"recording_session_id":old,"operation_id":"original-stop"});
    let stopped = action(&mut app, "winproc_stop_recording", args.clone());
    assert_eq!(stopped["state"], "succeeded");
    let second = action(
        &mut app,
        "winproc_start_recording",
        json!({"interval_seconds":10}),
    );
    let current = second["result"]["recording_session_id"].clone();
    assert_ne!(old, current);
    let path = app.recording_session.as_ref().unwrap().path.clone();
    app.request_recording_stop();
    assert_eq!(action(&mut app, "winproc_stop_recording", args), stopped);
    assert!(app.show_recording_stop_confirmation);
    for session in [old, json!("unknown-session")] {
        let rejected = action(
            &mut app,
            "winproc_stop_recording",
            json!({"recording_session_id":session}),
        );
        assert_eq!(rejected["result"]["error"]["code"], "STATE_CONFLICT");
        assert_eq!(app.recording_session.as_ref().unwrap().path, path);
        assert!(app.show_recording_stop_confirmation);
        assert!(app.mcp_state.confirmation.is_none());
    }
    let result = action(
        &mut app,
        "winproc_stop_recording",
        json!({"recording_session_id":current}),
    );
    assert_eq!(result["state"], "succeeded");
    assert!(!app.show_recording_stop_confirmation);
    assert!(app.recording_session.is_none());
    for file in std::fs::read_dir(&dir).unwrap() {
        std::fs::remove_file(file.unwrap().path()).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn mcp_recording_stop_reports_write_and_flush_failures_without_confirmation() {
    struct FailWriter {
        write: bool,
    }
    impl std::io::Write for FailWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.write {
                Err(std::io::Error::other("test write failure"))
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("test flush failure"))
        }
    }
    for write in [true, false] {
        let mut app = setup();
        let dir = unique_recording_dir("mcp-stop-error");
        std::fs::create_dir_all(&dir).unwrap();
        app.recording_last_dir = Some(dir.clone());
        action(
            &mut app,
            "winproc_update_tracking",
            json!({"action":"add","process_name":"absent.exe"}),
        );
        let started = action(
            &mut app,
            "winproc_start_recording",
            json!({"interval_seconds":10}),
        );
        let path = app.recording_session.as_ref().unwrap().path.clone();
        app.show_recording_tracking_fixed = true;
        app.replace_recording_writer_for_test(Box::new(FailWriter { write }));
        let result = action(
            &mut app,
            "winproc_stop_recording",
            json!({"recording_session_id":started["result"]["recording_session_id"]}),
        );
        assert_eq!(result["state"], "failed");
        assert_eq!(result["result"]["error"]["code"], "INTERNAL_ERROR");
        assert!(app.mcp_state.confirmation.is_none());
        assert!(!app.show_recording_stop_confirmation);
        assert!(!app.show_recording_tracking_fixed);
        assert!(app.recording_session.is_none());
        assert!(app.recording_error.is_some());
        assert!(path.is_file());
        assert!(render_app_to_text(&app, 100, 45).contains("RECORDING ERROR"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}

#[test]
fn operation_ids_conflict_and_only_one_pending_is_allowed() {
    let mut ops = operations::Operations::default();
    let a = json!({"operation_id":"a"});
    ops.submit("c", "x", &a, None);
    assert_eq!(
        ops.submit("c", "y", &a, None)["error"]["code"],
        "STATE_CONFLICT"
    );
    assert_eq!(
        ops.submit("c", "x", &json!({"operation_id":"b"}), None)["error"]["code"],
        "BUSY"
    );
    ops.cancel_connection(Some("other"));
    assert_eq!(ops.get("a")["state"], "pending");
    ops.cancel_connection(Some("c"));
    assert_eq!(ops.get("a")["state"], "failed");
}

#[test]
fn retained_history_removal_requires_confirmation_and_rechecks_revision() {
    let mut app = setup();
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"retained.exe"}),
    );
    record_tracked_process_history_samples(&mut app, "retained.exe", 300);
    let result = action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"remove","process_name":"retained.exe"}),
    );
    assert_eq!(result["state"], "pending_confirmation");
    assert_eq!(
        selected_process_history_sample_count(&app, "retained.exe"),
        300
    );
    let screen = render_app_to_text(&app, 100, 28);
    assert!(screen.contains("180 older samples"));
    assert!(screen.contains("Enter Confirm"));
    assert!(screen.contains("Esc Cancel"));
    app.watch_list.push("human-edit.exe".into());
    app.resolve_mcp_confirmation(true);
    let result = app
        .mcp_state
        .operations
        .lock()
        .unwrap()
        .get(result["operation_id"].as_str().unwrap());
    assert_eq!(result["result"]["error"]["code"], "STATE_CONFLICT");
    assert_eq!(
        selected_process_history_sample_count(&app, "retained.exe"),
        300
    );
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"remove","process_name":"retained.exe"}),
    );
    app.resolve_mcp_confirmation(true);
    assert_eq!(
        selected_process_history_sample_count(&app, "retained.exe"),
        120
    );
}
#[test]
fn bounded_saved_log_load_preserves_partial_frames_and_rejects_over_budget() {
    use std::sync::{Arc, atomic::AtomicBool};
    let path = unique_recording_path("mcp-bounded");
    let session = json!({"schema_version":2,"record_type":"session","session_id":"fixture","started_at":"2026-05-04T14:30:12Z"});
    let frame = json!({"schema_version":2,"record_type":"frame","session_id":"fixture","captured_at":"2026-05-04T14:30:12Z","processes":[{"pid":42,"name":"test.exe","start_time":100,"metrics":{"private_bytes":9007199254740993u64}}]});
    let base = format!("{session}\n{frame}\n");
    std::fs::write(&path, format!("{base}{{\"broken\":")).unwrap();
    let (loaded, _, partial) =
        crate::app::logs::load_mcp_log(&path, 64 * 1024 * 1024, Arc::new(AtomicBool::new(false)))
            .unwrap();
    assert!(partial);
    assert_eq!(loaded.frame_times.len(), 1);
    assert_eq!(
        loaded.snapshot.processes[0].private_bytes,
        Some(9007199254740993)
    );
    assert!(crate::app::logs::load_mcp_log(&path, 1024, Arc::new(AtomicBool::new(false))).is_err());
    assert!(
        crate::app::logs::load_mcp_log(&path, 64 * 1024 * 1024, Arc::new(AtomicBool::new(true)))
            .is_err()
    );
    std::fs::write(&path, format!("{base}{{}}\n")).unwrap();
    assert!(
        crate::app::logs::load_mcp_log(&path, 64 * 1024 * 1024, Arc::new(AtomicBool::new(false)))
            .is_err()
    );
    let end = json!({"schema_version":2,"record_type":"end","session_id":"fixture","ended_at":"2026-05-04T14:30:13Z"});
    std::fs::write(&path, format!("{base}{end}\n")).unwrap();
    assert!(
        !crate::app::logs::load_mcp_log(&path, 64 * 1024 * 1024, Arc::new(AtomicBool::new(false)))
            .unwrap()
            .2
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn confirmation_timeout_and_disable_never_apply_the_mutation() {
    let mut app = setup();
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"add","process_name":"retained.exe"}),
    );
    record_tracked_process_history_samples(&mut app, "retained.exe", 300);
    let result = action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"remove","process_name":"retained.exe"}),
    );
    app.mcp_state.confirmation.as_mut().unwrap().0.created =
        std::time::Instant::now() - std::time::Duration::from_secs(61);
    app.poll_mcp();
    assert!(app.mcp_state.confirmation.is_none());
    assert_eq!(
        app.mcp_state
            .operations
            .lock()
            .unwrap()
            .get(result["operation_id"].as_str().unwrap())["result"]["error"]["code"],
        "TIMEOUT"
    );
    assert_eq!(
        app.mcp_state
            .operations
            .lock()
            .unwrap()
            .get(result["operation_id"].as_str().unwrap())["state"],
        "failed"
    );
    assert_eq!(
        selected_process_history_sample_count(&app, "retained.exe"),
        300
    );
    action(
        &mut app,
        "winproc_update_tracking",
        json!({"action":"remove","process_name":"retained.exe"}),
    );
    app.mcp_connections_disabled = true;
    app.poll_mcp();
    assert!(app.mcp_state.confirmation.is_none());
    assert_eq!(
        selected_process_history_sample_count(&app, "retained.exe"),
        300
    );
}

#[test]
fn mcp_log_view_keeps_live_host_and_tracking_provenance() {
    let mut app = setup();
    app.snapshot.cpu_name = Some("live-cpu".into());
    app.snapshot.total_memory = 12345;
    app.watch_list = vec![app.snapshot.processes[0].name.clone()];
    let before = app.mcp_test_view();
    let mut recorded = app.snapshot.clone();
    recorded.captured_at -= chrono::Duration::days(10);
    recorded.cpu_name = Some("recorded-cpu".into());
    recorded.total_memory = 67890;
    recorded.processes[0].name = "recorded-only.exe".into();
    let path = std::path::PathBuf::from("recorded.log");
    app.apply_loaded_log(crate::app::logs::LoadedLog {
        path: path.clone(),
        summary: crate::app::logs::LogSummary {
            path,
            schema_version: Some(3),
            session_id: Some("recorded".into()),
            started_at: Some(recorded.captured_at),
            ended_at: Some(recorded.captured_at),
            host: Some("recorded-host".into()),
            tracked_names: vec!["recorded-only.exe".into()],
            interval_seconds: Some(5),
            frame_count: 1,
            error: None,
        },
        frame_times: vec![recorded.captured_at],
        snapshot: recorded,
        process_history: Default::default(),
        system_history: Default::default(),
        tracked_names: vec!["recorded-only.exe".into()],
        interval_seconds: 5,
    });
    let during = app.mcp_test_view();
    assert_eq!(during.activity, "log_view");
    assert_eq!(
        app.display_snapshot().cpu_name.as_deref(),
        Some("recorded-cpu")
    );
    assert_eq!(during.host, before.host);
    assert_eq!(during.snapshot.captured_at, before.snapshot.captured_at);
    assert_eq!(
        during.snapshot.processes[0].name,
        before.snapshot.processes[0].name
    );
    assert_eq!(during.tracked_names, before.tracked_names);
    app.exit_log_view();
    let after = app.mcp_test_view();
    assert_eq!(after.activity, "live");
    assert_eq!(after.host, before.host);
    assert_eq!(after.snapshot.captured_at, before.snapshot.captured_at);
}
