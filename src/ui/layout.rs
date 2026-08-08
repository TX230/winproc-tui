use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::{
    App,
    app::{GRAPH_SLOT_MIN_HEIGHT, GRAPH_SLOT_MIN_WIDTH, GraphSlotLayout},
};

pub(crate) const SYSTEM_PANEL_HEIGHT: u16 = 7;
pub(crate) const GRAPH_SAMPLES_TOGGLE_WIDTH: u16 = 15;
pub(crate) const GRAPH_DELTA_TOGGLE_WIDTH: u16 = 13;
pub(crate) const GRAPH_LAYOUT_TOGGLE_WIDTH: u16 = 15;
pub(crate) const GRAPH_ALL_SAMPLES_TOGGLE_WIDTH: u16 = 15;
pub(crate) const GRAPH_Y_AXIS_TOGGLE_WIDTH: u16 = 13;
pub(crate) const DETAILS_SHARED_CONTROLS_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_HEADER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT: u16 = 2;
pub(crate) const DETAILS_SAMPLES_AB_SUMMARY_HEIGHT: u16 = 3;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH: u16 = 49;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA: u16 = 32;
const DETAILS_GRAPH_MIN_WIDTH: u16 = 30;
const PROCESS_TABLE_CHROME_HEIGHT: u16 = 3;
pub(crate) const PROCESS_TABLE_MAX_HEIGHT: u16 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessTableLayout {
    pub(crate) area: Rect,
    pub(crate) page_size: usize,
    pub(crate) show_tracked_total: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MainPanelAreas {
    pub(crate) system: Rect,
    pub(crate) processes: ProcessTableLayout,
    pub(crate) details: Option<Rect>,
}

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

pub(crate) fn system_panel_area_for_screen(area: Rect) -> Rect {
    let layout = screen_layout(area);
    let sections = body_sections(layout[1]);
    sections[0]
}

pub(crate) fn main_panel_areas_for_app(area: Rect, app: &App) -> MainPanelAreas {
    main_panel_areas(
        area,
        app.show_details,
        app.visible_process_count(),
        app.has_visible_tracked_total_row(),
    )
}

pub(crate) fn main_panel_areas(
    area: Rect,
    show_details: bool,
    visible_process_rows: usize,
    has_tracked_total: bool,
) -> MainPanelAreas {
    let screen = screen_layout(area);
    let sections = body_sections(screen[1]);
    let system = sections[0];
    if show_details {
        let process_height = process_table_required_height(visible_process_rows, has_tracked_total);
        let lower = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(process_height), Constraint::Min(20)])
            .split(sections[1]);
        MainPanelAreas {
            system,
            processes: process_table_layout(lower[0], has_tracked_total),
            details: Some(lower[1]),
        }
    } else {
        MainPanelAreas {
            system,
            processes: process_table_layout(sections[1], has_tracked_total),
            details: None,
        }
    }
}

fn process_table_required_height(visible_process_rows: usize, has_tracked_total: bool) -> u16 {
    let max_rows = PROCESS_TABLE_MAX_HEIGHT.saturating_sub(PROCESS_TABLE_CHROME_HEIGHT) as usize;
    let rendered_rows = visible_process_rows
        .saturating_add(usize::from(has_tracked_total))
        .min(max_rows);
    PROCESS_TABLE_CHROME_HEIGHT.saturating_add(rendered_rows as u16)
}

fn process_table_layout(area: Rect, has_tracked_total: bool) -> ProcessTableLayout {
    let row_capacity = process_table_page_size(area);
    let show_tracked_total = has_tracked_total && row_capacity > 0;
    ProcessTableLayout {
        area,
        page_size: row_capacity.saturating_sub(usize::from(show_tracked_total)),
        show_tracked_total,
    }
}

#[cfg(test)]
pub(crate) fn details_slot_areas_for_app(
    area: Rect,
    app: &App,
    slot_count: usize,
    slot_layout: GraphSlotLayout,
) -> Vec<Rect> {
    let Some(content) = main_panel_areas_for_app(area, app).details else {
        return Vec::new();
    };
    details_slot_areas(content, slot_count, slot_layout)
}

