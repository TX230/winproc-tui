use super::support::{make_test_app_with_worker, test_snapshot};
use crate::app::{GraphSlot, SAMPLE_STALE_AFTER_SECONDS, SampleFreshness};
use crate::model;
use crate::model::SystemMetric;
use crate::samplers::gpu::is_filtered_dxgi_adapter;
use crate::samplers::pdh::{
    ProcessInstanceMap, map_process_counter_instances_to_pids, normalize_process_cpu_percent,
    sum_optional_values,
};
use crate::samplers::{CollectSnapshotResult, SampleRequest, SamplingWorker};
use chrono::Local;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use winapi::shared::dxgi::{DXGI_ADAPTER_FLAG_REMOTE, DXGI_ADAPTER_FLAG_SOFTWARE};

#[test]
fn sampling_result_updates_snapshot_and_clamps_selection() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(5, 10, sampling_worker);
    app.select_last_row();
    app.sampling_in_progress = true;
    app.status = "Selected column: PrivBytes".to_string();

    result_tx
        .send(CollectSnapshotResult {
            snapshot: test_snapshot(2),
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert!(!app.sampling_in_progress);
    assert_eq!(app.snapshot.process_count, 2);
    assert_eq!(app.visible_process_count(), 2);
    assert_eq!(app.process_table_state.selected(), Some(1));
    assert_eq!(app.status, "Selected column: PrivBytes");
    assert_eq!(app.process_history.len(), 2);
}

#[test]
fn successful_sample_returns_fresh_after_stale_state() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    app.snapshot.captured_at =
        Local::now() - chrono::Duration::seconds(SAMPLE_STALE_AFTER_SECONDS as i64 + 2);
    assert!(matches!(
        app.sample_freshness(),
        Some(SampleFreshness::Stale { .. })
    ));

    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: test_snapshot(1),
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();

    assert_eq!(app.sample_freshness(), Some(SampleFreshness::Fresh));
}

#[test]
fn sampling_worker_disconnect_keeps_existing_snapshot() {
    let (request_tx, _request_rx) = mpsc::channel::<SampleRequest>();
    let (result_tx, result_rx) = mpsc::channel::<CollectSnapshotResult>();
    drop(result_tx);
    let sampling_worker = SamplingWorker {
        dotnet: Default::default(),
        request_tx,
        result_rx,
        join_handle: None,
    };
    let mut app = make_test_app_with_worker(4, 10, sampling_worker);
    app.sampling_in_progress = true;

    app.poll_sample_results().unwrap();

    assert!(!app.sampling_in_progress);
    assert_eq!(app.snapshot.process_count, 4);
    assert!(app.status.contains("sampling worker stopped"));
}

#[test]
fn process_counter_instances_map_to_pids() {
    let process_ids = [
        ("chrome".to_string(), 4100),
        ("chrome#1".to_string(), 4120),
        ("_Total".to_string(), 999_999),
        ("Idle".to_string(), 0),
    ]
    .into_iter()
    .collect::<Vec<_>>();
    let handle_counts = [
        ("chrome".to_string(), 1200),
        ("chrome#1".to_string(), 800),
        ("_Total".to_string(), 2000),
    ]
    .into_iter()
    .collect::<Vec<_>>();

    let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

    assert_eq!(mapped.get(&4100), Some(&1200));
    assert_eq!(mapped.get(&4120), Some(&800));
    assert!(!mapped.contains_key(&0));
    assert_eq!(mapped.len(), 2);
}

#[test]
fn process_counter_instances_skip_missing_values() {
    let process_ids = [("app".to_string(), 1234), ("app#1".to_string(), 1235)]
        .into_iter()
        .collect::<Vec<_>>();
    let handle_counts = [("app".to_string(), 77)].into_iter().collect::<Vec<_>>();

    let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

    assert_eq!(mapped.get(&1234), Some(&77));
    assert!(!mapped.contains_key(&1235));
}

