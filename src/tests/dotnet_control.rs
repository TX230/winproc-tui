use super::support::{
    find_text_position, make_test_app, make_test_app_with_worker, render_app_to_buffer,
    render_app_to_text, test_snapshot,
};
use crate::samplers::dotnet_control::DotNetRuntimeKind;
use crate::{
    model::ProcessIdentity,
    samplers::{CollectSnapshotResult, SamplingWorker},
    ui::header::{HeaderAction, header_actions},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
fn key(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers)).unwrap();
}
fn click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn tracked_app(count: usize) -> crate::App {
    let mut app = make_test_app(count, 10);
    let names = app
        .snapshot
        .processes
        .iter()
        .map(|row| row.name.clone())
        .collect();
    app.apply_tracked_list_switch(app.prepare_tracked_list_switch(names));
    app
}

#[test]
fn dotnet_defaults_off_and_cancel_never_authorizes() {
    let mut app = tracked_app(2);
    assert!(!app.sampling_worker.dotnet.enabled());
    key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert!(app.dotnet_confirmation.is_some());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.dotnet_confirmation.is_none());
    assert!(!app.sampling_worker.dotnet.enabled());
}
#[test]
fn dotnet_confirmation_freezes_lifetimes_and_ignores_filters() {
    let mut app = tracked_app(2);
    app.snapshot.processes[0].start_time = Some(10);
    app.snapshot.processes[1].start_time = Some(20);
    let original = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    app.toggle_dotnet();
    app.snapshot.processes[1].start_time = Some(30);
    let replacement = ProcessIdentity::from_row(&app.snapshot.processes[1]);
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&original));
    assert!(!app.sampling_worker.dotnet.allows(&replacement));
    assert_eq!(
        app.sampling_worker.dotnet.status(&replacement),
        "Not enabled"
    );
    app.toggle_dotnet();
    assert!(!app.sampling_worker.dotnet.enabled());
    assert!(app.sampling_worker.dotnet.stopping());
    app.toggle_dotnet();
    assert!(app.dotnet_confirmation.is_none());
}
#[test]
fn dotnet_header_control_is_removed_at_compact_widths() {
    for width in [40, 65, 80, 120, 180] {
        let app = tracked_app(2);
        let buffer = render_app_to_buffer(&app, width, 24);
        let header = text_header(&buffer, width);
        assert!(!header.contains(".NET"), "{header}");
        assert!(!header.contains("Alt+N"), "{header}");
        assert!(header.contains("LIVE"), "{header}");
        assert!(
            !header_actions(Rect::new(0, 0, width, 1), &app)
                .iter()
                .any(|(action, _)| *action == HeaderAction::DotNet)
        );
    }
}

