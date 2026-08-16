use chrono::{DateTime, Local};
use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    App,
    app::logs::LogSummary,
    ui::{
        Theme,
        widgets::{block::panel_title, confirm_dialog, scrollable_modal::ScrollableModal},
    },
};

const SHORTCUT_ITEMS: [(&str, &str); 5] = [
    ("↑/↓", "select"),
    ("Enter", "open"),
    ("d", "change dir"),
    ("r", "refresh"),
    ("Esc", "close"),
];
pub(crate) const LOG_LIST_HEADER_LINE_COUNT: u16 = 2;
const FOOTER_HEIGHT: u16 = 1;
const LOG_LIST_CONTENT_WIDTH: u16 = 74;
const LOG_DIR_DIALOG_WIDTH: u16 = 78;
const LOG_DIR_DIALOG_HEIGHT: u16 = 8;
const LOG_DIR_INPUT_ROW: u16 = 1;
const LOG_DIR_ERROR_ROW: u16 = 2;

pub(crate) fn draw_log_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let modal = log_list_modal(app);
    let content_width = modal.layout(area).content.width as usize;
    let mut lines = vec![
        log_list_state_line(app, content_width, theme),
        log_dir_line(app, content_width, theme),
    ];

    if app.log_summaries.is_empty() {
        lines.push(Line::from(Span::styled(
            if app.log_list_worker.is_some() {
                "Loading..."
            } else {
                "No .log files. Press d to change directory; Esc then Ctrl+R to record."
            },
            Style::default().fg(theme.muted),
        )));
    } else {
        for (index, summary) in app.log_summaries.iter().enumerate() {
            lines.push(log_summary_line(index, summary, app, content_width, theme));
        }
    }

    let layout = modal.render(
        frame,
        area,
        Text::from(lines),
        app.log_list_scroll.offset,
        false,
        theme,
    );
    if !layout.footer.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(log_list_shortcut_spans(theme))).alignment(Alignment::Left),
            layout.footer,
        );
    }
}

pub(crate) fn draw_log_dir_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup =
        confirm_dialog::centered_dialog_rect(area, LOG_DIR_DIALOG_WIDTH, LOG_DIR_DIALOG_HEIGHT);
    let block =
        crate::ui::widgets::block::panel_block_focused(panel_title("LOG DIRECTORY"), theme, true);
    let content = block.inner(popup);
    let input_area = Rect::new(
        content.x,
        content.y.saturating_add(LOG_DIR_INPUT_ROW),
        content.width,
        1,
    );
    let (input, cursor_x) = path_input_view(
        &app.log_dir_draft,
        app.log_dir_cursor,
        input_area.width as usize,
    );

    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Directory").style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y, content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(theme.text).bg(theme.panel_alt)),
        input_area,
    );
    if let Some(error) = &app.log_dir_error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(theme.danger)),
            Rect::new(
                content.x,
                content.y.saturating_add(LOG_DIR_ERROR_ROW),
                content.width,
                1,
            ),
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(
            &[
                ("Enter", "apply"),
                ("Esc", "cancel"),
                ("Ctrl+Space", "complete"),
            ],
            theme,
        )))
        .alignment(Alignment::Left),
        Rect::new(
            content.x,
            content.bottom().saturating_sub(1),
            content.width,
            1,
        ),
    );
    frame.set_cursor_position(Position::new(
        input_area.x.saturating_add(cursor_x as u16),
        input_area.y,
    ));
}

pub(crate) fn log_list_page_size_for_screen(area: Rect) -> usize {
    log_list_modal_for_height(32).page_size(area)
}

pub(crate) fn log_list_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll_offset: usize,
    summary_count: usize,
) -> Option<usize> {
    if summary_count == 0 {
        return None;
    }
    let total_rows = log_list_total_rows_for_count(summary_count);
    let content = log_list_modal_for_height(total_rows as u16)
        .layout(area)
        .content;
    if x < content.x || x >= content.right() || y < content.y || y >= content.bottom() {
        return None;
    }
    let row = y
        .saturating_sub(content.y)
        .saturating_add(scroll_offset as u16) as usize;
    let index = row.checked_sub(LOG_LIST_HEADER_LINE_COUNT as usize)?;
    (index < summary_count).then_some(index)
}

pub(crate) fn log_list_total_rows_for_count(summary_count: usize) -> usize {
    LOG_LIST_HEADER_LINE_COUNT as usize + summary_count.max(1)
}

