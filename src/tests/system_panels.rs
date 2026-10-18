use super::support::{
    add_test_graph, assign_private_graph, buffer_to_text, find_text_position_in_area, left_click,
    make_test_app, make_test_app_with_worker, render_app_to_buffer, render_app_to_text,
    test_snapshot,
};
use crate::app;
use crate::app::{DetailsMetric, DetailsTarget, FocusedPanel, GraphSlot, GraphValueFormat};
use crate::model;
use crate::model::{
    ColumnPreset, MetricColumn, ProcessIdentity, SortColumn, SortDirection, SystemMetric,
};
use crate::samplers::{CollectSnapshotResult, SamplingWorker};
use crate::ui;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

fn assert_registered_value_token(
    buffer: &ratatui::buffer::Buffer,
    area: Rect,
    token: &str,
    theme: ui::Theme,
    bold: bool,
    background: Color,
) -> (u16, u16) {
    let (x, y) = find_text_position_in_area(buffer, area, token)
        .unwrap_or_else(|| panic!("registered value should render: {token}"));
    for offset in 0..token.chars().count() as u16 {
        let cell = &buffer[(x + offset, y)];
        assert_eq!(cell.fg, theme.active_series, "token: {token}");
        assert_eq!(cell.bg, background, "token: {token}");
        assert!(
            cell.modifier.contains(Modifier::UNDERLINED),
            "token: {token}"
        );
        assert_eq!(
            cell.modifier.contains(Modifier::BOLD),
            bold,
            "token: {token}"
        );
    }
    (x, y)
}

#[test]
fn clicking_system_activity_panel_moves_focus_to_system_activity() {
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::system_activity_panel_area_for_screen(screen, &app);

    app.on_mouse(left_click(area.x + 1, area.y + 1), screen);

    assert_eq!(app.focused_panel, FocusedPanel::SystemActivity);
    assert_eq!(app.status, "NW/DISK row: Net Rx");
}

#[test]
fn ram_vram_enter_does_not_assign_graph_metric() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::ModifiedMemory);
    assert_eq!(app.details_target, DetailsTarget::Process);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.details_target, DetailsTarget::Process);
    assert!(!app.show_details);
    assert!(app.status.contains("Modified"));
}

#[test]
fn ram_vram_up_down_only_selects_system_metric() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.selected_system_metric(), SystemMetric::ModifiedMemory);
    assert_eq!(app.details_target, DetailsTarget::Process);
    assert!(!app.show_details);

    app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.selected_system_metric(), SystemMetric::PhysicalMemory);
    assert_eq!(app.details_target, DetailsTarget::Process);
}

#[test]
fn ram_vram_space_toggles_selected_graph() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert!(app.watch_list.is_empty());
    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::ModifiedMemory))
    );
    assert!(app.show_details);
    assert_eq!(app.focused_panel, FocusedPanel::System);
}

#[test]
fn mem_and_gpu_g_toggle_graphs_without_switching_resource_panel() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    app.resource_panel = app::ResourcePanel::Memory;
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_details);

    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_details);
    assert_eq!(app.resource_panel, app::ResourcePanel::Memory);

    app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.show_details);
    assert_eq!(app.resource_panel, app::ResourcePanel::Memory);

    app.select_resource_panel(app::ResourcePanel::Gpu);
    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_details);
    assert_eq!(app.resource_panel, app::ResourcePanel::Gpu);

    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_details);
    assert_eq!(app.resource_panel, app::ResourcePanel::Gpu);
}

#[test]
fn ram_vram_active_graph_colors_the_value_without_a_slot_ordinal() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    app.snapshot.modified_memory = Some(424_000_000);

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::ModifiedMemory))
    );
    assert!(app.show_details);

    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::ram_vram_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position_in_area(&buffer, area, "424 MB")
        .expect("registered MEM value should render");
    let value = &buffer[(x, y)];
    assert_eq!(value.fg, app.theme().active_series);
    assert!(value.modifier.contains(Modifier::BOLD));
    assert!(find_text_position_in_area(&buffer, area, "1  Modified").is_none());
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("Slot#1 · MEM Modified"), "{rendered}");
}

