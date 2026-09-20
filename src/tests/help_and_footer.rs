use super::support::{
    assign_private_graph, find_symbol_position, find_text_position, find_text_position_in_area,
    make_test_app, render_app_to_buffer, render_app_to_text,
};
use crate::app::FocusedPanel;
use crate::ui;
use crate::ui::{help_area, help_scrollbar_area, main_panel_areas_for_app};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

#[test]
fn help_opens_with_f1_or_question_mark() {
    for key in [
        KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
    ] {
        let mut app = make_test_app(1, 10);

        app.on_key(key).unwrap();

        assert!(app.show_help);
    }
}

#[test]
fn help_closes_with_escape_enter_f1_or_question_mark() {
    for key in [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
    ] {
        let mut app = make_test_app(1, 10);
        app.show_help = true;

        app.on_key(key).unwrap();

        assert!(!app.show_help);
        assert!(!app.show_quit_confirmation);
    }
}

#[test]
fn help_blocks_normal_shortcuts_while_open() {
    let mut app = make_test_app(1, 10);
    app.show_help = true;

    app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.show_help);
    assert!(!app.show_quit_confirmation);
}

#[test]
fn help_dialog_buffer_shows_two_column_layout() {
    let mut app = make_test_app(3, 10);
    app.show_help = true;

    let rendered = render_app_to_text(&app, 240, 120);
    let rendered_lower = rendered.to_ascii_lowercase();

    assert!(
        rendered.contains(&format!(
            "winproc-tui {} · Keyboard shortcuts",
            env!("CARGO_PKG_VERSION")
        )),
        "{rendered}"
    );
    assert!(rendered.contains("Keyboard shortcuts"), "{rendered}");
    assert!(
        rendered.contains("History: 120/7,200 normal/tracked"),
        "{rendered}"
    );
    assert!(rendered.contains("Global  (any focus)"), "{rendered}");
    assert!(
        rendered.contains("Toggle Tracked-only (any main panel)"),
        "{rendered}"
    );
    assert!(rendered.contains("Processes"), "{rendered}");
    assert!(rendered.contains("Toggle Flat / Tree view"), "{rendered}");
    assert!(
        rendered.contains("Expand/collapse Tree row (no filter)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Expand/collapse subtree (no filter)"),
        "{rendered}"
    );
    assert!(rendered.contains("MEM/GPU"), "{rendered}");
    assert!(rendered.contains("NW/DISK"), "{rendered}");
    assert!(
        rendered.contains("Graph Workspace  (Graph focus)"),
        "{rendered}"
    );
    assert!(rendered.contains("Samples"), "{rendered}");
    assert!(
        rendered.contains("Tracking  (Processes focus)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("A/B comparison  (Graph or Samples)"),
        "{rendered}"
    );
    assert!(rendered.contains("Mouse"), "{rendered}");
    assert!(rendered.contains("Processes / Graph split"), "{rendered}");
    assert!(!rendered.contains("▋"), "{rendered}");

    assert!(rendered.contains("Set A range endpoint"), "{rendered}");
    assert!(
        rendered.contains("Set B; show range statistics"),
        "{rendered}"
    );
    assert!(rendered.contains("Jump to A or B"), "{rendered}");
    assert!(rendered.contains("Clear A/B comparison"), "{rendered}");

    assert!(rendered.contains("Pan time range"), "{rendered}");
    assert!(rendered.contains("Select previous Graph"), "{rendered}");
    assert!(rendered.contains("Select next Graph"), "{rendered}");
    assert!(rendered.contains("Select older sample"), "{rendered}");
    assert!(rendered.contains("Select newer sample"), "{rendered}");
    assert!(rendered.contains("Remove active Graph"), "{rendered}");
    assert!(
        rendered.contains("Toggle active Graph Raw / MA5"),
        "{rendered}"
    );
    assert!(
        rendered.contains("f/z") && rendered.contains("Fit all / compact Min0"),
        "{rendered}"
    );
    assert!(
        rendered.contains("v/d/l") && rendered.contains("Samples / Delta / layout"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Start recording / confirm stop"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Open Session menu (Quit is last)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Select item, switch menu (including Help), activate"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Toggle selected main menu checkbox"),
        "{rendered}"
    );
    assert!(rendered.contains("Click heading"), "{rendered}");
    assert!(!rendered.contains("Show MEM / GPU"), "{rendered}");
    assert!(rendered.contains("Pause / Resume"), "{rendered}");
    assert!(
        rendered.contains("Open an Investigation Profile"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Save the active profile; open Save As"),
        "{rendered}"
    );
    assert!(rendered.contains("Copy the focused row"), "{rendered}");
    assert!(!rendered.contains("Open Settings"), "{rendered}");

    assert!(rendered.contains("Select row range"), "{rendered}");
    assert!(
        rendered.contains("Select/deselect the focused live process"),
        "{rendered}"
    );
    assert!(rendered.contains("Ctrl+A (Tracked-only)"), "{rendered}");
    assert!(
        rendered.contains("Select all listed process rows"),
        "{rendered}"
    );
    assert!(
        rendered.contains("The stronger cell highlight shows keyboard focus."),
        "{rendered}"
    );
    assert!(
        rendered.contains("Kill selected processes, or the focused process if none are selected"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Open Files for the focused process"),
        "{rendered}"
    );
    assert!(rendered.contains("Switch Info tabs"), "{rendered}");
    assert!(rendered.contains("Refresh Info tab"), "{rendered}");

    assert!(rendered.contains("Click panel"), "{rendered}");
    assert!(
        rendered.contains("Context actions for process, Graph, Samples, or endpoint"),
        "{rendered}"
    );
    assert!(rendered.contains("PageUp/PageDown"), "{rendered}");
    assert!(rendered.contains("Change time span"), "{rendered}");
    assert!(
        rendered.contains("Increase / decrease Processes height"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Reset Processes height to Auto"),
        "{rendered}"
    );
    assert!(rendered.contains("Resize Processes height"), "{rendered}");

    assert!(!rendered.contains("Details panel"), "{rendered}");
    assert!(!rendered.contains("Dialogs"), "{rendered}");
    assert!(!rendered.contains("Recording path"), "{rendered}");
    assert!(!rendered.contains("Sampling interval"), "{rendered}");
    assert!(
        !rendered.contains("Esc / Enter closes this help dialog."),
        "{rendered}"
    );
    assert!(!rendered.contains("F6"), "{rendered}");
    assert!(!rendered.contains("[ Close ]"), "{rendered}");
    assert!(
        rendered.contains("F1/?") && rendered.contains("Help over any dialog"),
        "{rendered}"
    );
    assert!(rendered.contains("F12"), "{rendered}");
    assert!(
        rendered.contains("Cycle theme; Settings selects theme and contrast directly"),
        "{rendered}"
    );
    assert!(rendered.contains("Esc/Enter/F1/? Close"), "{rendered}");
    assert!(rendered.contains("Footer: fits width."), "{rendered}");
    assert!(
        rendered.contains("Scheme colors mark active items; T marks tracked."),
        "{rendered}"
    );
    assert!(!rendered_lower.contains("baseline"), "{rendered}");
}

#[test]
fn help_dialog_header_and_shortcuts_use_footer_like_styles() {
    let mut app = make_test_app(3, 10);
    app.show_help = true;

    let buffer = render_app_to_buffer(&app, 100, 45);
    let theme = ui::THEMES[0];

    let title = format!(
        "winproc-tui {} · Keyboard shortcuts",
        env!("CARGO_PKG_VERSION")
    );
    let (title_x, title_y) =
        find_text_position(&buffer, &title).expect("help dialog title should be rendered");
    assert_eq!(title_x, help_area(Rect::new(0, 0, 100, 45)).x + 2);
    let title_cell = &buffer[(title_x, title_y)];
    assert_eq!(title_cell.fg, theme.text);
    assert_ne!(title_cell.fg, theme.accent);
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));

    let (group_x, group_y) =
        find_text_position(&buffer, "Global").expect("group title should be rendered");
    let group_cell = &buffer[(group_x, group_y)];
    assert_eq!(group_cell.symbol(), "G");
    assert_eq!(group_cell.fg, theme.accent);
    assert!(group_cell.modifier.contains(ratatui::style::Modifier::BOLD));
    assert!(
        !group_cell
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );

    let (key_x, key_y) =
        find_text_position(&buffer, "Ctrl+F").expect("shortcut key should be rendered");
    let key_cell = &buffer[(key_x, key_y)];
    assert_eq!(key_cell.fg, theme.key_hint);
    assert_eq!(key_cell.bg, theme.panel_alt);
    assert!(!key_cell.modifier.contains(ratatui::style::Modifier::BOLD));

    let (label_x, label_y) = find_text_position(&buffer, "Edit filter").unwrap();
    let label_cell = &buffer[(label_x, label_y)];
    assert_eq!(label_cell.fg, theme.text);
}

#[test]
fn help_dialog_panel_fits_rendered_content() {
    let screen = Rect::new(0, 0, 120, 50);
    let popup = help_area(screen);

    assert!(popup.width <= screen.width);
    assert!(popup.height <= screen.height);
    assert!(popup.width >= 50, "popup too narrow: {popup:?}");
    assert!(popup.height >= 25, "popup too short: {popup:?}");
}

#[test]
fn help_dialog_scrolls_when_content_overflows() {
    let mut app = make_test_app(3, 10);
    app.show_help = true;
    let screen = Rect::new(0, 0, 100, 20);

    let top_rendered = render_app_to_text(&app, screen.width, screen.height);
    let top_buffer = render_app_to_buffer(&app, screen.width, screen.height);

    assert!(
        top_rendered.contains("Keyboard shortcuts"),
        "{top_rendered}"
    );
    assert!(top_rendered.contains("Global"), "{top_rendered}");
    assert!(
        find_symbol_position(&top_buffer, "█").is_some(),
        "{top_rendered}"
    );

    app.set_help_page_size(ui::help_page_size_for_screen(screen));
    app.scroll_help_end();
    let bottom_rendered = render_app_to_text(&app, screen.width, screen.height);

    assert!(
        bottom_rendered.contains("Esc/Enter/F1/? Close"),
        "{bottom_rendered}"
    );
    assert!(
        !bottom_rendered.contains("Esc / Enter closes this help dialog."),
        "{bottom_rendered}"
    );
}

#[test]
fn help_dialog_keyboard_scroll_updates_offset() {
    let mut app = make_test_app(3, 10);
    app.show_help = true;
    app.set_help_page_size(ui::help_page_size_for_screen(Rect::new(0, 0, 100, 20)));

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.help_scroll.offset, 1);

    app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        .unwrap();
    assert!(app.help_scroll.offset > 1);

    app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.help_scroll.offset, 0);
}

#[test]
fn help_dialog_scrollbar_drag_scrolls_content() {
    let mut app = make_test_app(3, 10);
    app.show_help = true;
    let screen = Rect::new(0, 0, 100, 20);
    app.set_help_page_size(ui::help_page_size_for_screen(screen));
    let scrollbar = help_scrollbar_area(screen, app.help_scroll.page_size)
        .expect("small help dialog should have a scrollbar");

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.y,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.help_scroll.dragging);
    assert_eq!(app.help_scroll.offset, 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.bottom().saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.help_scroll.offset > 0);

    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.bottom().saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(!app.help_scroll.dragging);
}

