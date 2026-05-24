use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::{App, SettingsSelection},
    ui::{
        Theme,
        widgets::{
            block::panel_block_focused,
            confirm_dialog::{button_areas, button_line, centered_dialog_rect},
        },
    },
};

const SETTINGS_WIDTH: u16 = 44;
const SETTINGS_HEIGHT: u16 = 8;
const OK_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 5;

pub(crate) fn draw_settings_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = centered_dialog_rect(area, SETTINGS_WIDTH, SETTINGS_HEIGHT);
    frame.render_widget(Clear, popup);
    let block = panel_block_focused("Settings", theme, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        setting_row(
            "Samples panel",
            app.show_samples_panel,
            app.settings_selection == SettingsSelection::SamplesPanel,
            theme,
        ),
        rows[0],
    );
    frame.render_widget(
        setting_row(
            "Delta",
            app.show_sample_delta,
            app.settings_selection == SettingsSelection::Delta,
            theme,
        ),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Space toggles. Enter closes.",
            Style::default().fg(theme.muted),
        ))),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(button_line(&[(" OK ", true)], theme))
            .alignment(ratatui::layout::Alignment::Center),
        rows[4],
    );
}

pub(crate) fn settings_ok_button_area(area: Rect) -> Option<Rect> {
    let popup = centered_dialog_rect(area, SETTINGS_WIDTH, SETTINGS_HEIGHT);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    button_areas(content, OK_BUTTON_ROW_FROM_CONTENT_TOP, &[" OK "])
        .into_iter()
        .next()
}

pub(crate) fn settings_selection_at(area: Rect, x: u16, y: u16) -> Option<SettingsSelection> {
    let popup = centered_dialog_rect(area, SETTINGS_WIDTH, SETTINGS_HEIGHT);
    let inner = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let samples_row = Rect::new(inner.x, inner.y, inner.width, 1);
    let delta_row = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    if contains_point(samples_row, x, y) {
        Some(SettingsSelection::SamplesPanel)
    } else if contains_point(delta_row, x, y) {
        Some(SettingsSelection::Delta)
    } else {
        None
    }
}

fn contains_point(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn setting_row(
    label: &'static str,
    enabled: bool,
    selected: bool,
    theme: Theme,
) -> Paragraph<'static> {
    let style = if selected {
        Style::default()
            .fg(theme.background)
            .bg(theme.accent_alt)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    let mark = if enabled { "[x]" } else { "[ ]" };
    Paragraph::new(Line::from(vec![
        Span::styled(mark, style),
        Span::styled(" ", style),
        Span::styled(label, style),
    ]))
}
