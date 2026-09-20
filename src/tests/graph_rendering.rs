use super::support::{
    add_test_graph, area_contains_foreground, assert_blank_row_above_text,
    assert_dialog_title_style, assert_title_style, assign_private_graph, buffer_to_text,
    find_styled_symbol_positions_in_area, find_text_position, find_text_position_in_area,
    left_click, make_test_app, mouse_move, render_app_to_buffer, render_app_to_text,
    test_graph_source,
};
use crate::app;
use crate::app::{
    DetailsMetric, FocusedPanel, GraphDisplayMode, GraphHoverTarget, GraphSlot, GraphSlotLayout,
    handle_mouse_event,
};
use crate::model::{
    MetricColumn, ProcessIdentity, SortColumn, SortDirection, SortSpec, SystemMetric,
};
use crate::ui;
use crate::ui::{THEMES, details_graph_area_for_app, main_panel_areas_for_app};
use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::time::{Duration, Instant};

#[test]
fn panel_focus_and_active_graph_use_distinct_frames_in_all_color_schemes() {
    let screen = Rect::new(0, 0, 180, 60);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        assign_private_graph(&mut app);
        app.show_samples_panel = true;
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let samples = layout
            .samples
            .expect("Samples should fit beside Graph Slots");
        let card = layout.graph_cards.first().expect("active graph card").area;

        app.focused_panel = FocusedPanel::DetailsGraph;
        let graph_focused = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(
            graph_focused[(
                layout.graph_slots.right().saturating_sub(1),
                layout.graph_slots.y
            )]
                .symbol(),
            "┓"
        );
        assert_eq!(
            graph_focused[(
                layout.graph_slots.right().saturating_sub(1),
                layout.graph_slots.y
            )]
                .fg,
            theme.focus_border
        );
        assert_eq!(
            graph_focused[(layout.graph_slots.x, layout.graph_slots.y + 1)].symbol(),
            "┃"
        );
        assert_eq!(
            graph_focused[(
                layout.graph_slots.x,
                layout.graph_slots.bottom().saturating_sub(1)
            )]
                .symbol(),
            "┗"
        );
        assert_eq!(graph_focused[(card.x, card.y)].symbol(), "╭");
        assert_eq!(graph_focused[(card.x, card.y)].fg, theme.active_series);
        assert!(
            graph_focused[(card.x, card.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(graph_focused[(samples.x, samples.y)].symbol(), "╭");
        assert_eq!(graph_focused[(samples.x, samples.y)].fg, theme.border);

        app.focused_panel = FocusedPanel::DetailsSamples;
        let samples_focused = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(
            samples_focused[(
                layout.graph_slots.right().saturating_sub(1),
                layout.graph_slots.y
            )]
                .symbol(),
            "╮"
        );
        assert_eq!(
            samples_focused[(
                layout.graph_slots.right().saturating_sub(1),
                layout.graph_slots.y
            )]
                .fg,
            theme.border
        );
        assert_eq!(samples_focused[(card.x, card.y)].symbol(), "╭");
        assert_eq!(samples_focused[(card.x, card.y)].fg, theme.active_series);
        assert!(
            samples_focused[(card.x, card.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(samples_focused[(samples.x, samples.y)].symbol(), "┏");
        assert_eq!(
            samples_focused[(samples.x, samples.y)].fg,
            theme.focus_border
        );

        app.focused_panel = FocusedPanel::Processes;
        let processes_focused = render_app_to_buffer(&app, screen.width, screen.height);
        assert_eq!(
            processes_focused[(
                layout.graph_slots.right().saturating_sub(1),
                layout.graph_slots.y
            )]
                .symbol(),
            "╮"
        );
        assert_eq!(processes_focused[(card.x, card.y)].symbol(), "╭");
        assert_eq!(processes_focused[(card.x, card.y)].fg, theme.active_series);
        assert!(
            processes_focused[(card.x, card.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(processes_focused[(samples.x, samples.y)].symbol(), "╭");
    }
}

#[test]
fn active_graph_series_and_slot_tokens_match_in_all_color_schemes() {
    let screen = Rect::new(0, 0, 180, 70);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(1, 10);
        app.theme_index = theme_index;
        let identity = app.selected_visible_process_identity().unwrap();
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), DetailsMetric::Private),
            FocusedPanel::Processes,
        );
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Workset),
            FocusedPanel::Processes,
        );
        let active_id = app.active_graph_id.unwrap();
        let first_time = app.snapshot.captured_at;
        app.snapshot.processes[0].private_bytes = Some(1_000);
        app.snapshot.processes[0].workset_bytes = Some(2_000);
        app.process_history.record_snapshot(
            first_time,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.snapshot.captured_at = first_time + chrono::Duration::seconds(1);
        app.snapshot.processes[0].private_bytes = Some(1_100);
        app.snapshot.processes[0].workset_bytes = Some(2_200);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.select_details_sample_latest();
        app.show_samples_panel = true;
        app::sync_layout_state(&mut app, screen);

        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let active_card = layout
            .graph_cards
            .iter()
            .find(|card| card.id == active_id)
            .expect("active graph card");
        let active_plot = active_card.plot;
        let inactive_plot = layout
            .graph_cards
            .iter()
            .find(|card| card.id != active_id)
            .expect("inactive graph card")
            .plot;
        let samples = layout.samples.expect("Samples should render");
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);

        assert!(
            area_contains_foreground(&buffer, active_plot, theme.active_series),
            "theme={theme_index}: active series should use the active data color"
        );
        assert!(
            !area_contains_foreground(&buffer, active_plot, theme.graph_line),
            "theme={theme_index}: active series should not use the inactive color"
        );
        assert!(
            area_contains_foreground(&buffer, inactive_plot, theme.graph_line),
            "theme={theme_index}: inactive series should stay monochrome"
        );
        let (value_x, value_y) = find_text_position_in_area(&buffer, samples, "2,200")
            .expect("active Samples metric value should render");
        assert_eq!(buffer[(value_x, value_y)].fg, theme.text);
        let (history_x, history_y) = find_text_position_in_area(&buffer, samples, "2,000")
            .expect("non-selected Samples metric value should render");
        assert_eq!(buffer[(history_x, history_y)].fg, theme.text);
        let (header_x, header_y) = find_text_position_in_area(&buffer, samples, "WS")
            .expect("active Samples metric header should render");
        assert_eq!(buffer[(header_x, header_y)].fg, theme.accent);
        assert!(
            buffer[(header_x, header_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        let (slot_x, slot_y) = find_text_position_in_area(&buffer, samples, "Slot#2")
            .expect("compact Samples slot title should render");
        assert_eq!(buffer[(slot_x, slot_y)].fg, theme.active_series);
        assert!(buffer[(slot_x, slot_y)].modifier.contains(Modifier::BOLD));
        let (graph_slot_x, graph_slot_y) =
            find_text_position_in_area(&buffer, active_card.title, "Slot#2")
                .expect("active Graph slot title should render");
        assert_eq!(buffer[(graph_slot_x, graph_slot_y)].fg, theme.active_series);
        assert!(
            buffer[(graph_slot_x, graph_slot_y)]
                .modifier
                .contains(Modifier::BOLD)
        );
    }
}

#[test]
fn exactly_one_top_level_panel_uses_high_contrast_focus_chrome() {
    let screen = Rect::new(0, 0, 180, 60);
    for theme_index in 0..ui::THEMES.len() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        assign_private_graph(&mut app);
        app.show_samples_panel = true;
        app::sync_layout_state(&mut app, screen);

        for focused_panel in [
            FocusedPanel::System,
            FocusedPanel::SystemActivity,
            FocusedPanel::Cpu,
            FocusedPanel::Processes,
            FocusedPanel::DetailsGraph,
            FocusedPanel::DetailsSamples,
        ] {
            app.focused_panel = focused_panel;
            let buffer = render_app_to_buffer(&app, screen.width, screen.height);
            let focused_top_left_corners = buffer
                .content()
                .iter()
                .filter(|cell| cell.symbol() == "┏" && cell.fg == app.theme().focus_border)
                .count();
            assert_eq!(focused_top_left_corners, 1, "one panel owns focus");
        }
    }
}

#[test]
fn tab_moves_focus_chrome_from_mem_to_gpu() {
    let screen = Rect::new(0, 0, 180, 30);
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    app.resource_panel = app::ResourcePanel::Memory;

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.focused_panel, FocusedPanel::System);
    assert_eq!(app.resource_panel, app::ResourcePanel::Gpu);
    assert_eq!(app.status, "Focus: GPU");

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let memory = ui::ram_vram_panel_area_for_screen(screen, &app);
    let gpu = ui::gpu_panel_area_for_screen(screen, &app);
    assert_eq!(buffer[(memory.x, memory.y)].symbol(), "╭");
    assert_eq!(buffer[(memory.x, memory.y)].fg, app.theme().border);
    assert_eq!(buffer[(gpu.x, gpu.y)].symbol(), "┏");
    assert_eq!(buffer[(gpu.x, gpu.y)].fg, app.theme().focus_border);
}

#[test]
fn top_level_panel_names_reverse_colors_and_retain_focus_in_all_color_schemes() {
    let screen = Rect::new(0, 0, 180, 60);
    for theme_index in 0..ui::THEMES.len() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        assign_private_graph(&mut app);
        app.show_samples_panel = true;
        app::sync_layout_state(&mut app, screen);

        let main_panels = main_panel_areas_for_app(screen, &app);
        let graph_layout = ui::layout::graph_workspace_layout(
            main_panels.details.expect("Graph Workspace should render"),
            &app,
        );
        let titled_panels = [
            (
                FocusedPanel::System,
                ui::ram_vram_panel_area_for_screen(screen, &app),
                "MEM",
            ),
            (
                FocusedPanel::SystemActivity,
                ui::system_activity_panel_area_for_screen(screen, &app),
                "NW/DISK",
            ),
            (
                FocusedPanel::Cpu,
                ui::cpu_panel_area_for_screen(screen, &app),
                "CPU",
            ),
            (
                FocusedPanel::Processes,
                main_panels.processes.area,
                "PROCESSES",
            ),
            (
                FocusedPanel::DetailsGraph,
                graph_layout.graph_slots,
                "GRAPHS",
            ),
            (
                FocusedPanel::DetailsSamples,
                graph_layout.samples.expect("Samples should render"),
                "SAMPLES",
            ),
        ];

        for (focused_panel, _, _) in titled_panels {
            app.focused_panel = focused_panel;
            let buffer = render_app_to_buffer(&app, screen.width, screen.height);
            for (panel, area, title) in titled_panels {
                let (x, y) = find_text_position_in_area(&buffer, area, title)
                    .unwrap_or_else(|| panic!("missing title {title}"));
                let expected = if panel == focused_panel {
                    app.theme().focus_border
                } else {
                    app.theme().muted
                };
                assert_eq!(
                    buffer[(x, y)].bg,
                    expected,
                    "theme={theme_index}, focus={focused_panel:?}, title={title}"
                );
                assert_eq!(buffer[(x, y)].fg, app.theme().panel);
                assert!(
                    buffer[(x, y)].modifier.contains(Modifier::BOLD),
                    "unexpected title weight: theme={theme_index}, focus={focused_panel:?}, title={title}"
                );
            }
        }
    }
}

#[test]
fn panel_and_dialog_title_names_are_uppercase_and_bold_in_all_color_schemes() {
    let screen = Rect::new(0, 0, 180, 60);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;

        app.show_help = true;
        let help = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&help, "HELP", theme);
        app.show_help = false;

        app.show_column_picker = true;
        let columns = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&columns, "COLUMNS", theme);
        app.show_column_picker = false;

        app.show_system_info_dialog = true;
        let system_info = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&system_info, "SYSTEM INFO", theme);
        app.show_system_info_dialog = false;

        app.show_log_dir_dialog = true;
        let log_directory = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&log_directory, "LOG DIRECTORY", theme);
        app.show_log_dir_dialog = false;

        app.open_selected_process_info_dialog().unwrap();
        let process_info = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&process_info, "PROCESS INFO", theme);
        let (metadata_x, metadata_y) = find_text_position(&process_info, "proc-0 · PID 0")
            .expect("Process Info target metadata should render");
        assert_eq!(
            process_info[(metadata_x, metadata_y)].fg,
            ui::theme::contrasting_foreground(theme.focus_border, theme)
        );
        assert_eq!(
            process_info[(metadata_x, metadata_y)].bg,
            theme.focus_border
        );
        assert!(
            !process_info[(metadata_x, metadata_y)]
                .modifier
                .contains(Modifier::BOLD),
            "Process Info target metadata should use normal weight"
        );
        app.close_process_info_dialog();

        app.show_recording_path_dialog = true;
        let recording = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&recording, "RECORDING", theme);
        app.show_recording_path_dialog = false;

        app.show_log_list = true;
        let logs = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&logs, "LOGS", theme);
        app.show_log_list = false;

        app.open_investigation_profiles();
        let profiles = render_app_to_buffer(&app, screen.width, screen.height);
        assert_dialog_title_style(&profiles, "OPEN INVESTIGATION PROFILE", theme);
        app.close_investigation_profiles();

        app.show_display_area_warning = true;
        let warning = render_app_to_buffer(&app, screen.width, screen.height);
        assert_title_style(
            &warning,
            "WARNING",
            theme.warning,
            ui::theme::contrasting_foreground(theme.warning, theme),
        );
        app.show_display_area_warning = false;

        app.show_quit_confirmation = true;
        let confirm = render_app_to_buffer(&app, screen.width, screen.height);
        assert_title_style(
            &confirm,
            "CONFIRM",
            theme.warning,
            ui::theme::contrasting_foreground(theme.warning, theme),
        );
    }
}