#[test]
fn footer_shows_process_context_on_one_row() {
    let mut app = make_test_app(3, 10);
    app.status = "Copied row: proc-0".to_string();

    let rendered = render_app_to_text(&app, 500, 30);

    assert!(rendered.contains("PROCESSES"), "{rendered}");
    assert!(rendered.contains("Ctrl+P Pause"), "{rendered}");
    assert!(rendered.contains("Ctrl+S Save Profile"), "{rendered}");
    assert!(rendered.contains("Ctrl+T Profiles"), "{rendered}");
    assert!(rendered.contains("c Columns"), "{rendered}");
    assert!(rendered.contains("s Sort"), "{rendered}");
    assert!(rendered.contains("v Flat/Tree"), "{rendered}");
    assert!(!rendered.contains("e Expand/Collapse"), "{rendered}");
    assert!(rendered.contains("g Graphs"), "{rendered}");
    assert!(rendered.contains("Ctrl+I Jump"), "{rendered}");
    assert!(!rendered.contains("Shift+←/→ Move column"), "{rendered}");
    assert!(rendered.contains("Space Graph"), "{rendered}");
    assert!(rendered.contains("Enter Process Info"), "{rendered}");
    assert!(rendered.contains("t Track process name"), "{rendered}");
    assert!(rendered.contains("Shift+T Tracked-only"), "{rendered}");
    assert!(rendered.contains("d Kill"), "{rendered}");
    assert!(rendered.contains("Ctrl+F Filter"), "{rendered}");
    assert!(rendered.contains("ESC Menu"), "{rendered}");
    assert!(rendered.contains("Tab Focus"), "{rendered}");
    assert!(rendered.contains("F12 Theme"), "{rendered}");
    assert!(rendered.contains("F1/? Help"), "{rendered}");
    assert!(!rendered.contains("Status  "), "{rendered}");
    assert!(rendered.contains("Copied row: proc-0"), "{rendered}");
    assert!(!rendered.contains("Up/Down Row"), "{rendered}");
    assert!(!rendered.contains("Left/Right Column"), "{rendered}");
    assert!(!rendered.contains("Ctrl+R Record"), "{rendered}");
    assert!(!rendered.contains("Ctrl+O"), "{rendered}");

    app.on_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE))
        .unwrap();
    let tree = render_app_to_text(&app, 500, 30);
    assert!(tree.contains("v Flat/Tree"), "{tree}");
    assert!(tree.contains("e Expand/Collapse"), "{tree}");

    app.filter_text = "proc".to_string();
    app.rebuild_visible_process_cache();
    let filtered_tree = render_app_to_text(&app, 500, 30);
    assert!(filtered_tree.contains("v Flat/Tree"), "{filtered_tree}");
    assert!(
        !filtered_tree.contains("e Expand/Collapse"),
        "{filtered_tree}"
    );
}

