use std::{path::PathBuf, sync::mpsc::TryRecvError};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, style::Modifier};

use super::support::{
    find_text_position, left_click, make_test_app, make_test_app_with_worker, mouse_move,
    render_app_to_buffer, render_app_to_text, track_process_name, unique_recording_path,
};
use crate::{
    app::{
        App, AppActivity, ProcessViewMode, handle_mouse_event, profiles::InvestigationProfilesView,
    },
    samplers::{SampleRequest, SamplingWorker},
    ui::{self, main_menu_area, main_menu_item_area},
};

fn press(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
}

fn menu_labels(app: &App) -> Vec<String> {
    app.main_menu_rows()
        .iter()
        .map(|row| app.main_menu_row_label(*row))
        .collect()
}

fn start_recording(app: &mut App, label: &str) -> PathBuf {
    let path = unique_recording_path(label);
    let _ = std::fs::remove_file(&path);
    track_process_name(app, "proc-0");
    app.recording_path_draft = path.display().to_string();
    app.recording_path_cursor = app.recording_path_draft.len();
    app.show_recording_path_dialog = true;
    app.confirm_recording_path().unwrap();
    assert_eq!(app.activity(), AppActivity::Recording);
    path
}

#[test]
fn menu_quit_is_immediate_in_live_and_log_view_but_confirms_recording() {
    for log_view in [false, true] {
        let mut app = make_test_app(1, 10);
        if log_view {
            app.log_view_path = Some(PathBuf::from("C:/logs/example.log"));
        }
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Enter);

        assert!(app.should_quit);
        assert!(!app.show_quit_confirmation);
        assert!(!app.is_main_menu_open());
    }

    let mut direct_q = make_test_app(1, 10);
    press(&mut direct_q, KeyCode::Char('q'));
    assert!(!direct_q.should_quit);
    assert!(direct_q.show_quit_confirmation);

    let mut recording = make_test_app(1, 10);
    let path = start_recording(&mut recording, "main-menu-quit-confirmation");
    press(&mut recording, KeyCode::Esc);
    press(&mut recording, KeyCode::End);
    press(&mut recording, KeyCode::Enter);

    assert!(!recording.should_quit);
    assert!(recording.show_quit_confirmation);
    assert!(!recording.is_main_menu_open());
    let rendered = render_app_to_text(&recording, 100, 45);
    assert!(rendered.contains("Stop recording and quit?"), "{rendered}");

    press(&mut recording, KeyCode::Esc);
    press(&mut recording, KeyCode::Esc);
    press(&mut recording, KeyCode::Right);
    press(&mut recording, KeyCode::Char('q'));
    assert!(recording.show_quit_confirmation);
    assert!(!recording.should_quit);
    press(&mut recording, KeyCode::Esc);
    recording.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn menu_quit_mouse_event_exits_live_and_log_view_but_confirms_recording() {
    let screen = Rect::new(0, 0, 100, 45);
    for log_view in [false, true] {
        let mut app = make_test_app(1, 10);
        if log_view {
            app.log_view_path = Some(PathBuf::from("C:/logs/example.log"));
        }
        app.open_main_menu();
        let quit_index = menu_labels(&app)
            .iter()
            .position(|label| label == "Quit")
            .expect("Quit row");
        let quit = main_menu_item_area(screen, &app, quit_index).expect("Quit item area");

        let outcome = handle_mouse_event(&mut app, left_click(quit.x, quit.y), screen);

        assert!(outcome.should_quit);
        assert!(app.should_quit);
        assert!(!app.show_quit_confirmation);
        assert!(!app.is_main_menu_open());
    }

    let mut recording = make_test_app(1, 10);
    let path = start_recording(&mut recording, "main-menu-mouse-quit-confirmation");
    recording.open_main_menu();
    let quit_index = menu_labels(&recording)
        .iter()
        .position(|label| label == "Quit")
        .expect("Quit row");
    let quit = main_menu_item_area(screen, &recording, quit_index).expect("Quit item area");

    let outcome = handle_mouse_event(&mut recording, left_click(quit.x, quit.y), screen);

    assert!(!outcome.should_quit);
    assert!(!recording.should_quit);
    assert!(recording.show_quit_confirmation);
    assert!(!recording.is_main_menu_open());
    recording.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn existing_modal_and_editing_escape_handlers_do_not_open_menu() {
    let mut app = make_test_app(1, 10);
    app.show_help = true;
    press(&mut app, KeyCode::Esc);
    assert!(!app.show_help);
    assert!(!app.is_main_menu_open());

    app.request_quit_confirmation();
    press(&mut app, KeyCode::Esc);
    assert!(!app.show_quit_confirmation);
    assert!(!app.is_main_menu_open());

    app.begin_filter_edit();
    press(&mut app, KeyCode::Esc);
    assert!(!app.is_filter_editing());
    assert!(!app.is_main_menu_open());

    app.begin_process_jump_edit();
    press(&mut app, KeyCode::Esc);
    assert!(!app.is_process_jump_editing());
    assert!(!app.is_main_menu_open());
}

#[test]
fn menu_does_not_block_sampling_or_recording_frame_writes() {
    let (sampling_worker, request_rx, _result_tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, sampling_worker);
    app.open_main_menu();

    assert!(!app.request_sample().unwrap());
    assert_eq!(request_rx.try_recv(), Ok(SampleRequest::Sample));
    assert!(app.is_main_menu_open());

    app.sampling_in_progress = false;
    app.dismiss_main_menu();
    let path = start_recording(&mut app, "main-menu-recording-continues");
    app.open_main_menu();
    app.write_current_recording_frame().unwrap();
    assert_eq!(app.activity(), AppActivity::Recording);
    assert!(app.is_main_menu_open());
    app.dismiss_main_menu();
    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);

    assert_eq!(request_rx.try_recv(), Err(TryRecvError::Empty));
}