#[test]
fn modal_scrim_dims_the_workspace_and_keeps_the_dialog_elevated() {
    let screen = Rect::new(0, 0, 120, 45);
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    let process_panel = main_panel_areas_for_app(screen, &app).processes.area;
    let base = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(
        base[(process_panel.x, process_panel.y)].fg,
        app.theme().border
    );

    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    let modal = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(modal[(process_panel.x, process_panel.y)].symbol(), "╭");
    assert_ne!(
        modal[(process_panel.x, process_panel.y)].fg,
        base[(process_panel.x, process_panel.y)].fg
    );

    let (message_x, message_y) = find_text_position(
        &modal,
        "Confirm the log file and interval, then press Enter to start.",
    )
    .expect("recording dialog message");
    assert_eq!(modal[(message_x, message_y)].bg, app.theme().panel_alt);
}

#[test]
fn nested_confirmation_dims_its_parent_dialog() {
    let screen = Rect::new(0, 0, 120, 45);
    let mut app = make_test_app(3, 10);
    app.show_recording_path_dialog = true;
    app.recording_path_draft = "C:/logs/example.log".to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    let parent = render_app_to_buffer(&app, screen.width, screen.height);
    let (label_x, label_y) = find_text_position(&parent, "Log file").expect("recording path label");

    app.show_recording_overwrite_confirmation = true;
    let nested = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(nested[(label_x, label_y)].symbol(), "L");
    assert_ne!(nested[(label_x, label_y)].fg, parent[(label_x, label_y)].fg);
    let (title_x, title_y) = find_text_position(&nested, "CONFIRM").expect("confirm title");
    assert_eq!(nested[(title_x, title_y)].bg, app.theme().warning);
}

#[test]
fn dialog_shortcut_guidance_is_separated_from_content_by_a_blank_row() {
    let screen = Rect::new(0, 0, 180, 60);
    let mut cases = Vec::new();

    let mut help = make_test_app(3, 10);
    help.show_help = true;
    cases.push((
        "help",
        render_app_to_buffer(&help, screen.width, screen.height),
        "↑/↓ Scroll  PageUp/PageDown Page",
    ));

    let mut logs = make_test_app(3, 10);
    logs.show_log_list = true;
    cases.push((
        "logs",
        render_app_to_buffer(&logs, screen.width, screen.height),
        "↑/↓ select  Enter open",
    ));

    let mut process_info = make_test_app(3, 10);
    process_info.open_selected_process_info_dialog().unwrap();
    cases.push((
        "process-info",
        render_app_to_buffer(&process_info, screen.width, screen.height),
        "←/→ tabs",
    ));

    let mut system_info = make_test_app(3, 10);
    system_info.show_system_info_dialog = true;
    cases.push((
        "system-info",
        render_app_to_buffer(&system_info, screen.width, screen.height),
        "Enter/Esc Close",
    ));

    let mut profiles = make_test_app(3, 10);
    profiles.open_investigation_profiles();
    cases.push((
        "investigation-profiles",
        render_app_to_buffer(&profiles, screen.width, screen.height),
        "Ctrl+S Save As  Esc Close",
    ));

    let mut recording = make_test_app(3, 10);
    recording.show_recording_path_dialog = true;
    cases.push((
        "recording",
        render_app_to_buffer(&recording, screen.width, screen.height),
        "Enter start  Esc cancel  Tab focus  ←/→ value  Ctrl+Space Complete path",
    ));

    let mut log_directory = make_test_app(3, 10);
    log_directory.show_log_list = true;
    log_directory.open_log_dir_dialog().unwrap();
    cases.push((
        "log-directory",
        render_app_to_buffer(&log_directory, screen.width, screen.height),
        "Enter apply  Esc cancel  Ctrl+Space Complete path",
    ));

    for (name, buffer, shortcuts) in cases {
        assert_blank_row_above_text(&buffer, shortcuts);
        let (_, y) = find_text_position(&buffer, shortcuts)
            .unwrap_or_else(|| panic!("{name} shortcuts should render"));
        assert!(y > 0, "{name} shortcuts should have a separator row");
    }
}

