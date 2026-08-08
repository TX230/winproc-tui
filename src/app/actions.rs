use std::time::Instant;

use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::{
    app::{
        App, AppActivity, FocusedPanel, GraphPanDrag, GraphPanDragButton, ProcessInfoFocus,
        ProcessKillSelection, QuitConfirmSelection, RecordingOverwriteSelection,
        TrackedListConfirmSelection, TrackedListsButton, TrackedListsView, TrackedRemoveSelection,
    },
    platform::send_terminal_zoom_shortcut,
    ui::{
        THEMES, TrackedListNameButton, column_picker_close_button_area_for_screen,
        column_picker_index_at, column_picker_scrollbar_area, cpu_panel_area_for_screen,
        details_panel::graph_y_axis_label_width,
        display_area_warning_ok_button_area, help_area, help_close_button_area,
        help_scrollbar_area,
        layout::{
            ProcessTableLayout, details_graph_area, details_graph_chart_area, details_samples_area,
            details_shared_controls_area, details_slot_areas, details_slot_title_area,
            graph_shared_control_areas,
        },
        log_dir_button_at, log_dir_input_area, log_list_button_at, log_list_index_at,
        main_panel_areas_for_app, metric_column_warning_ok_button_area,
        no_graph_metrics_warning_ok_button_area, process_info_close_button_area_for_screen,
        process_info_content_area_for_screen, process_info_tab_at, process_kill_button_at,
        process_metric_column_index_at, process_tracked_only_control_area, quit_confirm_button_at,
        ram_vram_panel_area_for_screen, recording_no_tracked_ok_button_area,
        recording_overwrite_button_at, recording_path_button_at, recording_path_input_area,
        system_activity_panel_area_for_screen, system_info_ok_button_area_for_screen,
        tracked_list_confirm_button_at, tracked_list_index_at, tracked_list_name_button_at,
        tracked_list_save_name_area_for_screen, tracked_list_startup_area_for_screen,
        tracked_lists_button_at, tracked_remove_button_at,
    },
};

const PROCESS_WHEEL_ROWS: usize = 1;
const RAM_VRAM_SEPARATOR_ROW: usize = 2;

impl App {
    pub(crate) fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.kind == KeyEventKind::Release {
            return Ok(());
        }

