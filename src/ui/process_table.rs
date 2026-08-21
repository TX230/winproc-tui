use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row, Table},
};

use crate::{
    App,
    app::{FocusedPanel, GraphSourceState, ProcessLifecycle, VisibleProcessRow},
    model::{MetricColumn, ProcessColumnWidths, ProcessRow, SortColumn, SortDirection},
    ui::{
        Theme,
        format::{format_compact_bytes, format_integer, format_kb_per_sec},
        graph_slot::graph_value_style,
        layout::ProcessTableLayout,
        widgets::block::panel_block_focused,
    },
};

const TRACKED_COLUMN_WIDTH: u16 = 1;
const TABLE_COLUMN_SPACING: u16 = 1;
const TABLE_BORDER_WIDTH: u16 = 2;
const FIXED_SELECTABLE_COLUMN_COUNT: usize = 2;
const PROCESS_TITLE: &str = "PROCESSES";
const TITLE_SEPARATOR: &str = " · ";
const TRUNCATION_MARKER: &str = "⋯";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessTitleSegmentKind {
    VisibleCount,
    TrackedOnly,
    Filter,
}

struct ProcessTitleSegment {
    kind: ProcessTitleSegmentKind,
    label: String,
}

pub(crate) fn draw_process_table(
    frame: &mut ratatui::Frame<'_>,
    layout: ProcessTableLayout,
    app: &App,
    theme: Theme,
) {
    let area = layout.area;
    let visible_columns = visible_metric_columns(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    let overflow_indicator =
        process_metric_overflow_indicator(&visible_columns, app.process_columns.len());
    let column_rects =
        process_table_column_rects(area, &visible_columns, &app.process_column_widths);
    let process_name_width = column_rects.get(2).map_or(0, |rect| rect.width);
    let full_path_width = full_path_column_render_width(&visible_columns, &column_rects);
    let title = process_table_title(app, theme);
    let block = process_table_block(title, overflow_indicator, app, theme);
    let table_area = block.inner(area);
    frame.render_widget(block, area);

    let total_row = layout
        .show_tracked_total
        .then(|| app.tracked_total_visible_row())
        .flatten();
    let page_size = layout.page_size;
    let visible_process_count = app.visible_process_count();
    let max_offset = visible_process_count.saturating_sub(page_size);
    let offset = app.process_table_state.offset().min(max_offset);
    let visible_processes = app.visible_process_row_window(offset, page_size);
    let selected_table_column_index = app.selected_process_column_index;
    let selected_row_index = app.process_table_state.selected();
    let mut rows = visible_processes
        .iter()
        .enumerate()
        .map(|(visible_offset, row)| {
            let row_selected = selected_row_index == Some(offset + visible_offset);
            process_table_row(
                row,
                app,
                &visible_columns,
                process_name_width,
                full_path_width,
                selected_table_column_index,
                row_selected,
                theme,
            )
        })
        .collect::<Vec<_>>();
    if let Some(total_row) = total_row {
        rows.push(process_table_row(
            &total_row,
            app,
            &visible_columns,
            process_name_width,
            full_path_width,
            selected_table_column_index,
            false,
            theme,
        ));
    }

    let mut header_cells = vec![
        header_cell(" ", Alignment::Left, false, theme),
        header_cell(
            header_label("PID", app.sort_indicator_for_column(SortColumn::Pid)),
            Alignment::Right,
            selected_table_column_index == 0,
            theme,
        ),
        header_cell(
            header_label(
                "Process",
                app.sort_indicator_for_column(SortColumn::ProcessName),
            ),
            Alignment::Left,
            selected_table_column_index == 1,
            theme,
        ),
    ];
    for (column_index, column) in &visible_columns {
        header_cells.push(header_cell(
            header_label(
                column.label(),
                app.sort_indicator_for_column(SortColumn::Metric(*column)),
            ),
            process_metric_alignment(*column),
            column_index + FIXED_SELECTABLE_COLUMN_COUNT == selected_table_column_index,
            theme,
        ));
    }
    let header = Row::new(header_cells);

    let constraints = process_table_constraints(&visible_columns, &app.process_column_widths);

    let table = Table::new(rows, constraints)
        .header(header)
        .column_spacing(TABLE_COLUMN_SPACING);

    let mut state = app.process_table_state;
    *state.offset_mut() = 0;
    let selected = app
        .process_table_state
        .selected()
        .and_then(|selected| selected.checked_sub(offset))
        .filter(|selected| *selected < visible_processes.len());
    state.select(selected);
    frame.render_stateful_widget(table, table_area, &mut state);
}

pub(crate) fn process_metric_column_index_at(
    area: Rect,
    x: u16,
    columns: &[MetricColumn],
    metric_offset: usize,
    widths: &ProcessColumnWidths,
) -> Option<usize> {
    let table_area = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if x < table_area.x || x >= table_area.right() {
        return None;
    }

    let visible_columns = visible_metric_columns(area.width, columns, metric_offset, widths);
    let column_rects = process_table_column_rects(area, &visible_columns, widths);

    if let Some(pid_rect) = column_rects.get(1)
        && x >= pid_rect.x
        && x < pid_rect.right()
    {
        return Some(0);
    }
    if let Some(process_rect) = column_rects.get(2)
        && x >= process_rect.x
        && x < process_rect.right()
    {
        return Some(1);
    }

    visible_columns
        .iter()
        .enumerate()
        .find_map(|(visible_metric_offset, (column_index, _))| {
            let rect_index = 3 + visible_metric_offset;
            let rect = column_rects.get(rect_index)?;
            (x >= rect.x && x < rect.right())
                .then_some(column_index + FIXED_SELECTABLE_COLUMN_COUNT)
        })
}

#[cfg(test)]
pub(crate) fn process_table_visible_column_count(
    area_width: u16,
    columns: &[MetricColumn],
    metric_offset: usize,
    widths: &ProcessColumnWidths,
) -> usize {
    FIXED_SELECTABLE_COLUMN_COUNT
        + visible_metric_columns(area_width, columns, metric_offset, widths).len()
}

pub(crate) fn process_table_visible_metric_range(
    area_width: u16,
    columns: &[MetricColumn],
    metric_offset: usize,
    widths: &ProcessColumnWidths,
) -> std::ops::Range<usize> {
    let visible = visible_metric_columns(area_width, columns, metric_offset, widths);
    let start = visible
        .first()
        .map(|(index, _)| *index)
        .unwrap_or(metric_offset);
    let end = visible
        .last()
        .map(|(index, _)| index.saturating_add(1))
        .unwrap_or(start);
    start..end
}

fn visible_metric_columns(
    area_width: u16,
    columns: &[MetricColumn],
    metric_offset: usize,
    widths: &ProcessColumnWidths,
) -> Vec<(usize, MetricColumn)> {
    let usable_width = area_width.saturating_sub(TABLE_BORDER_WIDTH);
    let fixed_width = TRACKED_COLUMN_WIDTH
        + widths.resolved(SortColumn::Pid)
        + widths.resolved(SortColumn::ProcessName)
        + TABLE_COLUMN_SPACING.saturating_mul(2);
    let metric_width = usable_width.saturating_sub(fixed_width);
    if columns.is_empty() || metric_width == 0 {
        return Vec::new();
    }

    let mut used_width = 0u16;
    let start = metric_offset.min(columns.len());
    columns
        .iter()
        .copied()
        .enumerate()
        .skip(start)
        .take_while(|(_, column)| {
            let candidate = *column;
            let width = metric_column_window_width(candidate, widths);
            if used_width.saturating_add(width) > metric_width {
                false
            } else {
                used_width = used_width.saturating_add(width);
                true
            }
        })
        .collect()
}

fn metric_column_window_width(column: MetricColumn, widths: &ProcessColumnWidths) -> u16 {
    TABLE_COLUMN_SPACING.saturating_add(metric_column_render_width(column, widths))
}

fn process_table_constraints(
    visible_columns: &[(usize, MetricColumn)],
    widths: &ProcessColumnWidths,
) -> Vec<Constraint> {
    let process_width = widths.resolved(SortColumn::ProcessName);
    let mut constraints = vec![
        Constraint::Length(TRACKED_COLUMN_WIDTH),
        Constraint::Length(widths.resolved(SortColumn::Pid)),
        Constraint::Length(process_width),
    ];
    for (_, column) in visible_columns {
        let width = metric_column_render_width(*column, widths);
        let constraint = if *column == MetricColumn::FullPath {
            Constraint::Min(width)
        } else {
            Constraint::Length(width)
        };
        constraints.push(constraint);
    }
    constraints
}

fn process_table_column_rects(
    area: Rect,
    visible_columns: &[(usize, MetricColumn)],
    widths: &ProcessColumnWidths,
) -> Vec<Rect> {
    let table_area = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    Layout::horizontal(process_table_constraints(visible_columns, widths))
        .spacing(TABLE_COLUMN_SPACING)
        .split(table_area)
        .to_vec()
}

fn metric_column_render_width(column: MetricColumn, widths: &ProcessColumnWidths) -> u16 {
    widths.resolved(SortColumn::Metric(column))
}

fn full_path_column_render_width(
    visible_columns: &[(usize, MetricColumn)],
    column_rects: &[Rect],
) -> Option<u16> {
    visible_columns
        .iter()
        .position(|(_, column)| *column == MetricColumn::FullPath)
        .and_then(|visible_index| column_rects.get(3 + visible_index))
        .map(|rect| rect.width)
}

fn process_table_block<'a>(
    title: Line<'a>,
    overflow_indicator: Option<String>,
    app: &App,
    theme: Theme,
) -> ratatui::widgets::Block<'a> {
    let input_active =
        (app.is_filter_editing() || app.is_process_jump_editing()) && !app.has_modal_focus();
    let mut block = panel_block_focused(
        title,
        theme,
        app.panel_has_focus(FocusedPanel::Processes) || input_active,
    );
    if let Some(indicator) = overflow_indicator {
        block = block.title_top(
            Line::from(Span::styled(indicator, Style::default().fg(theme.muted))).right_aligned(),
        );
    }
    if input_active {
        block.border_style(Style::default().fg(theme.focus_border))
    } else {
        block
    }
}