#[test]
fn clicking_graph_workspace_top_rule_moves_focus_to_graphs() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::Processes;
    let screen = Rect::new(0, 0, 180, 60);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);

    app.on_mouse(
        left_click(layout.graph_slots.right() - 1, layout.graph_slots.y),
        screen,
    );

    assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
    assert_eq!(app.status, "Focus: Graph 1/1");
}

#[test]
fn graph_span_buttons_zoom_and_highlight_on_hover() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.show_samples_panel = false;
    app.graph_time_span_seconds = 120;
    let screen = Rect::new(0, 0, 160, 60);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let zoom_out = layout.span_controls.zoom_out.unwrap();
    let zoom_in = layout.span_controls.zoom_in.unwrap();

    let initial = render_app_to_buffer(&app, screen.width, screen.height);
    let zoom_out_text = (zoom_out.x..zoom_out.right())
        .map(|x| initial[(x, zoom_out.y)].symbol())
        .collect::<String>();
    let zoom_in_text = (zoom_in.x..zoom_in.right())
        .map(|x| initial[(x, zoom_in.y)].symbol())
        .collect::<String>();
    assert_eq!(zoom_out_text, "[-]");
    assert_eq!(zoom_in_text, "[+]");
    assert_eq!(initial[(zoom_out.x + 1, zoom_out.y)].bg, app.theme().panel);

    let first_move = handle_mouse_event(&mut app, mouse_move(zoom_out.x + 1, zoom_out.y), screen);
    assert!(first_move.dirty);
    let repeated_move =
        handle_mouse_event(&mut app, mouse_move(zoom_out.x + 1, zoom_out.y), screen);
    assert!(!repeated_move.dirty);

    assert_eq!(app.graph_hovered_target, Some(GraphHoverTarget::ZoomOut));
    let hovered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(
        hovered[(zoom_out.x + 1, zoom_out.y)].bg,
        app.theme().focus_surface
    );
    assert!(
        hovered[(zoom_out.x + 1, zoom_out.y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    app.on_mouse(left_click(zoom_out.x + 1, zoom_out.y), screen);
    assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
    assert_eq!(app.graph_time_span_seconds, 300);

    app.on_mouse(mouse_move(zoom_in.x + 1, zoom_in.y), screen);
    assert_eq!(app.graph_hovered_target, Some(GraphHoverTarget::ZoomIn));
    app.on_mouse(left_click(zoom_in.x + 1, zoom_in.y), screen);
    assert_eq!(app.graph_time_span_seconds, 120);
}

#[test]
fn graph_remove_button_highlights_on_hover_and_clears_when_pointer_leaves() {
    let mut app = make_test_app(1, 10);
    let id = add_test_graph(&mut app, 0);
    app.show_samples_panel = false;
    let screen = Rect::new(0, 0, 120, 45);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let card = &layout.graph_cards[0];

    app.on_mouse(mouse_move(card.remove.x + 2, card.remove.y), screen);

    assert_eq!(app.graph_hovered_target, Some(GraphHoverTarget::Remove(id)));
    let hovered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(
        hovered[(card.remove.x + 2, card.remove.y)].bg,
        app.theme().focus_surface
    );
    assert!(
        hovered[(card.remove.x + 2, card.remove.y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    app.on_mouse(mouse_move(card.plot.x, card.plot.y), screen);
    assert_eq!(app.graph_hovered_target, None);
}

#[test]
fn graph_mode_button_targets_its_stable_id_without_activating_the_card() {
    let mut app = make_test_app(1, 10);
    let ids = (0..3)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    app.graph_entries.swap(0, 2);
    assert!(app.set_active_graph(ids[0]));
    app.show_samples_panel = false;
    app.graph_slot_layout = GraphSlotLayout::ThreeColumns;
    app.details_sample_selected = 7;
    app.graph_time_offset_seconds = 30;
    app.ab_comparison = Some(app::AbComparison { a: None, b: None });
    let screen = Rect::new(0, 0, 220, 60);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let card = layout
        .graph_cards
        .iter()
        .find(|card| card.id == ids[1])
        .expect("inactive reordered Graph card");
    assert!(card.display_mode.right() <= card.remove.x);

    app.on_mouse(
        mouse_move(card.display_mode.x + 2, card.display_mode.y),
        screen,
    );

    assert_eq!(
        app.graph_hovered_target,
        Some(GraphHoverTarget::DisplayMode(ids[1]))
    );
    let hovered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(
        hovered[(card.display_mode.x + 2, card.display_mode.y)].bg,
        app.theme().focus_surface
    );
    assert!(
        hovered[(card.display_mode.x + 2, card.display_mode.y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    app.on_mouse(
        left_click(card.display_mode.x + 2, card.display_mode.y),
        screen,
    );

    assert_eq!(app.active_graph_id, Some(ids[0]));
    assert_eq!(app.details_sample_selected, 7);
    assert_eq!(app.graph_time_offset_seconds, 30);
    assert_eq!(
        app.graph_entry_by_id(ids[1]).unwrap().display_mode,
        GraphDisplayMode::MovingAverage5
    );
    assert_eq!(
        app.graph_entry_by_id(ids[0]).unwrap().display_mode,
        GraphDisplayMode::Raw
    );
    let rendered = render_app_to_buffer(&app, screen.width, screen.height);
    let mode_text = (card.display_mode.x..card.display_mode.right())
        .map(|x| rendered[(x, card.display_mode.y)].symbol())
        .collect::<String>();
    let remove_text = (card.remove.x..card.remove.right())
        .map(|x| rendered[(x, card.remove.y)].symbol())
        .collect::<String>();
    assert!(mode_text.contains("[MA]"), "{mode_text:?}");
    assert!(remove_text.contains("[x]"), "{remove_text:?}");
}

#[test]
fn narrow_graph_card_keeps_mode_and_remove_buttons_distinct() {
    let mut app = make_test_app(1, 10);
    add_test_graph(&mut app, 0);
    app.show_samples_panel = false;
    let screen = Rect::new(0, 0, 80, 35);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let card = &layout.graph_cards[0];
    let rendered = render_app_to_buffer(&app, screen.width, screen.height);
    let mode_text = (card.display_mode.x..card.display_mode.right())
        .map(|x| rendered[(x, card.display_mode.y)].symbol())
        .collect::<String>();
    let remove_text = (card.remove.x..card.remove.right())
        .map(|x| rendered[(x, card.remove.y)].symbol())
        .collect::<String>();

    assert_eq!(card.display_mode.width, 7);
    assert_eq!(card.remove.width, 5);
    assert!(card.display_mode.right() <= card.remove.x);
    assert!(mode_text.contains("[RAW]"), "{mode_text:?}");
    assert!(remove_text.contains("[x]"), "{remove_text:?}");
}

#[test]
fn graph_fit_all_disables_zoom_out_and_zoom_in_uses_visible_span() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.show_samples_panel = false;
    for offset in [0, 120, 240] {
        app.process_history.record_snapshot(
            app.snapshot.captured_at + chrono::Duration::seconds(offset),
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    app.toggle_graph_all_samples();
    let screen = Rect::new(0, 0, 120, 45);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let zoom_out = layout.span_controls.zoom_out.unwrap();
    let zoom_in = layout.span_controls.zoom_in.unwrap();

    app.on_mouse(mouse_move(zoom_out.x + 1, zoom_out.y), screen);
    assert_eq!(app.graph_hovered_target, None);
    let rendered = render_app_to_buffer(&app, screen.width, screen.height);
    assert_eq!(
        rendered[(zoom_out.x + 1, zoom_out.y)].fg,
        app.theme().exited
    );

    app.on_mouse(left_click(zoom_out.x + 1, zoom_out.y), screen);
    assert!(app.graph_show_all_samples);
    assert_eq!(app.effective_graph_time_span_seconds(), 240);

    app.on_mouse(left_click(zoom_in.x + 1, zoom_in.y), screen);
    assert!(!app.graph_show_all_samples);
    assert_eq!(app.graph_time_span_seconds, 120);
}

#[test]
fn compact_workspace_keeps_process_row_panel_title_remove_and_resize_message() {
    let mut app = make_test_app(3, 10);
    add_test_graph(&mut app, 0);
    app.show_samples_panel = false;
    let screen = Rect::new(0, 0, 120, 25);
    app::sync_layout_state(&mut app, screen);
    let panels = main_panel_areas_for_app(screen, &app);
    let layout = ui::layout::graph_workspace_layout(panels.details.unwrap(), &app);

    assert!(layout.compact);
    assert_eq!(layout.graph_cards.len(), 1);
    assert!(layout.graph_cards[0].title.height > 0);
    assert!(layout.graph_cards[0].remove.width > 0);
    assert!(panels.processes.page_size >= 1);

    let rendered = render_app_to_text(&app, screen.width, screen.height);
    assert!(
        rendered.contains("GRAPHS  · 1 Slot · Span 60s"),
        "{rendered}"
    );
    assert!(rendered.contains("Slot#1"), "{rendered}");
    assert!(rendered.contains("[x]"), "{rendered}");
    assert!(
        rendered.contains("Resize terminal to view Graph"),
        "{rendered}"
    );
}

#[test]
fn long_graph_target_keeps_remove_button_visible_at_narrow_width() {
    let mut app = make_test_app(1, 10);
    let mut row = app.snapshot.processes[0].clone();
    row.name = "a-very-long-process-name-that-must-be-truncated.exe".to_string();
    app.add_or_reveal_graph_source(
        GraphSlot::process(
            ProcessIdentity::from_row(&row),
            DetailsMetric::WorksetPrivate,
        ),
        FocusedPanel::Processes,
    );
    app.show_samples_panel = false;
    let screen = Rect::new(0, 0, 60, 45);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);

    assert_eq!(layout.graph_cards.len(), 1);
    assert_eq!(layout.graph_cards[0].remove.width, 5);
    assert_eq!(layout.graph_cards[0].remove_label, "[x]");
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("[x]"), "{rendered}");
    let (remove_x, remove_y) =
        find_text_position(&buffer, "[x]").expect("remove control should render");
    assert_eq!(buffer[(remove_x, remove_y)].fg, THEMES[0].muted);
    assert_ne!(buffer[(remove_x, remove_y)].fg, THEMES[0].warning);
}

#[test]
fn ctrl_t_opens_investigation_profiles() {
    let mut app = make_test_app(1, 10);

    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();

    assert_eq!(app.status, "Investigation Profiles");
    assert!(app.investigation_profiles_dialog.is_some());
    assert!(app.has_modal_focus());
}

#[test]
fn changing_layout_never_hides_or_removes_registered_graphs() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    for index in 1..5 {
        add_test_graph(&mut app, index);
    }
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.last_screen_area = Rect::new(0, 0, 99, 60);
    let ids = app
        .graph_entries
        .iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
    assert_eq!(
        app.graph_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert!(!app.show_display_area_warning);
}

#[test]
fn samples_temporary_collapse_is_distinct_from_user_preference() {
    let mut app = make_test_app(30, 10);
    for index in 0..3 {
        add_test_graph(&mut app, index);
    }
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.show_samples_panel = true;
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.set_screen_area(Rect::new(0, 0, 70, 40));
    app.sync_graph_layout_visibility();
    assert!(app.show_samples_panel);
    assert!(app.samples_temporarily_collapsed);
    assert!(!app.effective_show_samples_panel());

    app.set_screen_area(Rect::new(0, 0, 180, 80));
    app.sync_graph_layout_visibility();
    assert!(app.show_samples_panel);
    assert!(!app.samples_temporarily_collapsed);
    assert!(app.effective_show_samples_panel());

    app.toggle_samples_panel();
    app.set_screen_area(Rect::new(0, 0, 220, 100));
    app.sync_graph_layout_visibility();
    assert!(!app.show_samples_panel);
    assert!(!app.effective_show_samples_panel());
}

#[test]
fn graph_shared_samples_and_layout_checkboxes_work_with_mouse() {
    let screen = Rect::new(0, 0, 120, 60);
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    add_test_graph(&mut app, 1);
    app.last_screen_area = screen;
    let initial = render_app_to_buffer(&app, screen.width, screen.height);
    let (layout_x, layout_y) =
        find_text_position(&initial, "l: Auto").expect("layout control should render");

    app.on_mouse(left_click(layout_x, layout_y), screen);

    assert_eq!(app.graph_slot_layout, GraphSlotLayout::OneColumn);
    assert!(app.show_samples_panel);
    let one_column = render_app_to_buffer(&app, screen.width, screen.height);
    let (layout_x, layout_y) =
        find_text_position(&one_column, "l: 1 col").expect("layout control should render");

    app.on_mouse(left_click(layout_x, layout_y), screen);

    assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
    assert!(app.show_samples_panel);
    let two_columns = render_app_to_buffer(&app, screen.width, screen.height);
    let (samples_x, samples_y) =
        find_text_position(&two_columns, "v: Samples").expect("Samples checkbox should render");

    app.on_mouse(left_click(samples_x, samples_y), screen);

    assert_eq!(app.graph_slot_layout, GraphSlotLayout::TwoColumns);
    assert!(!app.show_samples_panel);

    let (layout_x, layout_y) =
        find_text_position(&two_columns, "l: 1/2 cols").expect("layout control should render");
    app.on_mouse(left_click(layout_x, layout_y), screen);
    assert_eq!(app.graph_slot_layout, GraphSlotLayout::ThreeColumns);
}

#[test]
fn graph_y_axis_checkbox_uses_box_symbols() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    let rendered = render_app_to_text(&app, 120, 45);
    assert!(rendered.contains("☑  z: Min 0"), "{rendered}");

    app.graph_y_axis_zero_min = false;
    let rendered = render_app_to_text(&app, 120, 45);
    assert!(rendered.contains("☐  z: Min 0"), "{rendered}");
}

#[test]
fn graph_shared_controls_follow_footer_shortcut_color_roles_in_all_color_schemes() {
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        let mut app = make_test_app(3, 10);
        app.theme_index = theme_index;
        assign_private_graph(&mut app);
        app.show_samples_panel = true;
        app.show_sample_delta = true;
        app.graph_slot_layout = GraphSlotLayout::ThreeColumns;
        app.graph_show_all_samples = false;
        app.graph_y_axis_zero_min = true;

        let buffer = render_app_to_buffer(&app, 140, 45);

        let (checked_x, checked_y) = find_text_position(&buffer, "☑  v: Samples")
            .expect("checked Graph option should render");
        assert_eq!(buffer[(checked_x, checked_y)].fg, theme.accent);
        assert_eq!(buffer[(checked_x + 3, checked_y)].fg, theme.key_hint);
        assert_eq!(buffer[(checked_x + 4, checked_y)].fg, theme.muted);
        assert_eq!(buffer[(checked_x + 6, checked_y)].fg, theme.text);

        let (layout_x, layout_y) =
            find_text_position(&buffer, "l: 1/3 cols").expect("layout option should render");
        assert_eq!(buffer[(layout_x, layout_y)].fg, theme.key_hint);
        assert_eq!(buffer[(layout_x + 1, layout_y)].fg, theme.muted);
        assert_eq!(buffer[(layout_x + 3, layout_y)].fg, theme.text);

        let (unchecked_x, unchecked_y) = find_text_position(&buffer, "☐  f: Fit all")
            .expect("unchecked Graph option should render");
        assert_eq!(buffer[(unchecked_x, unchecked_y)].fg, theme.muted);
        assert_eq!(buffer[(unchecked_x + 3, unchecked_y)].fg, theme.key_hint);
        assert_eq!(buffer[(unchecked_x + 4, unchecked_y)].fg, theme.muted);
        assert_eq!(buffer[(unchecked_x + 6, unchecked_y)].fg, theme.text);
    }
}

#[test]
fn clicking_graph_card_selects_graph_without_changing_process_row() {
    let mut app = make_test_app(4, 10);
    let target = ProcessIdentity::from_row(&app.snapshot.processes[2]);
    app.add_or_reveal_graph_source(
        GraphSlot::process(target, DetailsMetric::Private),
        FocusedPanel::Processes,
    );
    let target_id = app.graph_entries[0].id;
    add_test_graph(&mut app, 1);
    app.show_samples_panel = false;
    app.select_process_index(0);

    let screen = Rect::new(0, 0, 140, 80);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let card = layout
        .graph_cards
        .iter()
        .find(|card| card.id == target_id)
        .unwrap();
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: card.title.x,
            row: card.title.y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert_eq!(app.active_graph_id, Some(target_id));
    assert_eq!(app.process_table_state.selected(), Some(0));
    assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
}

#[test]
fn process_metric_double_click_adds_then_removes_only_that_graph() {
    let mut app = make_test_app(2, 10);
    app.process_columns = vec![MetricColumn::HandleCount];
    app.selected_process_column_index = 2;
    let screen = Rect::new(0, 0, 140, 60);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, _) = find_text_position(&buffer, "Hndl").expect("handle column should render");
    let y = main_panel_areas_for_app(screen, &app).processes.area.y + 2;
    let source = app.selected_process_graph_source().unwrap();

    app.on_mouse(left_click(x, y), screen);
    assert!(app.graph_entries.is_empty());
    app.on_mouse(left_click(x, y), screen);

    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(app.graph_entries[0].source, source);
    let source_id = app.graph_entries[0].id;
    let other_id = add_test_graph(&mut app, 8);
    assert_eq!(app.active_graph_id, Some(other_id));

    app.on_mouse(left_click(x, y), screen);
    app.on_mouse(left_click(x, y), screen);

    assert_eq!(app.graph_entries.len(), 1);
    assert!(app.graph_entry_by_id(source_id).is_none());
    assert!(app.graph_entry_by_id(other_id).is_some());
    assert_eq!(app.active_graph_id, Some(other_id));
}

#[test]
fn process_metric_double_click_requires_the_same_visible_process_identity() {
    let mut app = make_test_app(2, 10);
    app.process_columns = vec![MetricColumn::HandleCount];
    app.sort = SortSpec {
        column: SortColumn::Pid,
        direction: SortDirection::Asc,
    };
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    let screen = Rect::new(0, 0, 140, 60);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let (x, _) = find_text_position(&buffer, "Hndl").expect("handle column should render");
    let y = main_panel_areas_for_app(screen, &app).processes.area.y + 2;

    app.on_mouse(left_click(x, y), screen);
    app.snapshot.processes[0].pid = 500;
    app.snapshot.processes[0].name = "reordered.exe".to_string();
    app.snapshot.processes[0].start_time = Some(1_900_000_000);
    app.rebuild_visible_process_cache();
    app.clamp_process_table_state();
    let visible_identity = ProcessIdentity::from_row(app.visible_process_at(0).unwrap());

    app.on_mouse(left_click(x, y), screen);
    assert!(app.graph_entries.is_empty());
    app.on_mouse(left_click(x, y), screen);

    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(
        app.graph_entries[0].source,
        GraphSlot::process(visible_identity, DetailsMetric::HandleCount)
    );
}

#[test]
fn process_identity_cell_double_click_opens_info_without_tracking_or_graph_changes() {
    let mut app = make_test_app(2, 10);
    app.process_columns = vec![MetricColumn::HandleCount];
    let screen = Rect::new(0, 0, 140, 60);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let process_area = main_panel_areas_for_app(screen, &app).processes.area;
    let (pid_x, _) =
        find_text_position_in_area(&buffer, process_area, "PID").expect("PID column should render");
    let (process_x, _) = find_text_position_in_area(&buffer, process_area, "Process")
        .expect("Process column should render");
    let y = process_area.y + 2;
    let selected_name = app.snapshot.processes[0].name.clone();

    app.on_mouse(left_click(pid_x, y), screen);
    app.on_mouse(left_click(process_x, y), screen);
    assert!(app.watch_list.is_empty());

    app.on_mouse(left_click(process_x, y), screen);
    assert!(app.watch_list.is_empty());
    assert!(app.show_process_info_dialog);
    assert_eq!(
        app.process_info_target.as_ref().unwrap().identity.name,
        selected_name
    );
    app.close_process_info_dialog();

    app.on_mouse(left_click(pid_x, y), screen);
    app.on_mouse(left_click(pid_x, y), screen);

    assert!(app.watch_list.is_empty());
    assert!(app.show_process_info_dialog);
    assert!(app.graph_entries.is_empty());
}

#[test]
fn semantic_double_click_rejects_changed_identity_metric_and_timeout() {
    let mut app = make_test_app(1, 10);
    let first = test_graph_source(&app, 0);
    let second = test_graph_source(&app, 1);
    let start = Instant::now();

    app.register_graph_source_click(first.clone(), start, FocusedPanel::Processes);
    app.register_graph_source_click(
        second.clone(),
        start + Duration::from_millis(100),
        FocusedPanel::Processes,
    );
    assert!(app.graph_entries.is_empty());

    app.register_graph_source_click(
        second.clone(),
        start + Duration::from_millis(200),
        FocusedPanel::Processes,
    );
    assert_eq!(app.graph_entries.len(), 1);
    assert_eq!(app.graph_entries[0].source, second);

    let third = test_graph_source(&app, 2);
    app.register_graph_source_click(
        third.clone(),
        start + Duration::from_millis(300),
        FocusedPanel::Processes,
    );
    app.register_graph_source_click(
        third,
        start + Duration::from_millis(801),
        FocusedPanel::Processes,
    );
    assert_eq!(app.graph_entries.len(), 1);

    let different_metric = match first {
        GraphSlot::Process { identity, .. } => {
            GraphSlot::process(identity, DetailsMetric::WorksetPrivate)
        }
        GraphSlot::System { .. } | GraphSlot::Gpu { .. } => unreachable!(),
    };
    app.register_graph_source_click(
        test_graph_source(&app, 3),
        start + Duration::from_millis(900),
        FocusedPanel::Processes,
    );
    app.register_graph_source_click(
        different_metric,
        start + Duration::from_millis(950),
        FocusedPanel::Processes,
    );
    assert_eq!(app.graph_entries.len(), 1);
}

#[test]
fn system_panel_double_clicks_toggle_ram_activity_and_cpu_graphs() {
    let mut app = make_test_app(2, 10);
    let screen = Rect::new(0, 0, 180, 70);
    app::sync_layout_state(&mut app, screen);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let points = [
        (
            find_text_position(&buffer, "In use").unwrap(),
            GraphSlot::system(SystemMetric::PhysicalMemory),
        ),
        (
            find_text_position(&buffer, "Pages Out/s").unwrap(),
            GraphSlot::system(SystemMetric::PagesOutput),
        ),
        (
            find_text_position(&buffer, "Net Rx").unwrap(),
            GraphSlot::system(SystemMetric::NetworkReceived),
        ),
        (
            find_text_position_in_area(
                &buffer,
                ui::cpu_panel_area_for_screen(screen, &app),
                "Usage",
            )
            .unwrap(),
            GraphSlot::system(SystemMetric::CpuAverage),
        ),
        (
            find_text_position_in_area(
                &buffer,
                ui::cpu_panel_area_for_screen(screen, &app),
                "Processes",
            )
            .unwrap(),
            GraphSlot::system(SystemMetric::ProcessCount),
        ),
    ];

    for ((x, y), expected) in points {
        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);
        assert!(app.graph_id_for_source(&expected).is_some());

        app.on_mouse(left_click(x, y), screen);
        app.on_mouse(left_click(x, y), screen);
        assert!(app.graph_id_for_source(&expected).is_none());
    }
    assert!(app.graph_entries.is_empty());
}

#[test]
fn graph_panel_title_omits_the_verbose_slot_list() {
    let mut app = make_test_app(1, 10);
    for index in 0..8 {
        add_test_graph(&mut app, index);
    }
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.show_samples_panel = false;
    let selected_at = app.snapshot.captured_at;
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint {
            captured_at: selected_at - chrono::Duration::seconds(10),
        }),
        b: Some(app::AbComparisonPoint {
            captured_at: selected_at,
        }),
    });
    let screen = Rect::new(0, 0, 120, 48);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let title_row = (layout.graph_slots.x..layout.graph_slots.right())
        .map(|x| buffer[(x, layout.graph_slots.y)].symbol())
        .collect::<String>();

    assert!(
        title_row.contains("GRAPHS  · 8 Slots · Span 60s"),
        "{title_row}"
    );
    assert!(!title_row.contains("graph-"), "{title_row}");
    assert!(!title_row.contains("Cursor"), "{title_row}");
    assert!(!title_row.contains("· A "), "{title_row}");
    assert!(!title_row.contains("· B "), "{title_row}");
    let (metadata_x, metadata_y) =
        find_text_position_in_area(&buffer, layout.graph_slots, "8 Slots")
            .expect("Graph slot metadata should render in the panel title");
    assert_eq!(buffer[(metadata_x, metadata_y)].fg, app.theme().muted);
    assert!(
        !buffer[(metadata_x, metadata_y)]
            .modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn graph_remove_button_has_priority_and_preserves_non_active_selection() {
    let mut app = make_test_app(1, 10);
    let ids = (0..3)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    app.show_samples_panel = false;
    let active = ids[2];
    let screen = Rect::new(0, 0, 160, 80);
    app::sync_layout_state(&mut app, screen);
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let remove = layout
        .graph_cards
        .iter()
        .find(|card| card.id == ids[0])
        .unwrap()
        .remove;

    app.on_mouse(left_click(remove.x, remove.y), screen);

    assert!(app.graph_entry_by_id(ids[0]).is_none());
    assert_eq!(app.active_graph_id, Some(active));
    assert_eq!(app.graph_entries.len(), 2);
}

#[test]
fn graph_scrollbar_track_click_and_thumb_drag_reach_later_rows() {
    let mut app = make_test_app(1, 10);
    let ids = (0..8)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.show_samples_panel = false;
    let screen = Rect::new(0, 0, 100, 48);
    app::sync_layout_state(&mut app, screen);
    assert!(app.set_active_graph(ids[0]));
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    let scrollbar = layout.graph_scrollbar.expect("graph scrollbar");
    let bottom = scrollbar.bottom().saturating_sub(2);

    app.on_mouse(left_click(scrollbar.x, bottom), screen);
    assert!(app.graph_scroll_row > 0);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar.x,
            row: bottom,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    app.set_graph_scroll_row(0);
    app.on_mouse(
        left_click(scrollbar.x, scrollbar.y.saturating_add(1)),
        screen,
    );
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar.x,
            row: bottom,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(app.graph_scroll_row, layout.max_scroll_row);
}

#[test]
fn samples_wheel_does_not_scroll_graph_workspace_rows() {
    let mut app = make_test_app(1, 10);
    let ids = (0..8)
        .map(|index| add_test_graph(&mut app, index))
        .collect::<Vec<_>>();
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.show_samples_panel = true;
    let screen = Rect::new(0, 0, 180, 60);
    app::sync_layout_state(&mut app, screen);
    assert!(app.set_active_graph(ids[0]));
    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let samples = ui::layout::graph_workspace_layout(details, &app)
        .samples
        .expect("Samples inspector");
    assert_eq!(app.graph_scroll_row, 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: samples.x.saturating_add(1),
            row: samples.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );

    assert_eq!(app.graph_scroll_row, 0);
    assert_eq!(app.focused_panel, FocusedPanel::DetailsSamples);
}

#[test]
fn increasing_process_height_keeps_the_active_graph_reachable() {
    let mut app = make_test_app(30, 10);
    let active_id = (0..8)
        .map(|index| add_test_graph(&mut app, index))
        .last()
        .unwrap();
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.show_samples_panel = false;
    app.focused_panel = FocusedPanel::DetailsGraph;
    let screen = Rect::new(0, 0, 100, 60);
    app::sync_layout_state(&mut app, screen);

    for _ in 0..10 {
        app.on_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .unwrap();
    }
    app::sync_layout_state(&mut app, screen);

    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    assert_eq!(app.active_graph_id, Some(active_id));
    assert!(
        layout.graph_cards.iter().any(|card| card.id == active_id),
        "active Graph should remain in the visible viewport: {layout:?}"
    );
}

#[test]
fn graph_current_line_label_draws_selected_value_in_accent() {
    let mut app = make_test_app(1, 10);
    app.snapshot.processes[0].private_bytes = Some(424_242);
    assign_private_graph(&mut app);
    app.show_samples_panel = false;
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.select_details_sample_latest();

    let screen = Rect::new(0, 0, 120, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let mut found_accent_value = false;
    for y in 0..screen.height {
        for x in 0..screen.width {
            let row = (x..screen.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if row.starts_with("424,242") && buffer[(x, y)].fg == ui::THEMES[0].accent {
                found_accent_value = true;
            }
        }
    }
    assert!(found_accent_value, "current value label should use accent");
}

#[test]
fn moving_average_graph_labels_smoothed_cursor_but_keeps_raw_ab_and_samples() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    let base = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap();
    for (seconds, value) in [(0, 100), (1, 200), (2, 300), (3, 400), (4, 500)] {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(value);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
    }
    app.select_details_sample_oldest();
    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    app.select_details_sample_latest();
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();
    let screen = Rect::new(0, 0, 180, 55);
    let raw_rendered = render_app_to_text(&app, screen.width, screen.height);
    app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE))
        .unwrap();

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let graph = details_graph_area_for_app(screen, &app).unwrap();
    let rendered = buffer_to_text(&buffer);

    assert!(
        find_text_position_in_area(&buffer, graph, "MA5: 300").is_some(),
        "{rendered}"
    );
    assert!(rendered.contains("[MA]"), "{rendered}");
    assert!(rendered.contains("B-A: +400"), "{rendered}");
    assert!(rendered.contains("History max: 500"), "{rendered}");
    assert!(rendered.contains("MA5: 300"), "{rendered}");
    for expected in [
        "A-B min: 100 @ 10:00:00",
        "A-B max: 500 @ 10:00:04",
        "A-B avg: 300",
        "A-B samples: 5/5  Missing: 0",
    ] {
        assert!(raw_rendered.contains(expected), "{raw_rendered}");
        assert!(rendered.contains(expected), "{rendered}");
    }
}

#[test]
fn ab_range_statistics_recalculate_per_graph_without_merging_reused_pid_identities() {
    let mut app = make_test_app(1, 10);
    let base = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap();
    let mut old_process = app.snapshot.processes[0].clone();
    old_process.pid = 42;
    old_process.name = "reused.exe".to_string();
    old_process.start_time = Some(1_700_000_001);
    let mut new_process = old_process.clone();
    new_process.start_time = Some(1_700_000_002);
    let old_identity = ProcessIdentity::from_row(&old_process);
    let new_identity = ProcessIdentity::from_row(&new_process);

    for (seconds, mut process) in [
        (0, old_process.clone()),
        (1, old_process),
        (2, new_process.clone()),
        (3, new_process),
    ] {
        process.private_bytes = Some(match seconds {
            0 => 10,
            1 => 20,
            2 => 100,
            _ => 200,
        });
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes = vec![process];
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
    }

    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(old_identity, DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
    let old_graph = app.active_graph_id.unwrap();
    assert!(app.add_or_reveal_graph_source(
        GraphSlot::process(new_identity, DetailsMetric::Private),
        FocusedPanel::Processes,
    ));
    let new_graph = app.active_graph_id.unwrap();
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: base }),
        b: Some(app::AbComparisonPoint {
            captured_at: base + chrono::Duration::seconds(3),
        }),
    });
    let screen = Rect::new(0, 0, 180, 60);

    assert!(app.set_active_graph(old_graph));
    let old_rendered = render_app_to_text(&app, screen.width, screen.height);
    for expected in [
        "A-B min: 10 @ 10:00:00",
        "A-B max: 20 @ 10:00:01",
        "A-B avg: 15",
        "A-B samples: 2/4  Missing: 2",
    ] {
        assert!(old_rendered.contains(expected), "{old_rendered}");
    }

    assert!(app.set_active_graph(new_graph));
    let new_rendered = render_app_to_text(&app, screen.width, screen.height);
    for expected in [
        "A-B min: 100 @ 10:00:02",
        "A-B max: 200 @ 10:00:03",
        "A-B avg: 150",
        "A-B samples: 2/4  Missing: 2",
    ] {
        assert!(new_rendered.contains(expected), "{new_rendered}");
    }
}

