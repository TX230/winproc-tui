use std::time::Instant;

use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::{
    app::{
        App, AppActivity, FocusedPanel, GraphPanDrag, GraphPanDragButton, QuitConfirmSelection,
        RecordingOverwriteSelection, TrackedRemoveSelection,
    },
    ui::{
        GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, THEMES,
        column_picker_close_button_area_for_screen, column_picker_index_at,
        column_picker_scrollbar_area, details_slot_areas_for_screen,
        display_area_warning_ok_button_area,
        format::format_integer,
        help_area, help_close_button_area, help_scrollbar_area,
        layout::{details_graph_area, details_samples_area},
        log_dir_button_at, log_list_index_at, metric_column_warning_ok_button_area,
        no_graph_metrics_warning_ok_button_area, open_files_close_button_area_for_screen,
        process_metric_column_index_at, process_table_area_for_screen, process_table_page_size,
        process_tracked_only_checkbox_area, quit_confirm_button_at, ram_vram_panel_area_for_screen,
        recording_no_tracked_ok_button_area, recording_overwrite_button_at,
        recording_path_button_at, settings_ok_button_area, settings_selection_at,
        tracked_remove_button_at,
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

        if self.show_settings_dialog {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.close_settings_dialog(),
                KeyCode::Up => self.select_previous_setting(),
                KeyCode::Down | KeyCode::Tab => self.select_next_setting(),
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                    self.toggle_selected_setting();
                }
                _ => {}
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

        if self.show_recording_path_dialog {
            match key.code {
                KeyCode::Esc => self.cancel_recording_path_dialog(),
                KeyCode::Enter => self.activate_recording_path_selection()?,
                KeyCode::Backspace => self.pop_recording_path_char(),
                KeyCode::Delete => self.delete_recording_path_char(),
                KeyCode::Left => self.move_recording_path_cursor_left(),
                KeyCode::Right => self.move_recording_path_cursor_right(),
                KeyCode::Tab => self.complete_recording_path(),
                KeyCode::Home => self.move_recording_path_cursor_home(),
                KeyCode::End => self.move_recording_path_cursor_end(),
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
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
                KeyCode::Backspace => self.pop_log_dir_char(),
                KeyCode::Delete => self.delete_log_dir_char(),
                KeyCode::Left => self.move_log_dir_cursor_left(),
                KeyCode::Right => self.move_log_dir_cursor_right(),
                KeyCode::Tab => self.complete_log_dir(),
                KeyCode::Home => self.move_log_dir_cursor_home(),
                KeyCode::End => self.move_log_dir_cursor_end(),
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
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
                KeyCode::Enter => self.load_selected_log(),
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

        if self.show_open_files {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.close_open_files(),
                KeyCode::Up => self.scroll_open_files_up(1),
                KeyCode::Down => self.scroll_open_files_down(1),
                KeyCode::PageUp => self.scroll_open_files_up(self.open_files_scroll.page_size),
                KeyCode::PageDown => self.scroll_open_files_down(self.open_files_scroll.page_size),
                KeyCode::Home => self.scroll_open_files_home(),
                KeyCode::End => self.scroll_open_files_end(),
                KeyCode::Left => self.move_open_files_filter_cursor_left(),
                KeyCode::Right => self.move_open_files_filter_cursor_right(),
                KeyCode::Backspace => self.pop_open_files_filter_char(),
                KeyCode::Delete => self.delete_open_files_filter_char(),
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'c')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.copy_open_files_to_clipboard()?;
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'u')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.refresh_open_files()?;
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.push_open_files_filter_char(ch);
                }
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
                KeyCode::PageUp => {
                    self.zoom_graph_time_span(true);
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.zoom_graph_time_span(false);
                    return Ok(());
                }
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

        match key.code {
            KeyCode::Esc if self.activity() == AppActivity::Playback => {
                self.exit_playback();
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
                    self.select_previous_process_column();
                }
            }
            KeyCode::Right => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.select_next_process_column();
                }
            }
            KeyCode::Up => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.move_selection_up(1);
                }
            }
            KeyCode::Down => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.move_selection_down(1);
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
                    self.status = "Use 1-4 to show selected metric in a graph".to_string();
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
                    if !self.clear_selected_graph_metric() {
                        self.hide_selected_ghost_row();
                    }
                }
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'c')
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.open_column_picker();
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
            KeyCode::Char(' ') => {
                if self.focused_panel == FocusedPanel::Processes {
                    self.toggle_selected_process_tracking();
                }
            }
            KeyCode::Char('t') if self.focused_panel == FocusedPanel::Processes => {
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
                self.toggle_info_panel_mode();
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
                if ch.eq_ignore_ascii_case(&'l')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.open_log_list()?;
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'o')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.open_settings_dialog();
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
                match log_dir_button_at(screen_area, mouse.column, mouse.row) {
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
                    if let Some(index) = log_list_index_at(
                        screen_area,
                        mouse.column,
                        mouse.row,
                        self.log_list_scroll.offset,
                        self.log_summaries.len(),
                    ) {
                        self.click_log_list_index(index, Instant::now());
                    }
                }
                MouseEventKind::ScrollUp => self.scroll_log_list_up(1),
                MouseEventKind::ScrollDown => self.scroll_log_list_down(1),
                _ => {}
            }
            return;
        }

        if self.show_open_files {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if open_files_close_button_area_for_screen(screen_area, self)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row)) =>
                {
                    self.close_open_files();
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.start_open_files_scrollbar_drag(mouse.column, mouse.row, screen_area);
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.open_files_scroll.stop_drag();
                }
                MouseEventKind::Drag(MouseButton::Left) if self.open_files_scroll.dragging => {
                    self.drag_open_files_scrollbar(mouse.row, screen_area);
                }
                MouseEventKind::ScrollUp => self.scroll_open_files_up(1),
                MouseEventKind::ScrollDown => self.scroll_open_files_down(1),
                _ => {}
            }
            return;
        }

        if self.show_settings_dialog {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if settings_ok_button_area(screen_area)
                        .is_some_and(|area| contains_point(area, mouse.column, mouse.row)) =>
                {
                    self.close_settings_dialog();
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(selection) =
                        settings_selection_at(screen_area, mouse.column, mouse.row)
                    {
                        self.settings_selection = selection;
                        self.toggle_selected_setting();
                    }
                }
                _ => {}
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

        if self.show_recording_path_dialog {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match recording_path_button_at(screen_area, mouse.column, mouse.row) {
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
                            self.focused_panel = FocusedPanel::System;
                        }
                    }
                    return;
                }
                if process_tracked_only_checkbox_area_for_screen(screen_area, self)
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
                self.focus_panel_at(mouse.column, mouse.row, screen_area);
                self.select_system_metric_row_at(mouse.column, mouse.row, screen_area);
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
        let Some((slot_index, area)) = graph_y_axis_toggle_area_at(self, screen_area, x, y) else {
            return false;
        };
        let _ = area;
        self.active_graph_slot_index = slot_index;
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_graph_y_axis_zero_min();
        true
    }

    fn toggle_graph_all_samples_at(&mut self, x: u16, y: u16, screen_area: Rect) -> bool {
        let Some((slot_index, area)) = graph_all_samples_toggle_area_at(self, screen_area, x, y)
        else {
            return false;
        };
        let _ = area;
        self.active_graph_slot_index = slot_index;
        self.focused_panel = FocusedPanel::DetailsGraph;
        self.toggle_graph_all_samples();
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
            process_table_area_for_screen(screen_area, self.show_details),
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
        let area = process_table_area_for_screen(screen_area, self.show_details);
        if !contains_point(area, x, y) {
            return;
        }

        let Some(row_index) = process_row_index_at(
            area,
            y,
            self.process_table_state.offset(),
            self.has_visible_tracked_total_row(),
        ) else {
            return;
        };
        if row_index < self.visible_process_count() {
            self.select_process_index(row_index);
            if let Some(column_index) = process_metric_column_index_at(
                area,
                x,
                &self.process_columns,
                self.process_metric_column_offset,
                self.process_table_state.selected().is_some(),
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
            process_table_area_for_screen(screen_area, self.show_details),
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
        self.active_graph_slot_index = slot_index;
        let Some(index) = sample_row_index_at(
            area,
            y,
            self.details_sample_offset,
            self.selected_sample_count(),
        ) else {
            return;
        };
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
        let max_value = self
            .graph_slots
            .iter()
            .filter_map(Option::as_ref)
            .flat_map(|slot| {
                self.graph_slot_samples(slot)
                    .into_iter()
                    .filter_map(|sample| sample.value.map(|value| value.round().max(0.0) as u64))
            })
            .max()
            .unwrap_or(0);
        let label_width = format_integer(nice_axis_max(max_value)).chars().count();
        label_width.max(1) as u16
    }
}

fn contains_point(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn process_row_index_at(
    area: Rect,
    y: u16,
    offset: usize,
    has_fixed_total_row: bool,
) -> Option<usize> {
    let first_row_y = area.y.saturating_add(2);
    let reserves_total_row = has_fixed_total_row && process_table_page_size(area) > 1;
    let bottom_margin = 1 + u16::from(reserves_total_row);
    let last_row_y = area.bottom().saturating_sub(bottom_margin);
    (y >= first_row_y && y < last_row_y).then(|| offset + (y - first_row_y) as usize)
}

fn visible_slot_areas_for_app(app: &App, screen_area: Rect) -> Vec<(usize, Rect)> {
    let indices = app.visible_graph_slot_indices();
    details_slot_areas_for_screen(screen_area, app.show_details, indices.len())
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
            let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
            let inner = shrink_rect(graph, 1);
            let reserved = GRAPH_ALL_SAMPLES_TOGGLE_WIDTH.saturating_add(GRAPH_Y_AXIS_TOGGLE_WIDTH);
            let item_width = inner.width.saturating_sub(reserved.min(inner.width));
            let item = Rect::new(inner.x, inner.y, item_width, 1);
            contains_point(item, x, y).then_some(index)
        })
}

