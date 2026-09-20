use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};

use super::{App, AppActivity, ProcessInfoFocus};
use crate::ui::open_files;

#[derive(Default)]
pub(crate) struct OpenFilesRefresh {
    pub(crate) next_due: Option<Instant>,
    pub(crate) received_at: Option<chrono::DateTime<chrono::Local>>,
    pub(crate) interval: Option<Duration>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) automatic: bool,
}

impl OpenFilesRefresh {
    pub(crate) fn completed(&mut self, elapsed: Duration, success: bool, now: Instant) {
        self.elapsed = Some(elapsed);
        self.received_at = Some(chrono::Local::now());
        // Leave at least ten times the collection cost idle. Expensive or failed collections
        // require a manual retry; a successful, inexpensive retry resumes automatic refresh.
        self.interval = (success && elapsed <= Duration::from_secs(1)).then(|| {
            Duration::from_secs((elapsed.as_nanos() * 10).div_ceil(1_000_000_000).max(2) as u64)
        });
        self.next_due = self.interval.map(|interval| now + interval);
    }
}

impl App {
    pub(crate) fn request_due_open_files_at(&mut self, now: Instant) -> Result<bool> {
        if !self.show_process_info_dialog
            || self.process_info_tab != super::ProcessInfoTab::Files
            || self.activity() == AppActivity::LogView
            || self.open_files_in_flight.is_some()
            || self.open_files_refresh.next_due.is_none_or(|due| now < due)
            || !self.process_info_target_is_currently_live()
        {
            return Ok(false);
        }
        self.request_open_files_for_target(false, None)?;
        Ok(true)
    }

    pub(crate) fn open_files_refresh_label(&self) -> String {
        if !self.process_info_target_is_currently_live() {
            return "Process exited".to_string();
        }
        match (
            self.open_files_refresh.interval,
            self.open_files_refresh.elapsed,
        ) {
            (Some(interval), Some(elapsed)) => {
                format!("Auto {}s · {} ms", interval.as_secs(), elapsed.as_millis())
            }
            (None, Some(elapsed)) if elapsed > Duration::from_secs(1) => {
                format!("Auto off (slow: {} ms)", elapsed.as_millis())
            }
            (None, Some(_)) => "Auto off (error)".to_string(),
            _ => "Loading...".to_string(),
        }
    }

    pub(crate) fn reset_open_file_selection(&mut self) {
        self.open_files_selected = 0;
        self.open_files_show_detail = false;
        self.open_files_scroll.scroll_home();
    }

    pub(crate) fn select_open_file(&mut self, index: usize) {
        let count = open_files::filtered_entries(self).len();
        self.open_files_selected = index.min(count.saturating_sub(1));
        self.ensure_open_file_visible();
    }

    pub(crate) fn ensure_open_file_visible(&mut self) {
        if self.open_files_show_detail {
            return;
        }
        let area = crate::ui::process_info_content_area_for_screen(self.last_screen_area);
        let stride = open_files::entry_row_height(area.width.saturating_sub(1) as usize);
        let line = self.open_files_selected * stride;
        let total = self.open_files_total_rows();
        self.open_files_scroll.ensure_visible(line, total);
        self.open_files_scroll
            .ensure_visible(line + stride.saturating_sub(1), total);
        if self.open_files_selected == 0 {
            self.open_files_scroll.scroll_home();
        }
    }

    pub(crate) fn on_open_files_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            if key.code == KeyCode::Esc {
                self.close_process_info_dialog();
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc if self.close_process_info_detail() => {}
            KeyCode::Esc => self.close_process_info_dialog(),
            KeyCode::Enter if self.open_files_show_detail => {
                self.close_process_info_detail();
            }
            KeyCode::Enter => {
                self.open_selected_process_info_detail();
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'u')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.refresh_open_files()?
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.copy_open_files_to_clipboard()?
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
                let amount = if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                    self.process_info_page_size()
                } else {
                    1
                };
                if self.open_files_show_detail {
                    match key.code {
                        KeyCode::Up | KeyCode::PageUp => self.scroll_process_info_up(amount),
                        KeyCode::Down | KeyCode::PageDown => self.scroll_process_info_down(amount),
                        KeyCode::Home => self.scroll_process_info_home(),
                        _ => self.scroll_process_info_end(),
                    }
                } else {
                    let area =
                        crate::ui::process_info_content_area_for_screen(self.last_screen_area);
                    let step = if amount > 1 {
                        (amount
                            / open_files::entry_row_height(area.width.saturating_sub(1) as usize))
                        .max(1)
                    } else {
                        1
                    };
                    let next = match key.code {
                        KeyCode::Up | KeyCode::PageUp => {
                            self.open_files_selected.saturating_sub(step)
                        }
                        KeyCode::Down | KeyCode::PageDown => {
                            self.open_files_selected.saturating_add(step)
                        }
                        KeyCode::Home => 0,
                        _ => usize::MAX,
                    };
                    self.select_open_file(next);
                }
            }
            _ if self.open_files_show_detail => {}
            KeyCode::Left => self.move_open_files_filter_cursor_left(),
            KeyCode::Right => self.move_open_files_filter_cursor_right(),
            KeyCode::Backspace => self.pop_open_files_filter_char(),
            KeyCode::Delete => self.delete_open_files_filter_char(),
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !ch.is_control() =>
            {
                self.push_open_files_filter_char(ch)
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn on_open_files_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        let area = crate::ui::process_info_content_area_for_screen(screen);
        if self.activity() == AppActivity::LogView {
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if area.contains((mouse.column, mouse.row).into()) =>
            {
                self.process_info_focus = ProcessInfoFocus::Content;
                if !self.start_process_info_scrollbar_drag(mouse.column, mouse.row, screen) {
                    if let Some(index) = open_files::index_at(area, self, mouse.column, mouse.row) {
                        self.select_open_file(index);
                    }
                } else {
                    self.select_open_file_at_scroll_offset(area);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.stop_process_info_scrollbar_drag(),
            MouseEventKind::Drag(MouseButton::Left) if self.process_info_scrollbar_dragging() => {
                self.drag_process_info_scrollbar(mouse.row, screen);
                self.select_open_file_at_scroll_offset(area);
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if area.contains((mouse.column, mouse.row).into()) =>
            {
                self.process_info_focus = ProcessInfoFocus::Content;
                let key = if mouse.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                let _ = self.on_open_files_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            _ => {}
        }
    }

    fn select_open_file_at_scroll_offset(&mut self, area: Rect) {
        if !self.open_files_show_detail {
            let stride = open_files::entry_row_height(area.width.saturating_sub(1) as usize);
            let index = self.open_files_scroll.offset.div_ceil(stride);
            self.open_files_selected =
                index.min(open_files::filtered_entries(self).len().saturating_sub(1));
        }
    }
}
