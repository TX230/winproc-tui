pub(crate) mod actions;
pub(crate) mod clipboard;
pub(crate) mod export;
pub(crate) mod logs;
pub(crate) mod navigation;
pub(crate) mod path_completion;
pub(crate) mod state;

use std::{
    io::Stdout,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};

use crate::ui::{
    column_picker_page_size_for_screen, details_slot_areas_for_screen, draw,
    help_page_size_for_screen,
    layout::{details_samples_area, details_samples_row_capacity},
    open_files_page_size_for_screen, process_table_area_for_screen, process_table_page_size,
};

pub(crate) use state::AbComparison;
pub(crate) use state::AbComparisonPoint;
pub(crate) use state::App;
pub(crate) use state::AppActivity;
pub(crate) use state::DetailsMetric;
#[cfg(test)]
pub(crate) use state::DetailsTarget;
pub(crate) use state::FocusedPanel;
pub(crate) use state::GRAPH_SLOT_MIN_HEIGHT;
pub(crate) use state::GraphPanDrag;
pub(crate) use state::GraphPanDragButton;
pub(crate) use state::GraphSample;
pub(crate) use state::GraphSlot;
pub(crate) use state::InfoPanelMode;
pub(crate) use state::LogDirSelection;
#[cfg(test)]
pub(crate) use state::PROCESS_INFO_DEBOUNCE;
pub(crate) use state::ProcessLifecycle;
pub(crate) use state::QuitConfirmSelection;
pub(crate) use state::RecordingOverwriteSelection;
pub(crate) use state::RecordingPathSelection;
pub(crate) use state::SettingsSelection;
pub(crate) use state::TrackedRemoveSelection;
#[cfg(test)]
pub(crate) use state::VisibleProcessEntry;
pub(crate) use state::VisibleProcessRow;

pub(crate) fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut screen_size = terminal.size()?;
    let mut dirty = true;

    loop {
        dirty |= app.poll_sample_results()?;
        dirty |= app.poll_process_info_results()?;
        dirty |= app.poll_open_files_results()?;
        dirty |= app.poll_log_workers();
        dirty |= app.request_due_process_info()?;

        if dirty {
            screen_size = terminal.size()?;
            sync_layout_state(app, Rect::new(0, 0, screen_size.width, screen_size.height));
            terminal.draw(|frame| draw(frame, app))?;
            dirty = false;
        }

        let timeout_until_tick = app
            .tick_interval()
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        let timeout = if app.sampling_in_progress {
            timeout_until_tick.min(Duration::from_millis(50))
        } else {
            timeout_until_tick
        };
        let timeout = app
            .process_info_poll_timeout()
            .map(|process_info_timeout| timeout.min(process_info_timeout))
            .unwrap_or(timeout);
        let timeout = app
            .open_files_poll_timeout()
            .map(|open_files_timeout| timeout.min(open_files_timeout))
            .unwrap_or(timeout);

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    app.on_key(key)?;
                    if app.should_quit {
                        break;
                    }
                    dirty = true;
                }
                Event::Mouse(mouse) => {
                    app.on_mouse(
                        mouse,
                        Rect::new(0, 0, screen_size.width, screen_size.height),
                    );
                    dirty = true;
                }
                Event::Resize(width, height) => {
                    screen_size.width = width;
                    screen_size.height = height;
                    dirty = true;
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= app.tick_interval() {
            app.request_sample()?;
            last_tick = Instant::now();
            dirty = true;
        }
    }

    Ok(())
}

fn sync_layout_state(app: &mut App, screen_area: Rect) {
    app.set_screen_area(screen_area);
    app.close_graph_slots_that_do_not_fit();
    let process_area = process_table_area_for_screen(screen_area, app.show_details);
    let process_page_size = process_table_page_size(process_area);
    let process_page_size = if app.has_visible_tracked_total_row() && process_page_size > 1 {
        process_page_size.saturating_sub(1)
    } else {
        process_page_size
    };
    app.set_process_page_size(process_page_size);
    app.set_details_sample_page_size(details_samples_page_size_for_app(screen_area, app));
    app.set_help_page_size(help_page_size_for_screen(screen_area));
    app.set_column_picker_page_size(column_picker_page_size_for_screen(screen_area));
    app.set_log_list_page_size(crate::ui::log_list_page_size_for_screen(screen_area));
    app.set_open_files_page_size(open_files_page_size_for_screen(screen_area, app));
    app.ensure_visible_panel_focus();
    app.clamp_process_table_state();
}

fn details_samples_page_size_for_app(screen_area: Rect, app: &App) -> usize {
    if !app.show_samples_panel {
        return 1;
    }
    let slot_count = app.active_graph_slot_count().max(1);
    let slot_areas = details_slot_areas_for_screen(screen_area, app.show_details, slot_count);
    let Some(slot) = slot_areas.get(app.active_graph_visible_index()).copied() else {
        return 1;
    };
    let samples = details_samples_area(slot, app.show_sample_delta);
    let inner = samples.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    details_samples_row_capacity(
        inner.height,
        app.active_ab_comparison().is_some(),
        app.active_graph_slot_count() <= 1,
    )
}