use crate::app::state::{MainMenuAction, MainMenuItem, MainMenuSection};
use crate::ui::header::HeaderAction;

fn alt(app: &mut App, ch: char) {
    app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT))
        .unwrap();
}
fn choose(app: &mut App, action: MainMenuAction) {
    app.main_menu_selected = app
        .main_menu_rows()
        .iter()
        .position(|row| row.item == MainMenuItem::Action(action))
        .expect("menu action");
    press(app, KeyCode::Enter);
}

#[test]
fn categories_contain_only_their_own_activity_specific_actions() {
    let mut app = make_test_app(1, 10);
    app.open_main_menu();
    assert_eq!(
        menu_labels(&app),
        [
            "Start Recording",
            "Open log",
            "Export Graphs to CSV",
            "Quit"
        ]
    );
    alt(&mut app, 'i');
    assert!(
        menu_labels(&app)
            .iter()
            .any(|label| label.contains("Tracking List"))
    );
    assert!(menu_labels(&app).contains(&"Open profile…".to_string()));
    assert!(menu_labels(&app).contains(&"Save profile".to_string()));
    assert!(menu_labels(&app).contains(&"Save profile as…".to_string()));
    assert!(menu_labels(&app).contains(&"Network endpoints".to_string()));
    alt(&mut app, 'v');
    assert_eq!(menu_labels(&app).len(), 8);
    assert!(menu_labels(&app).contains(&"Select columns…".to_string()));
    assert!(!menu_labels(&app).contains(&"Quit".to_string()));
    alt(&mut app, 't');
    assert_eq!(menu_labels(&app).len(), 6);
    assert_eq!(menu_labels(&app).last().unwrap(), "Startup Behavior");
    app.dismiss_main_menu();
    let path = start_recording(&mut app, "menu-categories");
    app.open_main_menu();
    assert_eq!(
        menu_labels(&app),
        ["Stop Recording", "Export Graphs to CSV", "Quit"]
    );
    alt(&mut app, 'i');
    assert_eq!(
        menu_labels(&app),
        [
            "Open profile…",
            "Network endpoints",
            "Find processes by file"
        ]
    );
    app.dismiss_main_menu();
    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
    app.log_view_path = Some(PathBuf::from("example.log"));
    app.open_main_menu();
    assert_eq!(
        menu_labels(&app),
        ["Open log", "Return to Live", "Export Graphs to CSV", "Quit"]
    );
    alt(&mut app, 'v');
    assert!(!menu_labels(&app).iter().any(|label| label.contains("Tree")));
}

#[test]
fn menu_routes_existing_dialogs_and_activity_transitions() {
    let mut app = make_test_app(1, 10);
    alt(&mut app, 'i');
    choose(&mut app, MainMenuAction::OpenProfiles);
    assert!(app.investigation_profiles_dialog.is_some());
    assert!(!app.is_main_menu_open());
    app.close_investigation_profiles();
    alt(&mut app, 'v');
    choose(&mut app, MainMenuAction::OpenColumns);
    assert!(app.show_column_picker);
    app.close_column_picker();
    alt(&mut app, 't');
    choose(&mut app, MainMenuAction::OpenStartupBehavior);
    assert!(matches!(
        app.investigation_profiles_view(),
        Some(InvestigationProfilesView::Startup { .. })
    ));
    app.close_investigation_profiles();
    alt(&mut app, 'f');
    choose(&mut app, MainMenuAction::StartRecording);
    assert!(app.show_recording_no_tracked_warning);
    press(&mut app, KeyCode::Esc);
    alt(&mut app, 'f');
    choose(&mut app, MainMenuAction::OpenLog);
    assert!(app.show_log_list);
    app.close_log_list();
    app.log_view_path = Some(PathBuf::from("example.log"));
    alt(&mut app, 'f');
    choose(&mut app, MainMenuAction::ReturnToLive);
    assert_eq!(app.activity(), AppActivity::Live);
    let path = start_recording(&mut app, "menu-stop");
    alt(&mut app, 'f');
    choose(&mut app, MainMenuAction::StopRecording);
    assert!(app.show_recording_stop_confirmation);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.activity(), AppActivity::Recording);
    app.stop_recording().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn menu_arrows_switch_categories_and_restore_workspace_focus() {
    let mut app = make_test_app(1, 10);
    let focus = app.focused_panel;
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.main_menu_section, MainMenuSection::Investigation);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.main_menu_selected, app.main_menu_rows().len() - 1);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.main_menu_selected, 0);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.main_menu_section, MainMenuSection::File);
    press(&mut app, KeyCode::Esc);
    assert!(!app.is_main_menu_open());
    assert_eq!(app.focused_panel, focus);
    app.network_browser.visible = true;
    alt(&mut app, 'v');
    assert_eq!(app.main_menu_section, MainMenuSection::View);
    press(&mut app, KeyCode::Esc);
    assert!(app.network_browser.visible);
    app.log_view_path = Some(PathBuf::from("example.log"));
    app.open_main_menu();
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.activity(), AppActivity::LogView);
}