#[test]
fn ram_vram_inactive_graph_colors_the_value_without_bold_or_an_ordinal() {
    let mut app = make_test_app(3, 10);
    app.snapshot.standby_memory = Some(616_000_000);
    let ids = (0..9)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::system(SystemMetric::StandbyMemory),
        FocusedPanel::System,
    ));
    assert!(app.set_active_graph(ids[0]));

    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::ram_vram_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position_in_area(&buffer, area, "616 MB")
        .expect("registered MEM value should render");
    let value = &buffer[(x, y)];

    assert_eq!(value.fg, app.theme().active_series);
    assert!(!value.modifier.contains(Modifier::BOLD));
    assert!(find_text_position_in_area(&buffer, area, "10 Standby").is_none());
}

#[test]
fn memory_and_gpu_panels_show_the_new_summary_rows() {
    let mut app = make_test_app(3, 10);
    app.snapshot.gpu_adapters.push(model::GpuAdapterSample {
        name: Some("Test GPU".to_string()),
        ..model::GpuAdapterSample::default()
    });

    let rendered = render_app_to_text(&app, 180, 30);

    assert!(rendered.contains("MEM"), "{rendered}");
    assert!(rendered.contains("GPU  1/1"), "{rendered}");
    assert!(!rendered.contains("[Max samples: 7200]"), "{rendered}");
    for label in [
        "In use",
        "Modified",
        "Standby",
        "Free + Zeroed",
        "Commit charge",
        "Paged Pool",
        "Nonpaged Pool",
        "Pages In/s",
        "Pages Out/s",
        "Threads",
        "Usage",
        "Encode",
        "Decode",
        "Dedicated",
        "Shared",
    ] {
        assert!(rendered.contains(label), "missing {label}: {rendered}");
    }
    let in_use_line = rendered
        .lines()
        .find(|line| line.contains("In use"))
        .unwrap();
    assert!(!in_use_line.contains("%)"), "{in_use_line}");
}

#[test]
fn gpu_panel_aligns_engine_and_memory_value_columns() {
    let mut app = make_test_app(3, 10);
    app.snapshot.gpu_adapters.push(model::GpuAdapterSample {
        utilization_percent: Some(56.0),
        encode: model::GpuEngineSummary {
            average_percent: Some(12.0),
            max_percent: Some(34.0),
            engine_count: 1,
        },
        decode: model::GpuEngineSummary {
            average_percent: Some(18.0),
            max_percent: Some(24.0),
            engine_count: 1,
        },
        dedicated_used: Some(821_000_000),
        dedicated_total: Some(8_406_000_000),
        shared_used: Some(54_000_000),
        shared_total: Some(17_044_000_000),
        ..model::GpuAdapterSample::default()
    });

    let rendered = render_app_to_text(&app, 180, 30);
    let value_column = |label: &str, value: &str| {
        let line = rendered
            .lines()
            .find(|line| line.contains(label) && line.contains(value))
            .unwrap_or_else(|| panic!("missing {label} row: {rendered}"));
        line.find(value)
            .unwrap_or_else(|| panic!("missing {value} in {label} row: {line}"))
    };

    let encode_column = value_column("Encode", " 12%");
    assert_eq!(value_column("Decode", " 18%"), encode_column);
    assert_eq!(value_column("Dedicated", "821 MB"), encode_column);
    assert_eq!(value_column("Shared", "54 MB"), encode_column);
    let dedicated_line = rendered
        .lines()
        .find(|line| line.contains("Dedicated") && line.contains("821 MB"))
        .unwrap();
    assert!(!dedicated_line.contains("( 10%)"), "{dedicated_line}");
}

#[test]
fn gpu_active_graph_colors_the_value_without_a_slot_ordinal() {
    let mut app = make_test_app(3, 10);
    let adapter = model::GpuAdapterSample {
        name: Some("Test GPU".to_string()),
        utilization_percent: Some(56.0),
        ..model::GpuAdapterSample::default()
    };
    let slot = GraphSlot::gpu(
        adapter.id,
        adapter.name.as_deref().unwrap(),
        SystemMetric::GpuUtilization,
    );
    app.snapshot.gpu_adapters.push(adapter);
    assert!(app.add_or_reveal_graph_source(slot, FocusedPanel::System));

    let screen = Rect::new(0, 0, 180, 30);
    let area = ui::gpu_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position_in_area(&buffer, area, "56%")
        .expect("registered GPU value should render");
    let value = &buffer[(x, y)];

    assert_eq!(value.fg, app.theme().active_series);
    assert!(value.modifier.contains(Modifier::BOLD));
    assert!(find_text_position_in_area(&buffer, area, "1  Usage").is_none());
}

