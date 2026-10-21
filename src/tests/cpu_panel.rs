use super::support::{
    add_test_graph, buffer_to_text, find_text_position_in_area, left_click, make_test_app,
    mouse_move, render_app_to_buffer, render_app_to_text,
};
use crate::app;
use crate::app::{FocusedPanel, GraphSlot, GraphValueFormat};
use crate::model::{CpuCoreKind, CpuLogicalProcessorSample, SystemMetric};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

#[test]
fn cpu_panel_renders_compact_summary_and_per_core_control() {
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_total_usage_percent = Some(42);
    app.snapshot.cpu_user_usage_percent = Some(31);
    app.snapshot.cpu_kernel_usage_percent = Some(11);
    app.snapshot.cpu_p_core_frequency_mhz = Some(3_200);
    app.snapshot.cpu_e_core_frequency_mhz = Some(1_800);
    app.snapshot.thread_count = Some(4_335);
    app.snapshot.cpu_logical_processors = vec![
        CpuLogicalProcessorSample {
            usage_percent: 1,
            kind: Some(CpuCoreKind::Performance),
        },
        CpuLogicalProcessorSample {
            usage_percent: 22,
            kind: Some(CpuCoreKind::Performance),
        },
        CpuLogicalProcessorSample {
            usage_percent: 99,
            kind: Some(CpuCoreKind::Efficiency),
        },
    ];

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(rendered.contains("CPU"), "{rendered}");
    assert!(
        rendered.contains("Usage       42% (U 31%, K 11%)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Freq(P/E)  3200 MHz / 1800 MHz"),
        "{rendered}"
    );
    assert!(rendered.contains("[Per-core Usage (P/E)]"), "{rendered}");
    assert!(rendered.contains("Threads    4,335"), "{rendered}");
    assert!(rendered.contains("Processes  3"), "{rendered}");
    assert!(!rendered.contains("P ▁▂"), "{rendered}");

    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::cpu_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (_, threads_y) = find_text_position_in_area(&buffer, area, "Threads").unwrap();
    let (_, processes_y) = find_text_position_in_area(&buffer, area, "Processes").unwrap();
    let (_, per_core_y) =
        find_text_position_in_area(&buffer, area, "[Per-core Usage (P/E)]").unwrap();
    assert!(threads_y < processes_y && processes_y < per_core_y);
}

#[test]
fn cpu_frequency_shows_placeholder_when_no_e_core_exists() {
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_p_core_frequency_mhz = Some(3_200);
    app.snapshot.cpu_e_core_frequency_mhz = Some(1_800);
    app.snapshot.cpu_logical_processors = vec![CpuLogicalProcessorSample {
        usage_percent: 25,
        kind: Some(CpuCoreKind::Performance),
    }];

    let rendered = render_app_to_text(&app, 120, 30);

    assert!(rendered.contains("Freq(P/E)  3200 MHz / --"), "{rendered}");
    assert!(!rendered.contains("1800 MHz"), "{rendered}");
}

#[test]
fn cpu_frequency_renders_unavailable_hybrid_clocks() {
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_p_core_frequency_mhz = Some(3200);
    app.snapshot.cpu_e_core_frequency_mhz = None;
    app.snapshot.cpu_logical_processors = vec![CpuLogicalProcessorSample {
        usage_percent: 25,
        kind: Some(CpuCoreKind::Efficiency),
    }];
    let rendered = render_app_to_text(&app, 120, 30);
    assert!(rendered.contains("Freq(P/E)  3200 MHz / --"), "{rendered}");
    app.snapshot.cpu_p_core_frequency_mhz = None;
    app.snapshot.cpu_current_frequency_mhz = None;
    let rendered = render_app_to_text(&app, 120, 30);
    assert!(rendered.contains("Freq(P/E)  -- / --"), "{rendered}");
}

#[test]
fn cpu_panel_width_does_not_grow_with_logical_cpu_count() {
    let screen = Rect::new(0, 0, 180, 30);
    let mut small = make_test_app(3, 10);
    small.snapshot.cpu_logical_processors = vec![CpuLogicalProcessorSample {
        usage_percent: 25,
        kind: Some(CpuCoreKind::Performance),
    }];
    let mut large = make_test_app(3, 10);
    large.snapshot.cpu_logical_processors = vec![
        CpuLogicalProcessorSample {
            usage_percent: 25,
            kind: Some(CpuCoreKind::Performance),
        };
        128
    ];

    assert_eq!(
        ui::cpu_panel_area_for_screen(screen, &small).width,
        ui::cpu_panel_area_for_screen(screen, &large).width
    );
}