        if self.show_display_area_warning {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.dismiss_display_area_warning(),
                _ => {}
            }
            return Ok(());
        }

        if self.show_metric_column_warning {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.dismiss_metric_column_warning(),
                _ => {}
            }
            return Ok(());
        }

        if self.show_no_graph_metrics_warning {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.dismiss_no_graph_metrics_warning(),
                _ => {}
            }
            return Ok(());
        }

        if let Some(view) = self.tracked_lists_view().cloned() {
            match view {
                TrackedListsView::Browse => match key.code {
                    KeyCode::Esc => self.close_tracked_lists(),
                    KeyCode::Tab => self.focus_next_tracked_lists_control(),
                    KeyCode::BackTab => self.focus_previous_tracked_lists_control(),
                    KeyCode::Backspace if self.tracked_lists_save_name_focused() => {
                        self.pop_tracked_list_save_name_char();
                    }
                    KeyCode::Delete if self.tracked_lists_save_name_focused() => {
                        self.delete_tracked_list_save_name_char();
                    }
                    KeyCode::Delete => self.request_delete_selected_tracked_list(),
                    KeyCode::F(2) => self.begin_tracked_list_rename(),
                    KeyCode::Left if self.tracked_lists_save_name_focused() => {
                        self.move_tracked_list_save_name_cursor_left();
                    }
                    KeyCode::Right if self.tracked_lists_save_name_focused() => {
                        self.move_tracked_list_save_name_cursor_right();
                    }
                    KeyCode::Left if self.tracked_lists_startup_focused() => {
                        self.select_previous_tracked_list_startup();
                    }
                    KeyCode::Right | KeyCode::Char(' ') if self.tracked_lists_startup_focused() => {
                        self.select_next_tracked_list_startup();
                    }
                    KeyCode::Home if self.tracked_lists_save_name_focused() => {
                        self.move_tracked_list_save_name_cursor_home();
                    }
                    KeyCode::End if self.tracked_lists_save_name_focused() => {
                        self.move_tracked_list_save_name_cursor_end();
                    }
                    KeyCode::Enter => {
                        if self.tracked_lists_save_name_focused() {
                            self.save_current_tracked_list();
                        } else if self.tracked_lists_startup_focused() {
                            self.select_next_tracked_list_startup();
                        } else if let Some(button) = self.tracked_lists_focused_button() {
                            self.activate_tracked_lists_button(button);
                        } else {
                            self.load_selected_tracked_list();
                        }
                    }
                    KeyCode::Left if self.tracked_lists_focused_button().is_some() => {
                        self.focus_previous_tracked_lists_control();
                    }
                    KeyCode::Right if self.tracked_lists_focused_button().is_some() => {
                        self.focus_next_tracked_lists_control();
                    }
                    KeyCode::Up
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_up(1);
                    }
                    KeyCode::Down
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_down(1);
                    }
                    KeyCode::PageUp
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_up(
                            self.tracked_lists_dialog
                                .as_ref()
                                .map(|dialog| dialog.scroll.page_size)
                                .unwrap_or(1),
                        );
                    }
                    KeyCode::PageDown
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_down(
                            self.tracked_lists_dialog
                                .as_ref()
                                .map(|dialog| dialog.scroll.page_size)
                                .unwrap_or(1),
                        );
                    }
                    KeyCode::Home
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_home();
                    }
                    KeyCode::End
                        if self.tracked_lists_focused_button().is_none()
                            && !self.tracked_lists_save_name_focused()
                            && !self.tracked_lists_startup_focused() =>
                    {
                        self.move_tracked_list_selection_end();
                    }
                    KeyCode::Char(ch)
                        if self.tracked_lists_save_name_focused()
                            && !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        self.push_tracked_list_save_name_char(ch);
                    }
                    KeyCode::Char(ch)
                        if ch.eq_ignore_ascii_case(&'s') && key.modifiers.is_empty() =>
                    {
                        self.save_current_tracked_list();
                    }
                    _ => {}
                },
                TrackedListsView::NameInput { .. } => match key.code {
                    KeyCode::Esc => self.cancel_tracked_list_subdialog(),
                    KeyCode::Enter => self.commit_tracked_list_name_input(),
                    KeyCode::Backspace => self.pop_tracked_list_name_char(),
                    KeyCode::Delete => self.delete_tracked_list_name_char(),
                    KeyCode::Left => self.move_tracked_list_name_cursor_left(),
                    KeyCode::Right => self.move_tracked_list_name_cursor_right(),
                    KeyCode::Home => self.move_tracked_list_name_cursor_home(),
                    KeyCode::End => self.move_tracked_list_name_cursor_end(),
                    KeyCode::Char(ch)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        self.push_tracked_list_name_char(ch);
                    }
                    _ => {}
                },
                TrackedListsView::ConfirmDelete { .. } | TrackedListsView::ConfirmSwitch { .. } => {
                    match key.code {
                        KeyCode::Esc => self.cancel_tracked_list_subdialog(),
                        KeyCode::Enter => self.activate_tracked_list_confirmation(),
                        KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'y') => {
                            self.set_tracked_list_confirmation_selection(
                                TrackedListConfirmSelection::Apply,
                            );
                            self.activate_tracked_list_confirmation();
                        }
                        KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                            self.cancel_tracked_list_subdialog();
                        }
                        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                            self.toggle_tracked_list_confirmation_selection();
                        }
                        _ => {}
                    }
                }
            }
            return Ok(());
        }

        if self.show_quit_confirmation {
            match key.code {
                KeyCode::Enter => self.activate_quit_selection()?,
                KeyCode::Char('q') => self.confirm_quit()?,
                KeyCode::Esc => self.cancel_quit_confirmation(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                    self.cancel_quit_confirmation();
                }
                KeyCode::Left => self.select_previous_quit_action(),
                KeyCode::Right | KeyCode::Tab => self.select_next_quit_action(),
                _ => {}
            }
            return Ok(());
        }

        if self.show_recording_overwrite_confirmation {
            match key.code {
                KeyCode::Enter => self.activate_recording_overwrite_selection()?,
                KeyCode::Esc => self.cancel_recording_overwrite_confirmation(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                    self.cancel_recording_overwrite_confirmation();
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'y') => {
                    self.confirm_recording_overwrite()?;
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    self.toggle_recording_overwrite_selection();
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_recording_no_tracked_warning {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.dismiss_recording_no_tracked_warning(),
                _ => {}
            }
            return Ok(());
        }

        if self.show_tracked_remove_confirmation {
            match key.code {
                KeyCode::Enter => self.activate_tracked_remove_selection(),
                KeyCode::Esc => self.cancel_tracked_remove_confirmation(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                    self.cancel_tracked_remove_confirmation();
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'y') => {
                    self.confirm_tracked_remove();
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    self.toggle_tracked_remove_selection();
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_process_kill_confirmation {
            match key.code {
                KeyCode::Enter => self.activate_process_kill_selection(),
                KeyCode::Esc => self.cancel_process_kill_confirmation(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                    self.cancel_process_kill_confirmation();
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'y') => {
                    self.confirm_process_kill();
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    self.toggle_process_kill_selection();
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_recording_path_dialog {
            match key.code {
                KeyCode::Esc => self.cancel_recording_path_dialog(),
                KeyCode::Enter => self.activate_recording_path_selection()?,
                KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.focus_previous_recording_path_control()
                }
                KeyCode::Tab => self.focus_next_recording_path_control(),
                KeyCode::BackTab => self.focus_previous_recording_path_control(),
                KeyCode::Char(' ')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.recording_path_selection
                            == crate::app::RecordingPathSelection::Path =>
                {
                    self.complete_recording_path();
                }
                KeyCode::Backspace
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.pop_recording_path_char();
                }
                KeyCode::Delete
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.delete_recording_path_char();
                }
                KeyCode::Left
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.move_recording_path_cursor_left();
                }
                KeyCode::Left => self.focus_previous_recording_path_control(),
                KeyCode::Right
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.move_recording_path_cursor_right();
                }
                KeyCode::Right => self.focus_next_recording_path_control(),
                KeyCode::Home
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.move_recording_path_cursor_home();
                }
                KeyCode::End
                    if self.recording_path_selection
                        == crate::app::RecordingPathSelection::Path =>
                {
                    self.move_recording_path_cursor_end();
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                        && self.recording_path_selection
                            == crate::app::RecordingPathSelection::Path =>
                {
                    self.push_recording_path_char(ch);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_log_dir_dialog {
            match key.code {
                KeyCode::Esc => self.cancel_log_dir_dialog(),
                KeyCode::Enter => self.activate_log_dir_selection()?,
                KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.focus_previous_log_dir_control()
                }
                KeyCode::Tab => self.focus_next_log_dir_control(),
                KeyCode::BackTab => self.focus_previous_log_dir_control(),
                KeyCode::Char(' ')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.log_dir_selection == crate::app::LogDirSelection::Path =>
                {
                    self.complete_log_dir();
                }
                KeyCode::Backspace
                    if self.log_dir_selection == crate::app::LogDirSelection::Path =>
                {
                    self.pop_log_dir_char();
                }
                KeyCode::Delete if self.log_dir_selection == crate::app::LogDirSelection::Path => {
                    self.delete_log_dir_char();
                }
                KeyCode::Left if self.log_dir_selection == crate::app::LogDirSelection::Path => {
                    self.move_log_dir_cursor_left();
                }
                KeyCode::Left => self.focus_previous_log_dir_control(),
                KeyCode::Right if self.log_dir_selection == crate::app::LogDirSelection::Path => {
                    self.move_log_dir_cursor_right();
                }
                KeyCode::Right => self.focus_next_log_dir_control(),
                KeyCode::Home if self.log_dir_selection == crate::app::LogDirSelection::Path => {
                    self.move_log_dir_cursor_home();
                }
                KeyCode::End if self.log_dir_selection == crate::app::LogDirSelection::Path => {
                    self.move_log_dir_cursor_end();
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                        && self.log_dir_selection == crate::app::LogDirSelection::Path =>
                {
                    self.push_log_dir_char(ch);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => {
                    self.close_help();
                }
                KeyCode::Up => self.scroll_help_up(1),
                KeyCode::Down => self.scroll_help_down(1),
                KeyCode::PageUp => self.scroll_help_up(self.help_scroll.page_size),
                KeyCode::PageDown => self.scroll_help_down(self.help_scroll.page_size),
                KeyCode::Home => self.scroll_help_home(),
                KeyCode::End => self.scroll_help_end(),
                _ => {}
            }
            return Ok(());
        }

        if self.is_log_list_open() {
            match key.code {
                KeyCode::Esc => self.close_log_list(),
                KeyCode::Enter => self.activate_log_list_control()?,
                KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.focus_previous_log_list_control()
                }
                KeyCode::Tab => self.focus_next_log_list_control(),
                KeyCode::BackTab => self.focus_previous_log_list_control(),
                KeyCode::Up => self.move_log_list_up(1),
                KeyCode::Down => self.move_log_list_down(1),
                KeyCode::PageUp => self.move_log_list_up(self.log_list_scroll.page_size),
                KeyCode::PageDown => self.move_log_list_down(self.log_list_scroll.page_size),
                KeyCode::Home => self.move_log_list_home(),
                KeyCode::End => self.move_log_list_end(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'r') => {
                    self.refresh_log_list()?;
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'d') => {
                    self.open_log_dir_dialog()?;
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_process_info_dialog {
            match key.code {
                KeyCode::Esc if self.close_process_info_detail() => {}
                KeyCode::Esc => self.close_process_info_dialog(),
                KeyCode::Enter if self.process_info_focus == ProcessInfoFocus::Close => {
                    self.close_process_info_dialog()
                }
                KeyCode::Enter if self.process_info_detail_is_open() => {
                    self.close_process_info_detail();
                }
                KeyCode::Enter
                    if matches!(
                        self.process_info_tab,
                        crate::app::ProcessInfoTab::Dlls | crate::app::ProcessInfoTab::Environment
                    ) =>
                {
                    self.open_selected_process_info_detail();
                }
                KeyCode::Enter => self.close_process_info_dialog(),
                KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.previous_process_info_tab()?
                }
                KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.next_process_info_tab()?
                }
                KeyCode::Tab
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.focus_previous_process_info_control()
                }
                KeyCode::Tab if key.modifiers.is_empty() => self.focus_next_process_info_control(),
                KeyCode::BackTab
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.focus_previous_process_info_control()
                }
                _ if self.process_info_focus != ProcessInfoFocus::Content => {}
                KeyCode::Up
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_up(1)
                }
                KeyCode::Down
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_down(1)
                }
                KeyCode::PageUp
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_up(self.process_info_page_size())
                }
                KeyCode::PageDown
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_down(self.process_info_page_size())
                }
                KeyCode::Home
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_home()
                }
                KeyCode::End
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_end()
                }
                KeyCode::Up
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_up(1)
                }
                KeyCode::Down
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_down(1)
                }
                KeyCode::PageUp
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_up(self.process_info_page_size())
                }
                KeyCode::PageDown
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_down(self.process_info_page_size())
                }
                KeyCode::Home
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_home()
                }
                KeyCode::End
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_end()
                }
                KeyCode::Up => self.scroll_process_info_up(1),
                KeyCode::Down => self.scroll_process_info_down(1),
                KeyCode::PageUp => self.scroll_process_info_up(self.process_info_page_size()),
                KeyCode::PageDown => self.scroll_process_info_down(self.process_info_page_size()),
                KeyCode::Home => self.scroll_process_info_home(),
                KeyCode::End => self.scroll_process_info_end(),
                KeyCode::Left if self.process_info_tab == crate::app::ProcessInfoTab::Files => {
                    self.move_open_files_filter_cursor_left()
                }
                KeyCode::Right if self.process_info_tab == crate::app::ProcessInfoTab::Files => {
                    self.move_open_files_filter_cursor_right()
                }
                KeyCode::Left
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_filter_cursor_left()
                }
                KeyCode::Right
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.move_process_modules_filter_cursor_right()
                }
                KeyCode::Left
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_filter_cursor_left()
                }
                KeyCode::Right
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.move_process_environment_filter_cursor_right()
                }
                KeyCode::Backspace
                    if self.process_info_tab == crate::app::ProcessInfoTab::Files =>
                {
                    self.pop_open_files_filter_char()
                }
                KeyCode::Delete if self.process_info_tab == crate::app::ProcessInfoTab::Files => {
                    self.delete_open_files_filter_char()
                }
                KeyCode::Backspace
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.pop_process_modules_filter_char()
                }
                KeyCode::Delete
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail =>
                {
                    self.delete_process_modules_filter_char()
                }
                KeyCode::Backspace
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.pop_process_environment_filter_char()
                }
                KeyCode::Delete
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail =>
                {
                    self.delete_process_environment_filter_char()
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.process_info_tab == crate::app::ProcessInfoTab::Files =>
                {
                    self.copy_open_files_to_clipboard()?;
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.process_info_tab == crate::app::ProcessInfoTab::Dlls =>
                {
                    self.copy_selected_process_module_to_clipboard()?;
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.process_info_tab == crate::app::ProcessInfoTab::Environment =>
                {
                    self.copy_selected_process_environment_to_clipboard()?;
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'u')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    match self.process_info_tab {
                        crate::app::ProcessInfoTab::Image => self.refresh_selected_process_info(),
                        crate::app::ProcessInfoTab::Files => self.refresh_open_files()?,
                        crate::app::ProcessInfoTab::Dlls => self.refresh_process_modules()?,
                        crate::app::ProcessInfoTab::Environment => {
                            self.refresh_process_environment()?
                        }
                        crate::app::ProcessInfoTab::Metrics => {}
                    }
                }
                KeyCode::Char(ch)
                    if self.process_info_tab == crate::app::ProcessInfoTab::Files
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.push_open_files_filter_char(ch);
                }
                KeyCode::Char(ch)
                    if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && !self.process_modules_show_detail
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.push_process_modules_filter_char(ch);
                }
                KeyCode::Char(ch)
                    if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && !self.process_environment_show_detail
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.push_process_environment_filter_char(ch);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.show_system_info_dialog {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.close_system_info_dialog(),
                _ => {}
            }
            return Ok(());
        }

        if self.is_process_jump_editing() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.close_process_jump_edit(),
                KeyCode::Up => {
                    self.close_process_jump_edit();
                    self.move_selection_up(1);
                }
                KeyCode::Down => {
                    self.close_process_jump_edit();
                    self.move_selection_down(1);
                }
                KeyCode::Backspace => self.pop_process_jump_char(),
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'i')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.jump_to_next_process_match();
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'j')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.jump_to_next_process_match();
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.push_process_jump_char(ch);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.is_filter_editing() {
            match key.code {
                KeyCode::Esc => self.clear_filter(),
                KeyCode::Enter => self.commit_filter_edit(),
                KeyCode::Up => {
                    self.commit_filter_edit();
                    self.move_selection_up(1);
                }
                KeyCode::Down => {
                    self.commit_filter_edit();
                    self.move_selection_down(1);
                }
                KeyCode::Backspace => self.pop_filter_char(),
                KeyCode::Char(' ')
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.add_selected_process_to_watch_list();
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.push_filter_char(ch);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.is_column_picker_open() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.close_column_picker(),
                KeyCode::Up => self.move_column_picker_up(),
                KeyCode::Down => self.move_column_picker_down(),
                KeyCode::PageUp => {
                    self.move_column_picker_up_by(self.column_picker_scroll.page_size)
                }
                KeyCode::PageDown => {
                    self.move_column_picker_down_by(self.column_picker_scroll.page_size)
                }
                KeyCode::Home => self.move_column_picker_home(),
                KeyCode::End => self.move_column_picker_end(),
                KeyCode::Char(' ') => self.toggle_picker_column(),
                _ => {}
            }
            return Ok(());
        }

        if matches!(
            self.focused_panel,
            FocusedPanel::DetailsGraph | FocusedPanel::DetailsSamples
        ) && self.show_details
        {
            match key.code {
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'z')
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.toggle_graph_y_axis_zero_min();
                    return Ok(());
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'f')
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.toggle_graph_all_samples();
                    return Ok(());
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'v')
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.toggle_samples_panel();
                    return Ok(());
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'d')
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.toggle_sample_delta();
                    return Ok(());
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'l')
                        && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.toggle_graph_slot_layout();
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.focused_panel == FocusedPanel::DetailsSamples && self.show_details {
            match key.code {
                KeyCode::Up => {
                    self.select_details_sample_older(1);
                    return Ok(());
                }
                KeyCode::Down => {
                    self.select_details_sample_newer(1);
                    return Ok(());
                }
                KeyCode::PageUp => {
                    self.select_details_sample_older(self.details_sample_page_size);
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.select_details_sample_newer(self.details_sample_page_size);
                    return Ok(());
                }
                KeyCode::Home => {
                    self.select_details_sample_oldest();
                    return Ok(());
                }
                KeyCode::End => {
                    self.select_details_sample_latest();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.status = format!("Sample selected: {}", self.details_sample_selected + 1);
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.focused_panel == FocusedPanel::DetailsGraph && self.show_details {
            match key.code {
                KeyCode::Enter => {
                    self.open_active_graph_process_info_dialog()?;
                    return Ok(());
                }
                KeyCode::PageUp => {
                    self.zoom_graph_time_span(true);
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.zoom_graph_time_span(false);
                    return Ok(());
                }
                KeyCode::Left => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        self.shift_graph_time_window(true);
                    } else {
                        self.select_details_sample_older(1);
                    }
                    return Ok(());
                }
                KeyCode::Right => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        self.shift_graph_time_window(false);
                    } else {
                        self.select_details_sample_newer(1);
                    }
                    return Ok(());
                }
                KeyCode::Home => {
                    self.select_details_sample_oldest();
                    return Ok(());
                }
                KeyCode::End => {
                    self.select_details_sample_latest();
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.focused_panel == FocusedPanel::System {
            match key.code {
                KeyCode::Up => {
                    self.select_previous_system_metric();
                    self.apply_selected_system_metric_to_visible_details();
                    return Ok(());
                }
                KeyCode::Down => {
                    self.select_next_system_metric();
                    self.apply_selected_system_metric_to_visible_details();
                    return Ok(());
                }
                KeyCode::Home => {
                    self.select_first_system_metric();
                    return Ok(());
                }
                KeyCode::End => {
                    self.select_last_system_metric();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.apply_selected_system_metric_to_details();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    self.status = "RAM/VRAM metrics keep 7200 samples automatically".to_string();
                    return Ok(());
                }
                KeyCode::Char(ch @ '1'..='4') if key.modifiers.is_empty() => {
                    self.toggle_selected_system_metric_for_graph_slot((ch as u8 - b'1') as usize);
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.focused_panel == FocusedPanel::SystemActivity {
            match key.code {
                KeyCode::Up => {
                    self.select_previous_system_activity_metric();
                    self.apply_selected_system_activity_metric_to_visible_details();
                    return Ok(());
                }
                KeyCode::Down => {
                    self.select_next_system_activity_metric();
                    self.apply_selected_system_activity_metric_to_visible_details();
                    return Ok(());
                }
                KeyCode::Home => {
                    self.select_first_system_activity_metric();
                    return Ok(());
                }
                KeyCode::End => {
                    self.select_last_system_activity_metric();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.apply_selected_system_activity_metric_to_details();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    self.status =
                        "System Activity metrics keep 7200 samples automatically".to_string();
                    return Ok(());
                }
                KeyCode::Char(ch @ '1'..='4') if key.modifiers.is_empty() => {
                    self.toggle_selected_system_activity_metric_for_graph_slot(
                        (ch as u8 - b'1') as usize,
                    );
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.focused_panel == FocusedPanel::Cpu {
            match key.code {
                KeyCode::Enter => {
                    self.status = "CPUs metric selected: CPU Usage".to_string();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    self.status = "CPU average keeps 7200 samples automatically".to_string();
                    return Ok(());
                }
                KeyCode::Char(ch @ '1'..='4') if key.modifiers.is_empty() => {
                    self.toggle_cpu_average_for_graph_slot((ch as u8 - b'1') as usize);
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc if self.activity() == AppActivity::LogView => {
                self.exit_log_view();
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.request_quit_confirmation();
            }
            KeyCode::Tab => {
                self.cycle_focus();
            }
            KeyCode::BackTab => {
                self.cycle_focus_previous();
            }
            KeyCode::Left => {
                if self.focused_panel == FocusedPanel::Processes {
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.move_selected_process_column_left();
                    } else {
                        self.select_previous_process_column();
                    }
                }
            }
            KeyCode::Right => {
                if self.focused_panel == FocusedPanel::Processes {
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.move_selected_process_column_right();
                    } else {
                        self.select_next_process_column();
                    }
                }
            }
            KeyCode::Up => {
                if self.focused_panel == FocusedPanel::Processes {
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.extend_process_selection_up(1);
                    } else if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.move_selection_cursor_up(1);
                    } else {
                        self.move_selection_up(1);
                    }
                }
            }
            KeyCode::Down => {
                if self.focused_panel == FocusedPanel::Processes {
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.extend_process_selection_down(1);
                    } else if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::SHIFT)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.move_selection_cursor_down(1);
                    } else {
                        self.move_selection_down(1);
                    }
                }
            }
            KeyCode::PageUp => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.move_selection_up(self.process_page_size);
                }
            }
            KeyCode::PageDown => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.move_selection_down(self.process_page_size);
                }
            }
            KeyCode::Home => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.select_first_row();
                }
            }
            KeyCode::End => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.select_last_row();
                }
            }
            KeyCode::Enter => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.open_selected_process_info_dialog()?;
                }
            }
            KeyCode::Char(ch @ '1'..='4') => {
                if self.focused_panel == FocusedPanel::Processes && key.modifiers.is_empty() {
                    self.toggle_selected_metric_for_graph_slot((ch as u8 - b'1') as usize);
                }
            }
            KeyCode::Char('0') => {
                if self.focused_panel == FocusedPanel::Processes && key.modifiers.is_empty() {
                    self.clear_graph_slots();
                }
            }
            KeyCode::Delete => {
                if self.focused_panel == FocusedPanel::Processes {
                    if !self.request_process_kill_confirmation()
                        && !self.clear_selected_graph_metric()
                    {
                        self.hide_selected_ghost_row();
                    }
                }
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'d')
                    && key.modifiers.is_empty()
                    && self.focused_panel == FocusedPanel::Processes =>
            {
                if !self.request_process_kill_confirmation() && !self.clear_selected_graph_metric()
                {
                    self.hide_selected_ghost_row();
                }
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'c')
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.open_column_picker();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'w')
                    && self.focused_panel == FocusedPanel::Processes
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if ch.is_ascii_uppercase() || key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.narrow_selected_process_column();
                } else {
                    self.widen_selected_process_column();
                }
            }
            KeyCode::Char('s') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cycle_sort_column();
            }
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'g') => {
                self.toggle_details();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'f')
                    && key.modifiers.is_empty()
                    && self.focused_panel == FocusedPanel::Processes =>
            {
                self.open_selected_process_files()?;
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'a')
                    && key.modifiers.contains(KeyModifiers::SHIFT)
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.jump_to_ab_point_a();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'b')
                    && key.modifiers.contains(KeyModifiers::SHIFT)
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.jump_to_ab_point_b();
            }
            KeyCode::Char('a') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.set_ab_point_a();
            }
            KeyCode::Char('b') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.set_ab_point_b();
            }
            KeyCode::Char('x') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear_ab_comparison_with_status();
            }
            KeyCode::Char(' ')
                if self.focused_panel == FocusedPanel::Processes
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.toggle_focused_process_multi_selection();
            }
            KeyCode::Char(' ') => {
                if self.focused_panel == FocusedPanel::Processes
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.toggle_selected_process_tracking();
                }
            }
            KeyCode::Char('t')
                if self.focused_panel == FocusedPanel::Processes && key.modifiers.is_empty() =>
            {
                self.toggle_watch_list();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'f')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.focused_panel == FocusedPanel::Processes =>
            {
                self.begin_filter_edit();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'i')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && self.focused_panel == FocusedPanel::Processes =>
            {
                self.begin_process_jump_edit();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'j')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && self.focused_panel == FocusedPanel::Processes =>
            {
                self.begin_process_jump_edit();
            }
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'i') && key.modifiers.is_empty() => {
                self.open_system_info_dialog();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'r')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.toggle_recording()?;
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'p')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.toggle_display_pause();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'t')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.open_tracked_lists();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'l')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.open_log_list()?;
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.copy_focused_cell_to_clipboard()?;
            }
            KeyCode::Char('+') => {
                self.status = "Sampling interval is fixed at 1s".to_string();
            }
            KeyCode::Char('-') => {
                self.status = "Sampling interval is fixed at 1s".to_string();
            }
            KeyCode::F(2) => {
                self.theme_index = (self.theme_index + 1) % THEMES.len();
                self.status = format!("Theme switched to {}", self.theme().name);
            }
            KeyCode::Char('?') => {
                self.toggle_help();
            }
            _ => {}
        }

        Ok(())
    }

    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent, screen_area: Rect) {
        if let Some(zoom_in) = terminal_zoom_direction(&mouse) {
            if let Err(error) = send_terminal_zoom_shortcut(zoom_in) {
                self.status = format!("Terminal zoom failed: {error}");
            }
            return;
        }

        if let Some(view) = self.tracked_lists_view().cloned() {
            if matches!(&view, TrackedListsView::Browse) && mouse.kind == MouseEventKind::Moved {
                let hovered = tracked_lists_button_at(screen_area, mouse.column, mouse.row);
                self.set_tracked_lists_hovered_button(hovered);
                return;
            }
            if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
                return;
            }
            match view {
                TrackedListsView::Browse => {
                    if let Some(index) = tracked_list_index_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        self.tracked_lists_scroll_offset(),
                        self.tracked_lists_entry_count(),
                    ) {
                        self.focus_tracked_lists_list();
                        self.set_tracked_lists_hovered_button(None);
                        self.select_tracked_list_index(index);
                        if index == 0 {
                            self.load_selected_tracked_list();
                        }
                        return;
                    }
                    if tracked_list_save_name_area_for_screen(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
                    {
                        self.focus_tracked_lists_save_name();
                        self.set_tracked_lists_hovered_button(None);
                        return;
                    }
                    if tracked_list_startup_area_for_screen(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
                    {
                        self.focus_tracked_lists_startup();
                        self.set_tracked_lists_hovered_button(None);
                        self.select_next_tracked_list_startup();
                        return;
                    }
                    if let Some(button) =
                        tracked_lists_button_at(screen_area, mouse.column, mouse.row)
                    {
                        self.focus_tracked_lists_button(button);
                        self.set_tracked_lists_hovered_button(Some(button));
                        self.activate_tracked_lists_button(button);
                    }
                }
                TrackedListsView::NameInput { .. } => {
                    match tracked_list_name_button_at(screen_area, mouse.column, mouse.row) {
                        Some(TrackedListNameButton::Apply) => self.commit_tracked_list_name_input(),
                        Some(TrackedListNameButton::Cancel) => self.cancel_tracked_list_subdialog(),
                        None => {}
                    }
                }
                confirm_view @ TrackedListsView::ConfirmDelete { .. }
                | confirm_view @ TrackedListsView::ConfirmSwitch { .. } => {
                    let apply_label =
                        if matches!(confirm_view, TrackedListsView::ConfirmDelete { .. }) {
                            " Delete "
                        } else {
                            " Load "
                        };
                    if let Some(selection) = tracked_list_confirm_button_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        apply_label,
                    ) {
                        self.set_tracked_list_confirmation_selection(selection);
                        self.activate_tracked_list_confirmation();
                    }
                }
            }
            return;
        }

        if self.show_display_area_warning {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && display_area_warning_ok_button_area(screen_area)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
            {
                self.dismiss_display_area_warning();
            }
            return;
        }

        if self.show_metric_column_warning {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && metric_column_warning_ok_button_area(screen_area)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
            {
                self.dismiss_metric_column_warning();
            }
            return;
        }

        if self.show_no_graph_metrics_warning {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && no_graph_metrics_warning_ok_button_area(screen_area)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
            {
                self.dismiss_no_graph_metrics_warning();
            }
            return;
        }

        if self.show_help {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if help_close_button_area_for_screen(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row)) =>
                {
                    self.close_help();
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.start_help_scrollbar_drag(mouse.column, mouse.row, screen_area);
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.help_scroll.stop_drag();
                }
                MouseEventKind::Drag(MouseButton::Left) if self.help_scroll.dragging => {
                    self.drag_help_scrollbar(mouse.row, screen_area);
                }
                MouseEventKind::ScrollUp => self.scroll_help_up(1),
                MouseEventKind::ScrollDown => self.scroll_help_down(1),
                _ => {}
            }
            return;
        }

        if self.show_column_picker {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if column_picker_close_button_area_for_screen(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row)) =>
                {
                    self.close_column_picker();
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if self.start_column_picker_scrollbar_drag(mouse.column, mouse.row, screen_area)
                    {
                        return;
                    }
                    if let Some(index) = column_picker_index_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        self.column_picker_scroll.offset,
                    ) {
                        self.toggle_picker_column_at(index);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.column_picker_scroll.stop_drag();
                }
                MouseEventKind::Drag(MouseButton::Left) if self.column_picker_scroll.dragging => {
                    self.drag_column_picker_scrollbar(mouse.row, screen_area);
                }
                MouseEventKind::ScrollUp => self.scroll_column_picker_up(1),
                MouseEventKind::ScrollDown => self.scroll_column_picker_down(1),
                _ => {}
            }
            return;
        }

        if self.show_log_dir_dialog {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if contains_point(log_dir_input_area(screen_area), mouse.column, mouse.row) {
                    self.log_dir_selection = crate::app::LogDirSelection::Path;
                    return;
                }
                match log_dir_button_at(screen_area, mouse.column, mouse.row) {
                    Some(crate::app::LogDirSelection::Path) => {}
                    Some(crate::app::LogDirSelection::Apply) => {
                        self.log_dir_selection = crate::app::LogDirSelection::Apply;
                        if let Err(error) = self.confirm_log_dir() {
                            self.status = format!("Log directory failed: {error}");
                        }
                    }
                    Some(crate::app::LogDirSelection::Cancel) => {
                        self.log_dir_selection = crate::app::LogDirSelection::Cancel;
                        self.cancel_log_dir_dialog();
                    }
                    None => {}
                }
            }
            return;
        }

        if self.show_log_list {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(focus) = log_list_button_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        self.log_summaries.len(),
                    ) {
                        self.log_list_focus = focus;
                        if let Err(error) = self.activate_log_list_control() {
                            self.status = format!("Log action failed: {error}");
                        }
                        return;
                    }
                    if let Some(index) = log_list_index_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        self.log_list_scroll.offset,
                        self.log_summaries.len(),
                    ) {
                        self.log_list_focus = crate::app::LogListFocus::List;
                        self.click_log_list_index(index, Instant::now());
                    }
                }
                MouseEventKind::ScrollUp => self.scroll_log_list_up(1),
                MouseEventKind::ScrollDown => self.scroll_log_list_down(1),
                _ => {}
            }
            return;
        }

        if self.show_process_info_dialog {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if process_info_close_button_area_for_screen(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row)) =>
                {
                    self.close_process_info_dialog();
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(tab) = process_info_tab_at(screen_area, mouse.column, mouse.row) {
                        if let Err(error) = self.activate_process_info_tab(tab) {
                            self.status = format!("Process Info tab failed: {error}");
                        }
                    } else if self.process_info_tab == crate::app::ProcessInfoTab::Dlls
                        && contains_point(
                            process_info_content_area_for_screen(screen_area),
                            mouse.column,
                            mouse.row,
                        )
                    {
                        self.process_info_focus = ProcessInfoFocus::Content;
                        let content = process_info_content_area_for_screen(screen_area);
                        if let Some(index) = crate::ui::process_modules::process_module_index_at(
                            content,
                            self,
                            mouse.column,
                            mouse.row,
                        ) {
                            self.select_process_module(index);
                        } else {
                            self.start_process_info_scrollbar_drag(
                                mouse.column,
                                mouse.row,
                                screen_area,
                            );
                        }
                    } else if self.process_info_tab == crate::app::ProcessInfoTab::Environment
                        && contains_point(
                            process_info_content_area_for_screen(screen_area),
                            mouse.column,
                            mouse.row,
                        )
                    {
                        self.process_info_focus = ProcessInfoFocus::Content;
                        let content = process_info_content_area_for_screen(screen_area);
                        if let Some(index) =
                            crate::ui::process_environment::process_environment_index_at(
                                content,
                                self,
                                mouse.column,
                                mouse.row,
                            )
                        {
                            self.select_process_environment(index);
                        } else {
                            self.start_process_info_scrollbar_drag(
                                mouse.column,
                                mouse.row,
                                screen_area,
                            );
                        }
                    } else if contains_point(
                        process_info_content_area_for_screen(screen_area),
                        mouse.column,
                        mouse.row,
                    ) {
                        self.process_info_focus = ProcessInfoFocus::Content;
                        self.start_process_info_scrollbar_drag(
                            mouse.column,
                            mouse.row,
                            screen_area,
                        );
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.stop_process_info_scrollbar_drag();
                }
                MouseEventKind::Drag(MouseButton::Left)
                    if self.process_info_scrollbar_dragging() =>
                {
                    self.drag_process_info_scrollbar(mouse.row, screen_area);
                }
                MouseEventKind::ScrollUp
                    if contains_point(
                        process_info_content_area_for_screen(screen_area),
                        mouse.column,
                        mouse.row,
                    ) =>
                {
                    self.process_info_focus = ProcessInfoFocus::Content;
                    self.scroll_process_info_up(1);
                }
                MouseEventKind::ScrollDown
                    if contains_point(
                        process_info_content_area_for_screen(screen_area),
                        mouse.column,
                        mouse.row,
                    ) =>
                {
                    self.process_info_focus = ProcessInfoFocus::Content;
                    self.scroll_process_info_down(1);
                }
                _ => {}
            }
            return;
        }

        if self.show_system_info_dialog {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && system_info_ok_button_area_for_screen(screen_area)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
            {
                self.close_system_info_dialog();
            }
            return;
        }

        if self.show_quit_confirmation {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match quit_confirm_button_at(
                    screen_area,
                    mouse.column,
                    mouse.row,
                    self.recording_session.is_some(),
                ) {
                    Some(QuitConfirmSelection::Quit) => {
                        if let Err(error) = self.confirm_quit() {
                            self.status = format!("Quit failed: {error}");
                        }
                    }
                    Some(QuitConfirmSelection::Cancel) => self.cancel_quit_confirmation(),
                    None => {}
                }
            }
            return;
        }

        if self.show_recording_overwrite_confirmation {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match recording_overwrite_button_at(screen_area, mouse.column, mouse.row) {
                    Some(RecordingOverwriteSelection::Overwrite) => {
                        if let Err(error) = self.confirm_recording_overwrite() {
                            self.status = format!("Recording failed: {error}");
                        }
                    }
                    Some(RecordingOverwriteSelection::Cancel) => {
                        self.cancel_recording_overwrite_confirmation();
                    }
                    None => {}
                }
            }
            return;
        }

        if self.show_recording_no_tracked_warning {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && recording_no_tracked_ok_button_area(screen_area)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
            {
                self.dismiss_recording_no_tracked_warning();
            }
            return;
        }

        if self.show_tracked_remove_confirmation {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match tracked_remove_button_at(screen_area, mouse.column, mouse.row) {
                    Some(TrackedRemoveSelection::Remove) => self.confirm_tracked_remove(),
                    Some(TrackedRemoveSelection::Cancel) => {
                        self.cancel_tracked_remove_confirmation()
                    }
                    None => {}
                }
            }
            return;
        }

        if self.show_process_kill_confirmation {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match process_kill_button_at(screen_area, mouse.column, mouse.row) {
                    Some(ProcessKillSelection::Kill) => self.confirm_process_kill(),
                    Some(ProcessKillSelection::Cancel) => self.cancel_process_kill_confirmation(),
                    None => {}
                }
            }
            return;
        }

        if self.show_recording_path_dialog {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if contains_point(
                    recording_path_input_area(screen_area),
                    mouse.column,
                    mouse.row,
                ) {
                    self.recording_path_selection = crate::app::RecordingPathSelection::Path;
                    return;
                }
                match recording_path_button_at(screen_area, mouse.column, mouse.row) {
                    Some(crate::app::RecordingPathSelection::Path) => {}
                    Some(crate::app::RecordingPathSelection::Start) => {
                        self.recording_path_selection = crate::app::RecordingPathSelection::Start;
                        if let Err(error) = self.confirm_recording_path() {
                            self.status = format!("Recording failed: {error}");
                        }
                    }
                    Some(crate::app::RecordingPathSelection::Cancel) => {
                        self.recording_path_selection = crate::app::RecordingPathSelection::Cancel;
                        self.cancel_recording_path_dialog();
                    }
                    None => {}
                }
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(slot_index) =
                    graph_item_area_at(self, screen_area, mouse.column, mouse.row)
                {
                    self.active_graph_slot_index = slot_index;
                    if let Some(slot) = self.graph_slot(slot_index).cloned() {
                        if let Some(identity) = slot.process_identity() {
                            let identity = identity.clone();
                            self.focused_panel = FocusedPanel::Processes;
                            self.select_process_identity(&identity);
                        } else {
                            self.focused_panel = if slot.system_metric()
                                == Some(crate::model::SystemMetric::CpuAverage)
                            {
                                FocusedPanel::Cpu
                            } else {
                                FocusedPanel::System
                            };
                        }
                    }
                    return;
                }
                if process_tracked_only_control_area_for_screen(screen_area, self)
                    .is_some_and(|area| contains_point(area, mouse.column, mouse.row))
                {
                    self.focused_panel = FocusedPanel::Processes;
                    self.toggle_watch_list();
                    return;
                }
                if self.start_samples_scrollbar_drag(mouse.column, mouse.row, screen_area) {
                    return;
                }
                if mouse.modifiers.contains(KeyModifiers::CONTROL)
                    && self.start_graph_pan_drag(
                        mouse.column,
                        mouse.row,
                        screen_area,
                        GraphPanDragButton::Left,
                    )
                {
                    return;
                }
                if self.toggle_graph_all_samples_at(mouse.column, mouse.row, screen_area) {
                    return;
                }
                if self.toggle_graph_y_axis_at(mouse.column, mouse.row, screen_area) {
                    return;
                }
                if self.toggle_samples_panel_at(mouse.column, mouse.row, screen_area) {
                    return;
                }
                if self.toggle_sample_delta_at(mouse.column, mouse.row, screen_area) {
                    return;
                }
                if self.toggle_graph_slot_layout_at(mouse.column, mouse.row, screen_area) {
                    return;
                }
                self.focus_panel_at(mouse.column, mouse.row, screen_area);
                self.select_system_metric_row_at(mouse.column, mouse.row, screen_area);
                self.select_system_activity_metric_row_at(mouse.column, mouse.row, screen_area);
                self.select_process_row_at(mouse.column, mouse.row, screen_area);
                self.select_details_sample_at(mouse.column, mouse.row, screen_area);
                self.select_details_sample_from_graph_at(mouse.column, mouse.row, screen_area);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.samples_scrollbar_dragging = false;
                self.samples_scrollbar_grab_offset = 0;
                self.stop_graph_pan_drag(GraphPanDragButton::Left);
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if self.start_graph_pan_drag(
                    mouse.column,
                    mouse.row,
                    screen_area,
                    GraphPanDragButton::Right,
                ) {
                    return;
                }
                if let Some((slot_index, _)) =
                    samples_area_at(self, screen_area, mouse.column, mouse.row)
                {
                    self.active_graph_slot_index = slot_index;
                    self.focused_panel = FocusedPanel::DetailsSamples;
                    self.enter_details_live_mode();
                }
            }
            MouseEventKind::Up(MouseButton::Right) => {
                if let Some(drag) = self.stop_graph_pan_drag(GraphPanDragButton::Right)
                    && !drag.moved
                {
                    self.reset_graph_to_live_edge();
                }
            }
            MouseEventKind::ScrollUp => self.scroll_at(
                mouse.column,
                mouse.row,
                screen_area,
                true,
                mouse.modifiers.contains(KeyModifiers::SHIFT),
            ),
            MouseEventKind::ScrollDown => {
                self.scroll_at(
                    mouse.column,
                    mouse.row,
                    screen_area,
                    false,
                    mouse.modifiers.contains(KeyModifiers::SHIFT),
                );
            }
            MouseEventKind::ScrollLeft => {
                self.pan_graph_at(mouse.column, mouse.row, screen_area, true, true);
            }
            MouseEventKind::ScrollRight => {
                self.pan_graph_at(mouse.column, mouse.row, screen_area, false, true);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.drag_graph_time_window(mouse.column, screen_area, GraphPanDragButton::Left)
                {
                    return;
                }
                if self.samples_scrollbar_dragging {
                    self.drag_samples_scrollbar(mouse.column, mouse.row, screen_area);
                    return;
                }
                if let Some((slot_index, _)) =
                    graph_area_at(self, screen_area, mouse.column, mouse.row)
                {
                    self.active_graph_slot_index = slot_index;
                    self.focused_panel = FocusedPanel::DetailsGraph;
                    self.select_details_sample_from_graph_at(mouse.column, mouse.row, screen_area);
                }
            }
            MouseEventKind::Drag(MouseButton::Right) => {
                self.drag_graph_time_window(mouse.column, screen_area, GraphPanDragButton::Right);
            }
            _ => {}
        }
    }

    fn activate_tracked_lists_button(&mut self, button: TrackedListsButton) {
        match button {
            TrackedListsButton::Save => self.save_current_tracked_list(),
            TrackedListsButton::Close => self.close_tracked_lists(),
        }
    }

    fn start_help_scrollbar_drag(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(scrollbar) = help_scrollbar_area(screen_area, self.help_scroll.page_size) else {
            self.help_scroll.stop_drag();
            return false;
        };
        if !contains_point(scrollbar, x, y) {
            self.help_scroll.stop_drag();
            return false;
        }

        let total = self.help_scroll_total();
        self.help_scroll.start_drag(scrollbar, y, total);
        self.help_scroll.drag_to(scrollbar, y, total);
        true
    }

    fn drag_help_scrollbar(&mut self, y: u16, screen_area: Rect) {
        let Some(scrollbar) = help_scrollbar_area(screen_area, self.help_scroll.page_size) else {
            self.help_scroll.stop_drag();
            return;
        };
        let total = self.help_scroll_total();
        self.help_scroll.drag_to(scrollbar, y, total);
    }

    fn start_column_picker_scrollbar_drag(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(scrollbar) =
            column_picker_scrollbar_area(screen_area, self.column_picker_scroll.page_size)
        else {
            self.column_picker_scroll.stop_drag();
            return false;
        };
        if !contains_point(scrollbar, x, y) {
            self.column_picker_scroll.stop_drag();
            return false;
        }

        let total = self.column_picker_scroll_total();
        self.column_picker_scroll.start_drag(scrollbar, y, total);
        self.column_picker_scroll.drag_to(scrollbar, y, total);
        true
    }

    fn drag_column_picker_scrollbar(&mut self, y: u16, screen_area: Rect) {
        let Some(scrollbar) =
            column_picker_scrollbar_area(screen_area, self.column_picker_scroll.page_size)
        else {
            self.column_picker_scroll.stop_drag();
            return;
        };
        let total = self.column_picker_scroll_total();
        self.column_picker_scroll.drag_to(scrollbar, y, total);
    }

    fn start_samples_scrollbar_drag(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some((slot_index, scrollbar)) = samples_scrollbar_area_at(self, screen_area, x, y)
        else {
            self.samples_scrollbar_dragging = false;
            return false;
        };

        self.active_graph_slot_index = slot_index;
        self.samples_scrollbar_dragging = true;
        self.samples_scrollbar_grab_offset = samples_scrollbar_grab_offset_at(
            scrollbar,
            y,
            self.selected_sample_count(),
            self.details_sample_page_size,
            self.details_sample_offset,
        )
        .unwrap_or(0);
        self.focused_panel = FocusedPanel::DetailsSamples;
        self.drag_samples_scrollbar(x, y, screen_area);
        true
    }

    fn toggle_graph_y_axis_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(area) = graph_shared_control_areas_for_app(self, screen_area).y_axis else {
            return false;
        };
        if !contains_point(area, x, y) {
            return false;
        }
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_graph_y_axis_zero_min();
        true
    }

    fn toggle_graph_all_samples_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(area) = graph_shared_control_areas_for_app(self, screen_area).all_samples else {
            return false;
        };
        if !contains_point(area, x, y) {
            return false;
        }
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_graph_all_samples();
        true
    }

    fn toggle_samples_panel_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(area) = graph_shared_control_areas_for_app(self, screen_area).samples else {
            return false;
        };
        if !contains_point(area, x, y) {
            return false;
        }
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_samples_panel();
        true
    }

    fn toggle_sample_delta_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(area) = graph_shared_control_areas_for_app(self, screen_area).delta else {
            return false;
        };
        if !contains_point(area, x, y) {
            return false;
        }
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_sample_delta();
        true
    }

    fn toggle_graph_slot_layout_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some(area) = graph_shared_control_areas_for_app(self, screen_area).layout else {
            return false;
        };
        if !contains_point(area, x, y) {
            return false;
        }
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_graph_slot_layout();
        true
    }

    fn drag_samples_scrollbar(&mut self, _x: u16, y: u16, screen_area: Rect) {
        let sample_count = self.selected_sample_count();
        let Some(scrollbar) = active_samples_scrollbar_area_for_screen(self, screen_area) else {
            self.samples_scrollbar_dragging = false;
            return;
        };
        if let Some(offset) = samples_scrollbar_offset_at(
            scrollbar,
            y,
            sample_count,
            self.details_sample_page_size,
            self.samples_scrollbar_grab_offset,
        ) {
            self.set_details_sample_offset(offset);
        }
    }

    fn start_graph_pan_drag(
        &mut self,
        x: u16,
        y: u16,
        screen_area: Rect,
        button: GraphPanDragButton,
    ) -> bool {
        let Some((slot_index, _)) = graph_area_at(self, screen_area, x, y) else {
            self.stop_graph_pan_drag(button);
            return false;
        };
        self.active_graph_slot_index = slot_index;
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.graph_pan_drag = Some(GraphPanDrag {
            button,
            start_x: x,
            start_offset_seconds: self.graph_time_offset_seconds,
            moved: false,
        });
        true
    }

    fn drag_graph_time_window(
        &mut self,
        x: u16,
        screen_area: Rect,
        button: GraphPanDragButton,
    ) -> bool {
        let Some(mut drag) = self.graph_pan_drag else {
            return false;
        };
        if drag.button != button {
            return false;
        }
        let Some(area) = active_graph_chart_area_for_screen(self, screen_area) else {
            self.graph_pan_drag = None;
            return false;
        };

        if self.graph_show_all_samples {
            drag.moved |= x != drag.start_x;
            self.graph_pan_drag = Some(drag);
            return true;
        }

        let plot_width = i64::from(area.width.saturating_sub(1).max(1));
        let dx = i64::from(x) - i64::from(drag.start_x);
        let offset_delta = dx * i64::from(self.graph_time_span_seconds) / plot_width;
        let next_offset = i64::from(drag.start_offset_seconds) + offset_delta;
        let next_offset = next_offset.max(0) as u32;
        drag.moved |= dx != 0;
        self.graph_pan_drag = Some(drag);
        self.set_graph_time_window_offset(next_offset);
        true
    }

    fn stop_graph_pan_drag(&mut self, button: GraphPanDragButton) -> Option<GraphPanDrag> {
        let drag = self.graph_pan_drag?;
        if drag.button == button {
            self.graph_pan_drag = None;
            Some(drag)
        } else {
            None
        }
    }

    fn focus_panel_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        if contains_point(ram_vram_panel_area_for_screen(screen_area, self), x, y) {
            self.focused_panel = FocusedPanel::System;
            self.status = "Focus: RAM/VRAM".to_string();
            return;
        }

        if contains_point(
            system_activity_panel_area_for_screen(screen_area, self),
            x,
            y,
        ) {
            self.focused_panel = FocusedPanel::SystemActivity;
            self.status = "Focus: NW/DISK".to_string();
            return;
        }

        if contains_point(cpu_panel_area_for_screen(screen_area, self), x, y) {
            self.focused_panel = FocusedPanel::Cpu;
            self.status = "Focus: CPUs".to_string();
            return;
        }

        if contains_point(
            main_panel_areas_for_app(screen_area, self).processes.area,
            x,
            y,
        ) {
            self.focused_panel = FocusedPanel::Processes;
            self.status = "Focus: Processes".to_string();
            return;
        }

        if let Some((slot_index, _)) = graph_area_at(self, screen_area, x, y) {
            self.active_graph_slot_index = slot_index;
            self.focused_panel = FocusedPanel::DetailsGraph;
            self.status = format!("Focus: Graph#{}", slot_index + 1);
            return;
        }

        if let Some((slot_index, _)) = samples_area_at(self, screen_area, x, y) {
            self.active_graph_slot_index = slot_index;
            self.focused_panel = FocusedPanel::DetailsSamples;
            self.status = format!("Focus: Samples#{}", slot_index + 1);
        }
    }

    fn select_process_row_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        let layout = main_panel_areas_for_app(screen_area, self).processes;
        let area = layout.area;
        if !contains_point(area, x, y) {
            return;
        }

        let Some(row_index) = process_row_index_at(layout, y, self.process_table_state.offset())
        else {
            return;
        };
        if row_index < self.visible_process_count() {
            self.select_process_index(row_index);
            if let Some(column_index) = process_metric_column_index_at(
                area,
                x,
                &self.process_columns,
                self.process_metric_column_offset,
                &self.process_column_widths,
            ) {
                self.select_process_column_index(column_index);
            }
            self.clamp_process_table_state();
        }
    }

    fn select_system_metric_row_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        let area = ram_vram_panel_area_for_screen(screen_area, self);
        if !contains_point(area, x, y) {
            return;
        }
        let first_row_y = area.y.saturating_add(1);
        let last_row_y = area.bottom().saturating_sub(1);
        if y < first_row_y || y >= last_row_y {
            return;
        }
        let row = usize::from(y - first_row_y);
        if row == RAM_VRAM_SEPARATOR_ROW {
            return;
        }
        let index = if row > RAM_VRAM_SEPARATOR_ROW {
            row.saturating_sub(1)
        } else {
            row
        };
        self.select_system_metric_index(index);
    }

    fn select_system_activity_metric_row_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        let area = system_activity_panel_area_for_screen(screen_area, self);
        if !contains_point(area, x, y) {
            return;
        }
        let first_row_y = area.y.saturating_add(1);
        let last_row_y = area.bottom().saturating_sub(1);
        if y < first_row_y || y >= last_row_y {
            return;
        }
        self.select_system_activity_metric_index(usize::from(y - first_row_y));
    }

    fn scroll_at(&mut self, x: u16, y: u16, screen_area: Rect, up: bool, _shift: bool) {
        if let Some((slot_index, _)) = graph_area_at(self, screen_area, x, y) {
            self.active_graph_slot_index = slot_index;
            self.focused_panel = FocusedPanel::DetailsGraph;
            self.zoom_graph_time_span(up);
            return;
        }

        if let Some((slot_index, _)) = samples_area_at(self, screen_area, x, y) {
            self.active_graph_slot_index = slot_index;
            self.focused_panel = FocusedPanel::DetailsSamples;
            if up {
                self.select_details_sample_older(1);
            } else {
                self.select_details_sample_newer(1);
            }
            return;
        }

        if self.focused_panel == FocusedPanel::DetailsGraph && self.show_details {
            self.zoom_graph_time_span(up);
            return;
        }

        if contains_point(
            main_panel_areas_for_app(screen_area, self).processes.area,
            x,
            y,
        ) || self.focused_panel == FocusedPanel::Processes
        {
            self.focused_panel = FocusedPanel::Processes;
            if up {
                self.move_selection_up(PROCESS_WHEEL_ROWS);
            } else {
                self.move_selection_down(PROCESS_WHEEL_ROWS);
            }
        }
    }

    fn pan_graph_at(
        &mut self,
        x: u16,
        y: u16,
        screen_area: Rect,
        older: bool,
        allow_focused: bool,
    ) {
        if let Some((slot_index, _)) = graph_area_at(self, screen_area, x, y) {
            self.active_graph_slot_index = slot_index;
            self.focused_panel = FocusedPanel::DetailsGraph;
            self.shift_graph_time_window(older);
        } else if allow_focused
            && self.focused_panel == FocusedPanel::DetailsGraph
            && self.show_details
        {
            self.shift_graph_time_window(older);
        }
    }

    fn select_details_sample_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        let Some((slot_index, area)) = samples_area_at(self, screen_area, x, y) else {
            return;
        };
        let rows = details_sample_page_size_for_samples_area(
            area,
            self.active_ab_comparison().is_some(),
            self.active_graph_slot_count() <= 1,
        );
        let Some(view_state) = self.details_sample_view_state_for_slot(slot_index, rows) else {
            return;
        };
        let total = self
            .graph_slot(slot_index)
            .map(|slot| self.graph_slot_samples(slot).len())
            .unwrap_or(0);
        let Some(index) = sample_row_index_at(area, y, view_state.offset, total, rows) else {
            return;
        };
        self.active_graph_slot_index = slot_index;
        self.set_details_sample_selected(index);
    }

    fn select_details_sample_from_graph_at(&mut self, x: u16, y: u16, screen_area: Rect) {
        let Some((slot_index, area)) = graph_chart_area_at(self, screen_area, x, y) else {
            return;
        };
        self.active_graph_slot_index = slot_index;
        let plot_width = area.width.saturating_sub(1).max(1);
        let x_offset = x.saturating_sub(area.x).min(plot_width);
        let left_age = i64::from(
            self.effective_graph_time_offset_seconds()
                .saturating_add(self.effective_graph_time_span_seconds()),
        );
        let right_age = i64::from(self.effective_graph_time_offset_seconds());
        let span = (left_age - right_age).max(1);
        let age = left_age - (span * i64::from(x_offset)) / i64::from(plot_width);
        self.select_details_sample_nearest_age_seconds(age);
    }

    fn graph_plot_left_padding(&self) -> u16 {
        graph_y_axis_label_width(self).saturating_sub(1) as u16
    }
}

