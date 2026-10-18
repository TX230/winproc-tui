use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::{App, FocusedPanel, ProcessInfoTab};
use crate::ui::help;

impl App {
    pub(crate) fn contextual_help_title(&self) -> &'static str {
        if self.log_open_error.is_some() {
            return "Global";
        }
        if self.show_process_info_dialog {
            return match self.process_info_tab {
                ProcessInfoTab::Network => "Network endpoints",
                ProcessInfoTab::Scheduling => "Scheduling",
                _ => "Process Info",
            };
        }
        if self.show_column_picker || self.show_process_kill_confirmation {
            return "Processes";
        }
        if self.investigation_profiles_dialog.is_some()
            || self.show_tracked_remove_confirmation
            || self.show_recording_tracking_fixed
        {
            return "Tracking";
        }
        if self.graph_reorder_dialog.is_some() || self.graph_csv_dialog.is_some() {
            return "Graph Workspace";
        }
        if self.show_cpu_core_dialog {
            return "CPU";
        }
        if self.is_main_menu_open() || self.has_workspace_overlay() {
            return "Global";
        }
        if self.network_browser.visible {
            return "Network endpoints";
        }
        if self.file_users.visible {
            return "Find processes by file";
        }
        match self.focused_panel {
            FocusedPanel::System => "MEM/GPU",
            FocusedPanel::SystemActivity => "NW/DISK",
            FocusedPanel::Cpu => "CPU",
            FocusedPanel::Processes => "Processes",
            FocusedPanel::DetailsGraph => "Graph Workspace",
            FocusedPanel::DetailsSamples => "Samples",
        }
    }

    pub(crate) fn jump_help_section(&mut self, index: usize) {
        self.help_section = index.min(help::section_titles().len().saturating_sub(1));
        self.help_section_picker = None;
        self.shortcut_hovered = None;
        self.help_scroll.reset();
        self.set_help_page_size(crate::ui::help_page_size_for_screen(self.last_screen_area));
        self.help_scroll.offset = help::section_offset(self.help_section, self.last_screen_area);
        let total = self.help_scroll_total();
        self.help_scroll
            .set_page_size(self.help_scroll.page_size, total);
    }

    pub(crate) fn help_key(&mut self, key: KeyEvent) {
        if self.task_guide.is_some() {
            self.task_guide_key(key);
            return;
        }
        if self.help_version_info {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                self.close_help();
            } else if matches!(key.code, KeyCode::F(1) | KeyCode::Char('?')) {
                self.open_help();
            }
            return;
        }
        if let Some(selected) = self.help_section_picker {
            let last = help::section_titles().len().saturating_sub(1);
            match key.code {
                KeyCode::Esc => self.help_section_picker = None,
                KeyCode::F(1) | KeyCode::Char('?') => self.close_help(),
                KeyCode::Enter => self.jump_help_section(selected),
                KeyCode::Up => self.help_section_picker = Some(selected.saturating_sub(1)),
                KeyCode::Down => self.help_section_picker = Some((selected + 1).min(last)),
                KeyCode::Home => self.help_section_picker = Some(0),
                KeyCode::End => self.help_section_picker = Some(last),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::F(1) | KeyCode::Char('?') => self.close_help(),
            KeyCode::Char('t' | 'T') => self.open_task_guides(),
            KeyCode::Char('s' | 'S') => {
                self.help_scroll.stop_drag();
                self.help_section_picker = Some(self.help_section);
            }
            KeyCode::Left => self.jump_help_section(self.help_section.saturating_sub(1)),
            KeyCode::Right => self.jump_help_section(self.help_section + 1),
            KeyCode::Up => self.scroll_help_up(1),
            KeyCode::Down => self.scroll_help_down(1),
            KeyCode::PageUp => self.scroll_help_up(self.help_scroll.page_size),
            KeyCode::PageDown => self.scroll_help_down(self.help_scroll.page_size),
            KeyCode::Home => self.scroll_help_home(),
            KeyCode::End => self.scroll_help_end(),
            _ => {}
        }
    }

    pub(crate) fn help_section_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        let selected = self.help_section_picker.unwrap_or(self.help_section);
        let hit = help::section_picker_row_at(screen, selected, mouse.column, mouse.row);
        self.shortcut_hovered = hit.map(|(_, area)| area);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((index, _)) = hit {
                    self.jump_help_section(index);
                } else if !help::section_picker_area(screen)
                    .contains((mouse.column, mouse.row).into())
                {
                    self.help_section_picker = None;
                }
            }
            MouseEventKind::ScrollUp => self.help_key(KeyCode::Up.into()),
            MouseEventKind::ScrollDown => self.help_key(KeyCode::Down.into()),
            _ => {}
        }
    }
}

