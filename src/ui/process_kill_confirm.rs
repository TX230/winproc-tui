use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    ui::{Theme, footer::warning_shortcut_spans, widgets::confirm_dialog},
};

const POPUP_WIDTH: u16 = 64;
const POPUP_HEIGHT: u16 = 9;

pub(crate) fn draw_process_kill_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = process_kill_dialog_area(area);
    let pids = app
        .process_kill_targets
        .iter()
        .map(|target| target.pid)
        .collect::<Vec<_>>();
    let pid_list = compact_pid_list(&pids, 54);
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
            format!("PIDs: {pid_list}"),
            Style::default().fg(theme.text),
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

fn compact_pid_list(pids: &[u32], max_chars: usize) -> String {
    let joined = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let shown = joined
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{shown}+")
}
