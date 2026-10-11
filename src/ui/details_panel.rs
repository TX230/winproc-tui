use std::borrow::Cow;

use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Chart, Dataset, GraphType, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Widget,
    },
};

use crate::{
    App,
    app::{
        AbComparison, AbComparisonPoint, FocusedPanel, GraphDisplayMode, GraphHoverTarget,
        GraphSample, GraphSlot, GraphValueFormat,
    },
    ui::{
        Theme,
        format::{
            format_compact_bytes, format_compact_bytes_with_precision, format_integer,
            format_io_rate, format_mb_per_sec, format_signed_integer, format_signed_io_rate,
        },
        layout::{
            GraphCardLayout, GraphSpanControlAreas, GraphWorkspaceLayout, details_graph_rows,
            details_samples_content_layout, details_samples_summary_visibility,
            graph_shared_control_areas, graph_workspace_layout, graph_workspace_title_label,
        },
        widgets::block::{graph_card_block, graph_workspace_block, panel_block_focused},
    },
};

const SAMPLE_METRIC_VALUE_WIDTH: usize = 15;
const SAMPLE_DELTA_WIDTH: usize = 15;

struct GraphCardRenderData {
    samples: Vec<GraphSample>,
    display_samples: Option<Vec<GraphSample>>,
    display_mode: GraphDisplayMode,
    metric: GraphValueFormat,
    bounds: (i64, i64),
    segments: Vec<Vec<(f64, f64)>>,
    y_min: f64,
    y_max: f64,
}

impl GraphCardRenderData {
    fn plotted_samples(&self) -> &[GraphSample] {
        self.display_samples.as_deref().unwrap_or(&self.samples)
    }
}

fn graph_frame_times(app: &App) -> Cow<'_, [DateTime<Local>]> {
    if !app.log_view_frame_times.is_empty() {
        Cow::Borrowed(app.log_view_frame_times.as_slice())
    } else {
        Cow::Owned(
            app.display_system_history()
                .samples_iter()
                .map(|sample| sample.captured_at)
                .collect(),
        )
    }
}

pub(crate) fn draw_details_panel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    if app.graph_entries.is_empty() {
        let lines = vec![Line::from(Span::styled(
            "No graph metrics selected",
            Style::default().fg(theme.muted),
        ))];
        frame.render_widget(details_paragraph(lines, theme), area);
        return;
    }

    let layout = graph_workspace_layout(area, app);
    draw_graph_shared_controls(frame, layout.controls, app, layout.columns, theme);
    let graph_focused = app.panel_has_focus(FocusedPanel::DetailsGraph);
    frame.render_widget(
        graph_workspace_block(
            graph_workspace_title(app, theme, graph_focused, layout.span_controls),
            theme,
            graph_focused,
        ),
        layout.graph_slots,
    );

    let bounds = graph_bounds(
        app.effective_graph_time_span_seconds(),
        app.effective_graph_time_offset_seconds(),
    );
    let time_reference_at = app.graph_time_reference_at();
    let frame_times = graph_frame_times(app);
    let frame_times = (!frame_times.is_empty()).then_some(frame_times.as_ref());
    let prepared_cards = layout
        .graph_cards
        .iter()
        .filter_map(|card| {
            let entry = app.graph_entry(card.ordinal)?;
            Some((
                card.ordinal,
                prepare_graph_card_render_data(
                    app,
                    &entry.source,
                    entry.display_mode,
                    bounds,
                    time_reference_at,
                    frame_times,
                ),
            ))
        })
        .collect::<Vec<_>>();
    let common_y_label_width = prepared_cards
        .iter()
        .map(|(_, data)| y_axis_label_width(&y_axis_labels(data.y_min, data.y_max, data.metric)))
        .max()
        .unwrap_or(1);
    let active_index = app.active_graph_index();
    let active_samples = prepared_cards
        .iter()
        .find(|(index, _)| Some(*index) == active_index)
        .map(|(_, data)| Cow::Borrowed(data.samples.as_slice()))
        .or_else(|| {
            app.active_graph_slot()
                .map(|slot| Cow::Owned(app.graph_slot_samples(slot)))
        })
        .unwrap_or(Cow::Borrowed(&[]));
    let selected_sample_time = active_samples
        .get(app.details_sample_selected)
        .map(|sample| sample.captured_at);
    for card in &layout.graph_cards {
        let Some(entry) = app.graph_entry(card.ordinal) else {
            continue;
        };
        let Some((_, data)) = prepared_cards
            .iter()
            .find(|(index, _)| *index == card.ordinal)
        else {
            continue;
        };
        render_graph_card(
            frame,
            card,
            graph_slot_title_line(
                &entry.source,
                card.ordinal,
                data.samples.as_slice(),
                data.metric,
                app.active_ab_comparison(),
                app.active_graph_id == Some(entry.id),
                theme,
            ),
            data,
            app,
            theme,
            common_y_label_width,
            selected_sample_time,
            layout.compact,
        );
    }
    render_graph_workspace_scrollbar(frame, &layout, app.graph_scroll_row, graph_focused, theme);
    if let Some(samples_area) = layout.samples {
        draw_active_samples_inspector(
            frame,
            samples_area,
            app,
            active_samples.as_ref(),
            frame_times,
            theme,
        );
    }
}

pub(crate) fn graph_y_axis_label_width(app: &App) -> usize {
    let indices = crate::ui::main_panel_areas_for_app(app.last_screen_area, app)
        .details
        .map(|area| graph_workspace_layout(area, app))
        .map(|layout| {
            layout
                .graph_cards
                .into_iter()
                .map(|card| card.ordinal)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| app.active_graph_index().into_iter().collect());
    graph_y_axis_label_width_for_indices(app, &indices)
}

fn graph_y_axis_label_width_for_indices(app: &App, indices: &[usize]) -> usize {
    let bounds = graph_bounds(
        app.effective_graph_time_span_seconds(),
        app.effective_graph_time_offset_seconds(),
    );
    let frame_times = graph_frame_times(app);
    let frame_times = (!frame_times.is_empty()).then_some(frame_times.as_ref());
    indices
        .iter()
        .filter_map(|index| app.graph_entry(*index))
        .map(|entry| {
            let data = prepare_graph_card_render_data(
                app,
                &entry.source,
                entry.display_mode,
                bounds,
                app.graph_time_reference_at(),
                frame_times,
            );
            y_axis_label_width(&y_axis_labels(data.y_min, data.y_max, data.metric))
        })
        .max()
        .unwrap_or(1)
}

fn prepare_graph_card_render_data(
    app: &App,
    slot: &GraphSlot,
    display_mode: GraphDisplayMode,
    bounds: (i64, i64),
    time_reference_at: Option<DateTime<Local>>,
    frame_times: Option<&[DateTime<Local>]>,
) -> GraphCardRenderData {
    let samples = app.graph_slot_samples(slot);
    let metric = slot.value_format();
    let display_samples = (display_mode == GraphDisplayMode::MovingAverage5)
        .then(|| moving_average_samples(&samples, frame_times));
    let plotted_samples = display_samples.as_deref().unwrap_or(&samples);
    let segment_frame_times = match display_mode {
        GraphDisplayMode::Raw => {
            (!app.log_view_frame_times.is_empty()).then_some(app.log_view_frame_times.as_slice())
        }
        GraphDisplayMode::MovingAverage5 => frame_times,
    };
    let segments = if display_mode == GraphDisplayMode::MovingAverage5 {
        chart_segments_preserving_sample_gaps(
            plotted_samples,
            bounds,
            time_reference_at,
            segment_frame_times,
        )
    } else {
        chart_segments(
            plotted_samples,
            bounds,
            time_reference_at,
            segment_frame_times,
        )
    };
    let stats = graph_stats_for_values(
        &samples,
        app.graph_slot_peak(slot),
        segments.iter().flatten().map(|(_, value)| *value),
    );
    let (y_min, y_max) = graph_y_bounds(&stats, app.graph_y_axis_zero_min);
    GraphCardRenderData {
        samples,
        display_samples,
        display_mode,
        metric,
        bounds,
        segments,
        y_min,
        y_max,
    }
}

// Graph rendering combines independent layout, series, and shared-time inputs.
#[allow(clippy::too_many_arguments)]
fn render_graph_card(
    frame: &mut ratatui::Frame<'_>,
    card: &GraphCardLayout,
    title: Line<'static>,
    data: &GraphCardRenderData,
    app: &App,
    theme: Theme,
    y_label_width: usize,
    selected_sample_time: Option<DateTime<Local>>,
    compact: bool,
) {
    let active = app.active_graph_id == Some(card.id);
    // Comparison values get their own border row so a long target cannot clip digits.
    let mut title = title;
    let mut comparison = title.spans.split_off(title.spans.len().saturating_sub(2));
    if let Some(first) = comparison.first_mut() {
        first.content = "B-A: ".into();
    }
    comparison.push(Span::raw(exact_unit_suffix(data.metric)));
    let available = card.display_mode.x.saturating_sub(card.title.x) as usize;
    if title.width() > available
        && let Some(entry) = app.graph_entry_by_id(card.id)
        && let Some(identity) = entry.source.process_identity()
    {
        title = Line::from(Span::styled(
            fit_graph_label(
                &format!(
                    "Slot#{} · PID {} · {}",
                    card.ordinal + 1,
                    identity.pid,
                    entry.source.graph_title_metric_label()
                ),
                available,
            ),
            Style::default().fg(if active { theme.text } else { theme.muted }),
        ));
    }
    let block = graph_card_block(title, theme, active).title_bottom(Line::from(comparison));
    let inner = block.inner(card.area);
    frame.render_widget(block, card.area);

    if compact || inner.height < 8 || inner.width < 30 {
        let lines = vec![Line::from(Span::styled(
            "Resize terminal to view Graph",
            Style::default().fg(theme.muted),
        ))];
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.muted).bg(theme.panel)),
            inner,
        );
    } else if data.samples.is_empty() {
        let lines = vec![Line::from(Span::styled(
            "No samples available",
            Style::default().fg(theme.muted),
        ))];
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.muted).bg(theme.panel)),
            inner,
        );
    } else {
        draw_graph_content(
            frame,
            card.plot,
            data.plotted_samples(),
            data.metric,
            data.display_mode,
            app.log_view_interval_seconds,
            selected_sample_time,
            app.graph_time_reference_at(),
            data.bounds,
            &data.segments,
            (data.y_min, data.y_max),
            app.active_ab_comparison(),
            active,
            theme,
            y_label_width,
        );
    }
    let display_mode_label = app
        .graph_entry_by_id(card.id)
        .map(|entry| entry.display_mode.button_label())
        .unwrap_or("[RAW]");
    frame.render_widget(
        Paragraph::new(format!(" {display_mode_label:<5} ")).style(graph_button_style(
            theme,
            app.graph_hovered_target == Some(GraphHoverTarget::DisplayMode(card.id)),
            true,
        )),
        card.display_mode,
    );
    frame.render_widget(
        Paragraph::new(format!(" {} ", card.remove_label)).style(graph_button_style(
            theme,
            app.graph_hovered_target == Some(GraphHoverTarget::Remove(card.id)),
            true,
        )),
        card.remove,
    );
}

