use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    model::MetricColumn,
    ui::{Theme, widgets::scrollable_modal::ScrollableModal},
};

const HEADER_TITLE: &str = "Select process columns";
const CLOSE_BUTTON: &str = "[ Close ]";
const SHORTCUT_ITEMS: [(&str, &str); 3] = [
    ("Up/Down", "move"),
    ("Space", "toggle"),
    ("Enter/Esc", "close"),
];
const HEADER_AND_GAP_LINE_COUNT: u16 = 3;
const LABEL_WIDTH: usize = 10;
const CLOSE_AREA_HEIGHT: u16 = 3;

pub(crate) fn draw_column_picker(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let mut lines = vec![
        Line::from(Span::styled(
            HEADER_TITLE,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(column_picker_shortcut_spans(theme)),
        Line::from(""),
    ];

    for (index, column) in MetricColumn::ALL.iter().enumerate() {
        let selected = app.process_columns.contains(column);
        let cursor = if index == app.column_picker_index {
            ">"
        } else {
            " "
        };
        let mark = if selected { "[x]" } else { "[ ]" };
        let style = if index == app.column_picker_index {
            Style::default()
                .fg(theme.text)
                .bg(theme.highlight)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(theme.text)
        } else {
            Style::default().fg(theme.muted)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{cursor} {mark} {:<width$}",
                    column.label(),
                    width = LABEL_WIDTH
                ),
                style,
            ),
            Span::styled(" / ", Style::default().fg(theme.muted)),
            Span::styled(
                column.description(),
                description_style(index, app, selected, theme),
            ),
        ]));
    }

    let layout = column_picker_modal().render(
        frame,
        area,
        Text::from(lines),
        app.column_picker_scroll.offset,
        false,
        theme,
    );
    if let Some(area) = column_picker_close_button_area_in_footer(layout.footer) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                CLOSE_BUTTON,
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            area,
        );
    }
}

#[cfg(test)]
pub(crate) fn column_picker_area(area: Rect) -> Rect {
    column_picker_modal().area(area)
}

pub(crate) fn column_picker_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll_offset: usize,
) -> Option<usize> {
    let content = column_picker_modal().layout(area).content;
    if x < content.x || x >= content.right() || y < content.y || y >= content.bottom() {
        return None;
    }

    let content_row = usize::from(y - content.y).saturating_add(scroll_offset);
    let header_rows = usize::from(HEADER_AND_GAP_LINE_COUNT);
    if content_row < header_rows {
        return None;
    }
    let index = content_row - header_rows;
    (index < MetricColumn::ALL.len()).then_some(index)
}

pub(crate) fn column_picker_close_button_area_for_screen(area: Rect) -> Option<Rect> {
    column_picker_close_button_area(column_picker_modal().area(area))
}

pub(crate) fn column_picker_close_button_area(popup: Rect) -> Option<Rect> {
    if popup.width < 11 || popup.height < 4 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        popup.x + popup.width.saturating_sub(width) / 2,
        popup.bottom().saturating_sub(3),
        width,
        1,
    ))
}

pub(crate) fn column_picker_page_size_for_screen(area: Rect) -> usize {
    column_picker_modal().page_size(area)
}

pub(crate) fn column_picker_scroll_max_for_page_size(page_size: usize) -> usize {
    column_picker_modal().max_offset_for_page_size(page_size)
}

pub(crate) fn column_picker_scrollbar_area(area: Rect, page_size: usize) -> Option<Rect> {
    column_picker_modal().scrollbar_area(area, page_size)
}

fn column_picker_content_width() -> u16 {
    [
        HEADER_TITLE.chars().count(),
        column_picker_shortcut_width(),
        CLOSE_BUTTON.chars().count(),
    ]
    .into_iter()
    .chain(
        MetricColumn::ALL
            .iter()
            .map(|column| column_picker_row_width(*column)),
    )
    .max()
    .unwrap_or_default() as u16
}

fn column_picker_content_height() -> u16 {
    HEADER_AND_GAP_LINE_COUNT.saturating_add(MetricColumn::ALL.len() as u16)
}

fn column_picker_row_width(column: MetricColumn) -> usize {
    format!(
        "> [x] {:<width$} / {}",
        column.label(),
        column.description(),
        width = LABEL_WIDTH
    )
    .chars()
    .count()
}

fn column_picker_shortcut_spans(theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, label)) in SHORTCUT_ITEMS.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text),
        ));
    }
    spans
}

fn column_picker_shortcut_width() -> usize {
    SHORTCUT_ITEMS
        .iter()
        .enumerate()
        .map(|(index, (key, label))| {
            let separator_width = if index > 0 { 2 } else { 0 };
            key.chars().count() + label.chars().count() + 1 + separator_width
        })
        .sum()
}

fn column_picker_close_button_area_in_footer(footer: Rect) -> Option<Rect> {
    if footer.width < 11 || footer.height == 0 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        footer.x + footer.width.saturating_sub(width) / 2,
        footer.y.saturating_add(1),
        width,
        1,
    ))
}

fn description_style(index: usize, app: &App, selected: bool, theme: Theme) -> Style {
    if index == app.column_picker_index {
        Style::default()
            .fg(theme.text)
            .bg(theme.highlight)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.exited)
    }
}

pub(crate) fn column_picker_row_for_index(index: usize) -> usize {
    usize::from(HEADER_AND_GAP_LINE_COUNT).saturating_add(index)
}

fn column_picker_modal() -> ScrollableModal {
    ScrollableModal::new(
        "Columns",
        column_picker_content_width(),
        column_picker_content_height(),
        CLOSE_AREA_HEIGHT,
    )
}