fn process_tracked_only_checkbox_area_for_screen(screen_area: Rect, app: &App) -> Option<Rect> {
    let area = process_table_area_for_screen(screen_area, app.show_details);
    process_tracked_only_checkbox_area(area, app)
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
    let inner = shrink_rect(samples, 1);
    if inner.is_empty() {
        return None;
    }
    Some(Rect::new(
        inner.right().saturating_sub(1),
        inner.y,
        1,
        inner.height,
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

fn graph_chart_area_for_graph(graph: Rect, left_padding: u16) -> Option<Rect> {
    let inner = shrink_rect(graph, 1);
    let x_padding = left_padding.min(inner.width.saturating_sub(1));
    Some(Rect::new(
        inner.x.saturating_add(x_padding),
        inner.y.saturating_add(3),
        inner.width.saturating_sub(x_padding),
        inner.height.saturating_sub(4),
    ))
}

fn graph_chart_area_at(app: &App, screen_area: Rect, x: u16, y: u16) -> Option<(usize, Rect)> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .filter_map(|(index, slot)| {
            let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
            let area = graph_chart_area_for_graph(graph, app.graph_plot_left_padding())?;
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
                graph_chart_area_for_graph(graph, app.graph_plot_left_padding())
            })?
        })
}

fn graph_y_axis_toggle_area_for_graph(graph: Rect) -> Option<Rect> {
    let inner = shrink_rect(graph, 1);
    if inner.width < GRAPH_Y_AXIS_TOGGLE_WIDTH {
        return None;
    }
    Some(Rect::new(
        inner.right().saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH),
        inner.y,
        GRAPH_Y_AXIS_TOGGLE_WIDTH,
        1,
    ))
}