#[test]
fn ab_range_summary_keeps_samples_accessible_on_narrow_and_short_screens() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    let base = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap();
    for (seconds, value) in [(0, 10), (1, 20)] {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(value);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
    }
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: base }),
        b: Some(app::AbComparisonPoint {
            captured_at: base + chrono::Duration::seconds(1),
        }),
    });

    for screen in [Rect::new(0, 0, 90, 55), Rect::new(0, 0, 120, 30)] {
        app::sync_layout_state(&mut app, screen);
        let rendered = render_app_to_text(&app, screen.width, screen.height);
        assert!(rendered.contains("A/B Time"), "{screen:?}\n{rendered}");
        assert!(
            rendered.contains("A-B min: 10 @ 10:00:00"),
            "{screen:?}\n{rendered}"
        );
        assert!(
            rendered.contains("A-B samples: 2/2  Missing: 0"),
            "{screen:?}\n{rendered}"
        );
    }
}

#[test]
fn ab_range_summary_survives_delta_toggle_at_reported_size() {
    let mut app = make_test_app(30, 10);
    let identity = app.selected_visible_process_identity().unwrap();
    for metric in [
        DetailsMetric::Private,
        DetailsMetric::Workset,
        DetailsMetric::WorksetPrivate,
        DetailsMetric::WorksetShareable,
    ] {
        assert!(app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), metric),
            FocusedPanel::Processes,
        ));
    }
    let base = Local.with_ymd_and_hms(2026, 5, 26, 8, 0, 0).unwrap();
    for (seconds, value) in [(0, 242_569_210), (1, 242_655_232)] {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(value);
        app.snapshot.processes[0].workset_bytes = Some(value);
        app.snapshot.processes[0].workset_private_bytes = Some(value);
        app.snapshot.processes[0].workset_shareable_bytes = Some(value);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
    }
    app.ab_comparison = Some(app::AbComparison {
        a: Some(app::AbComparisonPoint { captured_at: base }),
        b: Some(app::AbComparisonPoint {
            captured_at: base + chrono::Duration::seconds(1),
        }),
    });
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.show_sample_delta = false;
    let screen = Rect::new(0, 0, 140, 45);

    for show_delta in [false, true, false] {
        if app.show_sample_delta != show_delta {
            app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
                .unwrap();
        }
        app::sync_layout_state(&mut app, screen);
        let details = main_panel_areas_for_app(screen, &app).details.unwrap();
        let layout = ui::layout::graph_workspace_layout(details, &app);
        let rendered = render_app_to_text(&app, screen.width, screen.height);
        assert_eq!(app.show_sample_delta, show_delta);
        assert!(
            layout
                .graph_cards
                .iter()
                .any(|card| Some(card.id) == app.active_graph_id),
            "Delta={show_delta} active Graph should remain visible: {layout:?}"
        );
        for expected in [
            "A-B min: 242,569,210 @ 08:00:00",
            "A-B max: 242,655,232 @ 08:00:01",
            "A-B avg: 242,612,221",
            "A-B samples: 2/2  Missing: 0",
        ] {
            assert!(
                rendered.contains(expected),
                "Delta={show_delta}\n{rendered}"
            );
        }
    }
}

