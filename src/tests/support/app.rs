use chrono::{Local, TimeZone};
use ratatui::widgets::TableState;

use crate::app::{
    self, App, DetailsMetric, DetailsTarget, FocusedPanel, GraphSlot, GraphSlotLayout,
    ProcessPanelHeight, VisibleProcessEntry,
};
use crate::config::{self, RuntimeConfig};
use crate::model::{
    self, ColumnPreset, MetricColumn, ProcessColumnWidths, ProcessHistory, ProcessIdentity,
    ProcessRow, Snapshot, SortSpec, SystemHistory,
};
use crate::samplers::open_files::OpenFilesWorker;
use crate::samplers::process_environment::ProcessEnvironmentWorker;
use crate::samplers::process_info::ProcessInfoWorker;
use crate::samplers::process_modules::ProcessModulesWorker;
use crate::samplers::{self, SamplingWorker};
use crate::ui;

pub(in crate::tests) fn make_test_app(row_count: usize, page_size: usize) -> App {
    let (sampling_worker, _, _) = SamplingWorker::test_pair();
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    make_test_app_with_workers(
        row_count,
        page_size,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    )
}

pub(in crate::tests) fn assign_private_graph(app: &mut App) {
    let identity = app
        .selected_visible_process_identity()
        .expect("selected process identity");
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
}

pub(in crate::tests) fn test_graph_source(app: &App, index: usize) -> GraphSlot {
    let mut row = app.snapshot.processes[0].clone();
    row.pid = 10_000 + index as u32;
    row.start_time = Some(1_800_000_000 + index as u64);
    row.name = format!("graph-{index}.exe");
    GraphSlot::process(ProcessIdentity::from_row(&row), DetailsMetric::Private)
}

pub(in crate::tests) fn add_test_graph(app: &mut App, index: usize) -> app::GraphId {
    let source = test_graph_source(app, index);
    assert!(app.add_or_reveal_graph_source(source, FocusedPanel::Processes));
    app.graph_entries.last().unwrap().id
}

pub(in crate::tests) fn track_process_name(app: &mut App, name: &str) {
    app.watch_list = vec![name.to_string()];
    app.normalized_watch_names = std::collections::HashSet::from([name.to_ascii_lowercase()]);
    app.watch_enabled = true;
    app.rebuild_visible_process_cache();
}

pub(in crate::tests) fn record_tracked_process_history_samples(
    app: &mut App,
    name: &str,
    count: usize,
) {
    let mut process = app.snapshot.processes[0].clone();
    process.name = name.to_string();
    process.pid = 42;
    process.start_time = Some(1_700_000_042);
    let tracked_names = std::collections::HashSet::from([name.to_ascii_lowercase()]);
    let now = Local.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).unwrap();
    app.process_history = ProcessHistory::default();

    for offset in 0..count {
        process.private_bytes = Some(offset as u64);
        app.process_history.record_snapshot(
            now + chrono::Duration::seconds(offset as i64),
            &[process.clone()],
            &tracked_names,
        );
    }
}

pub(in crate::tests) fn selected_process_history_sample_count(app: &App, name: &str) -> usize {
    app.process_history.sample_count_for(&ProcessIdentity {
        pid: 42,
        name: name.to_string(),
        start_time: Some(1_700_000_042),
    })
}

pub(in crate::tests) fn make_test_app_with_worker(
    row_count: usize,
    page_size: usize,
    sampling_worker: SamplingWorker,
) -> App {
    let (process_info_worker, _, _) = ProcessInfoWorker::test_pair();
    let (open_files_worker, _, _) = OpenFilesWorker::test_pair();
    make_test_app_with_workers(
        row_count,
        page_size,
        sampling_worker,
        process_info_worker,
        open_files_worker,
    )
}

