use ratatui::{
    layout::Rect,
    prelude::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    App,
    app::{AppActivity, FocusedPanel, TrackedListsButton, TrackedListsView},
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
    if let Some(view) = app.tracked_lists_view() {
        let items = match view {
            TrackedListsView::Browse if app.tracked_lists_save_name_focused() => {
                vec![("Enter", "Save"), ("Tab", "Next"), ("Esc", "Close")]
            }
            TrackedListsView::Browse if app.tracked_lists_startup_focused() => {
                vec![("Left/Right", "Startup"), ("Tab", "Next"), ("Esc", "Close")]
            }
            TrackedListsView::Browse => match app.tracked_lists_focused_button() {
                Some(TrackedListsButton::Save) => {
                    vec![("Enter", "Save"), ("Tab", "Next"), ("Esc", "Close")]
                }
                Some(TrackedListsButton::Close) => {
                    vec![("Enter", "Close"), ("Tab", "Next"), ("Esc", "Close")]
                }
                None if app.tracked_lists_empty_selected() => {
                    vec![
                        ("Enter/Click", "Load Empty"),
                        ("Tab", "Next"),
                        ("Esc", "Close"),
                    ]
                }
                None => vec![
                    ("Enter", "Load"),
                    ("F2", "Rename"),
                    ("Del", "Delete"),
                    ("Tab", "Next"),
                    ("Esc", "Close"),
                ],
            },
            TrackedListsView::NameInput { .. } => {
                vec![("Enter", "Rename"), ("Esc", "Cancel")]
            }
            TrackedListsView::ConfirmDelete { .. } => {
                vec![("Y/Enter", "Confirm"), ("N/Esc", "Cancel")]
            }
            TrackedListsView::ConfirmSwitch { .. } => {
                vec![("Y/Enter", "Load"), ("N/Esc", "Cancel")]
            }
        };
        return shortcut_spans(&items, theme);
    }

    let mut items = match app.focused_panel {
        FocusedPanel::System | FocusedPanel::SystemActivity | FocusedPanel::Cpu => {
            vec![("1-4", "Graph"), ("Ctrl+C", "Copy"), ("i", "System info")]
        }
        FocusedPanel::Processes => {
            vec![
                ("Ctrl+T", "Lists"),
                ("c", "Columns"),
                ("w/W", "Width"),
                ("s", "Sort"),
                ("g", "Graphs"),
                ("Ctrl+I", "Jump"),
                ("1-4", "Graph"),
                ("Enter/f", "Info/Files"),
                ("Space", "Track"),
                ("d", "Kill"),
                ("Ctrl+F", "Filter"),
            ]
        }
        FocusedPanel::DetailsGraph => {
            vec![
                ("Enter", "Info"),
                ("Ctrl+Left/Right", "Pan"),
                ("PgUp/PgDn", "Span"),
                ("f", "Fit"),
                ("z", "Min 0"),
                ("a/b", "Set A/B"),
                ("Shift+A/B", "Jump A/B"),
            ]
        }
        FocusedPanel::DetailsSamples => {
            vec![
                ("PgUp/PgDn", "Page"),
                ("Home/End", "Edge"),
                ("f", "Fit"),
                ("z", "Min 0"),
                ("a/b", "Set A/B"),
                ("Shift+A/B", "Jump A/B"),
                ("x", "Clear A/B"),
            ]
        }
    };
    if app.activity() == AppActivity::LogView {
        items.insert(0, ("Esc", "Live"));
    } else {
        items.insert(0, ("Ctrl+P", "Pause"));
        items.push(("Esc", "Quit"));
    }
    items.insert(0, ("?", "Help"));

    shortcut_spans(&items, theme)
}

fn shortcut_spans(items: &[(&'static str, &'static str)], theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*key, Style::default().fg(theme.muted)));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(theme.text),
            ));
        }
    }
    spans
}
