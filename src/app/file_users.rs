use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::{AppActivity, ProcessInfoTab, ProcessLifecycle, state::ProcessInfoDialogTarget};
use crate::{
    App,
    model::file_users::*,
    ui::{file_users as ui, widgets::scrollable_modal::ScrollableModalState},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FileUsersFocus {
    #[default]
    Query,
    Results,
}

#[derive(Debug, Default)]
pub(crate) struct FileUsersView {
    pub(crate) visible: bool,
    pub(crate) draft: String,
    pub(crate) cursor: usize,
    pub(crate) report: FileSearchReport,
    pub(crate) focus: FileUsersFocus,
    pub(crate) selected: usize,
    pub(crate) scroll: ScrollableModalState,
    pub(crate) detail: bool,
    pub(crate) pending: Option<u64>,
    pub(crate) verifying: bool,
    pub(crate) navigation_match: Option<FileUserMatch>,
    pub(crate) notice: Option<String>,
}

impl FileUsersView {
    pub(crate) fn filtered_entries(&self) -> impl Iterator<Item = &FileUserMatch> {
        let terms = crate::model::search::terms(&self.draft)
            .iter()
            .map(|term| normalize_search_path(term))
            .collect::<Vec<_>>();
        self.report.matches.iter().filter(move |entry| {
            if terms.is_empty() {
                return true;
            }
            let path = normalize_search_path(&entry.path);
            terms.iter().all(|term| path.contains(term))
        })
    }

    pub(crate) fn filtered_count(&self) -> usize {
        self.filtered_entries().count()
    }

    pub(crate) fn selected_entry(&self) -> Option<&FileUserMatch> {
        self.filtered_entries().nth(self.selected)
    }
}

impl App {
    pub(crate) fn open_file_users(&mut self) {
        if self.activity() == AppActivity::LogView {
            return;
        }
        let opening = !self.file_users.visible;
        self.network_browser.visible = false;
        self.file_users.visible = true;
        self.file_users.detail = false;
        self.file_users.focus = FileUsersFocus::Query;
        self.file_users.cursor = self.file_users.draft.len();
        if opening {
            self.start_file_search();
        }
    }

    pub(crate) fn close_file_users(&mut self) {
        self.file_users_worker.cancel();
        self.file_users.visible = false;
        self.file_users.pending = None;
        self.file_users.verifying = false;
        self.file_users.navigation_match = None;
    }

    pub(crate) fn start_file_search(&mut self) {
        if self.activity() == AppActivity::LogView
            || !self.file_users.visible
            || self.file_users.pending.is_some()
        {
            return;
        }
        self.file_users_next_id = self.file_users_next_id.wrapping_add(1).max(1);
        let id = self.file_users_next_id;
        match self
            .file_users_worker
            .request(id, FileUsersRequest::CaptureAll)
        {
            Ok(()) => {
                let view = &mut self.file_users;
                view.report = FileSearchReport::default();
                view.pending = Some(id);
                view.verifying = false;
                view.selected = 0;
                view.detail = false;
                view.scroll.reset();
                view.notice = None;
            }
            Err(error) => self.file_users.notice = Some(error.to_string()),
        }
    }

    pub(crate) fn poll_file_users_results(&mut self) -> bool {
        let Some(update) = self.file_users_worker.take_update() else {
            return false;
        };
        if self.activity() == AppActivity::LogView || self.file_users.pending != Some(update.id) {
            return false;
        }
        let finished = update.report.end.is_some();
        if self.file_users.verifying {
            if finished {
                let navigation_match = self.file_users.navigation_match.take();
                self.file_users.pending = None;
                self.file_users.verifying = false;
                self.file_users.notice = match &update.report.end {
                    Some(FileSearchEnd::Failed(error)) => {
                        Some(format!("Original match unavailable: {error}"))
                    }
                    Some(FileSearchEnd::Complete) => update.report.cleanup_warning.clone(),
                    end => Some(
                        end.as_ref()
                            .map_or("Owner unavailable", FileSearchEnd::label)
                            .into(),
                    ),
                };
                if self.file_users.visible
                    && !self.has_workspace_overlay()
                    && update.report.end == Some(FileSearchEnd::Complete)
                    && update.report.cleanup_warning.is_none()
                    && let Some(owner) = update.owner
                    && let Some(matched) = navigation_match
                    && matched.owner == owner
                {
                    let target = ProcessInfoDialogTarget {
                        identity: owner.identity(),
                        process: owner.process_row(),
                        lifecycle: ProcessLifecycle::Live,
                    };
                    if let Err(error) =
                        self.open_verified_process_info_dialog(target, ProcessInfoTab::Files)
                    {
                        self.file_users.notice = Some(error.to_string());
                    } else {
                        self.open_files_filter = matched.path.clone();
                        self.open_files_filter_cursor = self.open_files_filter.len();
                        self.open_files_match_path = Some(matched.path);
                    }
                }
            }
        } else {
            let key = self
                .file_users
                .selected_entry()
                .map(|e| (e.path.clone(), e.owner.pid, e.owner.creation_time));
            self.file_users.report = update.report;
            if let Some((path, pid, created)) = key {
                let selected = self
                    .file_users
                    .filtered_entries()
                    .position(|e| {
                        e.path == path && e.owner.pid == pid && e.owner.creation_time == created
                    })
                    .unwrap_or(0);
                self.file_users.selected = selected;
            }
            if finished {
                self.file_users.pending = None;
                if self.file_users.notice.as_deref() == Some("Cancelling...") {
                    self.file_users.notice = None;
                }
            }
            self.sync_file_users_layout(self.last_screen_area);
        }
        true
    }

    fn open_file_user_owner(&mut self) {
        if self.file_users.pending.is_some() {
            self.file_users.notice =
                Some("Finish or cancel collection before opening a process".into());
            return;
        }
        let Some(entry) = self.file_users.selected_entry() else {
            return;
        };
        let matched = entry.clone();
        let owner = matched.owner.clone();
        self.file_users_next_id = self.file_users_next_id.wrapping_add(1).max(1);
        let id = self.file_users_next_id;
        match self
            .file_users_worker
            .request(id, FileUsersRequest::Verify(owner))
        {
            Ok(()) => {
                self.file_users.pending = Some(id);
                self.file_users.verifying = true;
                self.file_users.navigation_match = Some(matched);
                self.file_users.notice = None;
            }
            Err(error) => self.file_users.notice = Some(error.to_string()),
        }
    }

    pub(crate) fn sync_file_users_layout(&mut self, screen: Rect) {
        let area = ui::browser_layout(screen).content;
        let view = &mut self.file_users;
        let total = if view.detail {
            ui::detail_lines(view, area.width).len()
        } else {
            view.filtered_count()
        };
        let rows = if view.detail {
            area.height
        } else {
            ui::content_layout(area).rows.height
        };
        view.scroll.set_page_size(rows as usize, total);
        if !view.detail {
            view.selected = view.selected.min(total.saturating_sub(1));
            view.scroll.ensure_visible(view.selected, total);
        }
    }

    pub(crate) fn on_file_users_key(&mut self, key: KeyEvent) -> Result<()> {
        self.sync_file_users_layout(self.last_screen_area);
        if self.direct_file_action_key(key) {
            return Ok(());
        }
        if key.code == KeyCode::Esc {
            if self.file_users.detail {
                self.file_users.detail = false;
                self.file_users.scroll.reset();
            } else if self.file_users.pending.is_some()
                && self.file_users.notice.as_deref() != Some("Cancelling...")
            {
                self.file_users_worker.cancel();
                self.file_users.notice = Some("Cancelling...".into());
            } else {
                self.close_file_users();
            }
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('r' | 'R'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.start_file_search();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('c' | 'C'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            if let Some(entry) = self.file_users.selected_entry() {
                let text = entry.plain_text();
                self.file_users.notice =
                    Some(match super::clipboard::copy_text_to_clipboard(&text) {
                        Ok(()) => "Copied file user".into(),
                        Err(e) => format!("Copy failed: {e}"),
                    });
            }
            return Ok(());
        }
        let view = &mut self.file_users;
        if !view.detail
            && view.focus != FileUsersFocus::Query
            && key.code == KeyCode::Char('/')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            view.focus = FileUsersFocus::Query;
            view.cursor = view.draft.len();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            view.detail = false;
            view.focus = match view.focus {
                FileUsersFocus::Query => FileUsersFocus::Results,
                FileUsersFocus::Results => FileUsersFocus::Query,
            };
            return Ok(());
        }
        if view.detail {
            if key.code == KeyCode::Enter {
                view.detail = false;
                view.scroll.reset();
                return Ok(());
            }
        } else if view.focus == FileUsersFocus::Query {
            let previous_draft = view.draft.clone();
            let selected = view.selected_entry().cloned();
            match key.code {
                KeyCode::Enter => view.focus = FileUsersFocus::Results,
                KeyCode::Char('u' | 'U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.draft.clear();
                    view.cursor = 0;
                }
                KeyCode::Left => {
                    view.cursor = view.draft[..view.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i)
                }
                KeyCode::Right => {
                    view.cursor += view.draft[view.cursor..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
                }
                KeyCode::Home => view.cursor = 0,
                KeyCode::End => view.cursor = view.draft.len(),
                KeyCode::Backspace if view.cursor > 0 => {
                    let previous = view.draft[..view.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    view.draft.drain(previous..view.cursor);
                    view.cursor = previous;
                }
                KeyCode::Delete if view.cursor < view.draft.len() => {
                    view.draft.remove(view.cursor);
                }
                KeyCode::Char(ch)
                    if !ch.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if view.draft.encode_utf16().count() + ch.len_utf16() <= MAX_QUERY_UNITS {
                        view.draft.insert(view.cursor, ch);
                        view.cursor += ch.len_utf8();
                    } else {
                        view.notice = Some("Query limit: 4096 UTF-16 units".into());
                    }
                }
                _ => {}
            }
            if previous_draft != view.draft {
                view.selected = selected
                    .and_then(|selected| {
                        view.filtered_entries().position(|entry| {
                            entry.path == selected.path && entry.owner == selected.owner
                        })
                    })
                    .unwrap_or(0);
                view.scroll.reset();
                self.sync_file_users_layout(self.last_screen_area);
            }
            return Ok(());
        } else {
            match key.code {
                KeyCode::Enter => {
                    self.open_file_user_owner();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    view.detail = true;
                    view.scroll.reset();
                    return Ok(());
                }
                _ => {}
            }
        }
        let area = ui::browser_layout(self.last_screen_area).content;
        let view = &mut self.file_users;
        let total = if view.detail {
            ui::detail_lines(view, area.width).len()
        } else {
            view.filtered_count()
        };
        let amount = if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            view.scroll.page_size.max(1)
        } else {
            1
        };
        let current = if view.detail {
            view.scroll.offset
        } else {
            view.selected
        };
        let next = match key.code {
            KeyCode::Up | KeyCode::PageUp => current.saturating_sub(amount),
            KeyCode::Down | KeyCode::PageDown => current.saturating_add(amount),
            KeyCode::Home => 0,
            KeyCode::End => total.saturating_sub(1),
            _ => return Ok(()),
        }
        .min(total.saturating_sub(1));
        if view.detail {
            view.scroll.offset = next.min(view.scroll.max_offset(total));
        } else {
            view.selected = next;
            view.scroll.ensure_visible(next, total);
        }
        Ok(())
    }

    pub(crate) fn on_file_users_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        self.sync_file_users_layout(screen);
        let area = ui::browser_layout(screen).content;
        let position = Position::new(mouse.column, mouse.row);
        let layout = ui::content_layout(area);
        let view = &mut self.file_users;
        let total = if view.detail {
            ui::detail_lines(view, area.width).len()
        } else {
            view.filtered_count()
        };
        let bar = ui::scrollbar_area(area, view);
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => view.scroll.stop_drag(),
            MouseEventKind::Drag(MouseButton::Left) if view.scroll.dragging => {
                if let Some(bar) = bar {
                    view.scroll.drag_to(bar, mouse.row, total);
                    if !view.detail {
                        view.selected = view.scroll.offset;
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) if area.contains(position) => {
                if let Some(bar) = bar.filter(|b| b.contains(position)) {
                    view.scroll.start_drag(bar, mouse.row, total);
                    view.scroll.drag_to(bar, mouse.row, total);
                    if !view.detail {
                        view.selected = view.scroll.offset;
                    }
                } else if !view.detail && layout.query.contains(position) {
                    view.focus = FileUsersFocus::Query;
                    view.cursor = view.draft.len();
                } else if !view.detail && layout.rows.contains(position) {
                    view.focus = FileUsersFocus::Results;
                    view.selected = (view.scroll.offset + (mouse.row - layout.rows.y) as usize)
                        .min(total.saturating_sub(1));
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if area.contains(position) => {
                view.focus = FileUsersFocus::Results;
                let key = if mouse.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                let _ = self.on_file_users_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            _ => {}
        }
    }
}
