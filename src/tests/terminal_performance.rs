use jiff::SignedDuration;
use std::{
    fs::File,
    io::{self, BufWriter, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};

use super::support::make_test_app;
use crate::{
    App,
    app::{DetailsMetric, FocusedPanel, GraphSlot, GraphSlotLayout, sync_layout_state},
    model::ProcessHistory,
    terminal::{buffered_backend, draw_frame},
    ui,
};

#[derive(Clone, Copy, Debug)]
enum OutputMode {
    Original,
    Buffered,
    Synchronized,
    Current,
}

fn fixture(area: Rect, samples: usize, fit_all: bool) -> App {
    let mut app = make_test_app(100, 10);
    app.set_screen_area(area);
    if samples > 0 {
        let identity = app.selected_visible_process_identity().unwrap();
        for metric in [
            DetailsMetric::Private,
            DetailsMetric::Workset,
            DetailsMetric::CpuPercent,
            DetailsMetric::IoRead,
        ] {
            assert!(app.add_or_reveal_graph_source(
                GraphSlot::process(identity.clone(), metric),
                FocusedPanel::Processes,
            ));
        }
        app.graph_slot_layout = GraphSlotLayout::TwoColumns;
        app.show_samples_panel = true;
        app.process_history = ProcessHistory::default();
        let tracked_names = std::collections::HashSet::from([identity.name.to_ascii_lowercase()]);
        let base = app.snapshot.captured_at - SignedDuration::from_secs(samples as i64 - 1);
        for offset in 0..samples {
            let process = &mut app.snapshot.processes[0];
            process.private_bytes = Some(offset as u64 * 1024);
            process.workset_bytes = Some(offset as u64 * 2048);
            process.cpu_percent = Some((offset % 100) as f64);
            process.io_read_bytes_per_sec = Some(offset as u64 * 4096);
            app.snapshot.captured_at = base + SignedDuration::from_secs(offset as i64);
            app.process_history.record_snapshot(
                app.snapshot.captured_at,
                std::slice::from_ref(process),
                &tracked_names,
            );
        }
        app.graph_show_all_samples = fit_all;
        app.select_details_sample_latest();
    }
    sync_layout_state(&mut app, area);
    app
}

fn measure<W: Write>(
    backend: CrosstermBackend<W>,
    app: &mut App,
    mode: OutputMode,
    round: usize,
    csv: &mut impl Write,
) -> io::Result<()> {
    let area = app.last_screen_area;
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?;
    let synchronized = matches!(mode, OutputMode::Synchronized | OutputMode::Current);
    let mut present = |app: &App| -> io::Result<(Duration, Duration)> {
        let mut render_time = Duration::ZERO;
        let render = |frame: &mut ratatui::Frame<'_>| {
            let start = Instant::now();
            ui::draw(frame, app);
            render_time = start.elapsed();
        };
        let start = Instant::now();
        if synchronized {
            draw_frame(&mut terminal, render)?;
        } else {
            terminal.draw(render)?;
        }
        Ok((start.elapsed(), render_time))
    };
    present(app)?;
    // Warm up both dialog states before recording. Sleep outside the measured interval
    // to give the terminal reader time to drain output between transitions.
    for index in 0..24 {
        let opening = index % 2 == 0;
        let key = if opening {
            KeyCode::Char('c')
        } else {
            KeyCode::Esc
        };
        app.on_key(KeyEvent::new(key, KeyModifiers::NONE)).unwrap();
        assert_eq!(app.show_column_picker, opening);
        let (total, render) = present(app)?;
        if index >= 4 {
            writeln!(
                csv,
                "{mode:?},{round},{opening},{},{}",
                total.as_nanos(),
                render.as_nanos()
            )?;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    csv.flush()
}

// Run the release test executable with --ignored --nocapture --test-threads=1 in a
// dedicated terminal/ConPTY, not redirected stdout. Set WINPROC_TUI_FRAME_BENCH_CSV
// to an ignored/local output path. The fixture is frozen; timers cover widget
// generation plus buffer diffing and real stdout writes, not physical presentation.
#[test]
#[ignore = "manual real-terminal comparison; requires WINPROC_TUI_FRAME_BENCH_CSV"]
fn perf_modal_terminal_output() {
    assert!(io::stdout().is_terminal(), "requires a terminal stdout");
    let path = std::env::var_os("WINPROC_TUI_FRAME_BENCH_CSV").expect("CSV output path required");
    let mut csv = BufWriter::new(File::create(path).unwrap());
    writeln!(
        csv,
        "width,height,samples,fit_all,mode,round,opening,total_ns,render_ns"
    )
    .unwrap();
    let (width, height) = crossterm::terminal::size().unwrap();
    assert!(
        width >= 160 && height >= 50,
        "use at least 160x50 for four visible Graphs"
    );
    let area = Rect::new(0, 0, width, height);
    execute!(io::stdout(), EnterAlternateScreen).unwrap();
    let result = (|| -> io::Result<()> {
        for (samples, fit_all) in [(0, false), (60, false), (7200, false), (7200, true)] {
            let mut app = fixture(area, samples, fit_all);
            for round in 0..3 {
                let modes = [
                    OutputMode::Original,
                    OutputMode::Buffered,
                    OutputMode::Synchronized,
                    OutputMode::Current,
                ];
                for offset in 0..modes.len() {
                    let mode = modes[(offset + round) % modes.len()];
                    let mut rows = Vec::new();
                    match mode {
                        OutputMode::Original | OutputMode::Synchronized => measure(
                            CrosstermBackend::new(io::stdout()),
                            &mut app,
                            mode,
                            round,
                            &mut rows,
                        )?,
                        OutputMode::Buffered | OutputMode::Current => measure(
                            buffered_backend(io::stdout()),
                            &mut app,
                            mode,
                            round,
                            &mut rows,
                        )?,
                    }
                    for row in String::from_utf8(rows).unwrap().lines() {
                        writeln!(csv, "{width},{height},{samples},{fit_all},{row}")?;
                    }
                    csv.flush()?;
                }
            }
        }
        Ok(())
    })();
    execute!(
        io::stdout(),
        crossterm::terminal::EndSynchronizedUpdate,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )
    .unwrap();
    result.unwrap();
}
