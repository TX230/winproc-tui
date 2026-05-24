use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::{App, RecordingOverwriteSelection, RecordingPathSelection},
    ui::{
        Theme,
        layout::centered_rect,
        widgets::{block::panel_block_focused, confirm_dialog},
    },
};

const RECORDING_PATH_WIDTH: u16 = 78;
const RECORDING_PATH_HEIGHT: u16 = 8;
const RECORDING_PATH_INPUT_ROW: u16 = 1;
const RECORDING_PATH_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 5;
const OVERWRITE_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 4;
const NO_TRACKED_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 4;

pub(crate) fn draw_recording_path_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_PATH_WIDTH, RECORDING_PATH_HEIGHT);
    let block = recording_block("Recording", theme);
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
        Paragraph::new("Path").style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y, content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(theme.text).bg(theme.panel_alt)),
        input_area,
    );
    frame.render_widget(
        Paragraph::new("Missing directories will be created automatically. Tab completes.")
            .style(Style::default().fg(theme.muted)),
        Rect::new(content.x, content.y.saturating_add(3), content.width, 1),
    );
    frame.render_widget(
        Paragraph::new(confirm_dialog::button_line(
            &[
                (
                    " Start ",
                    app.recording_path_selection == RecordingPathSelection::Start,
                ),
                (
                    " Cancel ",
                    app.recording_path_selection == RecordingPathSelection::Cancel,
                ),
            ],
            theme,
        ))
        .alignment(Alignment::Right),
        Rect::new(
            content.x,
            content
                .y
                .saturating_add(RECORDING_PATH_BUTTON_ROW_FROM_CONTENT_TOP),
            content.width,
            1,
        ),
    );
    frame.set_cursor_position(Position::new(
        input_area.x.saturating_add(cursor_x as u16),
        input_area.y,
    ));
}

pub(crate) fn recording_path_button_at(
    area: Rect,
    x: u16,
    y: u16,
) -> Option<RecordingPathSelection> {
    let popup =
        confirm_dialog::centered_dialog_rect(area, RECORDING_PATH_WIDTH, RECORDING_PATH_HEIGHT);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let buttons = confirm_dialog::right_aligned_button_areas(
        content,
        RECORDING_PATH_BUTTON_ROW_FROM_CONTENT_TOP,
        &[" Start ", " Cancel "],
    );
    buttons
        .into_iter()
        .enumerate()
        .find(|(_, area)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
        .map(|(index, _)| {
            if index == 0 {
                RecordingPathSelection::Start
            } else {
                RecordingPathSelection::Cancel
            }
        })
}

pub(crate) fn draw_recording_overwrite_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = centered_rect(48, 16, area);
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
        overwrite_button_line(app.recording_overwrite_selection, theme),
        Line::from(Span::styled(
            "Enter selects / Esc cancels / y overwrites",
            Style::default().fg(theme.muted),
        )),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(recording_block("Confirm", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn draw_recording_no_tracked_warning(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    theme: Theme,
) {
    let popup = centered_rect(52, 16, area);
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
        confirm_dialog::button_line(&[(" OK ", true)], theme),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(recording_block("Warning", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn recording_overwrite_button_at(
    area: Rect,
    x: u16,
    y: u16,
) -> Option<RecordingOverwriteSelection> {
    let popup = centered_rect(48, 16, area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let buttons = confirm_dialog::button_areas(
        content,
        OVERWRITE_BUTTON_ROW_FROM_CONTENT_TOP,
        &[" Overwrite ", " Cancel "],
    );
    buttons
        .into_iter()
        .enumerate()
        .find(|(_, area)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
        .map(|(index, _)| {
            if index == 0 {
                RecordingOverwriteSelection::Overwrite
            } else {
                RecordingOverwriteSelection::Cancel
            }
        })
}

pub(crate) fn recording_no_tracked_ok_button_area(area: Rect) -> Option<Rect> {
    let popup = centered_rect(52, 16, area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    confirm_dialog::button_areas(content, NO_TRACKED_BUTTON_ROW_FROM_CONTENT_TOP, &[" OK "])
        .into_iter()
        .next()
}

fn recording_block(title: &'static str, theme: Theme) -> ratatui::widgets::Block<'static> {
    panel_block_focused(title, theme, true)
}

fn overwrite_button_line(selection: RecordingOverwriteSelection, theme: Theme) -> Line<'static> {
    Line::from(vec![
        button(
            " Overwrite ",
            selection == RecordingOverwriteSelection::Overwrite,
            theme,
        ),
        Span::raw("   "),
        button(
            " Cancel ",
            selection == RecordingOverwriteSelection::Cancel,
            theme,
        ),
    ])
}

fn button(label: &'static str, selected: bool, theme: Theme) -> Span<'static> {
    if selected {
        Span::styled(
            format!("[{label}]"),
            Style::default()
                .fg(confirm_button_text(theme))
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!("[{label}]"),
            Style::default().fg(theme.text).bg(theme.panel_alt),
        )
    }
}

fn confirm_button_text(theme: Theme) -> Color {
    match theme.name {
        "Light" => theme.background,
        _ => Color::Black,
    }
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