fn log_summary_line(
    index: usize,
    summary: &LogSummary,
    app: &App,
    width: usize,
    theme: Theme,
) -> Line<'static> {
    let selected = index == app.log_list_index;
    let style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.highlight)
            .add_modifier(Modifier::BOLD)
    } else if summary.error.is_some() {
        Style::default().fg(theme.danger)
    } else {
        Style::default().fg(theme.text)
    };
    let cursor = if selected { ">" } else { " " };
    let schema = summary
        .schema_version
        .map(|value| format!("v{value}"))
        .unwrap_or_else(|| "v?".to_string());
    let started = summary
        .started_at
        .map(format_time)
        .unwrap_or_else(|| "--".to_string());
    let duration = log_duration(summary).unwrap_or_else(|| "--".to_string());
    let name = summary
        .path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| summary.path.display().to_string());
    let prefix = format!("{cursor} {schema:<2} {started:<19} {duration:>8} ");
    let suffix = if summary.error.is_some() {
        " [invalid]"
    } else {
        ""
    };
    let name_width = width
        .saturating_sub(prefix.chars().count())
        .saturating_sub(suffix.chars().count());
    Line::from(Span::styled(
        format!("{prefix}{}{suffix}", compact_path(&name, name_width)),
        style,
    ))
}

fn log_dir_line(app: &App, width: usize, theme: Theme) -> Line<'static> {
    let dir = app
        .log_list_dir
        .as_ref()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "--".to_string());
    Line::from(vec![
        Span::styled(
            "Dir ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            compact_path(&dir, width.saturating_sub(4)),
            Style::default().fg(theme.text),
        ),
    ])
}

fn log_list_state_line(app: &App, width: usize, theme: Theme) -> Line<'static> {
    let Some(worker) = &app.log_load_worker else {
        return Line::from(Span::styled(
            "Select a log file and press Enter.",
            Style::default().fg(theme.text),
        ));
    };
    let name = worker
        .path()
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| worker.path().display().to_string());
    let prefix = "Opening ";
    let suffix = "...";
    let name_width = width
        .saturating_sub(prefix.chars().count())
        .saturating_sub(suffix.chars().count());
    Line::from(vec![
        Span::styled(
            prefix,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            compact_path(&name, name_width),
            Style::default().fg(theme.text),
        ),
        Span::styled(suffix, Style::default().fg(theme.muted)),
    ])
}

fn format_time(value: DateTime<Local>) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn log_duration(summary: &LogSummary) -> Option<String> {
    let started_at = summary.started_at?;
    let ended_at = summary.ended_at?;
    Some(format_duration(ended_at.signed_duration_since(started_at)))
}

fn format_duration(duration: chrono::Duration) -> String {
    let total_seconds = duration.num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn compact_path(value: &str, max_width: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let tail_len = max_width / 2;
    let head_len = max_width.saturating_sub(tail_len + 3);
    let head = value.chars().take(head_len).collect::<String>();
    let tail = value
        .chars()
        .rev()
        .take(tail_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}...{tail}")
}

fn path_input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }

    let cursor = cursor.min(value.len());
    let cursor_char = value[..cursor].chars().count();
    let char_count = value.chars().count();
    let start_char = cursor_char.saturating_sub(width.saturating_sub(1));
    let end_char = start_char.saturating_add(width).min(char_count);
    let rendered = value
        .chars()
        .skip(start_char)
        .take(end_char.saturating_sub(start_char))
        .collect::<String>();
    (
        rendered,
        cursor_char
            .saturating_sub(start_char)
            .min(width.saturating_sub(1)),
    )
}

fn log_list_shortcut_spans(theme: Theme) -> Vec<Span<'static>> {
    shortcut_spans(&SHORTCUT_ITEMS, theme)
}

fn shortcut_spans(items: &[(&str, &str)], theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().fg(theme.key_hint),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text),
        ));
    }
    spans
}

fn log_list_modal(app: &App) -> ScrollableModal {
    log_list_modal_for_height(app.log_list_total_rows() as u16)
}

fn log_list_modal_for_height(content_height: u16) -> ScrollableModal {
    ScrollableModal::new(
        "LOGS",
        LOG_LIST_CONTENT_WIDTH,
        content_height.max(LOG_LIST_HEADER_LINE_COUNT + 1),
        FOOTER_HEIGHT,
    )
    .with_bold_title()
}
