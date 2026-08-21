use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::{AppActivity, ProcessInfoFocus},
    model::ProcessModuleEntry,
    ui::Theme,
};

pub(crate) fn draw_process_modules_tab(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let lines = process_module_lines(app, theme, area.width as usize);
    let line_count = lines.len();
    let rows = area.height.max(1) as usize;
    let offset = app
        .process_info_dlls_scroll
        .offset
        .min(line_count.saturating_sub(rows));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.panel))
            .scroll((offset as u16, 0)),
        area,
    );
    set_filter_cursor(frame, area, app, line_count);
    render_scrollbar(frame, area, app, theme);
}

pub(crate) fn process_modules_total_rows(app: &App, width: u16) -> usize {
    process_module_lines(app, app.theme(), width as usize).len()
}

pub(crate) fn process_modules_scrollbar_area(area: Rect, app: &App) -> Option<Rect> {
    let rows = app.process_info_dlls_scroll.page_size.max(1);
    if process_modules_total_rows(app, area.width) <= rows || area.is_empty() {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(1),
        area.y,
        1,
        area.height,
    ))
}

pub(crate) fn process_module_index_at(area: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    if app.process_modules_show_detail
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
        .process_info_dlls_scroll
        .offset
        .saturating_add((y - area.y) as usize);
    let prefix = entry_row_prefix(app);
    let index = line.checked_sub(prefix)?;
    (index < entries.len()).then_some(index)
}

pub(crate) fn filtered_entries(app: &App) -> Vec<&ProcessModuleEntry> {
    let Some(report) = &app.process_modules_result else {
        return Vec::new();
    };
    let terms = app
        .process_modules_filter
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
            let haystack = entry.path.to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

pub(crate) fn selected_entry(app: &App) -> Option<&ProcessModuleEntry> {
    filtered_entries(app)
        .get(app.process_modules_selected)
        .copied()
}

fn process_module_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    if app.activity() == AppActivity::LogView {
        return vec![Line::from(Span::styled(
            "Not recorded in Log view.",
            Style::default().fg(theme.muted),
        ))];
    }

    let Some(report) = &app.process_modules_result else {
        return vec![Line::from(Span::styled(
            app.process_modules_error
                .map(|error| error.message())
                .unwrap_or("Loading..."),
            Style::default().fg(if app.process_modules_error.is_some() {
                theme.danger
            } else {
                theme.muted
            }),
        ))];
    };

    if app.process_modules_show_detail {
        return process_module_detail_lines(app, theme, width);
    }

    let entries = filtered_entries(app);
    let total = report.entries.len();
    let count_text = if app.process_modules_filter.is_empty() {
        format!("DLLs {total}")
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
            if app.process_modules_in_flight.is_some() {
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
    if let Some(error) = app.process_modules_error {
        lines.push(Line::from(Span::styled(
            format!("Last refresh failed: {}", error.message()),
            Style::default().fg(theme.warning),
        )));
    }

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.process_modules_filter.is_empty() {
                "No loaded DLLs."
            } else {
                "No matching DLLs."
            },
            Style::default().fg(theme.muted),
        )));
        return lines;
    }

    lines.push(path_header(width, theme));
    for (index, entry) in entries.iter().enumerate() {
        lines.push(path_row(
            entry,
            width,
            index == app.process_modules_selected,
            theme,
        ));
    }
    lines
}

fn process_module_detail_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let Some(entry) = selected_entry(app) else {
        return vec![Line::from(Span::styled(
            "No DLL selected.",
            Style::default().fg(theme.muted),
        ))];
    };
    let mut lines = vec![Line::from(Span::styled(
        "DLL details",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))];
    for (label, value) in [
        ("Path", entry.path.as_str()),
        ("DLL file", entry.dll_name.as_str()),
        ("Company", entry.company_name.text()),
        ("Product Version", entry.product_version.text()),
        ("File Version", entry.file_version.text()),
        ("Modified", entry.modified.text()),
        ("Directory", entry.directory.as_str()),
    ] {
        lines.extend(detail_lines(label, value, width, theme));
    }
    lines
}

