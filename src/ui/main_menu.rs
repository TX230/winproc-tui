use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
};

use crate::{
    App,
    app::state::{MainMenuAction, MainMenuItem, MainMenuRow},
    ui::{Theme, widgets::scrollable_modal::ScrollableModal},
};

const MENU_KEYS: &[(&str, &str)] = &[
    ("↑/↓", "Select"),
    ("←/→", "Menu"),
    ("Enter", "Open"),
    ("Esc", "Close"),
];

pub(crate) fn draw_main_menu(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let content_width = usize::from(
        main_menu_modal(app)
            .layout(anchored_area(area, app))
            .content
            .width,
    );
    let items = app.main_menu_rows();
    let lines = display_rows(app)
        .into_iter()
        .map(|index| {
            let Some(index) = index else {
                return Line::from(Span::styled(
                    "─".repeat(content_width),
                    Style::default().fg(theme.muted),
                ));
            };
            let row = &items[index];
            let cursor = if index == app.main_menu_selected {
                "> "
            } else {
                "  "
            };
            let style = if index == app.main_menu_selected {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.table_selection_surface)
                    .add_modifier(Modifier::BOLD)
            } else if app.main_menu_hovered == Some(index) {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.focus_surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let mut line = display_label(app, *row, theme, content_width.saturating_sub(2));
            line.spans.insert(0, Span::raw(cursor));
            line.spans.push(Span::raw(
                " ".repeat(content_width.saturating_sub(line.width())),
            ));
            line.style(style)
        })
        .collect::<Vec<_>>();

    let layout = main_menu_modal(app).render(
        frame,
        anchored_area(area, app),
        Text::from(lines),
        menu_offset(area, app),
        false,
        theme,
    );
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &ratatui::text::Text::from(Line::from(super::footer::shortcut_spans(MENU_KEYS, theme))),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(super::footer::shortcut_spans(
            MENU_KEYS, theme,
        ))),
        layout.footer,
    );
}

pub(crate) fn main_menu_index_at(area: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    let content = main_menu_modal(app)
        .layout(anchored_area(area, app))
        .content;
    if x < content.x || x >= content.right() || y < content.y || y >= content.bottom() {
        return None;
    }
    let row = menu_offset(area, app) + usize::from(y - content.y);
    display_rows(app).get(row).copied().flatten()
}

pub(crate) fn main_menu_area(area: Rect, app: &App) -> Rect {
    main_menu_modal(app).layout(anchored_area(area, app)).area
}

#[cfg(test)]
pub(crate) fn main_menu_item_area(area: Rect, app: &App, index: usize) -> Option<Rect> {
    let content = main_menu_modal(app)
        .layout(anchored_area(area, app))
        .content;
    let index = display_rows(app)
        .iter()
        .position(|row| *row == Some(index))?;
    let offset = menu_offset(area, app);
    (index >= offset
        && index < display_rows(app).len()
        && index - offset < usize::from(content.height))
    .then(|| {
        Rect::new(
            content.x,
            content.y.saturating_add((index - offset) as u16),
            content.width,
            1,
        )
    })
}

fn main_menu_modal(app: &App) -> ScrollableModal {
    ScrollableModal::new(
        "",
        main_menu_content_width(app),
        display_rows(app).len().min(usize::from(u16::MAX)) as u16,
        1,
    )
    .with_top_left_placement(super::layout::HEADER_HEIGHT)
}

fn anchored_area(area: Rect, app: &App) -> Rect {
    let width = main_menu_modal(app).layout(area).area.width;
    let actions = super::header::header_actions(crate::ui::screen_layout(area)[0], app);
    let x = actions
        .iter()
        .find(|(action, _)| action.section() == Some(app.main_menu_section))
        .or_else(|| {
            actions
                .iter()
                .find(|(action, _)| *action == super::header::HeaderAction::More)
        })
        .map_or(area.x, |(_, rect)| rect.x)
        .min(area.right().saturating_sub(width));
    Rect::new(x, area.y, area.right().saturating_sub(x), area.height)
}

