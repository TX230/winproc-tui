use super::support::{make_test_app, make_test_app_with_worker, test_snapshot};
use crate::app::{DetailsMetric, FocusedPanel, GraphSlot, GraphSlotLayout};
use crate::model::history::SystemSample;
use crate::model::{
    ProcessHistory, SortColumn, SortDirection, SortSpec, SystemHistory, sort_process_rows,
};
use crate::samplers::pdh::ProcessInstanceMap;
use crate::samplers::{CollectSnapshotResult, SamplingWorker};
use crate::ui;
use crate::ui::main_panel_areas;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jiff::SignedDuration;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_process_cursor_navigation_and_refresh_frames() {
    fn summarize(label: &str, durations: &[Duration]) {
        let mut micros = durations
            .iter()
            .map(|duration| duration.as_micros() as u64)
            .collect::<Vec<_>>();
        micros.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = micros.len().saturating_sub(1).saturating_mul(percent) / 100;
            micros[index]
        };
        let avg = micros.iter().sum::<u64>() / micros.len().max(1) as u64;
        println!(
            "{label}: avg={}us p50={}us p95={}us p99={}us max={}us",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            micros.last().copied().unwrap_or(0)
        );
    }

    let screen = Rect::new(0, 0, 100, 45);
    let page_size = main_panel_areas(screen, false, 1_000, false)
        .processes
        .page_size;

    for row_count in [120usize, 1_000usize] {
        let mut app = make_test_app(row_count, page_size);
        app.focused_panel = FocusedPanel::Processes;
        app.set_screen_area(screen);
        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("warmup render should succeed");

        let mut moving_down = true;
        let mut frame_durations = Vec::new();
        for _ in 0..300 {
            let selected = app.process_table_state.selected().unwrap_or(0);
            if selected >= row_count.saturating_sub(1) {
                moving_down = false;
            } else if selected == 0 {
                moving_down = true;
            }
            let key = if moving_down {
                KeyCode::Down
            } else {
                KeyCode::Up
            };
            let start = Instant::now();
            app.on_key(KeyEvent::new(key, KeyModifiers::NONE))
                .expect("navigation should succeed");
            terminal
                .draw(|frame| ui::draw(frame, &app))
                .expect("render should succeed");
            frame_durations.push(start.elapsed());
        }
        summarize(&format!("cursor+render rows={row_count}"), &frame_durations);

        let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
        let mut app = make_test_app_with_worker(row_count, page_size, sampling_worker);
        app.focused_panel = FocusedPanel::Processes;
        app.set_screen_area(screen);
        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("warmup render should succeed");

        let snapshots = (0..40)
            .map(|index| {
                let mut snapshot = test_snapshot(row_count);
                snapshot.captured_at =
                    app.snapshot.captured_at + SignedDuration::from_secs(index + 1);
                CollectSnapshotResult {
                    snapshot,
                    warning: None,
                }
            })
            .collect::<Vec<_>>();
        let mut refresh_durations = Vec::new();
        for sample in snapshots {
            app.sampling_in_progress = true;
            result_tx.send(sample).unwrap();
            let start = Instant::now();
            app.poll_sample_results()
                .expect("sample poll should succeed");
            terminal
                .draw(|frame| ui::draw(frame, &app))
                .expect("render should succeed");
            refresh_durations.push(start.elapsed());
        }
        summarize(
            &format!("sample+render rows={row_count}"),
            &refresh_durations,
        );
    }
}

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_long_history_graph_rendering() {
    fn summarize(label: &str, durations: &[Duration]) {
        let mut micros = durations
            .iter()
            .map(|duration| duration.as_micros() as u64)
            .collect::<Vec<_>>();
        micros.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = micros.len().saturating_sub(1).saturating_mul(percent) / 100;
            micros[index]
        };
        let avg = micros.iter().sum::<u64>() / micros.len().max(1) as u64;
        println!(
            "{label}: avg={}us p50={}us p95={}us p99={}us max={}us",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            micros.last().copied().unwrap_or(0)
        );
    }

    let screen = Rect::new(0, 0, 160, 80);
    let mut app = make_test_app(1, 10);
    app.set_screen_area(screen);
    let identity = app.selected_visible_process_identity().unwrap();
    for metric in [
        DetailsMetric::Private,
        DetailsMetric::Workset,
        DetailsMetric::CpuPercent,
        DetailsMetric::IoRead,
    ] {
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), metric),
            FocusedPanel::Processes,
        ));
    }
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.show_samples_panel = true;
    app.process_history = ProcessHistory::default();
    let tracked_names = std::collections::HashSet::from([identity.name.to_ascii_lowercase()]);
    let base = app.snapshot.captured_at - SignedDuration::from_secs(7_199);
    for offset in 0..7_200_i64 {
        let process = &mut app.snapshot.processes[0];
        process.private_bytes = Some(offset as u64 * 1_024);
        process.workset_bytes = Some(offset as u64 * 2_048);
        process.cpu_percent = Some((offset % 100) as f64);
        process.io_read_bytes_per_sec = Some(offset as u64 * 4_096);
        app.snapshot.captured_at = base + SignedDuration::from_secs(offset);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &tracked_names,
        );
    }
    app.select_details_sample_latest();

    let backend = TestBackend::new(screen.width, screen.height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");
    terminal
        .draw(|frame| ui::draw(frame, &app))
        .expect("warmup render should succeed");

    let mut render_durations = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .expect("render should succeed");
        render_durations.push(start.elapsed());
    }
    summarize("graph-render slots=4 samples=7200", &render_durations);
}

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_pause_long_histories() {
    fn summarize(label: &str, durations: &[Duration]) {
        let mut nanos = durations
            .iter()
            .map(|duration| duration.as_nanos() as u64)
            .collect::<Vec<_>>();
        nanos.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = nanos.len().saturating_sub(1).saturating_mul(percent) / 100;
            nanos[index]
        };
        let avg = nanos.iter().sum::<u64>() / nanos.len().max(1) as u64;
        println!(
            "{label}: avg={}ns p50={}ns p95={}ns p99={}ns max={}ns",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            nanos.last().copied().unwrap_or(0)
        );
    }

    let mut snapshot = test_snapshot(32);
    let tracked_names = snapshot
        .processes
        .iter()
        .map(|process| process.name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let base = snapshot.captured_at - SignedDuration::from_secs(7_199);
    let mut process_history = ProcessHistory::default();
    let mut system_history = SystemHistory::default();
    for offset in 0..7_200_i64 {
        snapshot.captured_at = base + SignedDuration::from_secs(offset);
        process_history.record_snapshot(snapshot.captured_at, &snapshot.processes, &tracked_names);
        system_history.record_snapshot(&snapshot);
    }

    let mut clone_durations = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        let paused_process_history = process_history.clone();
        let paused_system_history = system_history.clone();
        clone_durations.push(start.elapsed());
        std::hint::black_box((paused_process_history, paused_system_history));
    }
    summarize(
        "pause-history-clone processes=32 samples=7200",
        &clone_durations,
    );

    let mut pause_and_sample_durations = Vec::new();
    for offset in 0..50_i64 {
        snapshot.captured_at += SignedDuration::from_secs(1);
        let start = Instant::now();
        let paused_process_history = process_history.clone();
        let paused_system_history = system_history.clone();
        process_history.record_snapshot(snapshot.captured_at, &snapshot.processes, &tracked_names);
        system_history.record_snapshot(&snapshot);
        pause_and_sample_durations.push(start.elapsed());
        std::hint::black_box((paused_process_history, paused_system_history, offset));
    }
    summarize(
        "pause+next-history-sample processes=32 samples=7200",
        &pause_and_sample_durations,
    );
}

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_system_history_retention() {
    fn summarize(label: &str, durations: &[Duration]) {
        let mut nanos = durations
            .iter()
            .map(|duration| duration.as_nanos() as u64)
            .collect::<Vec<_>>();
        nanos.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = nanos.len().saturating_sub(1).saturating_mul(percent) / 100;
            nanos[index]
        };
        let avg = nanos.iter().sum::<u64>() / nanos.len().max(1) as u64;
        println!(
            "{label}: avg={}ns p50={}ns p95={}ns p99={}ns max={}ns",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            nanos.last().copied().unwrap_or(0)
        );
    }

    let mut snapshot = test_snapshot(0);
    let initial = SystemSample::from_snapshot(&snapshot);
    let mut legacy = vec![initial; 7_200];
    let mut current = SystemHistory::default();
    for _ in 0..7_200 {
        snapshot.captured_at += SignedDuration::from_secs(1);
        current.record_snapshot(&snapshot);
    }

    let mut legacy_durations = Vec::new();
    let mut current_durations = Vec::new();
    for _ in 0..2_000 {
        snapshot.captured_at += SignedDuration::from_secs(1);
        let sample = SystemSample::from_snapshot(&snapshot);
        let start = Instant::now();
        legacy.push(sample);
        legacy.drain(0..1);
        legacy_durations.push(start.elapsed());
        std::hint::black_box(legacy.first());

        let start = Instant::now();
        current.record_snapshot(&snapshot);
        current_durations.push(start.elapsed());
        std::hint::black_box(current.sample_at_index(0));
    }
    summarize(
        "system-history legacy vec-drain samples=7200",
        &legacy_durations,
    );
    summarize(
        "system-history current chunked-ring samples=7200",
        &current_durations,
    );
}

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_process_sorting() {
    fn summarize(label: &str, durations: &[Duration]) {
        let mut micros = durations
            .iter()
            .map(|duration| duration.as_micros() as u64)
            .collect::<Vec<_>>();
        micros.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = micros.len().saturating_sub(1).saturating_mul(percent) / 100;
            micros[index]
        };
        let avg = micros.iter().sum::<u64>() / micros.len().max(1) as u64;
        println!(
            "{label}: avg={}us p50={}us p95={}us p99={}us max={}us",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            micros.last().copied().unwrap_or(0)
        );
    }

    let template = test_snapshot(1_000).processes;
    let sort = SortSpec {
        column: SortColumn::ProcessName,
        direction: SortDirection::Asc,
    };
    let mut legacy_durations = Vec::new();
    let mut current_durations = Vec::new();
    for _ in 0..500 {
        let mut rows = template.clone();
        let start = Instant::now();
        rows.sort_by(|left, right| {
            right
                .workset_bytes
                .unwrap_or(0)
                .cmp(&left.workset_bytes.unwrap_or(0))
                .then_with(|| {
                    right
                        .private_bytes
                        .unwrap_or(0)
                        .cmp(&left.private_bytes.unwrap_or(0))
                })
                .then_with(|| left.name.cmp(&right.name))
        });
        rows.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| left.pid.cmp(&right.pid))
        });
        legacy_durations.push(start.elapsed());
        std::hint::black_box(rows.first());

        let mut rows = template.clone();
        let start = Instant::now();
        sort_process_rows(&mut rows, sort);
        current_durations.push(start.elapsed());
        std::hint::black_box(rows.first());
    }
    summarize("process-sort legacy rows=1000 passes=2", &legacy_durations);
    summarize(
        "process-sort current rows=1000 passes=1",
        &current_durations,
    );
}

