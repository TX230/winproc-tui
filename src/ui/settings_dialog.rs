use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Style,
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
const SETTINGS_HEIGHT: u16 = 9;
const OK_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 6;

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
        choice_row(
            "Tracked List startup",
            app.selected_tracked_list_startup().label(),
            app.settings_selection == SettingsSelection::TrackedListStartup,
            theme,
        ),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Left/Right or Space changes. Enter closes.",
            Style::default().fg(theme.muted),
        ))),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(button_line(&[(" OK ", true)], theme))
            .alignment(ratatui::layout::Alignment::Center),
        rows[5],
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
    let startup_row = Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1);
    if contains_point(samples_row, x, y) {
        Some(SettingsSelection::SamplesPanel)
    } else if contains_point(delta_row, x, y) {
        Some(SettingsSelection::Delta)
    } else if contains_point(startup_row, x, y) {
        Some(SettingsSelection::TrackedListStartup)
    } else {
        None
    }
}

fn choice_row(
    label: &'static str,
    value: &'static str,
    selected: bool,
    theme: Theme,
) -> Paragraph<'static> {
    let style = if selected {
        Style::default().fg(theme.text).bg(theme.focus_surface)
    } else {
        Style::default().fg(theme.text)
    };
    Paragraph::new(Line::from(vec![
        Span::styled(format!("{label:<22}"), style),
        Span::styled(format!("< {value} >"), style),
    ]))
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
        Style::default().fg(theme.text).bg(theme.focus_surface)
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