#[test]
fn menu_access_keys_respect_editors_modals_and_existing_alt_h() {
    use crate::app::{ProcessPanelHeight, file_users::FileUsersFocus};
    let mut app = make_test_app(1, 10);
    for ch in ['f', 'i', 'v', 't'] {
        app.begin_filter_edit();
        alt(&mut app, ch);
        assert!(!app.is_main_menu_open());
        app.clear_filter();
        app.open_column_picker();
        alt(&mut app, ch);
        assert!(!app.is_main_menu_open());
        app.close_column_picker();
        app.on_key(KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ))
        .unwrap();
        assert!(!app.is_main_menu_open());
    }
    app.show_details = true;
    app.process_panel_height = ProcessPanelHeight::Auto;
    alt(&mut app, 'h');
    assert!(!app.is_main_menu_open());
    assert_eq!(app.process_panel_height, ProcessPanelHeight::Auto);
    app.network_browser.visible = true;
    app.network_browser.editing = true;
    alt(&mut app, 'i');
    assert!(!app.is_main_menu_open());
    app.network_browser.visible = false;
    app.file_users.visible = true;
    app.file_users.focus = FileUsersFocus::Query;
    app.file_users.draft = "draft path".into();
    alt(&mut app, 'v');
    assert!(!app.is_main_menu_open());
    assert_eq!(app.file_users.draft, "draft path");
    app.activate_header_action(HeaderAction::File);
    alt(&mut app, 'i');
    assert_eq!(app.main_menu_section, MainMenuSection::Investigation);
    assert_eq!(app.file_users.draft, "draft path");
}

#[test]
fn checkbox_and_theme_choices_apply_on_activation_without_leaving_menu() {
    let mut app = make_test_app(1, 10);
    alt(&mut app, 'v');
    choose(&mut app, MainMenuAction::ToggleTrackedOnly);
    assert!(app.watch_enabled);
    choose(&mut app, MainMenuAction::ToggleTreeView);
    assert_eq!(app.process_view_mode, ProcessViewMode::Tree);
    let samples = app.show_samples_panel;
    choose(&mut app, MainMenuAction::ToggleSamples);
    assert_ne!(app.show_samples_panel, samples);
    let layout = app.graph_slot_layout;
    choose(&mut app, MainMenuAction::CycleGraphLayout);
    assert_ne!(app.graph_slot_layout, layout);
    assert!(app.is_main_menu_open());
    alt(&mut app, 't');
    press(&mut app, KeyCode::Down);
    assert_eq!(app.theme_index, 0);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.theme_index, 1);
    choose(&mut app, MainMenuAction::ToggleContrast);
    assert!(app.high_contrast);
    assert!(app.is_main_menu_open());
}

#[test]
fn header_geometry_is_stable_and_categories_switch_with_one_click() {
    for width in [80, 120, 180] {
        let screen = Rect::new(0, 0, width, 60);
        let mut app = make_test_app(1, 10);
        let original = ui::header::header_actions(ui::screen_layout(screen)[0], &app);
        app.active_investigation_profile = Some("長い日本語のプロファイル名を繰り返す".repeat(10));
        app.toggle_display_pause();
        assert_eq!(
            original,
            ui::header::header_actions(ui::screen_layout(screen)[0], &app)
        );
        let text = render_app_to_text(&app, width, 60);
        assert!(!text.lines().next().unwrap().contains("MENU"));
        assert_eq!(original[0].0, HeaderAction::File);
        assert_eq!(original[0].1.x, 14);
        for action in original
            .iter()
            .map(|(action, _)| *action)
            .filter(|action| action.section().is_some() && *action != HeaderAction::Help)
        {
            let rect = original.iter().find(|(item, _)| *item == action).unwrap().1;
            app.on_mouse(left_click(rect.x + 1, rect.y), screen);
            assert!(app.is_main_menu_open());
            assert_eq!(Some(app.main_menu_section), action.section());
            let popup = main_menu_area(screen, &app);
            assert_eq!(
                popup.x,
                rect.x.min(screen.right().saturating_sub(popup.width))
            );
            app.on_mouse(mouse_move(width - 1, 40), screen);
            let buffer = render_app_to_buffer(&app, width, 60);
            assert_eq!(buffer[(rect.x + 1, rect.y)].bg, app.theme().menu_active);
        }
        let view = original
            .iter()
            .rev()
            .find(|(item, _)| item.section().is_some() && *item != HeaderAction::Help)
            .unwrap()
            .1;
        app.on_mouse(left_click(view.x, view.y), screen);
        assert!(!app.is_main_menu_open());
        app.on_mouse(left_click(view.x, view.y), screen);
        app.on_mouse(left_click(width - 1, 40), screen);
        assert!(!app.is_main_menu_open());
    }
}