fn menu_offset(area: Rect, app: &App) -> usize {
    let height = main_menu_modal(app)
        .layout(anchored_area(area, app))
        .content
        .height
        .max(1) as usize;
    display_rows(app)
        .iter()
        .position(|row| *row == Some(app.main_menu_selected))
        .unwrap_or(0)
        .saturating_sub(height - 1)
}

fn main_menu_content_width(app: &App) -> u16 {
    app.main_menu_rows()
        .iter()
        .map(|row| 2 + Span::raw(app.main_menu_row_label(*row)).width() + 2 + shortcut(*row).len())
        .max()
        .unwrap_or(0)
        .max(43)
        .min(usize::from(u16::MAX)) as u16
}

fn display_rows(app: &App) -> Vec<Option<usize>> {
    let mut rows = Vec::new();
    let mut previous_group = None;
    for (index, row) in app.main_menu_rows().into_iter().enumerate() {
        use MainMenuAction::*;
        let group = match row.item {
            MainMenuItem::Header(_) => 2,
            MainMenuItem::Action(action) => match action {
                ToggleProcessTracking
                | StartRecording
                | StopRecording
                | OpenLog
                | ReturnToLive
                | OpenColumns
                | SetTheme(_) => 0,
                OpenProfiles | SaveProfile | SaveProfileAs | ExportGraphsCsv
                | ToggleTrackedOnly | ToggleShowExited | ToggleTreeView | ToggleContrast => 1,
                ToggleGraphs | ToggleSamples | ToggleDelta | QuitImmediately
                | QuitWithConfirmation | OpenStartupBehavior => 2,
                OpenTaskGuides | OpenVersionInfo | OpenKeyboardShortcuts => 0,
                CycleGraphLayout => 3,
            },
        };
        if previous_group.is_some_and(|previous| previous != group) {
            rows.push(None);
        }
        rows.push(Some(index));
        previous_group = Some(group);
    }
    rows
}

fn shortcut(row: MainMenuRow) -> &'static str {
    let MainMenuItem::Action(action) = row.item else {
        let MainMenuItem::Header(action) = row.item else {
            unreachable!()
        };
        return action.shortcut();
    };
    match action {
        MainMenuAction::OpenKeyboardShortcuts => "F1/?",
        MainMenuAction::ToggleProcessTracking => "t (Processes)",
        MainMenuAction::ExportGraphsCsv => "Ctrl+E",
        MainMenuAction::OpenProfiles => "Ctrl+T",
        MainMenuAction::SaveProfile => "Ctrl+S",
        MainMenuAction::OpenColumns => "c (Processes)",
        MainMenuAction::ToggleTrackedOnly => "Shift+T",
        MainMenuAction::ToggleTreeView => "v (Processes)",
        MainMenuAction::ToggleGraphs => "g (Processes)",
        MainMenuAction::ToggleSamples => "v (Graphs)",
        MainMenuAction::ToggleDelta => "d (Graphs)",
        MainMenuAction::CycleGraphLayout => "l (Graphs)",
        MainMenuAction::StartRecording | MainMenuAction::StopRecording => "Ctrl+R",
        MainMenuAction::ReturnToLive => "Ctrl+B",
        MainMenuAction::OpenLog => "Ctrl+L",
        MainMenuAction::QuitImmediately | MainMenuAction::QuitWithConfirmation => "q",
        _ => "",
    }
}

fn display_label(app: &App, row: MainMenuRow, theme: Theme, width: usize) -> Line<'static> {
    let key = shortcut(row);
    let label_budget = width.saturating_sub(if key.is_empty() { 0 } else { key.len() + 2 });
    let label = super::header::truncate_name(&app.main_menu_row_label(row), label_budget);
    let padding = width
        .saturating_sub(Span::raw(&label).width() + key.len())
        .max(2);
    let mut spans = vec![Span::raw(label)];
    if !key.is_empty() {
        spans.push(Span::raw(" ".repeat(padding)));
        let (key, context) = key.split_once(' ').unwrap_or((key, ""));
        spans.push(Span::styled(key, Style::default().fg(theme.key_hint)));
        if !context.is_empty() {
            spans.push(Span::raw(format!(" {context}")));
        }
    }
    Line::from(spans)
}
