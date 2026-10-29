use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::{
    model::{
        ProcessIdentity,
        network::{NetworkEndpoint, NetworkEndpointKey, NetworkReport},
    },
    samplers::network::{NetworkContext, NetworkPayload, NetworkRequest},
    ui::{network as ui, widgets::scrollable_modal::ScrollableModalState},
};

use super::{
    App, AppActivity, ProcessInfoFocus, ProcessInfoTab, ProcessLifecycle,
    state::ProcessInfoDialogTarget,
};

#[derive(Default)]
pub(crate) struct NetworkView {
    pub(crate) visible: bool,
    pub(crate) generation: u64,
    pub(crate) target: Option<ProcessIdentity>,
    pub(crate) pending: Option<NetworkContext>,
    pub(crate) report: Option<NetworkReport>,
    pub(crate) notice: Option<String>,
    pub(crate) attempted: bool,
    pub(crate) all: bool,
    pub(crate) filter: String,
    pub(crate) cursor: usize,
    pub(crate) editing: bool,
    pub(crate) selected: usize,
    pub(crate) detail: bool,
    pub(crate) detail_entry: Option<NetworkEndpoint>,
    pub(crate) navigation_key: Option<NetworkEndpointKey>,
    pub(crate) matched_key: Option<NetworkEndpointKey>,
    pub(crate) scroll: ScrollableModalState,
}

impl NetworkView {
    pub(crate) fn entries(&self) -> Vec<&NetworkEndpoint> {
        self.report
            .iter()
            .flat_map(|report| &report.endpoints)
            .filter(|entry| {
                self.matched_key
                    .as_ref()
                    .is_none_or(|key| &entry.key == key)
            })
            .filter(|entry| (self.all || entry.is_listener_or_udp()) && entry.matches(&self.filter))
            .collect()
    }

    pub(crate) fn selected_entry(&self) -> Option<&NetworkEndpoint> {
        self.detail
            .then_some(self.detail_entry.as_ref())
            .flatten()
            .or_else(|| self.entries().get(self.selected).copied())
    }

    pub(crate) fn reset_selection(&mut self) {
        if self.matched_key.take().is_some() {
            self.notice = None;
        }
        self.selected = 0;
        self.detail = false;
        self.detail_entry = None;
        self.scroll.reset();
    }
}

impl App {
    pub(crate) fn network_view(&self, global: bool) -> &NetworkView {
        if global {
            &self.network_browser
        } else {
            &self.process_network
        }
    }

    pub(super) fn network_view_mut(&mut self, global: bool) -> &mut NetworkView {
        if global {
            &mut self.network_browser
        } else {
            &mut self.process_network
        }
    }

    fn next_network_context(&mut self, global: bool) -> NetworkContext {
        self.network_next_id = self.network_next_id.wrapping_add(1).max(1);
        let view = self.network_view(global);
        NetworkContext {
            id: self.network_next_id,
            generation: view.generation,
            target: view.target.clone(),
        }
    }

    pub(crate) fn open_network_browser(&mut self) {
        if self.activity() == AppActivity::LogView {
            return;
        }
        self.file_users.visible = false;
        self.network_browser.visible = true;
        self.network_browser.detail = false;
        self.network_browser.detail_entry = None;
        self.network_browser.editing = true;
        self.network_browser.cursor = self.network_browser.filter.len();
        if self.network_browser.generation == 0 {
            self.network_next_id = self.network_next_id.wrapping_add(1).max(1);
            self.network_browser.generation = self.network_next_id;
        }
        if !self.network_browser.attempted {
            self.refresh_network(true);
        }
    }

    pub(crate) fn reset_process_network(&mut self) {
        self.process_network = NetworkView {
            visible: true,
            generation: self.process_info_generation,
            target: self
                .process_info_target
                .as_ref()
                .map(|target| target.identity.clone()),
            all: true,
            ..NetworkView::default()
        };
    }

    pub(crate) fn ensure_process_network(&mut self) {
        if !self.process_network.attempted {
            self.refresh_network(false);
        }
    }