fn render_graph_workspace_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    layout: &GraphWorkspaceLayout,
    scroll_row: usize,
    focused: bool,
    theme: Theme,
) {
    let Some(area) = layout.graph_scrollbar else {
        return;
    };
    let mut state = ScrollbarState::new(layout.total_rows)
        .position(samples_scrollbar_position(
            layout.total_rows,
            layout.visible_rows,
            scroll_row,
        ))
        .viewport_content_length(layout.visible_rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(theme.muted).bg(theme.panel))
        .thumb_style(
            Style::default()
                .fg(if focused {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel),
        );
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn draw_active_samples_inspector(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    samples: &[GraphSample],
    frame_times: Option<&[DateTime<Local>]>,
    theme: Theme,
) {
    let (Some(index), Some(entry)) = (
        app.active_graph_index(),
        app.active_graph_id.and_then(|id| app.graph_entry_by_id(id)),
    ) else {
        return;
    };
    let title = samples_inspector_title(&entry.source, index, area.width, theme);
    let block = panel_block_focused(
        title,
        theme,
        app.panel_has_focus(FocusedPanel::DetailsSamples),
    )
    .title_bottom(Line::from(Span::styled(
        app.sample_follow_label(),
        Style::default().fg(theme.muted),
    )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if samples.is_empty() {
        frame.render_widget(
            Paragraph::new("No samples available")
                .style(Style::default().fg(theme.muted).bg(theme.panel)),
            inner,
        );
        return;
    }
    let viewport = draw_samples_subpanel(
        frame,
        inner,
        app,
        samples,
        entry.source.value_format(),
        entry.source.metric_label(),
        app.details_sample_selected,
        app.details_sample_offset,
        app.active_ab_comparison(),
        theme,
        true,
        app.show_sample_delta,
        frame_times,
    );
    render_samples_scrollbar(
        frame,
        inner,
        viewport,
        app.panel_has_focus(FocusedPanel::DetailsSamples),
        theme,
    );
}

fn active_graph_slot_style(theme: Theme) -> Style {
    Style::default()
        .fg(theme.active_series)
        .add_modifier(Modifier::BOLD)
}

fn samples_inspector_title(
    slot: &GraphSlot,
    ordinal: usize,
    area_width: u16,
    theme: Theme,
) -> Line<'static> {
    let slot_label = format!("Slot#{}", ordinal + 1);
    let mut spans = vec![
        Span::styled("SAMPLES", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(theme.muted)),
        Span::styled(slot_label.clone(), active_graph_slot_style(theme)),
    ];

    if let Some(identity) = slot.process_identity() {
        let target = format!("PID {} {}", identity.pid, identity.name);
        let full_title = Line::from(format!("SAMPLES · {slot_label} · {target}"));
        let available_width = usize::from(area_width.saturating_sub(2));
        if full_title.width() + crate::ui::widgets::block::PANEL_NAME_PADDING_WIDTH
            <= available_width
        {
            spans.push(Span::styled(" · ", Style::default().fg(theme.muted)));
            spans.push(Span::styled(target, Style::default().fg(theme.text)));
        } else {
            let compact = format!("SAMPLES · {slot_label} · PID {}", identity.pid);
            if Line::from(compact).width() + crate::ui::widgets::block::PANEL_NAME_PADDING_WIDTH
                <= available_width
            {
                spans.push(Span::styled(
                    format!(" · PID {}", identity.pid),
                    Style::default().fg(theme.text),
                ));
            } else {
                return Line::from(Span::styled(
                    format!("SAMPLES · PID {}", identity.pid),
                    Style::default().fg(theme.text),
                ));
            }
        }
    }

    Line::from(spans)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SampleViewport {
    start: usize,
    rows: usize,
    total: usize,
}

// Samples rendering keeps selection, comparison, and layout inputs explicit for parity tests.
#[allow(clippy::too_many_arguments)]
fn draw_samples_subpanel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    samples: &[GraphSample],
    metric: GraphValueFormat,
    metric_label: &str,
    selected: usize,
    offset: usize,
    comparison: Option<&AbComparison>,
    theme: Theme,
    show_base_summary: bool,
    show_delta: bool,
    frame_times: Option<&[DateTime<Local>]>,
) -> SampleViewport {
    let inner = area;
    let unit = exact_unit_suffix(metric);
    let metric_label = format!("{metric_label}{unit}");
    let metric_header_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::from(vec![
        Span::styled("A/B ", Style::default().fg(theme.muted)),
        Span::styled("Time      ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{metric_label:<SAMPLE_METRIC_VALUE_WIDTH$}"),
            metric_header_style,
        ),
        if show_delta {
            Span::styled(
                format!("{:>SAMPLE_DELTA_WIDTH$}", format!("Delta{unit}")),
                Style::default().fg(theme.muted),
            )
        } else {
            Span::raw("")
        },
    ])];

    let content_height = inner.height as usize;
    let summary_visibility = details_samples_summary_visibility(comparison);
    let content_layout =
        details_samples_content_layout(inner.height, summary_visibility, show_base_summary);
    let row_capacity = content_layout.row_capacity;
    let view_state = crate::app::DetailsSampleViewState {
        selected_index: selected.min(samples.len().saturating_sub(1)),
        selected_exact: true,
        offset,
    };
    let (start, end) = sample_viewport_bounds(samples.len(), view_state.offset, row_capacity);
    for (index, sample) in samples[start..end].iter().enumerate() {
        let sample_index = start + index;
        let sample_selected =
            view_state.selected_exact && sample_index == view_state.selected_index;
        let style = if sample_selected {
            Style::default()
                .fg(theme.text)
                .bg(theme.table_selection_surface)
        } else {
            Style::default().fg(theme.text)
        };
        let row_bg = if sample_selected {
            theme.table_selection_surface
        } else {
            theme.panel
        };
        let delta_value = metric_value(sample, metric);
        let metric_value_style = Style::default()
            .fg(if delta_value.is_none() {
                theme.muted
            } else {
                theme.text
            })
            .bg(row_bg);
        let previous_value = sample_index
            .checked_sub(1)
            .and_then(|previous_index| samples.get(previous_index))
            .and_then(|previous| metric_value(previous, metric));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<4}", sample_ab_marker(comparison, sample.captured_at)),
                style,
            ),
            Span::styled(
                format!("{}  ", sample.captured_at.format("%H:%M:%S")),
                style,
            ),
            Span::styled(
                format!(
                    "{:>SAMPLE_METRIC_VALUE_WIDTH$}",
                    format_metric_sample_value(sample, metric)
                ),
                metric_value_style,
            ),
            if show_delta {
                Span::styled("  ", style)
            } else {
                Span::raw("")
            },
            if show_delta {
                Span::styled(
                    format!(
                        "{:>SAMPLE_DELTA_WIDTH$}",
                        format_sample_delta(delta_value, previous_value, metric)
                    ),
                    delta_style(delta_value, previous_value, sample_selected, theme).bg(row_bg),
                )
            } else {
                Span::raw("")
            },
        ]));
    }

    let summary_lines = sample_summary_lines(
        samples,
        view_state.selected_index,
        metric,
        comparison,
        theme,
        content_layout.show_base_summary,
        app.log_view_interval_seconds,
        frame_times,
    );
    let spacer_lines = content_layout.spacer_height as usize;
    while lines.len() + spacer_lines + summary_lines.len() < content_height {
        lines.push(Line::from(""));
    }

    for _ in 0..spacer_lines {
        if lines.len() + summary_lines.len() < content_height {
            lines.push(Line::from(""));
        }
    }
    lines.extend(summary_lines);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.panel)),
        inner,
    );

    SampleViewport {
        start,
        rows: end.saturating_sub(start),
        total: samples.len(),
    }
}

fn sample_viewport_bounds(total: usize, offset: usize, rows: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let rows = rows.max(1).min(total);
    let start = offset.min(total.saturating_sub(rows));
    (start, start + rows)
}

#[cfg(test)]
fn synced_sample_viewport_offset(
    total: usize,
    rows: usize,
    selected_index: usize,
    active_selected: usize,
    active_offset: usize,
) -> usize {
    if total == 0 {
        return 0;
    }
    let rows = rows.max(1).min(total);
    let max_offset = total.saturating_sub(rows);
    let selected_index = selected_index.min(total.saturating_sub(1));
    let active_row = active_selected.saturating_sub(active_offset).min(rows - 1);
    selected_index.saturating_sub(active_row).min(max_offset)
}

#[cfg(test)]
fn sample_age_seconds(samples: &[GraphSample], index: usize) -> Option<i64> {
    let latest = samples.last()?.captured_at;
    let sample = samples.get(index)?;
    Some(
        latest
            .signed_duration_since(sample.captured_at)
            .num_seconds()
            .max(0),
    )
}

#[cfg(test)]
fn sample_index_nearest_age_seconds(samples: &[GraphSample], age_seconds: i64) -> Option<usize> {
    samples
        .iter()
        .enumerate()
        .min_by_key(|(index, _)| {
            let diff = sample_age_seconds(samples, *index)
                .map(|age| (age - age_seconds).abs())
                .unwrap_or(i64::MAX);
            (diff, usize::MAX - *index)
        })
        .map(|(index, _)| index)
}

#[cfg(test)]
fn sample_index_at_age_seconds(samples: &[GraphSample], age_seconds: i64) -> Option<usize> {
    samples.iter().enumerate().find_map(|(index, _)| {
        (sample_age_seconds(samples, index) == Some(age_seconds)).then_some(index)
    })
}

fn sample_age_seconds_at_time(
    time_reference_at: Option<DateTime<Local>>,
    captured_at: DateTime<Local>,
) -> Option<i64> {
    let time_reference_at = time_reference_at?;
    Some(
        time_reference_at
            .signed_duration_since(captured_at)
            .num_seconds()
            .max(0),
    )
}

fn sample_index_at_time(samples: &[GraphSample], captured_at: DateTime<Local>) -> Option<usize> {
    samples
        .iter()
        .position(|sample| sample.captured_at == captured_at)
}

#[cfg(test)]
fn sample_index_nearest_time(
    samples: &[GraphSample],
    captured_at: DateTime<Local>,
) -> Option<usize> {
    samples
        .iter()
        .enumerate()
        .min_by_key(|(index, sample)| {
            let diff = (sample.captured_at - captured_at)
                .num_milliseconds()
                .unsigned_abs();
            (diff, usize::MAX - *index)
        })
        .map(|(index, _)| index)
}

fn render_samples_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    viewport: SampleViewport,
    focused: bool,
    theme: Theme,
) {
    if viewport.total <= viewport.rows {
        return;
    }

    let mut state = ScrollbarState::new(viewport.total)
        .position(samples_scrollbar_position(
            viewport.total,
            viewport.rows,
            viewport.start,
        ))
        .viewport_content_length(viewport.rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(theme.muted).bg(theme.panel))
        .thumb_style(
            Style::default()
                .fg(if focused {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel),
        );
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn samples_scrollbar_position(total: usize, rows: usize, start: usize) -> usize {
    let rows = rows.max(1).min(total);
    let max_offset = total.saturating_sub(rows);
    if total == 0 || max_offset == 0 {
        return 0;
    }
    let max_scrollbar_position = total.saturating_sub(1);
    (start.min(max_offset) * max_scrollbar_position + max_offset / 2) / max_offset
}

// Plot construction needs the complete shared-time and slot-specific rendering context.
#[allow(clippy::too_many_arguments)]
fn draw_graph_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    samples: &[GraphSample],
    metric: GraphValueFormat,
    display_mode: GraphDisplayMode,
    aggregate_interval_seconds: Option<u64>,
    selected_sample_time: Option<DateTime<Local>>,
    time_reference_at: Option<DateTime<Local>>,
    bounds: (i64, i64),
    segments: &[Vec<(f64, f64)>],
    y_bounds: (f64, f64),
    comparison: Option<&AbComparison>,
    active: bool,
    theme: Theme,
    y_label_width: usize,
) {
    let layout = details_graph_rows(area);

    let (y_min, y_max) = y_bounds;
    let plot_segments = segments
        .iter()
        .map(|segment| lift_floor_points_for_plot(segment, y_min, y_max))
        .collect::<Vec<_>>();
    let selected_age_seconds =
        selected_sample_time.and_then(|time| sample_age_seconds_at_time(time_reference_at, time));
    let selected_line = selected_age_seconds
        .map(|age| selected_age_line_points(age, y_min, y_max, bounds, layout[1].height))
        .unwrap_or_default();
    let a_line = comparison
        .and_then(|comparison| comparison.a)
        .map(|point| {
            ab_line_points(
                time_reference_at,
                point,
                y_min,
                y_max,
                bounds,
                layout[1].height,
            )
        })
        .unwrap_or_default();
    let b_line = comparison
        .and_then(|comparison| comparison.b)
        .map(|point| {
            ab_line_points(
                time_reference_at,
                point,
                y_min,
                y_max,
                bounds,
                layout[1].height,
            )
        })
        .unwrap_or_default();
    let mut datasets = Vec::new();
    if !selected_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(selected_cursor_line_style(theme))
                .data(&selected_line),
        );
    }
    if !a_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.accent))
                .data(&a_line),
        );
    }
    if !b_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.accent))
                .data(&b_line),
        );
    }
    for plot_data in &plot_segments {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(graph_series_style(theme, active))
                .data(plot_data),
        );
    }
    let y_labels = pad_y_axis_labels(y_axis_labels(y_min, y_max, metric), y_label_width);
    let chart = Chart::new(datasets)
        .style(Style::default().fg(theme.text).bg(theme.panel))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(theme.muted))
                .bounds([bounds.0 as f64, bounds.1 as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(theme.muted))
                .bounds([y_min, y_max])
                .labels(y_labels),
        );
    let selected_value_label = selected_age_seconds
        .and(selected_sample_time)
        .and_then(|time| sample_index_at_time(samples, time))
        .and_then(|index| samples.get(index))
        .and_then(|sample| {
            format_graph_cursor_value(sample, metric, display_mode, aggregate_interval_seconds)
        });
    let top_labels = Paragraph::new(graph_top_label_line(
        layout[0].width as usize,
        y_label_width,
        bounds,
        selected_age_seconds,
        selected_value_label.as_deref(),
        theme,
    ))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(top_labels, layout[0]);
    frame.render_widget(chart, layout[1]);
    frame.render_widget(
        ChartAxisOverlay {
            y_label_width,
            theme,
        },
        layout[1],
    );
    frame.render_widget(
        GraphAbAxisLabels {
            y_label_width,
            bounds,
            time_reference_at,
            comparison,
            theme,
        },
        layout[1],
    );

    let x_axis = Paragraph::new(axis_tick_label_line(
        layout[2].width as usize,
        y_label_width,
        bounds,
        time_reference_at,
        theme,
    ))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(x_axis, layout[2]);
}

fn draw_graph_shared_controls(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    effective: usize,
    theme: Theme,
) {
    let controls = graph_shared_control_areas(area, app.show_samples_panel);
    render_graph_toggle(
        frame,
        controls.samples,
        app.show_samples_panel,
        "v",
        "Samples",
        theme,
    );
    render_graph_toggle(
        frame,
        controls.delta,
        app.show_sample_delta,
        "d",
        "Delta",
        theme,
    );
    let requested = app.graph_slot_layout.columns() as usize;
    let label = if requested > 0 && requested != effective {
        format!("{effective}/{requested} cols")
    } else if requested == 0 {
        format!("Auto ({effective})")
    } else {
        app.graph_slot_layout.label().to_string()
    };
    render_graph_layout_mode(frame, controls.layout, &label, theme);
    render_graph_toggle(
        frame,
        controls.all_samples,
        app.graph_show_all_samples,
        "f",
        "Fit all",
        theme,
    );
    render_graph_toggle(
        frame,
        controls.y_axis,
        app.graph_y_axis_zero_min,
        "z",
        "Min 0",
        theme,
    );
}

fn graph_workspace_title(
    app: &App,
    theme: Theme,
    focused: bool,
    controls: GraphSpanControlAreas,
) -> Line<'static> {
    let style = Style::default()
        .fg(if focused {
            theme.focus_border
        } else {
            theme.muted
        })
        .add_modifier(if focused {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    let mut spans = vec![Span::styled(graph_workspace_title_label(app), style)];
    if controls.zoom_out.is_some() && controls.zoom_in.is_some() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[-]",
            graph_button_style(
                theme,
                app.graph_hovered_target == Some(GraphHoverTarget::ZoomOut),
                app.can_zoom_graph_time_span(false),
            ),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "[+]",
            graph_button_style(
                theme,
                app.graph_hovered_target == Some(GraphHoverTarget::ZoomIn),
                app.can_zoom_graph_time_span(true),
            ),
        ));
    }
    Line::from(spans)
}