#[test]
fn compact_system_graph_values_style_only_tokens_in_all_themes() {
    let screen = Rect::new(0, 0, 180, 30);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        app.snapshot.modified_memory = Some(424_000_000);
        app.snapshot.pages_input_per_sec = None;
        app.snapshot.network_received_bytes_per_sec = Some(30_000_000);
        app.snapshot.disk_read_bytes_per_sec = None;
        app.snapshot.disk_queue_length = Some(1.5);
        let adapter = model::GpuAdapterSample {
            name: Some("Test GPU".to_string()),
            utilization_percent: Some(56.0),
            encode: model::GpuEngineSummary::default(),
            ..model::GpuAdapterSample::default()
        };
        let gpu_usage = GraphSlot::gpu(
            adapter.id,
            adapter.name.as_deref().unwrap(),
            SystemMetric::GpuUtilization,
        );
        let gpu_encode = GraphSlot::gpu(
            adapter.id,
            adapter.name.as_deref().unwrap(),
            SystemMetric::GpuEncode,
        );
        app.snapshot.gpu_adapters.push(adapter);

        for slot in [
            GraphSlot::system(SystemMetric::ModifiedMemory),
            GraphSlot::system(SystemMetric::PagesInput),
            gpu_usage.clone(),
            gpu_encode,
            GraphSlot::system(SystemMetric::NetworkReceived),
            GraphSlot::system(SystemMetric::DiskRead),
            GraphSlot::system(SystemMetric::DiskQueueLength),
        ] {
            assert!(app.add_or_reveal_graph_source(slot, FocusedPanel::System));
        }
        let gpu_usage_id = app
            .graph_entries
            .iter()
            .find(|entry| entry.source == gpu_usage)
            .unwrap()
            .id;
        assert!(app.set_active_graph(gpu_usage_id));
        app.show_details = false;

        app.focused_panel = FocusedPanel::System;
        app.resource_panel = app::ResourcePanel::Memory;
        app.ram_vram_selected_index = 1;
        let memory_area = ui::ram_vram_panel_area_for_screen(screen, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        assert_registered_value_token(
            &buffer,
            memory_area,
            "424 MB",
            theme,
            false,
            theme.table_selection_surface,
        );
        let (pages_in_x, pages_in_y) =
            find_text_position_in_area(&buffer, memory_area, "Pages In/s").unwrap();
        let pages_in_row = Rect::new(
            pages_in_x,
            pages_in_y,
            memory_area.right().saturating_sub(pages_in_x),
            1,
        );
        let (missing_x, missing_y) =
            assert_registered_value_token(&buffer, pages_in_row, "--", theme, false, theme.panel);
        assert!(
            !buffer[(missing_x - 1, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        app.resource_panel = app::ResourcePanel::Gpu;
        app.ram_vram_selected_index = 0;
        let gpu_area = ui::gpu_panel_area_for_screen(screen, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (usage_x, usage_y) = assert_registered_value_token(
            &buffer,
            gpu_area,
            "56%",
            theme,
            true,
            theme.table_selection_surface,
        );
        assert_eq!(buffer[(usage_x - 1, usage_y)].symbol(), " ");
        assert!(
            !buffer[(usage_x - 1, usage_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        let (_, encode_y) = find_text_position_in_area(&buffer, gpu_area, "Encode").unwrap();
        let encode_row = Rect::new(gpu_area.x, encode_y, gpu_area.width, 1);
        let (missing_x, missing_y) =
            assert_registered_value_token(&buffer, encode_row, "--", theme, false, theme.panel);
        assert_eq!(buffer[(missing_x - 1, missing_y)].symbol(), " ");
        assert!(
            !buffer[(missing_x - 1, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        app.focused_panel = FocusedPanel::SystemActivity;
        app.system_activity_selected_index = 0;
        let activity_area = ui::system_activity_panel_area_for_screen(screen, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (rate_x, rate_y) = assert_registered_value_token(
            &buffer,
            activity_area,
            "240 Mbps",
            theme,
            false,
            theme.table_selection_surface,
        );
        assert_eq!(buffer[(rate_x - 1, rate_y)].symbol(), " ");
        assert!(
            !buffer[(rate_x - 1, rate_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        let (_, disk_read_y) =
            find_text_position_in_area(&buffer, activity_area, "Disk R").unwrap();
        let disk_read_row = Rect::new(activity_area.x, disk_read_y, activity_area.width, 1);
        let (missing_x, missing_y) =
            assert_registered_value_token(&buffer, disk_read_row, "--", theme, false, theme.panel);
        assert_eq!(buffer[(missing_x - 1, missing_y)].symbol(), " ");
        assert!(
            !buffer[(missing_x - 1, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        let (_, disk_queue_y) =
            find_text_position_in_area(&buffer, activity_area, "Disk Q").unwrap();
        let disk_queue_row = Rect::new(activity_area.x, disk_queue_y, activity_area.width, 1);
        let queue_prefix = find_text_position_in_area(&buffer, disk_queue_row, "   2")
            .expect("registered queue length should render");
        let queue_x = queue_prefix.0 + 3;
        assert_registered_value_token(
            &buffer,
            Rect::new(queue_x, queue_prefix.1, 1, 1),
            "2",
            theme,
            false,
            theme.panel,
        );
        assert!((queue_prefix.0..queue_x).all(|x| {
            buffer[(x, queue_prefix.1)].symbol() == " "
                && !buffer[(x, queue_prefix.1)]
                    .modifier
                    .contains(Modifier::UNDERLINED)
        }));
    }
}

#[test]
fn memory_pressure_panel_aligns_all_value_columns() {
    let mut app = make_test_app(3, 10);
    app.snapshot.paged_pool_memory = Some(2_769_000_000);
    app.snapshot.nonpaged_pool_memory = Some(2_097_000_000);
    app.snapshot.pages_input_per_sec = Some(25);
    app.snapshot.pages_output_per_sec = Some(15);
    app.snapshot.thread_count = Some(4_335);

    let rendered = render_app_to_text(&app, 180, 30);
    let value_column = |label: &str, value: &str| {
        let line = rendered
            .lines()
            .find(|line| line.contains(label) && line.contains(value))
            .unwrap_or_else(|| panic!("missing {label} row: {rendered}"));
        line.find(value)
            .unwrap_or_else(|| panic!("missing {value} in {label} row: {line}"))
    };

    let paged_pool_column = value_column("Paged Pool", "2,769 MB");
    assert_eq!(value_column("Nonpaged Pool", "2,097 MB"), paged_pool_column);
    assert_eq!(value_column("Pages In/s", "25"), paged_pool_column);
    assert_eq!(value_column("Pages Out/s", "15"), paged_pool_column);
    assert!(
        !rendered
            .lines()
            .any(|line| { line.contains("Threads") && line.contains("Paged Pool") })
    );
}

#[test]
fn memory_uses_columns_and_gpu_uses_one_based_pages() {
    let mut app = make_test_app(3, 10);
    app.snapshot
        .gpu_adapters
        .extend(std::iter::repeat_with(model::GpuAdapterSample::default).take(2));

    let memory = render_app_to_text(&app, 180, 30);
    assert!(memory.contains("Pages Out/s"), "{memory}");
    assert!(!memory.contains("MEM 1/2"), "{memory}");
    app.set_screen_area(Rect::new(0, 0, 180, 30));
    app.select_next_resource_page();
    assert_eq!(app.selected_system_metric(), SystemMetric::PagedPool);
    assert_eq!(app.status, "MEM row: Paged Pool");

    app.select_resource_panel(app::ResourcePanel::Gpu);
    let gpu_first = render_app_to_text(&app, 180, 30);
    assert!(gpu_first.contains("GPU  1/2"), "{gpu_first}");

    app.select_next_resource_page();
    let gpu_second = render_app_to_text(&app, 180, 30);
    assert!(gpu_second.contains("GPU  2/2"), "{gpu_second}");
    assert_eq!(app.status, "GPU adapter 2/2");
}

#[test]
fn memory_column_navigation_clamps_to_the_shorter_pressure_column() {
    let mut app = make_test_app(3, 10);
    app.set_screen_area(Rect::new(0, 0, 180, 30));
    app.focused_panel = FocusedPanel::System;
    app.ram_vram_selected_index = SystemMetric::MEMORY_OVERVIEW_PANEL.len() - 1;

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::PagesOutput);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::FreeZeroedMemory);
}

#[test]
fn system_activity_panel_shows_network_disk_and_queue_metrics() {
    let mut app = make_test_app(3, 10);
    app.snapshot.network_received_bytes_per_sec = Some(30_000_000);
    app.snapshot.network_sent_bytes_per_sec = Some(40_000_000);
    app.snapshot.disk_read_bytes_per_sec = Some(10_000_000);
    app.snapshot.disk_write_bytes_per_sec = Some(20_000_000);
    app.snapshot.disk_queue_length = Some(1.5);

    let rendered = render_app_to_text(&app, 120, 30);

    assert!(rendered.contains("NW/DISK"), "{rendered}");
    assert!(
        rendered.find("MEM").unwrap() < rendered.find("NW/DISK").unwrap(),
        "{rendered}"
    );
    assert!(
        rendered.find("NW/DISK").unwrap() < rendered.find("CPU").unwrap(),
        "{rendered}"
    );
    assert!(rendered.contains("Net Rx   240 Mbps"), "{rendered}");
    assert!(rendered.contains("Net Tx   320 Mbps"), "{rendered}");
    assert!(rendered.contains("Disk R    10 MB/s"), "{rendered}");
    assert!(rendered.contains("Disk W    20 MB/s"), "{rendered}");
    assert!(rendered.contains("Disk Q     2"), "{rendered}");
}

#[test]
fn system_activity_space_assigns_graph_and_colors_the_value() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::SystemActivity;
    app.snapshot.disk_queue_length = Some(91.0);
    app.system_history.record_snapshot(&app.snapshot);

    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.selected_system_activity_metric(),
        SystemMetric::DiskQueueLength
    );

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::DiskQueueLength))
    );
    assert!(app.show_details);
    assert_eq!(
        app.active_graph_slot().map(GraphSlot::value_format),
        Some(GraphValueFormat::QueueLength)
    );
    assert_eq!(
        app.graph_slot_samples(app.active_graph_slot().unwrap())
            .last()
            .and_then(|sample| sample.value),
        Some(91.0)
    );

    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::system_activity_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("Disk Q    91"), "{rendered}");
    assert!(!rendered.contains("1  Disk Q"), "{rendered}");
    assert!(rendered.contains("Slot#1 · NW/DISK Disk Q"), "{rendered}");

    let (x, y) = find_text_position_in_area(&buffer, area, "91")
        .expect("registered NW/DISK value should render");
    let value = &buffer[(x, y)];
    assert_eq!(value.fg, app.theme().active_series);
    assert!(value.modifier.contains(Modifier::BOLD));
}

#[test]
fn process_enter_does_not_assign_graph_metric() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = ColumnPreset::Resources.columns().to_vec();
    app.selected_process_column_index = 4;
    app.select_process_index(2);
    app.details_target = DetailsTarget::System(SystemMetric::Committed);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.details_target,
        DetailsTarget::System(SystemMetric::Committed)
    );
    assert_eq!(app.details_metric, DetailsMetric::Private);
    assert!(!app.show_details);
    assert_eq!(
        app.selected_process_identity
            .as_ref()
            .map(|identity| identity.name.as_str()),
        Some("proc-2")
    );
    assert!(app.show_process_info_dialog);
    assert!(app.pending_process_info.is_some());

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_process_info_dialog);
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_process_info_dialog);
}

#[test]
fn selected_process_metric_column_updates_details_metric() {
    let mut app = make_test_app(3, 10);
    app.process_columns = ColumnPreset::Resources.columns().to_vec();
    app.selected_process_column_index = 3;

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::ThreadCount)
    );
    assert_eq!(app.details_metric, DetailsMetric::Private);
}

#[test]
fn full_path_column_is_not_graphable() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = vec![MetricColumn::FullPath];
    app.selected_process_column_index = 2;
    app.select_process_index(0);

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert!(app.graph_entries.is_empty());
    assert_eq!(app.details_metric, DetailsMetric::Private);
    assert_eq!(app.status, "Select a graphable metric cell");
}

#[test]
fn sort_uses_selected_process_metric_column() {
    let mut app = make_test_app(3, 10);
    app.snapshot.processes[0].private_bytes = Some(10);
    app.snapshot.processes[1].private_bytes = Some(30);
    app.snapshot.processes[2].private_bytes = Some(20);
    app.selected_process_column_index = 2;

    app.cycle_sort_column();

    assert_eq!(
        app.sort.column,
        SortColumn::Metric(MetricColumn::PrivateBytes)
    );
    assert_eq!(app.snapshot.processes[0].private_bytes, Some(30));
    assert!(!app.is_display_paused());
}

#[test]
fn sort_uses_selected_pid_column() {
    let mut app = make_test_app(3, 10);
    app.snapshot.processes[0].pid = 30;
    app.snapshot.processes[1].pid = 10;
    app.snapshot.processes[2].pid = 20;
    app.selected_process_column_index = 0;

    app.cycle_sort_column();

    assert_eq!(app.sort.column, SortColumn::Pid);
    assert_eq!(app.sort.direction, SortDirection::Asc);
    assert_eq!(app.snapshot.processes[0].pid, 10);
    assert!(!app.is_display_paused());
}

#[test]
fn sort_uses_selected_process_name_column() {
    let mut app = make_test_app(3, 10);
    app.snapshot.processes[0].name = "zeta.exe".to_string();
    app.snapshot.processes[1].name = "alpha.exe".to_string();
    app.snapshot.processes[2].name = "mid.exe".to_string();
    app.selected_process_column_index = 1;

    app.cycle_sort_column();

    assert_eq!(app.sort.column, SortColumn::ProcessName);
    assert_eq!(app.sort.direction, SortDirection::Asc);
    assert_eq!(app.snapshot.processes[0].name, "alpha.exe");
    assert!(!app.is_display_paused());
}

#[test]
fn sample_refresh_resorts_process_rows_when_order_is_unlocked() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(3, 10, sampling_worker);
    app.snapshot.processes[0].private_bytes = Some(10);
    app.snapshot.processes[1].private_bytes = Some(30);
    app.snapshot.processes[2].private_bytes = Some(20);
    app.selected_process_column_index = 2;
    app.cycle_sort_column();
    let sorted_pids = app
        .snapshot
        .processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    assert_eq!(sorted_pids, vec![1, 2, 0]);

    let mut next = test_snapshot(3);
    next.processes[0].private_bytes = Some(100);
    next.processes[1].private_bytes = Some(30);
    next.processes[2].private_bytes = Some(20);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();

    app.poll_sample_results().unwrap();

    let refreshed_pids = app
        .snapshot
        .processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    assert_eq!(refreshed_pids, vec![0, 1, 2]);
}

#[test]
fn sample_refresh_keeps_process_order_while_navigation_is_active() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(3, 10, sampling_worker);
    app.snapshot.processes[0].private_bytes = Some(10);
    app.snapshot.processes[1].private_bytes = Some(30);
    app.snapshot.processes[2].private_bytes = Some(20);
    app.selected_process_column_index = 2;
    app.cycle_sort_column();
    app.select_first_row();
    app.move_selection_down(1);
    assert_eq!(app.process_table_state.selected(), Some(1));
    let sorted_pids = app
        .snapshot
        .processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    assert_eq!(sorted_pids, vec![1, 2, 0]);

    let mut next = test_snapshot(3);
    next.processes[0].private_bytes = Some(100);
    next.processes[1].private_bytes = Some(30);
    next.processes[2].private_bytes = Some(20);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();

    app.poll_sample_results().unwrap();

    let refreshed_pids = app
        .snapshot
        .processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    assert_eq!(refreshed_pids, vec![1, 2, 0]);
    assert_eq!(app.process_table_state.selected(), Some(1));
}