#[test]
fn cpu_per_core_control_hovers_and_opens_a_scrollable_dialog() {
    let screen = Rect::new(0, 0, 100, 15);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        app.snapshot.cpu_logical_processors = (0..128)
            .map(|index| CpuLogicalProcessorSample {
                usage_percent: index as u8,
                kind: Some(if index % 2 == 0 {
                    CpuCoreKind::Performance
                } else {
                    CpuCoreKind::Efficiency
                }),
            })
            .collect();
        app.focused_panel = FocusedPanel::Cpu;
        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        assert!(app.cpu_per_core_selected());
        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_cpu_metric(), Some(SystemMetric::ProcessCount));
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert!(app.cpu_per_core_selected());
        let button = ui::cpu_per_core_button_area(ui::cpu_panel_area_for_screen(screen, &app))
            .expect("per-core button should render");

        let selected = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(
            selected[(button.x, button.y)].bg,
            theme.table_selection_surface
        );

        app.on_mouse(mouse_move(button.x, button.y), screen);
        let hovered = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(hovered[(button.x, button.y)].bg, theme.focus_surface);
        assert!(
            hovered[(button.x, button.y)]
                .modifier
                .contains(Modifier::BOLD)
        );

        app.on_mouse(mouse_move(0, screen.height - 1), screen);
        assert!(!app.cpu_per_core_hovered);
        let unhovered = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(
            unhovered[(button.x, button.y)].bg,
            theme.table_selection_surface
        );

        app.on_mouse(left_click(button.x, button.y), screen);
        assert!(app.show_cpu_core_dialog);
        assert_eq!(app.focused_panel, FocusedPanel::Cpu);
        assert!(app.graph_entries.is_empty());
        app::sync_layout_state(&mut app, screen);

        app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        assert!(app.cpu_core_scroll.offset > 0);
        let dialog = render_app_to_text(&app, screen.width, screen.height);
        assert!(dialog.contains("PER-CORE CPU USAGE"), "{dialog}");
        assert!(dialog.contains("CPU 127 (E)"), "{dialog}");
        assert!(dialog.contains("Enter/Esc Close"), "{dialog}");

        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_cpu_core_dialog);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.show_cpu_core_dialog);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.show_cpu_core_dialog);
    }
}

#[test]
fn cpu_panel_space_assigns_cpu_average_graph() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Cpu;
    app.snapshot.cpu_total_usage_percent = Some(42);
    app.system_history.record_snapshot(&app.snapshot);

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::CpuAverage))
    );
    assert!(app.show_details);
    assert_eq!(
        app.active_graph_slot().map(GraphSlot::value_format),
        Some(GraphValueFormat::Percent)
    );
    assert_eq!(
        app.graph_slot_samples(app.active_graph_slot().unwrap())[0].value,
        Some(42.0)
    );

    let screen = Rect::new(0, 0, 180, 45);
    let area = ui::cpu_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("Usage       42%"), "{rendered}");
    assert!(!rendered.contains("1  Usage"), "{rendered}");
    assert!(rendered.contains("Slot#1 · CPU Usage"), "{rendered}");

    let (x, y) = find_text_position_in_area(&buffer, area, "42%")
        .expect("registered CPU value should render");
    let value = &buffer[(x, y)];
    assert_eq!(value.fg, app.theme().active_series);
    assert!(value.modifier.contains(Modifier::BOLD));
}

#[test]
fn cpu_panel_can_select_graph_and_copy_threads() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Cpu;
    app.snapshot.thread_count = Some(4_335);
    app.system_history.record_snapshot(&app.snapshot);

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_cpu_metric(), Some(SystemMetric::ThreadCount));
    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::ThreadCount))
    );
    assert_eq!(
        app.graph_slot_samples(app.active_graph_slot().unwrap())[0].value,
        Some(4_335.0)
    );

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("Threads\t4,335")
    );
}

#[test]
fn cpu_panel_can_select_graph_and_copy_processes() {
    let mut app = make_test_app(214, 10);
    app.focused_panel = FocusedPanel::Cpu;
    app.system_history.record_snapshot(&app.snapshot);

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_cpu_metric(), Some(SystemMetric::ProcessCount));

    app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.active_graph_slot(),
        Some(&GraphSlot::system(SystemMetric::ProcessCount))
    );
    assert_eq!(
        app.active_graph_slot().map(GraphSlot::value_format),
        Some(GraphValueFormat::Count)
    );
    assert_eq!(
        app.graph_slot_samples(app.active_graph_slot().unwrap())[0].value,
        Some(214.0)
    );

    let screen = Rect::new(0, 0, 180, 45);
    let area = ui::cpu_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let metric_view = buffer_to_text(&buffer);
    assert!(metric_view.contains("Space Graph"), "{metric_view}");
    let (x, y) = find_text_position_in_area(&buffer, area, "214")
        .expect("registered Processes value should render");
    assert_eq!(buffer[(x, y)].fg, app.theme().active_series);
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));

    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        app::clipboard::last_copied_text().as_deref(),
        Some("Processes\t214")
    );

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_cpu_core_dialog);

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_cpu_metric(), None);
    assert!(app.cpu_per_core_selected());
    let per_core_view = render_app_to_text(&app, screen.width, screen.height);
    assert!(per_core_view.contains("Enter Open"), "{per_core_view}");
    assert!(!per_core_view.contains("Space Graph"), "{per_core_view}");
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_cpu_core_dialog);
}