fn graph_button_style(theme: Theme, hovered: bool, enabled: bool) -> Style {
    if !enabled {
        Style::default().fg(theme.exited).bg(theme.panel)
    } else if hovered {
        Style::default()
            .fg(theme.text)
            .bg(theme.focus_surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted).bg(theme.panel)
    }
}

fn render_graph_toggle(
    frame: &mut ratatui::Frame<'_>,
    area: Option<Rect>,
    checked: bool,
    key: &'static str,
    label: &'static str,
    theme: Theme,
) {
    let Some(toggle_area) = area else {
        return;
    };
    let mark = if checked { "[x]" } else { "[ ]" };
    let mark_color = if checked { theme.accent } else { theme.muted };
    let toggle = Paragraph::new(Line::from(vec![
        Span::styled(mark, Style::default().fg(mark_color).bg(theme.panel)),
        Span::raw(" "),
        Span::styled(key, Style::default().fg(theme.key_hint).bg(theme.panel)),
        Span::styled(":", Style::default().fg(theme.muted).bg(theme.panel)),
        Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text).bg(theme.panel),
        ),
    ]))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(toggle, toggle_area);
}

fn render_graph_layout_mode(
    frame: &mut ratatui::Frame<'_>,
    area: Option<Rect>,
    mode: &str,
    theme: Theme,
) {
    let Some(area) = area else {
        return;
    };
    let content = Paragraph::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("l", Style::default().fg(theme.key_hint).bg(theme.panel)),
        Span::styled(":", Style::default().fg(theme.muted).bg(theme.panel)),
        Span::styled(
            format!(" {mode}"),
            Style::default().fg(theme.text).bg(theme.panel),
        ),
    ]))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(content, area);
}

fn details_paragraph<'a>(lines: Vec<Line<'a>>, theme: Theme) -> Paragraph<'a> {
    Paragraph::new(lines).style(Style::default().fg(theme.text).bg(theme.background))
}

fn format_metric_sample_value(sample: &GraphSample, metric: GraphValueFormat) -> String {
    metric_value(sample, metric)
        .map(|value| format_metric_exact_value(value, metric))
        .unwrap_or_else(|| "--".to_string())
}

fn moving_average_label(aggregate_interval_seconds: Option<u64>) -> String {
    aggregate_interval_seconds
        .map(|interval| format!("MA5 (5×{interval}s avg)"))
        .unwrap_or_else(|| "MA5".to_string())
}

fn format_graph_cursor_value(
    sample: &GraphSample,
    metric: GraphValueFormat,
    display_mode: GraphDisplayMode,
    aggregate_interval_seconds: Option<u64>,
) -> Option<String> {
    let value = metric_value(sample, metric)?;
    let value = match metric {
        GraphValueFormat::Bytes => format!("{} B", format_metric_exact_value(value, metric)),
        GraphValueFormat::BytesPerSec => {
            format!("{} B/s", format_integer(value.round().max(0.0) as u64))
        }
        _ => format_metric_exact_value(value, metric),
    };
    Some(match display_mode {
        GraphDisplayMode::Raw => value,
        GraphDisplayMode::MovingAverage5 => {
            format!(
                "{}: {value}",
                moving_average_label(aggregate_interval_seconds)
            )
        }
    })
}

fn sample_max_line(
    samples: &[GraphSample],
    metric: GraphValueFormat,
    theme: Theme,
    aggregate_interval_seconds: Option<u64>,
) -> Line<'static> {
    let label = aggregate_interval_seconds
        .filter(|interval| *interval > 1)
        .map(|interval| format!("History max ({interval}s avg)"))
        .unwrap_or_else(|| "History max".to_string());
    let Some((sample, value)) = sample_max(samples, metric) else {
        return Line::from(Span::styled(
            format!("{label}: --"),
            Style::default().fg(theme.muted),
        ));
    };
    Line::from(Span::styled(
        format!(
            "{label}: {} @ {}",
            format_metric_exact_value(value, metric),
            sample.captured_at.format("%H:%M:%S")
        ),
        Style::default().fg(theme.muted),
    ))
}

// Summary rendering combines independent selection, A/B, layout, and log-timing inputs.
#[allow(clippy::too_many_arguments)]
fn sample_summary_lines(
    samples: &[GraphSample],
    display_selected: usize,
    metric: GraphValueFormat,
    comparison: Option<&AbComparison>,
    theme: Theme,
    show_base_summary: bool,
    aggregate_interval_seconds: Option<u64>,
    frame_times: Option<&[DateTime<Local>]>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if show_base_summary {
        lines.push(sample_max_line(
            samples,
            metric,
            theme,
            aggregate_interval_seconds,
        ));
        lines.push(sample_moving_average_line(
            samples,
            display_selected,
            metric,
            theme,
            aggregate_interval_seconds,
            frame_times,
        ));
    }
    lines.extend(sample_ab_summary_lines(comparison, samples, metric, theme));
    if let Some(statistics) = ab_range_statistics(comparison, samples, frame_times) {
        lines.extend(sample_ab_range_summary_lines(
            &statistics,
            metric,
            theme,
            aggregate_interval_seconds,
        ));
    }
    lines
}

fn sample_moving_average_line(
    samples: &[GraphSample],
    selected: usize,
    metric: GraphValueFormat,
    theme: Theme,
    aggregate_interval_seconds: Option<u64>,
    frame_times: Option<&[DateTime<Local>]>,
) -> Line<'static> {
    let label = format!(
        "Selected {}",
        moving_average_label(aggregate_interval_seconds)
    );
    let Some((captured_at, value)) = sample_moving_average(samples, selected, frame_times) else {
        return Line::from(Span::styled(
            format!("{label}: --"),
            Style::default().fg(theme.muted),
        ));
    };
    Line::from(Span::styled(
        format!(
            "{label}: {} @ {}",
            format_metric_exact_value(value, metric),
            captured_at.format("%H:%M:%S")
        ),
        Style::default().fg(theme.muted),
    ))
}

fn sample_moving_average(
    samples: &[GraphSample],
    selected: usize,
    frame_times: Option<&[DateTime<Local>]>,
) -> Option<(DateTime<Local>, f64)> {
    let selected_sample = samples.get(selected)?;
    moving_average_samples(samples, frame_times)
        .into_iter()
        .find(|sample| sample.captured_at == selected_sample.captured_at)
        .and_then(|sample| sample.value.map(|value| (sample.captured_at, value)))
}

#[derive(Default)]
struct MovingAverageWindow {
    values: [f64; 5],
    len: usize,
    next: usize,
    total: f64,
}

impl MovingAverageWindow {
    fn update(&mut self, value: Option<f64>) -> Option<f64> {
        let Some(value) = value else {
            *self = Self::default();
            return None;
        };
        if self.len < self.values.len() {
            self.values[self.len] = value;
            self.total += value;
            self.len += 1;
        } else {
            self.total -= self.values[self.next];
            self.values[self.next] = value;
            self.total += value;
            self.next = (self.next + 1) % self.values.len();
        }
        (self.len == self.values.len()).then_some(self.total / self.values.len() as f64)
    }
}

fn moving_average_samples(
    samples: &[GraphSample],
    frame_times: Option<&[DateTime<Local>]>,
) -> Vec<GraphSample> {
    let mut window = MovingAverageWindow::default();
    let Some(frame_times) = frame_times.filter(|times| !times.is_empty()) else {
        return samples
            .iter()
            .map(|sample| GraphSample {
                captured_at: sample.captured_at,
                value: window.update(sample.value),
            })
            .collect();
    };

    let mut sample_index = 0;
    frame_times
        .iter()
        .map(|captured_at| {
            while sample_index < samples.len() && samples[sample_index].captured_at < *captured_at {
                sample_index += 1;
            }
            let value = samples
                .get(sample_index)
                .filter(|sample| sample.captured_at == *captured_at)
                .and_then(|sample| sample.value);
            GraphSample {
                captured_at: *captured_at,
                value: window.update(value),
            }
        })
        .collect()
}

fn sample_max(samples: &[GraphSample], metric: GraphValueFormat) -> Option<(&GraphSample, f64)> {
    let mut max: Option<(&GraphSample, f64)> = None;
    for sample in samples {
        let Some(value) = metric_value(sample, metric) else {
            continue;
        };
        if max.is_none_or(|(_, max_value)| value > max_value) {
            max = Some((sample, value));
        }
    }
    max
}

fn format_metric_axis_value(value: f64, metric: GraphValueFormat) -> String {
    match metric {
        GraphValueFormat::Bytes => format_compact_bytes(value.round().max(0.0) as u64),
        GraphValueFormat::BytesPerSec => {
            format!("{}/s", format_compact_bytes(value.round().max(0.0) as u64))
        }
        _ => format_metric_exact_value(value, metric),
    }
}

fn y_axis_labels(y_min: f64, y_max: f64, metric: GraphValueFormat) -> Vec<String> {
    let y_mid = y_min + (y_max - y_min) / 2.0;
    let [lower_label, middle_label, upper_label] = if metric == GraphValueFormat::Bytes {
        compact_byte_axis_labels([y_min, y_mid, y_max])
    } else {
        [
            if y_min == 0.0 {
                "0".to_string()
            } else {
                format_metric_axis_value(y_min, metric)
            },
            format_metric_axis_value(y_mid, metric),
            format_metric_axis_value(y_max, metric),
        ]
    };
    let visible_middle_label = if middle_label == lower_label || middle_label == upper_label {
        String::new()
    } else {
        middle_label
    };

    vec![lower_label, visible_middle_label, upper_label]
}

fn compact_byte_axis_labels(values: [f64; 3]) -> [String; 3] {
    let mut labels = values.map(|value| format_compact_byte_axis_value(value, 1));
    for precision in 1..=3 {
        labels = values.map(|value| format_compact_byte_axis_value(value, precision));
        if labels[0] != labels[2] && labels[1] != labels[0] && labels[1] != labels[2] {
            break;
        }
    }
    labels
}

fn format_compact_byte_axis_value(value: f64, precision: usize) -> String {
    let value = value.round().max(0.0);
    if value == 0.0 {
        return "0".to_string();
    }
    let bytes = value as u64;
    if bytes < 1_000 {
        format_integer(bytes)
    } else {
        format_compact_bytes_with_precision(bytes, precision)
    }
}

fn y_axis_label_width(labels: &[String]) -> usize {
    labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0)
        + 1
}

fn pad_y_axis_labels(labels: Vec<String>, y_label_width: usize) -> Vec<String> {
    let label_width = y_label_width.saturating_sub(1);
    labels
        .into_iter()
        .map(|label| {
            if label.is_empty() {
                label
            } else {
                format!("{label:>label_width$}")
            }
        })
        .collect()
}

fn format_metric_exact_value(value: f64, metric: GraphValueFormat) -> String {
    match metric {
        GraphValueFormat::Bytes | GraphValueFormat::Count => {
            format_integer(value.round().max(0.0) as u64)
        }
        GraphValueFormat::BytesPerSec => {
            format!("{}/s", format_integer(value.round().max(0.0) as u64))
        }
        GraphValueFormat::Percent => format!("{value:.1}%"),
        GraphValueFormat::AdaptiveBitsPerSec => format_io_rate(value.round().max(0.0) as u64),
        GraphValueFormat::MegabitsPerSec => {
            format!("{} Mbps", ((value * 8.0) / 1_000_000.0).round() as u64)
        }
        GraphValueFormat::MegabytesPerSec => format_mb_per_sec(value.round().max(0.0) as u64),
        GraphValueFormat::QueueLength => format!("{value:.1}"),
    }
}

fn format_sample_delta(
    value: Option<f64>,
    previous: Option<f64>,
    metric: GraphValueFormat,
) -> String {
    let Some(value) = value else {
        return "--".to_string();
    };
    let Some(previous) = previous else {
        return "--".to_string();
    };
    format_ab_delta(value - previous, metric)
}

fn selected_cursor_line_style(theme: Theme) -> Style {
    Style::default().fg(theme.cursor_guide)
}

fn graph_series_style(theme: Theme, active: bool) -> Style {
    Style::default().fg(if active {
        theme.active_series
    } else {
        theme.graph_line
    })
}