#[test]
fn overflow_preserves_destinations_and_hidden_access_keys() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 60, 24);
    render_app_to_buffer(&app, 60, 24);
    let actions = ui::header::header_actions(ui::screen_layout(screen)[0], &app);
    let more = actions
        .iter()
        .find(|(action, _)| *action == HeaderAction::More)
        .unwrap()
        .1;
    app.on_mouse(left_click(more.x, more.y), screen);
    assert!(menu_labels(&app).contains(&"Investigation".to_string()));
    assert!(!menu_labels(&app).contains(&"Quit".to_string()));
    alt(&mut app, 't');
    assert_eq!(app.main_menu_section, MainMenuSection::Settings);
    assert!(main_menu_area(screen, &app).right() <= screen.right());
    app.close_main_menu();
    app.activate_header_action(HeaderAction::More);
    app.main_menu_selected = app
        .main_menu_rows()
        .iter()
        .position(|row| row.item == MainMenuItem::Header(HeaderAction::Investigation))
        .unwrap();
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.main_menu_section, MainMenuSection::Investigation);
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::Enter);
    assert!(app.file_users.visible);
    assert!(app.file_users.pending.is_none());
    assert!(render_app_to_text(&app, 60, 24).contains("PROCESSES USING FILE"));
}

#[test]
fn menu_shortcuts_align_and_preserve_semantic_colors_and_hover() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 120, 60);
    for (theme_index, theme) in ui::THEMES.iter().copied().enumerate() {
        app.theme_index = theme_index;
        alt(&mut app, 'i');
        app.main_menu_selected = 2;
        let buffer = render_app_to_buffer(&app, 120, 60);
        let (x1, y1) = find_text_position(&buffer, "Ctrl+T").unwrap();
        let (x2, y2) = find_text_position(&buffer, "Ctrl+S").unwrap();
        assert_eq!(x1 + 6, x2 + 6);
        assert!(find_text_position(&buffer, "Ctrl+Shift+S").is_none());
        assert_eq!(buffer[(x1, y1)].fg, theme.key_hint);
        let row = main_menu_item_area(screen, &app, 1).unwrap();
        app.on_mouse(mouse_move(row.x, row.y), screen);
        let hover = render_app_to_buffer(&app, 120, 60);
        assert_eq!(hover[(x1, y1)].bg, theme.focus_surface);
        assert_eq!(hover[(x1, y1)].fg, theme.key_hint);
        assert!(hover[(x1, y1)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(x2, y2)].fg, theme.key_hint);
        let heading = ui::header::header_actions(ui::screen_layout(screen)[0], &app)
            .into_iter()
            .find(|(a, _)| *a == HeaderAction::Investigation)
            .unwrap()
            .1;
        assert!(
            buffer[(heading.x + 1, 0)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }
}

#[test]
fn menu_hit_testing_scrolls_to_selected_items_on_short_screens() {
    let mut app = make_test_app(1, 10);
    alt(&mut app, 'v');
    press(&mut app, KeyCode::End);
    let screen = Rect::new(0, 0, 80, 9);
    render_app_to_buffer(&app, screen.width, screen.height);
    let row = main_menu_item_area(screen, &app, app.main_menu_selected).unwrap();
    assert_eq!(
        ui::main_menu_index_at(screen, &app, row.x, row.y),
        Some(app.main_menu_selected)
    );
    let popup = main_menu_area(screen, &app);
    assert_eq!(ui::main_menu_index_at(screen, &app, popup.x, popup.y), None);
    assert_eq!(
        ui::main_menu_index_at(screen, &app, popup.x + 2, popup.bottom() - 2),
        None
    );
}

#[test]
fn menu_rejects_actions_after_activity_changes() {
    let mut app = make_test_app(1, 10);
    alt(&mut app, 'i');
    app.log_view_path = Some(PathBuf::from("example.log"));
    press(&mut app, KeyCode::Enter);
    assert!(!app.is_main_menu_open());
    assert!(app.investigation_profiles_dialog.is_none());
}

#[test]
fn menu_mouse_opening_keeps_filter_draft_and_takes_keyboard_priority() {
    let mut app = make_test_app(1, 10);
    app.begin_filter_edit();
    app.push_filter_char('x');
    app.activate_header_action(HeaderAction::File);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.main_menu_section, MainMenuSection::Investigation);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.main_menu_selected, 1);
    press(&mut app, KeyCode::Esc);
    assert!(!app.is_main_menu_open());
    assert!(app.is_filter_editing());
    assert_eq!(app.filter_draft, "x");
}

#[test]
fn overflow_resize_dismisses_before_rows_can_change_targets() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 80, 24);
    crate::app::sync_layout_state(&mut app, screen);
    render_app_to_buffer(&app, 80, 24);
    app.activate_header_action(HeaderAction::More);
    press(&mut app, KeyCode::End);
    crate::app::sync_layout_state(&mut app, Rect::new(0, 0, 180, 60));
    assert!(!app.is_main_menu_open());
    assert!(!app.file_users.visible);
}

#[test]
fn narrow_header_keeps_pause_and_stale_status_after_menu_controls() {
    let mut app = make_test_app(1, 10);
    app.snapshot.captured_at = chrono::Local::now() - chrono::Duration::seconds(5);
    app.toggle_display_pause();
    let text = render_app_to_text(&app, 80, 24);
    let header = text.lines().next().unwrap();
    assert!(header.contains("LIVE"), "{header}");
    assert!(header.contains("STALE"), "{header}");
    assert!(header.contains("DISPLAY PAUSED"), "{header}");
    assert!(header.find("More").unwrap() < header.find("LIVE").unwrap());
}

