use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::GRAPH_SLOT_MIN_HEIGHT;

pub(crate) const SYSTEM_PANEL_HEIGHT: u16 = 7;
pub(crate) const GRAPH_ALL_SAMPLES_TOGGLE_WIDTH: u16 = 17;
pub(crate) const GRAPH_Y_AXIS_TOGGLE_WIDTH: u16 = 12;
pub(crate) const DETAILS_SHARED_CONTROLS_HEIGHT: u16 = 1;
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
    let slots_area = details_slots_content_area(area);
    if slot_count == 0
        || slots_area.height < GRAPH_SLOT_MIN_HEIGHT.saturating_mul(slot_count as u16)
    {
        return Vec::new();
    }
    let base_height = slots_area.height / slot_count as u16;
    let extra = slots_area.height % slot_count as u16;
    let mut y = slots_area.y;
    (0..slot_count)
        .map(|index| {
            let height = base_height + u16::from(index < extra as usize);
            let rect = Rect::new(slots_area.x, y, slots_area.width, height);
            y = y.saturating_add(height);
            rect
        })
        .collect()
}

pub(crate) fn details_shared_controls_area(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y,
        area.width,
        DETAILS_SHARED_CONTROLS_HEIGHT.min(area.height),
    )
}

pub(crate) fn details_shared_controls_area_for_screen(
    area: Rect,
    show_details: bool,
) -> Option<Rect> {
    details_slots_area_for_screen(area, show_details).map(details_shared_controls_area)
}

fn details_slots_content_area(area: Rect) -> Rect {
    let controls_height = DETAILS_SHARED_CONTROLS_HEIGHT.min(area.height);
    Rect::new(
        area.x,
        area.y.saturating_add(controls_height),
        area.width,
        area.height.saturating_sub(controls_height),
    )
}

pub(crate) fn details_graph_area(
    area: Rect,
    show_samples_panel: bool,
    show_sample_delta: bool,
) -> Rect {
    let content = details_slot_content_area(area);
    if show_samples_panel {
        details_graph_samples_areas(content, show_sample_delta).0
    } else {
        content
    }
}

pub(crate) fn details_samples_area(area: Rect, show_sample_delta: bool) -> Rect {
    details_graph_samples_areas(details_slot_content_area(area), show_sample_delta).1
}

pub(crate) fn details_slot_content_area(area: Rect) -> Rect {
    area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    })
}

pub(crate) fn details_slot_title_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        u16::from(area.height > 0),
    )
}

pub(crate) fn details_graph_samples_areas(area: Rect, show_sample_delta: bool) -> (Rect, Rect) {
    if area.width == 0 {
        return (area, area);
    }

    let samples_max_width = details_samples_max_width(show_sample_delta);
    let divider_width = u16::from(area.width > 0);
    let available_width = area.width.saturating_sub(divider_width);
    let samples_width = if available_width > DETAILS_GRAPH_MIN_WIDTH {
        samples_max_width.min(available_width - DETAILS_GRAPH_MIN_WIDTH)
    } else {
        available_width.saturating_mul(30) / 100
    };
    let graph_width = available_width.saturating_sub(samples_width);
    (
        Rect::new(area.x, area.y, graph_width, area.height),
        Rect::new(
            area.x
                .saturating_add(graph_width)
                .saturating_add(divider_width),
            area.y,
            samples_width,
            area.height,
        ),
    )
}

pub(crate) fn details_samples_divider_area(samples_area: Rect) -> Option<Rect> {
    (samples_area.x > 0 && samples_area.height > 0).then(|| {
        Rect::new(
            samples_area.x.saturating_sub(1),
            samples_area.y,
            1,
            samples_area.height,
        )
    })
}

pub(crate) fn details_graph_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area)
}