fn delta_style(value: Option<f64>, previous: Option<f64>, selected: bool, theme: Theme) -> Style {
    if value.is_none() || previous.is_none() {
        return Style::default().fg(theme.muted);
    }
    if selected {
        Style::default().fg(theme.text)
    } else {
        Style::default().fg(theme.muted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AbRangeStatistics {
    min_value: f64,
    min_captured_at: DateTime<Local>,
    max_value: f64,
    max_captured_at: DateTime<Local>,
    mean: f64,
    available_sample_count: usize,
    expected_frame_count: Option<usize>,
    missing_frame_count: Option<usize>,
}

fn ab_range_statistics(
    comparison: Option<&AbComparison>,
    samples: &[GraphSample],
    frame_times: Option<&[DateTime<Local>]>,
) -> Option<AbRangeStatistics> {
    let (a, b) = comparison?.a.zip(comparison?.b)?;
    let (range_start, range_end) = if a.captured_at <= b.captured_at {
        (a.captured_at, b.captured_at)
    } else {
        (b.captured_at, a.captured_at)
    };
    let expected_frame_count = frame_times
        .filter(|frame_times| !frame_times.is_empty())
        .map(|frame_times| {
            frame_times
                .iter()
                .filter(|captured_at| **captured_at >= range_start && **captured_at <= range_end)
                .count()
        });

    let mut statistics: Option<AbRangeStatistics> = None;
    let mut total = 0.0;
    for sample in samples
        .iter()
        .filter(|sample| sample.captured_at >= range_start && sample.captured_at <= range_end)
    {
        let Some(value) = sample.value.filter(|value| value.is_finite()) else {
            continue;
        };
        total += value;
        match &mut statistics {
            Some(statistics) => {
                if value < statistics.min_value
                    || (value == statistics.min_value
                        && sample.captured_at < statistics.min_captured_at)
                {
                    statistics.min_value = value;
                    statistics.min_captured_at = sample.captured_at;
                }
                if value > statistics.max_value
                    || (value == statistics.max_value
                        && sample.captured_at < statistics.max_captured_at)
                {
                    statistics.max_value = value;
                    statistics.max_captured_at = sample.captured_at;
                }
                statistics.available_sample_count += 1;
            }
            None => {
                statistics = Some(AbRangeStatistics {
                    min_value: value,
                    min_captured_at: sample.captured_at,
                    max_value: value,
                    max_captured_at: sample.captured_at,
                    mean: 0.0,
                    available_sample_count: 1,
                    expected_frame_count,
                    missing_frame_count: None,
                });
            }
        }
    }

    let mut statistics = statistics?;
    statistics.mean = total / statistics.available_sample_count as f64;
    statistics.missing_frame_count = statistics
        .expected_frame_count
        .map(|expected| expected.saturating_sub(statistics.available_sample_count));
    Some(statistics)
}

fn sample_ab_range_summary_lines(
    statistics: &AbRangeStatistics,
    metric: GraphValueFormat,
    theme: Theme,
    aggregate_interval_seconds: Option<u64>,
) -> Vec<Line<'static>> {
    let min_label = aggregate_interval_seconds
        .map(|interval| format!("A-B min ({interval}s avg)"))
        .unwrap_or_else(|| "A-B min".to_string());
    let sample_count = match (
        statistics.expected_frame_count,
        statistics.missing_frame_count,
    ) {
        (Some(expected), Some(missing)) => format!(
            "{}/{}  Missing: {}",
            format_integer(statistics.available_sample_count as u64),
            format_integer(expected as u64),
            format_integer(missing as u64),
        ),
        _ => format_integer(statistics.available_sample_count as u64),
    };
    vec![
        Line::from(vec![
            Span::styled(format!("{min_label}: "), Style::default().fg(theme.accent)),
            Span::styled(
                format!(
                    "{} @ {}",
                    format_metric_exact_value(statistics.min_value, metric),
                    statistics.min_captured_at.format("%H:%M:%S")
                ),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("A-B max: ", Style::default().fg(theme.accent)),
            Span::styled(
                format!(
                    "{} @ {}",
                    format_metric_exact_value(statistics.max_value, metric),
                    statistics.max_captured_at.format("%H:%M:%S")
                ),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("A-B avg: ", Style::default().fg(theme.accent)),
            Span::styled(
                format_metric_exact_value(statistics.mean, metric),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("A-B samples: ", Style::default().fg(theme.accent)),
            Span::styled(sample_count, Style::default().fg(theme.text)),
        ]),
    ]
}

fn sample_ab_summary_lines(
    comparison: Option<&AbComparison>,
    samples: &[GraphSample],
    metric: GraphValueFormat,
    theme: Theme,
) -> Vec<Line<'static>> {
    if let Some(comparison) = comparison {
        let a_value = comparison
            .a
            .map(|point| format_ab_point(point, samples, metric))
            .unwrap_or_else(|| "--".to_string());
        let b_value = comparison
            .b
            .map(|point| format_ab_point(point, samples, metric))
            .unwrap_or_else(|| "--".to_string());
        let delta = comparison
            .a
            .zip(comparison.b)
            .map(|(a, b)| format_ab_delta_with_elapsed(a, b, samples, metric))
            .unwrap_or_else(|| "--".to_string());
        vec![
            Line::from(vec![
                Span::styled("A: ", Style::default().fg(theme.accent)),
                Span::styled(a_value, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("B: ", Style::default().fg(theme.accent)),
                Span::styled(b_value, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("B-A: ", Style::default().fg(theme.accent)),
                Span::styled(delta, Style::default().fg(theme.text)),
            ]),
        ]
    } else {
        Vec::new()
    }
}

fn format_ab_point(
    point: AbComparisonPoint,
    samples: &[GraphSample],
    metric: GraphValueFormat,
) -> String {
    let value = samples
        .iter()
        .find(|sample| sample.captured_at == point.captured_at)
        .and_then(|sample| metric_value(sample, metric))
        .map(|value| format_metric_exact_value(value, metric))
        .unwrap_or_else(|| "--".to_string());
    format!("{} {}", point.captured_at.format("%H:%M:%S"), value)
}

fn format_ab_delta(delta: f64, metric: GraphValueFormat) -> String {
    match metric {
        GraphValueFormat::Bytes | GraphValueFormat::Count => {
            format_signed_integer(delta.round() as i128)
        }
        GraphValueFormat::BytesPerSec => {
            format!("{}/s", format_signed_integer(delta.round() as i128))
        }
        GraphValueFormat::Percent => format!("{delta:+.1}%"),
        GraphValueFormat::AdaptiveBitsPerSec => format_signed_io_rate(delta.round() as i128),
        GraphValueFormat::MegabitsPerSec => {
            let mbps = ((delta * 8.0) / 1_000_000.0).round() as i128;
            format_signed_integer(mbps) + " Mbps"
        }
        GraphValueFormat::MegabytesPerSec => {
            let mb_per_sec = (delta / 1_000_000.0).round() as i128;
            format_signed_integer(mb_per_sec) + " MB/s"
        }
        GraphValueFormat::QueueLength => format!("{delta:+.1}"),
    }
}

fn format_ab_delta_with_elapsed(
    a: AbComparisonPoint,
    b: AbComparisonPoint,
    samples: &[GraphSample],
    metric: GraphValueFormat,
) -> String {
    let delta = ab_delta_value(a, b, samples, metric)
        .map(|delta| format_ab_delta(delta, metric))
        .unwrap_or_else(|| "--".to_string());
    format!(
        "{} ({})",
        delta,
        format_elapsed_delta(b.captured_at.signed_duration_since(a.captured_at))
    )
}

fn ab_delta_value(
    a: AbComparisonPoint,
    b: AbComparisonPoint,
    samples: &[GraphSample],
    metric: GraphValueFormat,
) -> Option<f64> {
    samples
        .iter()
        .find(|sample| sample.captured_at == a.captured_at)
        .and_then(|sample| metric_value(sample, metric))
        .zip(
            samples
                .iter()
                .find(|sample| sample.captured_at == b.captured_at)
                .and_then(|sample| metric_value(sample, metric)),
        )
        .map(|(a_value, b_value)| b_value - a_value)
}

fn format_graph_title_ab_delta(
    comparison: Option<&AbComparison>,
    samples: &[GraphSample],
    metric: GraphValueFormat,
) -> String {
    comparison
        .and_then(|comparison| comparison.a.zip(comparison.b))
        .and_then(|(a, b)| ab_delta_value(a, b, samples, metric))
        .map(|delta| format_ab_delta(delta, metric))
        .unwrap_or_else(|| "--".to_string())
}

fn format_elapsed_delta(delta: chrono::Duration) -> String {
    let seconds = delta.num_seconds();
    let sign = if seconds < 0 { "-" } else { "+" };
    let abs_seconds = seconds.abs();
    let hours = abs_seconds / 3_600;
    let minutes = (abs_seconds % 3_600) / 60;
    let seconds = abs_seconds % 60;
    if hours > 0 {
        format!("{sign}{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{sign}{minutes}m{seconds:02}s")
    } else {
        format!("{sign}{seconds}s")
    }
}

fn sample_ab_marker(
    comparison: Option<&AbComparison>,
    captured_at: DateTime<Local>,
) -> &'static str {
    let Some(comparison) = comparison else {
        return "";
    };
    let is_a = comparison
        .a
        .is_some_and(|point| point.captured_at == captured_at);
    let is_b = comparison
        .b
        .is_some_and(|point| point.captured_at == captured_at);
    match (is_a, is_b) {
        (true, true) => "AB",
        (true, false) => "A",
        (false, true) => "B",
        (false, false) => "",
    }
}

fn graph_slot_title_line(
    slot: &GraphSlot,
    ordinal: usize,
    samples: &[GraphSample],
    metric: GraphValueFormat,
    comparison: Option<&AbComparison>,
    active: bool,
    theme: Theme,
) -> Line<'static> {
    let main_style = if active {
        Style::default().fg(theme.text)
    } else {
        Style::default().fg(theme.muted)
    };
    let slot_style = if active {
        active_graph_slot_style(theme)
    } else {
        Style::default().fg(theme.muted)
    };
    let comparison_style = if active {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.muted)
    };
    let mut spans = vec![
        Span::styled(format!("Slot#{}", ordinal + 1), slot_style),
        Span::styled(" · ", Style::default().fg(theme.muted)),
        Span::styled(slot.graph_title_metric_label(), main_style),
    ];
    if let Some(target) = slot.graph_title_target_label() {
        spans.push(Span::styled(" · ", Style::default().fg(theme.muted)));
        spans.push(Span::styled(target.to_string(), main_style));
    }
    spans.extend([
        Span::styled(" · B-A: ", comparison_style),
        Span::styled(
            format_graph_title_ab_delta(comparison, samples, metric),
            main_style,
        ),
    ]);
    Line::from(spans)
}

fn metric_value(sample: &GraphSample, _metric: GraphValueFormat) -> Option<f64> {
    sample.value
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GraphStats {
    current: Option<f64>,
    window_min: Option<f64>,
    window_max: Option<f64>,
    max: Option<f64>,
    scale_max: f64,
}

#[cfg(test)]
fn graph_stats(samples: &[GraphSample], peak: Option<f64>, points: &[(f64, f64)]) -> GraphStats {
    graph_stats_for_values(samples, peak, points.iter().map(|(_, value)| *value))
}

fn graph_stats_for_values(
    samples: &[GraphSample],
    peak: Option<f64>,
    values: impl Iterator<Item = f64>,
) -> GraphStats {
    let current = samples.last().and_then(|sample| sample.value);
    let (window_min, window_max) = values.fold((None, None), |(min, max), value| {
        (
            Some(min.map_or(value, |current: f64| current.min(value))),
            Some(max.map_or(value, |current: f64| current.max(value))),
        )
    });
    let max = peak;
    GraphStats {
        current,
        window_min,
        window_max,
        max,
        scale_max: nice_axis_max(window_max.unwrap_or(0.0).round() as u64) as f64,
    }
}

fn graph_y_bounds(stats: &GraphStats, zero_min: bool) -> (f64, f64) {
    if zero_min {
        return (0.0, stats.scale_max.max(1.0));
    }

    let Some(window_min) = stats.window_min else {
        return (0.0, 1.0);
    };
    let Some(window_max) = stats.window_max else {
        return (0.0, 1.0);
    };
    nice_auto_y_bounds(window_min.max(0.0), window_max.max(0.0))
}

fn nice_auto_y_bounds(window_min: f64, window_max: f64) -> (f64, f64) {
    let window_min = window_min.min(window_max);
    let window_max = window_max.max(window_min);
    let raw_range = (window_max - window_min).max((window_max.abs() * 0.01).max(1.0));
    let mut step = nice_tick_step(raw_range / 2.0);

    loop {
        let mut y_min = floor_to_multiple_f64(window_min, step).max(0.0);
        if y_min >= window_min && y_min > 0.0 {
            y_min = (y_min - step).max(0.0);
        }
        let y_max = y_min + step * 2.0;
        if y_max >= window_max && y_max > y_min {
            return (y_min, y_max);
        }
        step = next_nice_tick_step(step);
    }
}

fn nice_tick_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let magnitude = 10_f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let factor = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

fn next_nice_tick_step(step: f64) -> f64 {
    if !step.is_finite() || step <= 0.0 {
        return 1.0;
    }
    let magnitude = 10_f64.powf(step.log10().floor());
    let normalized = step / magnitude;
    if normalized < 2.0 {
        2.0 * magnitude
    } else if normalized < 5.0 {
        5.0 * magnitude
    } else {
        10.0 * magnitude
    }
}

fn floor_to_multiple_f64(value: f64, step: f64) -> f64 {
    if !value.is_finite() || !step.is_finite() || step <= 0.0 {
        return 0.0;
    }
    (value / step).floor() * step
}

fn nice_axis_max(value: u64) -> u64 {
    if value <= 10 {
        return value.max(1);
    }

    let digits = value.ilog10() + 1;
    let step = pow10_u64(digits.saturating_sub(2));
    ceil_to_multiple(value, step)
}

fn ceil_to_multiple(value: u64, step: u64) -> u64 {
    value.div_ceil(step) * step
}

fn pow10_u64(power: u32) -> u64 {
    10_u64.pow(power)
}

#[derive(Clone, Copy)]
struct ChartAxisOverlay {
    y_label_width: usize,
    theme: Theme,
}

impl Widget for ChartAxisOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let style = Style::default().fg(self.theme.muted).bg(self.theme.panel);
        let axis_x_offset = graph_axis_x_offset(area.width as usize, self.y_label_width);
        let axis_x = area.x + axis_x_offset as u16;
        let bottom_y = area.bottom().saturating_sub(1);

        for y in area.y..area.bottom() {
            buf[(axis_x, y)].set_symbol("│").set_style(style);
        }

        for x in axis_x..area.right() {
            if buf[(x, bottom_y)].symbol() == " " {
                buf[(x, bottom_y)].set_symbol("─").set_style(style);
            }
        }

        for y_offset in y_axis_tick_positions(area.height as usize) {
            let y = area.y + y_offset as u16;
            let symbol = if y == bottom_y { "┼" } else { "┤" };
            buf[(axis_x, y)].set_symbol(symbol).set_style(style);
        }

        for x_offset in axis_tick_positions(area.width as usize, self.y_label_width) {
            let x = area.x + x_offset as u16;
            let symbol = if x == axis_x { "┼" } else { "┬" };
            buf[(x, bottom_y)].set_symbol(symbol).set_style(style);
        }
    }
}

struct GraphAbAxisLabels<'a> {
    y_label_width: usize,
    bounds: (i64, i64),
    time_reference_at: Option<DateTime<Local>>,
    comparison: Option<&'a AbComparison>,
    theme: Theme,
}

impl Widget for GraphAbAxisLabels<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height < 2 {
            return;
        }
        let (Some(time_reference_at), Some(comparison)) = (self.time_reference_at, self.comparison)
        else {
            return;
        };

        let label_y = area.bottom().saturating_sub(1);
        let style = Style::default().fg(self.theme.accent).bg(self.theme.panel);
        for (label, point) in [("A", comparison.a), ("B", comparison.b)] {
            let Some(point) = point else {
                continue;
            };
            let Some(x) = ab_point_x(
                area,
                self.y_label_width,
                self.bounds,
                time_reference_at,
                point,
            ) else {
                continue;
            };
            if x < area.right() && label_y < area.bottom() {
                buf[(x, label_y)].set_symbol(label).set_style(style);
            }
        }
    }
}