#[test]
fn footer_pause_label_tracks_display_pause_state() {
    let mut app = make_test_app(3, 10);
    for paused in [false, true, false] {
        if app.is_display_paused() != paused {
            app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
                .unwrap();
        }
        let rendered = render_app_to_text(&app, 420, 45);
        let footer = rendered.lines().last().unwrap();
        assert!(footer.contains(if paused {
            "Ctrl+P Resume"
        } else {
            "Ctrl+P Pause"
        }));
        assert!(!footer.contains(if paused {
            "Ctrl+P Pause"
        } else {
            "Ctrl+P Resume"
        }));
    }
}

#[test]
fn footer_keeps_menu_help_and_focus_visible_at_narrow_width() {
    let app = make_test_app(3, 10);
    let buffer = render_app_to_buffer(&app, 30, 24);
    let footer = Rect::new(0, 23, 30, 1);
    let (menu_x, menu_y) = find_text_position_in_area(&buffer, footer, "ESC Menu")
        .expect("menu shortcut should remain visible");

    assert_eq!(menu_x, 0);
    assert_eq!(menu_y, footer.y);
    assert!(find_text_position_in_area(&buffer, footer, "F1/? Help").is_some());
    assert!(find_text_position_in_area(&buffer, footer, "Tab Focus").is_some());
    assert!(find_text_position_in_area(&buffer, footer, "PROCESSES").is_none());
    assert!(find_text_position(&buffer, "Ctrl+O").is_none());
}

#[test]
fn process_footer_labels_space_for_the_selected_cell_action() {
    let mut app = make_test_app(3, 10);
    app.selected_process_column_index = 0;

    let identity_column = render_app_to_text(&app, 170, 30);
    assert!(
        identity_column.contains("Space Track process name"),
        "{identity_column}"
    );
    assert!(
        !identity_column.contains("Space Graph"),
        "{identity_column}"
    );

    app.selected_process_column_index = 2;
    let metric_column = render_app_to_text(&app, 170, 30);
    assert!(metric_column.contains("Space Graph"), "{metric_column}");
    assert!(
        !metric_column.contains("Space Track process name"),
        "{metric_column}"
    );
}

