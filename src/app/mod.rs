pub(crate) mod actions;
pub(crate) mod clipboard;
pub(crate) mod context_menu;
pub(crate) mod export;
pub(crate) mod feedback;
pub(crate) mod file_users;
pub(crate) mod graph_csv;
pub(crate) mod help;
pub(crate) mod log_format;
pub(crate) mod logs;
pub(crate) mod navigation;
pub(crate) mod network;
pub(crate) mod open_files;
pub(crate) mod path_completion;
pub(crate) mod profiles;
pub(crate) mod scheduling;
pub(crate) mod state;
pub(crate) mod system_info;
pub(crate) mod text_input;
pub(crate) mod threads;

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::terminal::{AppTerminal, draw_frame};

use crate::ui::{
    column_picker_page_size_for_screen, cpu_core_dialog_page_size_for_screen, draw,
    graph_reorder_page_size_for_screen, help_page_size_for_screen,
    investigation_profiles_page_size_for_screen,
    layout::{
        MainPanelAreas, details_samples_row_capacity, details_samples_summary_visibility,
        graph_workspace_layout,
    },
    main_panel_areas_for_app, process_info_page_size_for_screen,
};

const EVENT_POLL_SLICE: Duration = Duration::from_millis(50);
pub(crate) const SAMPLING_INTERVAL_SECONDS: u64 = 1;

pub(crate) use profiles::{InvestigationProfilesView, ProfileNameInputPurpose};
pub(crate) use state::AbComparison;
pub(crate) use state::AbComparisonPoint;
pub(crate) use state::App;
pub(crate) use state::AppActivity;
pub(crate) use state::DetailsMetric;
pub(crate) use state::DetailsSampleViewState;
#[cfg(test)]
pub(crate) use state::DetailsTarget;
pub(crate) use state::FocusedPanel;
#[cfg(test)]
pub(crate) use state::GRAPH_LIMIT;
pub(crate) use state::GRAPH_SLOT_MIN_HEIGHT;
pub(crate) use state::GRAPH_SLOT_MIN_WIDTH;
pub(crate) use state::GraphDisplayMode;
pub(crate) use state::GraphHoverTarget;
pub(crate) use state::GraphId;
pub(crate) use state::GraphPanDrag;
pub(crate) use state::GraphPanDragButton;
pub(crate) use state::GraphSample;
pub(crate) use state::GraphSlot;
pub(crate) use state::GraphSlotLayout;
pub(crate) use state::GraphSourceState;
pub(crate) use state::GraphValueFormat;
#[cfg(test)]
pub(crate) use state::PROCESS_INFO_DEBOUNCE;
pub(crate) use state::ProcessInfoFocus;
pub(crate) use state::ProcessInfoTab;
pub(crate) use state::ProcessLifecycle;
pub(crate) use state::ProcessPanelHeight;
pub(crate) use state::ProcessPanelResizeDrag;
pub(crate) use state::ProcessViewMode;
pub(crate) use state::ResourcePanel;
#[cfg(test)]
pub(crate) use state::SAMPLE_STALE_AFTER_SECONDS;
pub(crate) use state::SampleFreshness;
#[cfg(test)]
pub(crate) use state::VisibleProcessEntry;
pub(crate) use state::VisibleProcessRow;

