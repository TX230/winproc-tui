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
    ScrollableModal::new("Export Graphs to CSV", 72, 7, 1)
}
pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(d) = app.graph_csv_dialog.as_ref() else {
        return;
    };
    let layout = modal().layout(area);
    let (before, after) =
        crate::app::text_input::window(&d.path, d.cursor, layout.content.width as usize);
    let path = if d.range_focused {
        d.path.clone()
    } else {
        format!("{before}_{after}")
    };
    let active = Style::default().fg(theme.text).bg(theme.highlight);
    let normal = Style::default().fg(theme.text);
    let lines = vec![
        Line::from("Destination (new file):"),
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
    let footer = Line::from(shortcut_spans(&items, theme));
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &Text::from(footer.clone()),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(Paragraph::new(footer), layout.footer);
}
pub(crate) fn mouse(app: &mut App, event: MouseEvent, area: Rect) {
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
        1 => {
            d.range_focused = false;
            d.cursor = d.path.len();
        }
        3 => {
            d.range_focused = true;
            app.cycle_csv_range(true);
        }
        _ => {}
    }
}