#[test]
fn footer_shortcuts_follow_the_focused_panel() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::System;
    let system = render_app_to_text(&app, 170, 30);
    assert!(system.contains("i System info"), "{system}");
    assert!(system.contains("g Graphs"), "{system}");
    assert!(!system.contains("m/g MEM/GPU"), "{system}");
    assert!(!system.contains("Up/Down Metric"), "{system}");
    assert!(!system.contains("Left/Right Column"), "{system}");

    assign_private_graph(&mut app);
    app.focused_panel = FocusedPanel::DetailsGraph;
    let graph = render_app_to_text(&app, 300, 45);
    assert!(graph.contains("↑/↓ Slot"), "{graph}");
    assert!(graph.contains("←/→ Sample"), "{graph}");
    assert!(!graph.contains("Prev Slot"), "{graph}");
    assert!(!graph.contains("Next Slot"), "{graph}");
    assert!(graph.contains("Del Remove"), "{graph}");
    assert!(graph.contains("m Raw/MA5"), "{graph}");
    assert!(graph.contains("Enter Info"), "{graph}");
    assert!(graph.contains("Alt+←/→ Pan"), "{graph}");
    assert!(graph.contains("PgUp/PgDn Span"), "{graph}");
    assert!(graph.contains("f/z Fit/Min 0"), "{graph}");
    assert!(graph.contains("a/b A/B range"), "{graph}");
    assert!(graph.contains("Shift+A/B Jump A/B"), "{graph}");

    app.focused_panel = FocusedPanel::DetailsSamples;
    let samples = render_app_to_text(&app, 300, 45);
    assert!(samples.contains("↑/← Older"), "{samples}");
    assert!(samples.contains("↓/→ Newer"), "{samples}");
    assert!(samples.contains("Del Remove"), "{samples}");
    assert!(samples.contains("m Raw/MA5"), "{samples}");
    assert!(samples.contains("PgUp/PgDn Scroll"), "{samples}");
    assert!(samples.contains("Home/End Edge"), "{samples}");
    assert!(samples.contains("f/z Fit/Min 0"), "{samples}");
    assert!(samples.contains("Shift+A/B Jump A/B"), "{samples}");
    assert!(samples.contains("a/b A/B range"), "{samples}");
    assert!(samples.contains("x Clear A/B"), "{samples}");
}

#[test]
fn footer_shows_process_height_shortcuts_only_for_visible_workspace_focus() {
    let mut app = make_test_app(3, 10);
    let hidden = render_app_to_text(&app, 360, 45);
    assert!(!hidden.contains("h/H/Alt+H Height"), "{hidden}");

    assign_private_graph(&mut app);
    for focused_panel in [
        FocusedPanel::Processes,
        FocusedPanel::DetailsGraph,
        FocusedPanel::DetailsSamples,
    ] {
        app.focused_panel = focused_panel;
        let rendered = render_app_to_text(&app, 360, 45);
        assert!(
            rendered.contains("h/H/Alt+H Height"),
            "{focused_panel:?}: {rendered}"
        );
    }

    app.focused_panel = FocusedPanel::System;
    let system = render_app_to_text(&app, 360, 45);
    assert!(!system.contains("h/H/Alt+H Height"), "{system}");
}

#[test]
fn footer_shows_pause_and_focus_for_every_focused_panel() {
    let mut app = make_test_app(3, 10);

    for focused_panel in [
        FocusedPanel::System,
        FocusedPanel::SystemActivity,
        FocusedPanel::Cpu,
        FocusedPanel::Processes,
        FocusedPanel::DetailsGraph,
        FocusedPanel::DetailsSamples,
    ] {
        app.focused_panel = focused_panel;
        let rendered = render_app_to_text(&app, 420, 45);
        assert!(
            rendered.contains("Ctrl+P Pause"),
            "{focused_panel:?}: {rendered}"
        );
        assert!(
            rendered.contains("Tab Focus"),
            "{focused_panel:?}: {rendered}"
        );
    }
}

#[test]
fn footer_fits_whole_shortcuts_and_preserves_essential_actions() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    for panel in [
        FocusedPanel::Processes,
        FocusedPanel::DetailsGraph,
        FocusedPanel::DetailsSamples,
    ] {
        app.focused_panel = panel;
        for paused in [false, true] {
            if app.is_display_paused() != paused {
                app.toggle_display_pause();
            }
            let wide = render_app_to_text(&app, 500, 48);
            let all_groups: Vec<_> = wide.lines().last().unwrap().split("  ").collect();
            for width in [1, 8, 20, 30, 40, 60, 80, 120, 160, 260] {
                let buffer = render_app_to_buffer(&app, width, 48);
                let footer = (0..width)
                    .map(|x| buffer[(x, 47)].symbol())
                    .collect::<String>();
                let footer = footer.trim_end();
                for group in footer.split("  ").filter(|group| !group.is_empty()) {
                    assert!(
                        all_groups.contains(&group),
                        "partial shortcut at {width}: {footer}"
                    );
                }
                if width >= 160 {
                    for required in [
                        "ESC Menu",
                        "F1/? Help",
                        "Tab Focus",
                        if paused {
                            "Ctrl+P Resume"
                        } else {
                            "Ctrl+P Pause"
                        },
                    ] {
                        assert!(footer.contains(required), "{panel:?}, {width}: {footer}");
                    }
                }
            }
        }
    }
    app.toggle_display_pause();
    app.log_view_path = Some(std::path::PathBuf::from("sample.log"));
    let rendered = render_app_to_text(&app, 80, 48);
    let footer = rendered.lines().last().unwrap();
    assert!(footer.contains("ESC Menu") && footer.contains("F1/? Help"));
    assert!(!footer.contains("Ctrl+P"));
}

#[test]
fn help_dialog_takes_focus_border_from_previous_panel() {
    let mut app = make_test_app(3, 10);
    app.focused_panel = FocusedPanel::Processes;
    app.show_help = true;

    let screen = Rect::new(0, 0, 240, 70);
    let popup = help_area(screen);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    let process_table = main_panel_areas_for_app(screen, &app).processes.area;
    // Inspect a background corner that is not covered by the Help dialog.
    assert!(!popup.contains((process_table.x, process_table.y).into()));
    assert_eq!(buffer[(popup.x, popup.y)].fg, app.theme().focus_border);
    assert_eq!(buffer[(process_table.x, process_table.y)].symbol(), "╭");
    assert_ne!(
        buffer[(process_table.x, process_table.y)].fg,
        app.theme().border
    );
}