#[test]
fn cpu_inactive_graph_colors_the_value_without_bold_or_an_ordinal() {
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_total_usage_percent = Some(73);
    let ids = (0..9)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::system(SystemMetric::CpuAverage),
        FocusedPanel::Cpu,
    ));
    assert!(app.set_active_graph(ids[0]));

    let screen = Rect::new(0, 0, 180, 45);
    let area = ui::cpu_panel_area_for_screen(screen, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position_in_area(&buffer, area, "73%")
        .expect("registered CPU value should render");
    let value = &buffer[(x, y)];

    assert_eq!(value.fg, app.theme().active_series);
    assert!(!value.modifier.contains(Modifier::BOLD));
    assert!(find_text_position_in_area(&buffer, area, "10 Usage").is_none());
}

#[test]
fn cpu_graph_values_style_percent_missing_and_count_tokens_in_all_themes() {
    let screen = Rect::new(0, 0, 180, 45);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        app.snapshot.cpu_total_usage_percent = Some(42);
        app.snapshot.thread_count = None;

        let usage = GraphSlot::system(SystemMetric::CpuAverage);
        for slot in [
            usage.clone(),
            GraphSlot::system(SystemMetric::ThreadCount),
            GraphSlot::system(SystemMetric::ProcessCount),
        ] {
            assert!(app.add_or_reveal_graph_source(slot, FocusedPanel::Cpu));
        }
        let usage_id = app
            .graph_entries
            .iter()
            .find(|entry| entry.source == usage)
            .unwrap()
            .id;
        assert!(app.set_active_graph(usage_id));
        app.show_details = false;
        app.focused_panel = FocusedPanel::Cpu;

        let area = ui::cpu_panel_area_for_screen(screen, &app);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (usage_x, usage_y) = find_text_position_in_area(&buffer, area, "42%").unwrap();
        for offset in 0..3 {
            let cell = &buffer[(usage_x + offset, usage_y)];
            assert_eq!(cell.fg, theme.active_series);
            assert_eq!(cell.bg, theme.table_selection_surface);
            assert!(cell.modifier.contains(Modifier::UNDERLINED));
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
        assert_eq!(buffer[(usage_x - 1, usage_y)].symbol(), " ");
        assert!(
            !buffer[(usage_x - 1, usage_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        let (_, threads_y) = find_text_position_in_area(&buffer, area, "Threads").unwrap();
        let threads_row = Rect::new(area.x, threads_y, area.width, 1);
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, threads_row, "--").unwrap();
        for offset in 0..2 {
            let cell = &buffer[(missing_x + offset, missing_y)];
            assert_eq!(cell.fg, theme.active_series);
            assert_eq!(cell.bg, theme.panel);
            assert!(cell.modifier.contains(Modifier::UNDERLINED));
            assert!(!cell.modifier.contains(Modifier::BOLD));
        }
        assert_eq!(buffer[(missing_x - 1, missing_y)].symbol(), " ");
        assert!(
            !buffer[(missing_x - 1, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        let (_, processes_y) = find_text_position_in_area(&buffer, area, "Processes").unwrap();
        let processes_row = Rect::new(area.x, processes_y, area.width, 1);
        let (count_x, count_y) = find_text_position_in_area(&buffer, processes_row, "3").unwrap();
        let count = &buffer[(count_x, count_y)];
        assert_eq!(count.fg, theme.active_series);
        assert_eq!(count.bg, theme.panel);
        assert!(count.modifier.contains(Modifier::UNDERLINED));
        assert!(!count.modifier.contains(Modifier::BOLD));
    }
}

#[test]
fn clicking_cpu_panel_moves_focus_to_cpu() {
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 120, 45);
    let area = ui::cpu_panel_area_for_screen(screen, &app);

    app.on_mouse(left_click(area.x + 1, area.y + 1), screen);

    assert_eq!(app.focused_panel, FocusedPanel::Cpu);
    assert_eq!(app.status, "CPU row: CPU Usage");
}

#[test]
fn cpu_per_core_grid_wraps_in_index_order_without_clipping() {
    for count in [1, 3, 20, 21, 128] {
        let screen = Rect::new(0, 0, 120, 60);
        let mut app = make_test_app(3, 10);
        app.snapshot.cpu_logical_processors = vec![
            CpuLogicalProcessorSample {
                usage_percent: 100,
                kind: None,
            };
            count
        ];
        app.open_cpu_core_dialog();
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let content = ui::cpu_core_dialog_content_area(screen, &app);
        let columns = count.min(4);
        assert_eq!(content.height as usize, count.div_ceil(columns));
        assert!(
            ui::cpu_core_dialog_scrollbar_area(screen, &app, app.cpu_core_scroll.page_size)
                .is_none()
        );
        let index_width = (count - 1).to_string().len();
        for index in 0..count {
            let label = format!("CPU {index:>index_width$} (-)  100%");
            let (x, y) = find_text_position_in_area(&buffer, content, &label).expect(&label);
            assert_eq!(y, content.y + (index / columns) as u16);
            assert_eq!(
                x,
                content.x + ((index % columns) * (label.len() + 3)) as u16
            );
        }
        for x in content.x..content.right() {
            assert_eq!(buffer[(x, content.bottom())].symbol(), " ");
        }
        assert!(buffer_to_text(&buffer).contains("↑/↓ Rows"));
    }
}

#[test]
fn cpu_per_core_grid_scrolls_by_rows_and_clamps_after_resize() {
    let narrow = Rect::new(0, 0, 64, 20);
    let wide = Rect::new(0, 0, 120, 60);
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_logical_processors = vec![
        CpuLogicalProcessorSample {
            usage_percent: 100,
            kind: Some(CpuCoreKind::Performance),
        };
        128
    ];
    app.open_cpu_core_dialog();
    app::sync_layout_state(&mut app, narrow);
    let content = ui::cpu_core_dialog_content_area(narrow, &app);
    let buffer = render_app_to_buffer(&app, narrow.width, narrow.height);
    let (last_x, last_y) =
        find_text_position_in_area(&buffer, content, "CPU   2 (P)  100%").unwrap();
    assert_eq!(last_y, content.y);
    assert!(last_x + 17 <= content.right());
    assert_eq!(ui::cpu_core_dialog_total_rows(narrow, &app), 43);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: content.x,
            row: content.y,
            modifiers: KeyModifiers::NONE,
        },
        narrow,
    );
    assert_eq!(app.cpu_core_scroll.offset, 1);
    let buffer = render_app_to_buffer(&app, narrow.width, narrow.height);
    assert_eq!(
        find_text_position_in_area(&buffer, content, "CPU   3 (P)  100%")
            .unwrap()
            .1,
        content.y
    );
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.cpu_core_scroll.offset,
        43 - app.cpu_core_scroll.page_size
    );
    assert!(render_app_to_text(&app, narrow.width, narrow.height).contains("CPU 127 (P)  100%"));
    app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.cpu_core_scroll.offset, app.cpu_core_scroll.page_size);
    app.on_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.cpu_core_scroll.offset, 0);
    let scrollbar =
        ui::cpu_core_dialog_scrollbar_area(narrow, &app, app.cpu_core_scroll.page_size).unwrap();
    app.on_mouse(left_click(scrollbar.x, scrollbar.bottom() - 1), narrow);
    assert_eq!(
        app.cpu_core_scroll.offset,
        43 - app.cpu_core_scroll.page_size
    );
    app::sync_layout_state(&mut app, wide);
    assert_eq!(app.cpu_core_scroll.offset, 0);
    assert_eq!(app.cpu_core_scroll.page_size, 32);
    assert!(
        ui::cpu_core_dialog_scrollbar_area(wide, &app, app.cpu_core_scroll.page_size).is_none()
    );
    assert!(render_app_to_text(&app, wide.width, wide.height).contains("CPU 127 (P)  100%"));
}

#[test]
fn cpu_per_core_grid_keeps_empty_and_log_view_messages() {
    let screen = Rect::new(0, 0, 120, 60);
    let mut app = make_test_app(3, 10);
    app.snapshot.cpu_logical_processors.clear();
    app.open_cpu_core_dialog();
    for (log_path, message) in [
        (None, "Per-core usage is unavailable."),
        (
            Some(std::path::PathBuf::from("test.jsonl")),
            "Per-core usage is not recorded in logs.",
        ),
    ] {
        app.log_view_path = log_path;
        app::sync_layout_state(&mut app, screen);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let content = ui::cpu_core_dialog_content_area(screen, &app);
        assert_eq!(content.height, 1);
        assert!(find_text_position_in_area(&buffer, content, message).is_some());
    }
}