fn axis_tick_label_line(
    width: usize,
    y_label_width: usize,
    bounds: (i64, i64),
    latest_sample_at: Option<DateTime<Local>>,
    theme: Theme,
) -> Line<'static> {
    let mut chars = vec![' '; width];
    let labels = graph_tick_labels(bounds, latest_sample_at);
    let positions = axis_tick_positions(width, y_label_width);
    let mut occupied = Vec::<std::ops::Range<usize>>::new();
    // Keep the endpoints first, then the midpoint and quarter points when they fit.
    for index in [0, 4, 2, 1, 3] {
        let Some(&position) = positions.get(index) else {
            continue;
        };
        let label = &labels[index];
        let label_width = label.chars().count();
        if label_width > width {
            continue;
        }
        let start = axis_label_start(width, label_width, position);
        let end = start + label_width;
        if occupied
            .iter()
            .any(|range| start <= range.end && range.start <= end)
        {
            continue;
        }
        for (cell, ch) in chars[start..end].iter_mut().zip(label.chars()) {
            *cell = ch;
        }
        occupied.push(start..end);
    }
    Line::from(Span::styled(
        chars.into_iter().collect::<String>(),
        Style::default().fg(theme.muted),
    ))
}

fn graph_top_label_line(
    width: usize,
    y_label_width: usize,
    bounds: (i64, i64),
    selected_age_seconds: Option<i64>,
    selected_value_label: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    let area = Rect::new(0, 0, width as u16, 1);
    let mut labels = vec![None; width];
    if let (Some(age), Some(label)) = (selected_age_seconds, selected_value_label)
        && let Some(x) = age_point_x(area, y_label_width, bounds, age)
    {
        write_label_slots(&mut labels, label, x as usize, theme.accent);
    }

    Line::from(
        labels
            .into_iter()
            .map(|label| match label {
                Some((label, color)) => {
                    Span::styled(label, Style::default().fg(color).bg(theme.panel))
                }
                None => Span::styled(" ", Style::default().bg(theme.panel)),
            })
            .collect::<Vec<_>>(),
    )
}

fn write_label_slots(
    labels: &mut [Option<(String, ratatui::style::Color)>],
    label: &str,
    center: usize,
    color: ratatui::style::Color,
) {
    if labels.is_empty() {
        return;
    }
    let width = label.chars().count();
    let start = if center + width >= labels.len() {
        labels.len().saturating_sub(width)
    } else {
        center.saturating_sub(width / 2)
    };
    for (offset, ch) in label.chars().enumerate() {
        if let Some(slot) = labels.get_mut(start + offset) {
            *slot = Some((ch.to_string(), color));
        }
    }
}