fn aligned_cell<'a>(content: impl Into<Line<'a>>, alignment: Alignment) -> Cell<'a> {
    Cell::from(content.into().alignment(alignment))
}

fn aligned_styled_cell<'a>(
    content: impl Into<Line<'a>>,
    alignment: Alignment,
    style: Style,
) -> Cell<'a> {
    aligned_cell(content, alignment).style(style)
}

fn process_fixed_cell<'a>(
    content: impl Into<Line<'a>>,
    alignment: Alignment,
    selected: bool,
    selected_cell: bool,
    content_style: Style,
    theme: Theme,
) -> Cell<'a> {
    let mut cell = aligned_styled_cell(content, alignment, content_style);
    if selected_cell {
        cell = cell.style(Style::default().bg(theme.table_intersection_surface));
    } else if selected {
        cell = cell.style(Style::default().bg(theme.table_column_surface));
    }
    cell
}

fn header_cell<'a>(
    content: impl Into<Line<'a>>,
    alignment: Alignment,
    selected: bool,
    theme: Theme,
) -> Cell<'a> {
    let style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.table_column_surface)
    } else {
        Style::default().fg(theme.text).bg(theme.panel)
    };
    Cell::from(content.into().alignment(alignment)).style(style)
}

fn header_label(label: &str, direction: Option<SortDirection>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        label.to_string(),
        Style::default().add_modifier(Modifier::UNDERLINED),
    )];
    if let Some(symbol) = match direction {
        Some(SortDirection::Asc) => Some("↑"),
        Some(SortDirection::Desc) => Some("↓"),
        None => None,
    } {
        spans.push(Span::raw(" "));
        spans.push(Span::raw(symbol));
    }
    Line::from(spans)
}

