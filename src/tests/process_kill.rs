use super::support::{
    find_text_position, left_click, make_test_app, render_app_to_buffer, render_app_to_text,
};
use crate::app::process_kill::{KillJob, KillOutcome};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn process_kill_slow_batch_keeps_input_rendering_and_fixed_targets() {
    let mut app = make_test_app(3, 10);
    app.select_first_row();
    assert!(app.request_process_kill_confirmation());
    let target = app.process_kill_targets[0].clone();
    app.process_kill_targets = vec![target.clone(), target.clone(), target];
    let originals = app.process_kill_targets.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    app.process_kill_job = Some(KillJob::spawn_with(originals.clone(), move |_| {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        KillOutcome::Killed
    }));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let text = render_app_to_text(&app, 120, 40);
    assert!(text.contains("KILL IN PROGRESS"));
    assert!(text.contains("0/3 processed"));
    for key in [KeyCode::Enter, KeyCode::Esc, KeyCode::Delete] {
        app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();
    }
    assert!(app.show_process_kill_confirmation);
    app.confirm_process_kill();
    assert!(app.request_process_kill_confirmation());
    assert_eq!(app.process_kill_targets, originals);
    app.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.process_kill_scroll, 2);
    assert!(render_app_to_text(&app, 80, 24).contains("KILL IN PROGRESS"));
    let screen = Rect::new(0, 0, 120, 40);
    let buffer = render_app_to_buffer(&app, 120, 40);
    assert!(
        !app.shortcut_map
            .borrow()
            .regions
            .iter()
            .any(|region| region.key.code == KeyCode::Enter)
    );
    let (x, y) = find_text_position(&buffer, "Scroll").unwrap();
    app.on_mouse(left_click(x, y), screen);
    assert_eq!(app.process_kill_scroll, 1);
    assert!(app.show_process_kill_confirmation);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 60,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(app.process_kill_scroll, 2);
    app.on_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        screen,
    );
    assert_eq!(app.process_kill_scroll, 2);
    for completed in 1..=3 {
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while app
            .process_kill_job
            .as_ref()
            .unwrap()
            .outcomes
            .iter()
            .flatten()
            .count()
            < completed
        {
            app.poll_process_kill();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(render_app_to_text(&app, 120, 40).contains(&format!("{completed}/3 processed")));
        if completed < 3 {
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }
    let text = render_app_to_text(&app, 120, 40);
    assert!(text.contains("KILL RESULTS"));
    assert!(text.contains("Enter/Esc"));
    assert!(!text.contains("Continue?"));
    let buffer = render_app_to_buffer(&app, 120, 40);
    let (x, y) = find_text_position(&buffer, "Close").unwrap();
    app.on_mouse(left_click(x, y), screen);
    assert!(!app.show_process_kill_confirmation);
    assert!(app.process_kill_job.is_none());
}