#[test]
fn paused_display_freezes_visible_metrics_while_histories_continue() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(3, 10, sampling_worker);
    app.snapshot.used_memory = 10;
    app.snapshot.processes[0].private_bytes = Some(10);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.system_history.record_snapshot(&app.snapshot);
    app.rebuild_visible_process_cache();
    let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);

    app.toggle_display_pause();
    let mut next = test_snapshot(3);
    next.used_memory = 99;
    next.processes[0].private_bytes = Some(99);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();

    assert!(!app.poll_sample_results().unwrap());

    assert_eq!(app.snapshot.used_memory, 99);
    assert_eq!(app.snapshot.processes[0].private_bytes, Some(99));
    assert_eq!(app.display_snapshot().used_memory, 10);
    assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(10));
    assert_eq!(app.process_history.sample_count_for(&identity), 2);
    assert_eq!(app.display_process_history().sample_count_for(&identity), 1);
    assert_eq!(app.system_history.len(), 2);
    assert_eq!(app.display_system_history().len(), 1);
}

#[test]
fn unpausing_display_resumes_latest_snapshot() {
    let (sampling_worker, _request_rx, result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(3, 10, sampling_worker);
    app.snapshot.processes[0].private_bytes = Some(10);
    app.rebuild_visible_process_cache();
    app.toggle_display_pause();

    let mut next = test_snapshot(3);
    next.processes[0].private_bytes = Some(99);
    app.sampling_in_progress = true;
    result_tx
        .send(CollectSnapshotResult {
            snapshot: next,
            warning: None,
        })
        .unwrap();
    app.poll_sample_results().unwrap();
    assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(10));

    app.toggle_display_pause();

    assert_eq!(app.visible_process_at(0).unwrap().private_bytes, Some(99));
    assert!(!app.is_display_paused());
    assert_eq!(app.status, "Display resumed");
}