    pub(crate) fn refresh_network(&mut self, global: bool) {
        if self.activity() == AppActivity::LogView || !self.network_view(global).visible {
            return;
        }
        if !global
            && self
                .process_info_target
                .as_ref()
                .is_none_or(|target| target.lifecycle != ProcessLifecycle::Live)
        {
            self.process_network.attempted = true;
            self.process_network.notice = Some("Target is no longer live".into());
            return;
        }
        if self.network_view(global).pending.is_some() {
            return;
        }
        let context = self.next_network_context(global);
        let result = self
            .network_worker
            .request(NetworkRequest::Collect(context.clone()));
        let view = self.network_view_mut(global);
        view.attempted = true;
        view.detail = false;
        view.detail_entry = None;
        view.notice = result.as_ref().err().map(ToString::to_string);
        if result.is_ok() {
            view.pending = Some(context);
        }
    }

    pub(crate) fn poll_network_results(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.network_worker.try_recv() {
            let global = result.context.target.is_none();
            let view = self.network_view(global);
            if self.activity() == AppActivity::LogView
                || (!global && !view.visible)
                || view.pending.as_ref() != Some(&result.context)
                || view.generation != result.context.generation
                || view.target != result.context.target
                || (!global
                    && (!self.show_process_info_dialog
                        || self.process_info_generation != result.context.generation
                        || self
                            .process_info_target
                            .as_ref()
                            .map(|target| &target.identity)
                            != result.context.target.as_ref()))
            {
                continue;
            }
            if global
                && matches!(result.payload, NetworkPayload::Owner(_))
                && (!view.visible || self.has_workspace_overlay())
            {
                self.network_browser.pending = None;
                continue;
            }
            changed = true;
            let view = self.network_view_mut(global);
            view.pending = None;
            match result.payload {
                NetworkPayload::Report(Ok(report)) => {
                    let selected_key = view.selected_entry().map(|entry| entry.key.clone());
                    view.report = Some(report);
                    view.selected = selected_key
                        .and_then(|key| view.entries().iter().position(|entry| entry.key == key))
                        .unwrap_or(0);
                    view.notice = view.matched_key.as_ref().map(|_| {
                        if view.entries().is_empty() {
                            "Original endpoint unavailable in this capture."
                        } else {
                            "Matched endpoint (edit Filter to clear)"
                        }
                        .into()
                    });
                    view.scroll
                        .ensure_visible(view.selected, view.entries().len());
                }
                NetworkPayload::Report(Err(error)) | NetworkPayload::Owner(Err(error)) => {
                    view.notice = Some(error)
                }
                NetworkPayload::Owner(Ok(owner)) => {
                    let matched_key = self.network_view_mut(global).navigation_key.take();
                    if global && (!self.network_browser.visible || self.has_workspace_overlay()) {
                        continue;
                    }
                    // The worker reopens and verifies the precise native creation time before navigation.
                    let identity = owner.identity();
                    let process = self
                        .display_snapshot()
                        .processes
                        .iter()
                        .find(|process| ProcessIdentity::from_row(process) == identity)
                        .cloned()
                        .unwrap_or_else(|| owner.process_row());
                    let target = ProcessInfoDialogTarget {
                        identity,
                        process,
                        lifecycle: ProcessLifecycle::Live,
                    };
                    if let Err(error) = self.open_process_info_dialog(
                        target,
                        if global {
                            ProcessInfoTab::Network
                        } else {
                            ProcessInfoTab::Metrics
                        },
                    ) {
                        self.network_browser.notice = Some(error.to_string());
                    } else if global {
                        self.process_network.matched_key = matched_key;
                        self.process_info_focus = ProcessInfoFocus::Content;
                    }
                }
            }
        }
        changed
    }

    fn open_network_owner(&mut self) {
        if self.network_browser.pending.is_some() {
            return;
        }
        let owner = self
            .network_browser
            .selected_entry()
            .and_then(|entry| entry.owner.clone());
        let Some(owner) = owner else {
            self.network_browser.notice =
                Some("Owner unavailable or unverified; refresh to try again".into());
            return;
        };
        let key = self
            .network_browser
            .selected_entry()
            .map(|entry| entry.key.clone());
        self.verify_network_owner(owner, true);
        if self.network_browser.pending.is_some() {
            self.network_browser.navigation_key = key;
        }
    }