fn contains_point(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn process_row_index_at(layout: ProcessTableLayout, y: u16, offset: usize) -> Option<usize> {
    let area = layout.area;
    let first_row_y = area.y.saturating_add(2);
    let visible_height = u16::try_from(layout.page_size).unwrap_or(u16::MAX);
    let last_row_y = first_row_y.saturating_add(visible_height);
    (y >= first_row_y && y < last_row_y).then(|| offset + (y - first_row_y) as usize)
}

fn visible_slot_areas_for_app(app: &App, screen_area: Rect) -> Vec<(usize, Rect)> {
    let indices = app.visible_graph_slot_indices();
    let Some(details) = main_panel_areas_for_app(screen_area, app).details else {
        return Vec::new();
    };
    details_slot_areas(details, indices.len(), app.effective_graph_slot_layout())
        .into_iter()
        .zip(indices)
        .map(|(area, index)| (index, area))
        .collect()
}

fn graph_area_at(app: &App, screen_area: Rect, x: u16, y: u16) -> Option<(usize, Rect)> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .map(|(index, slot)| {
            (
                index,
                details_graph_area(slot, app.show_samples_panel, app.show_sample_delta),
            )
        })
        .find(|(_, area)| contains_point(*area, x, y))
}

fn samples_area_at(app: &App, screen_area: Rect, x: u16, y: u16) -> Option<(usize, Rect)> {
    if !app.show_samples_panel {
        return None;
    }
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .map(|(index, slot)| (index, details_samples_area(slot, app.show_sample_delta)))
        .find(|(_, area)| contains_point(*area, x, y))
}