pub(in crate::tests) fn make_test_app_with_workers(
    row_count: usize,
    page_size: usize,
    sampling_worker: SamplingWorker,
    process_info_worker: ProcessInfoWorker,
    open_files_worker: OpenFilesWorker,
) -> App {
    let process_modules_worker = ProcessModulesWorker::test_noop();
    let process_environment_worker = ProcessEnvironmentWorker::test_noop();
    let mut table_state = TableState::default();
    if row_count > 0 {
        table_state.select(Some(0));
    }

    let snapshot = test_snapshot(row_count);
    let selected_process_identity = table_state
        .selected()
        .and_then(|index| snapshot.processes.get(index))
        .map(model::ProcessIdentity::from_row);

    App {
        threads: crate::app::threads::ThreadsView::default(),
        threads_worker: crate::samplers::threads::ThreadWorker::spawn(),
        runtime: RuntimeConfig {
            mouse: true,
            config_path: None,
            recording_last_dir: None,
            initial_theme: "Green".to_string(),
            initial_high_contrast: false,
            initial_graph_slot_layout: GraphSlotLayout::Auto,
            initial_graph_time_span_seconds: 60,
            initial_graph_y_axis_zero_min: true,
            initial_show_samples_panel: true,
            initial_show_sample_delta: true,
            initial_recording_interval_seconds: 1,
            column_preset: ColumnPreset::Default,
            process_columns: vec![
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
            ],
            process_column_widths: ProcessColumnWidths::default(),
            sort: SortSpec::default(),
            initial_tracked_only: false,
            initial_show_exited: true,
            initial_process_view_mode: app::ProcessViewMode::Flat,
            initial_process_panel_height: ProcessPanelHeight::Auto,
            process_filters: Vec::new(),
            investigation_startup: config::InvestigationStartup::ResumeLast,
            active_investigation_profile: None,
            saved_investigation_profiles: Vec::new(),
            sampling_options: samplers::SamplingOptions::default(),
        },
        sampling_worker,
        process_info_worker,
        open_files_worker,
        process_modules_worker,
        process_environment_worker,
        network_worker: crate::samplers::network::NetworkWorker::test_pair().0,
        network_browser: crate::app::network::NetworkView::default(),
        process_network: crate::app::network::NetworkView::default(),
        network_next_id: 0,
        file_users_worker: crate::samplers::file_users::FileUsersWorker::test_pair().0,
        file_users: crate::app::file_users::FileUsersView::default(),
        scheduling_worker: crate::samplers::scheduling::SchedulingWorker::test_pair().0,
        scheduling: crate::app::scheduling::SchedulingView::default(),
        file_users_next_id: 0,
        sampling_in_progress: false,
        snapshot,
        system_info_host: app::system_info::SystemInfoHost::default(),
        process_table_state: table_state,
        process_page_size: page_size,
        process_panel_body_capacity: page_size,
        process_panel_height: ProcessPanelHeight::Auto,
        process_panel_resize_drag: None,
        process_panel_resize_hovered: false,
        selected_process_identity,
        process_selection_anchor: None,
        selected_process_identities: std::collections::HashSet::new(),
        selected_process_column_index: 2,
        process_metric_column_offset: 0,
        process_order_hold_until: None,
        main_menu_activity: None,
        main_menu_section: crate::app::state::MainMenuSection::File,
        main_menu_selected: 0,
        main_menu_tracking_target: None,
        main_menu_hovered: None,
        header_action_hovered: None,
        show_help: false,
        help_version_info: false,
        help_section: 0,
        help_section_picker: None,
        help_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        show_column_picker: false,
        investigation_profiles_dialog: None,
        active_investigation_profile: None,
        show_quit_confirmation: false,
        show_recording_no_tracked_warning: false,
        show_recording_path_dialog: false,
        recording_path_draft: String::new(),
        recording_path_cursor: 0,
        recording_path_completion: app::path_completion::PathCompletionState::default(),
        recording_dialog_focus: app::state::RecordingDialogFocus::default(),
        recording_interval_index: 0,
        show_recording_overwrite_confirmation: false,
        show_recording_stop_confirmation: false,
        show_recording_tracking_fixed: false,
        recording_error: None,
        show_tracked_remove_confirmation: false,
        tracked_remove_name: String::new(),
        tracked_remove_total_samples: 0,
        tracked_remove_discarded_samples: 0,
        show_process_kill_confirmation: false,
        process_kill_targets: Vec::new(),
        process_kill_job: None,
        process_kill_scroll: 0,
        show_display_area_warning: false,
        show_metric_column_warning: false,
        show_no_graph_metrics_warning: false,
        recording_session: None,
        recording_last_dir: None,
        recording_spinner_index: 0,
        log_view_path: None,
        log_view_interval_seconds: None,
        log_view_frame_times: Vec::new(),
        should_quit: false,
        column_picker_index: 0,
        column_picker_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        show_log_list: false,
        log_list_index: 0,
        log_list_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        show_log_dir_dialog: false,
        log_dir_draft: String::new(),
        log_dir_cursor: 0,
        log_dir_completion: app::path_completion::PathCompletionState::default(),
        log_dir_error: None,
        open_files_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        open_files_result: None,
        open_files_result_identity: None,
        open_files_in_flight: None,
        open_files_in_flight_generation: None,
        open_files_refresh: app::open_files::OpenFilesRefresh::default(),
        open_files_filter: String::new(),
        open_files_match_path: None,
        open_files_filter_cursor: 0,
        open_files_selected: 0,
        open_files_show_detail: false,
        file_action: None,
        file_action_worker: None,
        file_action_generation: 0,
        process_modules_result: None,
        process_modules_result_identity: None,
        process_modules_error: None,
        process_modules_in_flight: None,
        process_modules_in_flight_generation: None,
        process_modules_in_flight_request_id: None,
        process_modules_next_request_id: 0,
        process_modules_filter: String::new(),
        process_modules_filter_cursor: 0,
        process_modules_selected: 0,
        process_modules_show_detail: false,
        process_environment_result: None,
        process_environment_result_identity: None,
        process_environment_error: None,
        process_environment_in_flight: None,
        process_environment_in_flight_generation: None,
        process_environment_in_flight_request_id: None,
        process_environment_next_request_id: 0,
        process_environment_filter: String::new(),
        process_environment_filter_cursor: 0,
        process_environment_selected: 0,
        process_environment_show_detail: false,
        show_process_info_dialog: false,
        process_info_tab: app::ProcessInfoTab::Metrics,
        process_info_focus: app::ProcessInfoFocus::Content,
        process_info_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        process_info_image_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        process_info_dlls_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        process_info_environment_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        process_info_manual_height: None,
        process_info_resize_drag: None,
        process_info_target: None,
        process_info_verified_snapshot_at: None,
        process_info_generation: 0,
        show_cpu_core_dialog: false,
        cpu_core_scroll: ui::widgets::scrollable_modal::ScrollableModalState {
            page_size: 1,
            ..ui::widgets::scrollable_modal::ScrollableModalState::default()
        },
        show_system_info_dialog: false,
        log_summaries: Vec::new(),
        log_list_dir: None,
        log_list_worker: None,
        log_list_last_click: None,
        log_load_worker: None,
        log_view_watch_list: Vec::new(),
        log_view_normalized_watch_names: std::collections::HashSet::new(),
        focused_panel: FocusedPanel::Processes,
        show_details: false,
        graph_entries: Vec::new(),
        graph_reorder_dialog: None,
        graph_csv_dialog: None,
        active_graph_id: None,
        next_graph_id: 0,
        graph_scroll_row: 0,
        graph_scrollbar_dragging: false,
        graph_scrollbar_grab_offset: 0,
        graph_hovered_target: None,
        cpu_per_core_hovered: false,
        graph_return_focus: FocusedPanel::Processes,
        source_cell_last_click: None,
        details_target: DetailsTarget::Process,
        details_metric: DetailsMetric::Private,
        details_sample_selected: 0,
        sample_selection: None,
        details_sample_offset: 0,
        details_sample_page_size: 1,
        samples_scrollbar_dragging: false,
        samples_scrollbar_grab_offset: 0,
        executable_location: None,
        executable_location_worker: None,
        context_menu: None,
        pending_context_menu: None,
        graph_pan_drag: None,
        graph_time_span_seconds: 60,
        graph_time_offset_seconds: 0,
        graph_time_window_right_at: None,
        graph_show_all_samples: false,
        graph_y_axis_zero_min: true,
        graph_slot_layout: GraphSlotLayout::Auto,
        show_samples_panel: true,
        samples_temporarily_collapsed: false,
        show_sample_delta: true,
        details_live: true,
        column_preset: ColumnPreset::Default,
        process_columns: vec![
            MetricColumn::PrivateBytes,
            MetricColumn::WorksetPrivateBytes,
        ],
        process_column_widths: ProcessColumnWidths::default(),
        sort: SortSpec::default(),
        process_view_mode: app::ProcessViewMode::Flat,
        collapsed_process_identities: std::collections::HashSet::new(),
        draw_cursor: Default::default(),
        shortcut_map: Default::default(),
        shortcut_hovered: None,
        process_tracked_only_hovered: false,
        process_show_exited_hovered: false,
        process_view_mode_hovered: false,
        process_column_control_hovered: None,
        column_resize_hovered: None,
        column_resize_drag: None,
        process_disclosure_hovered: None,
        paused_display: None,
        log_view_display: None,
        filter_text: String::new(),
        filter_draft: String::new(),
        filter_cursor: 0,
        process_info_filter_editing: false,
        filter_editing: false,
        jump_draft: String::new(),
        jump_editing: false,
        watch_list: Vec::new(),
        normalized_watch_names: std::collections::HashSet::new(),
        watch_enabled: false,
        show_exited: true,
        visible_process_entries: (0..row_count)
            .map(|snapshot_index| VisibleProcessEntry::Live {
                snapshot_index,
                depth: 0,
                has_children: false,
                expanded: false,
                context_only: false,
            })
            .collect(),
        visible_process_match_count: row_count,
        tracked_total_row: None,
        exited_tracked_rows: std::collections::HashMap::new(),
        last_tracked_live_identities: std::collections::HashSet::new(),
        process_history: ProcessHistory::default(),
        system_history: SystemHistory::default(),
        ram_vram_selected_index: 0,
        resource_panel: app::ResourcePanel::Memory,
        gpu_adapter_index: 0,
        system_activity_selected_index: 0,
        cpu_selected_index: 0,
        process_info_cache: std::collections::HashMap::new(),
        process_info_display_identity: None,
        pending_process_info: None,
        process_info_in_flight: None,
        process_info_in_flight_generation: None,
        ab_comparison: None,
        last_screen_area: ratatui::layout::Rect::new(0, 0, 100, 45),
        theme_index: 0,
        high_contrast: false,
        status: String::new(),
        status_feedback: app::feedback::StatusFeedback::default(),
    }
}

