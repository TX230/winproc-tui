use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::{App, distinct_process_kill_image_names},
    ui::{Theme, footer::warning_shortcut_spans, widgets::confirm_dialog},
};

const POPUP_WIDTH: u16 = 64;
const POPUP_HEIGHT: u16 = 10;

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
        Line::from(warning_shortcut_spans(
            &[("Enter", "Kill"), ("Esc", "Cancel")],
            theme,
        )),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(confirm_dialog::warning_block("CONFIRM", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
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
