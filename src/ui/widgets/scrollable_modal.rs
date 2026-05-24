use ratatui::{
    layout::{Alignment, Rect},
    prelude::Style,
    text::Text,
    widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::ui::{Theme, widgets::block::panel_block_focused};

const HORIZONTAL_CONTENT_PADDING: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollableModal {
    pub(crate) title: &'static str,
    pub(crate) content_width: u16,
    pub(crate) content_height: u16,
    pub(crate) footer_height: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollableModalLayout {
    pub(crate) area: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScrollableModalState {
    pub(crate) offset: usize,
    pub(crate) page_size: usize,
    pub(crate) dragging: bool,
    pub(crate) grab_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollbarThumb {
    start: usize,
    len: usize,
}

impl ScrollableModal {
    pub(crate) const fn new(
        title: &'static str,
        content_width: u16,
        content_height: u16,
        footer_height: u16,
    ) -> Self {
        Self {
            title,
            content_width,
            content_height,
            footer_height,
        }
    }

    pub(crate) fn layout(self, area: Rect) -> ScrollableModalLayout {
        let width = self
            .content_width
            .saturating_add(HORIZONTAL_CONTENT_PADDING.saturating_mul(2))
            .saturating_add(2)
            .min(area.width);
        let height = self
            .content_height
            .saturating_add(self.footer_height)
            .saturating_add(2)
            .min(area.height);
        let popup = Rect::new(
            area.x.saturating_add(area.width.saturating_sub(width) / 2),
            area.y
                .saturating_add(area.height.saturating_sub(height) / 2),
            width,
            height,
        );
        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        let content_width = inner
            .width
            .saturating_sub(HORIZONTAL_CONTENT_PADDING.saturating_mul(2));
        let content = Rect::new(
            inner.x.saturating_add(HORIZONTAL_CONTENT_PADDING),
            inner.y,
            content_width,
            inner.height.saturating_sub(self.footer_height),
        );
        let footer = Rect::new(
            inner.x,
            content.bottom(),
            inner.width,
            inner.height.saturating_sub(content.height),
        );
        ScrollableModalLayout {
            area: popup,
            content,
            footer,
        }
    }

    pub(crate) fn area(self, area: Rect) -> Rect {
        self.layout(area).area
    }

    pub(crate) fn page_size(self, area: Rect) -> usize {
        self.layout(area).content.height.max(1) as usize
    }

    pub(crate) fn max_offset_for_page_size(self, page_size: usize) -> usize {
        (self.content_height as usize).saturating_sub(page_size.max(1))
    }

    pub(crate) fn scrollbar_area(self, area: Rect, page_size: usize) -> Option<Rect> {
        let layout = self.layout(area);
        let rows = page_size.max(1);
        if self.content_height as usize <= rows || layout.content.is_empty() {
            return None;
        }
        Some(Rect::new(
            layout
                .content
                .right()
                .min(layout.area.right().saturating_sub(2)),
            layout.content.y,
            1,
            layout.content.height,
        ))
    }

    pub(crate) fn render(
        self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        text: Text<'_>,
        offset: usize,
        wrap: bool,
        theme: Theme,
    ) -> ScrollableModalLayout {
        let layout = self.layout(area);
        let rows = layout.content.height.max(1) as usize;
        let offset = offset.min(text.lines.len().saturating_sub(rows));
        frame.render_widget(Clear, layout.area);
        frame.render_widget(panel_block_focused(self.title, theme, true), layout.area);

        let mut paragraph = Paragraph::new(text)
            .alignment(Alignment::Left)
            .style(Style::default().fg(theme.text).bg(theme.panel))
            .scroll((offset as u16, 0));
        if wrap {
            paragraph = paragraph.wrap(ratatui::widgets::Wrap { trim: true });
        }
        frame.render_widget(paragraph, layout.content);
        if let Some(scrollbar) = self.scrollbar_area(area, rows) {
            self.render_scrollbar(frame, scrollbar, rows, offset, theme);
        }
        layout
    }

    fn render_scrollbar(
        self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        rows: usize,
        offset: usize,
        theme: Theme,
    ) {
        let total = self.content_height as usize;
        if total <= rows {
            return;
        }

        let mut state = ScrollbarState::new(total)
            .position(scrollbar_position(total, rows, offset))
            .viewport_content_length(rows);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_symbol("█")
            .track_symbol(Some("│"))
            .style(Style::default().fg(theme.muted).bg(theme.panel))
            .thumb_style(Style::default().fg(theme.accent).bg(theme.panel));
        frame.render_stateful_widget(scrollbar, area, &mut state);
    }
}

impl ScrollableModalState {
    pub(crate) fn reset(&mut self) {
        self.offset = 0;
        self.dragging = false;
        self.grab_offset = 0;
    }

    pub(crate) fn set_page_size(&mut self, page_size: usize, total: usize) {
        self.page_size = page_size.max(1);
        self.clamp(total);
    }

    pub(crate) fn scroll_up(&mut self, amount: usize) {
        self.offset = self.offset.saturating_sub(amount);
    }

    pub(crate) fn scroll_down(&mut self, amount: usize, total: usize) {
        self.offset = self.offset.saturating_add(amount);
        self.clamp(total);
    }

    pub(crate) fn scroll_home(&mut self) {
        self.offset = 0;
    }

    pub(crate) fn scroll_end(&mut self, total: usize) {
        self.offset = self.max_offset(total);
    }

    pub(crate) fn ensure_visible(&mut self, row: usize, total: usize) {
        let rows = self.page_size.max(1);
        if row < self.offset {
            self.offset = row;
        } else if row >= self.offset.saturating_add(rows) {
            self.offset = row.saturating_add(1).saturating_sub(rows);
        }
        self.clamp(total);
    }

    pub(crate) fn start_drag(&mut self, area: Rect, y: u16, total: usize) {
        self.dragging = true;
        self.grab_offset =
            scrollbar_grab_offset_at(area, y, total, self.page_size, self.offset).unwrap_or(0);
    }

    pub(crate) fn drag_to(&mut self, area: Rect, y: u16, total: usize) {
        if let Some(offset) = scrollbar_offset_at(area, y, total, self.page_size, self.grab_offset)
        {
            self.offset = offset;
            self.clamp(total);
        }
    }

    pub(crate) fn stop_drag(&mut self) {
        self.dragging = false;
        self.grab_offset = 0;
    }

    pub(crate) fn max_offset(self, total: usize) -> usize {
        total.saturating_sub(self.page_size.max(1))
    }

    fn clamp(&mut self, total: usize) {
        self.offset = self.offset.min(self.max_offset(total));
    }
}

fn scrollbar_position(total: usize, rows: usize, offset: usize) -> usize {
    let max_offset = total.saturating_sub(rows);
    if max_offset == 0 {
        return 0;
    }
    let max_scrollbar_position = total.saturating_sub(1);
    (offset.min(max_offset) * max_scrollbar_position + max_offset / 2) / max_offset
}

fn scrollbar_track_len(area: Rect) -> Option<usize> {
    let track_len = area.height.saturating_sub(2);
    (track_len > 0).then_some(usize::from(track_len))
}

fn scrollbar_track_position(area: Rect, y: u16) -> Option<usize> {
    let track_len = scrollbar_track_len(area)?;
    let track_end = area.y.saturating_add(area.height).saturating_sub(2);
    if y <= area.y {
        return Some(0);
    }
    if y >= track_end {
        return Some(track_len.saturating_sub(1));
    }
    Some(usize::from(y - area.y - 1).min(track_len.saturating_sub(1)))
}

fn scrollbar_thumb(
    total: usize,
    rows: usize,
    offset: usize,
    track_len: usize,
) -> Option<ScrollbarThumb> {
    if total == 0 {
        return None;
    }
    let rows = rows.max(1).min(total);
    if total <= rows || track_len == 0 {
        return None;
    }

    let max_offset = total.saturating_sub(rows);
    if max_offset == 0 {
        return None;
    }
    let thumb_len = ((rows * track_len + total / 2) / total)
        .max(1)
        .min(track_len);
    let max_thumb_start = track_len.saturating_sub(thumb_len);
    let thumb_start = ((offset.min(max_offset) * max_thumb_start + max_offset / 2) / max_offset)
        .min(max_thumb_start);
    Some(ScrollbarThumb {
        start: thumb_start,
        len: thumb_len,
    })
}

fn scrollbar_grab_offset_at(
    area: Rect,
    y: u16,
    total: usize,
    rows: usize,
    offset: usize,
) -> Option<usize> {
    let track_len = scrollbar_track_len(area)?;
    let position = scrollbar_track_position(area, y)?;
    let thumb = scrollbar_thumb(total, rows, offset, track_len)?;
    let thumb_end = thumb.start.saturating_add(thumb.len);
    if position >= thumb.start && position < thumb_end {
        Some(position - thumb.start)
    } else {
        Some(thumb.len / 2)
    }
}

fn scrollbar_offset_at(
    area: Rect,
    y: u16,
    total: usize,
    rows: usize,
    grab_offset: usize,
) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let rows = rows.max(1).min(total);
    if total <= rows {
        return None;
    }

    let track_len = scrollbar_track_len(area)?;
    let position = scrollbar_track_position(area, y)?;
    let max_offset = total.saturating_sub(rows);
    let thumb_len = ((rows * track_len + total / 2) / total)
        .max(1)
        .min(track_len);
    let max_thumb_start = track_len.saturating_sub(thumb_len);
    if max_thumb_start == 0 {
        return Some(0);
    }
    let thumb_start = position.saturating_sub(grab_offset);
    Some(
        ((thumb_start.min(max_thumb_start) * max_offset + max_thumb_start / 2) / max_thumb_start)
            .min(max_offset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_drag_maps_scrollbar_track_to_offsets() {
        let area = Rect::new(10, 5, 1, 12);
        let mut state = ScrollableModalState {
            page_size: 10,
            ..ScrollableModalState::default()
        };

        state.start_drag(area, 5, 100);
        state.drag_to(area, 10, 100);

        assert_eq!(state.offset, 40);

        state.drag_to(area, 16, 100);
        assert_eq!(state.offset, 90);
    }
}
