use ratatui::{
    layout::Rect,
    prelude::Style,
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
    if app.has_modal_focus() {
        return Vec::new();
    }

    let mut items = match app.focused_panel {
        FocusedPanel::System => vec![
            ("m/g", "MEM/GPU"),
            ("←/→", "Column/Adapter"),
            ("Space", "Graph"),
            ("Ctrl+C", "Copy"),
            ("i", "System info"),
        ],
        FocusedPanel::SystemActivity => {
            vec![("Space", "Graph"), ("Ctrl+C", "Copy"), ("i", "System info")]
        }
        FocusedPanel::Cpu if app.cpu_per_core_selected() => {
            vec![("↑/↓", "Item"), ("Enter", "Open"), ("i", "System info")]
        }
        FocusedPanel::Cpu => vec![
            ("↑/↓", "Item"),
            ("Space", "Graph"),
            ("Ctrl+C", "Copy"),
            ("i", "System info"),
        ],
        FocusedPanel::Processes => {
            vec![
                (
                    "Space",
                    if app.selected_process_column_toggles_tracking() {
                        "Track"
                    } else {
                        "Graph"
                    },
                ),
                ("t", "Track"),
                ("Shift+T", "Tracked-only"),
                ("Ctrl+T", "Lists"),
                ("c", "Columns"),
                ("w/W", "Width"),
                ("s", "Sort"),
                ("g", "Graphs"),
                ("Ctrl+I", "Jump"),
                ("Enter/f", "Info/Files"),
                ("d", "Kill"),
                ("Ctrl+F", "Filter"),
            ]
        }
        FocusedPanel::DetailsGraph => {
            vec![
                ("↑/↓", "Slot"),
                ("←/→", "Sample"),
                ("Shift+↑/↓", "Move"),
                ("s", "Reorder"),
                ("Del", "Remove Graph"),
                ("a/b", "Set A/B"),
                ("PgUp/PgDn", "Span"),
                ("Ctrl+←/→", "Pan"),
                ("Enter", "Info"),
                ("f/z", "Fit/Min 0"),
                ("Shift+A/B", "Jump A/B"),
            ]
        }
        FocusedPanel::DetailsSamples => {
            vec![
                ("↑/←", "Older"),
                ("↓/→", "Newer"),
                ("Shift+↑/↓", "Move"),
                ("s", "Reorder"),
                ("Del", "Remove Graph"),
                ("a/b", "Set A/B"),
                ("PgUp/PgDn", "Scroll"),
                ("Home/End", "Edge"),
                ("f/z", "Fit/Min 0"),
                ("Shift+A/B", "Jump A/B"),
                ("x", "Clear A/B"),
            ]
        }
    };
    if app.activity() == AppActivity::Recording {
        if app.focused_panel == FocusedPanel::Processes {
            let identity_column_selected = app.selected_process_column_toggles_tracking();
            items.retain(|(key, _)| {
                *key != "t" && *key != "Ctrl+T" && !(identity_column_selected && *key == "Space")
            });
        }
        items.insert(0, ("Ctrl+R", "Stop"));
    }
    if app.activity() == AppActivity::LogView {
        items.push(("Esc", "Live"));
    } else {
        items.push(("Ctrl+P", "Pause"));
        items.push(("Esc", "Quit"));
    }
    items.push(("F1/?", "Help"));

    shortcut_spans(&items, theme)
}

pub(crate) fn shortcut_spans(
    items: &[(&'static str, &'static str)],
    theme: Theme,
) -> Vec<Span<'static>> {
    shortcut_spans_with_key_style(items, Style::default().fg(theme.key_hint), theme)
}

pub(crate) fn warning_shortcut_spans(
    items: &[(&'static str, &'static str)],
    theme: Theme,
) -> Vec<Span<'static>> {
    shortcut_spans_with_key_style(
        items,
        Style::default()
            .fg(theme.warning)
            .add_modifier(ratatui::style::Modifier::BOLD),
        theme,
    )
}

fn shortcut_spans_with_key_style(
    items: &[(&'static str, &'static str)],
    key_style: Style,
    theme: Theme,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*key, key_style));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(theme.text),
            ));
        }
    }
    spans
}