#[test]
fn action_feedback_is_visible_without_adding_footer_height() {
    let mut app = make_test_app(1, 10);
    for message in [
        "Copied environment variable",
        "Return to Live before changing the Tracking List",
        "Recording stopped",
    ] {
        app.status = message.to_string();
        let text = render_app_to_text(&app, 120, 60);
        assert!(text.lines().nth(58).unwrap().contains(message), "{text}");
        assert!(text.lines().last().unwrap().contains("F1/? Help"));
    }
}

#[test]
fn help_overlays_and_preserves_an_active_dialog() {
    let mut app = make_test_app(3, 10);
    app.show_log_dir_dialog = true;
    let before = app.log_dir_draft.clone();
    app.on_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE))
        .unwrap();
    assert!(app.show_help && app.show_log_dir_dialog);
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.show_help && app.show_log_dir_dialog);
    assert_eq!(app.log_dir_draft, before);
}

#[test]
fn investigation_actions_fit_at_120_columns() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    for (panel, expected) in [
        (
            FocusedPanel::Processes,
            vec!["Space Graph", "Ctrl+F Filter", "Enter Process Info"],
        ),
        (
            FocusedPanel::DetailsGraph,
            vec!["←/→ Sample", "a/b A/B range", "Del Remove"],
        ),
        (
            FocusedPanel::DetailsSamples,
            vec!["a/b A/B range", "Del Remove"],
        ),
    ] {
        app.focused_panel = panel;
        let text = render_app_to_text(&app, 120, 60);
        let footer = text.lines().last().unwrap();
        for hint in expected {
            assert!(footer.contains(hint), "{footer}");
        }
    }
    app.log_view_path = Some("example.log".into());
    app.focused_panel = FocusedPanel::Processes;
    let text = render_app_to_text(&app, 120, 60);
    assert!(text.lines().last().unwrap().contains("Ctrl+B Live"));
    app.on_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(app.log_view_path.is_none());
}

#[test]
fn graph_history_state_and_end_hint_follow_manual_selection() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    app.select_details_sample_oldest();
    let rendered = render_app_to_text(&app, 120, 60);
    assert!(rendered.contains("History · End Latest"));
    app.select_details_sample_latest();
    assert!(render_app_to_text(&app, 120, 60).contains("Follow latest"));
    app.toggle_display_pause();
    assert!(render_app_to_text(&app, 120, 60).contains("Paused · Latest"));
}

#[test]
fn compact_resource_panels_stay_separate_and_fill_the_row() {
    use crate::app::ResourcePanel;
    let mut app = make_test_app(2, 10);
    for width in [80, 100, 120, 140, 180, 300, 120] {
        let screen = Rect::new(0, 0, width, 60);
        app.set_screen_area(screen);
        let areas = [
            ui::ram_vram_panel_area_for_screen(screen, &app),
            ui::gpu_panel_area_for_screen(screen, &app),
            ui::system_activity_panel_area_for_screen(screen, &app),
            ui::cpu_panel_area_for_screen(screen, &app),
        ];
        assert_eq!(areas[0].x, 0);
        assert_eq!(areas[3].right(), width);
        for pair in areas.windows(2) {
            assert_eq!(pair[0].right(), pair[1].x);
            assert!(pair[0].width > 2);
        }
        let rendered = render_app_to_text(&app, width, 60);
        for label in [" MEM ", " GPU ", " NW/DISK ", " CPU "] {
            assert!(rendered.contains(label), "width={width}: {rendered}");
        }
        assert!(!rendered.contains("[MEM]"));
        assert_eq!(rendered.contains("Paged Pool"), width >= 140);
        for (area, resource) in [
            (areas[0], ResourcePanel::Memory),
            (areas[1], ResourcePanel::Gpu),
        ] {
            app.on_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: area.x + 2,
                    row: area.y,
                    modifiers: KeyModifiers::NONE,
                },
                screen,
            );
            assert_eq!(app.resource_panel, resource);
            assert_eq!(app.focused_panel, FocusedPanel::System);
        }
    }
}

#[test]
fn footer_advertises_ctrl_a_only_for_tracked_only_processes() {
    for (tracked_only, focused_panel, expected) in [
        (true, FocusedPanel::Processes, true),
        (false, FocusedPanel::Processes, false),
        (true, FocusedPanel::System, false),
    ] {
        let mut app = make_test_app(2, 10);
        app.watch_enabled = tracked_only;
        app.focused_panel = focused_panel;
        let buffer = render_app_to_buffer(&app, 120, 60);
        let footer = Rect::new(0, 59, 120, 1);
        assert_eq!(
            find_text_position_in_area(&buffer, footer, "Ctrl+A Select all").is_some(),
            expected
        );
    }
}

fn click_visible_shortcut(app: &mut crate::App, text: &str) {
    let screen = Rect::new(0, 0, 180, 60);
    let buffer = render_app_to_buffer(app, 180, 60);
    let (x, y) = find_text_position(&buffer, text).unwrap_or_else(|| panic!("missing {text}"));
    assert!(
        app.shortcut_map.borrow().regions.iter().any(|region| region
            .area
            .contains(ratatui::layout::Position::new(x + text.len() as u16 - 1, y))),
        "{text} at {x},{y}: {:?}",
        app.shortcut_map.borrow().regions
    );
    // Click the action label at the end, not just the key.
    app.on_mouse(
        super::support::left_click(x + text.len() as u16 - 1, y),
        screen,
    );
}