pub(crate) fn details_slot_areas(
    area: Rect,
    slot_count: usize,
    slot_layout: GraphSlotLayout,
) -> Vec<Rect> {
    let slots_area = details_slots_content_area(area);
    if slot_count == 0 {
        return Vec::new();
    }

    let column_count = match slot_layout {
        GraphSlotLayout::Auto | GraphSlotLayout::OneColumn => 1,
        GraphSlotLayout::TwoColumns => slot_count.min(2),
    };
    let row_count = slot_count.div_ceil(column_count);
    if slots_area.height < GRAPH_SLOT_MIN_HEIGHT.saturating_mul(row_count as u16)
        || (column_count > 1
            && slots_area.width < GRAPH_SLOT_MIN_WIDTH.saturating_mul(column_count as u16))
    {
        return Vec::new();
    }

    let base_height = slots_area.height / row_count as u16;
    let extra_height = slots_area.height % row_count as u16;
    let base_width = slots_area.width / column_count as u16;
    let extra_width = slots_area.width % column_count as u16;
    (0..slot_count)
        .map(|index| {
            let row = index / column_count;
            let column = index % column_count;
            let y = slots_area.y
                + base_height.saturating_mul(row as u16)
                + extra_height.min(row as u16);
            let x = slots_area.x
                + base_width.saturating_mul(column as u16)
                + extra_width.min(column as u16);
            Rect::new(
                x,
                y,
                base_width + u16::from(column < extra_width as usize),
                base_height + u16::from(row < extra_height as usize),
            )
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

#[cfg(test)]
pub(crate) fn details_shared_controls_area_for_app(area: Rect, app: &App) -> Option<Rect> {
    main_panel_areas_for_app(area, app)
        .details
        .map(details_shared_controls_area)
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GraphSharedControlAreas {
    pub(crate) status: Rect,
    pub(crate) samples: Option<Rect>,
    pub(crate) delta: Option<Rect>,
    pub(crate) layout: Option<Rect>,
    pub(crate) all_samples: Option<Rect>,
    pub(crate) y_axis: Option<Rect>,
}

pub(crate) fn graph_shared_control_areas(
    area: Rect,
    show_samples_panel: bool,
) -> GraphSharedControlAreas {
    let mut right = area.right();
    let mut remaining = area.width;
    let mut reserve = |width: u16| {
        if remaining < width {
            return None;
        }
        right = right.saturating_sub(width);
        remaining = remaining.saturating_sub(width);
        Some(Rect::new(right, area.y, width, area.height.min(1)))
    };

    let y_axis = reserve(GRAPH_Y_AXIS_TOGGLE_WIDTH);
    let all_samples = reserve(GRAPH_ALL_SAMPLES_TOGGLE_WIDTH);
    let layout = reserve(GRAPH_LAYOUT_TOGGLE_WIDTH);
    let delta = show_samples_panel
        .then(|| reserve(GRAPH_DELTA_TOGGLE_WIDTH))
        .flatten();
    let samples = reserve(GRAPH_SAMPLES_TOGGLE_WIDTH);
    let status = Rect::new(area.x, area.y, remaining, area.height);

    GraphSharedControlAreas {
        status,
        samples,
        delta,
        layout,
        all_samples,
        y_axis,
    }
}

pub(crate) fn details_samples_max_width(show_sample_delta: bool) -> u16 {
    if show_sample_delta {
        DETAILS_SAMPLES_MAX_WIDTH
    } else {
        DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA
    }
}

#[cfg(test)]
pub(crate) fn details_graph_area_for_app(area: Rect, app: &App) -> Option<Rect> {
    let slot = details_slot_areas_for_app(area, app, 1, GraphSlotLayout::OneColumn)
        .into_iter()
        .next()?;
    Some(details_graph_area(
        slot,
        app.show_samples_panel,
        app.show_sample_delta,
    ))
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_app(area: Rect, app: &App) -> Option<Rect> {
    let slot = details_slot_areas_for_app(area, app, 1, GraphSlotLayout::OneColumn)
        .into_iter()
        .next()?;
    Some(details_samples_area(slot, app.show_sample_delta))
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

pub(crate) fn body_sections(body_area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(SYSTEM_PANEL_HEIGHT), Constraint::Min(8)])
        .split(body_area)
}

pub(crate) fn process_table_page_size(area: Rect) -> usize {
    area.height.saturating_sub(PROCESS_TABLE_CHROME_HEIGHT) as usize
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
        let screen = Rect::new(0, 0, 100, 43);
        let body = screen_layout(screen)[1];
        let sections = body_sections(body);

        assert_eq!(
            main_panel_areas(screen, false, 0, false).processes.area,
            sections[1]
        );
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
        let details = main_panel_areas(screen, true, 3, false).details.unwrap();
        let controls = details_shared_controls_area(details);
        let slot = details_slot_areas(details, 1, GraphSlotLayout::OneColumn)[0];
        let graph = details_graph_area(slot, true, true);
        let samples = details_samples_area(slot, true);

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
    fn dynamic_process_height_matches_rendered_rows_and_caps_at_existing_maximum() {
        let screen = Rect::new(0, 0, 120, 60);
        let cases = [
            (0, false, 3, 0, false),
            (1, false, 4, 1, false),
            (4, false, 7, 4, false),
            (20, false, PROCESS_TABLE_MAX_HEIGHT, 10, false),
            (0, true, 4, 0, true),
            (1, true, 5, 1, true),
            (4, true, 8, 4, true),
            (20, true, PROCESS_TABLE_MAX_HEIGHT, 9, true),
        ];

        for (visible, has_total, height, page_size, show_total) in cases {
            let panels = main_panel_areas(screen, true, visible, has_total);
            assert_eq!(panels.processes.area.height, height, "visible={visible}");
            assert_eq!(panels.processes.page_size, page_size, "visible={visible}");
            assert_eq!(
                panels.processes.show_tracked_total, show_total,
                "visible={visible}"
            );
            assert_eq!(
                panels.details.unwrap().y,
                panels.processes.area.bottom(),
                "visible={visible}"
            );
        }
    }

    #[test]
    fn hidden_graphs_keep_full_height_process_layout() {
        let screen = Rect::new(0, 0, 120, 60);
        let empty = main_panel_areas(screen, false, 0, false);
        let overflowing = main_panel_areas(screen, false, 100, true);

        assert_eq!(empty.processes.area, overflowing.processes.area);
        assert!(empty.details.is_none());
        assert!(overflowing.details.is_none());
    }

    #[test]
    fn resizing_gives_all_reclaimed_height_to_graphs() {
        let short = main_panel_areas(Rect::new(0, 0, 120, 45), true, 2, false);
        let tall = main_panel_areas(Rect::new(0, 0, 120, 60), true, 2, false);

        assert_eq!(short.processes.area.height, 5);
        assert_eq!(tall.processes.area.height, 5);
        assert_eq!(short.details.unwrap().y, tall.details.unwrap().y);
        assert_eq!(
            tall.details.unwrap().height - short.details.unwrap().height,
            15
        );
    }

    #[test]
    fn two_column_graph_layout_is_row_major_and_uses_full_width_for_one_slot() {
        let area = Rect::new(0, 0, 101, 53);

        let one = details_slot_areas(area, 1, GraphSlotLayout::TwoColumns);
        assert_eq!(one, vec![Rect::new(0, 1, 101, 52)]);

        let three = details_slot_areas(area, 3, GraphSlotLayout::TwoColumns);
        assert_eq!(
            three,
            vec![
                Rect::new(0, 1, 51, 26),
                Rect::new(51, 1, 50, 26),
                Rect::new(0, 27, 51, 26),
            ]
        );
    }

    #[test]
    fn two_column_graph_layout_rejects_insufficient_width() {
        let area = Rect::new(0, 0, GRAPH_SLOT_MIN_WIDTH * 2 - 1, 53);

        assert!(details_slot_areas(area, 2, GraphSlotLayout::TwoColumns).is_empty());
        assert!(!details_slot_areas(area, 1, GraphSlotLayout::TwoColumns).is_empty());
    }

    #[test]
    fn shared_graph_controls_use_the_same_order_with_or_without_delta() {
        let area = Rect::new(0, 0, 120, 1);
        let with_delta = graph_shared_control_areas(area, true);
        let without_delta = graph_shared_control_areas(area, false);

        assert!(with_delta.samples.unwrap().x < with_delta.delta.unwrap().x);
        assert!(with_delta.delta.unwrap().x < with_delta.layout.unwrap().x);
        assert!(with_delta.layout.unwrap().x < with_delta.all_samples.unwrap().x);
        assert!(with_delta.all_samples.unwrap().x < with_delta.y_axis.unwrap().x);
        assert!(without_delta.delta.is_none());
        assert!(without_delta.samples.unwrap().x > with_delta.samples.unwrap().x);
    }

    #[test]
    fn details_samples_width_shrinks_when_delta_is_hidden() {
        let screen = Rect::new(0, 0, 120, 45);
        let details = main_panel_areas(screen, true, 3, false).details.unwrap();
        let slot = details_slot_areas(details, 1, GraphSlotLayout::OneColumn)[0];
        let samples_with_delta = details_samples_area(slot, true);
        let samples_without_delta = details_samples_area(slot, false);

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
        let details = main_panel_areas(screen, true, 3, false).details.unwrap();
        let slot = details_slot_areas(details, 1, GraphSlotLayout::OneColumn)[0];
        let samples = details_samples_area(slot, true);

        let without_ab = details_samples_row_capacity(samples.height, false, true);
        let with_ab = details_samples_row_capacity(samples.height, true, true);
        assert_eq!(
            without_ab,
            with_ab + DETAILS_SAMPLES_AB_SUMMARY_HEIGHT as usize
        );
    }
}
