use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    text::Line,
};

use crate::{
    App,
    app::{
        AbComparison, GRAPH_SLOT_MIN_HEIGHT, GRAPH_SLOT_MIN_WIDTH, GraphId, GraphSlotLayout,
        ProcessPanelHeight,
    },
};

pub(crate) const HEADER_HEIGHT: u16 = 1;

pub(crate) const SYSTEM_PANEL_HEIGHT: u16 = 7;
pub(crate) const GRAPH_SAMPLES_TOGGLE_WIDTH: u16 = 16;
pub(crate) const GRAPH_DELTA_TOGGLE_WIDTH: u16 = 14;
pub(crate) const GRAPH_LAYOUT_TOGGLE_WIDTH: u16 = 15;
pub(crate) const GRAPH_ALL_SAMPLES_TOGGLE_WIDTH: u16 = 16;
pub(crate) const GRAPH_Y_AXIS_TOGGLE_WIDTH: u16 = 14;
pub(crate) const DETAILS_SHARED_CONTROLS_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_HEADER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT: u16 = 1;
pub(crate) const DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT: u16 = 2;
pub(crate) const DETAILS_SAMPLES_AB_SUMMARY_HEIGHT: u16 = 3;
pub(crate) const DETAILS_SAMPLES_AB_RANGE_SUMMARY_HEIGHT: u16 = 4;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH: u16 = 50;
pub(crate) const DETAILS_SAMPLES_MAX_WIDTH_NO_DELTA: u16 = 33;
const DETAILS_SAMPLES_MIN_WIDTH: u16 = 30;
const DETAILS_SAMPLES_MIN_HEIGHT: u16 = 8;
const DETAILS_SAMPLES_FRAME_HEIGHT: u16 = 2;
const DETAILS_SAMPLES_MIN_ROW_HEIGHT: u16 = 1;
const GRAPH_WORKSPACE_INSET_SIZE: u16 = 2;
const GRAPH_WORKSPACE_MIN_HEIGHT: u16 = 10;
const PROCESS_TABLE_CHROME_HEIGHT: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessTableLayout {
    pub(crate) area: Rect,
    pub(crate) page_size: usize,
    pub(crate) body_capacity: usize,
    pub(crate) show_tracked_total: bool,
    pub(crate) resize_handle: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MainPanelAreas {
    pub(crate) system: Rect,
    pub(crate) processes: ProcessTableLayout,
    pub(crate) details: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SamplesPlacement {
    Right,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphCardLayout {
    pub(crate) id: GraphId,
    pub(crate) ordinal: usize,
    pub(crate) area: Rect,
    pub(crate) title: Rect,
    pub(crate) display_mode: Rect,
    pub(crate) remove: Rect,
    pub(crate) remove_label: &'static str,
    pub(crate) plot: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphWorkspaceLayout {
    pub(crate) controls: Rect,
    pub(crate) graph_slots: Rect,
    pub(crate) span_controls: GraphSpanControlAreas,
    pub(crate) graph_viewport: Rect,
    pub(crate) graph_cards: Vec<GraphCardLayout>,
    pub(crate) graph_scrollbar: Option<Rect>,
    pub(crate) samples: Option<Rect>,
    pub(crate) samples_placement: Option<SamplesPlacement>,
    pub(crate) columns: usize,
    pub(crate) total_rows: usize,
    pub(crate) visible_rows: usize,
    pub(crate) max_scroll_row: usize,
    pub(crate) compact: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GraphSpanControlAreas {
    pub(crate) zoom_out: Option<Rect>,
    pub(crate) zoom_in: Option<Rect>,
}

pub(crate) fn screen_layout(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Min(18),
            Constraint::Length(2),
        ])
        .split(area)
}

pub(crate) fn system_panel_area_for_screen(area: Rect) -> Rect {
    let layout = screen_layout(area);
    let sections = body_sections(layout[1]);
    sections[0]
}

pub(crate) fn main_panel_areas_for_app(area: Rect, app: &App) -> MainPanelAreas {
    main_panel_areas_with_height(
        area,
        app.show_details,
        app.visible_process_count(),
        app.has_visible_tracked_total_row(),
        app.process_panel_height,
    )
}

#[cfg(test)]
pub(crate) fn main_panel_areas(
    area: Rect,
    show_details: bool,
    visible_process_rows: usize,
    has_tracked_total: bool,
) -> MainPanelAreas {
    main_panel_areas_with_height(
        area,
        show_details,
        visible_process_rows,
        has_tracked_total,
        ProcessPanelHeight::Auto,
    )
}

pub(crate) fn main_panel_areas_with_height(
    area: Rect,
    show_details: bool,
    visible_process_rows: usize,
    has_tracked_total: bool,
    process_panel_height: ProcessPanelHeight,
) -> MainPanelAreas {
    let screen = screen_layout(area);
    let sections = body_sections(screen[1]);
    let system = sections[0];
    if show_details {
        let lower_area = sections[1];
        let preferred_process_height = process_table_required_height(
            visible_process_rows,
            has_tracked_total,
            process_panel_height,
            lower_area.height,
        );
        let available_for_process = lower_area.height.saturating_sub(GRAPH_WORKSPACE_MIN_HEIGHT);
        let process_height = preferred_process_height
            .min(available_for_process)
            .min(lower_area.height);
        let processes_area =
            Rect::new(lower_area.x, lower_area.y, lower_area.width, process_height);
        let details_area = Rect::new(
            lower_area.x,
            lower_area.y.saturating_add(process_height),
            lower_area.width,
            lower_area.height.saturating_sub(process_height),
        );
        MainPanelAreas {
            system,
            processes: process_table_layout(
                processes_area,
                has_tracked_total,
                process_resize_handle(processes_area, details_area),
            ),
            details: Some(details_area),
        }
    } else {
        MainPanelAreas {
            system,
            processes: process_table_layout(sections[1], has_tracked_total, None),
            details: None,
        }
    }
}

fn process_table_required_height(
    visible_process_rows: usize,
    has_tracked_total: bool,
    preference: ProcessPanelHeight,
    available_height: u16,
) -> u16 {
    let content_rows = visible_process_rows.saturating_add(usize::from(has_tracked_total));
    let preferred_rows = match preference {
        ProcessPanelHeight::Auto => {
            let usable = available_height.saturating_sub(PROCESS_TABLE_CHROME_HEIGHT);
            let readable_graph = GRAPH_SLOT_MIN_HEIGHT
                .saturating_add(GRAPH_WORKSPACE_INSET_SIZE)
                .saturating_add(DETAILS_SHARED_CONTROLS_HEIGHT);
            usize::from(
                (usable.saturating_mul(2) / 5)
                    .min(usable.saturating_sub(readable_graph))
                    .max(1),
            )
        }
        ProcessPanelHeight::Manual(rows) => usize::from(rows.max(1)),
    };
    let rendered_rows = content_rows.min(preferred_rows);
    PROCESS_TABLE_CHROME_HEIGHT.saturating_add(rendered_rows as u16)
}

fn process_table_layout(
    area: Rect,
    has_tracked_total: bool,
    resize_handle: Option<Rect>,
) -> ProcessTableLayout {
    let row_capacity = process_table_page_size(area);
    let show_tracked_total = has_tracked_total && row_capacity > 0;
    ProcessTableLayout {
        area,
        page_size: row_capacity.saturating_sub(usize::from(show_tracked_total)),
        body_capacity: row_capacity,
        show_tracked_total,
        resize_handle,
    }
}

fn process_resize_handle(processes: Rect, details: Rect) -> Option<Rect> {
    (!processes.is_empty() && !details.is_empty()).then(|| {
        Rect::new(
            processes.x,
            processes.bottom().saturating_sub(1),
            processes.width,
            1,
        )
    })
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

pub(crate) fn graph_workspace_layout(area: Rect, app: &App) -> GraphWorkspaceLayout {
    let controls = details_shared_controls_area(area);
    let content_y = area.y.saturating_add(controls.height);
    let content = Rect::new(
        area.x,
        content_y,
        area.width,
        area.bottom().saturating_sub(content_y),
    );
    let (graph_slots, samples, samples_placement) = graph_workspace_content_areas(content, app);
    let span_controls = graph_span_control_areas(graph_slots, app);
    let graph_viewport = graph_slots.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    let entry_count = app.graph_entries.len();
    let mut columns =
        effective_graph_columns(app.graph_slot_layout, entry_count, graph_viewport.width);
    let mut total_rows = entry_count.div_ceil(columns.max(1));
    let compact = entry_count > 0 && graph_viewport.height < GRAPH_SLOT_MIN_HEIGHT;
    let row_capacity = if entry_count == 0 {
        0
    } else if compact {
        1
    } else {
        usize::from(graph_viewport.height / GRAPH_SLOT_MIN_HEIGHT).max(1)
    };
    let mut visible_rows = total_rows.min(row_capacity);
    let mut max_scroll_row = total_rows.saturating_sub(visible_rows);
    if max_scroll_row > 0 {
        columns = columns
            .min(usize::from(graph_viewport.width.saturating_sub(1) / GRAPH_SLOT_MIN_WIDTH).max(1));
        total_rows = entry_count.div_ceil(columns);
        visible_rows = total_rows.min(row_capacity);
        max_scroll_row = total_rows.saturating_sub(visible_rows);
    }
    let scroll_row = app.graph_scroll_row.min(max_scroll_row);
    let graph_scrollbar = (max_scroll_row > 0 && graph_viewport.height > 0).then(|| {
        Rect::new(
            graph_viewport.right().saturating_sub(1),
            graph_viewport.y,
            1,
            graph_viewport.height,
        )
    });
    let cards_area = if graph_scrollbar.is_some() {
        Rect::new(
            graph_viewport.x,
            graph_viewport.y,
            graph_viewport.width.saturating_sub(1),
            graph_viewport.height,
        )
    } else {
        graph_viewport
    };
    let graph_cards =
        graph_card_layouts(cards_area, app, columns, scroll_row, visible_rows, compact);

    GraphWorkspaceLayout {
        controls,
        graph_slots,
        span_controls,
        graph_viewport,
        graph_cards,
        graph_scrollbar,
        samples,
        samples_placement,
        columns,
        total_rows,
        visible_rows,
        max_scroll_row,
        compact,
    }
}

pub(crate) fn graph_workspace_title_label(app: &App) -> String {
    let count = app.graph_entries.len();
    let slot_label = if count == 1 { "Slot" } else { "Slots" };
    format!(
        "GRAPHS · {count} {slot_label} · Span {}s · {}",
        app.effective_graph_time_span_seconds(),
        app.sample_follow_label()
    )
}

fn graph_span_control_areas(area: Rect, app: &App) -> GraphSpanControlAreas {
    const TITLE_INSET: u16 = 1;
    const TITLE_BUTTON_GAP: u16 = 2;
    const BUTTON_WIDTH: u16 = 3;
    const BETWEEN_BUTTONS: u16 = 1;

    if area.height == 0 {
        return GraphSpanControlAreas::default();
    }
    let title_width = (Line::from(graph_workspace_title_label(app)).width()
        + crate::ui::widgets::block::PANEL_NAME_PADDING_WIDTH) as u16;
    let zoom_out_x = area
        .x
        .saturating_add(TITLE_INSET)
        .saturating_add(title_width)
        .saturating_add(TITLE_BUTTON_GAP);
    let zoom_in_x = zoom_out_x
        .saturating_add(BUTTON_WIDTH)
        .saturating_add(BETWEEN_BUTTONS);
    if zoom_in_x.saturating_add(BUTTON_WIDTH) > area.right().saturating_sub(1) {
        return GraphSpanControlAreas::default();
    }
    GraphSpanControlAreas {
        zoom_out: Some(Rect::new(zoom_out_x, area.y, BUTTON_WIDTH, 1)),
        zoom_in: Some(Rect::new(zoom_in_x, area.y, BUTTON_WIDTH, 1)),
    }
}

fn graph_workspace_content_areas(
    content: Rect,
    app: &App,
) -> (Rect, Option<Rect>, Option<SamplesPlacement>) {
    if !app.show_samples_panel || app.graph_entries.is_empty() || content.is_empty() {
        return (content, None, None);
    }

    let samples_max_width = details_samples_max_width(app.show_sample_delta);
    let summary_visibility = details_samples_summary_visibility(app.active_ab_comparison());
    let samples_min_height = details_samples_min_height(summary_visibility);
    let divider_width = 1;
    let available_width = content.width.saturating_sub(divider_width);
    // A narrow single active card still has room for exact cursor values and axes.
    let card_min_width = if content.width < GRAPH_SLOT_MIN_WIDTH + DETAILS_SAMPLES_MIN_WIDTH + 3 {
        40
    } else {
        GRAPH_SLOT_MIN_WIDTH
    };
    let graph_slots_min_width = card_min_width.saturating_add(GRAPH_WORKSPACE_INSET_SIZE);
    let samples_width =
        samples_max_width.min(available_width.saturating_sub(graph_slots_min_width));
    if content.height >= samples_min_height && samples_width >= DETAILS_SAMPLES_MIN_WIDTH {
        let graph_width = available_width.saturating_sub(samples_width);
        return (
            Rect::new(content.x, content.y, graph_width, content.height),
            Some(Rect::new(
                content
                    .x
                    .saturating_add(graph_width)
                    .saturating_add(divider_width),
                content.y,
                samples_width,
                content.height,
            )),
            Some(SamplesPlacement::Right),
        );
    }

    let graph_slots_min_height = GRAPH_SLOT_MIN_HEIGHT.saturating_add(GRAPH_WORKSPACE_INSET_SIZE);
    if content.width >= DETAILS_SAMPLES_MIN_WIDTH
        && content.height >= graph_slots_min_height.saturating_add(samples_min_height)
    {
        let samples_height = samples_min_height
            .max(content.height / 3)
            .min(content.height.saturating_sub(graph_slots_min_height));
        let graph_height = content.height.saturating_sub(samples_height);
        return (
            Rect::new(content.x, content.y, content.width, graph_height),
            Some(Rect::new(
                content.x,
                content.y.saturating_add(graph_height),
                content.width,
                samples_height,
            )),
            Some(SamplesPlacement::Bottom),
        );
    }

    (content, None, None)
}

pub(crate) fn effective_graph_columns(
    layout: GraphSlotLayout,
    entry_count: usize,
    width: u16,
) -> usize {
    if entry_count <= 1 {
        return 1;
    }
    let requested = match layout {
        GraphSlotLayout::OneColumn => 1,
        GraphSlotLayout::TwoColumns => 2,
        GraphSlotLayout::Auto | GraphSlotLayout::ThreeColumns => 3,
    };
    requested
        .min(entry_count)
        .min(usize::from(width / GRAPH_SLOT_MIN_WIDTH).max(1))
}

fn graph_card_layouts(
    area: Rect,
    app: &App,
    columns: usize,
    scroll_row: usize,
    visible_rows: usize,
    compact: bool,
) -> Vec<GraphCardLayout> {
    if app.graph_entries.is_empty() || area.is_empty() || visible_rows == 0 {
        return Vec::new();
    }
    if compact {
        let index = app.active_graph_index().unwrap_or(0);
        return app
            .graph_entries
            .get(index)
            .map(|entry| graph_card_layout(area, entry.id, index))
            .into_iter()
            .collect();
    }

    let row_rects = split_evenly(area, visible_rows, true);
    let mut cards = Vec::new();
    for (visible_row, row_area) in row_rects.into_iter().enumerate() {
        let row = scroll_row + visible_row;
        let column_rects = split_evenly(row_area, columns, false);
        for (column, card_area) in column_rects.into_iter().enumerate() {
            let index = row.saturating_mul(columns).saturating_add(column);
            let Some(entry) = app.graph_entries.get(index) else {
                continue;
            };
            cards.push(graph_card_layout(card_area, entry.id, index));
        }
    }
    cards
}

fn graph_card_layout(area: Rect, id: GraphId, ordinal: usize) -> GraphCardLayout {
    let title = details_slot_title_area(area);
    let remove_label = "[x]";
    let remove_width = 5.min(title.width);
    let remove = Rect::new(
        title.right().saturating_sub(remove_width),
        title.y,
        remove_width,
        title.height,
    );
    let display_mode_width = 7.min(remove.x.saturating_sub(title.x));
    let display_mode = Rect::new(
        remove.x.saturating_sub(display_mode_width),
        title.y,
        display_mode_width,
        title.height,
    );
    GraphCardLayout {
        id,
        ordinal,
        area,
        title,
        display_mode,
        remove,
        remove_label,
        plot: area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        }),
    }
}

fn split_evenly(area: Rect, count: usize, vertical: bool) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let total = if vertical { area.height } else { area.width };
    let base = total / count as u16;
    let extra = total % count as u16;
    (0..count)
        .map(|index| {
            let index_u16 = index as u16;
            let offset = base
                .saturating_mul(index_u16)
                .saturating_add(extra.min(index_u16));
            let length = base.saturating_add(u16::from(index_u16 < extra));
            if vertical {
                Rect::new(area.x, area.y.saturating_add(offset), area.width, length)
            } else {
                Rect::new(area.x.saturating_add(offset), area.y, length, area.height)
            }
        })
        .collect()
}

pub(crate) fn details_slot_title_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        u16::from(area.height > 0),
    )
}

