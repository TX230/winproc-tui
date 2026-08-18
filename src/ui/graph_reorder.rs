use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::GraphSlot,
    ui::{Theme, footer::shortcut_spans, widgets::scrollable_modal::ScrollableModal},
};

const HEADER_ROW_COUNT: usize = 1;
const SLOT_WIDTH: usize = 4;
const METRIC_WIDTH: usize = 18;
const FOOTER_HEIGHT: u16 = 1;
const SHORTCUT_ITEMS: [(&str, &str); 4] = [
    ("↑/↓", "Select"),
    ("Shift+↑/↓", "Move"),
    ("Enter", "Apply"),
    ("Esc", "Cancel"),
];

pub(crate) fn draw_graph_reorder_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(dialog) = app.graph_reorder_dialog.as_ref() else {
        return;
    };

    let mut lines = vec![Line::from(Span::styled(
        format!(
            "  {:<SLOT_WIDTH$}  {:<METRIC_WIDTH$}  Target",
            "Slot", "Metric"
        ),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    ))];
    for (index, id) in dialog.order.iter().copied().enumerate() {
        let Some(entry) = app.graph_entry_by_id(id) else {
            continue;
        };
        let cursor = if index == dialog.selected { ">" } else { " " };
        let row = format!(
            "{cursor} {:<SLOT_WIDTH$}  {:<METRIC_WIDTH$}  {}",
            index + 1,
            entry.source.metric_label(),
            graph_target_label(&entry.source),
        );
        let style = if index == dialog.selected {
            Style::default()
                .fg(theme.text)
                .bg(theme.highlight)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        lines.push(Line::from(Span::styled(row, style)));
    }

    let layout = graph_reorder_modal(app).render(
        frame,
        area,
        Text::from(lines),
        dialog.scroll.offset,
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

pub(crate) fn graph_reorder_page_size_for_screen(area: Rect, app: &App) -> usize {
    graph_reorder_modal(app).page_size(area)
}

pub(crate) fn graph_reorder_scrollbar_area(
    area: Rect,
    app: &App,
    page_size: usize,
) -> Option<Rect> {
    graph_reorder_modal(app).scrollbar_area(area, page_size)
}

pub(crate) fn graph_reorder_index_at(area: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    let dialog = app.graph_reorder_dialog.as_ref()?;
    let content = graph_reorder_modal(app).layout(area).content;
    if x < content.x || x >= content.right() || y < content.y || y >= content.bottom() {
        return None;
    }
    let row = usize::from(y - content.y).saturating_add(dialog.scroll.offset);
    let index = row.checked_sub(HEADER_ROW_COUNT)?;
    (index < dialog.order.len()).then_some(index)
}

pub(crate) const fn graph_reorder_row_for_index(index: usize) -> usize {
    HEADER_ROW_COUNT.saturating_add(index)
}

fn graph_reorder_modal(app: &App) -> ScrollableModal {
    ScrollableModal::new(
        "REORDER GRAPHS",
        graph_reorder_content_width(app),
        app.graph_reorder_total_rows().min(usize::from(u16::MAX)) as u16,
        FOOTER_HEIGHT,
    )
    .with_bold_title()
}

fn graph_reorder_content_width(app: &App) -> u16 {
    let header_width = format!(
        "  {:<SLOT_WIDTH$}  {:<METRIC_WIDTH$}  Target",
        "Slot", "Metric"
    )
    .chars()
    .count();
    let shortcut_width = SHORTCUT_ITEMS
        .iter()
        .enumerate()
        .map(|(index, (key, label))| {
            usize::from(index > 0) * 2 + key.chars().count() + 1 + label.chars().count()
        })
        .sum::<usize>();
    let row_width = app
        .graph_reorder_dialog
        .as_ref()
        .into_iter()
        .flat_map(|dialog| dialog.order.iter().copied().enumerate())
        .filter_map(|(index, id)| {
            app.graph_entry_by_id(id).map(|entry| {
                format!(
                    "> {:<SLOT_WIDTH$}  {:<METRIC_WIDTH$}  {}",
                    index + 1,
                    entry.source.metric_label(),
                    graph_target_label(&entry.source),
                )
                .chars()
                .count()
            })
        })
        .max()
        .unwrap_or_default();
    header_width
        .max(shortcut_width)
        .max(row_width)
        .min(usize::from(u16::MAX)) as u16
}

fn graph_target_label(slot: &GraphSlot) -> String {
    match slot {
        GraphSlot::Process { identity, .. } => format!("{} (PID {})", identity.name, identity.pid),
        GraphSlot::System { metric } => metric.panel_label().to_string(),
        GraphSlot::Gpu { adapter_name, .. } => adapter_name.clone(),
    }
}
