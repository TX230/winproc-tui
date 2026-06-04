use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::{App, ProcessKillSelection, distinct_process_kill_image_names},
    ui::{
        Theme,
        widgets::{block::panel_block_focused, confirm_dialog},
    },
};

const BUTTON_ROW_FROM_CONTENT_TOP: u16 = 7;
const POPUP_WIDTH: u16 = 64;
const POPUP_HEIGHT: u16 = 11;

pub(crate) fn draw_process_kill_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = process_kill_dialog_area(area);
    let image_names = distinct_process_kill_image_names(&app.process_kill_targets);
    let image_list = compact_image_name_list(&image_names, 54);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Kill Selected Processes?",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Selected rows: {}", app.process_kill_targets.len()),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!("Image names: {image_list}"),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "taskkill /f /im terminates all matching image names.",
            Style::default().fg(theme.warning),
        )),
        Line::from(Span::styled("Continue?", Style::default().fg(theme.text))),
        Line::from(""),
        button_line(app.process_kill_selection, theme),
        Line::from(Span::styled(
            "Enter selects / Esc cancels / y kills",
            Style::default().fg(theme.muted),
        )),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(panel_block_focused("Confirm", theme, true))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn process_kill_button_at(area: Rect, x: u16, y: u16) -> Option<ProcessKillSelection> {
    let popup = process_kill_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let buttons = confirm_dialog::button_areas(
        content,
        BUTTON_ROW_FROM_CONTENT_TOP,
        &[" Kill ", " Cancel "],
    );
    buttons
        .into_iter()
        .enumerate()
        .find(|(_, area)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
        .map(|(index, _)| {
            if index == 0 {
                ProcessKillSelection::Kill
            } else {
                ProcessKillSelection::Cancel
            }
        })
}

pub(crate) fn process_kill_dialog_area(area: Rect) -> Rect {
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn button_line(selection: ProcessKillSelection, theme: Theme) -> Line<'static> {
    Line::from(vec![
        button(" Kill ", selection == ProcessKillSelection::Kill, theme),
        Span::raw("   "),
        button(" Cancel ", selection == ProcessKillSelection::Cancel, theme),
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

fn compact_image_name_list(names: &[String], max_chars: usize) -> String {
    let joined = names.join(", ");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let shown = joined
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{shown}+")
}
