use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::{AppActivity, ProcessInfoFocus},
    model::open_files::FileAttribute,
    samplers::open_files::OpenFileEntry,
    ui::Theme,
};

const HANDLE_COLUMN_WIDTH: usize = 10;
const ATTRIBUTE_COLUMNS: [(FileAttribute, usize); 4] = [
    (FileAttribute::Cache, 6),
    (FileAttribute::IoMode, 5),
    (FileAttribute::Access, 6),
    (FileAttribute::WriteThrough, 6),
];

pub(crate) fn selected_entry(app: &App) -> Option<&OpenFileEntry> {
    filtered_entries(app).get(app.open_files_selected).copied()
}

pub(crate) fn entry_row_height(width: usize) -> usize {
    if width < 72 {
        // Reserve enough wrapped lines for the longest labels, including unavailable fields.
        1 + "Cached: --  Async: --  Access: RWA  W-Thru: --"
            .len()
            .div_ceil(width.max(1))
    } else {
        1
    }
}

pub(crate) fn entry_row_prefix(app: &App) -> usize {
    let Some(report) = &app.open_files_result else {
        return 0;
    };
    3 + usize::from(report.inaccessible_handles > 0)
        + usize::from(report.unnamed_file_handles > 0)
        + usize::from(report.entries.iter().any(OpenFileEntry::has_unknown))
}

pub(crate) fn index_at(area: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    if app.open_files_show_detail
        || !area.contains((x, y).into())
        || x == area.right().saturating_sub(1)
    {
        return None;
    }
    let line = app.open_files_scroll.offset + usize::from(y - area.y);
    let index = line.checked_sub(entry_row_prefix(app))?
        / entry_row_height(area.width.saturating_sub(1) as usize);
    (index < filtered_entries(app).len()).then_some(index)
}

pub(crate) fn draw_open_files_tab(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let lines = open_files_lines(app, theme, area.width.saturating_sub(1) as usize);
    let line_count = lines.len();
    let rows = area.height.max(1) as usize;
    let offset = app
        .open_files_scroll
        .offset
        .min(line_count.saturating_sub(rows));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.panel_alt))
            .scroll((offset as u16, 0)),
        area,
    );
    set_open_files_filter_cursor(frame, area, app, line_count);
    render_open_files_scrollbar(frame, area, app, theme);
}

pub(crate) fn open_files_scrollbar_area(area: Rect, app: &App) -> Option<Rect> {
    let rows = app.open_files_scroll.page_size.max(1);
    if open_files_lines(app, app.theme(), area.width.saturating_sub(1) as usize).len() <= rows
        || area.is_empty()
    {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(1),
        area.y,
        1,
        area.height,
    ))
}

pub(crate) fn open_files_total_rows(app: &App) -> usize {
    let width = super::process_info_content_area_for_screen(app.last_screen_area)
        .width
        .saturating_sub(1) as usize;
    open_files_lines(app, app.theme(), width).len()
}

fn open_files_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.activity() == AppActivity::LogView {
        lines.push(Line::from(Span::styled(
            "This information is not included in recording logs.",
            Style::default().fg(theme.muted),
        )));
        return lines;
    }
    let Some(report) = &app.open_files_result else {
        lines.push(Line::from(Span::styled(
            "Loading...",
            Style::default().fg(theme.muted),
        )));
        return lines;
    };

    if let Some(error) = &report.error {
        lines.push(Line::from(Span::styled(
            format!(
                "{} ({} / PID {}) · Auto off; Ctrl+U retry",
                error.message(),
                report.process_name,
                report.pid
            ),
            Style::default().fg(theme.danger),
        )));
        return lines;
    }

    if app.open_files_show_detail {
        if let Some(entry) = selected_entry(app) {
            return entry
                .detail_text()
                .into_iter()
                .flat_map(|line| {
                    super::process_info_dialog::wrap_display_width(&line, width.max(1))
                })
                .map(Line::from)
                .collect();
        }
        return vec![Line::from("File is no longer present in this capture.")];
    }

    let entries = filtered_entries(app);
    let total_paths = report.entries.len();
    let path_count = if app.open_files_filter.is_empty() {
        format!("named handles {total_paths}")
    } else {
        format!("shown {}/{total_paths}", entries.len())
    };
    lines.push(Line::from(Span::styled(
        format!(
            "Live result {} · {}  handles {}  file handles {}  {}{}",
            app.open_files_refresh
                .received_at
                .map(|time| time.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "--".to_string()),
            app.open_files_refresh_label(),
            report.total_handles,
            report.file_handles,
            path_count,
            if app.open_files_in_flight.is_some() {
                "  refreshing..."
            } else {
                ""
            }
        ),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(theme.muted)),
        Span::styled(
            filter_input_text(app, width),
            Style::default().fg(theme.text),
        ),
    ]));
    if report.inaccessible_handles > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} handles could not be inspected",
                report.inaccessible_handles
            ),
            Style::default().fg(theme.warning),
        )));
    }
    if report.unnamed_file_handles > 0 {
        lines.push(Line::from(Span::styled(
            format!("{} file handles had no path", report.unnamed_file_handles),
            Style::default().fg(theme.warning),
        )));
    }
    if report.entries.iter().any(OpenFileEntry::has_unknown) {
        lines.push(Line::from(Span::styled(
            "Some I/O attributes are unavailable; Enter shows details",
            Style::default().fg(theme.warning),
        )));
    }

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.open_files_filter.is_empty() {
                "No named disk file handles."
            } else {
                "No matching paths."
            },
            Style::default().fg(theme.muted),
        )));
    } else {
        lines.push(open_files_table_header(theme, width));
        for (index, entry) in entries.into_iter().enumerate() {
            lines.extend(open_files_table_rows(
                entry,
                theme,
                width,
                index == app.open_files_selected,
                app.process_info_focus == ProcessInfoFocus::Content,
            ));
        }
    }
    lines
}