#[test]
fn dotnet_revocation_discards_queued_eventpipe_but_keeps_pdh() {
    let (worker, _, tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(2, 10, worker);
    let mut snapshot = test_snapshot(2);
    snapshot.processes[0].dotnet_eventpipe = Some(1);
    snapshot.processes[0].dotnet_heap_bytes = Some(123);
    snapshot.processes[1].dotnet_heap_bytes = Some(456);
    tx.send(CollectSnapshotResult {
        snapshot,
        warning: None,
    })
    .unwrap();
    app.poll_sample_results().unwrap();
    assert!(
        app.snapshot
            .processes
            .iter()
            .any(|row| row.dotnet_heap_bytes == Some(456))
    );
    assert!(
        !app.snapshot
            .processes
            .iter()
            .any(|row| row.dotnet_heap_bytes == Some(123))
    );
}

#[test]
fn dotnet_reenable_rejects_previous_generation_measurements() {
    let (worker, _, tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(2, 10, worker);
    let mut snapshot = test_snapshot(2);
    let identity = ProcessIdentity::from_row(&snapshot.processes[0]);
    assert!(app.sampling_worker.dotnet.enable(vec![identity.clone()]));
    snapshot.processes[0].dotnet_eventpipe = Some(app.sampling_worker.dotnet.generation());
    snapshot.processes[0].dotnet_heap_bytes = Some(123);
    app.sampling_worker.dotnet.disable();
    app.sampling_worker.dotnet.finish_suspend();
    assert!(app.sampling_worker.dotnet.enable(vec![identity]));
    tx.send(CollectSnapshotResult {
        snapshot,
        warning: None,
    })
    .unwrap();
    app.poll_sample_results().unwrap();
    assert!(
        !app.snapshot
            .processes
            .iter()
            .any(|row| row.dotnet_heap_bytes == Some(123))
    );
}

fn text_header(buffer: &ratatui::buffer::Buffer, width: u16) -> String {
    (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
}

#[test]
fn dotnet_empty_tracking_list_and_no_live_match_show_actionable_guidance() {
    for width in [65, 80, 120, 180] {
        for has_entry in [false, true] {
            let (worker, requests, _) = SamplingWorker::test_pair();
            let mut app = make_test_app_with_worker(2, 10, worker);
            if has_entry {
                app.apply_tracked_list_switch(
                    app.prepare_tracked_list_switch(vec!["absent.exe".into()]),
                );
            }
            let screen = Rect::new(0, 0, width, 24);
            key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
            assert_eq!(app.dotnet_confirmation, Some(vec![]));
            let buffer = render_app_to_buffer(&app, width, 24);
            let text = render_app_to_text(&app, width, 24);
            let reason = if has_entry {
                "No identifiable live processes match"
            } else {
                "The Tracking List is empty."
            };
            for message in [
                reason,
                "Connecting during initialization can hang target apps.",
                "Confirm target apps have finished initializing.",
                "See dotnet/runtime Issue #119014",
                "New or restarted processes will NOT be collected.",
                "press t",
                "Alt+N again",
                "No authorized targets.",
                "Enter/Esc Close",
            ] {
                assert!(text.contains(message), "{text}");
            }
            assert!(!text.contains("Enter Start"), "{text}");
            assert!(!app.sampling_worker.dotnet.enabled());
            let (x, y) = find_text_position(&buffer, "Close").unwrap();
            app.on_mouse(click(x, y), screen);
            assert!(app.dotnet_confirmation.is_none());
            // Keyboard activation must show exactly the same feedback, and Enter only closes it.
            key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
            assert_eq!(app.dotnet_confirmation, Some(vec![]));
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
            assert!(app.dotnet_confirmation.is_none());
            assert!(!app.sampling_worker.dotnet.enabled());
            assert_eq!(app.sampling_worker.dotnet.generation(), 0);
            assert!(requests.try_recv().is_err());
        }
    }
}

#[test]
fn dotnet_adding_a_tracked_target_after_guidance_allows_explicit_start() {
    let mut app = make_test_app(1, 10);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.dotnet_confirmation, Some(vec![]));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
    let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    assert!(app.is_tracked_process_name(&identity.name));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.dotnet_confirmation, Some(vec![identity.clone()]));
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&identity));
}

#[test]
fn dotnet_targets_exiting_during_confirmation_show_guidance() {
    let mut app = tracked_app(1);
    app.toggle_dotnet();
    app.snapshot.processes.clear();
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(app.dotnet_confirmation, Some(vec![]));
    assert!(app.sampling_worker.dotnet.enabled());
    let text = render_app_to_text(&app, 80, 24);
    assert!(
        text.contains("No identifiable live processes match"),
        "{text}"
    );
}

#[test]
fn dotnet_authorizes_only_tracked_known_lifetimes_not_later_additions() {
    let mut app = make_test_app(4, 10);
    app.snapshot.processes[0].name = "APP.EXE".into();
    app.snapshot.processes[1].name = "app.exe".into();
    app.snapshot.processes[2].name = "app.exe".into();
    app.snapshot.processes[2].start_time = None;
    app.snapshot.processes[3].name = "other.exe".into();
    app.apply_tracked_list_switch(app.prepare_tracked_list_switch(vec!["app.exe".into()]));
    // Tracked-only display is not required for collection scope.
    assert!(!app.watch_enabled);
    app.toggle_dotnet();
    assert_eq!(app.dotnet_confirmation.as_ref().unwrap().len(), 2);
    app.apply_tracked_list_switch(
        app.prepare_tracked_list_switch(vec!["app.exe".into(), "other.exe".into()]),
    );
    key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(!app.sampling_worker.dotnet.enabled());
    assert!(app.dotnet_confirmation.is_some());
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    for (index, row) in app.snapshot.processes.iter().enumerate() {
        assert_eq!(
            app.sampling_worker
                .dotnet
                .allows(&ProcessIdentity::from_row(row)),
            index < 2
        );
    }
}