#[test]
fn ctrl_p_toggles_display_pause_from_any_panel() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;

    app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.is_display_paused());
    assert_eq!(app.status, "Display paused");

    app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(!app.is_display_paused());
    assert_eq!(app.status, "Display resumed");
}

#[test]
fn l_does_not_toggle_display_pause() {
    let mut app = make_test_app(3, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
        .unwrap();

    assert!(!app.is_display_paused());
}

#[test]
fn ab_keys_set_points_instead_of_starting_filter() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.ab_comparison.as_ref().and_then(|ab| ab.b).is_some());
    assert!(!app.filter_editing);
}

#[test]
fn ab_clear_key_clears_comparison() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.ab_comparison.is_none());
    assert!(app.status.contains("cleared"));
}

#[test]
fn ab_keys_keep_current_focus() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::Processes;
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.focused_panel, FocusedPanel::Processes);
}

#[test]
fn shifted_ab_keys_jump_selection_to_points() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    let base = Local::now();
    for (seconds, value) in [(0, 10), (1, 20), (2, 30)] {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(value);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }

    app.set_details_sample_selected(0);
    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    app.set_details_sample_selected(2);
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();
    app.set_details_sample_selected(1);

    app.on_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.details_sample_selected, 0);

    app.on_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.details_sample_selected, 2);
}