fn graph_item_area_at(app: &App, screen_area: Rect, x: u16, y: u16) -> Option<usize> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .find_map(|(index, slot)| {
            contains_point(details_slot_title_area(slot), x, y).then_some(index)
        })
}

fn process_tracked_only_control_area_for_screen(screen_area: Rect, app: &App) -> Option<Rect> {
    let area = main_panel_areas_for_app(screen_area, app).processes.area;
    process_tracked_only_control_area(area, app)
}

fn active_samples_area_for_screen(app: &App, screen_area: Rect) -> Option<Rect> {
    if !app.show_samples_panel {
        return None;
    }
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .find(|(index, _)| *index == app.active_graph_slot_index)
        .map(|(_, slot)| details_samples_area(slot, app.show_sample_delta))
}

fn samples_scrollbar_area_for_screen(samples: Rect, total: usize, rows: usize) -> Option<Rect> {
    if total <= rows.max(1) {
        return None;
    }
    if samples.is_empty() {
        return None;
    }
    Some(Rect::new(
        samples.right().saturating_sub(1),
        samples.y,
        1,
        samples.height,
    ))
}

fn active_samples_scrollbar_area_for_screen(app: &App, screen_area: Rect) -> Option<Rect> {
    let samples = active_samples_area_for_screen(app, screen_area)?;
    samples_scrollbar_area_for_screen(
        samples,
        app.selected_sample_count(),
        app.details_sample_page_size,
    )
}

