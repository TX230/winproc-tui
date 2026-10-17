use super::support::{
    buffer_to_text, find_text_position, find_text_position_in_area, left_click, make_test_app,
    render_app_to_buffer, render_app_to_text, track_process_name,
};
use crate::app;
use crate::app::{App, DetailsMetric, FocusedPanel, GraphSlot};
use crate::model;
use crate::model::{
    ColumnPreset, MetricColumn, ProcessIdentity, SortColumn, SortDirection, SortSpec,
};
use crate::ui;
use crate::ui::{main_panel_areas_for_app, process_table_visible_column_count, screen_layout};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

#[test]
fn process_selection_tracks_identity_after_rows_reorder() {
    let mut app = make_test_app(4, 10);
    app.select_process_index(2);

    app.snapshot.processes.reverse();
    app.clamp_process_table_state();

    let selected = app
        .selected_visible_process()
        .expect("selected process should remain visible");
    assert_eq!(selected.name, "proc-2");
}

#[test]
fn left_right_selects_process_metric_column_when_processes_are_focused() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::WorksetPrivateBytes)
    );
    assert_eq!(app.details_metric, DetailsMetric::Private);
}

#[test]
fn left_right_selects_pid_and_process_columns() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.selected_process_column_index = 2;

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_process_column(), SortColumn::ProcessName);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_process_column(), SortColumn::Pid);

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected_process_column(), SortColumn::ProcessName);
}

#[test]
fn shift_left_right_reorders_selected_metric_column() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = vec![
        MetricColumn::CpuPercent,
        MetricColumn::PrivateBytes,
        MetricColumn::HandleCount,
    ];
    app.selected_process_column_index = 3;

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(
        app.process_columns,
        vec![
            MetricColumn::PrivateBytes,
            MetricColumn::CpuPercent,
            MetricColumn::HandleCount,
        ]
    );
    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::PrivateBytes)
    );
    assert_eq!(app.selected_process_column_index, 2);
    assert_eq!(app.column_preset, ColumnPreset::Custom);

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(
        app.process_columns,
        vec![
            MetricColumn::CpuPercent,
            MetricColumn::PrivateBytes,
            MetricColumn::HandleCount,
        ]
    );
    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::PrivateBytes)
    );
    assert_eq!(app.selected_process_column_index, 3);
}

#[test]
fn w_and_shift_w_adjust_selected_fixed_process_column_widths() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;

    app.selected_process_column_index = 0;
    app.process_metric_column_offset = usize::MAX;
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::Pid),
        SortColumn::Pid.default_width() + 1
    );
    assert_eq!(
        app.process_metric_column_offset,
        app.process_columns.len() - 1
    );

    app.selected_process_column_index = 1;
    app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::ProcessName),
        SortColumn::ProcessName.default_width() - 1
    );

    app.process_column_widths.set(
        SortColumn::ProcessName,
        SortColumn::ProcessName.default_width(),
    );
    app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::ProcessName),
        SortColumn::ProcessName.default_width() - 1
    );

    app.process_column_widths.set(
        SortColumn::ProcessName,
        SortColumn::ProcessName.default_width(),
    );
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::ProcessName),
        SortColumn::ProcessName.default_width() - 1
    );
}

#[test]
fn process_column_width_shortcuts_respect_limits_modifiers_focus_and_filter() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.selected_process_column_index = 0;

    app.process_column_widths.set(
        SortColumn::Pid,
        crate::model::columns::PROCESS_COLUMN_WIDTH_MAX,
    );
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::Pid),
        crate::model::columns::PROCESS_COLUMN_WIDTH_MAX
    );
    assert!(app.status.starts_with("Column width limit: PID"));

    app.process_column_widths
        .set(SortColumn::Pid, SortColumn::Pid.min_width());
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        app.process_column_widths.resolved(SortColumn::Pid),
        SortColumn::Pid.min_width()
    );

    let width = app.process_column_widths.resolved(SortColumn::Pid);
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT))
        .unwrap();
    assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

    app.focused_panel = FocusedPanel::System;
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

    app.focused_panel = FocusedPanel::Processes;
    app.begin_filter_edit();
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.filter_draft, "wW");
    assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_help);
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_column_widths.resolved(SortColumn::Pid), width);
}