impl App {
    pub(crate) fn open_task_guides(&mut self) {
        self.task_guide = Some((0, false));
        self.task_scroll.reset();
        self.shortcut_hovered = None;
    }

    fn task_guide_key(&mut self, key: KeyEvent) {
        let (index, open) = self.task_guide.unwrap();
        let modal = help::tasks::modal(self.last_screen_area, index, open);
        self.task_scroll.set_page_size(
            modal.page_size(self.last_screen_area),
            modal.content_height as usize,
        );
        match key.code {
            KeyCode::F(1) | KeyCode::Char('?') => self.close_help(),
            KeyCode::Esc if open => {
                self.task_guide = Some((index, false));
                self.task_scroll.reset();
            }
            KeyCode::Esc => self.close_help(),
            KeyCode::Char('r' | 'R') => {
                self.task_guide = None;
                self.shortcut_hovered = None;
            }
            KeyCode::Enter if !open => {
                self.task_guide = Some((index, true));
                self.task_scroll.reset();
            }
            KeyCode::Up if !open => self.task_guide = Some((index.saturating_sub(1), false)),
            KeyCode::Down if !open => self.task_guide = Some(((index + 1).min(3), false)),
            KeyCode::Home if !open => self.task_guide = Some((0, false)),
            KeyCode::End if !open => self.task_guide = Some((3, false)),
            KeyCode::Up => self.task_scroll.scroll_up(1),
            KeyCode::Down => self
                .task_scroll
                .scroll_down(1, modal.content_height as usize),
            KeyCode::PageUp => self.task_scroll.scroll_up(self.task_scroll.page_size),
            KeyCode::PageDown => self
                .task_scroll
                .scroll_down(self.task_scroll.page_size, modal.content_height as usize),
            KeyCode::Home => self.task_scroll.scroll_home(),
            KeyCode::End => self.task_scroll.scroll_end(modal.content_height as usize),
            _ => {}
        }
    }

    pub(crate) fn task_guide_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        let (index, open) = self.task_guide.unwrap();
        let modal = help::tasks::modal(screen, index, open);
        self.task_scroll
            .set_page_size(modal.page_size(screen), modal.content_height as usize);
        if !open {
            self.shortcut_hovered =
                help::tasks::row_at(screen, mouse.column, mouse.row).map(|_| {
                    let content = modal.layout(screen).content;
                    Rect::new(content.x, mouse.row, content.width, 1)
                });
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !open => {
                if let Some(row) = help::tasks::row_at(screen, mouse.column, mouse.row) {
                    self.task_guide = Some((row, true));
                    self.task_scroll.reset();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(bar) = modal.scrollbar_area(screen, self.task_scroll.page_size)
                    && bar.contains((mouse.column, mouse.row).into())
                {
                    self.task_scroll
                        .start_drag(bar, mouse.row, modal.content_height as usize);
                    self.task_scroll
                        .drag_to(bar, mouse.row, modal.content_height as usize);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.task_scroll.dragging => {
                if let Some(bar) = modal.scrollbar_area(screen, self.task_scroll.page_size) {
                    self.task_scroll
                        .drag_to(bar, mouse.row, modal.content_height as usize);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.task_scroll.stop_drag(),
            MouseEventKind::ScrollUp => self.task_guide_key(KeyCode::Up.into()),
            MouseEventKind::ScrollDown => self.task_guide_key(KeyCode::Down.into()),
            _ => {}
        }
    }
}
