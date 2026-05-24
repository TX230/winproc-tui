use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::{App, TrackedRemoveSelection},
    model::GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY,
    ui::{
        Theme,
        format::format_integer,
        layout::centered_rect,
        widgets::{block::panel_block_focused, confirm_dialog},
    },
};

const BUTTON_ROW_FROM_CONTENT_TOP: u16 = 6;

pub(crate) fn draw_tracked_remove_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = centered_rect(68, 26, area);
    let retained = format_integer(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as u64);
    let total = format_integer(app.tracked_remove_total_samples as u64);
    let discarded = format_integer(app.tracked_remove_discarded_samples as u64);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Remove from Tracked List?",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )),
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
        button_line(app.tracked_remove_selection, theme),
        Line::from(Span::styled(
            "Enter selects / Esc cancels / y removes",
            Style::default().fg(theme.muted),
        )),
    ]);

    frame.render_widget(Clear, popup);
    let dialog = Paragraph::new(lines)
        .block(panel_block_focused("Confirm", theme, true))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn tracked_remove_button_at(
    area: Rect,
    x: u16,
    y: u16,
) -> Option<TrackedRemoveSelection> {
    let popup = centered_rect(68, 26, area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let buttons = confirm_dialog::button_areas(
        content,
        BUTTON_ROW_FROM_CONTENT_TOP,
        &[" Remove ", " Cancel "],
    );
    buttons
        .into_iter()
        .enumerate()
        .find(|(_, area)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
        .map(|(index, _)| {
            if index == 0 {
                TrackedRemoveSelection::Remove
            } else {
                TrackedRemoveSelection::Cancel
            }
        })
}

fn button_line(selection: TrackedRemoveSelection, theme: Theme) -> Line<'static> {
    Line::from(vec![
        button(
            " Remove ",
            selection == TrackedRemoveSelection::Remove,
            theme,
        ),
        Span::raw("   "),
        button(
            " Cancel ",
            selection == TrackedRemoveSelection::Cancel,
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