fn graph_tick_labels(bounds: (i64, i64), latest_sample_at: Option<DateTime<Local>>) -> Vec<String> {
    let span = (bounds.1 - bounds.0).max(1);
    (0..=4)
        .map(|index| {
            let value = bounds.0 + (span * index + 2) / 4;
            latest_sample_at
                .map(|latest| {
                    (latest + chrono::Duration::seconds(value))
                        .format("%H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|| "--:--:--".to_string())
        })
        .collect()
}

fn y_axis_tick_positions(height: usize) -> [usize; 3] {
    let bottom = height.saturating_sub(1);
    [0, height / 2, bottom]
}

fn graph_axis_x_offset(width: usize, y_label_width: usize) -> usize {
    y_label_width.saturating_sub(1).min(width.saturating_sub(1))
}

fn axis_tick_positions(width: usize, y_label_width: usize) -> Vec<usize> {
    if width == 0 {
        return Vec::new();
    }

    let start = graph_axis_x_offset(width, y_label_width);
    let plot_width = width.saturating_sub(start).max(1);
    (0..=4)
        .map(|index| {
            let offset = ((plot_width - 1) * index + 2) / 4;
            start + offset
        })
        .collect()
}

fn axis_label_start(width: usize, label_width: usize, tick_position: usize) -> usize {
    if tick_position + label_width >= width {
        width.saturating_sub(label_width)
    } else {
        tick_position.saturating_sub(label_width / 2)
    }
}

fn ab_point_x(
    area: Rect,
    y_label_width: usize,
    bounds: (i64, i64),
    latest_sample_at: DateTime<Local>,
    point: AbComparisonPoint,
) -> Option<u16> {
    let age = latest_sample_at
        .signed_duration_since(point.captured_at)
        .num_seconds()
        .max(0);
    age_point_x(area, y_label_width, bounds, age)
}

fn age_point_x(area: Rect, y_label_width: usize, bounds: (i64, i64), age: i64) -> Option<u16> {
    if area.width == 0 {
        return None;
    }
    let x_value = -age;
    if x_value < bounds.0 || x_value > bounds.1 {
        return None;
    }
    let start = graph_axis_x_offset(area.width as usize, y_label_width);
    let plot_width = area.width as usize - start;
    if plot_width == 0 {
        return None;
    }
    let span = usize::try_from((bounds.1 - bounds.0).max(1)).unwrap_or(usize::MAX);
    let relative = usize::try_from((x_value - bounds.0).max(0)).unwrap_or(usize::MAX);
    let offset = ((plot_width.saturating_sub(1)) * relative + span / 2) / span;
    Some(area.x + (start + offset).min(area.width as usize - 1) as u16)
}

fn graph_bounds(span_seconds: u32, offset_seconds: u32) -> (i64, i64) {
    let right = -(i64::from(offset_seconds));
    let left = right - i64::from(span_seconds.max(1));
    (left, right)
}

fn chart_points(
    samples: &[GraphSample],
    bounds: (i64, i64),
    time_reference_at: Option<DateTime<Local>>,
) -> Vec<(f64, f64)> {
    let Some(time_reference_at) = time_reference_at else {
        return Vec::new();
    };

    let mut points = Vec::new();
    for sample in samples.iter().rev() {
        let age = time_reference_at
            .signed_duration_since(sample.captured_at)
            .num_seconds()
            .max(0);
        let x = -(age as f64);
        if x < bounds.0 as f64 {
            break;
        }
        if x > bounds.1 as f64 {
            continue;
        }
        if let Some(value) = sample.value {
            points.push((x, value));
        }
    }
    points.reverse();
    points
}

fn chart_segments(
    samples: &[GraphSample],
    bounds: (i64, i64),
    time_reference_at: Option<DateTime<Local>>,
    frame_times: Option<&[DateTime<Local>]>,
) -> Vec<Vec<(f64, f64)>> {
    let Some(frame_times) = frame_times.filter(|times| !times.is_empty()) else {
        let points = chart_points(samples, bounds, time_reference_at);
        return (!points.is_empty()).then_some(points).into_iter().collect();
    };
    let Some(time_reference_at) = time_reference_at else {
        return Vec::new();
    };

    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut sample_index = 0_usize;
    for captured_at in frame_times {
        while sample_index < samples.len() && samples[sample_index].captured_at < *captured_at {
            sample_index += 1;
        }
        let value = samples
            .get(sample_index)
            .filter(|sample| sample.captured_at == *captured_at)
            .and_then(|sample| sample.value);
        let age = time_reference_at
            .signed_duration_since(*captured_at)
            .num_seconds()
            .max(0);
        let x = -(age as f64);
        if x < bounds.0 as f64 {
            continue;
        }
        if x > bounds.1 as f64 {
            break;
        }
        if let Some(value) = value {
            current.push((x, value));
        } else if !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn chart_segments_preserving_sample_gaps(
    samples: &[GraphSample],
    bounds: (i64, i64),
    time_reference_at: Option<DateTime<Local>>,
    frame_times: Option<&[DateTime<Local>]>,
) -> Vec<Vec<(f64, f64)>> {
    if frame_times.is_some_and(|times| !times.is_empty()) {
        return chart_segments(samples, bounds, time_reference_at, frame_times);
    }
    let sample_times = samples
        .iter()
        .map(|sample| sample.captured_at)
        .collect::<Vec<_>>();
    chart_segments(samples, bounds, time_reference_at, Some(&sample_times))
}

fn lift_floor_points_for_plot(points: &[(f64, f64)], y_min: f64, y_max: f64) -> Vec<(f64, f64)> {
    let floor_y = floor_plot_value(y_min, y_max);
    points
        .iter()
        .map(|(x, y)| {
            (
                *x,
                if (*y - y_min).abs() <= f64::EPSILON {
                    floor_y
                } else {
                    *y
                },
            )
        })
        .collect()
}

fn floor_plot_value(y_min: f64, y_max: f64) -> f64 {
    let span = (y_max - y_min).max(1.0);
    y_min + (span * 0.05).max(f64::EPSILON)
}

#[cfg(test)]
fn selected_sample_line_points(
    samples: &[GraphSample],
    selected: usize,
    y_min: f64,
    y_max: f64,
    bounds: (i64, i64),
) -> Vec<(f64, f64)> {
    let Some(latest) = samples.last().map(|sample| sample.captured_at) else {
        return Vec::new();
    };
    let Some(sample) = samples.get(selected) else {
        return Vec::new();
    };
    let age = latest
        .signed_duration_since(sample.captured_at)
        .num_seconds()
        .max(0);
    let x = -(age as f64);
    if x < bounds.0 as f64 || x > bounds.1 as f64 {
        return Vec::new();
    }
    vec![(x, y_min), (x, y_max)]
}

fn selected_age_line_points(
    age_seconds: i64,
    y_min: f64,
    y_max: f64,
    bounds: (i64, i64),
    plot_height: u16,
) -> Vec<(f64, f64)> {
    let x = -(age_seconds.max(0) as f64);
    if x < bounds.0 as f64 || x > bounds.1 as f64 {
        return Vec::new();
    }
    vec![
        (x, graph_guide_bottom_value(y_min, y_max, plot_height)),
        (x, y_max),
    ]
}

fn ab_line_points(
    time_reference_at: Option<DateTime<Local>>,
    point: AbComparisonPoint,
    y_min: f64,
    y_max: f64,
    bounds: (i64, i64),
    plot_height: u16,
) -> Vec<(f64, f64)> {
    let Some(time_reference_at) = time_reference_at else {
        return Vec::new();
    };
    let age = time_reference_at
        .signed_duration_since(point.captured_at)
        .num_seconds()
        .max(0);
    let x = -(age as f64);
    if x < bounds.0 as f64 || x > bounds.1 as f64 {
        return Vec::new();
    }
    vec![
        (x, graph_guide_bottom_value(y_min, y_max, plot_height)),
        (x, y_max),
    ]
}

fn graph_guide_bottom_value(y_min: f64, y_max: f64, plot_height: u16) -> f64 {
    const BRAILLE_DOTS_PER_CELL_Y: f64 = 4.0;

    if plot_height <= 1 || y_max <= y_min {
        return y_min;
    }
    let vertical_resolution = f64::from(plot_height) * BRAILLE_DOTS_PER_CELL_Y;
    let one_cell = (y_max - y_min) * BRAILLE_DOTS_PER_CELL_Y / (vertical_resolution - 1.0);
    (y_min + one_cell).min(y_max)
}

fn exact_unit_suffix(metric: GraphValueFormat) -> &'static str {
    match metric {
        GraphValueFormat::Bytes => " B",
        GraphValueFormat::BytesPerSec => " B/s",
        _ => "",
    }
}

fn fit_graph_label(label: &str, width: usize) -> String {
    if Line::from(label).width() <= width {
        return label.to_string();
    }
    let mut result = String::new();
    for ch in label.chars() {
        if Line::from(format!("{result}{ch}…")).width() > width {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::THEMES;
    use chrono::TimeZone;

    fn sample(
        captured_at: chrono::DateTime<chrono::Local>,
        private_bytes: Option<u64>,
        _workset_private_bytes: Option<u64>,
    ) -> GraphSample {
        GraphSample {
            captured_at,
            value: private_bytes.map(|value| value as f64),
        }
    }

    #[test]
    fn graph_stats_report_current_and_max() {
        let now = chrono::Local::now();
        let samples = [
            sample(now, Some(10), Some(5)),
            sample(now, Some(30), Some(7)),
        ];
        let points = chart_points(&samples, (-60, 0), Some(now));

        assert_eq!(
            graph_stats(&samples, Some(50.0), &points),
            GraphStats {
                current: Some(30.0),
                window_min: Some(10.0),
                window_max: Some(30.0),
                max: Some(50.0),
                scale_max: 30.0,
            }
        );
    }

    #[test]
    fn graph_y_bounds_can_follow_visible_minimum() {
        let stats = GraphStats {
            current: Some(30.0),
            window_min: Some(20.0),
            window_max: Some(30.0),
            max: None,
            scale_max: 30.0,
        };

        assert_eq!(graph_y_bounds(&stats, true), (0.0, 30.0));
        assert_eq!(graph_y_bounds(&stats, false), (10.0, 30.0));
    }

    #[test]
    fn graph_y_bounds_use_readable_ticks_below_visible_minimum() {
        let stats = GraphStats {
            current: Some(2_863_476_736.0),
            window_min: Some(2_863_476_736.0),
            window_max: Some(2_863_476_736.0),
            max: None,
            scale_max: 2_900_000_000.0,
        };

        assert_eq!(
            graph_y_bounds(&stats, false),
            (2_860_000_000.0, 2_900_000_000.0)
        );
        assert_eq!(
            y_axis_labels(2_860_000_000.0, 2_900_000_000.0, GraphValueFormat::Bytes),
            vec![
                "2.86 GB".to_string(),
                "2.88 GB".to_string(),
                "2.90 GB".to_string()
            ]
        );
    }

    #[test]
    fn byte_axis_ticks_are_compact_while_exact_graph_values_remain_integers() {
        assert_eq!(
            y_axis_labels(0.0, 5_900_000.0, GraphValueFormat::Bytes),
            vec!["0".to_string(), "3.0 MB".to_string(), "5.9 MB".to_string()]
        );
        assert_eq!(
            format_metric_exact_value(5_900_123.0, GraphValueFormat::Bytes),
            "5,900,123"
        );
        assert_eq!(format_ab_delta(123.0, GraphValueFormat::Bytes), "+123");
    }

    #[test]
    fn count_axis_ticks_and_exact_values_remain_integers() {
        assert_eq!(
            y_axis_labels(0.0, 12_400.0, GraphValueFormat::Count),
            vec!["0".to_string(), "6,200".to_string(), "12,400".to_string()]
        );
        assert_eq!(
            format_metric_exact_value(12_345.0, GraphValueFormat::Count),
            "12,345"
        );
    }

    #[test]
    fn process_io_graph_values_switch_units_below_one_mbps() {
        assert_eq!(
            format_metric_exact_value(62_500.0, GraphValueFormat::AdaptiveBitsPerSec),
            "500 Kbps"
        );
        assert_eq!(
            format_metric_exact_value(125_000.0, GraphValueFormat::AdaptiveBitsPerSec),
            "1 Mbps"
        );
        assert_eq!(
            format_ab_delta(-62_500.0, GraphValueFormat::AdaptiveBitsPerSec),
            "-500 Kbps"
        );
        assert_eq!(
            format_metric_exact_value(62_500.0, GraphValueFormat::MegabitsPerSec),
            "1 Mbps"
        );
    }

    #[test]
    fn sample_max_line_reports_max_value_and_time() {
        let first = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let second = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 5)
            .unwrap();
        let samples = [
            sample(first, Some(1_000), Some(5)),
            sample(second, Some(3_000), Some(7)),
        ];
        let refs = samples.to_vec();
        let rendered = sample_max_line(&refs, GraphValueFormat::Count, THEMES[0], None)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(rendered, "History max: 3,000 @ 10:00:05");
    }

    #[test]
    fn sample_moving_average_uses_selected_sample_as_window_end() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [
            sample(base, Some(10), None),
            sample(base + chrono::Duration::seconds(1), Some(20), None),
            sample(base + chrono::Duration::seconds(2), Some(30), None),
            sample(base + chrono::Duration::seconds(3), Some(40), None),
            sample(base + chrono::Duration::seconds(4), Some(50), None),
            sample(base + chrono::Duration::seconds(5), Some(110), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            sample_moving_average(&refs, 5, None),
            Some((base + chrono::Duration::seconds(5), 50.0))
        );
    }

    #[test]
    fn sample_moving_average_requires_five_contiguous_available_values() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [
            sample(base, Some(10), None),
            sample(base + chrono::Duration::seconds(1), None, None),
            sample(base + chrono::Duration::seconds(2), Some(30), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(sample_moving_average(&refs, 2, None), None);
        assert_eq!(
            sample_moving_average_line(&refs, 1, GraphValueFormat::Count, THEMES[0], None, None)
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join(""),
            "Selected MA5: --"
        );
    }

    #[test]
    fn sample_moving_average_reports_missing_when_window_has_no_values() {
        let now = chrono::Local::now();
        let samples = [sample(now, None, None)];
        let refs = samples.to_vec();

        assert_eq!(sample_moving_average(&refs, 0, None), None);
        assert_eq!(
            sample_moving_average_line(&refs, 0, GraphValueFormat::Count, THEMES[0], None, None)
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join(""),
            "Selected MA5: --"
        );
    }

    #[test]
    fn moving_average_warms_up_rolls_and_stays_linear_at_history_capacity() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = (0..7_200)
            .map(|index| GraphSample {
                captured_at: base + chrono::Duration::seconds(index as i64),
                value: Some(index as f64),
            })
            .collect::<Vec<_>>();

        let averaged = moving_average_samples(&samples, None);

        assert_eq!(averaged.len(), samples.len());
        assert!(averaged[..4].iter().all(|sample| sample.value.is_none()));
        assert_eq!(averaged[4].value, Some(2.0));
        assert_eq!(averaged[5].value, Some(3.0));
        assert_eq!(averaged.last().unwrap().value, Some(7_197.0));

        let total_points = (0..16)
            .map(|_| moving_average_samples(&samples, None).len())
            .sum::<usize>();
        assert_eq!(total_points, 16 * 7_200);
    }

    #[test]
    fn moving_average_resets_for_missing_values_and_missing_frames() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let frame_times = (0..9)
            .map(|index| base + chrono::Duration::seconds(index))
            .collect::<Vec<_>>();
        let samples = frame_times
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 3)
            .map(|(index, captured_at)| GraphSample {
                captured_at: *captured_at,
                value: Some(index as f64),
            })
            .collect::<Vec<_>>();

        let averaged = moving_average_samples(&samples, Some(&frame_times));

        assert_eq!(averaged[3].value, None);
        assert!(averaged[4..8].iter().all(|sample| sample.value.is_none()));
        assert_eq!(averaged[8].value, Some(6.0));

        let mut unavailable = samples;
        unavailable.insert(
            3,
            GraphSample {
                captured_at: frame_times[3],
                value: None,
            },
        );
        let unavailable_average = moving_average_samples(&unavailable, Some(&frame_times));
        assert_eq!(unavailable_average, averaged);
    }

    #[test]
    fn moving_average_segments_never_connect_across_a_gap() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = (0..11)
            .map(|index| GraphSample {
                captured_at: base + chrono::Duration::seconds(index),
                value: (index != 5).then_some(index as f64),
            })
            .collect::<Vec<_>>();
        let averaged = moving_average_samples(&samples, None);

        assert_eq!(
            chart_segments_preserving_sample_gaps(
                &averaged,
                (-60, 0),
                samples.last().map(|sample| sample.captured_at),
                None,
            ),
            vec![vec![(-6.0, 2.0)], vec![(0.0, 8.0)]]
        );
    }

    #[test]
    fn moving_average_cursor_formats_cover_graph_metric_kinds() {
        let sample = GraphSample {
            captured_at: chrono::Local::now(),
            value: Some(30.0),
        };
        for (metric, expected) in [
            (GraphValueFormat::Bytes, "MA5: 30 B"),
            (GraphValueFormat::Count, "MA5: 30"),
            (GraphValueFormat::BytesPerSec, "MA5: 30 B/s"),
            (GraphValueFormat::Percent, "MA5: 30.0%"),
            (GraphValueFormat::QueueLength, "MA5: 30.0"),
        ] {
            assert_eq!(
                format_graph_cursor_value(&sample, metric, GraphDisplayMode::MovingAverage5, None,)
                    .as_deref(),
                Some(expected)
            );
        }
        assert_eq!(
            format_graph_cursor_value(
                &sample,
                GraphValueFormat::Count,
                GraphDisplayMode::Raw,
                None,
            )
            .as_deref(),
            Some("30")
        );
    }

    #[test]
    fn moving_average_log_labels_disclose_every_supported_frame_interval() {
        let sample = GraphSample {
            captured_at: chrono::Local::now(),
            value: Some(30.0),
        };
        for interval in [1, 2, 5, 10] {
            assert_eq!(
                format_graph_cursor_value(
                    &sample,
                    GraphValueFormat::Count,
                    GraphDisplayMode::MovingAverage5,
                    Some(interval),
                )
                .unwrap(),
                format!("MA5 (5×{interval}s avg): 30")
            );
        }
    }

    #[test]
    fn moving_average_y_axis_uses_only_displayed_values() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [0.0, 100.0, 0.0, 100.0, 0.0]
            .into_iter()
            .enumerate()
            .map(|(index, value)| GraphSample {
                captured_at: base + chrono::Duration::seconds(index as i64),
                value: Some(value),
            })
            .collect::<Vec<_>>();
        let raw_points = chart_points(&samples, (-60, 0), samples.last().map(|s| s.captured_at));
        let averaged = moving_average_samples(&samples, None);
        let averaged_points =
            chart_points(&averaged, (-60, 0), samples.last().map(|s| s.captured_at));
        let raw_stats =
            graph_stats_for_values(&samples, None, raw_points.iter().map(|(_, value)| *value));
        let averaged_stats = graph_stats_for_values(
            &samples,
            None,
            averaged_points.iter().map(|(_, value)| *value),
        );

        assert_eq!(raw_stats.window_max, Some(100.0));
        assert_eq!(averaged_stats.window_min, Some(40.0));
        assert_eq!(averaged_stats.window_max, Some(40.0));
        assert!(graph_y_bounds(&averaged_stats, false).1 < graph_y_bounds(&raw_stats, false).1);
    }

    #[test]
    fn aggregate_sample_summaries_expose_interval_semantics() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [
            sample(now - chrono::Duration::seconds(40), Some(10), None),
            sample(now - chrono::Duration::seconds(30), Some(20), None),
            sample(now - chrono::Duration::seconds(20), Some(30), None),
            sample(now - chrono::Duration::seconds(10), Some(40), None),
            sample(now, Some(50), None),
        ];
        let refs = samples.to_vec();
        let max = sample_max_line(&refs, GraphValueFormat::Count, THEMES[0], Some(10))
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        let average = sample_moving_average_line(
            &refs,
            4,
            GraphValueFormat::Count,
            THEMES[0],
            Some(10),
            None,
        )
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("");

        assert_eq!(max, "History max (10s avg): 50 @ 10:00:00");
        assert_eq!(average, "Selected MA5 (5×10s avg): 30 @ 10:00:00");
    }

    #[test]
    fn sample_metric_value_column_uses_metric_max_width() {
        assert_eq!(SAMPLE_METRIC_VALUE_WIDTH, 15);
        assert_eq!(
            format!(
                "{:>SAMPLE_METRIC_VALUE_WIDTH$}",
                format_integer(999_999_999_999)
            )
            .chars()
            .count(),
            SAMPLE_METRIC_VALUE_WIDTH
        );
        assert_eq!(
            format!(
                "{:>SAMPLE_DELTA_WIDTH$}",
                format_signed_integer(99_999_999_999)
            )
            .chars()
            .count(),
            SAMPLE_DELTA_WIDTH
        );
    }

    #[test]
    fn nice_axis_max_rounds_up_to_readable_value() {
        assert_eq!(nice_axis_max(5_335_224_320), 5_400_000_000);
        assert_eq!(nice_axis_max(3_178_864_640), 3_200_000_000);
    }

    #[test]
    fn axis_tick_positions_are_evenly_spaced() {
        let positions = axis_tick_positions(81, 14);
        assert_eq!(positions, vec![13, 30, 47, 63, 80]);

        let gaps = positions.windows(2).map(|pair| pair[1] - pair[0]);
        let min_gap = gaps.clone().min().expect("tick gaps should exist");
        let max_gap = gaps.max().expect("tick gaps should exist");
        assert!(max_gap - min_gap <= 1);
    }

    #[test]
    fn time_axis_labels_remain_whole_and_separated_at_every_width() {
        let latest = Local.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
        let labels = graph_tick_labels((-60, 0), Some(latest));
        for y_width in [0, 8, 14] {
            for width in 0..=120 {
                let line = axis_tick_label_line(
                    width,
                    y_width,
                    (-60, 0),
                    Some(latest),
                    crate::ui::THEMES[0],
                );
                let text = line.to_string();
                assert_eq!(text.chars().count(), width);
                for token in text.split_whitespace() {
                    assert!(
                        labels.iter().any(|label| label == token),
                        "width {width}: {text}"
                    );
                }
                if width >= 30 {
                    assert!(text.contains(&labels[0]), "{text}");
                    assert!(text.contains(&labels[4]), "{text}");
                }
                if width >= 81 {
                    assert!(labels.iter().all(|label| text.contains(label)), "{text}");
                }
            }
        }
    }

    #[test]
    fn y_axis_middle_tick_matches_chart_label_row() {
        assert_eq!(y_axis_tick_positions(17), [0, 8, 16]);
        assert_eq!(y_axis_tick_positions(18), [0, 9, 17]);
    }

    #[test]
    fn y_axis_labels_show_cpu_percent_with_one_decimal_place() {
        assert_eq!(
            y_axis_labels(0.0, 1.0, GraphValueFormat::Percent),
            vec!["0".to_string(), "0.5%".to_string(), "1.0%".to_string()]
        );
        assert_eq!(
            y_axis_labels(0.0, 2.0, GraphValueFormat::Percent),
            vec!["0".to_string(), "1.0%".to_string(), "2.0%".to_string()]
        );
        assert_eq!(
            y_axis_labels(20.0, 30.0, GraphValueFormat::Percent),
            vec![
                "20.0%".to_string(),
                "25.0%".to_string(),
                "30.0%".to_string()
            ]
        );
    }

    #[test]
    fn graph_tick_labels_use_clock_time() {
        let latest = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 14, 0)
            .unwrap();

        assert_eq!(
            graph_tick_labels((-240, 0), Some(latest)),
            vec!["10:10:00", "10:11:00", "10:12:00", "10:13:00", "10:14:00"]
        );
    }

    #[test]
    fn chart_axis_overlay_preserves_zero_value_line_cells() {
        let theme = THEMES[0];
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 4));
        buffer[(5, 3)]
            .set_symbol("⠉")
            .set_style(Style::default().fg(theme.graph_line));

        ChartAxisOverlay {
            y_label_width: 2,
            theme,
        }
        .render(Rect::new(0, 0, 12, 4), &mut buffer);

        assert_eq!(buffer[(5, 3)].symbol(), "⠉");
    }

    #[test]
    fn graph_ab_axis_labels_draw_on_x_axis_without_clearing_plot_cells() {
        let theme = THEMES[0];
        let latest = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 1, 0)
            .unwrap();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint {
                captured_at: latest - chrono::Duration::seconds(30),
            }),
            b: Some(AbComparisonPoint {
                captured_at: latest,
            }),
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 5));
        buffer[(4, 3)]
            .set_symbol("x")
            .set_style(Style::default().fg(theme.graph_line).bg(theme.panel));

        GraphAbAxisLabels {
            y_label_width: 4,
            bounds: (-60, 0),
            time_reference_at: Some(latest),
            comparison: Some(&comparison),
            theme,
        }
        .render(Rect::new(0, 0, 20, 5), &mut buffer);

        assert_eq!(buffer[(11, 4)].symbol(), "A");
        assert_eq!(buffer[(11, 4)].fg, theme.accent);
        assert_eq!(buffer[(19, 4)].symbol(), "B");
        assert_eq!(buffer[(19, 4)].fg, theme.accent);
        assert_eq!(buffer[(4, 3)].symbol(), "x");
    }

    #[test]
    fn graph_ab_line_stops_one_terminal_row_above_axis_label() {
        let latest = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 1, 0)
            .unwrap();
        let line = ab_line_points(
            Some(latest),
            AbComparisonPoint {
                captured_at: latest,
            },
            0.0,
            100.0,
            (-60, 0),
            10,
        );

        assert_eq!(line.len(), 2);
        assert!((line[0].1 - (400.0 / 39.0)).abs() < f64::EPSILON);
        assert_eq!(line[1].1, 100.0);
    }

    #[test]
    fn graph_cursor_line_stops_one_terminal_row_above_axis() {
        let line = selected_age_line_points(0, 0.0, 100.0, (-60, 0), 10);

        assert_eq!(line.len(), 2);
        assert!((line[0].1 - (400.0 / 39.0)).abs() < f64::EPSILON);
        assert_eq!(line[1].1, 100.0);
    }

    #[test]
    fn sample_delta_uses_previous_sample_value() {
        assert_eq!(
            format_sample_delta(Some(130.0), Some(100.0), GraphValueFormat::Count),
            "+30"
        );
        assert_eq!(
            format_sample_delta(Some(70.0), Some(100.0), GraphValueFormat::Count),
            "-30"
        );
        assert_eq!(
            format_sample_delta(Some(6.5), Some(5.0), GraphValueFormat::Percent),
            "+1.5%"
        );
        assert_eq!(
            format_sample_delta(Some(70.0), None, GraphValueFormat::Count),
            "--"
        );
    }

    #[test]
    fn graph_series_uses_active_color_only_for_the_active_slot() {
        for theme in crate::ui::theme::THEMES {
            assert_eq!(
                graph_series_style(theme, true).fg,
                Some(theme.active_series)
            );
            assert_eq!(graph_series_style(theme, false).fg, Some(theme.graph_line));
            assert_eq!(
                selected_cursor_line_style(theme).fg,
                Some(theme.cursor_guide)
            );
            assert_eq!(
                delta_style(Some(2.0), Some(1.0), false, theme).fg,
                Some(theme.muted)
            );
            assert_eq!(
                delta_style(Some(1.0), Some(2.0), false, theme).fg,
                Some(theme.muted)
            );
            assert_eq!(
                delta_style(Some(1.0), Some(1.0), false, theme).fg,
                Some(theme.muted)
            );
            assert_eq!(
                delta_style(Some(1.0), Some(2.0), true, theme).fg,
                Some(theme.text)
            );
            assert_eq!(
                delta_style(Some(1.0), None, true, theme).fg,
                Some(theme.muted)
            );
        }
    }

    #[test]
    fn ab_range_statistics_are_inclusive_chronological_and_choose_earliest_ties() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [
            GraphSample {
                captured_at: base,
                value: Some(3.0),
            },
            GraphSample {
                captured_at: base + chrono::Duration::seconds(1),
                value: None,
            },
            GraphSample {
                captured_at: base + chrono::Duration::seconds(2),
                value: Some(1.0),
            },
            GraphSample {
                captured_at: base + chrono::Duration::seconds(3),
                value: Some(1.0),
            },
            GraphSample {
                captured_at: base + chrono::Duration::seconds(4),
                value: Some(5.0),
            },
            GraphSample {
                captured_at: base + chrono::Duration::seconds(5),
                value: Some(5.0),
            },
        ];
        let frame_times = (0..=5)
            .map(|seconds| base + chrono::Duration::seconds(seconds))
            .collect::<Vec<_>>();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: base }),
            b: Some(AbComparisonPoint {
                captured_at: base + chrono::Duration::seconds(5),
            }),
        };

        let expected = AbRangeStatistics {
            min_value: 1.0,
            min_captured_at: base + chrono::Duration::seconds(2),
            max_value: 5.0,
            max_captured_at: base + chrono::Duration::seconds(4),
            mean: 3.0,
            available_sample_count: 5,
            expected_frame_count: Some(6),
            missing_frame_count: Some(1),
        };
        assert_eq!(
            ab_range_statistics(Some(&comparison), &samples, Some(&frame_times)),
            Some(expected)
        );

        let reversed = AbComparison {
            a: comparison.b,
            b: comparison.a,
        };
        assert_eq!(
            ab_range_statistics(Some(&reversed), &samples, Some(&frame_times)),
            Some(expected)
        );

        let equal = AbComparison {
            a: Some(AbComparisonPoint {
                captured_at: base + chrono::Duration::seconds(2),
            }),
            b: Some(AbComparisonPoint {
                captured_at: base + chrono::Duration::seconds(2),
            }),
        };
        assert_eq!(
            ab_range_statistics(Some(&equal), &samples, Some(&frame_times)),
            Some(AbRangeStatistics {
                min_value: 1.0,
                min_captured_at: base + chrono::Duration::seconds(2),
                max_value: 1.0,
                max_captured_at: base + chrono::Duration::seconds(2),
                mean: 1.0,
                available_sample_count: 1,
                expected_frame_count: Some(1),
                missing_frame_count: Some(0),
            })
        );
    }

    #[test]
    fn ab_range_statistics_exclude_missing_values_and_absent_frames() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let frame_times = (0..=2)
            .map(|seconds| base + chrono::Duration::seconds(seconds))
            .collect::<Vec<_>>();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: base }),
            b: Some(AbComparisonPoint {
                captured_at: base + chrono::Duration::seconds(2),
            }),
        };
        let one_available = [GraphSample {
            captured_at: base + chrono::Duration::seconds(1),
            value: Some(7.0),
        }];

        assert_eq!(
            ab_range_statistics(Some(&comparison), &one_available, Some(&frame_times)),
            Some(AbRangeStatistics {
                min_value: 7.0,
                min_captured_at: base + chrono::Duration::seconds(1),
                max_value: 7.0,
                max_captured_at: base + chrono::Duration::seconds(1),
                mean: 7.0,
                available_sample_count: 1,
                expected_frame_count: Some(3),
                missing_frame_count: Some(2),
            })
        );
        let without_frame_sequence =
            ab_range_statistics(Some(&comparison), &one_available, None).unwrap();
        assert_eq!(without_frame_sequence.expected_frame_count, None);
        assert_eq!(without_frame_sequence.missing_frame_count, None);
        let sample_count = sample_ab_range_summary_lines(
            &without_frame_sequence,
            GraphValueFormat::Count,
            THEMES[0],
            None,
        )[3]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("");
        assert_eq!(sample_count, "A-B samples: 1");
        assert_eq!(
            ab_range_statistics(
                Some(&comparison),
                &[GraphSample {
                    captured_at: base + chrono::Duration::seconds(1),
                    value: None,
                }],
                Some(&frame_times),
            ),
            None
        );
        assert_eq!(ab_range_statistics(None, &one_available, None), None);
        assert_eq!(
            ab_range_statistics(
                Some(&AbComparison {
                    a: comparison.a,
                    b: None,
                }),
                &one_available,
                None,
            ),
            None
        );
    }

    #[test]
    fn ab_range_summary_formats_every_metric_and_recording_interval() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let statistics = AbRangeStatistics {
            min_value: 10.0,
            min_captured_at: base,
            max_value: 30.0,
            max_captured_at: base + chrono::Duration::seconds(2),
            mean: 20.0,
            available_sample_count: 2,
            expected_frame_count: Some(3),
            missing_frame_count: Some(1),
        };
        for (metric, expected_mean) in [
            (GraphValueFormat::Bytes, "20"),
            (GraphValueFormat::BytesPerSec, "20/s"),
            (GraphValueFormat::Count, "20"),
            (GraphValueFormat::Percent, "20.0%"),
            (GraphValueFormat::AdaptiveBitsPerSec, "0 Kbps"),
            (GraphValueFormat::MegabitsPerSec, "0 Mbps"),
            (GraphValueFormat::MegabytesPerSec, "0.0 MB/s"),
            (GraphValueFormat::QueueLength, "20.0"),
        ] {
            let rendered = sample_ab_range_summary_lines(&statistics, metric, THEMES[0], None)
                .into_iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .collect::<Vec<_>>();
            assert_eq!(
                rendered[0],
                format!(
                    "A-B min: {} @ 10:00:00",
                    format_metric_exact_value(10.0, metric)
                )
            );
            assert_eq!(rendered[2], format!("A-B avg: {expected_mean}"));
            assert_eq!(rendered[3], "A-B samples: 2/3  Missing: 1");
        }

        for interval in [1, 2, 5, 10] {
            let first = sample_ab_range_summary_lines(
                &statistics,
                GraphValueFormat::Count,
                THEMES[0],
                Some(interval),
            )[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
            assert_eq!(first, format!("A-B min ({interval}s avg): 10 @ 10:00:00"));
        }
    }

    #[test]
    fn sample_ab_marker_matches_point_times() {
        let now = chrono::Local::now();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: now }),
            b: Some(AbComparisonPoint { captured_at: now }),
        };

        assert_eq!(sample_ab_marker(Some(&comparison), now), "AB");
        assert_eq!(
            sample_ab_marker(Some(&comparison), now + chrono::Duration::seconds(1)),
            ""
        );
    }

    #[test]
    fn sample_ab_summary_lines_format_points_and_delta_vertically() {
        let first = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let second = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 5)
            .unwrap();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: first }),
            b: Some(AbComparisonPoint {
                captured_at: second,
            }),
        };
        let samples = [
            sample(first, Some(1_000), None),
            sample(second, Some(1_500), None),
        ];
        let refs = samples.to_vec();
        let lines =
            sample_ab_summary_lines(Some(&comparison), &refs, GraphValueFormat::Bytes, THEMES[0]);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>();

        assert!(lines.iter().all(|line| {
            line.spans[0].style.fg == Some(THEMES[0].accent)
                && line.spans[0].style.fg != Some(THEMES[0].warning)
        }));
        assert_eq!(
            rendered,
            vec!["A: 10:00:00 1,000", "B: 10:00:05 1,500", "B-A: +500 (+5s)"]
        );
    }

    #[test]
    fn sample_ab_summary_lines_keep_partial_points_compact() {
        let first = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: first }),
            b: None,
        };
        let samples = [sample(first, Some(1_000), None)];
        let refs = samples.to_vec();
        let rendered =
            sample_ab_summary_lines(Some(&comparison), &refs, GraphValueFormat::Bytes, THEMES[0])
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["A: 10:00:00 1,000", "B: --", "B-A: --"]);
    }

    #[test]
    fn format_elapsed_delta_uses_signed_compact_units() {
        assert_eq!(format_elapsed_delta(chrono::Duration::seconds(5)), "+5s");
        assert_eq!(
            format_elapsed_delta(chrono::Duration::seconds(-65)),
            "-1m05s"
        );
        assert_eq!(
            format_elapsed_delta(chrono::Duration::seconds(3_725)),
            "+1h02m05s"
        );
    }

    #[test]
    fn sample_viewport_uses_explicit_offset() {
        assert_eq!(sample_viewport_bounds(20, 0, 5), (0, 5));
        assert_eq!(sample_viewport_bounds(20, 3, 5), (3, 8));
        assert_eq!(sample_viewport_bounds(20, 18, 5), (15, 20));
    }

    #[test]
    fn synced_sample_viewport_keeps_selected_time_on_same_visible_row() {
        assert_eq!(synced_sample_viewport_offset(20, 5, 10, 7, 5), 8);
        assert_eq!(synced_sample_viewport_offset(20, 5, 1, 7, 5), 0);
        assert_eq!(synced_sample_viewport_offset(20, 5, 19, 7, 5), 15);
    }

    #[test]
    fn sample_index_at_time_requires_exact_timestamp_but_nearest_can_center_viewport() {
        let base = Local.with_ymd_and_hms(2026, 5, 10, 10, 0, 0).unwrap();
        let samples = [
            sample(base, Some(10), None),
            sample(base + chrono::Duration::seconds(2), Some(20), None),
            sample(base + chrono::Duration::seconds(4), Some(30), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            sample_index_at_time(&refs, base + chrono::Duration::seconds(2)),
            Some(1)
        );
        assert_eq!(
            sample_index_at_time(&refs, base + chrono::Duration::seconds(3)),
            None
        );
        assert_eq!(
            sample_index_nearest_time(&refs, base + chrono::Duration::seconds(3)),
            Some(2)
        );
    }

    #[test]
    fn sample_index_nearest_age_prefers_latest_row_when_age_ties() {
        let base = Local.with_ymd_and_hms(2026, 5, 10, 10, 0, 0).unwrap();
        let samples = [
            sample(base, Some(10), None),
            sample(base + chrono::Duration::milliseconds(400), Some(20), None),
            sample(base + chrono::Duration::milliseconds(800), Some(30), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(sample_index_nearest_age_seconds(&refs, 0), Some(2));
    }

    #[test]
    fn sample_index_at_age_requires_exact_sample_time() {
        let now = chrono::Local::now();
        let samples = [
            sample(now - chrono::Duration::seconds(30), Some(10), None),
            sample(now, Some(20), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(sample_index_at_age_seconds(&refs, 30), Some(0));
        assert_eq!(sample_index_at_age_seconds(&refs, 15), None);
        assert_eq!(sample_index_nearest_age_seconds(&refs, 15), Some(1));
    }

    #[test]
    fn samples_scrollbar_position_reaches_end_at_last_viewport() {
        assert_eq!(samples_scrollbar_position(100, 10, 0), 0);
        assert_eq!(samples_scrollbar_position(100, 10, 90), 99);
    }

    #[test]
    fn chart_points_use_negative_age_seconds() {
        let now = chrono::Local::now();
        let samples = [
            sample(now - chrono::Duration::seconds(15), Some(10), None),
            sample(now, Some(20), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            chart_points(&refs, (-60, 0), Some(now)),
            vec![(-15.0, 10.0), (0.0, 20.0)]
        );
    }

    #[test]
    fn chart_points_preserve_supported_aggregate_intervals() {
        let now = chrono::Local::now();
        for interval in [1_i64, 2, 5, 10] {
            let samples = [
                sample(now - chrono::Duration::seconds(interval), Some(10), None),
                sample(now, Some(20), None),
            ];
            assert_eq!(
                chart_points(&samples, (-60, 0), Some(now)),
                vec![-(interval as f64), 0.0]
                    .into_iter()
                    .zip([10.0, 20.0])
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn chart_segments_preserve_missing_log_frames_as_gaps() {
        let base = chrono::Local::now();
        let frame_times = [
            base - chrono::Duration::seconds(20),
            base - chrono::Duration::seconds(10),
            base,
        ];
        let samples = [
            sample(frame_times[0], Some(10), None),
            sample(frame_times[2], Some(30), None),
        ];

        assert_eq!(
            chart_segments(&samples, (-60, 0), Some(base), Some(&frame_times)),
            vec![vec![(-20.0, 10.0)], vec![(0.0, 30.0)]]
        );
    }

    #[test]
    fn chart_segments_keep_contiguous_log_frames_connected() {
        let base = chrono::Local::now();
        let frame_times = [
            base - chrono::Duration::seconds(20),
            base - chrono::Duration::seconds(10),
            base,
        ];
        let samples = [
            sample(frame_times[0], Some(10), None),
            sample(frame_times[1], Some(20), None),
            sample(frame_times[2], Some(30), None),
        ];

        assert_eq!(
            chart_segments(&samples, (-60, 0), Some(base), Some(&frame_times)),
            vec![vec![(-20.0, 10.0), (-10.0, 20.0), (0.0, 30.0)]]
        );
    }

    #[test]
    fn chart_points_skip_samples_outside_visible_bounds() {
        let now = chrono::Local::now();
        let samples = [
            sample(now - chrono::Duration::seconds(90), Some(5), None),
            sample(now - chrono::Duration::seconds(45), Some(10), None),
            sample(now - chrono::Duration::seconds(15), Some(15), None),
            sample(now, Some(20), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            chart_points(&refs, (-60, -10), Some(now)),
            vec![(-45.0, 10.0), (-15.0, 15.0)]
        );
    }

    #[test]
    fn chart_points_use_the_shared_graph_time_reference() {
        let shared_latest = chrono::Local::now();
        let samples = [
            sample(
                shared_latest - chrono::Duration::seconds(120),
                Some(10),
                None,
            ),
            sample(
                shared_latest - chrono::Duration::seconds(60),
                Some(20),
                None,
            ),
        ];

        assert_eq!(
            chart_points(&samples, (-180, 0), Some(shared_latest)),
            vec![(-120.0, 10.0), (-60.0, 20.0)]
        );
    }

    #[test]
    fn floor_chart_points_are_lifted_only_for_plotting() {
        let raw_points = vec![(-2.0, 0.0), (-1.0, 10.0), (0.0, 0.0)];

        assert_eq!(
            lift_floor_points_for_plot(&raw_points, 0.0, 100.0),
            vec![(-2.0, 5.0), (-1.0, 10.0), (0.0, 5.0)]
        );

        let auto_floor_points = vec![(-2.0, 20.0), (-1.0, 30.0)];
        assert_eq!(
            lift_floor_points_for_plot(&auto_floor_points, 20.0, 40.0),
            vec![(-2.0, 21.0), (-1.0, 30.0)]
        );
    }

    #[test]
    fn selected_sample_line_points_use_selected_age_seconds() {
        let now = chrono::Local::now();
        let samples = [
            sample(now - chrono::Duration::seconds(15), Some(10), None),
            sample(now, Some(20), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            selected_sample_line_points(&refs, 0, 0.0, 100.0, (-60, 0)),
            vec![(-15.0, 0.0), (-15.0, 100.0)]
        );
    }

    #[test]
    fn graph_slot_title_combines_slot_item_metric_and_missing_ab_delta() {
        let identity = crate::model::ProcessIdentity {
            pid: 42,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_000),
        };
        let slot = GraphSlot::process(identity, crate::app::DetailsMetric::Private);
        let line = graph_slot_title_line(
            &slot,
            0,
            &[],
            GraphValueFormat::Bytes,
            None,
            true,
            crate::ui::THEMES[0],
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(rendered, "Slot#1 · PrivBytes · PID 42 app.exe · B-A: --");
        assert!(
            line.spans[0].style.fg == Some(crate::ui::THEMES[0].active_series)
                && line.spans[0].style.add_modifier.contains(Modifier::BOLD)
        );
        assert_eq!(line.spans[5].style.fg, Some(crate::ui::THEMES[0].accent));
        assert_ne!(line.spans[5].style.fg, Some(crate::ui::THEMES[0].warning));
        assert!(
            !line.spans[2]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(!rendered.contains("Process Name:"));
        assert!(!rendered.contains("Target Metric:"));
        assert!(!rendered.contains("Live ON"));
        assert!(!rendered.contains("F7 Save CSV"));
        assert!(!rendered.contains("Samples:"));
        assert!(!rendered.contains("Start Time:"));
    }

    #[test]
    fn samples_title_shows_process_name_only_when_it_fits() {
        let theme = crate::ui::THEMES[0];
        let slot = GraphSlot::process(
            crate::model::ProcessIdentity {
                pid: 42,
                name: "app.exe".to_string(),
                start_time: Some(1_700_000_000),
            },
            crate::app::DetailsMetric::Private,
        );

        let wide = samples_inspector_title(&slot, 1, 32, theme);
        let narrow = samples_inspector_title(&slot, 1, 23, theme);
        let rendered = |line: &Line<'_>| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join("")
        };

        assert_eq!(rendered(&wide), "SAMPLES · Slot#2 · PID 42");
        assert_eq!(rendered(&narrow), "SAMPLES · PID 42");
        assert_eq!(wide.spans[2].style.fg, Some(theme.active_series));
        assert!(wide.spans[2].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn samples_title_does_not_add_a_process_name_to_system_graphs() {
        let theme = crate::ui::THEMES[0];
        let slot = GraphSlot::system(crate::model::SystemMetric::CpuAverage);
        let line = samples_inspector_title(&slot, 0, 80, theme);
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(rendered, "SAMPLES · Slot#1");
    }

    #[test]
    fn graph_slot_title_formats_ab_delta_for_its_own_samples() {
        let identity = crate::model::ProcessIdentity {
            pid: 42,
            name: "DeepL.exe".to_string(),
            start_time: Some(1_700_000_000),
        };
        let slot = GraphSlot::process(identity, crate::app::DetailsMetric::HandleCount);
        let first = Local.with_ymd_and_hms(2026, 7, 26, 20, 50, 56).unwrap();
        let second = first + chrono::Duration::seconds(5);
        let samples = [
            sample(first, Some(2_080), None),
            sample(second, Some(2_082), None),
        ];
        let comparison = AbComparison {
            a: Some(AbComparisonPoint { captured_at: first }),
            b: Some(AbComparisonPoint {
                captured_at: second,
            }),
        };

        let line = graph_slot_title_line(
            &slot,
            0,
            &samples,
            GraphValueFormat::Count,
            Some(&comparison),
            true,
            crate::ui::THEMES[0],
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(rendered, "Slot#1 · Hndl · PID 42 DeepL.exe · B-A: +2");
    }

    #[test]
    fn system_graph_slot_titles_use_panel_qualified_metric_names() {
        for (metric, expected_metric) in [
            (crate::model::SystemMetric::PhysicalMemory, "MEM In use"),
            (crate::model::SystemMetric::ModifiedMemory, "MEM Modified"),
            (crate::model::SystemMetric::StandbyMemory, "MEM Standby"),
            (
                crate::model::SystemMetric::FreeZeroedMemory,
                "MEM Free + Zeroed",
            ),
            (crate::model::SystemMetric::Committed, "MEM Commit charge"),
            (crate::model::SystemMetric::PagedPool, "MEM Paged Pool"),
            (
                crate::model::SystemMetric::NonpagedPool,
                "MEM Nonpaged Pool",
            ),
            (crate::model::SystemMetric::PagesInput, "MEM Pages In/s"),
            (crate::model::SystemMetric::PagesOutput, "MEM Pages Out/s"),
            (crate::model::SystemMetric::CpuAverage, "CPU Usage"),
            (crate::model::SystemMetric::ThreadCount, "CPU Threads"),
            (crate::model::SystemMetric::ProcessCount, "CPU Processes"),
            (
                crate::model::SystemMetric::NetworkReceived,
                "NW/DISK Net Rx",
            ),
            (crate::model::SystemMetric::NetworkSent, "NW/DISK Net Tx"),
            (crate::model::SystemMetric::DiskRead, "NW/DISK Disk R"),
            (crate::model::SystemMetric::DiskWrite, "NW/DISK Disk W"),
            (
                crate::model::SystemMetric::DiskQueueLength,
                "NW/DISK Disk Q",
            ),
        ] {
            let slot = GraphSlot::system(metric);
            let line = graph_slot_title_line(
                &slot,
                0,
                &[],
                slot.value_format(),
                None,
                true,
                crate::ui::THEMES[0],
            );
            let rendered = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join("");

            assert_eq!(rendered, format!("Slot#1 · {expected_metric} · B-A: --"));
            assert!(!rendered.contains("SYSTEM"));
        }
    }

    #[test]
    fn gpu_graph_slot_titles_use_compact_panel_qualified_metric_names() {
        for (metric, expected_metric) in [
            (crate::model::SystemMetric::GpuUtilization, "GPU Usage"),
            (crate::model::SystemMetric::GpuEncode, "GPU Encode"),
            (crate::model::SystemMetric::GpuDecode, "GPU Decode"),
            (crate::model::SystemMetric::GpuDedicated, "GPU Dedicated"),
            (crate::model::SystemMetric::GpuShared, "GPU Shared"),
        ] {
            let slot = GraphSlot::gpu(
                crate::model::GpuAdapterId::default(),
                "NVIDIA GeForce Test Adapter",
                metric,
            );
            let line = graph_slot_title_line(
                &slot,
                0,
                &[],
                slot.value_format(),
                None,
                true,
                crate::ui::THEMES[0],
            );
            let rendered = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join("");

            assert_eq!(rendered, format!("Slot#1 · {expected_metric} · B-A: --"));
            assert!(!rendered.contains("NVIDIA GeForce Test Adapter"));
            assert!(!rendered.contains("SYSTEM"));
        }
    }

    #[test]
    fn graph_metric_titles_omit_separate_unit_labels() {
        let identity = crate::model::ProcessIdentity {
            pid: 42,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_000),
        };

        assert_eq!(
            GraphSlot::process(identity.clone(), crate::app::DetailsMetric::CpuPercent)
                .metric_label(),
            "CPU%"
        );
        assert_eq!(
            GraphSlot::system(crate::model::SystemMetric::CpuAverage).metric_label(),
            "CPU Usage"
        );
        assert_eq!(
            GraphSlot::process(identity.clone(), crate::app::DetailsMetric::Private).value_format(),
            GraphValueFormat::Bytes
        );
        assert_eq!(
            GraphSlot::process(identity.clone(), crate::app::DetailsMetric::IoRead).metric_label(),
            "IO Read/s"
        );
        assert_eq!(
            GraphSlot::process(identity, crate::app::DetailsMetric::ThreadCount).value_format(),
            GraphValueFormat::Count
        );
        assert_eq!(
            GraphSlot::system(crate::model::SystemMetric::DiskRead).metric_label(),
            "Disk R"
        );
        assert_eq!(
            GraphSlot::system(crate::model::SystemMetric::DiskQueueLength).metric_label(),
            "Disk Q"
        );
    }
}