#[test]
fn visible_footer_actions_complete_mouse_dialog_workflows() {
    let mut app = make_test_app(3, 10);
    click_visible_shortcut(&mut app, "F1/? Help");
    assert!(app.show_help);
    click_visible_shortcut(&mut app, "Esc/Enter/F1/? Close");
    assert!(!app.show_help);
    let screen = Rect::new(0, 0, 180, 60);
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (column, row) = find_text_position(&buffer, "PID").unwrap();
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert!(app.show_column_picker);
    click_visible_shortcut(&mut app, "Enter/Esc close");
    assert!(!app.show_column_picker);
    click_visible_shortcut(&mut app, "Enter Process Info");
    assert!(app.show_process_info_dialog);
    click_visible_shortcut(&mut app, "Esc close");
    assert!(!app.show_process_info_dialog);
    click_visible_shortcut(&mut app, "f Files");
    assert!(app.show_process_info_dialog);
    assert_eq!(app.process_info_tab, crate::app::ProcessInfoTab::Files);
    click_visible_shortcut(&mut app, "Esc close");
    assert!(!app.show_process_info_dialog);
    app.request_quit_confirmation();
    render_app_to_buffer(&app, 180, 60);
    app.on_mouse(
        super::support::left_click(179, 59),
        Rect::new(0, 0, 180, 60),
    );
    assert!(app.show_quit_confirmation);
    assert!(!app.should_quit);
    click_visible_shortcut(&mut app, "Esc Cancel");
    assert!(!app.show_quit_confirmation);
    assert!(!app.should_quit);
}

#[test]
fn menus_toggle_and_shortcut_hover_redraws_without_click_through() {
    use ratatui::style::Modifier;
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 180, 60);
    app.on_mouse(super::support::left_click(2, 0), screen);
    assert!(app.is_main_menu_open());
    app.on_mouse(super::support::left_click(2, 0), screen);
    assert!(!app.is_main_menu_open());
    app.open_main_menu();
    app.on_mouse(super::support::left_click(179, 30), screen);
    assert!(!app.is_main_menu_open());
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (x, y) = find_text_position(&buffer, "F1/? Help").unwrap();
    assert!(
        crate::app::handle_mouse_event(&mut app, super::support::mouse_move(x + 7, y), screen)
            .dirty
    );
    let buffer = render_app_to_buffer(&app, 180, 60);
    assert_eq!(buffer[(x + 7, y)].bg, app.theme().focus_surface);
    assert!(buffer[(x + 7, y)].modifier.contains(Modifier::BOLD));
    assert!(
        crate::app::handle_mouse_event(&mut app, super::support::mouse_move(179, 30), screen).dirty
    );
    app.open_help();
    render_app_to_buffer(&app, 180, 60);
    app.on_mouse(super::support::left_click(x + 7, y), screen);
    assert!(app.show_help);
    assert!(!app.is_main_menu_open());
}

#[test]
fn confirmation_footer_clicks_preserve_recording_stop_semantics() {
    let mut app = make_test_app(1, 10);
    app.show_recording_stop_confirmation = true;
    render_app_to_buffer(&app, 180, 60);
    let keys = app
        .shortcut_map
        .borrow()
        .regions
        .iter()
        .map(|r| r.key.code)
        .collect::<Vec<_>>();
    assert_eq!(keys, vec![KeyCode::Enter, KeyCode::Char('y')]);
    click_visible_shortcut(&mut app, "Enter/Esc/n Continue");
    assert!(!app.show_recording_stop_confirmation);
    app.show_recording_path_dialog = true;
    app.show_recording_overwrite_confirmation = true;
    render_app_to_buffer(&app, 180, 60);
    assert_eq!(
        app.shortcut_map
            .borrow()
            .regions
            .iter()
            .map(|r| r.key.code)
            .collect::<Vec<_>>(),
        keys
    );
    click_visible_shortcut(&mut app, "Enter/Esc/n Cancel");
    assert!(!app.show_recording_overwrite_confirmation);
}

#[test]
fn action_feedback_expires_without_sampling_and_discards_previous_context() {
    use std::time::{Duration, Instant};
    let mut app = make_test_app(3, 10);
    app.toggle_display_pause();
    let now = Instant::now();
    app.status = "Tracking added: proc-0".into();
    assert!(app.refresh_status_feedback(now));
    assert!(render_app_to_text(&app, 180, 60).contains("Tracking added: proc-0"));
    assert!(!app.refresh_status_feedback(now + Duration::from_secs(5)));
    assert!(app.refresh_status_feedback(now + Duration::from_secs(6)));
    assert!(app.status.is_empty());
    assert!(app.is_display_paused());

    app.status = "Copied row: proc-0".into();
    app.refresh_status_feedback(now);
    app.show_help = true;
    app.refresh_status_feedback(now);
    assert!(!render_app_to_text(&app, 180, 60).contains("Copied row: proc-0"));
    app.status = "Action unavailable".into();
    app.refresh_status_feedback(now);
    assert_eq!(app.status, "Action unavailable");
    app.show_help = false;
    app.refresh_status_feedback(now);
    assert!(app.status.is_empty());

    app.status = "Old process action".into();
    app.refresh_status_feedback(now);
    app.network_browser.visible = true;
    app.refresh_status_feedback(now);
    assert!(app.status.is_empty());
    app.status = "Focus: Processes".into();
    app.refresh_status_feedback(now);
    app.refresh_status_feedback(now + Duration::from_secs(2));
    assert!(app.status.is_empty());
}

#[test]
fn transient_feedback_does_not_dismiss_recording_errors_or_repeat_quit_instructions() {
    use std::time::{Duration, Instant};
    let mut app = make_test_app(3, 10);
    app.request_quit_confirmation();
    let text = render_app_to_text(&app, 180, 60);
    assert!(app.status.is_empty());
    assert!(!text.lines().nth(58).unwrap().contains("quit"));
    app.show_quit_confirmation = false;
    app.present_active_recording_error("partial.jsonl".into(), anyhow::anyhow!("write failed"));
    let before = app.recording_error.clone();
    let now = Instant::now();
    app.refresh_status_feedback(now);
    app.refresh_status_feedback(now + Duration::from_secs(60));
    assert_eq!(app.recording_error, before);
    assert!(render_app_to_text(&app, 180, 60).contains("write failed"));
}

