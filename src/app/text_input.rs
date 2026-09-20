use crate::app::{App, ProcessInfoFocus, ProcessInfoTab};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) const FILTER_SHORTCUTS: &[(&str, &str)] = &[
    ("Enter", "Apply"),
    ("Esc", "Clear"),
    ("Ctrl+U", "Clear text"),
    ("←/→", "Cursor"),
    ("Home/End", "Edge"),
    ("Backspace/Del", "Erase"),
];

pub(crate) const INSPECTION_FILTER_SHORTCUTS: &[(&str, &str)] = &[
    ("Enter", "Apply"),
    ("Esc", "tabs"),
    ("Ctrl+←/→", "tabs"),
    ("Ctrl+U", "Clear text"),
    ("←/→", "Cursor"),
    ("Home/End", "Edge"),
    ("Backspace/Del", "Erase"),
];

pub(crate) fn edit(text: &mut String, cursor: &mut usize, key: KeyEvent) -> bool {
    *cursor = (*cursor).min(text.len());
    while !text.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
    let previous = text[..*cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i);
    match key.code {
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = text.len(),
        KeyCode::Left => *cursor = previous,
        KeyCode::Right => *cursor += text[*cursor..].chars().next().map_or(0, char::len_utf8),
        KeyCode::Backspace => {
            text.drain(previous..*cursor);
            *cursor = previous;
        }
        KeyCode::Delete if *cursor < text.len() => {
            text.remove(*cursor);
        }
        KeyCode::Char(ch)
            if ch.eq_ignore_ascii_case(&'u') && key.modifiers == KeyModifiers::CONTROL =>
        {
            text.clear();
            *cursor = 0;
        }
        KeyCode::Char(ch)
            if !ch.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            text.insert(*cursor, ch);
            *cursor += ch.len_utf8();
        }
        _ => return false,
    }
    true
}

pub(crate) fn window(text: &str, cursor: usize, width: usize) -> (String, String) {
    let cursor = cursor.min(text.len());
    let mut before = text[..cursor].to_string();
    while ratatui::text::Line::from(before.as_str()).width() >= width.max(1) {
        before.remove(0);
    }
    let mut after = String::new();
    for ch in text[cursor..].chars() {
        if ratatui::text::Line::from(format!("{before}_{after}{ch}")).width() > width {
            break;
        }
        after.push(ch);
    }
    (before, after)
}

impl App {
    pub(crate) fn on_inspection_filter_key(&mut self, key: KeyEvent) -> bool {
        if self.process_info_detail_is_open()
            || !matches!(
                self.process_info_tab,
                ProcessInfoTab::Files
                    | ProcessInfoTab::Dlls
                    | ProcessInfoTab::Environment
                    | ProcessInfoTab::Network
            )
        {
            return false;
        }
        let environment_slash = self.process_info_tab == ProcessInfoTab::Environment
            && !self.process_info_filter_editing
            && key.code == KeyCode::Char('/')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if environment_slash
            || (key.code == KeyCode::Char('f') && key.modifiers == KeyModifiers::CONTROL)
        {
            self.process_info_focus = ProcessInfoFocus::Content;
            self.process_info_filter_editing = true;
            if environment_slash {
                self.process_environment_filter_cursor = self.process_environment_filter.len();
            }
            if self.process_info_tab == ProcessInfoTab::Network {
                self.process_network.editing = true;
            }
            return true;
        }
        if !self.process_info_filter_editing {
            return false;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.process_info_filter_editing = false;
            self.process_network.editing = false;
            return false;
        }
        let (text, cursor) = match self.process_info_tab {
            ProcessInfoTab::Files => (
                &mut self.open_files_filter,
                &mut self.open_files_filter_cursor,
            ),
            ProcessInfoTab::Dlls => (
                &mut self.process_modules_filter,
                &mut self.process_modules_filter_cursor,
            ),
            ProcessInfoTab::Environment => (
                &mut self.process_environment_filter,
                &mut self.process_environment_filter_cursor,
            ),
            ProcessInfoTab::Network => (
                &mut self.process_network.filter,
                &mut self.process_network.cursor,
            ),
            _ => return false,
        };
        match key.code {
            KeyCode::Enter => self.process_info_filter_editing = false,
            _ => {
                edit(text, cursor, key);
            }
        }
        self.process_network.editing =
            self.process_info_filter_editing && self.process_info_tab == ProcessInfoTab::Network;
        self.process_modules_selected = 0;
        self.process_environment_selected = 0;
        self.open_files_selected = 0;
        self.process_network.reset_selection();
        self.process_info_dlls_scroll.scroll_home();
        self.process_info_environment_scroll.scroll_home();
        true
    }
}
