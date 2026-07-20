use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row, Table},
};

use crate::{
    App,
    app::{FocusedPanel, ProcessLifecycle, VisibleProcessRow},
    model::{MetricColumn, ProcessRow, SortColumn, SortDirection},
    ui::{
        Theme,
        format::{format_integer, format_mbps},
        graph_slot::graph_slot_marker_span,
        widgets::block::panel_block_focused,
    },
};

const TRACKED_COLUMN_WIDTH: u16 = 1;
const PID_COLUMN_WIDTH: u16 = 6;
const PROCESS_COLUMN_MIN_WIDTH: u16 = 18;
const TABLE_COLUMN_SPACING: u16 = 1;
const TABLE_BORDER_WIDTH: u16 = 2;
const HIGHLIGHT_SYMBOL_WIDTH: u16 = 3;
const FIXED_SELECTABLE_COLUMN_COUNT: usize = 2;
const PROCESS_TITLE: &str = "Processes";
const TITLE_SEPARATOR: &str = " · ";

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
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let title = process_table_title(app, theme);
    let block = process_table_block(title, app, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let table_area = inner;
    let total_row = app.tracked_total_visible_row();
    let row_capacity = process_table_row_capacity(table_area);
    let reserve_total_row = total_row.is_some() && row_capacity > 1;
    let page_size = if reserve_total_row {
        row_capacity.saturating_sub(1)
    } else {
        row_capacity
    };
    let visible_process_count = app.visible_process_count();
    let max_offset = visible_process_count.saturating_sub(page_size);
    let offset = app.process_table_state.offset().min(max_offset);
    let visible_processes = app.visible_process_row_window(offset, page_size);
    let selected_table_column_index = app.selected_process_column_index;
    let visible_columns = visible_metric_columns(
        area.width,
        &app.process_columns,
        app.process_metric_column_offset,
    );
    let full_path_width = full_path_column_render_width(area.width, &visible_columns);
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
                full_path_width,
                selected_table_column_index,
                row_selected,
                theme,
            )
        })
        .collect::<Vec<_>>();
    if reserve_total_row {
        if let Some(total_row) = total_row {
            rows.push(process_table_row(
                &total_row,
                app,
                &visible_columns,
                full_path_width,
                selected_table_column_index,
                false,
                theme,
            ));
        }
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
    let header = Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD));

    let constraints = process_table_constraints(&visible_columns);

    let table = Table::new(rows, constraints)
        .header(header)
        .column_spacing(TABLE_COLUMN_SPACING)
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    let mut state = app.process_table_state.clone();
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
    has_row_selection: bool,
) -> Option<usize> {
    let table_area = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if x < table_area.x || x >= table_area.right() {
        return None;
    }

    let visible_columns = visible_metric_columns(area.width, columns, metric_offset);
    let constraints = process_table_constraints(&visible_columns);
    let selection_width = if has_row_selection {
        HIGHLIGHT_SYMBOL_WIDTH
    } else {
        0
    };
    let [_selection_area, columns_area] =
        Layout::horizontal([Constraint::Length(selection_width), Constraint::Fill(0)])
            .areas(table_area);
    let column_rects = Layout::horizontal(constraints)
        .spacing(TABLE_COLUMN_SPACING)
        .split(columns_area);

    if let Some(pid_rect) = column_rects.get(1) {
        if x >= pid_rect.x && x < pid_rect.right() {
            return Some(0);
        }
    }
    if let Some(process_rect) = column_rects.get(2) {
        if x >= process_rect.x && x < process_rect.right() {
            return Some(1);
        }
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
) -> usize {
    FIXED_SELECTABLE_COLUMN_COUNT + visible_metric_columns(area_width, columns, metric_offset).len()
}

pub(crate) fn process_table_visible_metric_range(
    area_width: u16,
    columns: &[MetricColumn],
    metric_offset: usize,
) -> std::ops::Range<usize> {
    let visible = visible_metric_columns(area_width, columns, metric_offset);
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
) -> Vec<(usize, MetricColumn)> {
    let usable_width = area_width.saturating_sub(TABLE_BORDER_WIDTH + HIGHLIGHT_SYMBOL_WIDTH);
    let fixed_width = TRACKED_COLUMN_WIDTH
        + PID_COLUMN_WIDTH
        + PROCESS_COLUMN_MIN_WIDTH
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
            let width = metric_column_window_width(candidate);
            if used_width.saturating_add(width) > metric_width {
                false
            } else {
                used_width = used_width.saturating_add(width);
                true
            }
        })
        .collect()
}