#[test]
fn latest_feedback_uses_paused_or_recorded_source_and_never_negative_zero() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    for (paused, recorded, expected) in [
        (false, false, "Follow latest"),
        (true, false, "Paused · Latest"),
        (false, true, "Recorded · Latest"),
    ] {
        if app.is_display_paused() != paused {
            app.toggle_display_pause();
        }
        app.log_view_path = recorded.then(|| "example.log".into());
        app.select_details_sample_oldest();
        app.enter_details_live_mode();
        assert_eq!(app.status, format!("Samples: {expected}"));
        assert_eq!(app.is_display_paused(), paused);
        app.shift_graph_time_window(false);
        assert_eq!(app.status, format!("Graph: {expected}"));
        app.set_graph_time_window_offset(0);
        assert_eq!(app.status, format!("Graph: {expected}"));
        let text = render_app_to_text(&app, 180, 60);
        assert!(!text.contains("-0s"));
        assert!(!text.contains("live mode enabled"));
    }
    app.status = format!("Tracking added: {}", "long-name-".repeat(50));
    let buffer = render_app_to_buffer(&app, 180, 60);
    let text = render_app_to_text(&app, 180, 60);
    assert_eq!(buffer.area, Rect::new(0, 0, 180, 60));
    assert!(text.lines().nth(58).unwrap().starts_with("Tracking added:"));
    assert!(text.lines().last().unwrap().contains("F1/? Help"));
}

#[test]
fn help_enters_the_current_panel_section_at_180_by_60() {
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    let screen = Rect::new(0, 0, 180, 60);
    crate::app::sync_layout_state(&mut app, screen);
    for (panel, section, control) in [
        (
            FocusedPanel::Processes,
            "Processes",
            "Toggle Flat / Tree view",
        ),
        (
            FocusedPanel::DetailsGraph,
            "Graph Workspace",
            "Remove active Graph",
        ),
        (
            FocusedPanel::DetailsSamples,
            "Samples",
            "Select older sample",
        ),
        (
            FocusedPanel::System,
            "MEM/GPU",
            "Select the previous/next metric",
        ),
        (
            FocusedPanel::SystemActivity,
            "NW/DISK",
            "Select the previous/next metric",
        ),
        (FocusedPanel::Cpu, "CPU", "Per-core"),
    ] {
        app.focused_panel = panel;
        let selected = app.process_table_state.selected();
        let active_graph = app.active_graph_id;
        app.on_key(KeyCode::F(1).into()).unwrap();
        assert_eq!(ui::help::section_titles()[app.help_section], section);
        let text = render_app_to_text(&app, 180, 60);
        assert!(text.contains(section), "{section}: {text}");
        assert!(text.contains(control), "{section}: {text}");
        assert!(text.contains("s Sections"));
        app.on_key(KeyCode::Esc.into()).unwrap();
        assert_eq!(app.focused_panel, panel);
        assert_eq!(app.process_table_state.selected(), selected);
        assert_eq!(app.active_graph_id, active_graph);
    }
}

#[test]
fn help_sections_share_wrapped_positions_and_picker_cancel_preserves_scroll() {
    let mut app = make_test_app(3, 10);
    for (width, height) in [(100, 25), (180, 60), (240, 60)] {
        let screen = Rect::new(0, 0, width, height);
        crate::app::sync_layout_state(&mut app, screen);
        app.open_help();
        for (index, title) in ui::help::section_titles().iter().enumerate() {
            app.jump_help_section(index);
            let text = render_app_to_text(&app, width, height);
            assert!(text.contains(title), "{width}x{height}, {title}: {text}");
        }
        app.jump_help_section(ui::help::section_index("Graph Workspace"));
        app.scroll_help_down(1);
        let before = (app.help_section, app.help_scroll.offset);
        app.on_key(KeyCode::Char('s').into()).unwrap();
        app.on_key(KeyCode::Home.into()).unwrap();
        app.on_key(KeyCode::Esc.into()).unwrap();
        assert_eq!((app.help_section, app.help_scroll.offset), before);
        app.on_key(KeyCode::Char('s').into()).unwrap();
        app.on_key(KeyCode::Home.into()).unwrap();
        app.on_key(KeyCode::Down.into()).unwrap();
        app.on_key(KeyCode::Enter.into()).unwrap();
        assert_eq!(ui::help::section_titles()[app.help_section], "Processes");
        assert!(app.show_help);
        assert!(app.help_section_picker.is_none());
        app.on_key(KeyCode::Right.into()).unwrap();
        assert_eq!(ui::help::section_titles()[app.help_section], "Process Info");
        app.on_key(KeyCode::Left.into()).unwrap();
        assert_eq!(ui::help::section_titles()[app.help_section], "Processes");
        app.close_help();
    }
}