#[test]
fn dotnet_tracking_changes_preserve_authorized_sessions_and_values() {
    let (worker, requests, tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(2, 10, worker);
    let first = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let second = ProcessIdentity::from_row(&app.snapshot.processes[1]);
    app.focused_panel = crate::app::FocusedPanel::Processes;
    app.sampling_worker
        .dotnet
        .report_managed(first.clone(), DotNetRuntimeKind::EventPipe);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&first));
    assert!(!app.sampling_worker.dotnet.allows(&second));
    let permit = app.sampling_worker.dotnet.permit(&first).unwrap();
    app.snapshot.processes[0].dotnet_eventpipe = Some(app.sampling_worker.dotnet.generation());
    app.snapshot.processes[0].dotnet_heap_bytes = Some(123);
    let sample = app.snapshot.clone();
    for names in [vec![first.name.clone()], vec![second.name.clone()], vec![]] {
        app.apply_tracked_list_switch(app.prepare_tracked_list_switch(names));
        assert!(!permit.cancelled());
        assert!(app.sampling_worker.dotnet.allows(&first));
        assert!(!app.sampling_worker.dotnet.allows(&second));
        assert_eq!(app.snapshot.processes[0].dotnet_heap_bytes, Some(123));
    }
    assert!(requests.try_recv().is_err());
    tx.send(CollectSnapshotResult {
        snapshot: sample,
        warning: None,
    })
    .unwrap();
    app.poll_sample_results().unwrap();
    assert_eq!(
        app.snapshot
            .processes
            .iter()
            .find(|row| ProcessIdentity::from_row(row) == first)
            .unwrap()
            .dotnet_heap_bytes,
        Some(123)
    );
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(permit.cancelled());
    assert!(!app.sampling_worker.dotnet.allows(&first));
    assert_eq!(
        requests.try_recv().unwrap(),
        crate::samplers::SampleRequest::RemoveDotNetTargets(vec![first])
    );
}

#[test]
fn dotnet_confirmation_rechecks_tracking_membership() {
    let mut app = tracked_app(2);
    let keep = ProcessIdentity::from_row(&app.snapshot.processes[1]);
    app.toggle_dotnet();
    app.apply_tracked_list_switch(app.prepare_tracked_list_switch(vec![keep.name.clone()]));
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        !app.sampling_worker
            .dotnet
            .allows(&ProcessIdentity::from_row(&app.snapshot.processes[0]))
    );
    assert!(app.sampling_worker.dotnet.allows(&keep));
}

#[test]
fn dotnet_individual_tracking_removal_preserves_permission() {
    let mut app = tracked_app(1);
    app.toggle_dotnet();
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.enabled());
    app.remove_selected_process_from_watch_list();
    assert!(app.sampling_worker.dotnet.enabled());
    app.add_selected_process_to_watch_list();
    assert!(app.sampling_worker.dotnet.enabled());
}

#[test]
fn dotnet_warning_and_issue_remain_visible_with_and_without_targets() {
    for has_target in [false, true] {
        let mut app = if has_target {
            tracked_app(1)
        } else {
            make_test_app(1, 10)
        };
        key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
        if has_target {
            key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        }
        for (width, height) in [(65, 20), (80, 24), (120, 35)] {
            let text = render_app_to_text(&app, width, height);
            for message in [
                "Connecting during initialization can hang target apps.",
                "Confirm target apps have finished initializing.",
                "See dotnet/runtime Issue #119014",
                "This dialog enables live Tracking List processes only.",
                "New or restarted processes will NOT be collected.",
            ] {
                assert!(
                    text.contains(message),
                    "targets={has_target}, {width}x{height}\n{text}"
                );
            }
            if has_target {
                assert!(text.contains("Space Toggle  Enter/Esc Close"), "{text}");
            } else {
                for message in [
                    "The Tracking List is empty.",
                    "press t",
                    "Alt+N again.",
                    "Enter/Esc Close",
                ] {
                    assert!(text.contains(message), "{text}");
                }
            }
            if width == 80 {
                println!("Rendered .NET dialog (targets={has_target}):\n{text}");
            }
        }
    }
}

