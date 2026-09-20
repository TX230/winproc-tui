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
    register_shortcut_text(
        app,
        Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        ),
        &ratatui::text::Text::from(Line::from(context_shortcuts(
            app,
            theme,
            area.width as usize,
        ))),
        ratatui::layout::Alignment::Left,
    );
    let footer = Paragraph::new(Line::from(context_shortcuts(
        app,
        theme,
        area.width as usize,
    )))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .title(Line::from(Span::styled(
                if app.status == "Ready" {
                    ""
                } else {
                    app.status.as_str()
                },
                Style::default().fg(theme.text),
            )))
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background)),
    );
    frame.render_widget(footer, area);
}

fn context_shortcuts(app: &App, theme: Theme, width: usize) -> Vec<Span<'static>> {
    if app.has_modal_focus() {
        return Vec::new();
    }

    if app.is_filter_editing() {
        return shortcut_spans(super::super::app::text_input::FILTER_SHORTCUTS, theme);
    }
    let mut items = match app.focused_panel {
        FocusedPanel::System => vec![
            ("←/→", "Column/Adapter"),
            ("Space", "Graph"),
            ("g", "Graphs"),
            ("Ctrl+C", "Copy"),
            ("i", "System info"),
        ],
        FocusedPanel::SystemActivity => {
            vec![
                ("Enter", "Network"),
                ("Space", "Graph"),
                ("Ctrl+C", "Copy"),
                ("i", "System info"),
            ]
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
                        "Track process name"
                    } else {
                        "Graph"
                    },
                ),
                ("t", "Track process name"),
                ("c", "Columns"),
                ("w/W", "Width"),
                ("s", "Sort"),
                ("g", "Graphs"),
                ("Ctrl+I", "Jump"),
                ("Enter", "Process Info"),
                ("f", "Files"),
                (
                    "d",
                    if app.selected_process_identities.is_empty() {
                        "Kill process"
                    } else {
                        "Kill selected"
                    },
                ),
                ("Ctrl+C", "Copy row"),
                ("Alt+↑/↓", "Focus"),
                ("Ctrl+F", "Filter"),
            ]
        }
        FocusedPanel::DetailsGraph => {
            vec![
                ("↑/↓", "Slot"),
                ("←/→", "Sample"),
                ("Shift+↑/↓", "Move"),
                ("s", "Reorder"),
                ("m", "Raw/MA5"),
                ("Del", "Remove"),
                ("a/b", "A/B range"),
                ("PgUp/PgDn", "Span"),
                ("Alt+←/→", "Pan"),
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
                ("m", "Raw/MA5"),
                ("Del", "Remove"),
                ("a/b", "A/B range"),
                ("PgUp/PgDn", "Scroll"),
                ("Home/End", "Edge"),
                ("f/z", "Fit/Min 0"),
                ("Shift+A/B", "Jump A/B"),
                ("x", "Clear A/B"),
            ]
        }
    };
    if app.focused_panel == FocusedPanel::Processes && app.watch_enabled {
        items.push(("Ctrl+A", "Select all"));
    }
    if items.first() == Some(&("Space", "Track process name")) {
        items.retain(|(key, _)| *key != "t");
    }
    let primary_key = items.first().map(|(key, _)| *key);
    if app.can_adjust_process_panel_height() {
        items.insert(0, ("h/H/Alt+H", "Height"));
    }
    if app.focused_panel == FocusedPanel::Processes && app.activity() != AppActivity::LogView {
        let view_index = items
            .iter()
            .position(|(key, _)| *key == "g")
            .unwrap_or(items.len());
        items.insert(view_index, ("v", "Flat/Tree"));
        if app.process_tree_expansion_available() {
            items.insert(view_index + 1, ("e", "Expand/Collapse"));
        }
    }
    if app.activity() != AppActivity::Live && app.focused_panel == FocusedPanel::Processes {
        let identity_column_selected = app.selected_process_column_toggles_tracking();
        items.retain(|(key, _)| *key != "t" && !(identity_column_selected && *key == "Space"));
    }
    if app.activity() == AppActivity::Recording {
        items.insert(0, ("Ctrl+R", "Stop"));
    }
    if app.activity() == AppActivity::LogView {
        items.retain(|(key, _)| *key != "v" && *key != "e" && *key != "d");
        items.insert(0, ("Ctrl+B", "Live"));
    } else {
        items.push((
            "Ctrl+P",
            if app.is_display_paused() {
                "Resume"
            } else {
                "Pause"
            },
        ));
    }
    items.insert(0, ("ESC", "Menu"));
    if app.activity() == AppActivity::Live {
        items.push(("Ctrl+S", "Save Profile"));
    }
    items.push(("Shift+T", "Tracked-only"));
    items.push(("Ctrl+T", "Profiles"));
    items.push(("F12", "Theme"));
    items.push(("F1/?", "Help"));
    items.push(("Tab", "Focus"));
    items.push(("Ctrl+↑↓←→", "Panel"));

    let mut prioritized = Vec::new();
    for key in [
        Some("ESC"),
        Some("F1/?"),
        Some("Ctrl+R"),
        app.is_display_paused().then_some("Ctrl+P"),
        Some("Tab"),
        Some("Ctrl+↑↓←→"),
        primary_key,
        Some("Ctrl+A"),
        Some("Ctrl+B"),
        (!app.selected_process_identities.is_empty()).then_some("d"),
        Some("Ctrl+F"),
        Some("Enter"),
        Some("f"),
        Some("←/→"),
        Some("a/b"),
        Some("Del"),
        Some("Ctrl+P"),
        Some("i"),
        Some("g"),
        Some("Shift+T"),
        Some("Ctrl+T"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(index) = items.iter().position(|(candidate, _)| *candidate == key) {
            prioritized.push(items.remove(index));
        }
    }
    prioritized.extend(items);
    let mut fitted = Vec::new();
    let mut used = 0;
    for item in prioritized {
        let item_width = Line::from(shortcut_spans(&[item], theme)).width();
        let separator = if fitted.is_empty() { 0 } else { 2 };
        if used + separator + item_width <= width {
            used += separator + item_width;
            fitted.push(item);
        }
    }
    shortcut_spans(&fitted, theme)
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

pub(crate) fn shortcut_action_at(
    items: &[(&str, &str)],
    area: Rect,
    x: u16,
    y: u16,
) -> Option<usize> {
    if y != area.y || area.height == 0 {
        return None;
    }
    let mut left = area.x;
    for (index, (key, label)) in items.iter().enumerate() {
        let width = Line::from(format!("{key} {label}")).width() as u16;
        let right = left.saturating_add(width);
        if right <= area.right() && x >= left && x < right {
            return Some(index);
        }
        left = right.saturating_add(2);
    }
    None
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ShortcutRegion {
    pub(crate) area: Rect,
    pub(crate) key: crossterm::event::KeyEvent,
}

#[derive(Debug, Default)]
pub(crate) struct ShortcutMap {
    pub(crate) screen: Rect,
    pub(crate) regions: Vec<ShortcutRegion>,
}

// Register exactly the complete groups that fit in the rendered shortcut line.
// Each visible layer replaces the underlying map, so modal input cannot leak through.
pub(crate) fn register_shortcut_text(
    app: &App,
    area: Rect,
    text: &ratatui::text::Text<'_>,
    alignment: ratatui::layout::Alignment,
) {
    let mut map = app.shortcut_map.borrow_mut();
    for (row, line) in text.lines.iter().take(area.height as usize).enumerate() {
        let align = line.alignment.unwrap_or(alignment);
        let padding = area.width.saturating_sub(line.width() as u16);
        let mut x = area.x
            + match align {
                ratatui::layout::Alignment::Center => {
                    (area.width / 2).saturating_sub(line.width() as u16 / 2)
                }
                ratatui::layout::Alignment::Right => padding,
                _ => 0,
            };
        for (index, span) in line.spans.iter().enumerate() {
            let width = span.width() as u16;
            if let Some(key) = shortcut_key(&span.content)
                && let Some(label) = line.spans.get(index + 1)
                && label.content.starts_with(' ')
                && !label.content.trim().is_empty()
            {
                let group_width = width.saturating_add(label.width() as u16);
                if x.saturating_add(group_width) <= area.right() {
                    map.regions.push(ShortcutRegion {
                        area: Rect::new(x, area.y + row as u16, group_width, 1),
                        key,
                    });
                }
            }
            x = x.saturating_add(width);
        }
    }
}

fn shortcut_key(label: &str) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let first = if label.trim() == "/" {
        "/"
    } else {
        label.split('/').next()?.trim()
    };
    let mut key = first;
    let mut modifiers = KeyModifiers::NONE;
    loop {
        if let Some(rest) = key.strip_prefix("Ctrl+") {
            modifiers |= KeyModifiers::CONTROL;
            key = rest;
        } else if let Some(rest) = key.strip_prefix("Shift+") {
            modifiers |= KeyModifiers::SHIFT;
            key = rest;
        } else if let Some(rest) = key.strip_prefix("Alt+") {
            modifiers |= KeyModifiers::ALT;
            key = rest;
        } else {
            break;
        }
    }
    let code = match key {
        "ESC" | "Esc" => KeyCode::Esc,
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Space" => KeyCode::Char(' '),
        "Backspace" => KeyCode::Backspace,
        "Delete" | "Del" => KeyCode::Delete,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PgUp" | "PageUp" => KeyCode::PageUp,
        "PgDn" | "PageDown" => KeyCode::PageDown,
        "↑" | "↑↓←→" => KeyCode::Up,
        "↓" => KeyCode::Down,
        "←" => KeyCode::Left,
        "→" => KeyCode::Right,
        _ if key.starts_with('F') && key.len() > 1 => KeyCode::F(key[1..].parse().ok()?),
        _ if key.chars().count() == 1 => {
            let ch = key.chars().next()?;
            KeyCode::Char(if modifiers.contains(KeyModifiers::CONTROL) {
                ch.to_ascii_lowercase()
            } else {
                ch
            })
        }
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

pub(crate) fn draw_shortcut_hover(frame: &mut ratatui::Frame<'_>, app: &App) {
    let Some(area) = app.shortcut_hovered else {
        return;
    };
    if !app
        .shortcut_map
        .borrow()
        .regions
        .iter()
        .any(|region| region.area == area)
    {
        return;
    }
    let style = Style::default()
        .bg(app.theme().focus_surface)
        .add_modifier(ratatui::style::Modifier::BOLD);
    frame.buffer_mut().set_style(area, style);
}