#[test]
fn process_column_widths_follow_identity_without_changing_table_state() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = vec![
        MetricColumn::PrivateBytes,
        MetricColumn::FullPath,
        MetricColumn::HandleCount,
    ];
    app.selected_process_column_index = 2;
    let original_sort = app.sort;
    let original_details_metric = app.details_metric;
    let original_show_details = app.show_details;
    let original_preset = app.column_preset;
    let original_order = app.process_columns.clone();

    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
        MetricColumn::PrivateBytes.width() + 1
    );
    assert_eq!(app.sort, original_sort);
    assert_eq!(app.details_metric, original_details_metric);
    assert_eq!(app.show_details, original_show_details);
    assert_eq!(app.column_preset, original_preset);
    assert_eq!(app.process_columns, original_order);

    app.selected_process_column_index = 3;
    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::FullPath)),
        MetricColumn::FullPath.width() + 1
    );
    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
        MetricColumn::PrivateBytes.width() + 1
    );

    app.selected_process_column_index = 2;
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(app.process_columns[1], MetricColumn::PrivateBytes);
    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
        MetricColumn::PrivateBytes.width() + 1
    );

    app.process_columns
        .retain(|column| *column != MetricColumn::FullPath);
    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::FullPath)),
        MetricColumn::FullPath.width() + 1
    );
    app.process_columns.push(MetricColumn::FullPath);
    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::FullPath)),
        MetricColumn::FullPath.width() + 1
    );
}

#[test]
fn shift_left_right_do_not_reorder_fixed_process_columns() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = vec![MetricColumn::PrivateBytes, MetricColumn::HandleCount];
    app.selected_process_column_index = 1;

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(
        app.process_columns,
        vec![MetricColumn::PrivateBytes, MetricColumn::HandleCount]
    );
    assert_eq!(app.selected_process_column(), SortColumn::ProcessName);
    assert_eq!(app.status, "Only metric columns can be reordered");
}

#[test]
fn process_metric_columns_scroll_only_when_selection_leaves_visible_range() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = MetricColumn::ALL.to_vec();
    app.show_details = false;
    let screen = Rect::new(0, 0, 72, 45);
    app.set_screen_area(screen);
    let area = main_panel_areas_for_app(screen, &app).processes.area;
    let visible_count = process_table_visible_column_count(
        area.width,
        &app.process_columns,
        0,
        &app.process_column_widths,
    );
    assert!(visible_count < 2 + app.process_columns.len());

    app.selected_process_column_index = visible_count - 1;
    app.process_metric_column_offset = 0;
    let before = render_app_to_text(&app, screen.width, screen.height);

    app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();

    let after_left = render_app_to_text(&app, screen.width, screen.height);
    let content_height = screen_layout(screen)[2].y.saturating_sub(1) as usize;
    assert_eq!(
        before.lines().take(content_height).collect::<Vec<_>>(),
        after_left.lines().take(content_height).collect::<Vec<_>>()
    );
    assert_eq!(app.selected_process_column_index, visible_count - 2);
    assert_eq!(app.process_metric_column_offset, 0);
    assert!(
        app.status.starts_with("Selected column: "),
        "{}",
        app.status
    );

    app.selected_process_column_index = visible_count - 1;
    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.selected_process_column_index, visible_count);
    assert!(app.process_metric_column_offset > 0);
    let visible_range = ui::process_table_visible_metric_range(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    assert!(visible_range.contains(&(visible_count - 2)));
}

#[test]
fn left_right_does_not_select_process_metric_outside_processes() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::DetailsSamples;

    app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::PrivateBytes)
    );
    assert_eq!(app.details_metric, DetailsMetric::Private);
}

#[test]
fn process_table_distinguishes_row_column_header_and_intersection_surfaces() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(2, 10);
        app.theme_index = theme_index;
        app.snapshot.processes[0].private_bytes = Some(987_654_321);
        app.snapshot.processes[1].private_bytes = Some(123_456_789);

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (row_x, row_y) =
            find_text_position(&buffer, "proc-0").expect("selected process row should be rendered");
        let (column_x, column_y) = find_text_position(&buffer, "123.5 MB")
            .expect("selected process column should be rendered");
        let (intersection_x, intersection_y) = find_text_position(&buffer, "987.7 MB")
            .expect("selected row and column intersection should be rendered");
        let (header_x, header_y) =
            find_text_position(&buffer, "PrivBytes").expect("selected header should render");

        assert_eq!(buffer[(row_x, row_y)].bg, theme.table_selection_surface);
        assert_eq!(buffer[(column_x, column_y)].bg, theme.table_column_surface);
        assert_eq!(buffer[(header_x, header_y)].bg, theme.table_column_surface);
        assert_eq!(
            buffer[(intersection_x, intersection_y)].bg,
            theme.table_intersection_surface
        );
        assert_ne!(
            theme.table_intersection_surface,
            theme.table_selection_surface
        );
        assert_ne!(theme.table_column_surface, theme.table_selection_surface);
    }
}

