use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
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
        AbComparison, AbComparisonPoint, FocusedPanel, GraphSample, GraphSlot, GraphValueFormat,
    },
    ui::{
        GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, Theme,
        format::{format_integer, format_mb_per_sec, format_signed_integer},
        layout::{
            DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT, details_graph_area, details_samples_area,
            details_samples_row_capacity, details_samples_summary_height, details_slot_areas,
        },
        widgets::block::{panel_block, panel_block_focused},
    },
};

const SAMPLE_METRIC_VALUE_WIDTH: usize = 15;
const SAMPLE_DELTA_WIDTH: usize = 15;

pub(crate) fn draw_details_panel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let slots = app
        .graph_slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.as_ref().map(|slot| (index, slot)))
        .collect::<Vec<_>>();
    if slots.is_empty() {
        let lines = vec![Line::from(Span::styled(
            "No graph metrics selected",
            Style::default().fg(theme.muted),
        ))];
        frame.render_widget(details_paragraph(lines, theme), area);
        return;
    }

    let bounds = graph_bounds(
        app.effective_graph_time_span_seconds(),
        app.effective_graph_time_offset_seconds(),
    );
    let common_y_label_width = slots
        .iter()
        .map(|(_, slot)| {
            let samples = app.graph_slot_samples(slot);
            let metric = slot.value_format();
            let data = chart_points(samples.as_slice(), bounds);
            let stats = graph_stats(samples.as_slice(), app.graph_slot_peak(slot), &data);
            let (y_min, y_max) = graph_y_bounds(&stats, app.graph_y_axis_zero_min);
            y_axis_label_width(&y_axis_labels(y_min, y_max, metric))
        })
        .max()
        .unwrap_or(1);
    let selected_sample_time = app.selected_details_sample_time();
    let slot_areas = details_slot_areas(area, slots.len());
    for ((slot_index, slot), slot_area) in slots.into_iter().zip(slot_areas) {
        let samples = app.graph_slot_samples(slot);
        let peak = app.graph_slot_peak(slot);
        let item_line = graph_slot_item_line(slot, theme);
        let metric = slot.value_format();
        render_details_content(
            frame,
            slot_area,
            slot_index,
            item_line,
            samples.as_slice(),
            peak,
            metric,
            slot.metric_label(),
            app,
            theme,
            app.active_graph_slot_count() == 1,
            common_y_label_width,
            selected_sample_time,
        );
    }
}