#[test]
fn dotnet_checkbox_is_red_on_second_row_and_owns_initial_focus() {
    for width in [65, 80, 120, 180] {
        let mut app = tracked_app(1);
        key(&mut app, KeyCode::Char('n'), KeyModifiers::ALT);
        let screen = Rect::new(0, 0, width, 24);
        let buffer = render_app_to_buffer(&app, width, 24);
        let (x, y) = find_text_position(&buffer, "[ ] Enable .NET metrics").unwrap();
        let (_, title_y) = find_text_position(&buffer, ".NET METRICS").unwrap();
        let checkbox = crate::ui::header::dotnet_checkbox_area(screen);
        assert_eq!((checkbox.x, checkbox.y), (x, y));
        assert_eq!(y, title_y + 2);
        for column in x..checkbox.right() {
            assert_eq!(buffer[(column, y - 1)].symbol(), " ");
            assert_eq!(buffer[(column, y)].fg, ratatui::style::Color::Red);
            assert_eq!(buffer[(column, y)].bg, app.theme().focus_surface);
        }
        assert_eq!(app.draw_cursor.get(), Some((x + 1, y).into()));
        for code in [KeyCode::Tab, KeyCode::BackTab] {
            key(&mut app, code, KeyModifiers::NONE);
            render_app_to_buffer(&app, width, 24);
            assert_eq!(app.draw_cursor.get(), Some((x + 1, y).into()));
        }
        // The blank first row is not part of the checkbox hit target.
        app.on_mouse(click(x, y - 1), screen);
        assert!(!app.sampling_worker.dotnet.enabled());
        app.on_mouse(click(checkbox.right() - 1, y), screen);
        assert!(app.sampling_worker.dotnet.enabled());
        assert!(app.dotnet_confirmation.is_some());
        let buffer = render_app_to_buffer(&app, width, 24);
        assert!(find_text_position(&buffer, "[x] Enable .NET metrics").is_some());
        assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Red);
    }
}

#[test]
fn dotnet_space_toggles_immediately_and_close_keeps_collection_state() {
    for close in [KeyCode::Enter, KeyCode::Esc] {
        let mut app = tracked_app(1);
        app.toggle_dotnet();
        key(&mut app, close, KeyModifiers::NONE);
        assert!(app.dotnet_confirmation.is_none());
        assert!(!app.sampling_worker.dotnet.enabled());

        app.toggle_dotnet();
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.sampling_worker.dotnet.enabled());
        assert!(app.dotnet_confirmation.is_some());
        let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
        let permit = app.sampling_worker.dotnet.permit(&identity).unwrap();
        let generation = app.sampling_worker.dotnet.generation();
        app.snapshot.processes[0].dotnet_eventpipe = Some(generation);
        app.snapshot.processes[0].dotnet_heap_bytes = Some(123);
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(!app.sampling_worker.dotnet.enabled());
        assert!(permit.cancelled());
        assert_eq!(app.snapshot.processes[0].dotnet_heap_bytes, None);
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(!app.sampling_worker.dotnet.enabled());
        let text = render_app_to_text(&app, 80, 24);
        assert!(text.contains("[ ] Enable .NET metrics"), "{text}");
        assert!(text.contains("Stopping .NET collection"), "{text}");
        assert!(text.contains("Space Toggle  Enter/Esc Close"), "{text}");
        assert!(text.contains("Issue #119014"), "{text}");
        app.sampling_worker.dotnet.finish_suspend();
        drop(permit);
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.sampling_worker.dotnet.enabled());
        key(&mut app, close, KeyModifiers::NONE);
        assert!(app.dotnet_confirmation.is_none());
        assert!(app.sampling_worker.dotnet.enabled());
    }
}