fn samples_scrollbar_area_at(
    app: &App,
    screen_area: Rect,
    x: u16,
    y: u16,
) -> Option<(usize, Rect)> {
    if !app.show_samples_panel {
        return None;
    }
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .find_map(|(index, slot)| {
            let samples = details_samples_area(slot, app.show_sample_delta);
            let rows = details_sample_page_size_for_samples_area(
                samples,
                app.active_ab_comparison().is_some(),
                app.active_graph_slot_count() <= 1,
            );
            let total = app
                .graph_slot(index)
                .map(|slot| app.graph_slot_samples(slot).len())
                .unwrap_or(0);
            let scrollbar = samples_scrollbar_area_for_screen(samples, total, rows)?;
            contains_point(scrollbar, x, y).then_some((index, scrollbar))
        })
}

fn graph_chart_area_at(app: &App, screen_area: Rect, x: u16, y: u16) -> Option<(usize, Rect)> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .filter_map(|(index, slot)| {
            let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
            let area = details_graph_chart_area(graph, app.graph_plot_left_padding())?;
            contains_point(area, x, y).then_some((index, area))
        })
        .next()
}

fn active_graph_chart_area_for_screen(app: &App, screen_area: Rect) -> Option<Rect> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .find_map(|(index, slot)| {
            (index == app.active_graph_slot_index).then(|| {
                let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
                details_graph_chart_area(graph, app.graph_plot_left_padding())
            })?
        })
}