#[test]
fn graph_ab_labels_render_on_x_axis_not_cursor_value_row() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    app.show_sample_delta = true;
    let base = Local.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap();
    for (seconds, value) in [(0, 100), (30, 200), (60, 424_242)] {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(value);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
    }
    app.select_details_sample_latest();
    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    app.select_details_sample_oldest();
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .unwrap();
    app.select_details_sample_latest();

    let screen = Rect::new(0, 0, 120, 45);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let graph = details_graph_area_for_app(screen, &app).unwrap();
    let (_, value_y) = find_text_position_in_area(&buffer, graph, "424,242")
        .expect("selected graph value should render in graph");
    let a_labels = find_styled_symbol_positions_in_area(&buffer, graph, "A", THEMES[0].accent);
    // The cursor now includes the byte unit B; only the remaining B is an axis marker.
    let b_labels = find_styled_symbol_positions_in_area(&buffer, graph, "B", THEMES[0].accent)
        .into_iter()
        .filter(|(_, y)| *y != value_y)
        .collect::<Vec<_>>();
    let graph_rows = ui::layout::details_graph_rows(graph);
    let expected_label_y = graph_rows[1].bottom().saturating_sub(1);

    assert_eq!(a_labels.len(), 1, "A label should render once in Graph");
    assert_eq!(b_labels.len(), 1, "B label should render once in Graph");
    assert_eq!(a_labels[0].1, expected_label_y);
    assert_eq!(b_labels[0].1, expected_label_y);
    assert!(a_labels[0].1 > value_y);
    assert!(b_labels[0].1 > value_y);
}

