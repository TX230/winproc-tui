use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::GRAPH_SLOT_MIN_HEIGHT;

pub(crate) const SYSTEM_PANEL_HEIGHT: u16 = 7;
pub(crate) const GRAPH_ALL_SAMPLES_TOGGLE_WIDTH: u16 = 17;
pub(crate) const GRAPH_Y_AXIS_TOGGLE_WIDTH: u16 = 12;
pub(crate) const DETAILS_SAMPLES_HEADER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT: u16 = 2;
pub(crate) const DETAILS_SAMPLES_AB_SUMMARY_HEIGHT: u16 = 3;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH: u16 = 49;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA: u16 = 32;
const DETAILS_GRAPH_MIN_WIDTH: u16 = 30;

pub(crate) fn screen_layout(area: Rect) -> std::rc::Rc<[Rect]> {
    std::rc::Rc::from(
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(18),
                Constraint::Length(2),
            ])
            .split(area),
    )
}

pub(crate) fn process_table_area(body_area: Rect) -> Rect {
    let sections = body_sections(body_area);
    sections[1]
}

pub(crate) fn process_table_area_for_screen(area: Rect, show_details: bool) -> Rect {
    let layout = screen_layout(area);
    if show_details {
        details_process_table_area(layout[1])
    } else {
        process_table_area(layout[1])
    }
}

pub(crate) fn system_panel_area_for_screen(area: Rect) -> Rect {
    let layout = screen_layout(area);
    let sections = body_sections(layout[1]);
    sections[0]
}

#[cfg(test)]
pub(crate) fn details_panel_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    details_slots_area_for_screen(area, show_details)
}

pub(crate) fn details_slots_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    show_details.then(|| {
        let layout = screen_layout(area);
        let sections = body_sections(layout[1]);
        let lower = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(13), Constraint::Min(20)])
            .split(sections[1]);
        lower[1]
    })
}

pub(crate) fn details_slot_areas_for_screen(
    area: Rect,
    show_details: bool,
    slot_count: usize,
) -> Vec<Rect> {
    let Some(content) = details_slots_area_for_screen(area, show_details) else {
        return Vec::new();
    };
    details_slot_areas(content, slot_count)
}

pub(crate) fn details_slot_areas(area: Rect, slot_count: usize) -> Vec<Rect> {
    if slot_count == 0 || area.height < GRAPH_SLOT_MIN_HEIGHT.saturating_mul(slot_count as u16) {
        return Vec::new();
    }
    let base_height = area.height / slot_count as u16;
    let extra = area.height % slot_count as u16;
    let mut y = area.y;
    (0..slot_count)
        .map(|index| {
            let height = base_height + u16::from(index < extra as usize);
            let rect = Rect::new(area.x, y, area.width, height);
            y = y.saturating_add(height);
            rect
        })
        .collect()
}

pub(crate) fn details_graph_area(
    area: Rect,
    show_samples_panel: bool,
    show_sample_delta: bool,
) -> Rect {
    if show_samples_panel {
        details_graph_samples_areas(area, show_sample_delta).0
    } else {
        area
    }
}

pub(crate) fn details_samples_area(area: Rect, show_sample_delta: bool) -> Rect {
    details_graph_samples_areas(area, show_sample_delta).1
}

pub(crate) fn details_graph_samples_areas(area: Rect, show_sample_delta: bool) -> (Rect, Rect) {
    if area.width == 0 {
        return (area, area);
    }

    let samples_max_width = details_samples_max_width(show_sample_delta);
    let samples_width = if area.width > DETAILS_GRAPH_MIN_WIDTH {
        samples_max_width.min(area.width - DETAILS_GRAPH_MIN_WIDTH)
    } else {
        area.width.saturating_mul(30) / 100
    };
    let graph_width = area.width.saturating_sub(samples_width);
    (
        Rect::new(area.x, area.y, graph_width, area.height),
        Rect::new(
            area.x.saturating_add(graph_width),
            area.y,
            samples_width,
            area.height,
        ),
    )
}

pub(crate) fn details_samples_max_width(show_sample_delta: bool) -> u16 {
    if show_sample_delta {
        DETAILS_SAMPLES_MAX_WIDTH
    } else {
        DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA
    }
}

#[cfg(test)]
pub(crate) fn details_samples_page_size_for_screen(
    area: Rect,
    show_details: bool,
    show_ab_summary: bool,
) -> usize {
    let Some(samples) = details_samples_area_for_screen(area, show_details) else {
        return 1;
    };
    let inner = samples.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    details_samples_row_capacity(inner.height, show_ab_summary, true)
}

