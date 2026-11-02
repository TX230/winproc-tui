//! Real Windows processes and production sampling/application/recording paths.
use super::support::{make_test_app, render_app_to_text};
use crate::{
    app::{App, ProcessViewMode},
    model::ProcessIdentity,
};
use std::{
    collections::HashSet,
    fs,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    dir: PathBuf,
    children: Vec<Child>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.dir.join("stop-all"), b"stop");
        for child in &mut self.children {
            let _ = child.wait();
        }
        // Orphaned descendants share the stop file and acknowledge normal exit.
        for role in ["worker", "leaf", "deep", "late"] {
            let until = Instant::now() + Duration::from_secs(5);
            while self.dir.join(format!("{role}.pid")).exists()
                && !self.dir.join(format!("{role}.done")).exists()
                && Instant::now() < until
            {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
fn wait_file(path: &std::path::Path) -> String {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && !text.is_empty()
        {
            return text;
        }
        assert!(
            Instant::now() < until,
            "missing fixture file: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}
fn capture(app: &mut App) {
    app.request_sample().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    while app.sampling_in_progress {
        app.poll_sample_results().unwrap();
        assert!(Instant::now() < until, "sampling timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
fn tracked_pids(app: &App) -> HashSet<u32> {
    app.snapshot
        .processes
        .iter()
        .filter(|p| app.is_live_tracked_process(p) && p.name.starts_with("w203"))
        .map(|p| p.pid)
        .collect()
}
#[test]
#[ignore = "native Windows process tree; approximately 140 seconds; requires rustc"]
fn native_descendant_tracking_system() {
    let prefix = format!("w203{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("notes")
        .join(format!("descendant-system-{prefix}"));
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("{prefix}-root.exe"));
    let compiled = Command::new("rustc")
        .args(["--edition=2024", "scripts/test-descendant-process.rs", "-o"])
        .arg(&exe)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    for name in ["worker", "leaf", "deep", "late"] {
        fs::copy(&exe, dir.join(format!("{prefix}-{name}.exe"))).unwrap();
    }
    let mut fixture = Fixture {
        dir: dir.clone(),
        children: vec![],
    };
    for (image, role) in [("root", "root"), ("worker", "unrelated")] {
        fixture.children.push(
            Command::new(dir.join(format!("{prefix}-{image}.exe")))
                .args([dir.to_str().unwrap(), role, &prefix])
                .creation_flags(0x08000000)
                .spawn()
                .unwrap(),
        );
    }
    let pid = |role: &str| {
        wait_file(&dir.join(format!("{role}.pid")))
            .parse::<u32>()
            .unwrap()
    };
    let root_pid = pid("root");
    let worker_pid = pid("worker");
    let leaf_pid = pid("leaf");
    let deep_pid = pid("deep");
    let unrelated_pid = pid("unrelated");
    let mut runtime = make_test_app(0, 10).runtime.clone();
    let root_name = format!("{prefix}-root.exe");
    runtime.process_filters = vec![root_name.clone()];
    runtime.initial_tracked_only = true;
    runtime.initial_mcp_connections_disabled = true;
    let mut app = App::new(runtime).unwrap();
    capture(&mut app);
    fs::write(
        dir.join("initial-processes.txt"),
        format!(
            "{:#?}",
            app.snapshot
                .processes
                .iter()
                .filter(|p| app.is_live_tracked_process(p) || p.name.starts_with("w203"))
                .collect::<Vec<_>>()
        ),
    )
    .unwrap();
    assert_eq!(
        tracked_pids(&app),
        HashSet::from([root_pid, worker_pid, leaf_pid, deep_pid])
    );
    let worker = app
        .snapshot
        .processes
        .iter()
        .find(|p| p.pid == worker_pid)
        .unwrap();
    assert_eq!(worker.parent_pid, Some(root_pid));
    let worker_id = ProcessIdentity::from_row(worker);
    assert!(!tracked_pids(&app).contains(&unrelated_pid));
    let expected: u64 = app
        .snapshot
        .processes
        .iter()
        .filter(|p| app.is_live_tracked_process(p))
        .filter_map(|p| p.private_bytes)
        .sum();
    assert!(expected > 0);
    assert_eq!(
        app.tracked_total_row.as_ref().unwrap().private_bytes,
        Some(expected)
    );
    app.process_view_mode = ProcessViewMode::Tree;
    app.rebuild_visible_process_cache();
    fs::write(dir.join("tree.txt"), render_app_to_text(&app, 160, 45)).unwrap();
    assert_eq!(
        app.mcp_test_view().tracked_identities.len(),
        app.snapshot
            .processes
            .iter()
            .filter(|p| app.is_live_tracked_process(p))
            .count()
    );
    eprintln!(
        "PASS native initial ancestry, unrelated-name exclusion, totals, Tree rendering and MCP view"
    );

    fs::write(dir.join("exit-root"), b"exit").unwrap();
    fixture.children[0].wait().unwrap();
    capture(&mut app);
    assert_eq!(
        tracked_pids(&app),
        HashSet::from([worker_pid, leaf_pid, deep_pid])
    );
    app.toggle_display_pause();
    app.recording_interval_index = 2; // 5-second aggregation
    let log = dir.join("descendants.log");
    app.recording_path_draft = log.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    fs::write(dir.join("start-later"), b"start").unwrap();
    let late_pid = pid("late");
    for n in 0..125 {
        thread::sleep(Duration::from_secs(1));
        capture(&mut app);
        assert_eq!(
            tracked_pids(&app),
            HashSet::from([worker_pid, leaf_pid, deep_pid, late_pid])
        );
        if n % 30 == 0 {
            eprintln!("native recording/history sample {} / 125", n + 1);
        }
    }
    assert!(app.process_history.sample_count_for(&worker_id) > 120);
    assert!(!app.visible_processes().iter().any(|p| p.pid == late_pid));
    app.stop_recording().unwrap();
    app.toggle_display_pause();
    assert!(app.visible_processes().iter().any(|p| p.pid == late_pid));
    let pending = app.prepare_tracked_list_switch(vec![]);
    assert!(pending.discarded_sample_count > 0);
    let discarded = pending.discarded_sample_count;
    let loaded = crate::app::logs::load_log(&log, app.sort).unwrap();
    assert_eq!(
        loaded.summary.tracked_names.as_slice(),
        std::slice::from_ref(&root_name)
    );
    assert_eq!(loaded.interval_seconds, 5);
    assert_eq!(
        loaded
            .snapshot
            .processes
            .iter()
            .filter(|p| p.name.starts_with("w203"))
            .map(|p| p.pid)
            .collect::<HashSet<_>>(),
        HashSet::from([worker_pid, leaf_pid, deep_pid, late_pid])
    );
    assert!(loaded.process_history.sample_count_for(&worker_id) > 20);
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .lines()
            .last()
            .unwrap()
            .starts_with("{\"e\":")
    );
    let logged_count = loaded.snapshot.processes.len();
    app.apply_loaded_log(loaded);
    assert_eq!(app.visible_process_count(), logged_count);
    fs::write(dir.join("log-view.txt"), render_app_to_text(&app, 160, 45)).unwrap();
    app.exit_log_view();
    let overlap =
        app.prepare_tracked_list_switch(vec![root_name.clone(), format!("{prefix}-worker.exe")]);
    app.apply_tracked_list_switch(overlap);
    assert_eq!(tracked_pids(&app).len(), 5);
    let remove_root = app.prepare_tracked_list_switch(vec![format!("{prefix}-worker.exe")]);
    assert_eq!(remove_root.discarded_sample_count, 0);
    app.apply_tracked_list_switch(remove_root);
    assert_eq!(tracked_pids(&app).len(), 5);
    let clear = app.prepare_tracked_list_switch(vec![]);
    app.apply_tracked_list_switch(clear);
    assert_eq!(app.process_history.sample_count_for(&worker_id), 120);
    assert!(tracked_pids(&app).is_empty());
    drop(app);
    drop(fixture);
    for role in ["worker", "leaf", "deep", "late", "unrelated"] {
        assert_eq!(wait_file(&dir.join(format!("{role}.done"))), "done");
    }
    fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","samples":125,"interval_seconds":5,"discarded_samples":discarded,"root_pid":root_pid,"worker_pid":worker_pid,"leaf_pid":leaf_pid,"deep_pid":deep_pid,"late_pid":late_pid,"unrelated_pid":unrelated_pid,"cleanup":"all fixture processes acknowledged exit","limitations":"production sampler and App with TestBackend; no physical terminal or MCP transport; no forced OS PID reuse"})).unwrap()).unwrap();
    eprintln!(
        "PASS parent exit, new descendants, paused recording, long history, aggregated log reload, overlapping roots, history pruning and fixture cleanup; evidence: {}",
        dir.display()
    );
}