#[test]
fn help_section_picker_uses_clickable_footer_and_hover_does_not_navigate() {
    use super::support::{left_click, mouse_move};
    let mut app = make_test_app(3, 10);
    let screen = Rect::new(0, 0, 180, 60);
    crate::app::sync_layout_state(&mut app, screen);
    app.open_help();
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (x, y) = find_text_position(&buffer, "s Sections").unwrap();
    app.on_mouse(left_click(x + 3, y), screen);
    assert!(app.help_section_picker.is_some());
    let before = (
        app.help_section,
        app.help_section_picker,
        app.help_scroll.offset,
    );
    let buffer = render_app_to_buffer(&app, 180, 60);
    let (x, y) = find_text_position(&buffer, "Network endpoints").unwrap();
    let outcome = crate::app::handle_mouse_event(&mut app, mouse_move(x, y), screen);
    assert!(outcome.dirty);
    assert_eq!(
        (
            app.help_section,
            app.help_section_picker,
            app.help_scroll.offset
        ),
        before
    );
    let buffer = render_app_to_buffer(&app, 180, 60);
    assert_eq!(buffer[(x, y)].bg, app.theme().focus_surface);
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    app.on_mouse(left_click(x, y), screen);
    assert!(app.help_section_picker.is_none());
    assert!(app.shortcut_hovered.is_none());
    assert_eq!(
        ui::help::section_titles()[app.help_section],
        "Network endpoints"
    );
    assert!(render_app_to_text(&app, 180, 60).contains("Endpoints are never recorded"));

    // All sections remain selectable when only a few rows fit.
    let small = Rect::new(0, 0, 80, 12);
    crate::app::sync_layout_state(&mut app, small);
    app.on_key(KeyCode::Char('s').into()).unwrap();
    app.on_key(KeyCode::End.into()).unwrap();
    let buffer = render_app_to_buffer(&app, 80, 12);
    let (x, y) = find_text_position(&buffer, "A/B comparison").unwrap();
    app.on_mouse(left_click(x, y), small);
    assert_eq!(
        ui::help::section_titles()[app.help_section],
        "A/B comparison"
    );
}

#[test]
fn help_preserves_process_info_target_focus_and_literal_query() {
    use crate::app::{ProcessInfoFocus, ProcessInfoTab};
    let mut app = make_test_app(3, 10);
    assign_private_graph(&mut app);
    crate::app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 60));
    app.open_selected_process_info_dialog().unwrap();
    app.process_info_tab = ProcessInfoTab::Environment;
    app.process_info_focus = ProcessInfoFocus::Content;
    app.process_environment_filter = "日本語".into();
    app.process_environment_filter_cursor = app.process_environment_filter.len();
    app.on_key(KeyCode::Char('?').into()).unwrap();
    assert!(!app.show_help);
    assert_eq!(app.process_environment_filter, "日本語?");
    let target = app.process_info_target.as_ref().unwrap().identity.clone();
    let generation = app.process_info_generation;
    let focus = app.focused_panel;
    let cursor = app.process_environment_filter_cursor;
    app.on_key(KeyCode::F(1).into()).unwrap();
    assert_eq!(ui::help::section_titles()[app.help_section], "Process Info");
    assert!(render_app_to_text(&app, 180, 60).contains("Switch Info tabs"));
    for code in [
        KeyCode::Char('x'),
        KeyCode::Char('s'),
        KeyCode::Down,
        KeyCode::Enter,
        KeyCode::PageDown,
        KeyCode::Esc,
    ] {
        app.on_key(code.into()).unwrap();
    }
    assert!(!app.show_help && app.show_process_info_dialog);
    assert_eq!(app.process_info_target.as_ref().unwrap().identity, target);
    assert_eq!(app.process_info_generation, generation);
    assert_eq!(app.focused_panel, focus);
    assert_eq!(app.process_info_tab, ProcessInfoTab::Environment);
    assert_eq!(app.process_info_focus, ProcessInfoFocus::Content);
    assert_eq!(app.process_environment_filter, "日本語?");
    assert_eq!(app.process_environment_filter_cursor, cursor);
}

#[test]
fn help_preserves_network_query_selection_and_workspace() {
    let mut app = make_test_app(3, 10);
    crate::app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 60));
    app.open_network_browser();
    app.network_browser.filter = "127".into();
    app.network_browser.cursor = 3;
    app.network_browser.editing = true;
    app.on_key(KeyCode::Char('?').into()).unwrap();
    assert!(!app.show_help);
    assert_eq!(app.network_browser.filter, "127?");
    let generation = app.network_browser.generation;
    let selected = app.network_browser.selected;
    app.on_key(KeyCode::F(1).into()).unwrap();
    assert_eq!(
        ui::help::section_titles()[app.help_section],
        "Network endpoints"
    );
    assert!(render_app_to_text(&app, 180, 60).contains("Network endpoints"));
    app.on_key(KeyCode::Char('s').into()).unwrap();
    app.on_key(KeyCode::End.into()).unwrap();
    app.on_key(KeyCode::Enter.into()).unwrap();
    app.on_key(KeyCode::F(1).into()).unwrap();
    assert!(app.network_browser.visible && app.network_browser.editing);
    assert_eq!(app.network_browser.filter, "127?");
    assert_eq!(app.network_browser.cursor, 4);
    assert_eq!(app.network_browser.generation, generation);
    assert_eq!(app.network_browser.selected, selected);
}

#[test]
fn resource_titles_use_readable_panel_colors_at_every_width() {
    use crate::app::ResourcePanel;
    for theme_index in 0..ui::THEMES.len() {
        for high_contrast in [false, true] {
            for focused in [false, true] {
                for active in [ResourcePanel::Memory, ResourcePanel::Gpu] {
                    let mut app = make_test_app(2, 10);
                    app.theme_index = theme_index;
                    app.high_contrast = high_contrast;
                    app.resource_panel = active;
                    app.focused_panel = if focused {
                        FocusedPanel::System
                    } else {
                        FocusedPanel::Processes
                    };
                    let theme = app.theme();
                    for width in [80, 120, 180] {
                        let buffer = render_app_to_buffer(&app, width, 60);
                        for (label, resource) in [
                            (" MEM ", ResourcePanel::Memory),
                            (" GPU ", ResourcePanel::Gpu),
                        ] {
                            let (x, y) = find_text_position(&buffer, label).unwrap();
                            let bg = if focused && active == resource {
                                theme.focus_border
                            } else {
                                theme.muted
                            };
                            for offset in 0..5 {
                                let cell = &buffer[(x + offset, y)];
                                assert_eq!((cell.fg, cell.bg), (theme.panel, bg));
                            }
                        }
                    }
                }
            }
        }
    }
}