fn set_open_files_filter_cursor(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    line_count: usize,
) {
    if app.process_info_focus != ProcessInfoFocus::Content || app.open_files_show_detail {
        return;
    }
    let Some(report) = &app.open_files_result else {
        return;
    };
    if report.error.is_some() {
        return;
    }

    let filter_row = 1usize;
    let rows = area.height.max(1) as usize;
    let offset = app
        .open_files_scroll
        .offset
        .min(line_count.saturating_sub(rows));
    if filter_row < offset || filter_row >= offset.saturating_add(rows) {
        return;
    }

    let (_, cursor_x) = filter_input_view(
        &app.open_files_filter,
        app.open_files_filter_cursor,
        filter_input_width(area.width as usize),
    );
    let label_width = filter_label_width();
    frame.set_cursor_position(Position::new(
        area.x
            .saturating_add((label_width + cursor_x) as u16)
            .min(area.right().saturating_sub(1)),
        area.y.saturating_add((filter_row - offset) as u16),
    ));
}

fn render_open_files_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(scrollbar_area) = open_files_scrollbar_area(area, app) else {
        return;
    };
    let total = open_files_lines(app, theme, area.width.saturating_sub(1) as usize).len();
    let rows = app.open_files_scroll.page_size.max(1);
    let mut state = ScrollbarState::new(total)
        .position(open_files_scrollbar_position(
            total,
            rows,
            app.open_files_scroll.offset,
        ))
        .viewport_content_length(rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(theme.muted).bg(theme.panel_alt))
        .thumb_style(
            Style::default()
                .fg(if app.process_info_focus == ProcessInfoFocus::Content {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel_alt),
        );
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut state);
}

fn open_files_scrollbar_position(total: usize, rows: usize, offset: usize) -> usize {
    let rows = rows.max(1).min(total);
    let max_offset = total.saturating_sub(rows);
    if total == 0 || max_offset == 0 {
        return 0;
    }
    let max_scrollbar_position = total.saturating_sub(1);
    (offset.min(max_offset) * max_scrollbar_position + max_offset / 2) / max_offset
}

pub(crate) fn filtered_entries(app: &App) -> Vec<&OpenFileEntry> {
    let Some(report) = &app.open_files_result else {
        return Vec::new();
    };
    let terms = filter_terms(&app.open_files_filter);
    if terms.is_empty() {
        return report.entries.iter().collect();
    }
    report
        .entries
        .iter()
        .filter(|entry| {
            let path = entry.path.to_lowercase();
            terms.iter().any(|term| path.contains(term))
        })
        .collect()
}

fn filter_terms(filter: &str) -> Vec<String> {
    filter.split_whitespace().map(str::to_lowercase).collect()
}

fn filter_input_text(app: &App, width: usize) -> String {
    filter_input_view(
        &app.open_files_filter,
        app.open_files_filter_cursor,
        filter_input_width(width),
    )
    .0
}

fn filter_label_width() -> usize {
    "Filter: ".len()
}

fn filter_input_width(width: usize) -> usize {
    width.saturating_sub(filter_label_width()).max(1)
}

fn filter_input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    let width = width.max(1);
    let cursor = cursor.min(value.len());
    let cursor_char = value[..cursor].chars().count();
    let chars = value.chars().collect::<Vec<_>>();
    let start_char = cursor_char.saturating_sub(width.saturating_sub(1));
    let visible = chars
        .iter()
        .skip(start_char)
        .take(width)
        .collect::<String>();
    let cursor_x = cursor_char
        .saturating_sub(start_char)
        .min(width.saturating_sub(1));
    (visible, cursor_x)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn directory_name(path: &str) -> &str {
    path.rfind(['\\', '/'])
        .map(|index| &path[..index])
        .unwrap_or("")
}