fn graph_y_axis_toggle_area_at(
    app: &App,
    screen_area: Rect,
    x: u16,
    y: u16,
) -> Option<(usize, Rect)> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .filter_map(|(index, slot)| {
            let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
            let area = graph_y_axis_toggle_area_for_graph(graph)?;
            contains_point(area, x, y).then_some((index, area))
        })
        .next()
}

fn graph_all_samples_toggle_area_for_graph(graph: Rect) -> Option<Rect> {
    let inner = shrink_rect(graph, 1);
    let required = GRAPH_ALL_SAMPLES_TOGGLE_WIDTH.saturating_add(GRAPH_Y_AXIS_TOGGLE_WIDTH);
    if inner.width < required {
        return None;
    }
    Some(Rect::new(
        inner.right().saturating_sub(required),
        inner.y,
        GRAPH_ALL_SAMPLES_TOGGLE_WIDTH,
        1,
    ))
}

fn graph_all_samples_toggle_area_at(
    app: &App,
    screen_area: Rect,
    x: u16,
    y: u16,
) -> Option<(usize, Rect)> {
    visible_slot_areas_for_app(app, screen_area)
        .into_iter()
        .filter_map(|(index, slot)| {
            let graph = details_graph_area(slot, app.show_samples_panel, app.show_sample_delta);
            let area = graph_all_samples_toggle_area_for_graph(graph)?;
            contains_point(area, x, y).then_some((index, area))
        })
        .next()
}

fn details_sample_page_size_for_samples_area(
    samples: Rect,
    show_ab_summary: bool,
    show_base_summary: bool,
) -> usize {
    let inner = samples.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    crate::ui::layout::details_samples_row_capacity(
        inner.height,
        show_ab_summary,
        show_base_summary,
    )
}

fn help_close_button_area_for_screen(screen_area: Rect) -> Option<Rect> {
    help_close_button_area(help_area(screen_area))
}

fn nice_axis_max(value: u64) -> u64 {
    if value <= 10 {
        return value.max(1);
    }
    let digits = value.ilog10() + 1;
    let step = 10_u64.pow(digits.saturating_sub(2));
    value.div_ceil(step) * step
}

fn shrink_rect(area: Rect, margin: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(margin),
        area.y.saturating_add(margin),
        area.width.saturating_sub(margin.saturating_mul(2)),
        area.height.saturating_sub(margin.saturating_mul(2)),
    )
}

fn sample_row_index_at(area: Rect, y: u16, offset: usize, total: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let inner = shrink_rect(area, 1);
    let first_row_y = inner.y.saturating_add(1);
    if y < first_row_y || y >= inner.bottom() {
        return None;
    }
    let rows = inner.height.saturating_sub(3).max(1) as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_row_index_uses_table_header_and_offset() {
        let area = Rect::new(0, 10, 80, 13);

        assert_eq!(process_row_index_at(area, 12, 5, false), Some(5));
        assert_eq!(process_row_index_at(area, 15, 5, false), Some(8));
        assert_eq!(process_row_index_at(area, 11, 5, false), None);
        assert_eq!(process_row_index_at(area, 22, 5, false), None);
        assert_eq!(process_row_index_at(area, 21, 5, true), None);
    }

    #[test]
    fn process_wheel_moves_one_row_per_notch() {
        assert_eq!(PROCESS_WHEEL_ROWS, 1);
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
