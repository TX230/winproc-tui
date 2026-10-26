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
const DOTNET_NOTICE: &str = concat!(
    ".NET 8/9/10 runtime bug: collecting metrics during app startup may cause the app to hang. ",
    "Selecting .NET 8/9/10 columns here does not start collection. ",
    "Select a process and press n to start or stop collection. ",
    ".NET Framework metrics are collected automatically.",
);

fn dotnet_notice_lines(width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in DOTNET_NOTICE.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > usize::from(width.max(1)) {
            lines.push(Line::from(std::mem::take(&mut line)));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(Line::from(line));
    lines
}

fn shortcut_area(footer: Rect) -> Rect {
    Rect::new(
        footer.x,
        footer.bottom().saturating_sub(1),
        footer.width,
        footer.height.min(1),
    )
}

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

    let layout = column_picker_modal(area).render(
        frame,
        area,
        Text::from(lines),
        app.column_picker_scroll.offset,
        false,
        theme,
    );
    if !layout.footer.is_empty() {
        let notice = Rect::new(
            layout.footer.x,
            layout.footer.y,
            layout.footer.width,
            layout.footer.height.saturating_sub(2),
        );
        frame.render_widget(
            Paragraph::new(Text::from(dotnet_notice_lines(notice.width)))
                .style(Style::default().fg(theme.warning).bg(theme.panel_alt)),
            notice,
        );
        let shortcuts = shortcut_area(layout.footer);
        crate::ui::footer::register_shortcut_text(
            app,
            shortcuts,
            &ratatui::text::Text::from(Line::from(shortcut_spans(&SHORTCUT_ITEMS, theme))),
            ratatui::layout::Alignment::Left,
        );
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(&SHORTCUT_ITEMS, theme))),
            shortcuts,
        );
    }
}

#[cfg(test)]
pub(crate) fn column_picker_area(area: Rect) -> Rect {
    column_picker_modal(area).area(area)
}

pub(crate) fn column_picker_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll_offset: usize,
) -> Option<usize> {
    let content = column_picker_modal(area).layout(area).content;
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
    column_picker_modal(area).page_size(area)
}

pub(crate) fn column_picker_scroll_max_for_page_size(page_size: usize) -> usize {
    usize::from(column_picker_content_height()).saturating_sub(page_size.max(1))
}

pub(crate) fn column_picker_scrollbar_area(area: Rect, page_size: usize) -> Option<Rect> {
    column_picker_modal(area).scrollbar_area(area, page_size)
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

fn column_picker_modal(area: Rect) -> ScrollableModal {
    let footer_width = column_picker_content_width()
        .saturating_add(4)
        .min(area.width)
        .saturating_sub(2);
    let footer_height = (dotnet_notice_lines(footer_width).len() as u16 + 2)
        .min(area.height.saturating_sub(7).max(1));
    ScrollableModal::new(
        "COLUMNS",
        column_picker_content_width(),
        column_picker_content_height(),
        footer_height,
    )
}

pub(crate) fn close_action_at(screen: Rect, x: u16, y: u16) -> bool {
    super::footer::shortcut_action_at(
        &SHORTCUT_ITEMS,
        shortcut_area(column_picker_modal(screen).layout(screen).footer),
        x,
        y,
    ) == Some(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotnet_notice_wraps_to_available_width_without_forced_breaks() {
        for width in [40, 63, 78, 98] {
            let lines = dotnet_notice_lines(width)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            assert_eq!(lines.join(" "), DOTNET_NOTICE);
            for line in &lines {
                assert!(line.len() <= usize::from(width));
            }
            for pair in lines.windows(2) {
                let next_word = pair[1].split_whitespace().next().unwrap();
                assert!(pair[0].len() + 1 + next_word.len() > usize::from(width));
            }
        }
    }
}