fn column_widths(width: usize) -> (usize, usize) {
    let attributes = if entry_row_height(width) == 1 {
        ATTRIBUTE_COLUMNS.iter().map(|(_, size)| size + 1).sum()
    } else {
        0
    };
    let remaining = width.saturating_sub(HANDLE_COLUMN_WIDTH + 2 + attributes);
    let file = (remaining / 2).max(8).min(remaining);
    (file, remaining.saturating_sub(file))
}

fn open_files_table_header(theme: Theme, width: usize) -> Line<'static> {
    let header_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let (file, directory) = column_widths(width);
    let mut spans = vec![
        Span::styled(fit_cell("Handle", HANDLE_COLUMN_WIDTH), header_style),
        Span::raw(" "),
        Span::styled(fit_cell("File", file), header_style),
    ];
    if entry_row_height(width) == 1 {
        for (attribute, size) in ATTRIBUTE_COLUMNS {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                fit_cell(attribute.label(), size),
                header_style,
            ));
        }
    }
    spans.push(Span::raw(" "));
    spans.push(Span::styled(fit_cell("Directory", directory), header_style));
    Line::from(spans)
}

fn open_files_table_rows(
    entry: &OpenFileEntry,
    theme: Theme,
    width: usize,
    selected: bool,
    focused: bool,
) -> Vec<Line<'static>> {
    let style = Style::default().fg(theme.text).bg(if selected {
        if focused {
            theme.focus_surface
        } else {
            theme.table_selection_surface
        }
    } else {
        theme.panel_alt
    });
    let style = if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    };
    let (file, directory) = column_widths(width);
    let mut spans = vec![
        Span::styled(
            fit_cell(&format!("0x{:X}", entry.handle.value), HANDLE_COLUMN_WIDTH),
            style.fg(theme.accent),
        ),
        Span::raw(" "),
        Span::styled(fit_cell_start(file_name(&entry.path), file), style),
    ];
    if entry_row_height(width) == 1 {
        for (attribute, size) in ATTRIBUTE_COLUMNS {
            spans.push(Span::styled(
                format!(" {}", fit_cell(&entry.attribute(attribute), size)),
                style,
            ));
        }
    }
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        fit_cell_start(directory_name(&entry.path), directory),
        style.fg(theme.muted),
    ));
    if entry_row_height(width) == 1 {
        vec![Line::from(spans).style(style)]
    } else {
        let mut rows = vec![Line::from(spans).style(style)];
        let compact = format!(
            "Cached: {}  Async: {}  Access: {}  W-Thru: {}",
            entry.attribute(FileAttribute::Cache),
            entry.attribute(FileAttribute::IoMode),
            entry.attribute(FileAttribute::Access),
            entry.attribute(FileAttribute::WriteThrough)
        );
        rows.extend(
            super::process_info_dialog::wrap_display_width(&compact, width.max(1))
                .into_iter()
                .map(|line| Line::from(Span::styled(line, style))),
        );
        rows.resize_with(entry_row_height(width), || {
            Line::from(Span::styled("", style))
        });
        rows
    }
}

fn fit_cell(value: &str, width: usize) -> String {
    let truncated = truncate_end(value, width);
    let padding = width.saturating_sub(Span::raw(&truncated).width());
    format!("{truncated}{}", " ".repeat(padding))
}

fn fit_cell_start(value: &str, width: usize) -> String {
    let truncated = truncate_path_start(value, width);
    let padding = width.saturating_sub(Span::raw(&truncated).width());
    format!("{truncated}{}", " ".repeat(padding))
}

fn truncate_end(value: &str, width: usize) -> String {
    if Span::raw(value).width() <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut used = 0;
    let head = value
        .chars()
        .take_while(|ch| {
            used += Span::raw(ch.to_string()).width();
            used <= width - 3
        })
        .collect::<String>();
    format!("{head}...")
}

fn truncate_path_start(path: &str, width: usize) -> String {
    if Span::raw(path).width() <= width {
        return path.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut used = 0;
    let tail = path
        .chars()
        .rev()
        .take_while(|ch| {
            used += Span::raw(ch.to_string()).width();
            used <= width - 3
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_uses_last_path_segment() {
        assert_eq!(file_name(r"C:\tmp\app.log"), "app.log");
        assert_eq!(file_name("/tmp/app.log"), "app.log");
    }

    #[test]
    fn truncate_path_start_keeps_file_name_side() {
        assert_eq!(
            truncate_path_start(r"C:\very\long\path\app.log", 14),
            r"...ath\app.log"
        );
    }

    #[test]
    fn open_files_scrollbar_position_reaches_end_at_last_viewport() {
        assert_eq!(open_files_scrollbar_position(100, 10, 0), 0);
        assert_eq!(open_files_scrollbar_position(100, 10, 90), 99);
        assert_eq!(open_files_scrollbar_position(100, 10, 900), 99);
    }
}
