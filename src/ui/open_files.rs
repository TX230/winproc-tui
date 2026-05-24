use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    samplers::open_files::OpenFileEntry,
    ui::{Theme, widgets::scrollable_modal::ScrollableModal},
};

const CLOSE_BUTTON: &str = "[ Close ]";
const CONTENT_HEIGHT: u16 = 18;
const FOOTER_HEIGHT: u16 = 3;
const COUNT_COLUMN_WIDTH: usize = 5;
const FILE_COLUMN_WIDTH: usize = 38;
const DIRECTORY_COLUMN_WIDTH: usize = 92;

pub(crate) fn draw_open_files(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let modal = open_files_modal();
    let expected_layout = modal.layout(area);
    let lines = open_files_lines(app, theme, expected_layout.content.width as usize);
    let line_count = lines.len();
    let layout = modal.render(
        frame,
        area,
        Text::from(lines),
        app.open_files_scroll.offset,
        false,
        theme,
    );
    set_open_files_filter_cursor(frame, &layout, app, line_count);
    render_open_files_scrollbar(frame, area, app, theme);
    if layout.footer.height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(theme))),
            Rect::new(layout.footer.x, layout.footer.y, layout.footer.width, 1),
        );
    }
    if let Some(area) = open_files_close_button_area_in_footer(layout.footer) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                CLOSE_BUTTON,
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))),
            area,
        );
    }
}

pub(crate) fn open_files_page_size_for_screen(area: Rect, app: &App) -> usize {
    let _ = app;
    open_files_modal().page_size(area)
}

pub(crate) fn open_files_scrollbar_area_for_screen(area: Rect, app: &App) -> Option<Rect> {
    let layout = open_files_modal().layout(area);
    let rows = app.open_files_scroll.page_size.max(1);
    if open_files_total_rows(app) <= rows || layout.content.is_empty() {
        return None;
    }
    Some(Rect::new(
        layout
            .content
            .right()
            .min(layout.area.right().saturating_sub(2)),
        layout.content.y,
        1,
        layout.content.height,
    ))
}

pub(crate) fn open_files_close_button_area_for_screen(area: Rect, app: &App) -> Option<Rect> {
    let _ = app;
    open_files_close_button_area(open_files_modal().area(area))
}

pub(crate) fn open_files_total_rows(app: &App) -> usize {
    match &app.open_files_result {
        Some(report) => {
            if report.error.is_some() {
                return 1;
            }
            let diagnostics = usize::from(report.inaccessible_handles > 0)
                + usize::from(report.unnamed_file_handles > 0);
            3 + diagnostics + filtered_entries(app).len()
        }
        None => 1,
    }
}

fn open_files_lines(app: &App, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
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
                "{} ({} / PID {})",
                error.message(),
                report.process_name,
                report.pid
            ),
            Style::default().fg(theme.danger),
        )));
        return lines;
    }

    lines.push(Line::from(Span::styled(
        format!(
            "{} / PID {}  handles {}  file handles {}  paths {}{}",
            report.process_name,
            report.pid,
            report.total_handles,
            report.file_handles,
            filtered_entries(app).len(),
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

    let entries = filtered_entries(app);
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.open_files_filter.is_empty() {
                "No named disk file handles."
            } else {
                "No matching file names."
            },
            Style::default().fg(theme.muted),
        )));
    } else {
        lines.push(open_files_table_header(theme));
        for entry in entries {
            lines.push(open_files_table_row(entry, theme));
        }
    }
    lines
}

fn set_open_files_filter_cursor(
    frame: &mut ratatui::Frame<'_>,
    layout: &crate::ui::widgets::scrollable_modal::ScrollableModalLayout,
    app: &App,
    line_count: usize,
) {
    let Some(report) = &app.open_files_result else {
        return;
    };
    if report.error.is_some() {
        return;
    }

    let filter_row = 1usize;
    let rows = layout.content.height.max(1) as usize;
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
        filter_input_width(layout.content.width as usize),
    );
    let label_width = filter_label_width();
    frame.set_cursor_position(Position::new(
        layout
            .content
            .x
            .saturating_add((label_width + cursor_x) as u16)
            .min(layout.content.right().saturating_sub(1)),
        layout
            .content
            .y
            .saturating_add((filter_row - offset) as u16),
    ));
}

fn shortcut_spans(theme: Theme) -> Vec<Span<'static>> {
    let items = [
        ("Ctrl+U", "refresh"),
        ("Ctrl+C", "copy paths"),
        ("Esc/Enter", "close"),
    ];
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text),
        ));
    }
    spans
}

fn open_files_modal() -> ScrollableModal {
    ScrollableModal::new("Open files", 140, CONTENT_HEIGHT, FOOTER_HEIGHT)
}

fn render_open_files_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(scrollbar_area) = open_files_scrollbar_area_for_screen(area, app) else {
        return;
    };
    let total = open_files_total_rows(app);
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
        .style(Style::default().fg(theme.muted).bg(theme.panel))
        .thumb_style(Style::default().fg(theme.accent).bg(theme.panel));
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
            let name = file_name(&entry.path).to_lowercase();
            terms.iter().any(|term| name.contains(term))
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

fn open_files_table_header(theme: Theme) -> Line<'static> {
    let header_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    Line::from(vec![
        Span::styled(fit_cell("Count", COUNT_COLUMN_WIDTH), header_style),
        Span::raw(" "),
        Span::styled(fit_cell("File", FILE_COLUMN_WIDTH), header_style),
        Span::raw(" "),
        Span::styled(fit_cell("Directory", DIRECTORY_COLUMN_WIDTH), header_style),
    ])
}

fn open_files_table_row(entry: &OpenFileEntry, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:>width$}", entry.handle_count, width = COUNT_COLUMN_WIDTH),
            Style::default().fg(theme.accent),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell_start(file_name(&entry.path), FILE_COLUMN_WIDTH),
            Style::default().fg(theme.text),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell_start(directory_name(&entry.path), DIRECTORY_COLUMN_WIDTH),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn fit_cell(value: &str, width: usize) -> String {
    let truncated = truncate_end(value, width);
    format!("{truncated:<width$}")
}

fn fit_cell_start(value: &str, width: usize) -> String {
    let truncated = truncate_path_start(value, width);
    format!("{truncated:<width$}")
}

fn truncate_end(value: &str, width: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let head = value
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    format!("{head}...")
}

fn truncate_path_start(path: &str, width: usize) -> String {
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
        .take(width.saturating_sub(3))
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

fn open_files_close_button_area(popup: Rect) -> Option<Rect> {
    if popup.width < 11 || popup.height < 4 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        popup.x + popup.width.saturating_sub(width) / 2,
        popup.bottom().saturating_sub(3),
        width,
        1,
    ))
}

fn open_files_close_button_area_in_footer(footer: Rect) -> Option<Rect> {
    if footer.width < 11 || footer.height == 0 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        footer.x + footer.width.saturating_sub(width) / 2,
        footer.y.saturating_add(1),
        width,
        1,
    ))
}