#[test]
fn ab_key_does_not_open_details_panel() {
    let mut app = make_test_app(1, 10);
    let status = app.status.clone();

    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.ab_comparison.is_none());
    assert!(!app.show_details);
    assert_eq!(app.status, status);
}

#[test]
fn compact_memory_navigation_excludes_hidden_pressure_column_after_resize() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    app.set_screen_area(Rect::new(0, 0, 180, 30));
    app.on_key(KeyCode::Right.into()).unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::PagedPool);
    app.on_key(KeyCode::Char(' ').into()).unwrap();
    app.set_screen_area(Rect::new(0, 0, 120, 30));
    assert_eq!(app.selected_system_metric(), SystemMetric::PhysicalMemory);
    app.on_key(KeyCode::Right.into()).unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::PhysicalMemory);
    let area = ui::ram_vram_panel_area_for_screen(app.last_screen_area, &app);
    for x in area.x..area.right() {
        assert_ne!(
            ui::memory_metric_at_position(app.last_screen_area, &app, x, area.y + 1),
            Some(SystemMetric::PagedPool)
        );
    }
    app.set_screen_area(Rect::new(0, 0, 180, 30));
    app.on_key(KeyCode::Right.into()).unwrap();
    assert_eq!(app.selected_system_metric(), SystemMetric::PagedPool);
    assert_eq!(app.graph_entries.len(), 1);
}