#[test]
#[ignore = "manual performance probe; run with --ignored --nocapture"]
fn perf_process_counter_mapping() {
    fn legacy_map<T: Copy>(
        process_ids: Vec<(String, u64)>,
        counter_values: Vec<(String, T)>,
    ) -> std::collections::HashMap<u32, T> {
        let mut values = std::collections::HashMap::new();
        let mut counters_by_instance =
            std::collections::HashMap::<String, std::collections::VecDeque<T>>::new();
        for (instance_name, counter_value) in counter_values {
            counters_by_instance
                .entry(instance_name)
                .or_default()
                .push_back(counter_value);
        }
        for (instance_name, pid_value) in process_ids {
            if instance_name == "_Total" || pid_value == 0 || pid_value > u32::MAX as u64 {
                continue;
            }
            let Some(counter_value) = counters_by_instance
                .get_mut(&instance_name)
                .and_then(std::collections::VecDeque::pop_front)
            else {
                continue;
            };
            values.insert(pid_value as u32, counter_value);
        }
        values
    }

    fn summarize(label: &str, durations: &[Duration]) {
        let mut micros = durations
            .iter()
            .map(|duration| duration.as_micros() as u64)
            .collect::<Vec<_>>();
        micros.sort_unstable();
        let percentile = |percent: usize| -> u64 {
            let index = micros.len().saturating_sub(1).saturating_mul(percent) / 100;
            micros[index]
        };
        let avg = micros.iter().sum::<u64>() / micros.len().max(1) as u64;
        println!(
            "{label}: avg={}us p50={}us p95={}us p99={}us max={}us",
            avg,
            percentile(50),
            percentile(95),
            percentile(99),
            micros.last().copied().unwrap_or(0)
        );
    }

    let process_ids = (1..=1_000_u64)
        .map(|pid| (format!("process-{}", pid % 250), pid))
        .collect::<Vec<_>>();
    let counter_sets = (0..6_u64)
        .map(|counter| {
            process_ids
                .iter()
                .enumerate()
                .map(|(index, (name, _))| (name.clone(), counter * 10_000 + index as u64))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut legacy_durations = Vec::new();
    let mut current_durations = Vec::new();
    for _ in 0..500 {
        let mut legacy_process_ids = Some(process_ids.clone());
        let legacy_counter_sets = counter_sets.clone();
        let legacy_count = legacy_counter_sets.len();
        let start = Instant::now();
        for (index, counter_values) in legacy_counter_sets.into_iter().enumerate() {
            let ids = if index + 1 == legacy_count {
                legacy_process_ids.take().unwrap()
            } else {
                legacy_process_ids.as_ref().unwrap().clone()
            };
            std::hint::black_box(legacy_map(ids, counter_values));
        }
        legacy_durations.push(start.elapsed());

        let current_process_ids = process_ids.clone();
        let current_counter_sets = counter_sets.clone();
        let start = Instant::now();
        let process_instances = ProcessInstanceMap::new(current_process_ids);
        for counter_values in current_counter_sets {
            std::hint::black_box(process_instances.map_counter_values(counter_values));
        }
        current_durations.push(start.elapsed());
    }
    summarize(
        "process-counter-map legacy instances=1000 counters=6",
        &legacy_durations,
    );
    summarize(
        "process-counter-map current instances=1000 counters=6",
        &current_durations,
    );
}