pub(in crate::tests) fn test_snapshot(row_count: usize) -> Snapshot {
    let processes = (0..row_count)
        .map(|index| ProcessRow {
            pid: index as u32,
            parent_pid: None,
            name: format!("proc-{index}"),
            executable_path: None,
            start_time: Some(1_700_000_000 + index as u64),
            cpu_percent: None,
            private_bytes: Some(index as u64),
            workset_bytes: Some(index as u64),
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        })
        .collect::<Vec<_>>();

    Snapshot {
        captured_at: Local::now(),
        total_memory: 0,
        used_memory: 0,
        available_memory: None,
        modified_memory: None,
        standby_memory: None,
        free_zeroed_memory: None,
        committed_memory: None,
        commit_limit: None,
        paged_pool_memory: None,
        nonpaged_pool_memory: None,
        pages_input_per_sec: None,
        pages_output_per_sec: None,
        cpu_name: None,
        cpu_frequency_mhz: None,
        cpu_current_frequency_mhz: None,
        cpu_p_core_frequency_mhz: None,
        cpu_e_core_frequency_mhz: None,
        cpu_total_usage_percent: None,
        cpu_user_usage_percent: None,
        cpu_kernel_usage_percent: None,
        cpu_logical_processors: Vec::new(),
        cpu_topology: None,
        cpu_cache: None,
        gpu_adapters: Vec::new(),
        disks: Vec::new(),
        disk_read_bytes_per_sec: None,
        disk_write_bytes_per_sec: None,
        disk_queue_length: None,
        network_received_bytes_per_sec: None,
        network_sent_bytes_per_sec: None,
        process_count: row_count,
        thread_count: None,
        processes,
    }
}

pub(in crate::tests) fn populate_system_info(app: &mut App) {
    app.system_info_host = app::system_info::SystemInfoHost {
        windows_version: Some("Windows 11 Pro".to_string()),
        windows_build: Some("26100".to_string()),
        architecture: Some("x64".to_string()),
    };
    app.snapshot.total_memory = 34_000_000_000;
    app.snapshot.commit_limit = Some(51_000_000_000);
    app.snapshot.cpu_name = Some("Test CPU".to_string());
    app.snapshot.cpu_frequency_mhz = Some(2_100);
    app.snapshot.cpu_topology = Some("8 P-cores / 4 E-cores".to_string());
    app.snapshot.cpu_cache = Some("L3 25.0 MB".to_string());
    app.snapshot.gpu_adapters = vec![model::GpuAdapterSample {
        name: Some("Test GPU".to_string()),
        dedicated_total: Some(8_406_000_000),
        shared_total: Some(17_000_000_000),
        ..model::GpuAdapterSample::default()
    }];
    app.snapshot.disks = vec![model::DiskUsageSample {
        name: "C:".to_string(),
        free_bytes: 123_000_000_000,
        total_bytes: 500_000_000_000,
    }];
}
