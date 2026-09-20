use super::support::{find_text_position, make_test_app, render_app_to_buffer, render_app_to_text};
use crate::{
    app::{App, ProcessInfoFocus, ProcessInfoTab},
    model::scheduling::{PriorityChange, PriorityClass, PriorityOutcome, PriorityReport},
    samplers::scheduling::{SchedulingRequest, SchedulingResult, SchedulingWorker},
    ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::sync::mpsc::{Receiver, Sender};

struct Fixture {
    app: App,
    requests: Receiver<SchedulingRequest>,
    results: Sender<SchedulingResult>,
}
impl Fixture {
    fn new() -> Self {
        let mut app = make_test_app(2, 10);
        let (worker, requests, results) = SchedulingWorker::test_pair();
        app.scheduling_worker = worker;
        app.open_selected_process_info_dialog().unwrap();
        app.activate_process_info_tab(ProcessInfoTab::Scheduling)
            .unwrap();
        app.process_info_focus = ProcessInfoFocus::Content;
        Self {
            app,
            requests,
            results,
        }
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.app.on_key(KeyEvent::new(code, modifiers)).unwrap();
    }
    fn deliver(&mut self, request: SchedulingRequest, report: PriorityReport) -> bool {
        self.results
            .send(SchedulingResult {
                affinity: None,
                generation: request.generation,
                identity: request.identity,
                report,
            })
            .unwrap();
        self.app.poll_scheduling_results()
    }
    fn finish_pending(&mut self) {
        if !self.app.scheduling.pending {
            return;
        }
        let request = self.requests.try_recv().unwrap();
        let mut priority = self.app.scheduling.report.clone().unwrap();
        let mut affinity = self.app.scheduling.affinity.clone();
        if let Some(change) = request.change {
            priority.current = Some(change.desired);
            priority.restore = if change.restore {
                None
            } else {
                Some(PriorityChange {
                    expected: change.desired,
                    desired: change.expected,
                    restore: true,
                })
            };
            priority.outcome = if change.restore {
                PriorityOutcome::Restored
            } else {
                PriorityOutcome::Changed
            };
        }
        if let Some(change) = request.affinity_change {
            let report = affinity.as_mut().unwrap();
            report.current = Some(change.desired);
            report.restore = if change.restore {
                None
            } else {
                Some(crate::model::affinity::AffinityChange {
                    expected: change.desired,
                    desired: change.expected,
                    restore: true,
                })
            };
            report.outcome = if change.restore {
                PriorityOutcome::Restored
            } else {
                PriorityOutcome::Changed
            };
        }
        self.results
            .send(SchedulingResult {
                generation: request.generation,
                identity: request.identity,
                report: priority,
                affinity,
            })
            .unwrap();
        assert!(self.app.poll_scheduling_results());
    }
    fn ready(&mut self) {
        let request = self.requests.try_recv().unwrap();
        assert!(request.change.is_none());
        assert!(self.deliver(request, report(PriorityClass::Normal)));
    }
}
fn report(current: PriorityClass) -> PriorityReport {
    PriorityReport {
        current: Some(current),
        writable: true,
        restore: None,
        outcome: PriorityOutcome::Read,
    }
}

#[test]
fn scheduling_radio_selection_applies_once_and_restore_is_direct_on_the_fixed_target() {
    let mut f = Fixture::new();
    f.ready();
    f.app.process_table_state.select(Some(1));
    let text = render_app_to_text(&f.app, 160, 35);
    assert!(!text.contains("Enter apply"));
    assert!(!text.contains("review"));
    f.key(KeyCode::Right, KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    let change = PriorityChange {
        expected: PriorityClass::Normal,
        desired: PriorityClass::AboveNormal,
        restore: false,
    };
    assert_eq!(request.identity.name, "proc-0");
    assert_eq!(request.change, Some(change));
    assert!(f.app.scheduling.applying);
    let text = render_app_to_text(&f.app, 160, 35);
    assert!(text.contains("Priority"));
    assert!(!text.contains("Change priority?"));
    f.key(KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(f.app.process_info_tab, ProcessInfoTab::Scheduling);
    for key in [KeyCode::Enter, KeyCode::Tab, KeyCode::Esc] {
        f.key(key, KeyModifiers::NONE);
    }
    assert!(f.requests.try_recv().is_err());
    assert!(f.app.show_process_info_dialog);
    let restore = PriorityChange {
        expected: change.desired,
        desired: change.expected,
        restore: true,
    };
    let mut changed = report(change.desired);
    changed.outcome = PriorityOutcome::Changed;
    changed.restore = Some(restore);
    assert!(f.deliver(request, changed));
    f.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(f.requests.try_recv().unwrap().change, Some(restore));
}

#[test]
fn scheduling_blocks_readonly_pause_log_exit_and_realtime_restore() {
    let mut f = Fixture::new();
    f.ready();
    f.app.scheduling.report.as_mut().unwrap().writable = false;
    f.app.apply_priority_change(false);
    assert!(f.requests.try_recv().is_err());
    let mut current = report(PriorityClass::Normal);
    current.restore = Some(PriorityChange {
        expected: PriorityClass::Normal,
        desired: PriorityClass::Realtime,
        restore: true,
    });
    f.app.scheduling.report = Some(current);
    f.app.apply_priority_change(true);
    assert!(f.requests.try_recv().is_err());
    f.app.select_priority(4);
    f.app.toggle_display_pause();
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(f.requests.try_recv().is_err());
    f.app.toggle_display_pause();
    f.app.log_view_path = Some("recording.log".into());
    f.app.refresh_scheduling();
    f.app.apply_priority_change(false);
    assert!(
        render_app_to_text(&f.app, 80, 24)
            .contains("This information is not included in recording logs.")
    );
    f.app.log_view_path = None;
    f.app.snapshot.processes.remove(0);
    f.app.refresh_scheduling();
    f.app.apply_priority_change(false);
    assert!(f.requests.try_recv().is_err());
}

#[test]
fn scheduling_ignores_old_sessions_and_refreshes_only_on_explicit_request() {
    let mut f = Fixture::new();
    let old = f.requests.try_recv().unwrap();
    f.app.close_process_info_dialog();
    f.app.open_selected_process_info_dialog().unwrap();
    let new = f.requests.try_recv().unwrap();
    assert_ne!(old.generation, new.generation);
    assert!(!f.deliver(old, report(PriorityClass::High)));
    assert!(f.app.scheduling.pending);
    assert!(f.deliver(new, report(PriorityClass::Normal)));
    f.app
        .activate_process_info_tab(ProcessInfoTab::Metrics)
        .unwrap();
    f.app
        .activate_process_info_tab(ProcessInfoTab::Scheduling)
        .unwrap();
    assert!(f.requests.try_recv().is_err());
    f.app.refresh_scheduling();
    f.app.refresh_scheduling();
    let refresh = f.requests.try_recv().unwrap();
    assert!(f.requests.try_recv().is_err());
    assert!(f.deliver(refresh, report(PriorityClass::BelowNormal)));
    assert_eq!(
        f.app.scheduling.report.as_ref().unwrap().current,
        Some(PriorityClass::BelowNormal)
    );
}

#[test]
fn scheduling_mouse_selection_and_small_layout_share_option_geometry() {
    let mut f = Fixture::new();
    f.ready();
    for (width, height) in [(60, 18), (80, 24), (160, 35)] {
        let screen = Rect::new(0, 0, width, height);
        crate::app::sync_layout_state(&mut f.app, screen);
        f.app.select_priority(4);
        let area = ui::process_info_content_area_for_screen(screen);
        let (column, row) = control_position(&f.app, screen, ui::scheduling::Control::Priority(4));
        assert!(row < area.bottom());
        assert_eq!(
            ui::scheduling::control_at(&f.app, area, column, row),
            Some(ui::scheduling::Control::Priority(4))
        );
        f.app.scheduling.selected = 0;
        f.app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        f.finish_pending();
        assert_eq!(f.app.scheduling.selected, 4);
        let rendered = render_app_to_text(&f.app, width, height);
        assert!(rendered.contains("High"), "{rendered}");
        assert!(!rendered.contains("( ) Realtime"));
    }
}

#[test]
fn scheduling_changes_during_recording_do_not_record_priority_or_actions() {
    let mut f = Fixture::new();
    f.ready();
    let path = super::support::unique_recording_path("scheduling");
    super::support::track_process_name(&mut f.app, "proc-0");
    f.app.recording_path_draft = path.to_string_lossy().into_owned();
    f.app.recording_path_cursor = f.app.recording_path_draft.len();
    f.app.show_recording_path_dialog = true;
    f.app.confirm_recording_path().unwrap();
    f.app.select_priority(0);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    assert!(request.change.is_some());
    let mut changed = report(PriorityClass::Idle);
    changed.outcome = PriorityOutcome::Changed;
    assert!(f.deliver(request, changed));
    f.app.scheduling.affinity = Some(affinity_report());
    f.app.scheduling.affinity_draft = 1;
    f.key(KeyCode::Char('a'), KeyModifiers::NONE);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    assert!(request.affinity_change.is_some());
    assert!(f.deliver(request, report(PriorityClass::Idle)));
    f.app.stop_recording().unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    for absent in [
        "scheduling",
        "priority",
        "Priority",
        "Idle",
        "affinity",
        "Affinity",
    ] {
        assert!(!saved.contains(absent), "{absent}");
    }
    std::fs::remove_file(path).unwrap();
}

fn affinity_report() -> crate::model::affinity::AffinityReport {
    crate::model::affinity::AffinityReport {
        current: Some(0b1011),
        allowed: 0b1011,
        kinds: vec![],
        restore: None,
        outcome: PriorityOutcome::Read,
    }
}
fn affinity_ready(f: &mut Fixture) {
    let request = f.requests.try_recv().unwrap();
    f.results
        .send(SchedulingResult {
            generation: request.generation,
            identity: request.identity,
            report: report(PriorityClass::Normal),
            affinity: Some(affinity_report()),
        })
        .unwrap();
    assert!(f.app.poll_scheduling_results());
    f.app.process_info_focus = ProcessInfoFocus::Content;
    f.key(KeyCode::Char('a'), KeyModifiers::NONE);
}
#[test]
fn scheduling_affinity_toggle_applies_once_and_restore_and_pause_guards_remain() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.process_table_state.select(Some(1));
    f.key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(f.app.scheduling.affinity_draft, 0b1010);
    let request = f.requests.try_recv().unwrap();
    let change = request.affinity_change.unwrap();
    assert_eq!(change.desired, 0b1010);
    assert_eq!(request.identity.name, "proc-0");
    assert!(request.change.is_none());
    assert!(!render_app_to_text(&f.app, 100, 30).contains("Change CPU affinity?"));
    f.key(KeyCode::Char('p'), KeyModifiers::NONE);
    assert!(f.app.scheduling.affinity_focused);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    f.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(f.app.show_process_info_dialog);
    assert!(f.requests.try_recv().is_err());
    let mut affinity = affinity_report();
    affinity.current = Some(change.desired);
    affinity.restore = Some(crate::model::affinity::AffinityChange {
        expected: change.desired,
        desired: change.expected,
        restore: true,
    });
    affinity.outcome = PriorityOutcome::Changed;
    f.results
        .send(SchedulingResult {
            generation: request.generation,
            identity: request.identity,
            report: report(PriorityClass::Normal),
            affinity: Some(affinity),
        })
        .unwrap();
    assert!(f.app.poll_scheduling_results());
    f.app.toggle_display_pause();
    f.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(f.requests.try_recv().is_err());
    f.app.toggle_display_pause();
    f.app.scheduling.affinity_draft = 0;
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(f.requests.try_recv().is_err());
    f.app.scheduling.report.as_mut().unwrap().writable = false;
    f.key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(f.app.scheduling.affinity_draft, 0);
    f.app.scheduling.report.as_mut().unwrap().writable = true;
    f.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(
        f.requests
            .try_recv()
            .unwrap()
            .affinity_change
            .unwrap()
            .restore
    );
}

#[test]
fn scheduling_affinity_sparse_and_64_cpu_mouse_and_keyboard_geometry() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    for mask in [0b1011, usize::MAX] {
        let report = f.app.scheduling.affinity.as_mut().unwrap();
        report.allowed = mask;
        report.current = Some(mask);
        f.app.scheduling.affinity_draft = mask;
        for (width, height) in [(60, 18), (80, 24), (160, 35)] {
            let screen = Rect::new(0, 0, width, height);
            crate::app::sync_layout_state(&mut f.app, screen);
            f.key(KeyCode::End, KeyModifiers::NONE);
            let index = mask.count_ones() as usize - 1;
            assert_eq!(f.app.scheduling.affinity_selected, index);
            let area = ui::process_info_content_area_for_screen(screen);
            let (column, row) =
                control_position(&f.app, screen, ui::scheduling::Control::Affinity(index));
            assert!(row < area.bottom());
            assert_eq!(
                ui::scheduling::control_at(&f.app, area, column, row),
                Some(ui::scheduling::Control::Affinity(index))
            );
            let old = f.app.scheduling.affinity_draft;
            f.app.on_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                },
                screen,
            );
            f.finish_pending();
            assert_eq!(
                f.app.scheduling.affinity_draft ^ old,
                1usize << (usize::BITS - 1 - mask.leading_zeros())
            );
            let rendered = render_app_to_text(&f.app, width, height);
            assert!(rendered.contains("Unknown"), "{rendered}");
        }
    }
}