fn process_table_row_capacity(table_area: Rect) -> usize {
    table_area.height.saturating_sub(1).max(1) as usize
}

fn metric_column_window_width(column: MetricColumn) -> u16 {
    TABLE_COLUMN_SPACING.saturating_add(metric_column_render_width(column))
}

fn process_table_constraints(visible_columns: &[(usize, MetricColumn)]) -> Vec<Constraint> {
    let process_constraint = if visible_columns
        .iter()
        .any(|(_, column)| *column == MetricColumn::FullPath)
    {
        Constraint::Length(PROCESS_COLUMN_MIN_WIDTH)
    } else {
        Constraint::Min(PROCESS_COLUMN_MIN_WIDTH)
    };
    let mut constraints = vec![
        Constraint::Length(TRACKED_COLUMN_WIDTH),
        Constraint::Length(PID_COLUMN_WIDTH),
        process_constraint,
    ];
    for (_, column) in visible_columns {
        let constraint = if *column == MetricColumn::FullPath {
            Constraint::Min(column.width())
        } else {
            Constraint::Length(column.width())
        };
        constraints.push(constraint);
    }
    constraints
}

fn metric_column_render_width(column: MetricColumn) -> u16 {
    column.width()
}

fn full_path_column_render_width(
    area_width: u16,
    visible_columns: &[(usize, MetricColumn)],
) -> Option<u16> {
    visible_columns
        .iter()
        .any(|(_, column)| *column == MetricColumn::FullPath)
        .then(|| {
            let usable_width =
                area_width.saturating_sub(TABLE_BORDER_WIDTH + HIGHLIGHT_SYMBOL_WIDTH);
            let metric_width = visible_columns
                .iter()
                .map(|(_, column)| metric_column_render_width(*column))
                .sum::<u16>();
            let total_columns = 3 + visible_columns.len() as u16;
            let required_width = TRACKED_COLUMN_WIDTH
                + PID_COLUMN_WIDTH
                + PROCESS_COLUMN_MIN_WIDTH
                + metric_width
                + TABLE_COLUMN_SPACING.saturating_mul(total_columns.saturating_sub(1));
            MetricColumn::FullPath
                .width()
                .saturating_add(usable_width.saturating_sub(required_width))
        })
}

fn process_table_block<'a>(
    title: Line<'a>,
    app: &App,
    theme: Theme,
) -> ratatui::widgets::Block<'a> {
    let input_active =
        (app.is_filter_editing() || app.is_process_jump_editing()) && !app.has_modal_focus();
    let block = panel_block_focused(
        title,
        theme,
        app.panel_has_focus(FocusedPanel::Processes) || input_active,
    );
    if input_active {
        block.border_style(
            Style::default()
                .fg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        )
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
        cell = cell.style(
            Style::default()
                .bg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        );
    } else if selected {
        cell = cell.style(Style::default().bg(theme.panel_alt));
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
        Style::default().fg(theme.background).bg(theme.accent)
    } else {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    };
    let style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    Cell::from(content.into().alignment(alignment)).style(style)
}

fn header_label(label: &str, direction: Option<SortDirection>) -> String {
    match direction {
        Some(SortDirection::Asc) => format!("{label} ↑"),
        Some(SortDirection::Desc) => format!("{label} ↓"),
        None => label.to_string(),
    }
}

fn process_table_row(
    row: &VisibleProcessRow<'_>,
    app: &App,
    visible_columns: &[(usize, MetricColumn)],
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
            process_name_line(row, app, theme),
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
        let graph_slot_numbers = if row.is_tracked_total {
            None
        } else {
            graph_slot_numbers_for_cell(app, process, *column)
        };
        let column_width = if *column == MetricColumn::FullPath {
            full_path_width.unwrap_or_else(|| column.width())
        } else {
            column.width()
        };
        cells.push(process_metric_cell(
            process,
            *column,
            column_width,
            app,
            selected_column,
            selected_cell,
            graph_slot_numbers.as_deref(),
            text_style,
            theme,
        ));
    }
    Row::new(cells).style(process_row_style(row_selected, row.multi_selected, theme))
}