#[test]
fn investigation_exposes_keyboard_routes_and_no_workspace_buttons_remain_in_header() {
    let mut app = make_test_app(1, 10);
    for width in [80, 120, 180] {
        let text = render_app_to_text(&app, width, 60);
        let header = text.lines().next().unwrap();
        for removed in ["[Processes]", "[Network]", "[Find by file]"] {
            assert!(!header.contains(removed));
        }
        for label in if width == 80 {
            vec!["File", "More"]
        } else {
            vec!["File", "Investigation", "View", "Settings", "Help"]
        } {
            assert!(header.contains(label), "{header}");
        }
        assert_eq!(header.contains("More"), width == 80, "{header}");
        alt(&mut app, 'i');
        let labels = menu_labels(&app);
        assert!(labels.contains(&"Network endpoints".to_string()));
        assert!(labels.contains(&"Find processes by file".to_string()));
        let text = render_app_to_text(&app, width, 60);
        assert!(text.contains("F3") && text.contains("F4"));
        press(&mut app, KeyCode::Esc);
    }
    let screen = Rect::new(0, 0, 120, 60);
    let heading = ui::header::header_actions(ui::screen_layout(screen)[0], &app)
        .into_iter()
        .find(|(a, _)| *a == HeaderAction::Investigation)
        .unwrap()
        .1;
    app.on_mouse(left_click(heading.x, heading.y), screen);
    let index = app
        .main_menu_rows()
        .iter()
        .position(|row| row.item == MainMenuItem::Header(HeaderAction::FileUsers))
        .unwrap();
    let row = main_menu_item_area(screen, &app, index).unwrap();
    app.on_mouse(left_click(row.x, row.y), screen);
    assert!(app.file_users.visible);
    press(&mut app, KeyCode::F(2));
    app.log_view_path = Some(PathBuf::from("example.log"));
    alt(&mut app, 'i');
    assert_eq!(menu_labels(&app), ["Open profile…"]);
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::F(3));
    press(&mut app, KeyCode::F(4));
    assert!(!app.network_browser.visible && !app.file_users.visible);
}

#[test]
fn profile_label_keeps_both_padding_cells_when_truncated() {
    for name in ["chrome".to_string(), "調査プロファイル名".repeat(10)] {
        for width in [80, 120, 180] {
            let mut app = make_test_app(1, 10);
            app.active_investigation_profile = Some(name.clone());
            let buffer = render_app_to_buffer(&app, width, 60);
            let (x, y) = find_text_position(&buffer, "PF:").unwrap();
            assert_eq!(buffer[(x - 1, y)].symbol(), " ");
            assert_eq!(buffer[(x - 1, y)].bg, app.theme().menu_bar);
            assert_eq!(buffer[(width - 1, y)].symbol(), " ");
            assert_eq!(buffer[(width - 1, y)].bg, app.theme().menu_bar);
        }
    }
}

#[test]
fn header_arrows_reach_help_in_every_activity_and_terminal_width() {
    for activity in [
        AppActivity::Live,
        AppActivity::Recording,
        AppActivity::LogView,
    ] {
        for width in [80, 120, 180] {
            let mut app = make_test_app(1, 10);
            let path = if activity == AppActivity::Recording {
                Some(start_recording(&mut app, "header-help-navigation"))
            } else {
                None
            };
            if activity == AppActivity::LogView {
                app.log_view_path = Some(PathBuf::from("example.log"));
            }
            let screen = Rect::new(0, 0, width, 60);
            app.set_screen_area(screen);
            render_app_to_buffer(&app, width, 60);
            let focus = app.focused_panel;
            press(&mut app, KeyCode::Esc);
            let mut expected = vec![MainMenuSection::Investigation, MainMenuSection::View];
            expected.extend([MainMenuSection::Settings, MainMenuSection::Help]);
            for section in expected {
                press(&mut app, KeyCode::Right);
                assert_eq!(app.main_menu_section, section);
                assert!(app.is_main_menu_open());
                assert!(!app.show_help);
            }
            let buffer = render_app_to_buffer(&app, width, 60);
            let heading = ui::header::header_actions(ui::screen_layout(screen)[0], &app)
                .into_iter()
                .find(|(action, _)| {
                    *action
                        == if width == 80 {
                            HeaderAction::More
                        } else {
                            HeaderAction::Help
                        }
                })
                .unwrap()
                .1;
            assert_eq!(
                buffer[(heading.x + 1, heading.y)].bg,
                app.theme().menu_active
            );
            assert!(render_app_to_text(&app, width, 60).contains("Version Info"));
            assert!(!main_menu_area(screen, &app).is_empty());
            assert!(main_menu_item_area(screen, &app, 0).is_some());
            if width == 80 {
                press(&mut app, KeyCode::Right);
                assert_eq!(app.main_menu_section, MainMenuSection::More);
            }
            press(&mut app, KeyCode::Right);
            assert_eq!(app.main_menu_section, MainMenuSection::File);
            press(&mut app, KeyCode::Left);
            if width == 80 {
                assert_eq!(app.main_menu_section, MainMenuSection::More);
                press(&mut app, KeyCode::Left);
            }
            assert_eq!(app.main_menu_section, MainMenuSection::Help);
            press(&mut app, KeyCode::Left);
            assert_eq!(app.main_menu_section, MainMenuSection::Settings);
            press(&mut app, KeyCode::Right);
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Enter);
            assert!(app.show_help);
            assert!(!app.is_main_menu_open());
            assert_eq!(ui::help::section_titles()[app.help_section], "Global");
            press(&mut app, KeyCode::Esc);
            assert_eq!(app.focused_panel, focus);
            assert_eq!(app.activity(), activity);
            if let Some(path) = path {
                app.stop_recording().unwrap();
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}

#[test]
fn focused_help_cancels_or_opens_from_header_and_footer_without_click_through() {
    let screen = Rect::new(0, 0, 120, 60);
    let mut app = make_test_app(1, 10);
    render_app_to_buffer(&app, screen.width, screen.height);
    let focus = app.focused_panel;
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.main_menu_section, MainMenuSection::Help);
    for key in [KeyCode::Up, KeyCode::Down, KeyCode::Home, KeyCode::End] {
        press(&mut app, key);
        assert!(!app.show_help);
    }
    press(&mut app, KeyCode::Esc);
    assert!(!app.is_main_menu_open());
    assert_eq!(app.focused_panel, focus);

    for label in ["Version Info", "Keyboard shortcuts"] {
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Left);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        let (x, y) = find_text_position(&buffer, label).unwrap();
        app.on_mouse(left_click(x + 1, y), screen);
        assert!(app.show_help);
        assert!(!app.is_main_menu_open());
        press(&mut app, KeyCode::Esc);
    }
    press(&mut app, KeyCode::F(1));
    assert!(app.show_help);
    assert_eq!(ui::help::section_titles()[app.help_section], "Processes");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Left);
    app.on_mouse(left_click(screen.width - 1, 20), screen);
    assert!(!app.is_main_menu_open());
    assert!(!app.show_help);
    assert_eq!(app.focused_panel, focus);
}