#[test]
fn scheduling_affinity_unsupported_notice_and_empty_selection_remain_visible() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    let screen = Rect::new(0, 0, 60, 18);
    crate::app::sync_layout_state(&mut f.app, screen);
    f.app.scheduling.affinity = Some(crate::model::affinity::AffinityReport::unavailable(
        "CPU affinity unsupported: multiple processor groups.".into(),
    ));
    let text = render_app_to_text(&f.app, 60, 18);
    assert!(text.contains("multiple processor groups"), "{text}");
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(f.requests.try_recv().is_err());
    f.app.scheduling.affinity = Some(affinity_report());
    f.app.scheduling.affinity.as_mut().unwrap().allowed = usize::MAX;
    f.app.scheduling.affinity_draft = 0;
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(render_app_to_text(&f.app, 60, 18).contains("Select at least one allowed CPU."));
    f.app.scheduling.scroll.scroll_home();
    for control in [
        ui::scheduling::Control::Priority(2),
        ui::scheduling::Control::Affinity(0),
    ] {
        let (column, row) = control_position(&f.app, screen, control);
        f.app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            screen,
        );
        assert_eq!(
            f.app.scheduling.affinity_focused,
            matches!(control, ui::scheduling::Control::Affinity(_))
        );
    }
    f.finish_pending();
    let text = render_app_to_text(&f.app, 60, 18);
    for expected in ["Space toggle", "Ctrl+D default"] {
        assert!(text.contains(expected), "{text}");
    }
}