#[test]
fn details_rendering_shows_workspace_title_and_active_samples() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    let rendered = render_app_to_text(&app, 120, 45);

    assert!(
        rendered.contains("GRAPHS  · 1 Slot · Span 60s"),
        "{rendered}"
    );
    assert!(!rendered.contains("Slot#1/"), "{rendered}");
    assert!(
        rendered.contains("Slot#1 · PrivBytes · PID 0 proc-0"),
        "{rendered}"
    );
    assert!(rendered.contains("SAMPLES  · Slot#1"), "{rendered}");
    assert!(!rendered.contains("SAMPLES  · Slot#1/"), "{rendered}");
    assert!(rendered.contains("A/B Time      PrivBytes"), "{rendered}");
    assert!(rendered.contains("MA5:"), "{rendered}");
    assert!(!rendered.contains("Details"), "{rendered}");
    assert!(!rendered.contains("A/B not set"), "{rendered}");
}

#[test]
fn multi_graph_rendering_uses_one_shared_samples_inspector() {
    let mut app = make_test_app(3, 10);
    app.set_screen_area(Rect::new(0, 0, 140, 80));
    let identity = app.selected_visible_process_identity().unwrap();
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    );
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::Workset),
        FocusedPanel::Processes,
    );
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );

    let rendered = render_app_to_text(&app, 140, 80);

    assert!(
        rendered.contains("Slot#1 · PrivBytes · PID 0 proc-0"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Slot#2 · WS · PID 0 proc-0"),
        "{rendered}"
    );
    assert_eq!(rendered.matches("B-A: --").count(), 2, "{rendered}");
    assert_eq!(rendered.matches("f: Fit all").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("z: Min 0").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("v: Samples").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("d: Delta").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("l: Auto").count(), 1, "{rendered}");
    assert_eq!(
        rendered.matches("SAMPLES  · Slot#2 · PID 0 proc-0").count(),
        1,
        "{rendered}"
    );

    let buffer = render_app_to_buffer(&app, 140, 80);
    let (active_x, active_y) =
        find_text_position(&buffer, "Slot#2 · WS").expect("active Graph title should render");
    let (inactive_x, inactive_y) = find_text_position(&buffer, "Slot#1 · PrivBytes")
        .expect("inactive Graph title should render");
    assert_eq!(buffer[(active_x + 9, active_y)].fg, THEMES[0].text);
    assert_eq!(buffer[(inactive_x + 9, inactive_y)].fg, THEMES[0].muted);
}