// Row rendering keeps geometry and selection inputs explicit for cell-level styling.
#[allow(clippy::too_many_arguments)]
fn process_table_row(
    row: &VisibleProcessRow<'_>,
    app: &App,
    visible_columns: &[(usize, MetricColumn)],
    process_name_width: u16,
    full_path_width: Option<u16>,
    selected_table_column_index: usize,
    row_selected: bool,
    theme: Theme,
) -> Row<'static> {
    let process = row.process;
    let text_style = process_text_style(row, theme);
    let mut cells = vec![
        tracked_cell(row, theme),
        process_fixed_cell(
            process.pid.to_string(),
            Alignment::Right,
            selected_table_column_index == 0,
            row_selected && selected_table_column_index == 0,
            text_style,
            theme,
        ),
        process_fixed_cell(
            process_name_line(row, process_name_width, app, theme),
            Alignment::Left,
            selected_table_column_index == 1,
            row_selected && selected_table_column_index == 1,
            text_style,
            theme,
        ),
    ];
    for (column_index, column) in visible_columns {
        let table_column_index = column_index + FIXED_SELECTABLE_COLUMN_COUNT;
        let selected_column = table_column_index == selected_table_column_index;
        let selected_cell = row_selected && selected_column;
        let graph_state = if row.is_tracked_total {
            None
        } else {
            graph_state_for_cell(app, process, *column)
        };
        let column_width = if *column == MetricColumn::FullPath {
            full_path_width.unwrap_or_else(|| {
                app.process_column_widths
                    .resolved(SortColumn::Metric(*column))
            })
        } else {
            app.process_column_widths
                .resolved(SortColumn::Metric(*column))
        };
        cells.push(process_metric_cell(
            process,
            *column,
            column_width,
            app,
            selected_column,
            selected_cell,
            graph_state,
            text_style,
            theme,
        ));
    }
    Row::new(cells).style(process_row_style(row_selected, row.multi_selected, theme))
}

// Metric cells combine value, graph, and two-dimensional selection state.
#[allow(clippy::too_many_arguments)]
fn process_metric_cell(
    process: &ProcessRow,
    column: MetricColumn,
    column_width: u16,
    app: &App,
    selected: bool,
    selected_cell: bool,
    graph_state: Option<GraphSourceState>,
    text_style: Style,
    theme: Theme,
) -> Cell<'static> {
    let value_style = graph_value_style(text_style, graph_state, theme);
    let mut cell = Cell::from(process_metric_line(
        process,
        column,
        column_width,
        app,
        value_style,
        theme,
    ));
    if selected_cell {
        cell = cell.style(Style::default().bg(theme.table_intersection_surface));
    } else if selected {
        cell = cell.style(Style::default().bg(theme.table_column_surface));
    }
    cell
}

fn graph_state_for_cell(
    app: &App,
    process: &ProcessRow,
    column: MetricColumn,
) -> Option<GraphSourceState> {
    let identity = crate::model::ProcessIdentity::from_row(process);
    let metric = crate::app::DetailsMetric::from_graphable_column(column)?;
    app.graph_source_state(&crate::app::GraphSlot::process(identity, metric))
}

fn process_row_style(selected: bool, multi_selected: bool, theme: Theme) -> Style {
    let fg = theme.text;
    if selected {
        Style::default().fg(fg).bg(theme.table_selection_surface)
    } else if multi_selected {
        Style::default()
            .fg(fg)
            .bg(theme.table_multi_selection_surface)
    } else {
        Style::default().fg(fg).bg(theme.panel)
    }
}

fn process_metric_line(
    process: &ProcessRow,
    column: MetricColumn,
    column_width: u16,
    app: &App,
    text_style: Style,
    theme: Theme,
) -> Line<'static> {
    let value = format_process_column(process, column, column_width);
    let line = if column == MetricColumn::FullPath {
        match active_filter_query(app) {
            Some(query) if !process_name_matches_query(process, query) => {
                highlighted_match_line(value, query, text_style, theme)
            }
            Some(_) => Line::from(Span::styled(value, text_style)),
            None => Line::from(Span::styled(value, text_style)),
        }
    } else {
        Line::from(Span::styled(value, text_style))
    };
    line.alignment(process_metric_alignment(column))
}

fn tracked_cell(row: &VisibleProcessRow<'_>, theme: Theme) -> Cell<'static> {
    if !row.tracked {
        return Cell::from(Line::from(Span::raw(tracked_symbol(false))));
    }
    Cell::from(Line::from(Span::styled(
        tracked_symbol(true),
        tracked_marker_style(&row.lifecycle, theme),
    )))
}

fn tracked_marker_style(lifecycle: &ProcessLifecycle, theme: Theme) -> Style {
    let background = match lifecycle {
        ProcessLifecycle::Live => theme.tracked,
        ProcessLifecycle::Exited { .. } => theme.exited,
    };
    Style::default()
        .fg(theme.background)
        .bg(background)
        .remove_modifier(Modifier::BOLD)
}

fn tracked_symbol(tracked: bool) -> &'static str {
    if tracked { "T" } else { " " }
}