#[test]
fn scheduling_affinity_rejects_reopened_results_and_reveals_failed_application() {
    let mut f = Fixture::new();
    let old = f.requests.try_recv().unwrap();
    f.app.close_process_info_dialog();
    f.app.open_selected_process_info_dialog().unwrap();
    f.results
        .send(SchedulingResult {
            generation: old.generation,
            identity: old.identity,
            report: report(PriorityClass::Normal),
            affinity: Some(affinity_report()),
        })
        .unwrap();
    assert!(!f.app.poll_scheduling_results());
    assert!(f.app.scheduling.affinity.is_none());
    affinity_ready(&mut f);
    crate::app::sync_layout_state(&mut f.app, Rect::new(0, 0, 80, 24));
    f.app.scheduling.affinity_draft = 1;
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    f.key(KeyCode::Enter, KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    let mut failed = affinity_report();
    failed.allowed = usize::MAX;
    failed.outcome = PriorityOutcome::Failed(
        "Affinity changed externally; refresh and review the current value.".into(),
    );
    f.results
        .send(SchedulingResult {
            generation: request.generation,
            identity: request.identity,
            report: report(PriorityClass::Normal),
            affinity: Some(failed),
        })
        .unwrap();
    assert!(f.app.poll_scheduling_results());
    assert!(render_app_to_text(&f.app, 80, 24).contains("Affinity changed externally"));
}

fn control_position(app: &App, screen: Rect, control: ui::scheduling::Control) -> (u16, u16) {
    let area = ui::process_info_content_area_for_screen(screen);
    let geometry = ui::scheduling::layout(app, area.width);
    let cell = geometry
        .cells
        .iter()
        .find(|cell| cell.control == control)
        .unwrap();
    (
        area.x + cell.area.x,
        area.y + (cell.area.y as usize - app.scheduling.scroll.offset) as u16,
    )
}

#[test]
fn scheduling_combined_layout_uses_horizontal_radios_and_multiple_cpu_columns() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    let report = f.app.scheduling.affinity.as_mut().unwrap();
    report.allowed = (1 << 20) - 1;
    report.current = Some(report.allowed);
    f.app.scheduling.affinity_draft = report.allowed;
    let screen = Rect::new(0, 0, 160, 40);
    crate::app::sync_layout_state(&mut f.app, screen);
    let buffer = render_app_to_buffer(&f.app, screen.width, screen.height);
    let text = super::support::buffer_to_text(&buffer);
    let priority_y = find_text_position(&buffer, "( ) Idle").unwrap().1;
    for label in [
        "( ) Below normal",
        "(●) Normal",
        "( ) Above normal",
        "( ) High",
    ] {
        assert_eq!(
            find_text_position(&buffer, label).unwrap().1,
            priority_y,
            "{text}"
        );
    }
    let cpu0 = find_text_position(&buffer, "[x] CPU  0").unwrap();
    let cpu1 = find_text_position(&buffer, "[x] CPU  1").unwrap();
    assert_eq!(cpu0.1, cpu1.1);
    assert!(cpu1.0 > cpu0.0);
    assert!(
        find_text_position(&buffer, "[x] CPU 19").is_some(),
        "{text}"
    );
    assert!(!text.contains("[Priority]"));
    assert!(!text.contains("[Affinity]"));
    // Evidence is a TestBackend buffer, not a desktop-terminal screenshot.
    println!("{text}");
}