pub(crate) fn details_graph_chart_area(area: Rect, left_padding: u16) -> Option<Rect> {
    let rows = details_graph_rows(area);
    let chart = *rows.get(1)?;
    let x_padding = left_padding.min(chart.width.saturating_sub(1));
    Some(Rect::new(
        chart.x.saturating_add(x_padding),
        chart.y.saturating_add(1),
        chart.width.saturating_sub(x_padding),
        chart.height.saturating_sub(1),
    ))
}

pub(crate) fn graph_shared_status_area(area: Rect) -> Rect {
    let reserved = GRAPH_ALL_SAMPLES_TOGGLE_WIDTH.saturating_add(GRAPH_Y_AXIS_TOGGLE_WIDTH);
    Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(reserved.min(area.width)),
        area.height,
    )
}

pub(crate) fn graph_y_axis_toggle_area(area: Rect) -> Option<Rect> {
    (area.width >= GRAPH_Y_AXIS_TOGGLE_WIDTH).then(|| {
        Rect::new(
            area.right().saturating_sub(GRAPH_Y_AXIS_TOGGLE_WIDTH),
            area.y,
            GRAPH_Y_AXIS_TOGGLE_WIDTH,
            area.height.min(1),
        )
    })
}

pub(crate) fn graph_all_samples_toggle_area(area: Rect) -> Option<Rect> {
    let required = GRAPH_ALL_SAMPLES_TOGGLE_WIDTH.saturating_add(GRAPH_Y_AXIS_TOGGLE_WIDTH);
    (area.width >= required).then(|| {
        Rect::new(
            area.right().saturating_sub(required),
            area.y,
            GRAPH_ALL_SAMPLES_TOGGLE_WIDTH,
            area.height.min(1),
        )
    })
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
    details_samples_row_capacity(samples.height, show_ab_summary, true)
}

#[cfg(test)]
pub(crate) fn details_graph_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    let slot = details_slot_areas_for_screen(area, show_details, 1)
        .into_iter()
        .next()?;
    Some(details_graph_area(slot, true, true))
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_screen(area: Rect, show_details: bool) -> Option<Rect> {
    let slot = details_slot_areas_for_screen(area, show_details, 1)
        .into_iter()
        .next()?;
    Some(details_samples_area(slot, true))
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_screen_with_delta(
    area: Rect,
    show_details: bool,
    show_sample_delta: bool,
) -> Option<Rect> {
    let slot = details_slot_areas_for_screen(area, show_details, 1)
        .into_iter()
        .next()?;
    Some(details_samples_area(slot, show_sample_delta))
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
    fn details_layout_reserves_shared_controls_and_one_slot_frame() {
        let screen = Rect::new(0, 0, 100, 45);
        let details = details_panel_area_for_screen(screen, true).unwrap();
        let controls = details_shared_controls_area_for_screen(screen, true).unwrap();
        let slot = details_slot_areas_for_screen(screen, true, 1)[0];
        let graph = details_graph_area_for_screen(screen, true).unwrap();
        let samples = details_samples_area_for_screen(screen, true).unwrap();

        assert_eq!(controls, Rect::new(details.x, details.y, details.width, 1));
        assert_eq!(slot.y, details.y + 1);
        assert_eq!(graph.x, slot.x + 1);
        assert_eq!(graph.y, slot.y + 1);
        assert_eq!(graph.height, slot.height - 2);
        assert_eq!(samples.y, graph.y);
        assert_eq!(samples.height, graph.height);
        assert_eq!(samples.right(), slot.right() - 1);
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
    fn details_samples_page_size_uses_shared_slot_content_height() {
        let screen = Rect::new(0, 0, 100, 45);
        let samples = details_samples_area_for_screen(screen, true).unwrap();

        assert_eq!(
            details_samples_page_size_for_screen(screen, true, false),
            details_samples_row_capacity(samples.height, false, true)
        );
        assert_eq!(
            details_samples_page_size_for_screen(screen, true, false),
            details_samples_page_size_for_screen(screen, true, true) + 3
        );
    }
}