fn process_name_line(
    row: &VisibleProcessRow<'_>,
    width: u16,
    app: &App,
    theme: Theme,
) -> Line<'static> {
    let process = row.process;
    let base_style = process_text_style(row, theme);
    let query = (if app.is_process_jump_editing() {
        Some(app.process_jump_draft().trim())
    } else {
        active_filter_query(app)
    })
    .filter(|query| !query.is_empty());
    let line = match query {
        Some(query) => highlighted_process_name_line(&process.name, query, base_style, theme),
        None => Line::from(Span::styled(process.name.clone(), base_style)),
    };
    let width = width as usize;
    if let ProcessLifecycle::Exited { exited_at } = &row.lifecycle {
        let suffix = format!("({})", exited_at.format("%H:%M:%S"));
        let suffix_width = text_width(&suffix);
        if suffix_width < width {
            let mut line = truncate_line_end(line, width - suffix_width, base_style);
            line.spans.push(Span::styled(suffix, base_style));
            return line;
        }
    }
    truncate_line_end(line, width, base_style)
}

fn active_filter_query(app: &App) -> Option<&str> {
    let query = app.active_filter_text().trim();
    (!query.is_empty()).then_some(query)
}

fn process_name_matches_query(process: &ProcessRow, query: &str) -> bool {
    process
        .name
        .to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}

fn highlighted_process_name_line(
    process_name: &str,
    query: &str,
    base_style: Style,
    theme: Theme,
) -> Line<'static> {
    let name_lower = process_name.to_ascii_lowercase();
    let query_lower = query.to_ascii_lowercase();
    let Some(start) = name_lower.find(&query_lower) else {
        return Line::from(Span::styled(process_name.to_string(), base_style));
    };
    let end = start + query_lower.len();
    if !process_name.is_char_boundary(start) || !process_name.is_char_boundary(end) {
        return Line::from(Span::styled(process_name.to_string(), base_style));
    }
    highlighted_match_line_at(process_name, start, end, base_style, theme)
}

fn highlighted_match_line(
    value: String,
    query: &str,
    base_style: Style,
    theme: Theme,
) -> Line<'static> {
    let value_lower = value.to_ascii_lowercase();
    let query_lower = query.to_ascii_lowercase();
    let Some(start) = value_lower.find(&query_lower) else {
        return Line::from(Span::styled(value, base_style));
    };
    let end = start + query_lower.len();
    if !value.is_char_boundary(start) || !value.is_char_boundary(end) {
        return Line::from(Span::styled(value, base_style));
    }
    highlighted_match_line_at(&value, start, end, base_style, theme)
}

fn highlighted_match_line_at(
    value: &str,
    start: usize,
    end: usize,
    base_style: Style,
    theme: Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(value[..start].to_string(), base_style),
        Span::styled(
            value[start..end].to_string(),
            Style::default().fg(theme.warning),
        ),
        Span::styled(value[end..].to_string(), base_style),
    ])
}

fn truncate_line_end(line: Line<'static>, max_width: usize, marker_style: Style) -> Line<'static> {
    if line.width() <= max_width {
        return line;
    }

    let marker_width = text_width(TRUNCATION_MARKER);
    if max_width < marker_width {
        return Line::default();
    }

    let content_width = max_width.saturating_sub(marker_width);
    let mut remaining = content_width;
    let mut spans = Vec::new();
    for span in line.spans {
        let content = prefix_to_width(span.content.as_ref(), remaining);
        let fully_consumed = content.len() == span.content.len();
        let width = text_width(&content);
        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }
        remaining = remaining.saturating_sub(width);
        if remaining == 0 || !fully_consumed {
            break;
        }
    }
    spans.push(Span::styled(TRUNCATION_MARKER, marker_style));
    Line::from(spans)
}

fn prefix_to_width(value: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0usize;
    for ch in value.chars() {
        let char_width = text_width(&ch.to_string());
        if width.saturating_add(char_width) > max_width {
            break;
        }
        result.push(ch);
        width = width.saturating_add(char_width);
    }
    result
}

fn suffix_to_width(value: &str, max_width: usize) -> String {
    let mut reversed = Vec::new();
    let mut width = 0usize;
    for ch in value.chars().rev() {
        let char_width = text_width(&ch.to_string());
        if width.saturating_add(char_width) > max_width {
            break;
        }
        reversed.push(ch);
        width = width.saturating_add(char_width);
    }
    reversed.into_iter().rev().collect()
}

fn text_width(value: &str) -> usize {
    Line::from(value).width()
}

fn process_metric_overflow_indicator(
    visible_columns: &[(usize, MetricColumn)],
    total_columns: usize,
) -> Option<String> {
    if total_columns == 0 {
        return None;
    }

    let start = visible_columns.first().map(|(index, _)| *index);
    let end = visible_columns
        .last()
        .map(|(index, _)| index.saturating_add(1));
    if start == Some(0) && end == Some(total_columns) {
        return None;
    }

    Some(match (start, end) {
        (Some(start), Some(end)) => format!("‹ {}–{end}/{total_columns} ›", start + 1),
        _ => format!("‹ 0/{total_columns} ›"),
    })
}

fn process_text_style(row: &VisibleProcessRow<'_>, theme: Theme) -> Style {
    if matches!(row.lifecycle, ProcessLifecycle::Exited { .. }) {
        Style::default().fg(theme.exited)
    } else {
        Style::default().fg(theme.text)
    }
}

