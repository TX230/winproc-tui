use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::{
    app::state::RecordingErrorKind,
    app::{App, export::RECORDING_INTERVAL_OPTIONS_SECONDS},
    ui::{
        Theme,
        footer::{shortcut_spans, warning_shortcut_spans},
        widgets::{
            block::{panel_block_focused, panel_title},
            confirm_dialog,
        },
    },
};

const RECORDING_PATH_WIDTH: u16 = 78;
const RECORDING_PATH_HEIGHT: u16 = 13;
const RECORDING_PATH_INPUT_ROW: u16 = 3;
const RECORDING_INTERVAL_ROW: u16 = 5;
const RECORDING_INFO_LABEL_WIDTH: u16 = 15;
const RECORDING_OVERWRITE_WIDTH: u16 = 48;
const RECORDING_OVERWRITE_HEIGHT: u16 = 7;
const RECORDING_NO_TRACKED_WIDTH: u16 = 52;
const RECORDING_NO_TRACKED_HEIGHT: u16 = 7;
const RECORDING_FIXED_WIDTH: u16 = 58;
const RECORDING_FIXED_HEIGHT: u16 = 6;
const RECORDING_STOP_WIDTH: u16 = 62;
const RECORDING_STOP_HEIGHT: u16 = 7;
const RECORDING_ERROR_WIDTH: u16 = 72;
const RECORDING_ERROR_HEIGHT: u16 = 8;

pub(crate) fn draw_recording_path_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_PATH_WIDTH, RECORDING_PATH_HEIGHT);
    let block = recording_block(panel_title("RECORDING"), theme);
    let content = block.inner(popup);
    let input_area = Rect::new(
        content.x,
        content.y.saturating_add(RECORDING_PATH_INPUT_ROW),
        content.width,
        1,
    );
    let input_width = input_area.width as usize;
    let (input, cursor_x) = path_input_view(
        &app.recording_path_draft,
        app.recording_path_cursor,
        input_width,
    );

    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Confirm the log file and interval, then press Enter to start.")
            .style(Style::default().fg(theme.text)),
        Rect::new(content.x, content.y, content.width, 1),
    );
    frame.render_widget(
        Paragraph::new("Log file").style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y.saturating_add(2), content.width, 1),
    );
    let input_style = if app.recording_path_focused() {
        Style::default()
            .fg(theme.text)
            .bg(theme.focus_surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    };
    frame.render_widget(Paragraph::new(input).style(input_style), input_area);
    frame.render_widget(
        Paragraph::new("Interval").style(Style::default().fg(theme.muted)),
        Rect::new(
            content.x,
            content.y.saturating_add(RECORDING_INTERVAL_ROW),
            RECORDING_INFO_LABEL_WIDTH,
            1,
        ),
    );
    frame.render_widget(
        Paragraph::new(recording_interval_line(app, theme)),
        recording_interval_selector_area(area),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{:<width$}",
                    "Tracking List",
                    width = RECORDING_INFO_LABEL_WIDTH as usize
                ),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!(
                    "{} {} (fixed while recording)",
                    app.watch_list.len(),
                    if app.watch_list.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    }
                ),
                Style::default().fg(theme.text),
            ),
        ])),
        Rect::new(content.x, content.y.saturating_add(6), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{:<width$}",
                    "Format",
                    width = RECORDING_INFO_LABEL_WIDTH as usize
                ),
                Style::default().fg(theme.muted),
            ),
            Span::styled("JSON Lines (.log)", Style::default().fg(theme.text)),
        ])),
        Rect::new(content.x, content.y.saturating_add(7), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{:<width$}",
                    "Max duration",
                    width = RECORDING_INFO_LABEL_WIDTH as usize
                ),
                Style::default().fg(theme.muted),
            ),
            Span::styled("24 hours", Style::default().fg(theme.text)),
        ])),
        Rect::new(content.x, content.y.saturating_add(8), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(shortcut_line(
            &[
                ("Enter", "start"),
                ("Esc", "cancel"),
                ("Tab", "focus"),
                ("←/→", "value"),
                ("Ctrl+Space", "complete"),
            ],
            theme,
        )),
        Rect::new(
            content.x,
            content.bottom().saturating_sub(1),
            content.width,
            1,
        ),
    );
    if app.recording_path_focused() {
        frame.set_cursor_position(Position::new(
            input_area.x.saturating_add(cursor_x as u16),
            input_area.y,
        ));
    }
}

fn recording_interval_line(app: &App, theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, seconds) in RECORDING_INTERVAL_OPTIONS_SECONDS.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let selected = index == app.recording_interval_index;
        let marker = if selected { "(*)" } else { "( )" };
        let style = if selected && app.recording_interval_focused() {
            Style::default()
                .fg(theme.text)
                .bg(theme.focus_surface)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        spans.push(Span::styled(format!("{marker} {seconds}s"), style));
    }
    Line::from(spans)
}