    pub(crate) fn verify_network_owner(
        &mut self,
        owner: crate::model::network::NetworkOwner,
        global: bool,
    ) {
        if self.activity() == AppActivity::LogView
            || !self.network_view(global).visible
            || self.network_view(global).pending.is_some()
        {
            return;
        }
        self.network_view_mut(global).navigation_key = None;
        let context = self.next_network_context(global);
        match self
            .network_worker
            .request(NetworkRequest::Verify(context.clone(), owner))
        {
            Ok(()) => {
                self.network_view_mut(global).pending = Some(context);
                self.network_view_mut(global).notice = None;
            }
            Err(error) => self.network_view_mut(global).notice = Some(error.to_string()),
        }
    }

    pub(crate) fn sync_network_layout(&mut self, global: bool, screen: Rect) {
        let area = if global {
            ui::browser_layout(screen).content
        } else {
            crate::ui::process_info_content_area_for_screen(screen, self.process_info_height())
        };
        let view = self.network_view_mut(global);
        let layout = ui::content_layout(area);
        let (rows, total) = if view.detail {
            (
                area.height as usize,
                ui::detail_lines(view, area.width).len(),
            )
        } else {
            (layout.rows.height as usize, view.entries().len())
        };
        view.scroll.set_page_size(rows, total);
        if !view.detail {
            view.selected = view.selected.min(total.saturating_sub(1));
            view.scroll.ensure_visible(view.selected, total);
        }
    }