fn graph_shared_control_areas_for_app(
    app: &App,
    screen_area: Rect,
) -> crate::ui::layout::GraphSharedControlAreas {
    let controls = main_panel_areas_for_app(screen_area, app)
        .details
        .map(details_shared_controls_area)
        .unwrap_or_default();
    graph_shared_control_areas(controls, app.show_samples_panel)
}

fn details_sample_page_size_for_samples_area(
    samples: Rect,
    show_ab_summary: bool,
    show_base_summary: bool,
) -> usize {
    crate::ui::layout::details_samples_row_capacity(
        samples.height,
        show_ab_summary,
        show_base_summary,
    )
}

fn help_close_button_area_for_screen(screen_area: Rect) -> Option<Rect> {
    help_close_button_area(help_area(screen_area))
}

fn sample_row_index_at(
    area: Rect,
    y: u16,
    offset: usize,
    total: usize,
    rows: usize,
) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let first_row_y = area.y.saturating_add(1);
    let last_row_y = first_row_y.saturating_add(rows as u16).min(area.bottom());
    if y < first_row_y || y >= last_row_y {
        return None;
    }
    let start = offset.min(total.saturating_sub(rows.min(total)));
    let index = start + usize::from(y - first_row_y);
    (index < total).then_some(index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SamplesScrollbarThumb {
    start: usize,
    len: usize,
}

fn samples_scrollbar_track_len(area: Rect) -> Option<usize> {
    let track_len = area.height.saturating_sub(2);
    (track_len > 0).then_some(usize::from(track_len))
}

fn samples_scrollbar_track_position(area: Rect, y: u16) -> Option<usize> {
    let track_len = samples_scrollbar_track_len(area)?;
    let track_end = area.y.saturating_add(area.height).saturating_sub(2);
    if y <= area.y {
        return Some(0);
    }
    if y >= track_end {
        return Some(track_len.saturating_sub(1));
    }
    Some(usize::from(y - area.y - 1).min(track_len.saturating_sub(1)))
}

fn samples_scrollbar_thumb(
    total: usize,
    rows: usize,
    offset: usize,
    track_len: usize,
) -> Option<SamplesScrollbarThumb> {
    if total == 0 {
        return None;
    }
    let rows = rows.max(1).min(total);
    if total <= rows || track_len == 0 {
        return None;
    }

    let max_offset = total.saturating_sub(rows);
    if max_offset == 0 {
        return None;
    }
    let thumb_len = ((rows * track_len + total / 2) / total)
        .max(1)
        .min(track_len);
    let max_thumb_start = track_len.saturating_sub(thumb_len);
    let thumb_start = ((offset.min(max_offset) * max_thumb_start + max_offset / 2) / max_offset)
        .min(max_thumb_start);
    Some(SamplesScrollbarThumb {
        start: thumb_start,
        len: thumb_len,
    })
}

fn samples_scrollbar_grab_offset_at(
    area: Rect,
    y: u16,
    total: usize,
    rows: usize,
    offset: usize,
) -> Option<usize> {
    let track_len = samples_scrollbar_track_len(area)?;
    let position = samples_scrollbar_track_position(area, y)?;
    let thumb = samples_scrollbar_thumb(total, rows, offset, track_len)?;
    let thumb_end = thumb.start.saturating_add(thumb.len);
    if position >= thumb.start && position < thumb_end {
        Some(position - thumb.start)
    } else {
        Some(thumb.len / 2)
    }
}

fn samples_scrollbar_offset_at(
    area: Rect,
    y: u16,
    total: usize,
    rows: usize,
    grab_offset: usize,
) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let rows = rows.max(1).min(total);
    if total <= rows {
        return None;
    }

    let track_len = samples_scrollbar_track_len(area)?;
    let position = samples_scrollbar_track_position(area, y)?;
    let max_offset = total.saturating_sub(rows);
    let thumb_len = ((rows * track_len + total / 2) / total)
        .max(1)
        .min(track_len);
    let max_thumb_start = track_len.saturating_sub(thumb_len);
    if max_thumb_start == 0 {
        return Some(0);
    }
    let thumb_start = position.saturating_sub(grab_offset);
    Some(
        ((thumb_start.min(max_thumb_start) * max_offset + max_thumb_start / 2) / max_thumb_start)
            .min(max_offset),
    )
}