fn process_metric_cell(
    process: &ProcessRow,
    column: MetricColumn,
    column_width: u16,
    app: &App,
    selected: bool,
    selected_cell: bool,
    graph_slot_numbers: Option<&str>,
    text_style: Style,
    theme: Theme,
) -> Cell<'static> {
    if let Some(graph_slot_numbers) = graph_slot_numbers {
        let mut cell = Cell::from(process_metric_line_with_graph_slots(
            process,
            column,
            column_width,
            graph_slot_numbers,
            theme,
            text_style,
        ));
        if selected_cell {
            cell = cell.style(
                Style::default()
                    .bg(theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            );
        } else if selected {
            cell = cell.style(Style::default().bg(theme.panel_alt));
        }
        return cell;
    }
    let mut cell = Cell::from(process_metric_line(
        process,
        column,
        column_width,
        app,
        text_style,
        theme,
    ));
    if selected_cell {
        cell = cell.style(
            Style::default()
                .bg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        );
    } else if selected {
        cell = cell.style(Style::default().bg(theme.panel_alt));
    }
    cell
}

fn graph_slot_numbers_for_cell(
    app: &App,
    process: &ProcessRow,
    column: MetricColumn,
) -> Option<String> {
    let identity = crate::model::ProcessIdentity::from_row(process);
    let numbers = app
        .graph_slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            slot.as_ref()
                .is_some_and(|slot| {
                    slot.process_metric()
                        .is_some_and(|metric| metric.column() == column)
                        && slot.process_identity() == Some(&identity)
                })
                .then(|| char::from(b'1' + index as u8))
        })
        .collect::<String>();
    (!numbers.is_empty()).then_some(numbers)
}

fn process_row_style(selected: bool, multi_selected: bool, theme: Theme) -> Style {
    let fg = theme.text;
    if selected {
        Style::default()
            .fg(fg)
            .bg(theme.highlight)
            .add_modifier(Modifier::BOLD)
    } else if multi_selected {
        Style::default().fg(fg).bg(theme.selection)
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

fn process_metric_line_with_graph_slots(
    process: &ProcessRow,
    column: MetricColumn,
    column_width: u16,
    graph_slot_numbers: &str,
    theme: Theme,
    text_style: Style,
) -> Line<'static> {
    let value = format_process_column(process, column, column_width);
    let column_width = column_width as usize;
    let number_width = graph_slot_numbers.chars().count().min(column_width);
    let value_width = value.chars().count();
    let spacing = column_width.saturating_sub(number_width + value_width);
    Line::from(vec![
        graph_slot_marker_span(graph_slot_numbers, number_width, theme),
        Span::raw(" ".repeat(spacing)),
        Span::styled(value, text_style),
    ])
}

fn tracked_cell(row: &VisibleProcessRow<'_>, theme: Theme) -> Cell<'static> {
    let symbol = tracked_symbol(row.tracked);
    let color = match row.lifecycle {
        ProcessLifecycle::Live => theme.tracked,
        ProcessLifecycle::Exited { .. } => theme.exited,
    };
    Cell::from(Line::from(Span::styled(symbol, Style::default().fg(color))))
}

fn tracked_symbol(tracked: bool) -> &'static str {
    if tracked { "★" } else { " " }
}

fn process_display_name(process: &ProcessRow, lifecycle: &ProcessLifecycle) -> String {
    match lifecycle {
        ProcessLifecycle::Live => process.name.clone(),
        ProcessLifecycle::Exited { exited_at } => {
            format!("{}({})", process.name, exited_at.format("%H:%M:%S"))
        }
    }
}

fn process_name_line(row: &VisibleProcessRow<'_>, app: &App, theme: Theme) -> Line<'static> {
    let process = row.process;
    let display_name = process_display_name(process, &row.lifecycle);
    let base_style = process_text_style(row, theme);
    let query = (if app.is_process_jump_editing() {
        Some(app.process_jump_draft().trim())
    } else {
        active_filter_query(app)
    })
    .filter(|query| !query.is_empty());
    match query {
        Some(query) => {
            highlighted_process_name_line(&display_name, &process.name, query, base_style, theme)
        }
        None => Line::from(Span::styled(display_name, base_style)),
    }
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
    display_name: &str,
    process_name: &str,
    query: &str,
    base_style: Style,
    theme: Theme,
) -> Line<'static> {
    let name_lower = process_name.to_ascii_lowercase();
    let query_lower = query.to_ascii_lowercase();
    let Some(start) = name_lower.find(&query_lower) else {
        return Line::from(Span::styled(display_name.to_string(), base_style));
    };
    let end = start + query_lower.len();
    if !display_name.is_char_boundary(start) || !display_name.is_char_boundary(end) {
        return Line::from(Span::styled(display_name.to_string(), base_style));
    }
    highlighted_match_line_at(display_name, start, end, base_style, theme)
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
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
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
            spans.push(Span::styled(
                segment.label,
                process_title_segment_style(segment.kind, app, theme),
            ));
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
            .fg(theme.warning)
            .remove_modifier(Modifier::BOLD),
        ProcessTitleSegmentKind::TrackedOnly => Style::default()
            .fg(theme.muted)
            .remove_modifier(Modifier::BOLD),
        ProcessTitleSegmentKind::Filter => Style::default()
            .fg(theme.warning)
            .remove_modifier(Modifier::BOLD),
    }
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
                .bg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            query.to_string(),
            Style::default()
                .fg(theme.accent_alt)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "_",
            Style::default()
                .fg(theme.background)
                .bg(theme.accent_alt)
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

    let mut prefix_width = PROCESS_TITLE.chars().count();
    for segment in process_table_state_segments(app) {
        prefix_width = prefix_width.saturating_add(TITLE_SEPARATOR.chars().count());
        if segment.kind == ProcessTitleSegmentKind::TrackedOnly {
            let title_x = area.x.saturating_add(1).saturating_add(prefix_width as u16);
            if title_x >= area.right() {
                return None;
            }
            return Some(Rect::new(
                title_x,
                area.y,
                (segment.label.chars().count() as u16).min(area.right().saturating_sub(title_x)),
                1,
            ));
        }
        prefix_width = prefix_width.saturating_add(segment.label.chars().count());
    }
    None
}