#[test]
fn dotnet_empty_targets_toggle_with_space_and_mouse_without_authorizing_later_targets() {
    for width in [65, 80, 120] {
        for use_mouse in [false, true] {
            let (worker, requests, _) = SamplingWorker::test_pair();
            let mut app = make_test_app_with_worker(1, 10, worker);
            app.toggle_dotnet();
            let screen = Rect::new(0, 0, width, 24);
            let checkbox = crate::ui::header::dotnet_checkbox_area(screen);
            for enabled in [true, false, true] {
                render_app_to_buffer(&app, width, 24);
                if use_mouse {
                    app.on_mouse(click(checkbox.x, checkbox.y), screen);
                } else {
                    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
                }
                assert_eq!(app.sampling_worker.dotnet.enabled(), enabled);
                assert!(!app.sampling_worker.dotnet.stopping());
                assert_eq!(app.dotnet_confirmation, Some(vec![]));
                let text = render_app_to_text(&app, width, 24);
                let label = if enabled {
                    "[x] Enable .NET metrics"
                } else {
                    "[ ] Enable .NET metrics"
                };
                assert!(text.contains(label), "{text}");
                assert!(text.contains("Issue #119014"), "{text}");
                assert!(text.contains("No authorized targets."), "{text}");
                assert!(text.contains("Space Toggle  Enter/Esc Close"), "{text}");
                assert!(requests.try_recv().is_err());
            }
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            assert!(app.dotnet_confirmation.is_none());
            assert!(app.sampling_worker.dotnet.enabled());
            let identity = ProcessIdentity::from_row(&app.snapshot.processes[0]);
            app.apply_tracked_list_switch(
                app.prepare_tracked_list_switch(vec![identity.name.clone()]),
            );
            app.sampling_worker
                .dotnet
                .retain(&[identity.clone()].into_iter().collect());
            assert!(app.sampling_worker.dotnet.enabled());
            assert!(!app.sampling_worker.dotnet.allows(&identity));
            assert!(app.sampling_worker.dotnet.permit(&identity).is_none());
            assert_eq!(app.sampling_worker.dotnet.status(&identity), "Not enabled");
            assert!(
                !app.sampling_worker
                    .dotnet
                    .accepts(&identity, Some(app.sampling_worker.dotnet.generation()))
            );
            assert!(requests.try_recv().is_err());
            // A fresh explicit dialog captures and authorizes the newly tracked target.
            app.toggle_dotnet();
            assert!(!app.sampling_worker.dotnet.enabled());
            app.toggle_dotnet();
            key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
            assert!(app.sampling_worker.dotnet.allows(&identity));
        }
    }
}

#[test]
fn dotnet_checkbox_does_not_receive_clicks_through_help() {
    let mut app = tracked_app(1);
    app.toggle_dotnet();
    let screen = Rect::new(0, 0, 120, 35);
    render_app_to_buffer(&app, 120, 35);
    let checkbox = crate::ui::header::dotnet_checkbox_area(screen);
    key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    render_app_to_buffer(&app, 120, 35);
    assert_eq!(app.draw_cursor.get(), None);
    app.on_mouse(click(checkbox.x, checkbox.y), screen);
    assert!(!app.sampling_worker.dotnet.enabled());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    render_app_to_buffer(&app, 120, 35);
    assert_eq!(
        app.draw_cursor.get(),
        Some((checkbox.x + 1, checkbox.y).into())
    );
}

#[test]
fn dotnet_n_key_toggles_only_selected_untracked_lifetime_and_marker_shows_collection() {
    let mut app = make_test_app(2, 10);
    assert!(app.watch_list.is_empty());
    app.focused_panel = crate::app::FocusedPanel::Processes;
    let first = app.selected_live_process_identity().unwrap();
    for row in &app.snapshot.processes {
        app.sampling_worker
            .dotnet
            .report_managed(ProcessIdentity::from_row(row), DotNetRuntimeKind::EventPipe);
    }
    let screen = Rect::new(0, 0, 120, 35);
    let area = crate::ui::layout::main_panel_areas_for_app(screen, &app)
        .processes
        .area;
    let buffer = render_app_to_buffer(&app, 120, 35);
    let (_, y) = find_text_position(&buffer, &first.name).unwrap();
    let x = area.x + 2;
    assert!(!app.sampling_worker.dotnet.allows(&first));
    assert_eq!(buffer[(x, y)].symbol(), "N");
    assert_eq!(buffer[(x, y)].bg, ratatui::style::Color::Gray);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&first));
    assert_eq!(
        render_app_to_buffer(&app, 120, 35)[(x, y)].bg,
        crate::ui::header::DOTNET_BACKGROUND
    );
    app.sampling_worker
        .dotnet
        .report(&first, "Unavailable: connection failed".into());
    assert_eq!(
        render_app_to_buffer(&app, 120, 35)[(x, y)].bg,
        ratatui::style::Color::Gray
    );
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    app.sampling_worker
        .dotnet
        .finish_remove(std::slice::from_ref(&first));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(
        render_app_to_buffer(&app, 120, 35)[(x, y)].bg,
        crate::ui::header::DOTNET_BACKGROUND
    );
    app.sampling_worker
        .dotnet
        .report(&first, "Collecting".into());
    assert_eq!(
        render_app_to_buffer(&app, 120, 35)[(x, y)].bg,
        crate::ui::header::DOTNET_BACKGROUND
    );
    app.process_table_state.select(Some(1));
    let second = app.selected_live_process_identity().unwrap();
    assert_ne!(first, second);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&first));
    assert!(app.sampling_worker.dotnet.allows(&second));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&first));
    assert!(!app.sampling_worker.dotnet.allows(&second));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(!app.sampling_worker.dotnet.allows(&second));
    app.sampling_worker
        .dotnet
        .finish_remove(std::slice::from_ref(&second));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.sampling_worker.dotnet.allows(&second));
}