#[test]
fn scheduling_combined_tab_and_arrow_navigation_preserve_drafts() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    crate::app::sync_layout_state(&mut f.app, Rect::new(0, 0, 160, 40));
    f.app.select_priority(2);
    f.key(KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(f.app.scheduling.selected, 3);
    f.finish_pending();
    let draft = f.app.scheduling.affinity_draft;
    f.key(KeyCode::Tab, KeyModifiers::NONE);
    assert!(f.app.scheduling.affinity_focused);
    f.key(KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(f.app.scheduling.affinity_selected, 1);
    f.key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(f.app.scheduling.affinity_draft, draft ^ 2);
    f.finish_pending();
    f.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert!(!f.app.scheduling.affinity_focused);
    assert_eq!(f.app.scheduling.selected, 3);
    f.finish_pending();
    f.key(KeyCode::Down, KeyModifiers::NONE);
    assert!(f.app.scheduling.affinity_focused);
    f.key(KeyCode::Up, KeyModifiers::NONE);
    assert!(!f.app.scheduling.affinity_focused);
    f.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(f.app.process_info_focus, ProcessInfoFocus::Tabs);
    f.key(KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(f.app.process_info_focus, ProcessInfoFocus::Content);
    assert!(f.app.scheduling.affinity_focused);
    f.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(f.app.process_info_focus, ProcessInfoFocus::Tabs);
    assert_eq!(f.app.scheduling.affinity_draft, draft ^ 2);
    f.finish_pending();
    assert!(f.requests.try_recv().is_err());
}

#[test]
fn scheduling_grid_every_cpu_click_matches_rendered_checkbox_after_resize() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.scheduling.affinity.as_mut().unwrap().allowed = usize::MAX;
    f.app.scheduling.affinity.as_mut().unwrap().current = Some(usize::MAX);
    f.app.scheduling.affinity_draft = usize::MAX;
    for (width, height) in [(40, 18), (60, 18), (80, 24), (160, 40)] {
        let screen = Rect::new(0, 0, width, height);
        crate::app::sync_layout_state(&mut f.app, screen);
        for index in 0..64 {
            f.app.scheduling.affinity.as_mut().unwrap().current = Some(usize::MAX);
            f.app.scheduling.affinity_draft = usize::MAX;
            f.app.select_affinity_cpu(index);
            let buffer = render_app_to_buffer(&f.app, width, height);
            let label = format!("CPU {index:2}");
            let (x, y) = find_text_position(&buffer, &label).unwrap();
            let before = f.app.scheduling.affinity_draft;
            f.app.on_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
                screen,
            );
            f.finish_pending();
            assert_eq!(f.app.scheduling.affinity_draft ^ before, 1usize << index);
        }
    }
}