fn process_table_title(app: &App, theme: Theme) -> Line<'static> {
    let filter = app.active_filter_text();
    let mut spans = vec![Span::styled(
        PROCESS_TITLE,
        Style::default().add_modifier(Modifier::BOLD),
    )];
    if app.is_filter_editing() {
        spans.push(title_separator(theme));
        spans.extend(filter_title_spans(filter, theme));
    } else if app.is_process_jump_editing() {
        spans.push(title_separator(theme));
        spans.extend(jump_title_spans(app.process_jump_draft(), theme));
    } else {
        for segment in process_table_state_segments(app) {
            spans.push(title_separator(theme));
            if segment.kind == ProcessTitleSegmentKind::TrackedOnly {
                spans.extend(process_tracked_only_title_spans(app, theme));
            } else {
                spans.push(Span::styled(
                    segment.label,
                    process_title_segment_style(segment.kind, app, theme),
                ));
            }
        }
    }
    Line::from(spans)
}

fn process_table_state_segments(app: &App) -> Vec<ProcessTitleSegment> {
    let mut segments = vec![
        ProcessTitleSegment {
            kind: ProcessTitleSegmentKind::VisibleCount,
            label: format!("{} visible", app.visible_process_count()),
        },
        ProcessTitleSegment {
            kind: ProcessTitleSegmentKind::TrackedOnly,
            label: process_tracked_only_label(app).to_string(),
        },
    ];
    let filter = app.active_filter_text();
    if !filter.is_empty() {
        segments.push(ProcessTitleSegment {
            kind: ProcessTitleSegmentKind::Filter,
            label: format!("Filter \"{filter}\""),
        });
    }
    segments
}

fn process_title_segment_style(kind: ProcessTitleSegmentKind, app: &App, theme: Theme) -> Style {
    match kind {
        ProcessTitleSegmentKind::VisibleCount => Style::default()
            .fg(theme.muted)
            .remove_modifier(Modifier::BOLD),
        ProcessTitleSegmentKind::TrackedOnly if app.watch_enabled => Style::default()
            .fg(theme.tracked)
            .remove_modifier(Modifier::BOLD),
        ProcessTitleSegmentKind::TrackedOnly => Style::default()
            .fg(theme.muted)
            .remove_modifier(Modifier::BOLD),
        ProcessTitleSegmentKind::Filter => Style::default()
            .fg(theme.warning)
            .remove_modifier(Modifier::BOLD),
    }
}

fn process_tracked_only_title_spans(app: &App, theme: Theme) -> Vec<Span<'static>> {
    if app.watch_enabled {
        let plain = |color| Style::default().fg(color).remove_modifier(Modifier::BOLD);
        return vec![
            Span::styled("☑", plain(theme.tracked)),
            Span::styled(" Tracked-only", plain(theme.text)),
            Span::styled("(Shift+T)", plain(theme.muted)),
        ];
    }
    let label_style = process_title_segment_style(ProcessTitleSegmentKind::TrackedOnly, app, theme);
    vec![Span::styled(process_tracked_only_label(app), label_style)]
}

fn filter_title_spans(filter: &str, theme: Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            "Filter ",
            Style::default()
                .fg(theme.background)
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ",
            Style::default()
                .fg(theme.warning)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            filter.to_string(),
            Style::default()
                .fg(theme.warning)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "_",
            Style::default()
                .fg(theme.background)
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ),
    ]
}

fn jump_title_spans(query: &str, theme: Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            "Jump ",
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            query.to_string(),
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "_",
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]
}

fn title_separator(theme: Theme) -> Span<'static> {
    Span::styled(
        TITLE_SEPARATOR,
        Style::default()
            .fg(theme.muted)
            .remove_modifier(Modifier::BOLD),
    )
}

pub(crate) fn process_tracked_only_control_area(area: Rect, app: &App) -> Option<Rect> {
    if app.is_filter_editing() || app.is_process_jump_editing() {
        return None;
    }

    let visible_columns = visible_metric_columns(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
        &app.process_column_widths,
    );
    let control_right =
        process_metric_overflow_indicator(&visible_columns, app.process_columns.len())
            .map(|indicator| {
                area.right()
                    .saturating_sub(1)
                    .saturating_sub(text_width(&indicator) as u16)
            })
            .unwrap_or_else(|| area.right());
    let mut prefix_width = text_width(PROCESS_TITLE);
    for segment in process_table_state_segments(app) {
        prefix_width = prefix_width.saturating_add(text_width(TITLE_SEPARATOR));
        if segment.kind == ProcessTitleSegmentKind::TrackedOnly {
            let title_x = area.x.saturating_add(1).saturating_add(prefix_width as u16);
            if title_x >= control_right {
                return None;
            }
            return Some(Rect::new(
                title_x,
                area.y,
                (text_width(&segment.label) as u16).min(control_right.saturating_sub(title_x)),
                1,
            ));
        }
        prefix_width = prefix_width.saturating_add(text_width(&segment.label));
    }
    None
}

fn process_tracked_only_label(app: &App) -> &'static str {
    if app.watch_enabled {
        "☑ Tracked-only(Shift+T)"
    } else {
        "☐ Tracked-only(Shift+T)"
    }
}

fn format_optional_integer(value: Option<u64>) -> String {
    value
        .map(format_integer)
        .unwrap_or_else(|| "--".to_string())
}

fn format_optional_compact_bytes(value: Option<u64>) -> String {
    value
        .map(format_compact_bytes)
        .unwrap_or_else(|| "--".to_string())
}