#[test]
fn dotnet_n_rejects_native_framework_and_log_view_targets() {
    let mut app = make_test_app(1, 10);
    app.focused_panel = crate::app::FocusedPanel::Processes;
    let identity = app.selected_live_process_identity().unwrap();
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(!app.sampling_worker.dotnet.allows(&identity));
    assert!(app.status.contains("No supported .NET 8+ runtime"));
    app.sampling_worker
        .dotnet
        .report_managed(identity.clone(), DotNetRuntimeKind::Framework);
    assert!(app.sampling_worker.dotnet.is_managed(&identity));
    // A detected Framework runtime must keep its PDH values without advertising
    // the EventPipe-only n action through an N marker.
    let row = app
        .snapshot
        .processes
        .iter_mut()
        .find(|row| ProcessIdentity::from_row(row) == identity)
        .unwrap();
    row.dotnet_heap_bytes = Some(4096);
    let area = crate::ui::layout::main_panel_areas_for_app(Rect::new(0, 0, 120, 35), &app)
        .processes
        .area;
    let buffer = render_app_to_buffer(&app, 120, 35);
    let (_, y) = find_text_position(&buffer, &identity.name).unwrap();
    assert_eq!(buffer[(area.x + 2, y)].symbol(), " ");
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(!app.sampling_worker.dotnet.allows(&identity));
    assert_eq!(
        app.snapshot
            .processes
            .iter()
            .find(|row| { ProcessIdentity::from_row(row) == identity })
            .unwrap()
            .dotnet_heap_bytes,
        Some(4096)
    );
    app.sampling_worker
        .dotnet
        .report_managed(identity.clone(), DotNetRuntimeKind::EventPipe);
    app.log_view_path = Some("example.log".into());
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(!app.sampling_worker.dotnet.allows(&identity));
}

fn metric_row_text(app: &crate::App) -> String {
    let buffer = render_app_to_buffer(app, 120, 35);
    let (_, y) = find_text_position(&buffer, &app.snapshot.processes[0].name).unwrap();
    (0..120)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
}