#[cfg(test)]
pub(crate) fn details_graph_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    let content = details_content_area_for_screen(area, show_details)?;
    Some(details_graph_samples_areas(content, true).0)
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    let content = details_content_area_for_screen(area, show_details)?;
    Some(details_graph_samples_areas(content, true).1)
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_screen_with_delta(
    area: Rect,
    show_details: bool,
    show_sample_delta: bool,
) -> Option<Rect> {
    let content = details_content_area_for_screen(area, show_details)?;
    Some(details_graph_samples_areas(content, show_sample_delta).1)
}

#[cfg(test)]
fn details_content_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    details_panel_area_for_screen(area, show_details)
}

pub(crate) fn details_samples_row_capacity(
    inner_height: u16,
    show_ab_summary: bool,
    show_base_summary: bool,
) -> usize {
    inner_height
        .saturating_sub(DETAILS_SAMPLES_HEADER_HEIGHT)
        .saturating_sub(DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT)
        .saturating_sub(details_samples_summary_height(
            show_ab_summary,
            show_base_summary,
        ))
        .max(1) as usize
}

pub(crate) fn details_samples_summary_height(
    show_ab_summary: bool,
    show_base_summary: bool,
) -> u16 {
    let base = if show_base_summary {
        DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT
    } else {
        0
    };
    let ab = if show_ab_summary {
        DETAILS_SAMPLES_AB_SUMMARY_HEIGHT
    } else {
        0
    };
    base + ab
}

pub(crate) fn details_process_table_area(body_area: Rect) -> Rect {
    let sections = body_sections(body_area);
    let lower = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(13), Constraint::Min(20)])
        .split(sections[1]);
    lower[0]
}

pub(crate) fn body_sections(body_area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(SYSTEM_PANEL_HEIGHT), Constraint::Min(8)])
        .split(body_area)
}

pub(crate) fn process_table_page_size(area: Rect) -> usize {
    area.height.saturating_sub(3).max(1) as usize
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_reserves_one_content_row() {
        let layout = screen_layout(Rect::new(0, 0, 100, 45));

        assert_eq!(layout[2].height, 2);
    }

    #[test]
    fn process_table_area_matches_body_sections_without_details() {
        let body = Rect::new(0, 1, 100, 40);
        let sections = body_sections(body);

        assert_eq!(process_table_area(body), sections[1]);
    }

    #[test]
    fn system_panel_height_removes_empty_rows() {
        let body = Rect::new(0, 1, 100, 40);
        let sections = body_sections(body);

        assert_eq!(sections[0].height, SYSTEM_PANEL_HEIGHT);
    }

    #[test]
    fn details_graph_and_samples_use_full_details_area() {
        let screen = Rect::new(0, 0, 100, 45);
        let details = details_panel_area_for_screen(screen, true).unwrap();
        let graph = details_graph_area_for_screen(screen, true).unwrap();
        let samples = details_samples_area_for_screen(screen, true).unwrap();

        assert_eq!(graph.x, details.x);
        assert_eq!(graph.y, details.y);
        assert_eq!(graph.height, details.height);
        assert_eq!(samples.y, details.y);
        assert_eq!(samples.height, details.height);
        assert_eq!(samples.right(), details.right());
        assert!(samples.width <= DETAILS_SAMPLES_MAX_WIDTH);
    }

    #[test]
    fn details_samples_width_shrinks_when_delta_is_hidden() {
        let screen = Rect::new(0, 0, 120, 45);
        let samples_with_delta = details_samples_area_for_screen_with_delta(screen, true, true)
            .expect("samples area with delta");
        let samples_without_delta = details_samples_area_for_screen_with_delta(screen, true, false)
            .expect("samples area without delta");

        assert_eq!(samples_with_delta.width, DETAILS_SAMPLES_MAX_WIDTH);
        assert_eq!(
            samples_without_delta.width,
            DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA
        );
        assert!(samples_without_delta.width < samples_with_delta.width);
    }

    #[test]
    fn details_samples_page_size_uses_summary_reservation() {
        let screen = Rect::new(0, 0, 100, 45);
        let samples = details_samples_area_for_screen(screen, true).unwrap();
        let inner = samples.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        assert_eq!(
            details_samples_page_size_for_screen(screen, true, false),
            details_samples_row_capacity(inner.height, false, true)
        );
        assert_eq!(
            details_samples_page_size_for_screen(screen, true, false),
            details_samples_page_size_for_screen(screen, true, true) + 3
        );
    }
}