fn format_process_column(process: &ProcessRow, column: MetricColumn, column_width: u16) -> String {
    match column {
        MetricColumn::CpuPercent => process
            .cpu_percent
            .map(format_cpu_percent)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::PrivateBytes => format_optional_compact_bytes(process.private_bytes),
        MetricColumn::WorksetBytes => format_optional_compact_bytes(process.workset_bytes),
        MetricColumn::WorksetPrivateBytes => {
            format_optional_compact_bytes(process.workset_private_bytes)
        }
        MetricColumn::WorksetShareableBytes => {
            format_optional_compact_bytes(process.workset_shareable_bytes)
        }
        MetricColumn::ThreadCount => format_optional_integer(process.thread_count),
        MetricColumn::HandleCount => format_optional_integer(process.handle_count),
        MetricColumn::UserObjectCount => format_optional_integer(process.user_object_count),
        MetricColumn::GdiObjectCount => format_optional_integer(process.gdi_object_count),
        MetricColumn::GpuPercent => process
            .gpu_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::DotNetHeapBytes => format_optional_compact_bytes(process.dotnet_heap_bytes),
        MetricColumn::DotNetGcGen0HeapBytes => {
            format_optional_compact_bytes(process.dotnet_gc_gen0_heap_bytes)
        }
        MetricColumn::DotNetGcGen1HeapBytes => {
            format_optional_compact_bytes(process.dotnet_gc_gen1_heap_bytes)
        }
        MetricColumn::DotNetGcGen2HeapBytes => {
            format_optional_compact_bytes(process.dotnet_gc_gen2_heap_bytes)
        }
        MetricColumn::DotNetGcLohBytes => {
            format_optional_compact_bytes(process.dotnet_gc_loh_bytes)
        }
        MetricColumn::DotNetGcPohBytes => {
            format_optional_compact_bytes(process.dotnet_gc_poh_bytes)
        }
        MetricColumn::DotNetGcCommittedBytes => {
            format_optional_compact_bytes(process.dotnet_gc_committed_bytes)
        }
        MetricColumn::DotNetGcFragmentationBytes => {
            format_optional_compact_bytes(process.dotnet_gc_fragmentation_bytes)
        }
        MetricColumn::DotNetAllocationBytesPerSec => process
            .dotnet_allocation_bytes_per_sec
            .map(|value| format!("{}/s", format_compact_bytes(value)))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::GpuDedicatedBytes => {
            format_optional_compact_bytes(process.gpu_dedicated_bytes)
        }
        MetricColumn::GpuSharedBytes => format_optional_compact_bytes(process.gpu_shared_bytes),
        MetricColumn::IoReadBytesPerSec => process
            .io_read_bytes_per_sec
            .map(format_kb_per_sec)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::IoWriteBytesPerSec => process
            .io_write_bytes_per_sec
            .map(format_kb_per_sec)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::FullPath => process
            .executable_path
            .as_deref()
            .map(|path| compact_path_start(path, column_width as usize))
            .unwrap_or_else(|| "--".to_string()),
    }
}

fn process_metric_alignment(column: MetricColumn) -> Alignment {
    if matches!(column, MetricColumn::FullPath) {
        Alignment::Left
    } else {
        Alignment::Right
    }
}

fn compact_path_start(path: &str, width: usize) -> String {
    if text_width(path) <= width {
        return path.to_string();
    }
    let marker_width = text_width(TRUNCATION_MARKER);
    if width < marker_width {
        return String::new();
    }
    let tail = suffix_to_width(path, width.saturating_sub(marker_width));
    format!("{TRUNCATION_MARKER}{tail}")
}