#[test]
fn scheduling_grid_resize_keeps_focused_cpu_visible_without_another_key() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.scheduling.affinity.as_mut().unwrap().allowed = usize::MAX;
    crate::app::sync_layout_state(&mut f.app, Rect::new(0, 0, 160, 40));
    f.app.select_affinity_cpu(63);
    for (width, height) in [(60, 18), (80, 24), (160, 40)] {
        crate::app::sync_layout_state(&mut f.app, Rect::new(0, 0, width, height));
        let text = render_app_to_text(&f.app, width, height);
        assert!(text.contains("CPU 63"), "{text}");
    }
    f.app.scheduling.affinity.as_mut().unwrap().allowed = 0;
    f.app.process_info_focus = ProcessInfoFocus::Tabs;
    f.key(KeyCode::Tab, KeyModifiers::NONE);
    assert!(!f.app.scheduling.affinity_focused);
    f.key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(f.app.process_info_focus, ProcessInfoFocus::Tabs);
}

#[test]
fn scheduling_default_key_applies_only_focused_setting_immediately() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.scheduling.report = Some(report(PriorityClass::High));
    f.app.select_priority(4);
    f.app.scheduling.affinity.as_mut().unwrap().current = Some(1);
    f.app.scheduling.affinity_draft = 1;
    f.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(f.app.scheduling.selected, 2);
    assert_eq!(f.app.scheduling.affinity_draft, 1);
    let request = f.requests.try_recv().unwrap();
    assert_eq!(request.change.unwrap().desired, PriorityClass::Normal);
    assert!(request.affinity_change.is_none());
    let mut changed = report(PriorityClass::Normal);
    changed.outcome = PriorityOutcome::Changed;
    assert!(f.deliver(request, changed));
    f.key(KeyCode::Char('a'), KeyModifiers::NONE);
    f.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(f.app.scheduling.affinity_draft, 0b1011);
    assert_eq!(f.app.scheduling.selected, 2);
    let request = f.requests.try_recv().unwrap();
    let change = request.affinity_change.unwrap();
    assert_eq!(change.expected, 1);
    assert_eq!(change.desired, 0b1011);
    assert!(!change.restore);
    assert!(request.change.is_none());
}

