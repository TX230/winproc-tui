use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    model::MetricColumn,
    ui::{Theme, footer::shortcut_spans, widgets::scrollable_modal::ScrollableModal},
};

const HEADER_TITLE: &str = "Select process columns";
const SHORTCUT_ITEMS: [(&str, &str); 3] = [
    ("↑/↓", "select"),
    ("Space", "toggle"),
    ("Enter/Esc", "close"),
];
const HEADER_AND_GAP_LINE_COUNT: u16 = 2;
const LABEL_WIDTH: usize = 10;
const FOOTER_HEIGHT: u16 = 1;

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
    if !layout.footer.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(&SHORTCUT_ITEMS, theme))),
            layout.footer,
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
    [HEADER_TITLE.chars().count(), column_picker_shortcut_width()]
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
        "COLUMNS",
        column_picker_content_width(),
        column_picker_content_height(),
        FOOTER_HEIGHT,
    )
    .with_bold_title()
}
