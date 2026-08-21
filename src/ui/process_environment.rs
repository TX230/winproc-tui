use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::{AppActivity, ProcessInfoFocus},
    model::ProcessEnvironmentEntry,
    ui::Theme,
};

const NAME_MIN_WIDTH: usize = 12;
const NAME_PREFERRED_WIDTH: usize = 32;

pub(crate) fn draw_process_environment_tab(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let lines = environment_lines(app, theme, area.width as usize);
    let total = lines.len();
    let rows = area.height.max(1) as usize;
    let offset = app
        .process_info_environment_scroll
        .offset
        .min(total.saturating_sub(rows));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.panel))
            .scroll((offset as u16, 0)),
        area,
    );
    set_filter_cursor(frame, area, app, total);
    render_scrollbar(frame, area, app, theme);
}

pub(crate) fn process_environment_total_rows(app: &App, width: u16) -> usize {
    environment_lines(app, app.theme(), width as usize).len()
}

pub(crate) fn process_environment_scrollbar_area(area: Rect, app: &App) -> Option<Rect> {
    let rows = app.process_info_environment_scroll.page_size.max(1);
    if area.is_empty() || process_environment_total_rows(app, area.width) <= rows {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(1),
        area.y,
        1,
        area.height,
    ))
}

pub(crate) fn process_environment_index_at(area: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    if app.process_environment_show_detail
        || x < area.x
        || x >= area.right()
        || y < area.y
        || y >= area.bottom()
    {
        return None;
    }
    let entries = filtered_entries(app);
    if entries.is_empty() {
        return None;
    }
    let line = app
        .process_info_environment_scroll
        .offset
        .saturating_add((y - area.y) as usize);
    let index = line.checked_sub(entry_row_prefix(app))?;
    (index < entries.len()).then_some(index)
}

pub(crate) fn filtered_entries(app: &App) -> Vec<&ProcessEnvironmentEntry> {
    let Some(report) = &app.process_environment_result else {
        return Vec::new();
    };
    let terms = app
        .process_environment_filter
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return report.entries.iter().collect();
    }
    report
        .entries
        .iter()
        .filter(|entry| {
            let name = entry.name.to_lowercase();
            let value = entry.value.to_lowercase();
            terms
                .iter()
                .any(|term| name.contains(term) || value.contains(term))
        })
        .collect()
}

pub(crate) fn selected_entry(app: &App) -> Option<&ProcessEnvironmentEntry> {
    filtered_entries(app)
        .get(app.process_environment_selected)
        .copied()
}

fn environment_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    if app.activity() == AppActivity::LogView {
        return vec![Line::from(Span::styled(
            "Not recorded in Log view.",
            Style::default().fg(theme.muted),
        ))];
    }
    let Some(report) = &app.process_environment_result else {
        return vec![Line::from(Span::styled(
            app.process_environment_error
                .map(|error| error.message())
                .unwrap_or("Loading..."),
            Style::default().fg(if app.process_environment_error.is_some() {
                theme.danger
            } else {
                theme.muted
            }),
        ))];
    };

    if app.process_environment_show_detail {
        return process_environment_detail_lines(app, theme, width);
    }

    let entries = filtered_entries(app);
    let total = report.entries.len();
    let count_text = if app.process_environment_filter.is_empty() {
        format!("variables {total}")
    } else {
        format!("shown {}/{total}", entries.len())
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} / PID {}  Captured {}{}  {}{}",
            report.process_name,
            report.pid,
            report.captured_at.format("%Y-%m-%d %H:%M:%S"),
            if app.process_info_target_is_currently_live() {
                ""
            } else {
                " · process exited"
            },
            count_text,
            if app.process_environment_in_flight.is_some() {
                "  refreshing..."
            } else {
                ""
            }
        ),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(theme.muted)),
        Span::styled(
            filter_input_text(app, width),
            Style::default().fg(theme.text),
        ),
    ]));
    if let Some(error) = app.process_environment_error {
        lines.push(Line::from(Span::styled(
            format!("Last refresh failed: {}", error.message()),
            Style::default().fg(theme.warning),
        )));
    }
    if report.malformed_entries > 0 {
        lines.push(Line::from(Span::styled(
            format!("{} malformed entries skipped", report.malformed_entries),
            Style::default().fg(theme.warning),
        )));
    }

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.process_environment_filter.is_empty() {
                "No environment variables."
            } else {
                "No matching environment variables."
            },
            Style::default().fg(theme.muted),
        )));
        return lines;
    }

    let (name_width, value_width) = column_widths(width);
    let header = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    lines.push(Line::from(vec![
        Span::styled(fit_cell("Name", name_width), header),
        Span::raw(" "),
        Span::styled(fit_cell("Value", value_width), header),
    ]));
    for (index, entry) in entries.iter().enumerate() {
        let style =
            Style::default()
                .fg(theme.text)
                .bg(if index == app.process_environment_selected {
                    theme.table_selection_surface
                } else {
                    theme.panel
                });
        lines.push(Line::from(vec![
            Span::styled(fit_cell(&entry.name, name_width), style.fg(theme.accent)),
            Span::styled(" ", style),
            Span::styled(fit_cell(&entry.value, value_width), style),
        ]));
    }

    lines
}

