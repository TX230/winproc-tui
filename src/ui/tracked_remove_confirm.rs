use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY,
    ui::{Theme, footer::warning_shortcut_spans, format::format_integer, widgets::confirm_dialog},
};

const POPUP_WIDTH: u16 = 74;
const POPUP_HEIGHT: u16 = 9;

pub(crate) fn draw_tracked_remove_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = tracked_remove_dialog_area(area);
    let retained = format_integer(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as u64);
    let total = format_integer(app.tracked_remove_total_samples as u64);
    let discarded = format_integer(app.tracked_remove_discarded_samples as u64);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Remove from Tracking List?",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(""),
        Line::from(Span::styled(
            format!("{} has {total} in-memory samples.", app.tracked_remove_name),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!(
                "This will keep the latest {retained} samples and discard {discarded} older samples."
            ),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled("Continue?", Style::default().fg(theme.text))),
        Line::from(""),
        Line::from(warning_shortcut_spans(
            &[("Enter", "Remove"), ("Esc", "Cancel")],
            theme,
        ))
        .alignment(Alignment::Center),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines).block(confirm_dialog::warning_block("CONFIRM", theme));
    frame.render_widget(dialog, popup);
}

pub(crate) fn tracked_remove_dialog_area(area: Rect) -> Rect {
    confirm_dialog::centered_dialog_rect(area, POPUP_WIDTH, POPUP_HEIGHT)
}