fn render_details_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    slot_index: usize,
    item_line: Line<'static>,
    samples: &[GraphSample],
    peak: Option<f64>,
    metric: GraphValueFormat,
    metric_label: &'static str,
    app: &App,
    theme: Theme,
    show_base_summary: bool,
    y_label_width: usize,
    selected_sample_time: Option<DateTime<Local>>,
) {
    if samples.is_empty() {
        let lines = vec![Line::from(Span::styled(
            "No samples available",
            Style::default().fg(theme.muted),
        ))];
        frame.render_widget(details_paragraph(lines, theme), area);
        return;
    }

    let graph_area = details_graph_area(area, app.show_samples_panel, app.show_sample_delta);
    let samples_area = app
        .show_samples_panel
        .then(|| details_samples_area(area, app.show_sample_delta))
        .unwrap_or_default();
    draw_graph_panel(
        frame,
        graph_area,
        item_line,
        samples,
        peak,
        metric,
        selected_sample_time,
        app.effective_graph_time_span_seconds(),
        app.effective_graph_time_offset_seconds(),
        app.graph_show_all_samples,
        app.graph_y_axis_zero_min,
        app.active_ab_comparison(),
        theme,
        app.panel_has_focus(FocusedPanel::DetailsGraph)
            && app.active_graph_slot_index == slot_index,
        slot_index,
        y_label_width,
    );
    if !app.show_samples_panel {
        return;
    }
    let sample_viewport = draw_samples_subpanel(
        frame,
        samples_area,
        app,
        samples,
        metric,
        metric_label,
        app.details_sample_selected,
        app.details_sample_offset,
        app.active_graph_slot_index == slot_index,
        app.active_ab_comparison(),
        theme,
        app.panel_has_focus(FocusedPanel::DetailsSamples)
            && app.active_graph_slot_index == slot_index,
        slot_index,
        show_base_summary,
        app.show_sample_delta,
    );
    render_samples_scrollbar(frame, samples_area, sample_viewport, theme);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SampleViewport {
    start: usize,
    rows: usize,
    total: usize,
}

fn draw_samples_subpanel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    samples: &[GraphSample],
    metric: GraphValueFormat,
    metric_label: &str,
    selected: usize,
    offset: usize,
    is_active_slot: bool,
    comparison: Option<&AbComparison>,
    theme: Theme,
    focused: bool,
    slot_index: usize,
    show_base_summary: bool,
    show_delta: bool,
) -> SampleViewport {
    let block = panel_block_focused(format!("Samples#{}", slot_index + 1), theme, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let metric_header_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::from(vec![
        Span::styled("M  ", Style::default().fg(theme.muted)),
        Span::styled("Time      ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{metric_label:<SAMPLE_METRIC_VALUE_WIDTH$}"),
            metric_header_style,
        ),
        if show_delta {
            Span::styled(
                format!("{:>SAMPLE_DELTA_WIDTH$}", "Delta"),
                Style::default().fg(theme.muted),
            )
        } else {
            Span::raw("")
        },
    ])];

    let content_height = inner.height as usize;
    let row_capacity =
        details_samples_row_capacity(inner.height, comparison.is_some(), show_base_summary);
    let view_state = app
        .details_sample_view_state_for_slot(slot_index, row_capacity)
        .unwrap_or(crate::app::DetailsSampleViewState {
            selected_index: selected.min(samples.len().saturating_sub(1)),
            selected_exact: is_active_slot,
            offset,
        });
    let (start, end) = sample_viewport_bounds(samples.len(), view_state.offset, row_capacity);
    for (index, sample) in samples[start..end].iter().enumerate() {
        let sample_index = start + index;
        let sample_selected =
            view_state.selected_exact && sample_index == view_state.selected_index;
        let style = if sample_selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_alt)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let row_bg = if sample_selected {
            theme.accent_alt
        } else {
            theme.panel
        };
        let delta_value = metric_value(sample, metric);
        let previous_value = sample_index
            .checked_sub(1)
            .and_then(|previous_index| samples.get(previous_index))
            .and_then(|previous| metric_value(previous, metric));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<3}", sample_ab_marker(comparison, sample.captured_at)),
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
                style,
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
                    delta_style(delta_value, previous_value, theme).bg(row_bg),
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
        show_base_summary,
    );
    let spacer_lines = DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT as usize;
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
    samples: &[GraphSample],
    captured_at: DateTime<Local>,
) -> Option<i64> {
    let latest = samples.last()?.captured_at;
    Some(
        latest
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
        .thumb_style(Style::default().fg(theme.accent_alt).bg(theme.panel));
    let inner = panel_block("", theme).inner(area);
    frame.render_stateful_widget(scrollbar, inner, &mut state);
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

fn draw_graph_panel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    item_line: Line<'static>,
    samples: &[GraphSample],
    peak: Option<f64>,
    metric: GraphValueFormat,
    selected_sample_time: Option<DateTime<Local>>,
    span_seconds: u32,
    offset_seconds: u32,
    show_all_samples: bool,
    y_axis_zero_min: bool,
    comparison: Option<&AbComparison>,
    theme: Theme,
    focused: bool,
    slot_index: usize,
    y_label_width: usize,
) {
    let block = panel_block_focused(format!("Graph#{}", slot_index + 1), theme, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(inner);

    let bounds = graph_bounds(span_seconds, offset_seconds);
    let data = chart_points(samples, bounds);
    let stats = graph_stats(samples, peak, &data);
    let (y_min, y_max) = graph_y_bounds(&stats, y_axis_zero_min);
    let plot_data = lift_floor_points_for_plot(&data, y_min, y_max);
    let header = Paragraph::new(item_line).style(Style::default().fg(theme.text).bg(theme.panel));
    frame.render_widget(header, layout[0]);
    render_graph_all_samples_toggle(frame, layout[0], show_all_samples, theme);
    render_graph_y_axis_toggle(frame, layout[0], y_axis_zero_min, theme);

    let selected_age_seconds =
        selected_sample_time.and_then(|time| sample_age_seconds_at_time(samples, time));
    let selected_line = selected_age_seconds
        .map(|age| selected_age_line_points(age, y_min, y_max, bounds))
        .unwrap_or_default();
    let a_line = comparison
        .and_then(|comparison| comparison.a)
        .map(|point| ab_line_points(samples, point, y_min, y_max, bounds))
        .unwrap_or_default();
    let b_line = comparison
        .and_then(|comparison| comparison.b)
        .map(|point| ab_line_points(samples, point, y_min, y_max, bounds))
        .unwrap_or_default();
    let mut datasets = Vec::new();
    if !selected_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.accent_alt))
                .data(&selected_line),
        );
    }
    if !a_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.warning))
                .data(&a_line),
        );
    }
    if !b_line.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.warning))
                .data(&b_line),
        );
    }
    datasets.push(
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme.graph_line))
            .data(&plot_data),
    );
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
        .and_then(|_| selected_sample_time)
        .and_then(|time| sample_index_at_time(samples, time))
        .and_then(|index| samples.get(index))
        .map(|sample| format_metric_sample_value(sample, metric));
    let top_labels = Paragraph::new(graph_top_label_line(
        layout[1].width as usize,
        y_label_width,
        bounds,
        selected_age_seconds,
        selected_value_label.as_deref(),
        theme,
    ))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(top_labels, layout[1]);
    frame.render_widget(chart, layout[2]);
    frame.render_widget(
        ChartAxisOverlay {
            y_label_width,
            theme,
        },
        layout[2],
    );
    frame.render_widget(
        GraphAbAxisLabels {
            y_label_width,
            bounds,
            latest_sample_at: samples.last().map(|sample| sample.captured_at),
            comparison,
            theme,
        },
        layout[2],
    );

    let x_axis = Paragraph::new(axis_tick_label_line(
        layout[3].width as usize,
        y_label_width,
        bounds,
        samples.last().map(|sample| sample.captured_at),
        theme,
    ))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(x_axis, layout[3]);
}