    pub(crate) fn on_network_key(&mut self, key: KeyEvent, global: bool) -> Result<()> {
        if self.activity() == AppActivity::LogView {
            if key.code == KeyCode::Esc {
                self.close_process_info_dialog();
            }
            return Ok(());
        }
        self.sync_network_layout(global, self.last_screen_area);
        let width =
            ui::active_content_area(self.last_screen_area, global, self.process_info_height())
                .width;
        let view = self.network_view_mut(global);
        let explicit_filter_edit = view.editing;
        if explicit_filter_edit || (!global && !view.detail) {
            let mut handled = true;
            match key.code {
                KeyCode::Enter if explicit_filter_edit => view.editing = false,
                KeyCode::Esc if explicit_filter_edit => {
                    view.filter.clear();
                    view.cursor = 0;
                    view.reset_selection();
                    view.editing = false;
                }
                KeyCode::Left => {
                    view.cursor = view.filter[..view.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i)
                }
                KeyCode::Right => {
                    view.cursor += view.filter[view.cursor..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
                }
                KeyCode::Home if explicit_filter_edit => view.cursor = 0,
                KeyCode::End if explicit_filter_edit => view.cursor = view.filter.len(),
                KeyCode::Backspace if view.cursor > 0 => {
                    let previous = view.filter[..view.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    view.filter.drain(previous..view.cursor);
                    view.cursor = previous;
                    view.reset_selection();
                }
                KeyCode::Delete if view.cursor < view.filter.len() => {
                    view.filter.remove(view.cursor);
                    view.reset_selection();
                }
                KeyCode::Char('u')
                    if explicit_filter_edit && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    view.filter.clear();
                    view.cursor = 0;
                    view.reset_selection();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !ch.is_control() =>
                {
                    view.filter.insert(view.cursor, ch);
                    view.cursor += ch.len_utf8();
                    view.reset_selection();
                }
                _ => handled = false,
            }
            if handled || explicit_filter_edit {
                return Ok(());
            }
        }
        let total = if view.detail {
            ui::detail_lines(view, width).len()
        } else {
            view.entries().len()
        };
        let view = self.network_view_mut(global);
        match key.code {
            KeyCode::Esc if view.detail => {
                view.detail = false;
                view.detail_entry = None;
                view.scroll.reset();
            }
            KeyCode::Esc => {
                if global {
                    self.network_browser.visible = false;
                } else {
                    self.close_process_info_dialog();
                }
            }
            KeyCode::Enter if view.detail => {
                view.detail = false;
                view.detail_entry = None;
                view.scroll.reset();
            }
            KeyCode::Enter if global => self.open_network_owner(),
            KeyCode::Enter | KeyCode::Char(' ')
                if (global || key.code == KeyCode::Enter)
                    && (view.report.is_some() || view.notice.is_some()) =>
            {
                view.detail_entry = view.selected_entry().cloned();
                view.detail = true;
                view.scroll.reset();
            }
            KeyCode::Char(ch)
                if !view.detail
                    && ((global
                        && ch == '/'
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
                        || (ch.eq_ignore_ascii_case(&'f')
                            && key.modifiers.contains(KeyModifiers::CONTROL))) =>
            {
                view.editing = true;
                view.cursor = view.filter.len();
            }
            KeyCode::Char(ch)
                if !view.detail
                    && ((global && ch == 'a')
                        || (!global
                            && ch.eq_ignore_ascii_case(&'a')
                            && key.modifiers == KeyModifiers::ALT)) =>
            {
                view.all = !view.all;
                view.reset_selection();
            }
            KeyCode::Char('r') if global => self.refresh_network(global),
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'u')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.refresh_network(global)
            }
            KeyCode::Char(ch)
                if ch.eq_ignore_ascii_case(&'c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if let Some(entry) = view.selected_entry() {
                    let text = entry.plain_text();
                    view.notice = Some(match super::clipboard::copy_text_to_clipboard(&text) {
                        Ok(()) => "Copied endpoint".into(),
                        Err(error) => format!("Copy failed: {error}"),
                    });
                }
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
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
                    _ => total.saturating_sub(1),
                }
                .min(total.saturating_sub(1));
                if view.detail {
                    view.scroll.offset = next.min(view.scroll.max_offset(total));
                } else {
                    view.selected = next;
                    view.scroll.ensure_visible(next, total);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn on_network_mouse(&mut self, mouse: MouseEvent, global: bool, screen: Rect) {
        if self.activity() == AppActivity::LogView {
            return;
        }
        self.sync_network_layout(global, screen);
        let area = ui::active_content_area(screen, global, self.process_info_height());
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        if inside && !global {
            self.process_info_focus = ProcessInfoFocus::Content;
        }
        let view = self.network_view_mut(global);
        let layout = ui::content_layout(area);
        let total = if view.detail {
            ui::detail_lines(view, area.width).len()
        } else {
            view.entries().len()
        };
        let scrollbar = ui::scrollbar_area(area, view);
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => view.scroll.stop_drag(),
            MouseEventKind::Drag(MouseButton::Left) if view.scroll.dragging => {
                if let Some(bar) = scrollbar {
                    view.scroll.drag_to(bar, mouse.row, total);
                }
                if !view.detail {
                    view.selected = view.scroll.offset;
                }
            }
            MouseEventKind::Down(MouseButton::Left) if inside => {
                if let Some(bar) =
                    scrollbar.filter(|bar| bar.contains(Position::new(mouse.column, mouse.row)))
                {
                    view.scroll.start_drag(bar, mouse.row, total);
                    view.scroll.drag_to(bar, mouse.row, total);
                    if !view.detail {
                        view.selected = view.scroll.offset;
                    }
                } else if !view.detail
                    && layout
                        .filter
                        .contains(Position::new(mouse.column, mouse.row))
                {
                    view.editing = global;
                    view.cursor = view.filter.len();
                } else if !view.detail
                    && layout.mode.contains(Position::new(mouse.column, mouse.row))
                {
                    view.all = !view.all;
                    view.reset_selection();
                } else if !view.detail
                    && layout.rows.contains(Position::new(mouse.column, mouse.row))
                {
                    view.editing = false;
                    view.selected = (view.scroll.offset + usize::from(mouse.row - layout.rows.y))
                        .min(total.saturating_sub(1));
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if inside => {
                let code = if mouse.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                let _ = self.on_network_key(KeyEvent::new(code, KeyModifiers::NONE), global);
            }
            _ => {}
        }
    }
}