pub(crate) fn details_graph_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(if area.height < 10 { 3 } else { 8 }),
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
    GraphSharedControlAreas {
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
    let details = main_panel_areas_for_app(area, app).details?;
    let layout = graph_workspace_layout(details, app);
    let active_id = app.active_graph_id?;
    layout
        .graph_cards
        .into_iter()
        .find(|card| card.id == active_id)
        .map(|card| card.plot)
}

#[cfg(test)]
pub(crate) fn details_samples_area_for_app(area: Rect, app: &App) -> Option<Rect> {
    let details = main_panel_areas_for_app(area, app).details?;
    graph_workspace_layout(details, app).samples.map(|samples| {
        samples.inner(Margin {
            horizontal: 1,
            vertical: 1,
        })
    })
}

pub(crate) fn details_samples_row_capacity(
    inner_height: u16,
    summary_visibility: DetailsSamplesSummaryVisibility,
    show_base_summary: bool,
) -> usize {
    details_samples_content_layout(inner_height, summary_visibility, show_base_summary).row_capacity
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DetailsSamplesSummaryVisibility {
    pub(crate) show_ab_summary: bool,
    pub(crate) show_ab_range_summary: bool,
}

impl DetailsSamplesSummaryVisibility {
    fn required_height(self) -> u16 {
        u16::from(self.show_ab_summary) * DETAILS_SAMPLES_AB_SUMMARY_HEIGHT
            + u16::from(self.show_ab_range_summary) * DETAILS_SAMPLES_AB_RANGE_SUMMARY_HEIGHT
    }
}

pub(crate) fn details_samples_summary_visibility(
    comparison: Option<&AbComparison>,
) -> DetailsSamplesSummaryVisibility {
    DetailsSamplesSummaryVisibility {
        show_ab_summary: comparison.is_some(),
        show_ab_range_summary: comparison
            .is_some_and(|comparison| comparison.a.is_some() && comparison.b.is_some()),
    }
}

fn details_samples_min_height(summary_visibility: DetailsSamplesSummaryVisibility) -> u16 {
    DETAILS_SAMPLES_MIN_HEIGHT.max(
        DETAILS_SAMPLES_FRAME_HEIGHT
            + DETAILS_SAMPLES_HEADER_HEIGHT
            + DETAILS_SAMPLES_MIN_ROW_HEIGHT
            + summary_visibility.required_height(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DetailsSamplesContentLayout {
    pub(crate) row_capacity: usize,
    pub(crate) show_base_summary: bool,
    pub(crate) spacer_height: u16,
}

pub(crate) fn details_samples_content_layout(
    inner_height: u16,
    summary_visibility: DetailsSamplesSummaryVisibility,
    request_base_summary: bool,
) -> DetailsSamplesContentLayout {
    let required_summary_height = summary_visibility.required_height();
    let height_after_required_content = inner_height
        .saturating_sub(DETAILS_SAMPLES_HEADER_HEIGHT)
        .saturating_sub(1)
        .saturating_sub(required_summary_height);
    let show_base_summary = request_base_summary
        && height_after_required_content
            >= DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT + DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT;
    let base_summary_height = if show_base_summary {
        DETAILS_SAMPLES_BASE_SUMMARY_HEIGHT
    } else {
        0
    };
    let spacer_height = if height_after_required_content > base_summary_height {
        DETAILS_SAMPLES_SUMMARY_SPACER_HEIGHT
    } else {
        0
    };
    let row_capacity = inner_height
        .saturating_sub(DETAILS_SAMPLES_HEADER_HEIGHT)
        .saturating_sub(required_summary_height)
        .saturating_sub(base_summary_height)
        .saturating_sub(spacer_height)
        .max(1) as usize;

    DetailsSamplesContentLayout {
        row_capacity,
        show_base_summary,
        spacer_height,
    }
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
    fn dynamic_process_height_matches_content_and_available_height() {
        let screen = Rect::new(0, 0, 120, 60);
        let cases = [
            (0, false, 3, 0, false),
            (1, false, 4, 1, false),
            (4, false, 7, 4, false),
            (20, false, 21, 18, false),
            (0, true, 4, 0, true),
            (1, true, 5, 1, true),
            (4, true, 8, 4, true),
            (20, true, 21, 17, true),
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
    fn manual_process_height_can_exceed_the_automatic_cap() {
        let screen = Rect::new(0, 0, 120, 60);
        let automatic = main_panel_areas(screen, true, 30, false);
        let manual =
            main_panel_areas_with_height(screen, true, 30, false, ProcessPanelHeight::Manual(20));

        assert_eq!(automatic.processes.body_capacity, 18);
        assert_eq!(manual.processes.body_capacity, 20);
        assert_eq!(
            manual.processes.area.height,
            PROCESS_TABLE_CHROME_HEIGHT + 20
        );
        assert!(manual.details.unwrap().height >= GRAPH_WORKSPACE_MIN_HEIGHT);
    }

    #[test]
    fn manual_process_height_does_not_create_blank_body_rows() {
        let panels = main_panel_areas_with_height(
            Rect::new(0, 0, 120, 60),
            true,
            2,
            false,
            ProcessPanelHeight::Manual(20),
        );

        assert_eq!(panels.processes.body_capacity, 2);
        assert_eq!(panels.processes.page_size, 2);
    }

    #[test]
    fn manual_process_height_clamps_without_changing_the_preference() {
        let preference = ProcessPanelHeight::Manual(20);
        let short =
            main_panel_areas_with_height(Rect::new(0, 0, 120, 25), true, 30, false, preference);
        let tall =
            main_panel_areas_with_height(Rect::new(0, 0, 120, 60), true, 30, false, preference);

        assert_eq!(short.details.unwrap().height, GRAPH_WORKSPACE_MIN_HEIGHT);
        assert!(short.processes.body_capacity < 20);
        assert_eq!(tall.processes.body_capacity, 20);
    }

    #[test]
    fn preferred_body_capacity_includes_the_tracked_total_row() {
        let panels = main_panel_areas_with_height(
            Rect::new(0, 0, 120, 60),
            true,
            3,
            true,
            ProcessPanelHeight::Manual(3),
        );

        assert_eq!(panels.processes.body_capacity, 3);
        assert_eq!(panels.processes.page_size, 2);
        assert!(panels.processes.show_tracked_total);
    }

    #[test]
    fn process_resize_handle_is_the_shared_bottom_border() {
        let panels = main_panel_areas(Rect::new(0, 0, 120, 60), true, 30, false);
        let handle = panels
            .processes
            .resize_handle
            .expect("visible Graphs should expose the shared resize border");

        assert_eq!(handle.x, panels.processes.area.x);
        assert_eq!(handle.width, panels.processes.area.width);
        assert_eq!(handle.y, panels.processes.area.bottom() - 1);
        assert_eq!(panels.details.unwrap().y, panels.processes.area.bottom());
        assert!(
            main_panel_areas(Rect::new(0, 0, 120, 60), false, 30, false)
                .processes
                .resize_handle
                .is_none()
        );
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
    fn short_samples_layout_keeps_one_row_and_the_complete_ab_range_summary() {
        let summary_visibility = DetailsSamplesSummaryVisibility {
            show_ab_summary: true,
            show_ab_range_summary: true,
        };
        let layout = details_samples_content_layout(9, summary_visibility, true);

        assert_eq!(layout.row_capacity, 1);
        assert!(!layout.show_base_summary);
        assert_eq!(layout.spacer_height, 0);
        assert_eq!(details_samples_min_height(summary_visibility), 11);
    }

    #[test]
    fn tall_samples_layout_restores_base_summary_and_spacing() {
        let layout = details_samples_content_layout(
            14,
            DetailsSamplesSummaryVisibility {
                show_ab_summary: true,
                show_ab_range_summary: true,
            },
            true,
        );

        assert_eq!(layout.row_capacity, 3);
        assert!(layout.show_base_summary);
        assert_eq!(layout.spacer_height, 1);
    }
}