fn render_graph_y_axis_toggle(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    y_axis_zero_min: bool,
    theme: Theme,
) {
    if area.width < GRAPH_Y_AXIS_TOGGLE_WIDTH {
        return;
    }
    let toggle_area = Rect::new(
        area.right().saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH),
        area.y,
        GRAPH_Y_AXIS_TOGGLE_WIDTH,
        1,
    );
    let mark = if y_axis_zero_min { "☑" } else { "☐" };
    let toggle = Paragraph::new(Line::from(vec![
        Span::styled(mark, Style::default().fg(theme.accent).bg(theme.panel)),
        Span::styled(
            "  z: Min 0",
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
    ]))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(toggle, toggle_area);
}

fn render_graph_all_samples_toggle(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    show_all_samples: bool,
    theme: Theme,
) {
    let required = GRAPH_ALL_SAMPLES_TOGGLE_WIDTH.saturating_add(GRAPH_Y_AXIS_TOGGLE_WIDTH);
    if area.width < required {
        return;
    }
    let toggle_area = Rect::new(
        area.right().saturating_sub(required),
        area.y,
        GRAPH_ALL_SAMPLES_TOGGLE_WIDTH,
        1,
    );
    let mark = if show_all_samples { "☑" } else { "☐" };
    let toggle = Paragraph::new(Line::from(vec![
        Span::styled(mark, Style::default().fg(theme.accent).bg(theme.panel)),
        Span::styled(
            "  f: Fit all",
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
    ]))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(toggle, toggle_area);
}

fn details_paragraph<'a>(lines: Vec<Line<'a>>, theme: Theme) -> Paragraph<'a> {
    Paragraph::new(lines).style(Style::default().fg(theme.text).bg(theme.background))
}

fn format_metric_sample_value(sample: &GraphSample, metric: GraphValueFormat) -> String {
    metric_value(sample, metric)
        .map(|value| format_metric_value(value, metric))
        .unwrap_or_else(|| "--".to_string())
}

fn sample_max_line(
    samples: &[GraphSample],
    metric: GraphValueFormat,
    theme: Theme,
) -> Line<'static> {
    let Some((sample, value)) = sample_max(samples, metric) else {
        return Line::from(Span::styled("Max: --", Style::default().fg(theme.muted)));
    };
    Line::from(Span::styled(
        format!(
            "Max: {} @ {}",
            format_metric_value(value, metric),
            sample.captured_at.format("%H:%M:%S")
        ),
        Style::default().fg(theme.muted),
    ))
}