#[test]
fn scheduling_default_key_rejects_pending_paused_readonly_log_and_unavailable_affinity() {
    for case in 0..6 {
        let mut f = Fixture::new();
        affinity_ready(&mut f);
        f.app.scheduling.affinity_draft = 1;
        match case {
            0 => f.app.scheduling.pending = true,
            1 => {
                f.app.toggle_display_pause();
            }
            2 => f.app.scheduling.report.as_mut().unwrap().writable = false,
            3 => f.app.log_view_path = Some("test.log".into()),
            4 => f.app.scheduling.affinity.as_mut().unwrap().current = None,
            _ => {
                f.app.snapshot.processes.remove(0);
            }
        }
        f.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(f.app.scheduling.affinity_draft, 1);
        assert!(f.requests.try_recv().is_err());
    }
}

#[test]
fn scheduling_spacing_separates_tabs_priority_affinity_and_summary() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    let screen = Rect::new(0, 0, 160, 40);
    crate::app::sync_layout_state(&mut f.app, screen);
    let area = ui::process_info_content_area_for_screen(screen);
    let buffer = render_app_to_buffer(&f.app, 160, 40);
    let priority = find_text_position(&buffer, "Priority (").unwrap();
    let affinity = find_text_position(&buffer, "Affinity (Group 0)").unwrap();
    assert_eq!(priority.1, area.y + 1);
    assert_eq!(affinity.1, priority.1 + 2);
    for y in [area.y, priority.1 + 1] {
        for x in area.x..area.right().saturating_sub(1) {
            assert_eq!(buffer[(x, y)].symbol(), " ");
        }
    }
    assert!(find_text_position(&buffer, "Ctrl+D default").is_some());
    let (x, y) = priority;
    assert_eq!(
        ui::scheduling::control_at(&f.app, area, x + 9, y),
        Some(ui::scheduling::Control::Priority(0))
    );
}

