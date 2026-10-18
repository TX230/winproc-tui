use crate::{
    App,
    ui::{Theme, footer::shortcut_spans, widgets::scrollable_modal::ScrollableModal},
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    prelude::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
};
fn modal() -> ScrollableModal {
    ScrollableModal::new("Export Graphs to CSV", 87, 8, 2)
}
pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(d) = app.graph_csv_dialog.as_ref() else {
        return;
    };
    if app.graph_entries.is_empty() {
        ScrollableModal::new("Export Graphs to CSV", 87, 1, 0).render(
            frame,
            area,
            Text::from(Span::styled(
                "Please add metrics to Graph slots.",
                Style::default().fg(theme.danger),
            )),
            0,
            false,
            theme,
        );
        return;
    }
    let layout = modal().layout(area);
    let (before, after) =
        crate::app::text_input::window(&d.path, d.cursor, layout.content.width as usize);
    let path = format!("{before}{after}");
    let active = Style::default().fg(theme.text).bg(theme.highlight);
    let normal = Style::default().fg(theme.text);
    let lines = vec![
        Line::from("Export time series from all Graph slots to one CSV file."),
        Line::from(""),
        Line::from("Destination file (full path):"),
        Line::from(Span::styled(
            path,
            if d.range_focused { normal } else { active },
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Range: {}", d.range.label()),
            if d.range_focused { active } else { normal },
        )),
        Line::from(if app.csv_ab_available() {
            ""
        } else {
            "A-B unavailable: set both endpoints first"
        }),
        Line::from(if d.busy() { "Exporting..." } else { &d.error }),
    ];
    let layout = modal().render(frame, area, Text::from(lines), 0, false, theme);
    let items = if d.busy() {
        vec![]
    } else {
        vec![
            ("Enter", "Export"),
            ("Esc", "Cancel"),
            ("Tab", "Field"),
            if d.range_focused {
                ("←/→", "Range")
            } else {
                ("Ctrl+V", "Paste")
            },
        ]
    };
    let mut footer = Text::from(Line::from(shortcut_spans(&items, theme)));
    if !d.busy() && !d.range_focused {
        footer.lines.push(Line::from(shortcut_spans(
            &[("←/→", "Cursor"), ("Ctrl+Space", "Complete path")],
            theme,
        )));
    }
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &footer,
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(Paragraph::new(footer), layout.footer);
    let cursor_y = layout.content.y.saturating_add(3);
    if !d.range_focused
        && !d.busy()
        && layout.content.width > 0
        && cursor_y < layout.content.bottom()
    {
        crate::ui::request_cursor(
            app,
            (
                layout.content.x + Line::from(before).width() as u16,
                cursor_y,
            ),
        );
    }
}
pub(crate) fn mouse(app: &mut App, event: MouseEvent, area: Rect) {
    if app.graph_entries.is_empty() {
        return;
    }
    let content = modal().layout(area).content;
    if event.kind != MouseEventKind::Down(MouseButton::Left)
        || !content.contains((event.column, event.row).into())
    {
        return;
    }
    let Some(d) = app.graph_csv_dialog.as_mut() else {
        return;
    };
    if d.busy() {
        return;
    }
    match event.row.saturating_sub(content.y) {
        3 => {
            d.range_focused = false;
            d.cursor = d.path.len();
        }
        5 => {
            d.range_focused = true;
            app.cycle_csv_range(true);
        }
        _ => {}
    }
}