#[test]
fn four_graphs_render_in_a_two_by_two_row_major_grid() {
    let mut app = make_test_app(3, 10);
    app.set_screen_area(Rect::new(0, 0, 140, 80));
    let identity = app.selected_visible_process_identity().unwrap();
    for metric in [
        DetailsMetric::Private,
        DetailsMetric::Workset,
        DetailsMetric::CpuPercent,
        DetailsMetric::IoRead,
    ] {
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity.clone(), metric),
            FocusedPanel::Processes,
        );
    }
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.show_samples_panel = false;
    let details = main_panel_areas_for_app(Rect::new(0, 0, 140, 80), &app)
        .details
        .unwrap();
    let cards = ui::layout::graph_workspace_layout(details, &app).graph_cards;

    assert_eq!(cards.len(), 4);
    assert!(cards[0].area.x < cards[1].area.x);
    assert_eq!(cards[0].area.y, cards[1].area.y);
    assert_eq!(cards[0].area.x, cards[2].area.x);
    assert!(cards[0].area.y < cards[2].area.y);
    assert_eq!(cards[2].area.y, cards[3].area.y);
}

#[test]
fn one_column_graphs_share_compact_y_axis_width_and_keep_samples_exact() {
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 140, 80);
    app.set_screen_area(screen);
    let identity = app.selected_visible_process_identity().unwrap();
    app.snapshot.processes[0].private_bytes = Some(5_900_000);
    app.snapshot.processes[0].handle_count = Some(42);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    );
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::HandleCount),
        FocusedPanel::Processes,
    );
    app.graph_slot_layout = GraphSlotLayout::OneColumn;
    app.show_samples_panel = true;

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("Slot#1 · PrivBytes"), "{rendered}");
    assert!(rendered.contains("Slot#2 · Hndl"), "{rendered}");
    assert!(rendered.contains("5.9 MB"), "{rendered}");
    assert!(rendered.contains("5,900,000"), "{rendered}");

    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    let layout = ui::layout::graph_workspace_layout(details, &app);
    for card in layout.graph_cards {
        let chart = ui::layout::details_graph_rows(card.plot)[1];
        assert!(buffer[(chart.x, chart.y)].symbol() == " " || chart.width > 0);
    }
}

