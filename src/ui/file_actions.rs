use crate::{App, ui::Theme};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Text},
};

fn rows(app: &App, width: usize) -> Vec<(String, Option<usize>)> {
    let Some(view) = &app.file_action else {
        return vec![];
    };
    let mut rows = Vec::new();
    for (text, action) in [
        ("Ctrl+E Open containing folder".into(), Some(0)),
        ("Ctrl+O Open with default app".into(), Some(1)),
        (String::new(), None),
        (format!("Target: {}", view.path.display()), None),
        (String::new(), None),
        (view.message.clone(), None),
    ] {
        for line in super::process_info_dialog::wrap_display_width(&text, width.max(1)) {
            rows.push((line, action));
        }
    }
    rows
}
pub(crate) fn lines(app: &App, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let view = app.file_action.as_ref().unwrap();
    rows(app, width)
        .into_iter()
        .map(|(text, action)| {
            let style = if action == Some(view.selected) {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.focus_surface)
                    .bold()
            } else if view.failed && action.is_none() {
                Style::default().fg(theme.warning)
            } else {
                Style::default().fg(theme.text)
            };
            Line::styled(text, style)
        })
        .collect()
}

use super::widgets::scrollable_modal::{ScrollableModal, ScrollableModalLayout};
const MODAL: ScrollableModal = ScrollableModal::new("FILE ACTIONS", 58, 8, 1);

pub(crate) fn layout(screen: Rect) -> ScrollableModalLayout {
    MODAL.layout(screen)
}

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, screen: Rect, app: &App, theme: Theme) {
    let view = app.file_action.as_ref().unwrap();
    let area = layout(screen);
    let text = Text::from(lines(app, area.content.width as usize, theme));
    ScrollableModal::new(
        "FILE ACTIONS",
        58,
        text.lines.len().max(8).min(u16::MAX as usize) as u16,
        1,
    )
    .render(frame, area.area, text, view.scroll.offset, false, theme);
    let shortcuts = super::footer::shortcut_spans(
        if view.busy() {
            &[("Esc", "Back")]
        } else {
            &[("Enter", "Open"), ("↑/↓", "Select"), ("Esc", "Back")]
        },
        theme,
    );
    let text = Text::from(Line::from(shortcuts));
    super::footer::register_shortcut_text(
        app,
        area.footer,
        &text,
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        ratatui::widgets::Paragraph::new(text)
            .style(Style::default().fg(theme.text).bg(theme.panel_alt)),
        area.footer,
    );
}

impl App {
    pub(crate) fn scroll_file_actions(&mut self, key: crossterm::event::KeyCode) {
        let area = layout(self.last_screen_area).content;
        let total = rows(self, area.width as usize).len();
        let view = self.file_action.as_mut().unwrap();
        let page = usize::from(area.height).max(1);
        let max = total.saturating_sub(page);
        use crossterm::event::KeyCode;
        view.scroll.offset = match key {
            KeyCode::PageUp => view.scroll.offset.saturating_sub(page),
            KeyCode::PageDown => view.scroll.offset.saturating_add(page).min(max),
            KeyCode::Home => 0,
            KeyCode::End => max,
            _ => view.scroll.offset.min(max),
        };
    }

    pub(crate) fn file_action_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        let area = layout(screen).content;
        let total = rows(self, area.width as usize).len();
        let view = self.file_action.as_mut().unwrap();
        view.scroll.page_size = usize::from(area.height).max(1);
        view.scroll.offset = view
            .scroll
            .offset
            .min(total.saturating_sub(view.scroll.page_size));
        let bar = Rect::new(area.right(), area.y, 1, area.height);
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => view.scroll.stop_drag(),
            MouseEventKind::Drag(MouseButton::Left) if view.scroll.dragging => {
                view.scroll.drag_to(bar, mouse.row, total);
                return;
            }
            MouseEventKind::Down(MouseButton::Left)
                if total > view.scroll.page_size
                    && bar.contains((mouse.column, mouse.row).into()) =>
            {
                view.scroll.start_drag(bar, mouse.row, total);
                view.scroll.drag_to(bar, mouse.row, total);
                return;
            }
            _ => {}
        }
        if !area.contains((mouse.column, mouse.row).into()) {
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let row = self.file_action.as_ref().unwrap().scroll.offset.min(
                    rows(self, area.width as usize)
                        .len()
                        .saturating_sub(area.height as usize),
                ) + usize::from(mouse.row - area.y);
                if let Some((_, Some(action))) = rows(self, area.width as usize).get(row) {
                    self.launch_file_action(*action);
                }
            }
            MouseEventKind::ScrollUp => self.scroll_file_actions(crossterm::event::KeyCode::PageUp),
            MouseEventKind::ScrollDown => {
                self.scroll_file_actions(crossterm::event::KeyCode::PageDown)
            }
            _ => {}
        }
    }
}
