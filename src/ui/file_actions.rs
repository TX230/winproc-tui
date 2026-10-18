use crate::{App, ui::Theme};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, style::Style, text::Line};

fn rows(app: &App, width: usize) -> Vec<(String, Option<usize>)> {
    let Some(view) = &app.file_action else {
        return vec![];
    };
    let mut rows = Vec::new();
    for (text, action) in [
        ("File actions".into(), None),
        ("Ctrl+E Open containing folder".into(), Some(0)),
        ("Ctrl+O Open with default app".into(), Some(1)),
        (String::new(), None),
        (format!("Target: {}", view.path.display()), None),
        ("Opens the file currently at this path.".into(), None),
        ("Folders open directly.".into(), None),
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
impl App {
    pub(crate) fn file_action_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        if !area.contains((mouse.column, mouse.row).into()) {
            return;
        }
        if mouse.column == area.right().saturating_sub(1) {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.start_process_info_scrollbar_drag(
                        mouse.column,
                        mouse.row,
                        self.last_screen_area,
                    );
                }
                MouseEventKind::Drag(MouseButton::Left)
                    if self.process_info_scrollbar_dragging() =>
                {
                    self.drag_process_info_scrollbar(mouse.row, self.last_screen_area)
                }
                MouseEventKind::Up(MouseButton::Left) => self.stop_process_info_scrollbar_drag(),
                _ => {}
            }
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let row = self.open_files_scroll.offset + usize::from(mouse.row - area.y);
                if let Some((_, Some(action))) =
                    rows(self, area.width.saturating_sub(1) as usize).get(row)
                {
                    self.launch_file_action(*action);
                }
            }
            MouseEventKind::ScrollUp => self.scroll_process_info_up(1),
            MouseEventKind::ScrollDown => self.scroll_process_info_down(1),
            _ => {}
        }
    }
}