#[test]
fn compact_system_rows_use_the_process_selection_surface_in_all_color_schemes() {
    let screen = Rect::new(0, 0, 180, 30);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(2, 10);
        app.theme_index = theme_index;
        app.snapshot
            .gpu_adapters
            .push(model::GpuAdapterSample::default());
        let selected_background = |app: &App, area: Rect, label: &str| {
            let buffer = render_app_to_buffer(app, screen.width, screen.height);
            let (x, y) = find_text_position_in_area(&buffer, area, label)
                .unwrap_or_else(|| panic!("selected row should render: {label}"));
            buffer[(x, y)].bg
        };

        app.focused_panel = FocusedPanel::Processes;
        let process_area = main_panel_areas_for_app(screen, &app).processes.area;
        let process_background = selected_background(&app, process_area, "proc-0");
        assert_eq!(process_background, theme.table_selection_surface);

        app.focused_panel = FocusedPanel::System;
        app.select_resource_panel(app::ResourcePanel::Memory);
        assert_eq!(
            selected_background(
                &app,
                ui::ram_vram_panel_area_for_screen(screen, &app),
                "In use",
            ),
            process_background
        );

        app.select_resource_panel(app::ResourcePanel::Gpu);
        assert_eq!(
            selected_background(&app, ui::gpu_panel_area_for_screen(screen, &app), "Usage",),
            process_background
        );

        app.focused_panel = FocusedPanel::SystemActivity;
        assert_eq!(
            selected_background(
                &app,
                ui::system_activity_panel_area_for_screen(screen, &app),
                "Net Rx",
            ),
            process_background
        );

        app.focused_panel = FocusedPanel::Cpu;
        assert_eq!(
            selected_background(&app, ui::cpu_panel_area_for_screen(screen, &app), "Usage",),
            process_background
        );
    }
}

#[test]
fn process_table_multi_selection_shares_cursor_surface_and_keeps_cursor_cell_distinct() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        app.snapshot.processes[0].private_bytes = Some(111_111_111);
        app.snapshot.processes[1].private_bytes = Some(222_222_222);
        app.snapshot.processes[2].private_bytes = Some(333_333_333);
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();

        for cursor_index in [1, 2] {
            if cursor_index == 2 {
                app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))
                    .unwrap();
            }
            let buffer = render_app_to_buffer(&app, 100, 30);
            let rendered = buffer_to_text(&buffer);
            assert!(rendered.contains("2 selected (*)"), "{rendered}");

            for (index, value) in ["111.1 MB", "222.2 MB", "333.3 MB"].iter().enumerate() {
                let (name_x, name_y) =
                    find_text_position(&buffer, &format!("proc-{index}")).unwrap();
                let expected_row = if index < 2 || index == cursor_index {
                    theme.table_selection_surface
                } else {
                    theme.panel
                };
                assert_eq!(buffer[(name_x, name_y)].bg, expected_row);
                let (value_x, value_y) = find_text_position(&buffer, value).unwrap();
                let expected_cell = if index == cursor_index {
                    theme.table_intersection_surface
                } else {
                    theme.table_column_surface
                };
                assert_eq!(buffer[(value_x, value_y)].bg, expected_cell);
                let selected_pid = format!("*{}", app.snapshot.processes[index].pid);
                assert_eq!(rendered.contains(&selected_pid), index < 2, "{rendered}");
            }
        }

        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        let buffer = render_app_to_buffer(&app, 100, 30);
        assert!(!buffer_to_text(&buffer).contains("selected (*)"));
        for name in ["proc-0", "proc-2"] {
            let (x, y) = find_text_position(&buffer, name).unwrap();
            assert_eq!(buffer[(x, y)].bg, theme.panel);
        }
    }
}