fn process_environment_detail_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let Some(entry) = selected_entry(app) else {
        return vec![Line::from(Span::styled(
            "No environment variable selected.",
            Style::default().fg(theme.muted),
        ))];
    };
    let mut lines = vec![Line::from(Span::styled(
        "Environment variable details",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))];
    lines.extend(detail_lines("Name", &entry.name, width, theme));
    lines.extend(detail_lines("Value", &entry.value, width, theme));
    lines
}

fn entry_row_prefix(app: &App) -> usize {
    3 + usize::from(app.process_environment_error.is_some())
        + app
            .process_environment_result
            .as_ref()
            .map(|report| usize::from(report.malformed_entries > 0))
            .unwrap_or(0)
}

fn column_widths(width: usize) -> (usize, usize) {
    let available = width.saturating_sub(1).max(2);
    let name = NAME_PREFERRED_WIDTH
        .min(available.saturating_sub(1))
        .max(NAME_MIN_WIDTH.min(available / 2));
    (name, available.saturating_sub(name).max(1))
}

fn fit_cell(value: &str, width: usize) -> String {
    let value = truncate_end(value, width);
    format!(
        "{value}{}",
        " ".repeat(width.saturating_sub(display_width(&value)))
    )
}

fn truncate_end(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let available = width - 3;
    let mut result = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = display_width(&ch.to_string()).max(1);
        if used.saturating_add(ch_width) > available {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    result.push_str("...");
    result
}

fn detail_lines(label: &str, value: &str, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let label_width = 10usize.min(width.saturating_sub(1)).max(1);
    let value_width = width.saturating_sub(label_width).max(1);
    wrap_display(value, value_width)
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            Line::from(vec![
                Span::styled(
                    format!("{:<label_width$}", if index == 0 { label } else { "" }),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(value, Style::default().fg(theme.text)),
            ])
        })
        .collect()
}

fn wrap_display(value: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = display_width(&ch.to_string()).max(1);
        if used > 0 && used.saturating_add(ch_width) > width.max(1) {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(ch);
        used += ch_width;
    }
    lines.push(line);
    lines
}

fn display_width(value: &str) -> usize {
    Span::raw(value.to_string()).width()
}

fn filter_input_text(app: &App, width: usize) -> String {
    filter_input_view(
        &app.process_environment_filter,
        app.process_environment_filter_cursor,
        width.saturating_sub("Filter: ".len()).max(1),
    )
    .0
}

fn filter_input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    let cursor = cursor.min(value.len());
    let cursor_char = value[..cursor].chars().count();
    let chars = value.chars().collect::<Vec<_>>();
    let start = cursor_char.saturating_sub(width.saturating_sub(1));
    (
        chars.iter().skip(start).take(width).collect(),
        cursor_char
            .saturating_sub(start)
            .min(width.saturating_sub(1)),
    )
}

fn set_filter_cursor(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, total: usize) {
    if app.process_info_focus != ProcessInfoFocus::Content
        || app.process_environment_show_detail
        || app.process_environment_result.is_none()
    {
        return;
    }
    let filter_row = 1usize;
    let rows = area.height.max(1) as usize;
    let offset = app
        .process_info_environment_scroll
        .offset
        .min(total.saturating_sub(rows));
    if filter_row < offset || filter_row >= offset.saturating_add(rows) {
        return;
    }
    let input_width = (area.width as usize)
        .saturating_sub("Filter: ".len())
        .max(1);
    let (_, cursor_x) = filter_input_view(
        &app.process_environment_filter,
        app.process_environment_filter_cursor,
        input_width,
    );
    frame.set_cursor_position(Position::new(
        area.x
            .saturating_add(("Filter: ".len() + cursor_x) as u16)
            .min(area.right().saturating_sub(1)),
        area.y.saturating_add((filter_row - offset) as u16),
    ));
}

fn render_scrollbar(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(scrollbar_area) = process_environment_scrollbar_area(area, app) else {
        return;
    };
    let total = process_environment_total_rows(app, area.width);
    let rows = app.process_info_environment_scroll.page_size.max(1);
    let max_offset = total.saturating_sub(rows.min(total));
    let position = (app.process_info_environment_scroll.offset.min(max_offset)
        * total.saturating_sub(1))
    .checked_div(max_offset)
    .unwrap_or(0);
    let mut state = ScrollbarState::new(total)
        .position(position)
        .viewport_content_length(rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(theme.muted).bg(theme.panel))
        .thumb_style(
            Style::default()
                .fg(if app.process_info_focus == ProcessInfoFocus::Content {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel),
        );
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_environment_table_keeps_name_and_value_columns() {
        let (name, value) = column_widths(30);
        assert!(name >= NAME_MIN_WIDTH);
        assert!(value > 0);
        assert_eq!(name + value + 1, 30);
    }

    #[test]
    fn long_values_are_wrapped_without_loss() {
        assert_eq!(wrap_display("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }
}