#[test]
fn two_column_graphs_share_compact_y_axis_width() {
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 140, 80);
    app.set_screen_area(screen);
    let identity = app.selected_visible_process_identity().unwrap();
    app.snapshot.processes[0].private_bytes = Some(5_900_000);
    app.snapshot.processes[0].handle_count = Some(42);
    app.process_history.record_snapshot(
        app.snapshot.captured_at,
        &app.snapshot.processes,
        &app.normalized_watch_names,
    );
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity.clone(), DetailsMetric::Private),
        FocusedPanel::Processes,
    );
    app.add_or_reveal_graph_source(
        GraphSlot::process(identity, DetailsMetric::HandleCount),
        FocusedPanel::Processes,
    );
    app.graph_slot_layout = GraphSlotLayout::TwoColumns;
    app.show_samples_panel = false;

    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let rendered = buffer_to_text(&buffer);
    assert!(rendered.contains("Slot#1 · PrivBytes"), "{rendered}");
    assert!(rendered.contains("Slot#2 · Hndl"), "{rendered}");
    assert!(rendered.contains("5.9 MB"), "{rendered}");

    let details = main_panel_areas_for_app(screen, &app).details.unwrap();
    assert_eq!(
        ui::layout::graph_workspace_layout(details, &app)
            .graph_cards
            .len(),
        2
    );
}

#[test]
fn narrow_graphs_keep_same_name_pids_and_complete_byte_deltas() {
    let mut app = make_test_app(3, 10);
    for process in &mut app.snapshot.processes {
        process.name = "a-very-long-shared-process-name.exe".to_string();
        process.private_bytes = Some(100);
    }
    let a = app.snapshot.captured_at;
    app.process_history
        .record_snapshot(a, &app.snapshot.processes, &app.normalized_watch_names);
    for process in &mut app.snapshot.processes {
        process.private_bytes = Some(3_321_956);
    }
    let b = a + chrono::Duration::seconds(1);
    app.process_history
        .record_snapshot(b, &app.snapshot.processes, &app.normalized_watch_names);
    let identities: Vec<_> = app
        .snapshot
        .processes
        .iter()
        .map(ProcessIdentity::from_row)
        .collect();
    for identity in identities {
        app.add_or_reveal_graph_source(
            GraphSlot::process(identity, DetailsMetric::Private),
            FocusedPanel::Processes,
        );
    }
    app.ab_comparison = Some(crate::app::AbComparison {
        a: Some(crate::app::AbComparisonPoint { captured_at: a }),
        b: Some(crate::app::AbComparisonPoint { captured_at: b }),
    });
    app.graph_slot_layout = GraphSlotLayout::ThreeColumns;
    app.show_samples_panel = false;
    let text = render_app_to_text(&app, 120, 60);
    for pid in 0..3 {
        assert!(text.contains(&format!("PID {pid}")), "{text}");
    }
    assert_eq!(text.matches("B-A: +3,321,856 B").count(), 3, "{text}");
    assert!(text.contains("l: 2/3 cols"), "{text}");
}

#[test]
fn graph_cursor_units_and_summary_scopes_remain_clear_at_review_size() {
    let mut app = make_test_app(1, 10);
    assign_private_graph(&mut app);
    let base = Local.with_ymd_and_hms(2026, 9, 17, 10, 0, 0).unwrap();
    for seconds in 0..6 {
        app.snapshot.captured_at = base + chrono::Duration::seconds(seconds);
        app.snapshot.processes[0].private_bytes = Some(540_426_240 + seconds as u64);
        app.process_history.record_snapshot(
            app.snapshot.captured_at,
            &app.snapshot.processes,
            &app.normalized_watch_names,
        );
        app.system_history.record_snapshot(&app.snapshot);
    }
    app.select_details_sample_latest();
    for show_samples in [true, false] {
        app.show_samples_panel = show_samples;
        for ab in [false, true] {
            app.ab_comparison = ab.then_some(app::AbComparison {
                a: Some(app::AbComparisonPoint { captured_at: base }),
                b: Some(app::AbComparisonPoint {
                    captured_at: base + chrono::Duration::seconds(5),
                }),
            });
            let screen = Rect::new(0, 0, 180, 60);
            app::sync_layout_state(&mut app, screen);
            let buffer = render_app_to_buffer(&app, 180, 60);
            let graph = details_graph_area_for_app(screen, &app).unwrap();
            assert!(find_text_position_in_area(&buffer, graph, "540,426,245 B").is_some());
            if show_samples {
                let text = buffer_to_text(&buffer);
                assert!(text.contains("History max:"), "{text}");
                assert!(text.contains("Selected MA5:"), "{text}");
                assert_eq!(text.contains("A-B max:"), ab, "{text}");
            }
        }
    }
}

#[test]
fn graph_workspace_border_stays_outside_cards_samples_and_scrollbar() {
    for screen in [Rect::new(0, 0, 180, 60), Rect::new(0, 0, 75, 60)] {
        for count in [1, 8] {
            for show_samples in [true, false] {
                let mut app = make_test_app(30, 10);
                for index in 0..count {
                    add_test_graph(&mut app, index);
                }
                app.graph_slot_layout = GraphSlotLayout::OneColumn;
                app.show_samples_panel = show_samples;
                app.focused_panel = FocusedPanel::DetailsGraph;
                app::sync_layout_state(&mut app, screen);
                let layout = ui::layout::graph_workspace_layout(
                    main_panel_areas_for_app(screen, &app).details.unwrap(),
                    &app,
                );
                let rect = layout.graph_slots;
                let buffer = render_app_to_buffer(&app, screen.width, screen.height);
                for (x, y, symbol) in [
                    (rect.x, rect.y, "┏"),
                    (rect.right() - 1, rect.y, "┓"),
                    (rect.x, rect.bottom() - 1, "┗"),
                    (rect.right() - 1, rect.bottom() - 1, "┛"),
                ] {
                    assert_eq!(buffer[(x, y)].symbol(), symbol);
                    assert_eq!(buffer[(x, y)].fg, app.theme().focus_border);
                }
                for card in &layout.graph_cards {
                    assert!(card.area.x > rect.x && card.area.right() < rect.right());
                    assert!(card.area.y > rect.y && card.area.bottom() < rect.bottom());
                    app.focused_panel = FocusedPanel::Processes;
                    app.on_mouse(left_click(card.title.x, card.title.y), screen);
                    assert_eq!(app.active_graph_id, Some(card.id));
                    assert_eq!(app.focused_panel, FocusedPanel::DetailsGraph);
                }
                if let Some(samples) = layout.samples {
                    assert!(!rect.intersects(samples));
                }
                if let Some(scrollbar) = layout.graph_scrollbar {
                    assert!(scrollbar.right() < rect.right());
                }
            }
        }
    }
}

#[test]
fn automatic_split_scales_with_terminal_height_and_keeps_graphs_readable() {
    let mut app = make_test_app(100, 10);
    for index in 0..4 {
        add_test_graph(&mut app, index);
    }
    for show_samples in [true, false] {
        app.show_samples_panel = show_samples;
        for (height, expected_rows) in [(45, 12), (60, 18), (90, 30), (60, 18)] {
            let screen = Rect::new(0, 0, 180, height);
            app::sync_layout_state(&mut app, screen);
            let panels = main_panel_areas_for_app(screen, &app);
            assert_eq!(panels.processes.body_capacity, expected_rows);
            let layout = ui::layout::graph_workspace_layout(panels.details.unwrap(), &app);
            assert!(!layout.compact);
            assert!(
                layout
                    .graph_cards
                    .iter()
                    .all(|card| card.area.height >= crate::app::GRAPH_SLOT_MIN_HEIGHT)
            );
            assert!(
                layout
                    .graph_cards
                    .iter()
                    .any(|card| Some(card.id) == app.active_graph_id)
            );
        }
    }
    app.process_panel_height = crate::app::ProcessPanelHeight::Manual(24);
    app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 45));
    app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 90));
    assert_eq!(
        app.process_panel_height,
        crate::app::ProcessPanelHeight::Manual(24)
    );
    assert_eq!(
        main_panel_areas_for_app(Rect::new(0, 0, 180, 90), &app)
            .processes
            .body_capacity,
        24
    );
}

#[test]
fn graph_state_text_stays_readable_in_every_appearance() {
    let mut app = make_test_app(2, 10);
    assign_private_graph(&mut app);
    for index in 0..4 {
        app.theme_index = index;
        for high_contrast in [false, true] {
            app.high_contrast = high_contrast;
            for focused in [false, true] {
                app.focused_panel = if focused {
                    FocusedPanel::DetailsGraph
                } else {
                    FocusedPanel::Processes
                };
                let buffer = render_app_to_buffer(&app, 180, 60);
                let (x, y) = find_text_position(&buffer, "1 Slot").unwrap();
                assert_eq!(
                    buffer[(x, y)].fg,
                    if focused {
                        app.theme().focus_border
                    } else {
                        app.theme().muted
                    }
                );
                let text = buffer_to_text(&buffer);
                assert!(text.contains("Follow latest"));
                assert!(text.contains("1 Slot"));
            }
        }
    }
}