#[test]
fn process_table_underlines_full_header_including_sort_arrow() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(1, 10);
        app.theme_index = theme_index;
        app.sort = SortSpec {
            column: SortColumn::Metric(MetricColumn::PrivateBytes),
            direction: SortDirection::Desc,
        };
        let buffer = render_app_to_buffer(&app, 100, 30);
        let (x, y) = find_text_position(&buffer, "PrivBytes ↓")
            .expect("sorted process header should be rendered");
        let (pid_x, pid_y) =
            find_text_position(&buffer, "PID").expect("ordinary process header should render");

        assert_eq!(buffer[(pid_x, pid_y)].bg, theme.panel);
        assert_eq!(buffer[(x, y)].bg, theme.table_column_surface);
        for position in [(x, y), (pid_x, pid_y)] {
            assert_eq!(buffer[position].fg, theme.accent);
            assert!(buffer[position].modifier.contains(Modifier::BOLD));
        }

        for offset in 0.."PrivBytes".len() as u16 {
            assert!(
                buffer[(x + offset, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
            );
        }
        assert!(
            buffer[(x + "PrivBytes ".len() as u16, y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );
    }
}

#[test]
fn process_table_dotnet_header_keeps_sort_arrow_at_compact_width() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![MetricColumn::DotNetHeapBytes];
    app.sort = SortSpec {
        column: SortColumn::Metric(MetricColumn::DotNetHeapBytes),
        direction: SortDirection::Desc,
    };

    let buffer = render_app_to_buffer(&app, 100, 30);

    assert!(find_text_position(&buffer, ".NET Heap ↓").is_some());
}

#[test]
fn header_and_footer_roles_apply_to_all_color_schemes() {
    for theme_index in 0..ui::THEMES.len() {
        let mut app = make_test_app(1, 10);
        app.theme_index = theme_index;
        let theme = ui::THEMES[theme_index];
        let buffer = render_app_to_buffer(&app, 100, 30);

        let (session_x, session_y) = find_text_position(&buffer, "File").expect("leftmost menu");
        assert_eq!(
            session_x,
            concat!(" winproc-tui ", env!("CARGO_PKG_VERSION"), " ").len() as u16 + 1
        );
        assert_eq!(session_y, 0);
        assert_eq!(buffer[(session_x, session_y)].fg, theme.key_hint);
        assert!(
            buffer[(session_x, session_y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );

        let (live_x, live_y) =
            find_text_position(&buffer, "LIVE").expect("live badge should be rendered");
        assert_eq!(live_y, 0);
        assert_eq!(buffer[(live_x, live_y)].fg, theme.background);
        assert_eq!(buffer[(live_x, live_y)].bg, theme.active_series);

        let (shortcut_x, shortcut_y) = find_text_position(&buffer, "Space Graph")
            .expect("process shortcut should be rendered");
        assert_eq!(buffer[(shortcut_x, shortcut_y)].fg, theme.key_hint);
    }
}

#[test]
fn process_table_renders_live_metric_values_neutrally() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].private_bytes = Some(987_654_321);
    app.process_table_state.select(None);

    let buffer = render_app_to_buffer(&app, 100, 30);
    let (x, y) = find_text_position(&buffer, "987.7 MB").expect("private bytes should be rendered");

    assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].text);
    assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
}

