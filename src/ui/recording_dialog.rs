use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    app::state::RecordingErrorKind,
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
const RECORDING_PATH_HEIGHT: u16 = 9;
const RECORDING_PATH_INPUT_ROW: u16 = 1;
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
        Paragraph::new("Log file").style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y, content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(theme.text).bg(theme.panel_alt)),
        input_area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Tracking List  ", Style::default().fg(theme.muted)),
            Span::styled(
                format!(
                    "{} {} · fixed until recording stops.",
                    app.watch_list.len(),
                    if app.watch_list.len() == 1 {
                        "name"
                    } else {
                        "names"
                    }
                ),
                Style::default().fg(theme.text),
            ),
        ])),
        Rect::new(content.x, content.y.saturating_add(3), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new("Up to 24 hours · JSON Lines (.log) · open later with Ctrl+L.")
            .style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y.saturating_add(4), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(shortcut_line(
            &[
                ("Enter", "start"),
                ("Esc", "cancel"),
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
    frame.set_cursor_position(Position::new(
        input_area.x.saturating_add(cursor_x as u16),
        input_area.y,
    ));
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