#[test]
fn compact_summary_render_evidence() {
    let mut app = make_test_app(3, 10);
    app.process_columns = model::MetricColumn::ALL.to_vec();
    app.snapshot.used_memory = 12_345_000_000;
    app.snapshot.total_memory = 64_000_000_000;
    app.snapshot.modified_memory = Some(u64::MAX);
    app.snapshot.standby_memory = None;
    app.snapshot.thread_count = Some(u64::MAX);
    app.snapshot.network_received_bytes_per_sec = Some(u64::MAX);
    app.filter_text = "no-match-長い-filter-abcdefghijklmnopqrstuvwxyz-TAIL".into();
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    for width in [80, 120, 240] {
        let rendered = render_app_to_text(&app, width, 24);
        assert!(rendered.contains("Filter(Ctrl+F)"), "{rendered}");
        assert!(rendered.contains("0 visible"), "{rendered}");
        assert!(rendered.contains("--"), "{rendered}");
        assert!(
            rendered.contains("18.4EB") || rendered.contains("18,446,744,073,710 MB"),
            "{rendered}"
        );
        println!("COMPACT EVIDENCE {width}x24\n{rendered}");
    }
    app.begin_filter_edit();
    assert!(app.filter_draft.ends_with("TAIL"));
    let rendered = render_app_to_text(&app, 80, 24);
    assert!(rendered.contains("TAIL"), "{rendered}");
}