#[test]
fn process_table_colors_graphed_value_without_a_slot_number_and_keeps_name_plain() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].name = "target.exe".to_string();
    app.snapshot.processes[0].private_bytes = Some(107_374_182_400);
    app.selected_process_column_index = 1;
    app.process_table_state.select(None);
    app.watch_list = vec!["target.exe".to_string()];
    app.normalized_watch_names = std::collections::HashSet::from(["target.exe".to_string()]);
    app.rebuild_visible_process_cache();
    app.add_or_reveal_graph_source(
        GraphSlot::process(
            ProcessIdentity::from_row(&app.snapshot.processes[0]),
            DetailsMetric::Private,
        ),
        FocusedPanel::Processes,
    );
    app.show_details = false;

    let buffer = render_app_to_buffer(&app, 120, 45);
    let (value_x, value_y) =
        find_text_position(&buffer, "107.4 GB").expect("graphed private bytes should be rendered");
    let (name_x, name_y) =
        find_text_position(&buffer, "target.exe").expect("tracked name should be rendered");
    let tracked_x = (0..name_x)
        .find(|&x| buffer[(x, name_y)].symbol() == "T")
        .expect("tracked marker should be rendered beside the process name");
    let value_cell = &buffer[(value_x, value_y)];
    let tracked_cell = &buffer[(tracked_x, name_y)];
    let value_width = "107.4 GB".chars().count() as u16;
    let cell_start = value_x.saturating_sub(
        MetricColumn::PrivateBytes
            .width()
            .saturating_sub(value_width),
    );

    assert!((cell_start..value_x).all(|x| buffer[(x, value_y)].symbol() == " "));
    assert!(
        (cell_start..value_x)
            .all(|x| !buffer[(x, value_y)].modifier.contains(Modifier::UNDERLINED))
    );
    assert_eq!(value_cell.fg, ui::THEMES[0].active_series);
    assert!(value_cell.modifier.contains(Modifier::BOLD));
    assert!((0..value_width).all(|offset| {
        buffer[(value_x + offset, value_y)]
            .modifier
            .contains(Modifier::UNDERLINED)
    }));
    assert_ne!(value_cell.bg, ui::THEMES[0].warning);
    assert_eq!(buffer[(name_x, name_y)].fg, ui::THEMES[0].text);
    assert_eq!(tracked_cell.fg, ui::THEMES[0].background);
    assert_eq!(tracked_cell.bg, ui::THEMES[0].tracked);
    assert!(!tracked_cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn process_graph_value_registration_styles_survive_table_backgrounds_in_all_themes() {
    let screen = Rect::new(0, 0, 120, 45);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(2, 10);
        app.theme_index = theme_index;
        app.process_columns = vec![MetricColumn::PrivateBytes];
        app.snapshot.processes[0].private_bytes = Some(107_374_182_400);
        app.snapshot.processes[1].private_bytes = None;

        let first = GraphSlot::process(
            ProcessIdentity::from_row(&app.snapshot.processes[0]),
            DetailsMetric::Private,
        );
        let second = GraphSlot::process(
            ProcessIdentity::from_row(&app.snapshot.processes[1]),
            DetailsMetric::Private,
        );
        assert!(app.add_or_reveal_graph_source(first, FocusedPanel::Processes));
        let first_id = app.active_graph_id.unwrap();
        assert!(app.add_or_reveal_graph_source(second, FocusedPanel::Processes));
        let second_id = app.active_graph_id.unwrap();
        assert!(app.set_active_graph(first_id));
        app.show_details = false;
        app.selected_process_column_index = 1;
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
            .unwrap();

        let process_area = main_panel_areas_for_app(screen, &app).processes.area;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (available_x, available_y) =
            find_text_position_in_area(&buffer, process_area, "107.4 GB").unwrap();
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, process_area, "--").unwrap();
        let value_width = "107.4 GB".len() as u16;
        let column_width = app
            .process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::PrivateBytes));
        let padding_start = available_x.saturating_sub(column_width - value_width);

        assert_eq!(buffer[(available_x, available_y)].fg, theme.active_series);
        assert_eq!(
            buffer[(available_x, available_y)].bg,
            theme.table_selection_surface
        );
        assert!(
            buffer[(available_x, available_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(
            buffer[(available_x, available_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!((padding_start..available_x).all(|x| {
            buffer[(x, available_y)].symbol() == " "
                && !buffer[(x, available_y)]
                    .modifier
                    .contains(Modifier::UNDERLINED)
        }));

        assert_eq!(buffer[(missing_x, missing_y)].fg, theme.active_series);
        assert_eq!(
            buffer[(missing_x, missing_y)].bg,
            theme.table_selection_surface
        );
        assert!((0..2).all(|offset| {
            buffer[(missing_x + offset, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        }));
        assert!(
            !buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::BOLD)
        );

        app.selected_process_column_index = 2;
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, process_area, "--").unwrap();
        assert_eq!(
            buffer[(missing_x, missing_y)].bg,
            theme.table_intersection_surface
        );
        assert!(
            buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        assert!(app.set_active_graph(second_id));
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (available_x, available_y) =
            find_text_position_in_area(&buffer, process_area, "107.4 GB").unwrap();
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, process_area, "--").unwrap();
        assert!(
            !buffer[(available_x, available_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            buffer[(available_x, available_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(
            buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );

        assert!(app.move_active_graph_earlier());
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, process_area, "--").unwrap();
        assert!(
            buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );

        app.remove_active_graph();
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (available_x, available_y) =
            find_text_position_in_area(&buffer, process_area, "107.4 GB").unwrap();
        let (missing_x, missing_y) =
            find_text_position_in_area(&buffer, process_area, "--").unwrap();
        assert_eq!(buffer[(missing_x, missing_y)].fg, theme.text);
        assert!(
            !buffer[(missing_x, missing_y)]
                .modifier
                .contains(Modifier::UNDERLINED | Modifier::BOLD)
        );
        assert!(
            buffer[(available_x, available_y)]
                .modifier
                .contains(Modifier::UNDERLINED | Modifier::BOLD)
        );
    }
}

#[test]
fn process_table_keeps_tracked_badge_visible_on_the_selected_row() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(1, 10);
        app.theme_index = theme_index;
        app.snapshot.processes[0].name = "target.exe".to_string();
        track_process_name(&mut app, "target.exe");

        let buffer = render_app_to_buffer(&app, 100, 30);
        let (name_x, name_y) =
            find_text_position(&buffer, "target.exe").expect("tracked name should render");
        let marker_x = (0..name_x)
            .find(|&x| buffer[(x, name_y)].symbol() == "T")
            .expect("tracked badge should render on the selected row");
        let marker = &buffer[(marker_x, name_y)];

        assert_eq!(marker.fg, theme.background);
        assert_eq!(marker.bg, theme.tracked);
        assert!(!marker.modifier.contains(Modifier::BOLD));
    }
}

#[test]
fn process_table_current_row_uses_background_without_cursor_symbol() {
    let app = make_test_app(1, 10);
    let buffer = render_app_to_buffer(&app, 100, 30);
    let rendered = buffer_to_text(&buffer);
    let (name_x, name_y) =
        find_text_position(&buffer, "proc-0").expect("current process row should render");

    assert!(!rendered.contains(">>"), "{rendered}");
    assert_eq!(
        buffer[(name_x, name_y)].bg,
        ui::THEMES[0].table_selection_surface
    );
}

#[test]
fn process_table_marks_only_names_that_are_actually_truncated() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![MetricColumn::FullPath];
    app.process_column_widths.set(SortColumn::ProcessName, 8);
    app.snapshot.processes[0].name = "process-name.exe".to_string();
    app.snapshot.processes[0].executable_path = Some(r"C:\app.exe".to_string());

    let truncated = render_app_to_text(&app, 100, 30);

    assert!(truncated.contains("process⋯"), "{truncated}");

    app.snapshot.processes[0].name = "app.exe".to_string();
    let complete = render_app_to_text(&app, 100, 30);

    assert!(complete.contains("app.exe"), "{complete}");
    assert!(!complete.contains('⋯'), "{complete}");
}

#[test]
fn process_name_width_shortcuts_change_the_rendered_width_immediately() {
    let screen = Rect::new(0, 0, 100, 30);
    let mut app = make_test_app(1, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.process_columns = vec![MetricColumn::PrivateBytes];
    app.process_column_widths.set(SortColumn::ProcessName, 8);
    app.selected_process_column_index = 1;
    app.snapshot.processes[0].name = "process-name.exe".to_string();
    app.set_screen_area(screen);

    let initial = render_app_to_text(&app, screen.width, screen.height);
    assert!(initial.contains("process⋯"), "{initial}");

    app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE))
        .unwrap();
    let widened = render_app_to_text(&app, screen.width, screen.height);
    assert!(widened.contains("process-⋯"), "{widened}");
    assert!(!widened.contains("process⋯"), "{widened}");

    app.on_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))
        .unwrap();
    let narrowed = render_app_to_text(&app, screen.width, screen.height);
    assert!(narrowed.contains("process⋯"), "{narrowed}");
}

#[test]
fn process_table_overflow_indicator_tracks_offsets_and_custom_widths() {
    let screen = Rect::new(0, 0, 72, 30);
    let mut app = make_test_app(1, 10);
    app.process_columns = MetricColumn::ALL.to_vec();
    app.show_details = false;

    let area = ui::main_panel_areas_for_app(screen, &app).processes.area;
    let leading_range = ui::process_table_visible_metric_range(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    let leading_indicator = format!(
        "‹ {}–{}/{} ›",
        leading_range.start + 1,
        leading_range.end,
        app.process_columns.len()
    );
    let leading = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, _) = find_text_position(&leading, &leading_indicator)
        .expect("the leading visible metric range should render");
    assert_eq!(
        x + leading_indicator.chars().count() as u16,
        area.right().saturating_sub(1)
    );

    app.process_metric_column_offset = 10;
    let offset_range = ui::process_table_visible_metric_range(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    let offset_indicator = format!(
        "‹ {}–{}/{} ›",
        offset_range.start + 1,
        offset_range.end,
        app.process_columns.len()
    );
    let offset = render_app_to_text(&app, screen.width, screen.height);
    assert!(offset.contains(&offset_indicator), "{offset}");

    app.process_metric_column_offset = 0;
    app.process_column_widths.set(SortColumn::ProcessName, 40);
    let custom_range = ui::process_table_visible_metric_range(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    let custom_indicator = format!(
        "‹ {}–{}/{} ›",
        custom_range.start + 1,
        custom_range.end,
        app.process_columns.len()
    );
    let custom = render_app_to_text(&app, screen.width, screen.height);
    assert!(custom.contains(&custom_indicator), "{custom}");
    assert!(custom_range.end < leading_range.end);
}

#[test]
fn process_table_overflow_indicator_handles_zero_or_no_hidden_metrics() {
    let mut app = make_test_app(1, 10);
    app.process_columns = MetricColumn::ALL.to_vec();

    let narrow_buffer = render_app_to_buffer(&app, 35, 20);
    let narrow = buffer_to_text(&narrow_buffer);
    assert!(narrow.contains("‹ 0/24 ›"), "{narrow}");
    let (indicator_x, indicator_y) = find_text_position(&narrow_buffer, "‹ 0/24 ›")
        .expect("the zero-column indicator should render");
    app.on_mouse(
        left_click(indicator_x, indicator_y),
        Rect::new(0, 0, 35, 20),
    );
    assert!(!app.watch_enabled);

    let wide = render_app_to_text(&app, 400, 20);
    assert!(!wide.contains("‹ 1–24/24 ›"), "{wide}");
    assert!(!wide.contains("‹ 0/24 ›"), "{wide}");
}

#[test]
fn process_table_mouse_click_selects_row_and_metric_column() {
    let mut app = make_test_app(2, 10);
    app.process_columns = vec![
        MetricColumn::PrivateBytes,
        MetricColumn::ThreadCount,
        MetricColumn::HandleCount,
    ];
    app.process_column_widths.set(SortColumn::Pid, 8);
    app.process_column_widths.set(SortColumn::ProcessName, 27);
    app.process_column_widths.set(
        SortColumn::Metric(MetricColumn::ThreadCount),
        MetricColumn::ThreadCount.width() + 4,
    );
    app.selected_process_column_index = 2;
    app.snapshot.processes[1].thread_count = Some(77);
    app.snapshot.processes[1].handle_count = Some(888);

    let buffer = render_app_to_buffer(&app, 100, 30);
    let (x, _) =
        find_text_position(&buffer, "Hndl").expect("target column header should be rendered");
    let (_, y) =
        find_text_position(&buffer, "888").expect("target handle count should be rendered");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 100, 30),
    );

    assert_eq!(app.process_table_state.selected(), Some(1));
    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::HandleCount)
    );
    assert_eq!(app.details_metric, DetailsMetric::Private);
}