fn sample_summary_lines(
    samples: &[GraphSample],
    display_selected: usize,
    metric: GraphValueFormat,
    comparison: Option<&AbComparison>,
    theme: Theme,
    show_base_summary: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if show_base_summary {
        lines.push(sample_max_line(samples, metric, theme));
        lines.push(sample_moving_average_line(
            samples,
            display_selected,
            metric,
            theme,
        ));
    }
    lines.extend(sample_ab_summary_lines(comparison, samples, metric, theme));
    lines
        .truncate(details_samples_summary_height(comparison.is_some(), show_base_summary) as usize);
    lines
}

fn sample_moving_average_line(
    samples: &[GraphSample],
    selected: usize,
    metric: GraphValueFormat,
    theme: Theme,
) -> Line<'static> {
    let Some((captured_at, value)) = sample_moving_average(samples, selected, metric) else {
        return Line::from(Span::styled("MA5: --", Style::default().fg(theme.muted)));
    };
    Line::from(Span::styled(
        format!(
            "MA5: {} @ {}",
            format_metric_value(value, metric),
            captured_at.format("%H:%M:%S")
        ),
        Style::default().fg(theme.muted),
    ))
}

fn sample_moving_average(
    samples: &[GraphSample],
    selected: usize,
    metric: GraphValueFormat,
) -> Option<(DateTime<Local>, f64)> {
    let selected_sample = samples.get(selected)?;
    let start = selected.saturating_sub(4);
    let mut total = 0.0;
    let mut count = 0;
    for sample in &samples[start..=selected] {
        if let Some(value) = metric_value(sample, metric) {
            total += value;
            count += 1;
        }
    }
    (count > 0).then_some((selected_sample.captured_at, total / f64::from(count)))
}

fn sample_max<'a>(
    samples: &'a [GraphSample],
    metric: GraphValueFormat,
) -> Option<(&'a GraphSample, f64)> {
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
    format_metric_value(value, metric)
}

