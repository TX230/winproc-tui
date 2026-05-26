use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{App, app::AppActivity, ui::Theme};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub(crate) fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let mut spans = vec![
        Span::styled(
            format!(" winproc-tui {} ", env!("CARGO_PKG_VERSION")),
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];

    match app.activity() {
        AppActivity::Live => spans.push(mode_span("LIVE", theme.accent, theme)),
        AppActivity::Recording => {
            spans.push(mode_span("REC", theme.danger, theme));
            if !app.is_display_paused() {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    SPINNER[app.recording_spinner_index % SPINNER.len()].to_string(),
                    Style::default()
                        .fg(theme.danger)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if let Some(path) = app.active_log_path() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    truncate_middle(&path.display().to_string(), area.width.saturating_sub(22)),
                    Style::default().fg(theme.warning),
                ));
            }
        }
        AppActivity::Playback => {
            spans.push(mode_span("PLAY", theme.warning, theme));
            if let Some(path) = app.active_log_path() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    truncate_middle(&path.display().to_string(), area.width.saturating_sub(23)),
                    Style::default().fg(theme.text),
                ));
            }
        }
    }
    if app.is_display_paused() {
        spans.push(Span::raw("  "));
        spans.push(mode_span("PAUSED", theme.warning, theme));
    }

    let header = Line::from(spans);

    let header_widget = Paragraph::new(header)
        .style(Style::default().bg(theme.panel))
        .alignment(Alignment::Left);
    frame.render_widget(header_widget, area);
}

fn mode_span(label: &'static str, color: ratatui::prelude::Color, theme: Theme) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(theme.background)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn truncate_middle(value: &str, max_width: u16) -> String {
    let max_width = max_width as usize;
    let char_count = value.chars().count();
    if char_count <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let tail_len = (max_width / 2).max(1);
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