fn terminal_zoom_direction(mouse: &MouseEvent) -> Option<bool> {
    if !mouse.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(true),
        MouseEventKind::ScrollDown => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_row_index_uses_table_header_and_offset() {
        let area = Rect::new(0, 10, 80, 13);
        let without_total = ProcessTableLayout {
            area,
            page_size: 10,
            show_tracked_total: false,
        };
        let with_total = ProcessTableLayout {
            area,
            page_size: 9,
            show_tracked_total: true,
        };

        assert_eq!(process_row_index_at(without_total, 12, 5), Some(5));
        assert_eq!(process_row_index_at(without_total, 15, 5), Some(8));
        assert_eq!(process_row_index_at(without_total, 11, 5), None);
        assert_eq!(process_row_index_at(without_total, 22, 5), None);
        assert_eq!(process_row_index_at(with_total, 21, 5), None);
    }

    #[test]
    fn process_wheel_moves_one_row_per_notch() {
        assert_eq!(PROCESS_WHEEL_ROWS, 1);
    }

    #[test]
    fn ctrl_wheel_maps_to_terminal_zoom_direction() {
        assert_eq!(
            terminal_zoom_direction(&MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::CONTROL,
            }),
            Some(true)
        );
        assert_eq!(
            terminal_zoom_direction(&MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::CONTROL,
            }),
            Some(false)
        );
        assert_eq!(
            terminal_zoom_direction(&MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }),
            None
        );
    }

    #[test]
    fn samples_scrollbar_offset_maps_track_to_offsets() {
        let area = Rect::new(10, 5, 1, 11);

        assert_eq!(samples_scrollbar_offset_at(area, 5, 100, 10, 0), Some(0));
        assert_eq!(samples_scrollbar_offset_at(area, 10, 100, 10, 0), Some(45));
        assert_eq!(samples_scrollbar_offset_at(area, 15, 100, 10, 0), Some(90));
        assert_eq!(samples_scrollbar_offset_at(area, 20, 100, 10, 0), Some(90));
    }

    #[test]
    fn samples_scrollbar_thumb_reaches_bottom_at_last_offset() {
        let area = Rect::new(10, 5, 1, 11);
        let track_len = samples_scrollbar_track_len(area).unwrap();
        let thumb = samples_scrollbar_thumb(100, 10, 90, track_len).unwrap();

        assert_eq!(thumb.start + thumb.len, track_len);
    }

    #[test]
    fn samples_scrollbar_grab_offset_keeps_cursor_inside_thumb() {
        let area = Rect::new(10, 5, 1, 32);
        let track_len = samples_scrollbar_track_len(area).unwrap();
        let thumb = samples_scrollbar_thumb(100, 20, 40, track_len).unwrap();
        let cursor_y = area.y + 1 + thumb.start as u16 + 2;

        assert_eq!(
            samples_scrollbar_grab_offset_at(area, cursor_y, 100, 20, 40),
            Some(2)
        );
        assert_eq!(
            samples_scrollbar_offset_at(area, cursor_y, 100, 20, 2),
            Some(40)
        );
    }
}