#[test]
fn process_counter_instances_keep_duplicate_names_by_occurrence_order() {
    let process_ids = [
        ("svchost".to_string(), 3144),
        ("svchost".to_string(), 3068),
        ("svchost".to_string(), 2568),
    ]
    .into_iter()
    .collect::<Vec<_>>();
    let handle_counts = [
        ("svchost".to_string(), 274),
        ("svchost".to_string(), 400),
        ("svchost".to_string(), 156),
    ]
    .into_iter()
    .collect::<Vec<_>>();

    let mapped = map_process_counter_instances_to_pids(process_ids, handle_counts);

    assert_eq!(mapped.get(&3144), Some(&274));
    assert_eq!(mapped.get(&3068), Some(&400));
    assert_eq!(mapped.get(&2568), Some(&156));
}

#[test]
fn process_counter_instances_map_double_values_to_pids() {
    let process_ids = [("app".to_string(), 1000), ("app#1".to_string(), 1001)]
        .into_iter()
        .collect::<Vec<_>>();
    let cpu_values = [("app".to_string(), 12.5), ("app#1".to_string(), 25.0)]
        .into_iter()
        .collect::<Vec<_>>();

    let mapped = map_process_counter_instances_to_pids(process_ids, cpu_values);

    assert_eq!(mapped.get(&1000), Some(&12.5));
    assert_eq!(mapped.get(&1001), Some(&25.0));
}

#[test]
fn process_counter_instance_map_is_reusable_across_counters() {
    let instances =
        ProcessInstanceMap::new(vec![("app".to_string(), 1000), ("app".to_string(), 1001)]);

    let private_bytes = instances.map_counter_values(vec![
        ("app".to_string(), 10_u64),
        ("app".to_string(), 20_u64),
    ]);
    let handle_counts = instances.map_counter_values(vec![
        ("app".to_string(), 30_u64),
        ("app".to_string(), 40_u64),
    ]);

    assert_eq!(private_bytes.get(&1000), Some(&10));
    assert_eq!(private_bytes.get(&1001), Some(&20));
    assert_eq!(handle_counts.get(&1000), Some(&30));
    assert_eq!(handle_counts.get(&1001), Some(&40));
}

#[test]
fn normalize_process_cpu_percent_scales_uncapped_pdh_percent_to_total_capacity() {
    assert_eq!(normalize_process_cpu_percent(100.0, 20), Some(5.0));
    assert_eq!(normalize_process_cpu_percent(400.0, 8), Some(50.0));
    assert_eq!(normalize_process_cpu_percent(2_000.0, 20), Some(100.0));
    assert_eq!(normalize_process_cpu_percent(2_500.0, 20), Some(100.0));
    assert_eq!(normalize_process_cpu_percent(-1.0, 8), None);
}

#[test]
fn standby_cache_sum_uses_available_counters() {
    assert_eq!(sum_optional_values([Some(10), None, Some(25)]), Some(35));
    assert_eq!(sum_optional_values([None, None, None]), None);
}

#[test]
fn filtered_dxgi_adapters_are_skipped() {
    assert!(is_filtered_dxgi_adapter(DXGI_ADAPTER_FLAG_SOFTWARE));
    assert!(is_filtered_dxgi_adapter(DXGI_ADAPTER_FLAG_REMOTE));
    assert!(!is_filtered_dxgi_adapter(0));
}

#[test]
fn gpu_graph_identity_uses_luid_and_metric_not_display_name() {
    let adapter_id = model::GpuAdapterId { high: 1, low: 2 };
    assert_eq!(
        GraphSlot::gpu(adapter_id, "old name", SystemMetric::GpuEncode),
        GraphSlot::gpu(adapter_id, "new name", SystemMetric::GpuEncode)
    );
    assert_ne!(
        GraphSlot::gpu(adapter_id, "GPU", SystemMetric::GpuEncode),
        GraphSlot::gpu(adapter_id, "GPU", SystemMetric::GpuDecode)
    );
}

#[test]
fn sampling_request_is_not_sent_while_in_progress() {
    let (sampling_worker, request_rx, _result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(3, 10, sampling_worker);
    app.status = "Copied row: proc-0".to_string();

    assert!(!app.request_sample().unwrap());
    assert!(app.sampling_in_progress);
    assert_eq!(request_rx.try_recv(), Ok(SampleRequest::Sample));
    assert_eq!(app.status, "Copied row: proc-0");

    assert!(!app.request_sample().unwrap());
    assert_eq!(request_rx.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(app.status, "Copied row: proc-0");
}
