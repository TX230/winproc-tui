use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    App,
    app::{AppActivity, SampleFreshness},
    ui::Theme,
};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub(crate) fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let mut spans = Vec::new();

    let activity = app.activity();
    match activity {
        AppActivity::Live => spans.push(mode_span("LIVE", theme.success, theme)),
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
        }
        AppActivity::LogView => {
            spans.push(mode_span("LOG", theme.warning, theme));
        }
    }

    if let Some(SampleFreshness::Stale { age_seconds }) = app.sample_freshness() {
        spans.push(stale_span(age_seconds, theme));
    }
    if app.is_display_paused() && activity != AppActivity::LogView {
        spans.push(Span::raw("  "));
        spans.push(mode_span("DISPLAY PAUSED", theme.warning, theme));
    }
    if let Some(path) = app.active_log_path() {
        append_log_path(
            &mut spans,
            area,
            &path.display().to_string(),
            if activity == AppActivity::Recording {
                theme.warning
            } else {
                theme.text
            },
        );
    }

    let header = Line::from(spans);

    let header_widget = Paragraph::new(header)
        .style(Style::default().bg(theme.panel))
        .alignment(Alignment::Left);
    frame.render_widget(header_widget, area);
}

fn stale_span(age_seconds: u64, theme: Theme) -> Span<'static> {
    Span::styled(
        format!(" · STALE {age_seconds}s"),
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
    )
}

fn append_log_path(
    spans: &mut Vec<Span<'static>>,
    area: Rect,
    path: &str,
    color: ratatui::prelude::Color,
) {
    let used_width = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    let path_width = usize::from(area.width).saturating_sub(used_width.saturating_add(2));
    if path_width == 0 {
        return;
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        truncate_middle(path, path_width.min(usize::from(u16::MAX)) as u16),
        Style::default().fg(color),
    ));
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
