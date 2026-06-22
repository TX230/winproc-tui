use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

use crate::{App, ui::Theme};

pub(crate) fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let footer = Paragraph::new(Text::from(vec![
        Line::from(global_shortcuts(app, theme)),
        Line::from(context_shortcuts(app, theme)),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background)),
    );
    frame.render_widget(footer, area);
}

fn global_shortcuts(app: &App, theme: Theme) -> Vec<Span<'static>> {
    let pause_label = if app.is_display_paused() {
        "Resume"
    } else {
        "Pause"
    };
    shortcut_spans(
        &[
            ("Tab", "Focus panel"),
            ("1-4", "Show in graph"),
            ("0", "Clear graphs"),
            ("c", "Pick columns"),
            ("t", "Tracked only"),
            ("g", "Toggle graphs"),
            ("f", "Open files"),
            ("s", "Sort rows"),
            ("Space", "Track"),
            ("Ctrl+O", "Settings"),
            ("Ctrl+P", pause_label),
        ],
        theme,
    )
}

fn context_shortcuts(app: &App, theme: Theme) -> Vec<Span<'static>> {
    let mut items = vec![
        ("Ctrl+F", "Filter"),
        ("Ctrl+I/J", "Jump"),
        ("Enter", "Process info"),
        ("i", "Info page"),
        ("Ctrl+L", "Logs"),
        ("Ctrl+R", "Record"),
        ("a/b", "Set A/B"),
        ("Shift+A/B", "Jump A/B"),
        ("x", "Clear A/B"),
        ("q", "Quit"),
        ("?", "Help"),
    ];
    if app.activity() == crate::app::AppActivity::Playback {
        items.insert(0, ("Esc", "Live"));
    }
    shortcut_spans(&items, theme)
}

fn shortcut_spans(items: &[(&'static str, &'static str)], theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(theme.text),
            ));
        }
    }
    spans
}