fn process_tracked_only_label(app: &App) -> &'static str {
    if app.watch_enabled {
        "Tracked only"
    } else {
        "All processes"
    }
}

fn format_optional_integer(value: Option<u64>) -> String {
    value
        .map(format_integer)
        .unwrap_or_else(|| "--".to_string())
}

fn format_process_column(process: &ProcessRow, column: MetricColumn, column_width: u16) -> String {
    match column {
        MetricColumn::CpuPercent => process
            .cpu_percent
            .map(format_cpu_percent)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::PrivateBytes => format_optional_integer(process.private_bytes),
        MetricColumn::WorksetBytes => format_optional_integer(process.workset_bytes),
        MetricColumn::WorksetPrivateBytes => format_optional_integer(process.workset_private_bytes),
        MetricColumn::WorksetShareableBytes => {
            format_optional_integer(process.workset_shareable_bytes)
        }
        MetricColumn::WorksetSharedBytes => format_optional_integer(process.workset_shared_bytes),
        MetricColumn::ThreadCount => format_optional_integer(process.thread_count),
        MetricColumn::HandleCount => format_optional_integer(process.handle_count),
        MetricColumn::UserObjectCount => format_optional_integer(process.user_object_count),
        MetricColumn::GdiObjectCount => format_optional_integer(process.gdi_object_count),
        MetricColumn::GpuPercent => process
            .gpu_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::DotNetHeapBytes => format_optional_integer(process.dotnet_heap_bytes),
        MetricColumn::GpuDedicatedBytes => format_optional_integer(process.gpu_dedicated_bytes),
        MetricColumn::GpuSharedBytes => format_optional_integer(process.gpu_shared_bytes),
        MetricColumn::IoReadBytesPerSec => process
            .io_read_bytes_per_sec
            .map(format_mbps)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::IoWriteBytesPerSec => process
            .io_write_bytes_per_sec
            .map(format_mbps)
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
    let char_count = path.chars().count();
    if char_count <= width {
        return path.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let tail = path
        .chars()
        .rev()
        .take(width - 3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{tail}")
}

fn format_cpu_percent(value: f64) -> String {
    format!("{value:.1}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Styled;

    #[test]
    fn tracked_cell_uses_star_for_tracked_rows_only() {
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
            workset_shared_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
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

        assert_eq!(tracked_symbol(tracked.tracked), "★");
        assert_eq!(tracked_symbol(ordinary.tracked), " ");
    }

    #[test]
    fn pid_column_width_matches_practical_pid_width() {
        assert_eq!(PID_COLUMN_WIDTH, 6);
        assert!(PID_COLUMN_WIDTH >= 5);
    }

    #[test]
    fn header_label_shows_sort_direction() {
        assert_eq!(header_label("CPU%", Some(SortDirection::Asc)), "CPU% ↑");
        assert_eq!(header_label("CPU%", Some(SortDirection::Desc)), "CPU% ↓");
        assert_eq!(header_label("CPU%", None), "CPU%");
    }

    #[test]
    fn header_cells_are_underlined() {
        let theme = crate::ui::theme::THEMES[0];
        let cell = header_cell("Private", Alignment::Right, false, theme);

        let style = Styled::style(&cell);

        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn full_path_column_is_left_aligned_and_keeps_path_tail() {
        assert_eq!(
            process_metric_alignment(MetricColumn::FullPath),
            Alignment::Left
        );
        assert_eq!(
            compact_path_start(r"C:\very\long\workspace\target\debug\app.exe", 18),
            r"...t\debug\app.exe"
        );
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
            workset_shared_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
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
    fn multi_selected_rows_use_selection_color() {
        let theme = crate::ui::theme::THEMES[0];

        assert_eq!(
            process_row_style(false, true, theme).bg,
            Some(theme.selection)
        );
        assert!(
            !process_row_style(false, true, theme)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn visible_metric_columns_keep_pid_and_process_width_reserved() {
        let columns = MetricColumn::ALL.to_vec();

        let visible = visible_metric_columns(100, &columns, 0);

        assert!(!visible.is_empty());
        let metric_width = visible
            .iter()
            .map(|(_, column)| metric_column_render_width(*column))
            .sum::<u16>();
        let total_columns = 3 + visible.len() as u16;
        let total_width = TRACKED_COLUMN_WIDTH
            + PID_COLUMN_WIDTH
            + PROCESS_COLUMN_MIN_WIDTH
            + metric_width
            + TABLE_COLUMN_SPACING.saturating_mul(total_columns.saturating_sub(1));
        assert!(total_width <= 100 - TABLE_BORDER_WIDTH - HIGHLIGHT_SYMBOL_WIDTH);
    }

    #[test]
    fn full_path_column_takes_extra_width_when_visible() {
        let visible = vec![(0, MetricColumn::PrivateBytes), (1, MetricColumn::FullPath)];

        assert_eq!(
            process_table_constraints(&visible),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(PID_COLUMN_WIDTH),
                Constraint::Length(PROCESS_COLUMN_MIN_WIDTH),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
                Constraint::Min(MetricColumn::FullPath.width()),
            ]
        );
        assert_eq!(
            full_path_column_render_width(140, &visible),
            Some(MetricColumn::FullPath.width() + 54)
        );
    }

    #[test]
    fn process_column_takes_extra_width_when_full_path_is_hidden() {
        let visible = vec![(0, MetricColumn::PrivateBytes)];

        assert_eq!(
            process_table_constraints(&visible),
            vec![
                Constraint::Length(TRACKED_COLUMN_WIDTH),
                Constraint::Length(PID_COLUMN_WIDTH),
                Constraint::Min(PROCESS_COLUMN_MIN_WIDTH),
                Constraint::Length(MetricColumn::PrivateBytes.width()),
            ]
        );
        assert_eq!(full_path_column_render_width(140, &visible), None);
    }

    #[test]
    fn visible_metric_columns_drop_metrics_when_fixed_columns_need_space() {
        let columns = MetricColumn::ALL.to_vec();

        let visible = visible_metric_columns(35, &columns, 0);

        assert!(visible.is_empty());
        assert!(PID_COLUMN_WIDTH >= 5);
    }

    #[test]
    fn visible_metric_columns_start_at_requested_offset() {
        let columns = MetricColumn::ALL.to_vec();
        let offset = columns.len() - 2;

        let visible = visible_metric_columns(72, &columns, offset);

        assert!(!visible.is_empty());
        assert_eq!(visible.first().map(|(index, _)| *index), Some(offset));
    }
}
