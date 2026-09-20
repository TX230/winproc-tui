use super::{App, AppActivity, ProcessInfoFocus};
use crate::model::affinity::{AffinityChange, AffinityReport};
use crate::{
    model::scheduling::{PriorityChange, PriorityClass, PriorityOutcome, PriorityReport},
    samplers::scheduling::SchedulingRequest,
    ui::widgets::scrollable_modal::ScrollableModalState,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::sync::mpsc::TryRecvError;

pub(crate) struct SchedulingView {
    pub(crate) affinity: Option<AffinityReport>,
    pub(crate) affinity_focused: bool,
    pub(crate) affinity_selected: usize,
    pub(crate) affinity_draft: usize,
    pub(crate) report: Option<PriorityReport>,
    pub(crate) pending: bool,
    pub(crate) applying: bool,
    pub(crate) selected: usize,
    pub(crate) notice: Option<String>,
    pub(crate) scroll: ScrollableModalState,
}

impl Default for SchedulingView {
    fn default() -> Self {
        Self {
            affinity: None,
            affinity_focused: false,
            affinity_selected: 0,
            affinity_draft: 0,

            report: None,
            pending: false,
            applying: false,
            selected: 2,

            notice: None,
            scroll: ScrollableModalState::default(),
        }
    }
}

impl App {
    pub(crate) fn reset_scheduling(&mut self) {
        self.scheduling_worker.cancel();
        self.scheduling = SchedulingView::default();
    }

    pub(crate) fn ensure_scheduling(&mut self) {
        if self.scheduling.report.is_none() && !self.scheduling.pending {
            self.refresh_scheduling();
        }
    }

    pub(crate) fn scheduling_disabled_reason(&self) -> Option<&'static str> {
        if self.activity() == AppActivity::LogView {
            Some("This information is not included in recording logs.")
        } else if self.is_display_paused() {
            Some("Resume the display before changing scheduling.")
        } else if !self.show_process_info_dialog || !self.process_info_target_is_currently_live() {
            Some("Process exited or no live process is selected.")
        } else if self.scheduling.affinity_focused
            && self
                .scheduling
                .affinity
                .as_ref()
                .and_then(|r| r.current)
                .is_none()
        {
            Some("Current affinity is unavailable; refresh to retry.")
        } else if !self.scheduling.affinity_focused
            && self
                .scheduling
                .report
                .as_ref()
                .and_then(|report| report.current)
                .is_none()
        {
            Some("Current priority is unavailable; refresh to retry.")
        } else if !self
            .scheduling
            .report
            .as_ref()
            .is_some_and(|report| report.writable)
        {
            Some("Read-only: changing scheduling is unavailable.")
        } else {
            None
        }
    }

    pub(crate) fn refresh_scheduling(&mut self) {
        if self.activity() == AppActivity::LogView
            || !self.show_process_info_dialog
            || self.scheduling.pending
        {
            return;
        }
        if !self.process_info_target_is_currently_live() {
            self.scheduling.notice = Some("Process exited or no live process is selected.".into());
            return;
        }

        self.submit_scheduling(None, None);
    }

    fn submit_scheduling(
        &mut self,
        change: Option<PriorityChange>,
        affinity_change: Option<AffinityChange>,
    ) {
        let Some(target) = self.process_info_target.as_ref() else {
            return;
        };
        match self.scheduling_worker.request(SchedulingRequest {
            generation: self.process_info_generation,
            identity: target.identity.clone(),
            change,
            affinity_change,
        }) {
            Ok(()) => {
                self.scheduling.pending = true;
                self.scheduling.applying = change.is_some() || affinity_change.is_some();
                self.scheduling.notice = None;
            }
            Err(error) => self.scheduling.notice = Some(error),
        }
    }

    pub(crate) fn poll_scheduling_results(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.scheduling_worker.try_recv() {
                Ok(result) => {
                    if !self.show_process_info_dialog
                        || self.activity() == AppActivity::LogView
                        || !self.scheduling.pending
                        || result.generation != self.process_info_generation
                        || self
                            .process_info_target
                            .as_ref()
                            .map(|target| &target.identity)
                            != Some(&result.identity)
                    {
                        continue;
                    }
                    self.scheduling.selected = result
                        .report
                        .current
                        .and_then(|current| {
                            PriorityClass::EDITABLE
                                .iter()
                                .position(|class| *class == current)
                        })
                        .unwrap_or(2);
                    if let Some(report) = result.affinity {
                        self.scheduling.affinity_draft = report.current.unwrap_or(0);
                        self.scheduling.affinity_selected = self
                            .scheduling
                            .affinity_selected
                            .min(report.cpus().len().saturating_sub(1));
                        self.scheduling.affinity = Some(report);
                    }

                    self.scheduling.report = Some(result.report);
                    self.scheduling.pending = false;
                    self.scheduling.applying = false;

                    if matches!(
                        self.scheduling.report.as_ref().map(|r| &r.outcome),
                        Some(PriorityOutcome::Failed(_) | PriorityOutcome::AppliedUnverified(_))
                    ) || self.scheduling.affinity.as_ref().is_some_and(|report| {
                        matches!(
                            report.outcome,
                            PriorityOutcome::Failed(_) | PriorityOutcome::AppliedUnverified(_)
                        )
                    }) {
                        let area =
                            crate::ui::process_info_content_area_for_screen(self.last_screen_area);
                        let total =
                            crate::ui::scheduling::lines(self, area.width, self.theme()).len();
                        self.scheduling
                            .scroll
                            .ensure_visible(total.saturating_sub(1), total);
                    }
                    changed = true;
                }
                Err(TryRecvError::Empty) => return changed,
                Err(TryRecvError::Disconnected) => {
                    if self.scheduling.pending {
                        self.scheduling.pending = false;
                        self.scheduling.applying = false;
                        self.scheduling.notice = Some(
                            "Scheduling worker stopped; the result of a pending change is unknown."
                                .into(),
                        );
                        return true;
                    }
                    return changed;
                }
            }
        }
    }

    pub(crate) fn apply_priority_change(&mut self, restore: bool) {
        if self.scheduling.pending {
            return;
        }
        if let Some(reason) = self.scheduling_disabled_reason() {
            self.scheduling.notice = Some(reason.into());
            return;
        }
        let Some(report) = self.scheduling.report.as_ref() else {
            return;
        };
        let Some(current) = report.current else {
            self.scheduling.notice = Some("Refresh to verify the current priority first.".into());
            return;
        };
        let change = if restore {
            let Some(change) = report.restore else {
                self.scheduling.notice =
                    Some("No previous change to restore in this Process Info session.".into());
                return;
            };
            if change.expected != current {
                self.scheduling.notice =
                    Some("Priority changed externally; restoration is unavailable.".into());
                return;
            }
            change
        } else {
            PriorityChange {
                expected: current,
                desired: PriorityClass::EDITABLE[self.scheduling.selected],
                restore: false,
            }
        };
        if !change.desired.editable() {
            self.scheduling.notice = Some("The previous priority cannot be restored: Realtime and unknown classes are not selectable.".into());
            return;
        }
        if change.expected == change.desired {
            self.scheduling.notice = None;
            return;
        }
        self.submit_scheduling(Some(change), None);
    }

    pub(crate) fn on_scheduling_key(&mut self, key: KeyEvent) {
        if self.scheduling.applying {
            return;
        }
        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.select_scheduling_default();
            return;
        }
        if matches!(key.code, KeyCode::Char('a') | KeyCode::Char('p')) && key.modifiers.is_empty() {
            self.focus_scheduling_affinity(key.code == KeyCode::Char('a'));
            return;
        }
        if matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.move_scheduling_vertical(key.code == KeyCode::Down);
            return;
        }
        if self.scheduling.affinity_focused {
            match key.code {
                KeyCode::Enter => {
                    self.apply_affinity_change(false);
                    return;
                }
                KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.apply_affinity_change(true);
                    return;
                }
                KeyCode::Char(' ') => {
                    self.toggle_affinity_cpu();
                    return;
                }
                KeyCode::Left => {
                    self.select_affinity_cpu(self.scheduling.affinity_selected.saturating_sub(1));
                    return;
                }
                KeyCode::Right => {
                    self.select_affinity_cpu(self.scheduling.affinity_selected + 1);
                    return;
                }
                KeyCode::Home => {
                    self.select_affinity_cpu(0);
                    return;
                }
                KeyCode::End => {
                    self.select_affinity_cpu(usize::MAX);
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Esc => self.close_process_info_dialog(),
            KeyCode::Enter | KeyCode::Char(' ') => self.apply_priority_change(false),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.refresh_scheduling()
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_priority_change(true)
            }
            KeyCode::Left => {
                self.change_priority_selection(self.scheduling.selected.saturating_sub(1))
            }
            KeyCode::Right => self.change_priority_selection((self.scheduling.selected + 1).min(4)),
            KeyCode::Home => self.change_priority_selection(0),
            KeyCode::End => self.change_priority_selection(4),
            KeyCode::PageUp => self
                .scheduling
                .scroll
                .scroll_up(self.scheduling.scroll.page_size),
            KeyCode::PageDown => {
                let total =
                    crate::ui::scheduling::lines(self, self.last_screen_area.width, self.theme())
                        .len();
                self.scheduling
                    .scroll
                    .scroll_down(self.scheduling.scroll.page_size, total);
            }
            _ => {}
        }
    }

    fn select_scheduling_default(&mut self) {
        if self.scheduling.pending {
            return;
        }
        if let Some(reason) = self.scheduling_disabled_reason() {
            self.affinity_notice(reason);
            return;
        }
        if self.scheduling.affinity_focused {
            let Some(report) = self.scheduling.affinity.as_ref() else {
                return;
            };
            self.scheduling.affinity_draft = report.allowed;
            self.select_affinity_cpu(0);
            self.apply_affinity_change(false);
        } else {
            self.select_priority(2);
            self.apply_priority_change(false);
        }
    }

    pub(crate) fn focus_scheduling_affinity(&mut self, affinity: bool) {
        self.scheduling.notice = None;
        if affinity {
            self.select_affinity_cpu(self.scheduling.affinity_selected);
        } else {
            self.select_priority(self.scheduling.selected);
        }
    }

    fn move_scheduling_vertical(&mut self, down: bool) {
        use crate::ui::scheduling::{Control, layout};
        let area = crate::ui::process_info_content_area_for_screen(self.last_screen_area);
        let geometry = layout(self, area.width);
        let current = if self.scheduling.affinity_focused {
            Control::Affinity(self.scheduling.affinity_selected)
        } else {
            Control::Priority(self.scheduling.selected)
        };
        let Some(cell) = geometry.cells.iter().find(|cell| cell.control == current) else {
            self.select_priority(self.scheduling.selected);
            return;
        };
        let row = geometry
            .cells
            .iter()
            .filter(|c| {
                if down {
                    c.area.y > cell.area.y
                } else {
                    c.area.y < cell.area.y
                }
            })
            .min_by_key(|c| c.area.y.abs_diff(cell.area.y))
            .map(|c| c.area.y);
        let Some(row) = row else {
            return;
        };
        let center = cell.area.x + cell.area.width / 2;
        let Some(next) = geometry
            .cells
            .iter()
            .filter(|c| c.area.y == row)
            .min_by_key(|c| (c.area.x + c.area.width / 2).abs_diff(center))
        else {
            return;
        };
        match next.control {
            Control::Priority(index) if matches!(current, Control::Priority(_)) => {
                self.change_priority_selection(index)
            }
            Control::Priority(index) => self.select_priority(index),
            Control::Affinity(index) => self.select_affinity_cpu(index),
        }
    }

    pub(crate) fn select_affinity_cpu(&mut self, index: usize) {
        self.scheduling.affinity_focused = true;
        let count = self
            .scheduling
            .affinity
            .as_ref()
            .map(|r| r.cpus().len())
            .unwrap_or(0);
        self.scheduling.affinity_selected = index.min(count.saturating_sub(1));
        let area = crate::ui::process_info_content_area_for_screen(self.last_screen_area);
        let total = crate::ui::scheduling::lines(self, area.width, self.theme()).len();
        self.scheduling.scroll.ensure_visible(
            crate::ui::scheduling::layout(self, area.width)
                .cells
                .iter()
                .find(|cell| {
                    cell.control
                        == crate::ui::scheduling::Control::Affinity(
                            self.scheduling.affinity_selected,
                        )
                })
                .map(|cell| cell.area.y as usize)
                .unwrap_or(0),
            total,
        );
    }

    pub(crate) fn toggle_affinity_cpu(&mut self) {
        if self.scheduling.pending {
            return;
        }
        if let Some(reason) = self.scheduling_disabled_reason() {
            self.affinity_notice(reason);
            return;
        }
        if let Some(cpu) = self
            .scheduling
            .affinity
            .as_ref()
            .and_then(|r| r.cpus().get(self.scheduling.affinity_selected).copied())
        {
            let current = self
                .scheduling
                .affinity
                .as_ref()
                .and_then(|r| r.current)
                .unwrap_or(0);
            let desired = current ^ (1usize << cpu);
            if desired == 0 {
                self.affinity_notice("At least one CPU must remain enabled.");
                return;
            }
            self.scheduling.affinity_draft = desired;
            self.apply_affinity_change(false);
        }
    }

    pub(crate) fn apply_affinity_change(&mut self, restore: bool) {
        if self.scheduling.pending {
            return;
        }
        if let Some(reason) = self.scheduling_disabled_reason() {
            self.affinity_notice(reason);
            return;
        }
        let Some(report) = self.scheduling.affinity.as_ref() else {
            return;
        };
        let Some(current) = report.current else {
            return;
        };
        let change = if restore {
            let Some(change) = report.restore else {
                self.affinity_notice("No previous affinity to restore in this session.");
                return;
            };
            change
        } else {
            AffinityChange {
                expected: current,
                desired: self.scheduling.affinity_draft,
                restore: false,
            }
        };
        let error = if change.expected != current {
            Some("Affinity changed externally; restoration is unavailable.")
        } else if change.desired == 0 || change.desired & !report.allowed != 0 {
            Some("Select at least one allowed CPU.")
        } else if change.desired == current {
            self.scheduling.notice = None;
            return;
        } else {
            None
        };
        if let Some(error) = error {
            self.affinity_notice(error);
            return;
        }
        self.submit_scheduling(None, Some(change));
    }

    fn affinity_notice(&mut self, message: &str) {
        self.scheduling.notice = Some(message.into());
        let area = crate::ui::process_info_content_area_for_screen(self.last_screen_area);
        let total = crate::ui::scheduling::lines(self, area.width, self.theme()).len();
        self.scheduling
            .scroll
            .ensure_visible(total.saturating_sub(1), total);
    }

    fn change_priority_selection(&mut self, index: usize) {
        if self.scheduling.pending {
            return;
        }
        self.scheduling.affinity_focused = false;
        if let Some(reason) = self.scheduling_disabled_reason() {
            self.affinity_notice(reason);
            return;
        }
        self.select_priority(index);
        self.apply_priority_change(false);
    }

    pub(crate) fn select_priority(&mut self, index: usize) {
        self.scheduling.affinity_focused = false;
        self.scheduling.selected = index.min(4);
        let area = crate::ui::process_info_content_area_for_screen(self.last_screen_area);
        let total = crate::ui::scheduling::lines(self, area.width, self.theme()).len();
        self.scheduling.scroll.ensure_visible(
            crate::ui::scheduling::layout(self, area.width)
                .cells
                .iter()
                .find(|cell| {
                    cell.control
                        == crate::ui::scheduling::Control::Priority(self.scheduling.selected)
                })
                .map(|cell| cell.area.y as usize)
                .unwrap_or(0),
            total,
        );
    }

    pub(crate) fn scheduling_mouse_selection(&mut self, mouse: MouseEvent, screen: Rect) -> bool {
        if self.scheduling.applying {
            return true;
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            let area = crate::ui::process_info_content_area_for_screen(screen);
            if let Some(control) =
                crate::ui::scheduling::control_at(self, area, mouse.column, mouse.row)
            {
                self.process_info_focus = ProcessInfoFocus::Content;
                match control {
                    crate::ui::scheduling::Control::Priority(index) => {
                        self.change_priority_selection(index)
                    }
                    crate::ui::scheduling::Control::Affinity(index) => {
                        self.select_affinity_cpu(index);
                        self.toggle_affinity_cpu();
                    }
                }
                return true;
            }
        }
        false
    }
}