#[test]
fn process_table_resize_preserves_custom_column_widths() {
    let mut app = make_test_app(2, 10);
    app.process_column_widths.set(
        SortColumn::Metric(MetricColumn::PrivateBytes),
        crate::model::columns::PROCESS_COLUMN_WIDTH_MAX,
    );

    let _ = render_app_to_buffer(&app, 40, 20);
    let _ = render_app_to_buffer(&app, 160, 40);

    assert_eq!(
        app.process_column_widths
            .resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
        crate::model::columns::PROCESS_COLUMN_WIDTH_MAX
    );
}

#[test]
fn process_table_tracked_only_title_checkbox_click_toggles_filter() {
    let mut app = make_test_app(2, 10);
    app.snapshot.processes[0].name = "target.exe".to_string();
    app.snapshot.processes[1].name = "other.exe".to_string();
    app.watch_list = vec!["target.exe".to_string()];
    app.normalized_watch_names = ["target.exe".to_string()].into_iter().collect();

    let screen = Rect::new(0, 0, 120, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position(&buffer, "[ ] Tracked-only(Shift+T)")
        .expect("unchecked tracked-only control should render in the process title");
    assert_eq!(buffer[(x, y)].fg, ui::THEMES[0].muted);
    let (shortcut_x, shortcut_y) = find_text_position(&buffer, "(Shift+T)")
        .expect("tracked-only shortcut should render in the process title");
    assert_eq!(shortcut_y, y);
    assert_eq!(buffer[(shortcut_x, shortcut_y)].fg, ui::THEMES[0].muted);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert!(app.watch_enabled);
    assert_eq!(app.visible_process_count(), 1);
    assert_eq!(app.visible_process_at(0).unwrap().name, "target.exe");
    assert_eq!(
        app.tracked_total_visible_row().unwrap().process.name,
        "Tracked Total"
    );

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, y) = find_text_position(&buffer, "[x] Tracked-only(Shift+T)")
        .expect("checked tracked-only control should render in the process title");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert!(!app.watch_enabled);
    assert_eq!(app.visible_process_count(), 2);
}