#[test]
fn group_separators_are_not_clickable_and_navigation_uses_action_indices() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 120, 30);
    alt(&mut app, 'i');
    let first = main_menu_item_area(screen, &app, 0).unwrap();
    let profiles = main_menu_item_area(screen, &app, 1).unwrap();
    assert_eq!(profiles.y, first.y + 2);
    let buffer = render_app_to_buffer(&app, screen.width, screen.height);
    for x in first.x..first.right() {
        assert_eq!(buffer[(x, first.y + 1)].symbol(), "─");
    }
    assert_eq!(
        ui::main_menu_index_at(screen, &app, first.x, first.y + 1),
        None
    );
    let selected = app.main_menu_selected;
    app.on_mouse(left_click(first.x, first.y + 1), screen);
    assert!(app.is_main_menu_open());
    assert_eq!(app.main_menu_selected, selected);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.main_menu_selected, 1);
    press(&mut app, KeyCode::End);
    assert_eq!(app.main_menu_selected, app.main_menu_rows().len() - 1);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.main_menu_selected, 0);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.main_menu_selected, app.main_menu_rows().len() - 1);
    press(&mut app, KeyCode::Home);
    assert_eq!(app.main_menu_selected, 0);
}

#[test]
fn menu_tracking_uses_captured_name_after_selection_changes() {
    let mut app = make_test_app(2, 10);
    alt(&mut app, 'i');
    let target = app.main_menu_tracking_target.clone().unwrap();
    assert!(menu_labels(&app)[0].contains(&target));
    app.select_process_index(1);
    choose(&mut app, MainMenuAction::ToggleProcessTracking);
    assert!(app.is_tracked_process_name(&target));
    assert!(!app.is_main_menu_open());
    app.open_main_menu_section(MainMenuSection::View);
    assert!(!menu_labels(&app).contains(&"Copy process table".to_string()));
    assert!(!menu_labels(&app).contains(&"Export Graphs to CSV".to_string()));
}

#[test]
fn menu_tracking_is_unavailable_during_recording_log_or_other_workspaces() {
    let mut app = make_test_app(1, 10);
    let path = start_recording(&mut app, "menu-tracking-locked");
    alt(&mut app, 'i');
    assert!(
        !app.main_menu_rows()
            .iter()
            .any(|row| row.item == MainMenuItem::Action(MainMenuAction::ToggleProcessTracking))
    );
    app.dismiss_main_menu();
    app.stop_recording().unwrap();
    std::fs::remove_file(path).unwrap();
    app.log_view_path = Some(PathBuf::from("example.log"));
    alt(&mut app, 'i');
    assert_eq!(menu_labels(&app), ["Open profile…"]);
    app.dismiss_main_menu();
    app.log_view_path = None;
    app.network_browser.visible = true;
    app.open_main_menu_section(MainMenuSection::Investigation);
    assert!(app.main_menu_tracking_target.is_none());
    assert!(
        !menu_labels(&app)
            .iter()
            .any(|label| label.contains("Tracking List"))
    );
}

