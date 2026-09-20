use super::support::{
    assert_blank_row_above_text, assert_modal_rect_focus_border, assign_private_graph,
    buffer_to_text, find_text_position, make_test_app, populate_system_info, render_app_to_buffer,
    render_app_to_text,
};
use crate::app;
use crate::app::FocusedPanel;
use crate::model;
use crate::model::{ColumnPreset, MetricColumn};
use crate::ui;
use crate::ui::{column_picker_area, column_picker_scrollbar_area};
use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

#[test]
fn column_picker_toggles_visible_columns() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_column_picker);

    app.column_picker_index = 0;
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert!(app.process_columns.contains(&MetricColumn::CpuPercent));
    assert_eq!(app.column_preset, ColumnPreset::Custom);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_column_picker);
}

#[test]
fn column_picker_mouse_click_toggles_clicked_column() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![MetricColumn::PrivateBytes];
    app.show_column_picker = true;

    let buffer = render_app_to_buffer(&app, 100, 45);
    let (x, y) = find_text_position(&buffer, "CPU%")
        .expect("CPU column row should be rendered in the picker");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 100, 45),
    );

    assert_eq!(app.column_picker_index, 0);
    assert!(app.process_columns.contains(&MetricColumn::CpuPercent));
    assert_eq!(app.column_preset, ColumnPreset::Custom);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 100, 45),
    );

    assert!(!app.process_columns.contains(&MetricColumn::CpuPercent));
    assert_eq!(app.process_columns, vec![MetricColumn::PrivateBytes]);
}

#[test]
fn column_picker_scrollbar_drag_scrolls_content() {
    let mut app = make_test_app(1, 10);
    app.show_column_picker = true;
    let screen = Rect::new(0, 0, 100, 10);
    app.set_column_picker_page_size(ui::column_picker_page_size_for_screen(screen));
    let scrollbar = column_picker_scrollbar_area(screen, app.column_picker_scroll.page_size)
        .expect("small column picker should have a scrollbar");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.column_picker_scroll.dragging);
    assert_eq!(app.column_picker_scroll.offset, 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.bottom().saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.column_picker_scroll.offset > 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.bottom().saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(!app.column_picker_scroll.dragging);
}

#[test]
fn column_picker_panel_fits_rendered_content_height() {
    let popup = column_picker_area(Rect::new(0, 0, 100, 45));

    assert_eq!(popup.height, MetricColumn::ALL.len() as u16 + 6);
}

#[test]
fn column_picker_header_uses_footer_like_shortcut_styles() {
    let mut app = make_test_app(1, 10);
    app.show_column_picker = true;

    let buffer = render_app_to_buffer(&app, 100, 45);
    let rendered = buffer_to_text(&buffer);
    let theme = ui::THEMES[0];

    assert!(!rendered.contains("Descriptions are concise"), "{rendered}");
    assert!(
        rendered.contains("↑/↓ select  Space toggle  Enter/Esc close"),
        "{rendered}"
    );
    assert!(!rendered.contains("[ Close ]"), "{rendered}");

    let (title_x, title_y) = find_text_position(&buffer, "Select process columns")
        .expect("column picker title should be rendered");
    assert_eq!(title_x, column_picker_area(Rect::new(0, 0, 100, 45)).x + 2);
    let title_cell = &buffer[(title_x, title_y)];
    assert_eq!(title_cell.fg, theme.text);
    assert_ne!(title_cell.fg, theme.accent);
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));

    let (key_x, key_y) =
        find_text_position(&buffer, "↑/↓").expect("shortcut key should be rendered");
    let key_cell = &buffer[(key_x, key_y)];
    assert_eq!(key_cell.fg, theme.key_hint);
    assert!(!key_cell.modifier.contains(ratatui::style::Modifier::BOLD));

    let label_cell = &buffer[(key_x + "↑/↓ ".chars().count() as u16, key_y)];
    assert_eq!(label_cell.fg, theme.text);
    assert_blank_row_above_text(&buffer, "↑/↓ select  Space toggle  Enter/Esc close");
}

#[test]
fn column_picker_takes_focus_border_from_previous_panel() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::Processes;

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
        .unwrap();

    assert_modal_rect_focus_border(&app, column_picker_area(Rect::new(0, 0, 100, 45)));
}

#[test]
fn number_keys_do_not_switch_column_presets() {
    let mut app = make_test_app(1, 10);
    let columns_before = app.process_columns.clone();

    app.on_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.column_preset, ColumnPreset::Default);
    assert_eq!(app.process_columns, columns_before);
}

#[test]
fn ctrl_c_copies_selected_process_row_text() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![
        MetricColumn::PrivateBytes,
        MetricColumn::WorksetPrivateBytes,
    ];
    app.snapshot.processes[0].private_bytes = Some(388_067_328);

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(!app.show_column_picker);
    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("0\tproc-0\t388,067,328\t--")
    );
    assert_eq!(app.status, "Copied process row: proc-0");
}

#[test]
fn ctrl_c_copies_selected_ram_vram_row_text() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::System;
    app.ram_vram_selected_index = 4;
    app.snapshot.committed_memory = Some(9_000_000_000);
    app.snapshot.commit_limit = Some(18_000_000_000);

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("Commit charge\t9,000 MB / 18,000 MB")
    );
    assert_eq!(app.status, "Copied row: Commit charge");
}