pub(crate) fn run_tui(terminal: &mut AppTerminal, app: &mut App) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut screen_size = terminal.size()?;
    let mut dirty = true;
    let mut loop_trace = LoopTrace::from_env();
    let mut last_sample_freshness = app.sample_freshness();

    loop {
        if crate::platform::termination_requested() {
            app.confirm_quit()?;
            break;
        }

        dirty |= app.enforce_recording_duration_limit();
        let trace_selected = app.process_table_state.selected();
        let trace_start = Instant::now();
        let sample_dirty = app.poll_sample_results()?;
        if sample_dirty && let Some(trace) = loop_trace.as_mut() {
            trace.log(
                "sample",
                trace_start.elapsed(),
                trace_selected,
                trace_selected,
            );
        }
        dirty |= sample_dirty;
        dirty |= app.poll_process_info_results()?;
        dirty |= app.poll_open_files_results()?;
        dirty |= app.poll_process_modules_results()?;
        dirty |= app.poll_process_environment_results()?;
        dirty |= app.poll_network_results();
        dirty |= app.poll_file_users_results();
        dirty |= app.poll_scheduling_results();
        dirty |= app.poll_graph_csv();
        dirty |= app.poll_threads();
        dirty |= app.poll_log_workers();
        dirty |= app.request_due_process_info()?;
        dirty |= app.request_due_open_files_at(Instant::now())?;
        dirty |= app.refresh_status_feedback(Instant::now());
        let sample_freshness = app.sample_freshness();
        if sample_freshness != last_sample_freshness {
            last_sample_freshness = sample_freshness;
            dirty = true;
        }

        if dirty {
            screen_size = terminal.size()?;
            sync_layout_state(app, Rect::new(0, 0, screen_size.width, screen_size.height));
            let trace_selected = app.process_table_state.selected();
            let trace_start = Instant::now();
            draw_frame(terminal, |frame| draw(frame, app))?;
            if let Some(trace) = loop_trace.as_mut() {
                trace.log(
                    "draw",
                    trace_start.elapsed(),
                    trace_selected,
                    trace_selected,
                );
            }
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
        let timeout = app
            .process_modules_poll_timeout()
            .map(|modules_timeout| timeout.min(modules_timeout))
            .unwrap_or(timeout);
        let timeout = app
            .process_environment_poll_timeout()
            .map(|environment_timeout| timeout.min(environment_timeout))
            .unwrap_or(timeout);

        let wait = timeout.min(EVENT_POLL_SLICE);
        if event::poll(wait)? {
            if crate::platform::termination_requested() {
                app.confirm_quit()?;
                break;
            }
            match event::read()? {
                Event::Key(key) => {
                    let trace_before = app.process_table_state.selected();
                    let trace_start = Instant::now();
                    app.on_key(key)?;
                    if let Some(trace) = loop_trace.as_mut() {
                        trace.log(
                            "key",
                            trace_start.elapsed(),
                            trace_before,
                            app.process_table_state.selected(),
                        );
                    }
                    if app.should_quit {
                        break;
                    }
                    dirty = true;
                }
                Event::Mouse(mouse) => {
                    let outcome = handle_mouse_event(
                        app,
                        mouse,
                        Rect::new(0, 0, screen_size.width, screen_size.height),
                    );
                    dirty |= outcome.dirty;
                    if outcome.should_quit {
                        break;
                    }
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
            dirty |= app.request_sample()? && !app.is_display_paused();
            last_tick = Instant::now();
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MouseEventOutcome {
    pub(crate) dirty: bool,
    pub(crate) should_quit: bool,
}

pub(crate) fn handle_mouse_event(
    app: &mut App,
    mouse: MouseEvent,
    screen_area: Rect,
) -> MouseEventOutcome {
    let previous_hover = (
        app.graph_hovered_target,
        app.cpu_per_core_hovered,
        app.process_view_mode_hovered,
        app.process_tracked_only_hovered,
        app.process_show_exited_hovered,
        app.shortcut_hovered,
        app.context_menu.as_ref().and_then(|menu| menu.hovered),
        app.process_disclosure_hovered.clone(),
        app.main_menu_hovered,
        app.header_action_hovered,
        (
            app.process_column_control_hovered,
            app.column_resize_hovered,
        ),
        app.process_panel_resize_hovered,
    );
    app.on_mouse(mouse, screen_area);
    let dirty = mouse.kind != MouseEventKind::Moved
        || previous_hover
            != (
                app.graph_hovered_target,
                app.cpu_per_core_hovered,
                app.process_view_mode_hovered,
                app.process_tracked_only_hovered,
                app.process_show_exited_hovered,
                app.shortcut_hovered,
                app.context_menu.as_ref().and_then(|menu| menu.hovered),
                app.process_disclosure_hovered.clone(),
                app.main_menu_hovered,
                app.header_action_hovered,
                (
                    app.process_column_control_hovered,
                    app.column_resize_hovered,
                ),
                app.process_panel_resize_hovered,
            );
    MouseEventOutcome {
        dirty,
        should_quit: app.should_quit,
    }
}

struct LoopTrace {
    start: Instant,
    writer: BufWriter<File>,
}

impl LoopTrace {
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("WINPROC_TUI_TRACE_LOOP")?;
        let path = if path == "1" {
            PathBuf::from("logs/loop-trace.csv")
        } else {
            PathBuf::from(path)
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).ok()?;
        }
        let mut writer = BufWriter::new(File::create(path).ok()?);
        writeln!(
            writer,
            "elapsed_ms,event,duration_us,selected_before,selected_after"
        )
        .ok()?;
        Some(Self {
            start: Instant::now(),
            writer,
        })
    }

    fn log(
        &mut self,
        event: &str,
        duration: Duration,
        selected_before: Option<usize>,
        selected_after: Option<usize>,
    ) {
        let _ = writeln!(
            self.writer,
            "{},{},{},{},{}",
            self.start.elapsed().as_millis(),
            event,
            duration.as_micros(),
            selected_before
                .map(|value| value.to_string())
                .unwrap_or_default(),
            selected_after
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
    }
}

pub(crate) fn sync_layout_state(app: &mut App, screen_area: Rect) {
    let resized = app.last_screen_area != screen_area;
    app.set_screen_area(screen_area);
    if resized && app.is_main_menu_open() && app.main_menu_section == state::MainMenuSection::More {
        app.close_main_menu();
    }
    app.cancel_process_panel_resize_if_unavailable();
    app.sync_graph_layout_visibility();
    let panels = main_panel_areas_for_app(screen_area, app);
    let process_height_changed = app.process_panel_body_capacity != panels.processes.body_capacity;
    app.set_process_table_layout(panels.processes.page_size, panels.processes.body_capacity);
    if resized || process_height_changed {
        app.reveal_active_graph();
    }
    app.set_details_sample_page_size(details_samples_page_size_for_app(&panels, app));
    app.set_help_page_size(help_page_size_for_screen(screen_area));
    app.set_column_picker_page_size(column_picker_page_size_for_screen(screen_area));
    let graph_reorder_page_size = graph_reorder_page_size_for_screen(screen_area, app);
    app.set_graph_reorder_page_size(graph_reorder_page_size);
    app.set_log_list_page_size(crate::ui::log_list_page_size_for_screen(screen_area));
    app.set_process_info_page_size(process_info_page_size_for_screen(screen_area));
    if resized
        && app.show_process_info_dialog
        && app.process_info_tab == ProcessInfoTab::Scheduling
        && app.process_info_focus == ProcessInfoFocus::Content
    {
        if app.scheduling.affinity_focused {
            app.select_affinity_cpu(app.scheduling.affinity_selected);
        } else {
            app.select_priority(app.scheduling.selected);
        }
    }
    app.sync_network_layout(true, screen_area);
    app.sync_file_users_layout(screen_area);
    app.sync_network_layout(false, screen_area);
    let cpu_core_page_size = cpu_core_dialog_page_size_for_screen(screen_area, app);
    app.set_cpu_core_page_size(cpu_core_page_size);
    app.set_investigation_profiles_page_size(investigation_profiles_page_size_for_screen(
        screen_area,
        app,
    ));
    app.ensure_visible_panel_focus();
    app.clamp_process_table_state();
}

fn details_samples_page_size_for_app(panels: &MainPanelAreas, app: &App) -> usize {
    if !app.effective_show_samples_panel() {
        return 1;
    }
    let Some(details) = panels.details else {
        return 1;
    };
    let layout = graph_workspace_layout(details, app);
    let Some(samples) = layout.samples else {
        return 1;
    };
    let comparison = app.active_ab_comparison();
    details_samples_row_capacity(
        samples.height.saturating_sub(2),
        details_samples_summary_visibility(comparison),
        true,
    )
}