fn y_axis_labels(y_min: f64, y_max: f64, metric: GraphValueFormat) -> Vec<String> {
    let y_mid = y_min + (y_max - y_min) / 2.0;
    let lower_label = if y_min == 0.0 {
        "0".to_string()
    } else {
        format_metric_axis_value(y_min, metric)
    };
    let middle_label = format_metric_axis_value(y_mid, metric);
    let upper_label = format_metric_axis_value(y_max, metric);
    let visible_middle_label = if middle_label == lower_label || middle_label == upper_label {
        String::new()
    } else {
        middle_label
    };

    vec![lower_label, visible_middle_label, upper_label]
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

fn format_metric_value(value: f64, metric: GraphValueFormat) -> String {
    match metric {
        GraphValueFormat::Percent => format!("{value:.1}%"),
        GraphValueFormat::MegabitsPerSec => {
            format!("{} Mbps", ((value * 8.0) / 1_000_000.0).round() as u64)
        }
        GraphValueFormat::MegabytesPerSec => format_mb_per_sec(value.round().max(0.0) as u64),
        GraphValueFormat::QueueLength => format!("{value:.1}"),
        GraphValueFormat::Integer => format_integer(value.round().max(0.0) as u64),
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

fn delta_style(value: Option<f64>, previous: Option<f64>, theme: Theme) -> Style {
    let Some(value) = value else {
        return Style::default().fg(theme.muted);
    };
    let Some(previous) = previous else {
        return Style::default().fg(theme.muted);
    };
    let delta = value - previous;
    if delta > 0.0 {
        Style::default().fg(theme.warning)
    } else if delta < 0.0 {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.muted)
    }
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
                Span::styled("A: ", Style::default().fg(theme.warning)),
                Span::styled(a_value, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("B: ", Style::default().fg(theme.warning)),
                Span::styled(b_value, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("B-A: ", Style::default().fg(theme.warning)),
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
        .map(|value| format_metric_value(value, metric))
        .unwrap_or_else(|| "--".to_string());
    format!("{} {}", point.captured_at.format("%H:%M:%S"), value)
}

fn format_ab_delta(delta: f64, metric: GraphValueFormat) -> String {
    match metric {
        GraphValueFormat::Percent => format!("{delta:+.1}%"),
        GraphValueFormat::MegabitsPerSec => {
            let mbps = ((delta * 8.0) / 1_000_000.0).round() as i128;
            format_signed_integer(mbps) + " Mbps"
        }
        GraphValueFormat::MegabytesPerSec => {
            let mb_per_sec = (delta / 1_000_000.0).round() as i128;
            format_signed_integer(mb_per_sec) + " MB/s"
        }
        GraphValueFormat::QueueLength => format!("{delta:+.1}"),
        GraphValueFormat::Integer => format_signed_integer(delta.round() as i128),
    }
}

fn format_ab_delta_with_elapsed(
    a: AbComparisonPoint,
    b: AbComparisonPoint,
    samples: &[GraphSample],
    metric: GraphValueFormat,
) -> String {
    let delta = samples
        .iter()
        .find(|sample| sample.captured_at == a.captured_at)
        .and_then(|sample| metric_value(sample, metric))
        .zip(
            samples
                .iter()
                .find(|sample| sample.captured_at == b.captured_at)
                .and_then(|sample| metric_value(sample, metric)),
        )
        .map(|(a_value, b_value)| format_ab_delta(b_value - a_value, metric))
        .unwrap_or_else(|| "--".to_string());
    format!(
        "{} ({})",
        delta,
        format_elapsed_delta(b.captured_at.signed_duration_since(a.captured_at))
    )
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

fn graph_slot_item_line(slot: &GraphSlot, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("Item: ", Style::default().fg(theme.text)),
        Span::styled(
            slot.item_label(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
    ])
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

fn graph_stats(samples: &[GraphSample], peak: Option<f64>, points: &[(f64, f64)]) -> GraphStats {
    let current = samples.last().and_then(|sample| sample.value);
    let window_min = points.iter().map(|(_, value)| *value).reduce(f64::min);
    let window_max = points.iter().map(|(_, value)| *value).reduce(f64::max);
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
    latest_sample_at: Option<DateTime<Local>>,
    comparison: Option<&'a AbComparison>,
    theme: Theme,
}

impl Widget for GraphAbAxisLabels<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height < 2 {
            return;
        }
        let (Some(latest_sample_at), Some(comparison)) = (self.latest_sample_at, self.comparison)
        else {
            return;
        };

        let label_y = area.bottom().saturating_sub(1);
        let style = Style::default().fg(self.theme.warning).bg(self.theme.panel);
        for (label, point) in [("A", comparison.a), ("B", comparison.b)] {
            let Some(point) = point else {
                continue;
            };
            let Some(x) = ab_point_x(
                area,
                self.y_label_width,
                self.bounds,
                latest_sample_at,
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
    for (label, position) in labels
        .into_iter()
        .zip(axis_tick_positions(width, y_label_width))
    {
        write_axis_label(&mut chars, &label, position);
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

fn write_axis_label(chars: &mut [char], label: &str, tick_position: usize) {
    if chars.is_empty() {
        return;
    }

    let label_width = label.chars().count();
    let start = if tick_position + label_width >= chars.len() {
        chars.len().saturating_sub(label_width)
    } else {
        tick_position.saturating_sub(label_width / 2)
    };

    for (offset, ch) in label.chars().enumerate() {
        if let Some(cell) = chars.get_mut(start + offset) {
            *cell = ch;
        }
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

fn chart_points(samples: &[GraphSample], bounds: (i64, i64)) -> Vec<(f64, f64)> {
    let Some(latest) = samples.last().map(|sample| sample.captured_at) else {
        return Vec::new();
    };

    let mut points = Vec::new();
    for sample in samples.iter().rev() {
        let age = latest
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
) -> Vec<(f64, f64)> {
    let x = -(age_seconds.max(0) as f64);
    if x < bounds.0 as f64 || x > bounds.1 as f64 {
        return Vec::new();
    }
    vec![(x, y_min), (x, y_max)]
}

fn ab_line_points(
    samples: &[GraphSample],
    point: AbComparisonPoint,
    y_min: f64,
    y_max: f64,
    bounds: (i64, i64),
) -> Vec<(f64, f64)> {
    let Some(latest) = samples.last().map(|sample| sample.captured_at) else {
        return Vec::new();
    };
    let age = latest
        .signed_duration_since(point.captured_at)
        .num_seconds()
        .max(0);
    let x = -(age as f64);
    if x < bounds.0 as f64 || x > bounds.1 as f64 {
        return Vec::new();
    }
    vec![(x, y_min), (x, y_max)]
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
        let points = chart_points(&samples, (-60, 0));

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
            y_axis_labels(2_860_000_000.0, 2_900_000_000.0, GraphValueFormat::Integer),
            vec![
                "2,860,000,000".to_string(),
                "2,880,000,000".to_string(),
                "2,900,000,000".to_string()
            ]
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
        let rendered = sample_max_line(&refs, GraphValueFormat::Integer, THEMES[0])
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(rendered, "Max: 3,000 @ 10:00:05");
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
            sample_moving_average(&refs, 5, GraphValueFormat::Integer),
            Some((base + chrono::Duration::seconds(5), 50.0))
        );
    }

    #[test]
    fn sample_moving_average_uses_partial_window_and_skips_missing_values() {
        let base = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 10, 0, 0)
            .unwrap();
        let samples = [
            sample(base, Some(10), None),
            sample(base + chrono::Duration::seconds(1), None, None),
            sample(base + chrono::Duration::seconds(2), Some(30), None),
        ];
        let refs = samples.to_vec();

        assert_eq!(
            sample_moving_average(&refs, 2, GraphValueFormat::Integer),
            Some((base + chrono::Duration::seconds(2), 20.0))
        );
        assert_eq!(
            sample_moving_average_line(&refs, 1, GraphValueFormat::Integer, THEMES[0])
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join(""),
            "MA5: 10 @ 10:00:01"
        );
    }

    #[test]
    fn sample_moving_average_reports_missing_when_window_has_no_values() {
        let now = chrono::Local::now();
        let samples = [sample(now, None, None)];
        let refs = samples.to_vec();

        assert_eq!(
            sample_moving_average(&refs, 0, GraphValueFormat::Integer),
            None
        );
        assert_eq!(
            sample_moving_average_line(&refs, 0, GraphValueFormat::Integer, THEMES[0])
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join(""),
            "MA5: --"
        );
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
            latest_sample_at: Some(latest),
            comparison: Some(&comparison),
            theme,
        }
        .render(Rect::new(0, 0, 20, 5), &mut buffer);

        assert_eq!(buffer[(11, 4)].symbol(), "A");
        assert_eq!(buffer[(11, 4)].fg, theme.warning);
        assert_eq!(buffer[(19, 4)].symbol(), "B");
        assert_eq!(buffer[(19, 4)].fg, theme.warning);
        assert_eq!(buffer[(4, 3)].symbol(), "x");
    }

    #[test]
    fn sample_delta_uses_previous_sample_value() {
        assert_eq!(
            format_sample_delta(Some(130.0), Some(100.0), GraphValueFormat::Integer),
            "+30"
        );
        assert_eq!(
            format_sample_delta(Some(70.0), Some(100.0), GraphValueFormat::Integer),
            "-30"
        );
        assert_eq!(
            format_sample_delta(Some(6.5), Some(5.0), GraphValueFormat::Percent),
            "+1.5%"
        );
        assert_eq!(
            format_sample_delta(Some(70.0), None, GraphValueFormat::Integer),
            "--"
        );
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
        let rendered = sample_ab_summary_lines(
            Some(&comparison),
            &refs,
            GraphValueFormat::Integer,
            THEMES[0],
        )
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join("")
        })
        .collect::<Vec<_>>();

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
        let rendered = sample_ab_summary_lines(
            Some(&comparison),
            &refs,
            GraphValueFormat::Integer,
            THEMES[0],
        )
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
            chart_points(&refs, (-60, 0)),
            vec![(-15.0, 10.0), (0.0, 20.0)]
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
            chart_points(&refs, (-60, -10)),
            vec![(-45.0, 10.0), (-15.0, 15.0)]
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
    fn process_summary_uses_single_item_line_without_old_labels() {
        let identity = crate::model::ProcessIdentity {
            pid: 42,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_000),
        };
        let slot = GraphSlot::process(identity, crate::app::DetailsMetric::Private);
        let line = graph_slot_item_line(&slot, crate::ui::THEMES[0]);
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert!(rendered.contains("Item: app.exe - Private"));
        assert!(
            line.spans[1].style.fg == Some(crate::ui::THEMES[0].accent)
                && line.spans[1]
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
}
