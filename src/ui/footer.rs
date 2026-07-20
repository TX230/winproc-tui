use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    App,
    app::{AppActivity, FocusedPanel},
    ui::Theme,
};

pub(crate) fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let footer = Paragraph::new(Line::from(context_shortcuts(app, theme))).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background)),
    );
    frame.render_widget(footer, area);
}

fn context_shortcuts(app: &App, theme: Theme) -> Vec<Span<'static>> {
    let (focus_label, mut items) = match app.focused_panel {
        FocusedPanel::System => (
            "RAM/VRAM".to_string(),
            vec![("1-4", "Graph"), ("Ctrl+C", "Copy"), ("i", "System info")],
        ),
        FocusedPanel::SystemActivity => (
            "NW/DISK".to_string(),
            vec![("1-4", "Graph"), ("Ctrl+C", "Copy"), ("i", "System info")],
        ),
        FocusedPanel::Cpu => (
            "CPUs".to_string(),
            vec![("1-4", "Graph"), ("Ctrl+C", "Copy"), ("i", "System info")],
        ),
        FocusedPanel::Processes => (
            "Processes".to_string(),
            vec![
                ("c", "Columns"),
                ("s", "Sort"),
                ("g", "Graphs"),
                ("Ctrl+I", "Jump"),
                ("Shift+←/→", "Move column"),
                ("1-4", "Graph"),
                ("Enter", "Info"),
                ("Space", "Track"),
                ("d", "Kill"),
                ("Ctrl+F", "Filter"),
            ],
        ),
        FocusedPanel::DetailsGraph => (
            format!("Graph#{}", app.active_graph_slot_index + 1),
            vec![
                ("Ctrl+Left/Right", "Pan"),
                ("PgUp/PgDn", "Span"),
                ("f", "Fit"),
                ("z", "Min 0"),
                ("a/b", "Set A/B"),
            ],
        ),
        FocusedPanel::DetailsSamples => (
            format!("Samples#{}", app.active_graph_slot_index + 1),
            vec![
                ("PgUp/PgDn", "Page"),
                ("Home/End", "Edge"),
                ("a/b", "Set A/B"),
                ("x", "Clear A/B"),
            ],
        ),
    };
    if app.activity() == AppActivity::Playback {
        items.insert(0, ("Esc", "Live"));
    } else {
        items.push(("Esc", "Quit"));
    }
    items.push(("Tab", "Focus"));
    items.push(("?", "Help"));

    let mut spans = vec![Span::styled(
        focus_label,
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    if !items.is_empty() {
        spans.push(Span::raw("  "));
    }
    spans.extend(shortcut_spans(&items, theme));
    spans
}

fn shortcut_spans(items: &[(&'static str, &'static str)], theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*key, Style::default().fg(theme.accent)));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(theme.text),
            ));
        }
    }
    spans
}