pub(crate) fn draw_recording_tracking_fixed(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    theme: Theme,
) {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_FIXED_WIDTH, RECORDING_FIXED_HEIGHT);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Tracking List is fixed while recording.",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "Stop recording before changing it.",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(shortcut_spans(&[("Enter/Esc", "Close")], theme)),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(recording_block(panel_title("RECORDING"), theme))
            .alignment(Alignment::Center),
        popup,
    );
}

pub(crate) fn draw_recording_stop_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    theme: Theme,
) {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_STOP_WIDTH, RECORDING_STOP_HEIGHT);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Stop recording and close this log?",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Recording continues until Stop is confirmed.",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(warning_shortcut_spans(
            &[("Enter/Esc/n", "Continue"), ("y", "Stop")],
            theme,
        )),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(confirm_dialog::warning_block("STOP RECORDING", theme))
            .alignment(Alignment::Center),
        popup,
    );
}

pub(crate) fn draw_recording_error(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(error) = app.recording_error.as_ref() else {
        return;
    };
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_ERROR_WIDTH, RECORDING_ERROR_HEIGHT);
    let message = match error.kind {
        RecordingErrorKind::CouldNotStart => "Recording could not start.",
        RecordingErrorKind::Stopped => "Recording stopped because the log could not be written.",
    };
    let path = error.path.display().to_string();
    let lines = Text::from(vec![
        Line::from(Span::styled(message, Style::default().fg(theme.text))),
        Line::from(""),
        Line::from(vec![
            Span::styled("Log: ", Style::default().fg(theme.muted)),
            Span::styled(compact_path(&path, 62), Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Error: ", Style::default().fg(theme.muted)),
            Span::styled(
                compact_path(&error.message, 60),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(""),
        Line::from(shortcut_spans(&[("Enter/Esc", "Close")], theme)),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(recording_error_block(theme))
            .alignment(Alignment::Center),
        popup,
    );
}

pub(crate) fn recording_path_input_area(area: Rect) -> Rect {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_PATH_WIDTH, RECORDING_PATH_HEIGHT);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    Rect::new(
        content.x,
        content.y.saturating_add(RECORDING_PATH_INPUT_ROW),
        content.width,
        1,
    )
}

pub(crate) fn recording_interval_selector_area(area: Rect) -> Rect {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_PATH_WIDTH, RECORDING_PATH_HEIGHT);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    Rect::new(
        content.x.saturating_add(RECORDING_INFO_LABEL_WIDTH),
        content.y.saturating_add(RECORDING_INTERVAL_ROW),
        content.width.saturating_sub(RECORDING_INFO_LABEL_WIDTH),
        1,
    )
}

pub(crate) fn recording_interval_option_at(area: Rect, column: u16, row: u16) -> Option<usize> {
    let selector = recording_interval_selector_area(area);
    if row != selector.y || column < selector.x || column >= selector.right() {
        return None;
    }
    let relative = column.saturating_sub(selector.x);
    let mut start = 0_u16;
    for (index, seconds) in RECORDING_INTERVAL_OPTIONS_SECONDS.into_iter().enumerate() {
        let width = format!("( ) {seconds}s").chars().count() as u16;
        if relative >= start && relative < start.saturating_add(width) {
            return Some(index);
        }
        start = start.saturating_add(width).saturating_add(2);
    }
    None
}

pub(crate) fn draw_recording_overwrite_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = recording_overwrite_dialog_area(area);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Overwrite existing log?",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            compact_path(&app.recording_path_draft, 42),
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(warning_shortcut_spans(
            &[("Enter/Esc/n", "Cancel"), ("y", "Overwrite")],
            theme,
        )),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(confirm_dialog::warning_block("CONFIRM", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn draw_recording_no_tracked_warning(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    theme: Theme,
) {
    let popup = confirm_dialog::centered_dialog_rect(
        area,
        RECORDING_NO_TRACKED_WIDTH,
        RECORDING_NO_TRACKED_HEIGHT,
    );
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "No tracked processes",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Track a process before starting recording.",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(warning_shortcut_spans(&[("Enter/Esc", "Close")], theme)),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(confirm_dialog::warning_block("WARNING", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

fn recording_overwrite_dialog_area(area: Rect) -> Rect {
    confirm_dialog::centered_dialog_rect(
        area,
        RECORDING_OVERWRITE_WIDTH,
        RECORDING_OVERWRITE_HEIGHT,
    )
}

fn recording_block<'a>(title: impl Into<Line<'a>>, theme: Theme) -> ratatui::widgets::Block<'a> {
    panel_block_focused(title, theme, true)
}

fn recording_error_block(theme: Theme) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            "RECORDING ERROR",
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.panel))
}

fn shortcut_line(items: &[(&str, &str)], theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", Style::default().fg(theme.muted)));
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
    Line::from(spans)
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

fn compact_path(value: &str, max_width: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_width {
        return value.to_string();
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