#[test]
fn process_columns_mouse_workflow_covers_picker_range_and_width() {
    let mut app = make_test_app(3, 10);
    app.process_columns = MetricColumn::ALL.to_vec();
    let screen = Rect::new(0, 0, 180, 60);
    crate::app::sync_layout_state(&mut app, screen);
    let area = main_panel_areas_for_app(screen, &app).processes.area;
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (x, y) = super::support::find_text_position(&buffer, "PrivBytes").unwrap();
    app.on_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.show_column_picker);
    assert_eq!(
        MetricColumn::ALL[app.column_picker_index],
        MetricColumn::PrivateBytes
    );
    let columns = app.process_columns.clone();
    let dialog = render_app_to_buffer(&app, 180, 60);
    let (cx, cy) = super::support::find_text_position(&dialog, "[x] CPU%").unwrap();
    app.on_mouse(super::support::left_click(cx, cy), screen);
    assert!(app.show_column_picker);
    assert!(!app.process_columns.contains(&MetricColumn::CpuPercent));
    assert_ne!(app.process_columns, columns);
    let (x, y) = super::support::find_text_position(&dialog, "Enter/Esc close").unwrap();
    app.on_mouse(super::support::left_click(x + 10, y), screen);
    assert!(!app.show_column_picker);
    for _ in 0..30 {
        let next = ui::process_table::column_control_areas(area, &app)
            .into_iter()
            .find(|(control, _)| *control == ui::process_table::ColumnControl::Next);
        let Some((_, rect)) = next else {
            break;
        };
        app.on_mouse(super::support::left_click(rect.x, rect.y), screen);
    }
    assert_eq!(
        app.selected_process_column(),
        SortColumn::Metric(MetricColumn::FullPath)
    );
    assert!(app.process_metric_column_offset > 0);
    for _ in 0..30 {
        let previous = ui::process_table::column_control_areas(area, &app)
            .into_iter()
            .find(|(control, _)| *control == ui::process_table::ColumnControl::Previous);
        let Some((_, rect)) = previous else {
            break;
        };
        app.on_mouse(super::support::left_click(rect.x, rect.y), screen);
    }
    assert_eq!(app.process_metric_column_offset, 0);
    app.selected_process_column_index = 1;
    let original = app.process_column_widths.resolved(SortColumn::ProcessName);
    for (control, expected) in [
        (ui::process_table::ColumnControl::Widen, original + 1),
        (ui::process_table::ColumnControl::Narrow, original),
    ] {
        let (_, rect) = ui::process_table::column_control_areas(area, &app)
            .into_iter()
            .find(|(candidate, _)| *candidate == control)
            .unwrap();
        app.on_mouse(super::support::left_click(rect.x, rect.y), screen);
        assert_eq!(
            app.process_column_widths.resolved(SortColumn::ProcessName),
            expected
        );
    }
}