fn entry_row_prefix(app: &App) -> usize {
    3 + usize::from(app.process_modules_error.is_some())
}

fn path_header(width: usize, theme: Theme) -> Line<'static> {
    let style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    Line::from(Span::styled(fit_cell("DLL path", width.max(1)), style))
}

fn path_row(
    entry: &ProcessModuleEntry,
    width: usize,
    selected: bool,
    theme: Theme,
) -> Line<'static> {
    let style = Style::default().fg(theme.text).bg(if selected {
        theme.table_selection_surface
    } else {
        theme.panel
    });
    Line::from(Span::styled(fit_cell(&entry.path, width.max(1)), style))
}

fn detail_lines(label: &str, value: &str, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let label_width = 18usize.min(width.saturating_sub(1)).max(1);
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

fn fit_cell(value: &str, width: usize) -> String {
    let value = truncate_end(value, width);
    let padding = width.saturating_sub(display_width(&value));
    format!("{value}{}", " ".repeat(padding))
}

fn truncate_end(value: &str, width: usize) -> String {
    truncate_display(value, width, false)
}

fn truncate_display(value: &str, width: usize, from_start: bool) -> String {
    if display_width(value) <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let available = width - 3;
    let chars = value.chars().collect::<Vec<_>>();
    let mut kept = Vec::new();
    let iterator: Box<dyn Iterator<Item = &char>> = if from_start {
        Box::new(chars.iter().rev())
    } else {
        Box::new(chars.iter())
    };
    let mut used = 0usize;
    for ch in iterator {
        let ch_width = display_width(&ch.to_string()).max(1);
        if used.saturating_add(ch_width) > available {
            break;
        }
        kept.push(*ch);
        used += ch_width;
    }
    if from_start {
        kept.reverse();
        format!("...{}", kept.into_iter().collect::<String>())
    } else {
        format!("{}...", kept.into_iter().collect::<String>())
    }
}

fn wrap_display(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = display_width(&ch.to_string()).max(1);
        if used > 0 && used.saturating_add(ch_width) > width {
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
        &app.process_modules_filter,
        app.process_modules_filter_cursor,
        width.saturating_sub("Filter: ".len()).max(1),
    )
    .0
}

fn filter_input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    let cursor = cursor.min(value.len());
    let cursor_char = value[..cursor].chars().count();
    let chars = value.chars().collect::<Vec<_>>();
    let start = cursor_char.saturating_sub(width.saturating_sub(1));
    let visible = chars.iter().skip(start).take(width).collect::<String>();
    (
        visible,
        cursor_char
            .saturating_sub(start)
            .min(width.saturating_sub(1)),
    )
}

fn set_filter_cursor(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, total: usize) {
    if app.process_info_focus != ProcessInfoFocus::Content
        || app.process_modules_show_detail
        || app.process_modules_result.is_none()
    {
        return;
    }
    let filter_row = 1usize;
    let rows = area.height.max(1) as usize;
    let offset = app
        .process_info_dlls_scroll
        .offset
        .min(total.saturating_sub(rows));
    if filter_row < offset || filter_row >= offset.saturating_add(rows) {
        return;
    }
    let input_width = (area.width as usize)
        .saturating_sub("Filter: ".len())
        .max(1);
    let (_, cursor_x) = filter_input_view(
        &app.process_modules_filter,
        app.process_modules_filter_cursor,
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
    let Some(scrollbar_area) = process_modules_scrollbar_area(area, app) else {
        return;
    };
    let total = process_modules_total_rows(app, area.width);
    let rows = app.process_info_dlls_scroll.page_size.max(1);
    let max_offset = total.saturating_sub(rows.min(total));
    let position = (app.process_info_dlls_scroll.offset.min(max_offset) * total.saturating_sub(1))
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
    fn path_truncation_keeps_path_start() {
        assert_eq!(
            truncate_end(r"C:\very\long\path\module.dll", 16),
            "C:\\very\\long\\..."
        );
    }
}
