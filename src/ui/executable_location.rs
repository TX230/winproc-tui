use crate::App;

use crate::app::executable_location::LocationView;
use ratatui::{
    layout::{Margin, Rect},
    text::Line,
};
pub(crate) struct LocationLayout {
    pub(crate) area: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
    pub(crate) rows: Vec<Line<'static>>,
}
impl LocationLayout {
    pub(crate) fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(self.content.height as usize)
    }
}
pub(crate) fn layout(screen: Rect, view: &LocationView) -> LocationLayout {
    let width = screen.width.min(88);
    let height = screen.height.min(14);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let footer = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        u16::from(inner.height > 0),
    );
    let text = format!(
        "{} / PID {}\n\n{}",
        view.target.name, view.target.pid, view.message
    );
    let rows = text
        .lines()
        .flat_map(|line| {
            crate::ui::process_info_dialog::wrap_display_width(line, inner.width.max(1) as usize)
        })
        .map(Line::from)
        .collect();
    LocationLayout {
        area,
        content,
        footer,
        rows,
    }
}

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    use ratatui::{
        style::Style,
        text::{Line, Span},
        widgets::{Block, Borders, Clear, Paragraph},
    };
    let Some(view) = &app.executable_location else {
        return;
    };
    let layout = layout(frame.area(), view);
    let area = layout.area;
    let theme = app.theme();
    let color = if view.failed {
        theme.warning
    } else {
        theme.focus_border
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Open executable location")
            .border_style(Style::default().fg(color))
            .style(Style::default().bg(theme.panel_alt).fg(theme.text)),
        area,
    );
    let offset = view.offset.min(layout.max_offset());
    frame.render_widget(
        Paragraph::new(layout.rows.into_iter().skip(offset).collect::<Vec<_>>()),
        layout.content,
    );
    if !layout.footer.is_empty() {
        let footer = ratatui::text::Text::from(Line::from(vec![
            Span::styled("Enter/Esc", Style::default().fg(color).bold()),
            Span::raw(" Close  "),
            Span::styled("↑/↓", Style::default().fg(color).bold()),
            Span::raw(" Scroll"),
        ]));
        crate::ui::footer::register_shortcut_text(
            app,
            layout.footer,
            &footer,
            ratatui::layout::Alignment::Left,
        );
        frame.render_widget(Paragraph::new(footer), layout.footer);
    }
}