#[test]
fn panel_selection_feedback_tracks_keyboard_and_mouse_focus() {
    use std::time::{Duration, Instant};
    for (width, height) in [(80, 24), (180, 60)] {
        let mut app = make_test_app(3, 10);
        let screen = Rect::new(0, 0, width, height);
        app.set_screen_area(screen);
        let now = Instant::now();
        app.focused_panel = FocusedPanel::System;
        app.select_resource_panel(app::ResourcePanel::Gpu);
        app.refresh_status_feedback(now);
        assert!(app.status.starts_with("GPU row: "));
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        app.refresh_status_feedback(now);
        assert_eq!(app.focused_panel, FocusedPanel::SystemActivity);
        assert!(app.status.is_empty());
        assert!(!render_app_to_text(&app, width, height).contains("GPU row: "));
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
            .unwrap();
        app.refresh_status_feedback(now);
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        assert!(app.status.is_empty());

        // Border clicks change focus without activating a metric or process action.
        for (panel, resource, area) in [
            (
                FocusedPanel::System,
                Some(app::ResourcePanel::Memory),
                ui::ram_vram_panel_area_for_screen(screen, &app),
            ),
            (
                FocusedPanel::System,
                Some(app::ResourcePanel::Gpu),
                ui::gpu_panel_area_for_screen(screen, &app),
            ),
            (
                FocusedPanel::SystemActivity,
                None,
                ui::system_activity_panel_area_for_screen(screen, &app),
            ),
            (
                FocusedPanel::Cpu,
                None,
                ui::cpu_panel_area_for_screen(screen, &app),
            ),
            (
                FocusedPanel::Processes,
                None,
                ui::main_panel_areas_for_app(screen, &app).processes.area,
            ),
        ] {
            app.on_mouse(left_click(area.x, area.y), screen);
            app.refresh_status_feedback(now);
            assert_eq!(app.focused_panel, panel);
            if let Some(resource) = resource {
                assert_eq!(app.resource_panel, resource);
            }
            assert!(app.status.starts_with("Focus:"));
            assert!(render_app_to_text(&app, width, height).contains(&app.status));
        }

        // Every resource row message has the short lifetime, even without sampling.
        for (panel, resource, message) in [
            (
                FocusedPanel::System,
                app::ResourcePanel::Memory,
                "MEM row: Physical",
            ),
            (
                FocusedPanel::System,
                app::ResourcePanel::Gpu,
                "GPU row: Usage",
            ),
            (
                FocusedPanel::System,
                app::ResourcePanel::Gpu,
                "GPU adapter 1/1",
            ),
            (
                FocusedPanel::SystemActivity,
                app::ResourcePanel::Gpu,
                "NW/DISK row: Net Rx",
            ),
            (FocusedPanel::Cpu, app::ResourcePanel::Gpu, "CPU row: Usage"),
        ] {
            app.focused_panel = panel;
            app.resource_panel = resource;
            app.status = message.into();
            app.refresh_status_feedback(now);
            assert_eq!(app.status, message);
            app.refresh_status_feedback(now + Duration::from_secs(2));
            assert!(app.status.is_empty());
            app.status = message.into();
            app.refresh_status_feedback(now);
            app.focused_panel = FocusedPanel::Processes;
            app.refresh_status_feedback(now);
            assert!(app.status.is_empty());
        }
    }
}

#[test]
fn panel_focus_feedback_cleanup_preserves_action_results_and_warnings() {
    use std::time::{Duration, Instant};
    for message in [
        "Warning: sampling worker stopped",
        "Copied row: proc-0",
        "Exported Graph CSV",
    ] {
        let mut app = make_test_app(3, 10);
        app.set_screen_area(Rect::new(0, 0, 180, 60));
        app.focused_panel = FocusedPanel::System;
        app.resource_panel = app::ResourcePanel::Gpu;
        app.status = message.into();
        let now = Instant::now();
        app.refresh_status_feedback(now);
        for key in [KeyCode::Right, KeyCode::Right, KeyCode::Down] {
            app.on_key(KeyEvent::new(key, KeyModifiers::CONTROL))
                .unwrap();
            app.refresh_status_feedback(now + Duration::from_secs(1));
            assert_eq!(app.status, message);
        }
        assert_eq!(app.focused_panel, FocusedPanel::Processes);
        app.refresh_status_feedback(now + Duration::from_secs(5));
        assert_eq!(app.status, message);
        app.refresh_status_feedback(now + Duration::from_secs(6));
        assert!(app.status.is_empty());
    }
}