#[test]
fn column_boundary_drag_resizes_without_sorting_and_stops_on_resize() {
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 160, 40);
    app.set_screen_area(screen);
    app.process_columns = vec![MetricColumn::CpuPercent, MetricColumn::PrivateBytes];
    app.process_metric_column_offset = 1;
    app.toggle_display_pause();
    let area = main_panel_areas_for_app(screen, &app).processes.area;
    let column = SortColumn::Metric(MetricColumn::PrivateBytes);
    let (_, edge) = ui::process_table::column_resize_areas(area, &app)
        .into_iter()
        .find(|(c, _)| *c == column)
        .unwrap();
    let width = app.process_column_widths.resolved(column);
    let sort = app.sort;
    let selected = app.selected_process_identity.clone();
    let outcome =
        app::handle_mouse_event(&mut app, super::support::mouse_move(edge.x, edge.y), screen);
    assert!(outcome.dirty);
    assert_eq!(app.column_resize_hovered, Some(column));
    app.on_mouse(left_click(edge.x, edge.y), screen);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: edge.x + 5,
            row: edge.y + 4,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(app.process_column_widths.resolved(column), width + 5);
    assert_eq!(app.sort, sort);
    assert_eq!(app.selected_process_identity, selected);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: edge.y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(
        app.process_column_widths.resolved(column),
        column.min_width()
    );
    assert!(app.column_resize_drag.is_none());
    let edge = ui::process_table::column_resize_areas(area, &app)[0].1;
    app.on_mouse(left_click(edge.x, edge.y), screen);
    assert!(app.column_resize_drag.is_some());
    app.set_screen_area(Rect::new(0, 0, 140, 40));
    assert!(app.column_resize_drag.is_none());
}

#[test]
fn full_path_drag_uses_rendered_width_then_keeps_manual_width() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![MetricColumn::FullPath];
    let area = Rect::new(0, 0, 140, 10);
    let column = SortColumn::Metric(MetricColumn::FullPath);
    let initial = crate::ui::process_table::column_rendered_width(area, &app, column);
    assert!(initial > MetricColumn::FullPath.width());
    app.process_column_widths.set(column, 50);
    assert_eq!(
        crate::ui::process_table::column_rendered_width(area, &app, column),
        50
    );
    app.process_column_widths
        .set(column, MetricColumn::FullPath.width());
    assert_eq!(
        crate::ui::process_table::column_rendered_width(area, &app, column),
        MetricColumn::FullPath.width()
    );
}

#[test]
fn header_underlines_include_padding_without_an_idle_edge_background() {
    let mut app = make_test_app(1, 10);
    app.process_columns = vec![MetricColumn::GpuPercent, MetricColumn::IoReadBytesPerSec];
    app.selected_process_column_index = 1;
    for width in [80, 140] {
        let screen = Rect::new(0, 0, width, 40);
        app::sync_layout_state(&mut app, screen);
        let area = main_panel_areas_for_app(screen, &app).processes.area;
        let buffer = render_app_to_buffer(&app, width, 40);
        for (column, edge) in ui::process_table::column_resize_areas(area, &app) {
            let cell_width = ui::process_table::column_rendered_width(area, &app, column);
            let start = edge.x + 1 - cell_width;
            for x in start..=edge.x {
                assert!(buffer[(x, edge.y)].modifier.contains(Modifier::UNDERLINED));
            }
            assert_eq!(buffer[(edge.x, edge.y)].bg, buffer[(start, edge.y)].bg);
            if column == SortColumn::ProcessName {
                assert_eq!(buffer[(edge.x, edge.y)].symbol(), " ");
            }
            if edge.x + 1 < area.right() - 1 {
                assert!(
                    !buffer[(edge.x + 1, edge.y)]
                        .modifier
                        .contains(Modifier::UNDERLINED)
                );
            }
        }
        app.column_resize_hovered = Some(SortColumn::ProcessName);
        let hovered = render_app_to_buffer(&app, width, 40);
        let (_, edge) = ui::process_table::column_resize_areas(area, &app)
            .into_iter()
            .find(|(c, _)| *c == SortColumn::ProcessName)
            .unwrap();
        assert_eq!(hovered[(edge.x, edge.y)].bg, app.theme().focus_surface);
        assert!(
            hovered[(edge.x, edge.y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        app.column_resize_hovered = None;
    }
}