#[test]
fn compact_header_keeps_recording_pause_stale_and_stable_geometry() {
    let mut app = make_test_app(1, 10);
    let screen = Rect::new(0, 0, 80, 24);
    let original = ui::header::header_actions(ui::screen_layout(screen)[0], &app);
    app.recording_interval_index = 3;
    let path = start_recording(&mut app, "menu-critical-status");
    app.snapshot.captured_at = chrono::Local::now() - chrono::Duration::seconds(999999);
    app.toggle_display_pause();
    app.active_investigation_profile = Some("調査プロファイル".repeat(20));
    let text = render_app_to_text(&app, 80, 24);
    let header = text.lines().next().unwrap();
    for label in ["REC", "STALE", "DISPLAY PAUSED", "File", "More"] {
        assert!(header.contains(label), "{header}");
    }
    assert!(!header.contains("AVG"), "{header}");
    assert_eq!(
        original,
        ui::header::header_actions(ui::screen_layout(screen)[0], &app)
    );
    app.dismiss_main_menu();
    app.stop_recording().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn header_bar_has_continuous_neutral_surface_and_distinct_active_state() {
    for theme in 0..ui::THEMES.len() {
        for high_contrast in [false, true] {
            let mut app = make_test_app(1, 10);
            app.theme_index = theme;
            app.high_contrast = high_contrast;
            app.active_investigation_profile = None;
            let screen = Rect::new(0, 0, 120, 30);
            let buffer = render_app_to_buffer(&app, 120, 30);
            let colors = app.theme();
            assert_ne!(colors.menu_bar, colors.panel);
            assert_ne!(colors.menu_active, colors.menu_bar);
            // Unused cells between headings and status retain the same full-width bar.
            for x in 56..80 {
                assert_eq!(buffer[(x, 0)].bg, colors.menu_bar);
            }
            let actions = ui::header::header_actions(ui::screen_layout(screen)[0], &app);
            for (action, rect) in actions {
                assert_eq!(buffer[(rect.x, 0)].bg, colors.menu_bar);
                if action == HeaderAction::File {
                    app.open_main_menu_section(MainMenuSection::File);
                    let buffer = render_app_to_buffer(&app, 120, 30);
                    assert_eq!(buffer[(rect.x + 1, 0)].bg, colors.menu_active);
                    assert!(buffer[(rect.x + 1, 0)].modifier.contains(Modifier::BOLD));
                    app.close_main_menu();
                }
            }
        }
    }
}

#[test]
fn file_export_opens_the_same_empty_graph_dialog_and_keeps_shortcut_hint() {
    let mut app = make_test_app(1, 10);
    alt(&mut app, 'f');
    let text = render_app_to_text(&app, 120, 30);
    assert!(text.contains("Ctrl+E"));
    choose(&mut app, MainMenuAction::ExportGraphsCsv);
    assert!(render_app_to_text(&app, 120, 30).contains("Please add metrics to Graph slots."));
}

#[test]
fn long_unicode_tracking_name_keeps_shortcuts_visible_and_overflow_target_fixed() {
    let mut app = make_test_app(2, 10);
    app.snapshot.processes[0].name = "長い調査対象プロセス名".repeat(20);
    app.rebuild_visible_process_cache();
    app.select_process_index(0);
    for width in [60, 80, 120] {
        app.close_main_menu();
        let screen = Rect::new(0, 0, width, 24);
        render_app_to_buffer(&app, width, 24);
        app.open_main_menu();
        let target = app.main_menu_tracking_target.clone();
        app.select_process_index(1);
        if width == 60 {
            app.open_main_menu_section(MainMenuSection::More);
            let index = app
                .main_menu_rows()
                .iter()
                .position(|row| row.item == MainMenuItem::Header(HeaderAction::Investigation))
                .unwrap();
            app.activate_main_menu_at(index).unwrap();
        } else {
            app.open_main_menu_section(MainMenuSection::Investigation);
        }
        assert_eq!(app.main_menu_tracking_target, target);
        let buffer = render_app_to_buffer(&app, width, 24);
        let row = main_menu_item_area(screen, &app, 0).unwrap();
        assert!(find_text_position(&buffer, "t (Processes)").is_some());
        assert!(find_text_position(&buffer, "Ctrl+Shift+S").is_none());
        assert!(find_text_position(&buffer, "Save profile as").is_some());
        assert_eq!(ui::main_menu_index_at(screen, &app, row.x, row.y), Some(0));
        app.select_process_index(0);
    }
}

#[test]
fn view_separates_column_selection_visibility_graphs_and_layout() {
    for log_view in [false, true] {
        let mut app = make_test_app(1, 10);
        if log_view {
            app.log_view_path = Some(PathBuf::from("example.log"));
        }
        alt(&mut app, 'v');
        let screen = Rect::new(0, 0, 120, 30);
        let buffer = render_app_to_buffer(&app, screen.width, screen.height);
        assert!(find_text_position(&buffer, "Select columns…").is_some());
        let items = app.main_menu_rows();
        let area = |action| {
            main_menu_item_area(
                screen,
                &app,
                items
                    .iter()
                    .position(|item| item.item == MainMenuItem::Action(action))
                    .unwrap(),
            )
            .unwrap()
        };
        for (before, after) in [
            (
                MainMenuAction::OpenColumns,
                MainMenuAction::ToggleTrackedOnly,
            ),
            (
                MainMenuAction::ToggleDelta,
                MainMenuAction::CycleGraphLayout,
            ),
        ] {
            let before = area(before);
            let after = area(after);
            assert_eq!(after.y, before.y + 2);
            assert_eq!(
                ui::main_menu_index_at(screen, &app, before.x, before.y + 1),
                None
            );
            for x in before.x..before.right() {
                assert_eq!(buffer[(x, before.y + 1)].symbol(), "─");
            }
        }
    }
}

#[test]
fn workspaces_and_dropdowns_start_directly_below_the_single_row_menu() {
    for width in [80, 120, 180] {
        for theme_index in 0..ui::THEMES.len() {
            for high_contrast in [false, true] {
                let mut app = make_test_app(1, 10);
                app.theme_index = theme_index;
                app.high_contrast = high_contrast;
                let screen = Rect::new(0, 0, width, 30);
                for (workspace, title) in ["MEM", "NETWORK ENDPOINTS", "PROCESSES USING FILE"]
                    .into_iter()
                    .enumerate()
                {
                    app.network_browser.visible = workspace == 1;
                    app.file_users.visible = workspace == 2;
                    let buffer = render_app_to_buffer(&app, width, 30);
                    let (menu_x, menu_y) = find_text_position(&buffer, "File").unwrap();
                    assert_eq!(menu_y, 0);
                    assert_eq!(
                        ui::header::header_action_at(screen, &app, menu_x, menu_y),
                        Some(ui::header::HeaderAction::File)
                    );
                    assert_eq!(find_text_position(&buffer, title).unwrap().1, 1);
                    for x in 0..width {
                        assert_eq!(ui::header::header_action_at(screen, &app, x, 1), None);
                    }
                    app.open_main_menu_section(MainMenuSection::File);
                    let buffer = render_app_to_buffer(&app, width, 30);
                    let dropdown = main_menu_area(screen, &app);
                    assert_eq!(dropdown.y, 1);
                    assert_ne!(buffer[(dropdown.x, dropdown.y)].symbol(), " ");
                    assert_eq!(find_text_position(&buffer, "File").unwrap().1, 0);
                    app.close_main_menu();
                }
            }
        }
    }
}

#[test]
fn header_keeps_product_name_without_version() {
    for width in [80, 120, 180] {
        let mut app = make_test_app(1, 10);
        let screen = Rect::new(0, 0, width, 30);
        let text = render_app_to_text(&app, width, 30);
        assert!(text.lines().next().unwrap().starts_with(" winproc-tui "));
        assert_eq!(ui::header::header_action_at(screen, &app, 1, 0), None);
        app.on_mouse(left_click(1, 0), screen);
        assert!(!app.is_main_menu_open());
        assert!(
            !text
                .lines()
                .next()
                .unwrap()
                .contains(env!("CARGO_PKG_VERSION"))
        );
        let actions = ui::header::header_actions(ui::screen_layout(screen)[0], &app);
        assert_eq!(actions[0].1.x, 14);
        assert_eq!(
            ui::header::header_action_at(screen, &app, 15, 0),
            Some(HeaderAction::File)
        );
        app.on_mouse(left_click(15, 0), screen);
        assert!(app.is_main_menu_open());
        assert_eq!(app.main_menu_section, MainMenuSection::File);
    }
}

#[test]
fn version_info_contains_metadata_and_blocks_workspace_input() {
    for width in [80, 120, 180] {
        for activity in [
            AppActivity::Live,
            AppActivity::Recording,
            AppActivity::LogView,
        ] {
            let mut app = make_test_app(2, 10);
            let path = if activity == AppActivity::Recording {
                Some(start_recording(&mut app, "version-info"))
            } else {
                None
            };
            if activity == AppActivity::LogView {
                app.log_view_path = Some(PathBuf::from("example.log"));
            }
            let screen = Rect::new(0, 0, width, 30);
            render_app_to_buffer(&app, width, 30);
            let focus = app.focused_panel;
            app.open_main_menu_section(MainMenuSection::Help);
            assert_eq!(menu_labels(&app), ["Version Info", "Keyboard shortcuts"]);
            choose(&mut app, MainMenuAction::OpenVersionInfo);
            assert!(app.show_help && app.help_version_info);
            assert!(!app.is_main_menu_open());
            let buffer = render_app_to_buffer(&app, width, 30);
            let text = render_app_to_text(&app, width, 30);
            assert!(text.contains("Version Info"));
            let (x, y) =
                find_text_position(&buffer, concat!("winproc-tui v", env!("CARGO_PKG_VERSION")))
                    .unwrap();
            let (url_x, url_y) = find_text_position(&buffer, env!("CARGO_PKG_REPOSITORY")).unwrap();
            assert_eq!((url_x, url_y), (x, y + 1));
            let (footer_x, footer_y) = find_text_position(&buffer, "Enter/Esc Close").unwrap();
            assert!(footer_y >= url_y + 2);
            for key in [
                KeyCode::Char('q'),
                KeyCode::Char('s'),
                KeyCode::Down,
                KeyCode::Tab,
            ] {
                press(&mut app, key);
                assert!(app.show_help && app.help_version_info);
                assert!(app.help_section_picker.is_none());
                assert!(!app.should_quit);
                assert_eq!(app.focused_panel, focus);
            }
            render_app_to_buffer(&app, width, 30);
            app.on_mouse(left_click(footer_x, footer_y), screen);
            assert!(!app.show_help && !app.help_version_info);
            assert_eq!(app.activity(), activity);
            app.open_main_menu_section(MainMenuSection::Help);
            choose(&mut app, MainMenuAction::OpenKeyboardShortcuts);
            assert!(app.show_help && !app.help_version_info);
            assert_eq!(app.help_scroll.offset, 0);
            assert_eq!(app.help_section, 0);
            let text = render_app_to_text(&app, width, 30);
            let title_row = text
                .lines()
                .position(|line| line.contains("Keyboard shortcuts"))
                .unwrap();
            assert!(text.lines().nth(title_row + 1).unwrap().contains("Global"));
            assert!(!text.contains(env!("CARGO_PKG_REPOSITORY")));
            press(&mut app, KeyCode::Esc);
            if let Some(path) = path {
                app.stop_recording().unwrap();
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}