#[test]
fn ctrl_c_copies_cpu_average_row_text() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::Cpu;
    app.snapshot.cpu_total_usage_percent = Some(37);
    app.snapshot.cpu_user_usage_percent = Some(29);
    app.snapshot.cpu_kernel_usage_percent = Some(8);

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("CPU Usage\t37% (U 29%, K 8%)")
    );
    assert_eq!(app.status, "Copied row: CPU Usage");
}

#[test]
fn ctrl_c_copies_selected_sample_row_text_when_samples_are_focused() {
    let mut app = make_test_app(1, 10);
    let first = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
    let second = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 1).unwrap();
    let tracked = app.normalized_watch_names.clone();
    app.snapshot.captured_at = first;
    app.snapshot.processes[0].private_bytes = Some(100);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &tracked,
    );
    app.snapshot.captured_at = second;
    app.snapshot.processes[0].private_bytes = Some(1_234);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &tracked,
    );
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsSamples;
    app.details_sample_selected = 1;

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("10:00:01\t1,234\t+1,134")
    );
    assert_eq!(app.status, "Copied row: 10:00:01 PrivBytes=1,234");
}

#[test]
fn plain_c_opens_column_picker() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_column_picker);
}

#[test]
fn plain_i_opens_system_info_dialog() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_system_info_dialog);
    assert_eq!(app.process_info_cache.len(), 0);
    assert!(app.pending_process_info.is_none());

    let rendered = render_app_to_text(&app, 100, 20);
    assert_eq!(rendered.matches("SYSTEM INFO").count(), 1, "{rendered}");
    assert!(!rendered.contains("System Activity"), "{rendered}");
    assert!(!rendered.contains("[Per-core Usage (P/E)]"), "{rendered}");
    assert!(!rendered.contains("Net Rx"), "{rendered}");
    assert!(!rendered.contains("Disk Q"), "{rendered}");
    assert!(
        rendered.contains("Ctrl+C Copy  Enter/Esc Close"),
        "{rendered}"
    );

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.show_system_info_dialog);
    assert!(app.pending_process_info.is_none());
}

#[test]
fn system_info_displays_expanded_host_and_capacity_fields() {
    let mut app = make_test_app(1, 10);
    populate_system_info(&mut app);
    app.show_system_info_dialog = true;

    let rendered = render_app_to_text(&app, 100, 30);

    for expected in [
        "winproc-tui",
        env!("CARGO_PKG_VERSION"),
        "Windows",
        "Windows 11 Pro",
        "Build",
        "26100",
        "Architecture",
        "x64",
        "Test CPU / 2.10 GHz",
        "8 P-cores / 4 E-cores",
        "L3 25.0 MB",
        "Physical memory",
        "34.0 GB",
        "Commit limit",
        "51.0 GB",
        "GPU 1",
        "Dedicated 8.4 GB / Shared 17.0 GB",
        "Disk C",
        "123.0 GB free / 500.0 GB total",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}: {rendered}"
        );
    }
    assert!(
        rendered.contains("Ctrl+C Copy  Enter/Esc Close"),
        "{rendered}"
    );
}

#[test]
fn system_info_ctrl_c_copies_all_fields_even_when_dialog_is_clipped() {
    let mut app = make_test_app(1, 10);
    populate_system_info(&mut app);
    for letter in 'D'..='Z' {
        app.snapshot.disks.push(model::DiskUsageSample {
            name: format!("{letter}:"),
            free_bytes: 1_000_000_000,
            total_bytes: 2_000_000_000,
        });
    }
    app.show_system_info_dialog = true;
    let rendered = render_app_to_text(&app, 60, 12);
    assert!(!rendered.contains("Disk Z"), "{rendered}");

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    let copied = app::clipboard::last_copied_text().unwrap();
    assert!(copied.starts_with(&format!(
        "winproc-tui: {}\nWindows: Windows 11 Pro",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(copied.contains("Physical memory: 34.0 GB"), "{copied}");
    assert!(copied.contains("GPU 1: Test GPU"), "{copied}");
    assert!(
        copied.contains("Disk Z: 1.0 GB free / 2.0 GB total"),
        "{copied}"
    );
    assert!(!copied.contains("SYSTEM INFO"), "{copied}");
    assert!(app.show_system_info_dialog);
    assert_eq!(app.status, "Copied System Info (34 fields)");
}

#[test]
fn system_info_uses_latest_host_snapshot_while_paused_and_in_log_view() {
    let mut app = make_test_app(1, 10);
    populate_system_info(&mut app);
    app.snapshot.total_memory = 1_000_000_000;
    app.toggle_display_pause();
    app.snapshot.total_memory = 34_000_000_000;

    app.copy_system_info_to_clipboard().unwrap();
    let paused_copy = app::clipboard::last_copied_text().unwrap();
    assert!(
        paused_copy.contains("Physical memory: 34.0 GB"),
        "{paused_copy}"
    );
    assert!(
        !paused_copy.contains("Physical memory: 1.0 GB"),
        "{paused_copy}"
    );

    app.log_view_display = app.paused_display.take();
    app.log_view_path = Some(std::path::PathBuf::from("recording.log"));
    app.copy_system_info_to_clipboard().unwrap();
    let log_copy = app::clipboard::last_copied_text().unwrap();
    assert!(log_copy.contains("Physical memory: 34.0 GB"), "{log_copy}");
}