#[test]
fn dotnet_metric_cells_distinguish_off_starting_failure_and_framework_capability() {
    use crate::model::MetricColumn::*;
    for column in [
        DotNetHeapBytes,
        DotNetGcGen0HeapBytes,
        DotNetGcGen1HeapBytes,
        DotNetGcGen2HeapBytes,
        DotNetGcLohBytes,
        DotNetGcPohBytes,
        DotNetGcCommittedBytes,
        DotNetGcFragmentationBytes,
        DotNetAllocationBytesPerSec,
    ] {
        let mut app = make_test_app(1, 10);
        app.process_columns = vec![column];
        app.selected_process_column_index = 2;
        let id = ProcessIdentity::from_row(&app.snapshot.processes[0]);
        assert!(metric_row_text(&app).contains("--"));
        app.sampling_worker
            .dotnet
            .report_managed(id.clone(), DotNetRuntimeKind::Other);
        assert!(metric_row_text(&app).contains("--"));
        app.sampling_worker
            .dotnet
            .report_managed(id.clone(), DotNetRuntimeKind::Framework);
        assert!(
            metric_row_text(&app).contains("--"),
            "{:?}: {}",
            column,
            metric_row_text(&app)
        );
        assert!(!metric_row_text(&app).contains("N/A"));
        app.sampling_worker
            .dotnet
            .report_managed(id.clone(), DotNetRuntimeKind::EventPipe);
        assert!(metric_row_text(&app).contains("OFF"));
        assert!(app.sampling_worker.dotnet.enable_target(id.clone()));
        assert!(metric_row_text(&app).contains("..."));
        app.sampling_worker.dotnet.report(&id, "Collecting".into());
        // A successfully started stream can still lack this particular metric.
        assert!(metric_row_text(&app).contains("--"));
        app.sampling_worker
            .dotnet
            .report(&id, "Unavailable: denied".into());
        assert!(metric_row_text(&app).contains("--"));
        app.sampling_worker.dotnet.disable_target(&id);
        assert!(metric_row_text(&app).contains("OFF"));
        app.toggle_display_pause();
        assert!(!metric_row_text(&app).contains("OFF"));
        app.toggle_display_pause();
        app.log_view_path = Some("example.log".into());
        assert!(metric_row_text(&app).contains("--"));
        assert!(!metric_row_text(&app).contains("OFF"));
    }
}

#[test]
fn dotnet_and_gpu_measured_zero_values_are_not_missing_states() {
    use crate::model::MetricColumn::*;
    let mut app = make_test_app(1, 10);
    let id = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    app.sampling_worker
        .dotnet
        .report_managed(id, DotNetRuntimeKind::Framework);
    app.snapshot.processes[0].dotnet_heap_bytes = Some(0);
    app.snapshot.processes[0].gpu_percent = Some(0.0);
    app.snapshot.processes[0].gpu_dedicated_bytes = Some(0);
    app.snapshot.processes[0].gpu_shared_bytes = Some(0);
    for (column, expected) in [
        (DotNetHeapBytes, "0 B"),
        (GpuPercent, "0.0%"),
        (GpuDedicatedBytes, "0 B"),
        (GpuSharedBytes, "0 B"),
    ] {
        app.process_columns = vec![column];
        assert!(
            metric_row_text(&app).contains(expected),
            "{}",
            metric_row_text(&app)
        );
    }
    app.snapshot.processes[0].gpu_percent = None;
    app.snapshot.processes[0].gpu_dedicated_bytes = None;
    app.snapshot.processes[0].gpu_shared_bytes = None;
    for column in [GpuPercent, GpuDedicatedBytes, GpuSharedBytes] {
        app.process_columns = vec![column];
        assert!(metric_row_text(&app).contains("--"));
    }
}

#[test]
fn dotnet_failures_show_process_and_reason_in_footer_once_per_attempt() {
    let (worker, _requests, _tx) = SamplingWorker::test_pair();
    let mut app = make_test_app_with_worker(1, 10, worker);
    let id = ProcessIdentity::from_row(&app.snapshot.processes[0]);
    let error = "Unavailable: no .NET metrics received within 5s";
    assert!(app.sampling_worker.dotnet.enable_target(id.clone()));
    app.sampling_worker.dotnet.report(&id, error.into());
    assert!(app.poll_sample_results().unwrap());
    assert!(app.status.contains(&id.name));
    assert!(app.status.contains(&id.pid.to_string()));
    assert!(app.status.contains("no .NET metrics received within 5s"));
    assert!(render_app_to_text(&app, 160, 35).contains("no .NET metrics received within 5s"));
    app.status = "New user action".into();
    app.sampling_worker.dotnet.report(&id, error.into());
    assert!(!app.poll_sample_results().unwrap());
    assert_eq!(app.status, "New user action");
    app.sampling_worker.dotnet.disable_target(&id);
    app.sampling_worker
        .dotnet
        .finish_remove(std::slice::from_ref(&id));
    assert!(app.sampling_worker.dotnet.enable_target(id.clone()));
    app.sampling_worker.dotnet.report(&id, error.into());
    assert!(app.poll_sample_results().unwrap());
    assert!(app.status.contains("no .NET metrics received within 5s"));
}