fn format_cpu_percent(value: f64) -> String {
    format!("{value:.1}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Styled;

    #[test]
    fn tracked_cell_uses_t_for_tracked_rows_only() {
        let process = ProcessRow {
            pid: 1,
            name: "app.exe".to_string(),
            executable_path: None,
            start_time: Some(1_700_000_001),
            cpu_percent: None,
            private_bytes: Some(120),
            workset_bytes: None,
            workset_private_bytes: Some(80),
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        };
        let tracked = VisibleProcessRow {
            process: &process,
            tracked: true,
            lifecycle: ProcessLifecycle::Live,
            multi_selected: false,
            is_tracked_total: false,
        };
        let ordinary = VisibleProcessRow {
            process: &process,
            tracked: false,
            lifecycle: ProcessLifecycle::Live,
            multi_selected: false,
            is_tracked_total: false,
        };

        assert_eq!(tracked_symbol(tracked.tracked), "T");
        assert_eq!(tracked_symbol(ordinary.tracked), " ");
        for theme in crate::ui::THEMES {
            let style = tracked_marker_style(&tracked.lifecycle, theme);
            assert_eq!(style.fg, Some(theme.background));
            assert_eq!(style.bg, Some(theme.tracked));
            assert!(!style.add_modifier.contains(Modifier::BOLD));

            let exited = ProcessLifecycle::Exited {
                exited_at: chrono::Local::now(),
            };
            let exited_style = tracked_marker_style(&exited, theme);
            assert_eq!(exited_style.fg, Some(theme.background));
            assert_eq!(exited_style.bg, Some(theme.exited));
            assert!(!exited_style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn pid_column_width_matches_practical_pid_width() {
        assert_eq!(SortColumn::Pid.default_width(), 6);
        assert!(SortColumn::Pid.min_width() >= 5);
    }

    #[test]
    fn process_io_columns_use_decimal_kilobytes_per_second() {
        let process = ProcessRow {
            pid: 1,
            name: "app.exe".to_string(),
            executable_path: None,
            start_time: Some(1_700_000_001),
            cpu_percent: None,
            private_bytes: None,
            workset_bytes: None,
            workset_private_bytes: None,
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            io_read_bytes_per_sec: Some(12_345_678),
            io_write_bytes_per_sec: Some(98_765_432),
        };

        assert_eq!(
            format_process_column(&process, MetricColumn::IoReadBytesPerSec, 12),
            "12,346 KB/s"
        );
        assert_eq!(
            format_process_column(&process, MetricColumn::IoWriteBytesPerSec, 12),
            "98,765 KB/s"
        );
    }

    #[test]
    fn header_label_underlines_name_but_not_sort_direction() {
        for (direction, expected) in [
            (Some(SortDirection::Asc), "CPU% ↑"),
            (Some(SortDirection::Desc), "CPU% ↓"),
            (None, "CPU%"),
        ] {
            let line = header_label("CPU%", direction);
            let rendered = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();

            assert_eq!(rendered, expected);
            assert!(
                line.spans[0]
                    .style
                    .add_modifier
                    .contains(Modifier::UNDERLINED)
            );
            assert!(
                line.spans[1..]
                    .iter()
                    .all(|span| !span.style.add_modifier.contains(Modifier::UNDERLINED))
            );
        }
    }

    #[test]
    fn header_cells_use_the_column_selection_surface() {
        let theme = crate::ui::theme::THEMES[0];
        let ordinary = header_cell("PrivBytes", Alignment::Right, false, theme);
        let focused = header_cell("PrivBytes", Alignment::Right, true, theme);

        let ordinary_style = Styled::style(&ordinary);
        let focused_style = Styled::style(&focused);

        assert_eq!(ordinary_style.bg, Some(theme.panel));
        assert_eq!(focused_style.bg, Some(theme.table_column_surface));
        assert!(!ordinary_style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!ordinary_style.add_modifier.contains(Modifier::BOLD));
        assert!(!focused_style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!focused_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn graphed_process_values_use_green_with_bold_reserved_for_the_active_graph() {
        for theme in crate::ui::THEMES {
            let inactive = graph_value_style(
                Style::default(),
                Some(GraphSourceState {
                    ordinal: 0,
                    active: false,
                }),
                theme,
            );
            let active = graph_value_style(
                Style::default(),
                Some(GraphSourceState {
                    ordinal: 1,
                    active: true,
                }),
                theme,
            );

            assert_eq!(inactive.fg, Some(theme.active_series));
            assert!(!inactive.add_modifier.contains(Modifier::BOLD));
            assert_eq!(active.fg, Some(theme.active_series));
            assert!(active.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn full_path_column_is_left_aligned_and_keeps_path_tail() {
        assert_eq!(
            process_metric_alignment(MetricColumn::FullPath),
            Alignment::Left
        );
        assert_eq!(
            compact_path_start(r"C:\very\long\workspace\target\debug\app.exe", 18),
            r"⋯get\debug\app.exe"
        );
    }

    #[test]
    fn truncation_marker_uses_one_terminal_cell() {
        assert_eq!(text_width(TRUNCATION_MARKER), 1);
        assert_eq!(Line::from(TRUNCATION_MARKER).width(), 1);
    }

    #[test]
    fn process_name_marker_is_added_only_when_the_line_is_truncated() {
        let style = Style::default();
        let exact = truncate_line_end(Line::from(Span::styled("app.exe", style)), 7, style);
        let truncated = truncate_line_end(Line::from(Span::styled("app.exe", style)), 6, style);

        assert_eq!(line_text(&exact), "app.exe");
        assert_eq!(line_text(&truncated), "app.e⋯");
        assert_eq!(truncated.width(), 6);
    }

    #[test]
    fn process_name_truncation_preserves_visible_filter_highlighting() {
        let theme = crate::ui::theme::THEMES[0];
        let base_style = Style::default().fg(theme.text);
        let line = highlighted_match_line_at("winproc-tui.exe", 0, 3, base_style, theme);

        let truncated = truncate_line_end(line, 8, base_style);

        assert_eq!(line_text(&truncated), "winproc⋯");
        assert_eq!(truncated.width(), 8);
        assert_eq!(truncated.spans[0].style.fg, Some(theme.warning));
        assert_eq!(truncated.spans.last().unwrap().content, TRUNCATION_MARKER);
        assert_eq!(truncated.spans.last().unwrap().style, base_style);
    }

    #[test]
    fn metric_overflow_indicator_reports_selected_column_window() {
        let columns = MetricColumn::ALL;
        let all = columns.iter().copied().enumerate().collect::<Vec<_>>();
        let leading = all[..3].to_vec();
        let offset = all[9..12].to_vec();

        assert_eq!(process_metric_overflow_indicator(&all, columns.len()), None);
        assert_eq!(
            process_metric_overflow_indicator(&leading, columns.len()).as_deref(),
            Some("‹ 1–3/24 ›")
        );
        assert_eq!(
            process_metric_overflow_indicator(&offset, columns.len()).as_deref(),
            Some("‹ 10–12/24 ›")
        );
        assert_eq!(
            process_metric_overflow_indicator(&[], columns.len()).as_deref(),
            Some("‹ 0/24 ›")
        );
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn tracked_total_text_style_uses_neutral_text_color() {
        let theme = crate::ui::theme::THEMES[0];
        let process = ProcessRow {
            pid: 0,
            name: "Tracked Total".to_string(),
            executable_path: None,
            start_time: None,
            cpu_percent: Some(1.0),
            private_bytes: Some(120),
            workset_bytes: None,
            workset_private_bytes: Some(80),
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        };
        let row = VisibleProcessRow {
            process: &process,
            tracked: false,
            lifecycle: ProcessLifecycle::Live,
            multi_selected: false,
            is_tracked_total: true,
        };

        assert_eq!(process_text_style(&row, theme).fg, Some(theme.text));
        assert_eq!(process_row_style(false, false, theme).fg, Some(theme.text));
        assert_eq!(process_row_style(true, false, theme).fg, Some(theme.text));
    }

    #[test]
    fn multi_selected_rows_use_the_process_table_selection_surface() {
        let theme = crate::ui::theme::THEMES[0];

        assert_eq!(
            process_row_style(false, true, theme).bg,
            Some(theme.table_multi_selection_surface)
        );
        assert!(
            !process_row_style(false, true, theme)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn current_row_uses_table_selection_surface_without_bold() {
        let theme = crate::ui::theme::THEMES[0];
        let style = process_row_style(true, false, theme);

        assert_eq!(style.bg, Some(theme.table_selection_surface));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn visible_metric_columns_keep_pid_and_process_width_reserved() {
        let columns = MetricColumn::ALL.to_vec();
        let widths = ProcessColumnWidths::default();

        let visible = visible_metric_columns(100, &columns, 0, &widths);

        assert!(!visible.is_empty());
        let metric_width = visible
            .iter()
            .map(|(_, column)| metric_column_render_width(*column, &widths))
            .sum::<u16>();
        let total_columns = 3 + visible.len() as u16;
        let total_width = TRACKED_COLUMN_WIDTH
            + widths.resolved(SortColumn::Pid)
            + widths.resolved(SortColumn::ProcessName)
            + metric_width
            + TABLE_COLUMN_SPACING.saturating_mul(total_columns.saturating_sub(1));
        assert!(total_width <= 100 - TABLE_BORDER_WIDTH);
    }

    #[test]
    fn full_path_column_takes_extra_width_when_visible() {
        let visible = vec![(0, MetricColumn::PrivateBytes), (1, MetricColumn::FullPath)];
        let widths = ProcessColumnWidths::default();
        let rects = process_table_column_rects(Rect::new(0, 0, 140, 3), &visible, &widths);

        assert_eq!(
            process_table_constraints(&visible, &widths),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(SortColumn::Pid.default_width()),
                Constraint::Length(SortColumn::ProcessName.default_width()),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
                Constraint::Min(MetricColumn::FullPath.width()),
            ]
        );
        assert_eq!(
            full_path_column_render_width(&visible, &rects),
            Some(MetricColumn::FullPath.width() + 62)
        );
    }

    #[test]
    fn process_column_uses_resolved_width_when_full_path_is_hidden() {
        let visible = vec![(0, MetricColumn::PrivateBytes)];
        let widths = ProcessColumnWidths::default();
        let rects = process_table_column_rects(Rect::new(0, 0, 140, 3), &visible, &widths);

        assert_eq!(
            process_table_constraints(&visible, &widths),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(SortColumn::Pid.default_width()),
                Constraint::Length(SortColumn::ProcessName.default_width()),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
            ]
        );
        assert_eq!(full_path_column_render_width(&visible, &rects), None);
    }

    #[test]
    fn visible_metric_columns_drop_metrics_when_fixed_columns_need_space() {
        let columns = MetricColumn::ALL.to_vec();
        let widths = ProcessColumnWidths::default();

        let visible = visible_metric_columns(35, &columns, 0, &widths);

        assert!(visible.is_empty());
        assert!(SortColumn::Pid.min_width() >= 5);
    }

    #[test]
    fn visible_metric_columns_start_at_requested_offset() {
        let columns = MetricColumn::ALL.to_vec();
        let offset = columns.len() - 2;
        let widths = ProcessColumnWidths::default();

        let visible = visible_metric_columns(72, &columns, offset, &widths);

        assert!(!visible.is_empty());
        assert_eq!(visible.first().map(|(index, _)| *index), Some(offset));
    }

    #[test]
    fn custom_widths_change_the_visible_metric_range() {
        let columns = MetricColumn::ALL.to_vec();
        let default_widths = ProcessColumnWidths::default();
        let default_visible = visible_metric_columns(72, &columns, 0, &default_widths);
        let mut custom_widths = ProcessColumnWidths::default();
        custom_widths.set(
            SortColumn::Metric(MetricColumn::PrivateBytes),
            MetricColumn::PrivateBytes.width() + 20,
        );

        let custom_visible = visible_metric_columns(72, &columns, 0, &custom_widths);

        assert!(custom_visible.len() < default_visible.len());
        assert_eq!(
            custom_visible.first().map(|(_, column)| *column),
            default_visible.first().map(|(_, column)| *column)
        );
    }

    #[test]
    fn custom_process_width_is_fixed_while_full_path_keeps_flexible_extra_space() {
        let visible = vec![(0, MetricColumn::PrivateBytes), (1, MetricColumn::FullPath)];
        let mut widths = ProcessColumnWidths::default();
        widths.set(SortColumn::ProcessName, 28);
        widths.set(SortColumn::Metric(MetricColumn::FullPath), 60);
        let rects = process_table_column_rects(Rect::new(0, 0, 180, 3), &visible, &widths);

        assert_eq!(
            process_table_constraints(&visible, &widths),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(SortColumn::Pid.default_width()),
                Constraint::Length(28),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
                Constraint::Min(60),
            ]
        );
        assert_eq!(full_path_column_render_width(&visible, &rects), Some(128));

        let without_full_path = vec![(0, MetricColumn::PrivateBytes)];
        assert_eq!(
            process_table_constraints(&without_full_path, &widths),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(SortColumn::Pid.default_width()),
                Constraint::Length(28),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
            ]
        );
    }

    #[test]
    fn rendering_at_different_widths_does_not_mutate_saved_widths() {
        let columns = MetricColumn::ALL.to_vec();
        let mut widths = ProcessColumnWidths::default();
        widths.set(
            SortColumn::Metric(MetricColumn::PrivateBytes),
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX,
        );

        let _ = visible_metric_columns(40, &columns, 0, &widths);
        let _ = visible_metric_columns(160, &columns, 0, &widths);

        assert_eq!(
            widths.resolved(SortColumn::Metric(MetricColumn::PrivateBytes)),
            crate::model::columns::PROCESS_COLUMN_WIDTH_MAX
        );
    }
}