#[test]
fn scheduling_emphasizes_headings_without_routine_summary_or_success_messages() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.scheduling.report.as_mut().unwrap().outcome = PriorityOutcome::Changed;
    f.app.scheduling.affinity.as_mut().unwrap().outcome = PriorityOutcome::Restored;
    let buffer = render_app_to_buffer(&f.app, 160, 40);
    let text = super::support::buffer_to_text(&buffer);
    for label in ["Priority", "Affinity"] {
        let (x, y) = find_text_position(&buffer, label).unwrap();
        for dx in 0..label.len() as u16 {
            assert!(
                buffer[(x + dx, y)]
                    .modifier
                    .contains(ratatui::prelude::Modifier::BOLD)
            );
        }
    }
    for absent in [
        "Previous:",
        "Selected:",
        "Mask:",
        "Current affinity:",
        "changed and verified",
        "restored and verified",
        "Enter applies",
    ] {
        assert!(!text.contains(absent), "{text}");
    }
}

#[test]
fn scheduling_checkbox_reflects_readback_and_keeps_the_last_cpu_enabled() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    f.app.scheduling.affinity.as_mut().unwrap().current = Some(1);
    f.app.scheduling.affinity_draft = 1;
    f.key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert!(f.requests.try_recv().is_err());
    assert_eq!(f.app.scheduling.affinity_draft, 1);
    assert!(render_app_to_text(&f.app, 160, 40).contains("At least one CPU must remain enabled."));
    f.key(KeyCode::Right, KeyModifiers::NONE);
    f.key(KeyCode::Char(' '), KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    assert_eq!(request.affinity_change.unwrap().desired, 3);
    let mut failed = affinity_report();
    failed.current = Some(1);
    failed.outcome = PriorityOutcome::Failed("Access denied".into());
    f.results
        .send(SchedulingResult {
            generation: request.generation,
            identity: request.identity,
            report: report(PriorityClass::Normal),
            affinity: Some(failed),
        })
        .unwrap();
    assert!(f.app.poll_scheduling_results());
    assert_eq!(f.app.scheduling.affinity_draft, 1);
    let text = render_app_to_text(&f.app, 160, 40);
    assert!(text.contains("[ ] CPU  1"));
    assert!(text.contains("Access denied"));
}

#[test]
fn scheduling_wrapped_radio_arrow_changes_apply_but_entering_the_group_does_not() {
    let mut f = Fixture::new();
    affinity_ready(&mut f);
    let screen = Rect::new(0, 0, 60, 18);
    crate::app::sync_layout_state(&mut f.app, screen);
    f.app.select_priority(0);
    f.key(KeyCode::Down, KeyModifiers::NONE);
    let request = f.requests.try_recv().unwrap();
    assert!(request.change.is_some());
    assert!(request.affinity_change.is_none());
    let mut failed = report(PriorityClass::Normal);
    failed.outcome = PriorityOutcome::Failed("Access denied".into());
    assert!(f.deliver(request, failed));
    let text = render_app_to_text(&f.app, 60, 18);
    assert!(text.contains("(●) Normal"));
    assert!(text.contains("Access denied"));
}
