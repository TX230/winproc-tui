pub(crate) mod column_picker;
mod cpu_panel;
pub(crate) mod details_panel;
pub(crate) mod footer;
pub(crate) mod format;
pub(crate) mod header;
pub(crate) mod help;
pub(crate) mod layout;
pub(crate) mod log_list;
pub(crate) mod open_files;
pub(crate) mod process_info_dialog;
pub(crate) mod process_kill_confirm;
pub(crate) mod process_table;
pub(crate) mod quit_confirm;
pub(crate) mod recording_dialog;
pub(crate) mod settings_dialog;
pub(crate) mod system_panel;
pub(crate) mod theme;
pub(crate) mod tracked_remove_confirm;
pub(crate) mod widgets;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::Style,
    widgets::{Block, Clear},
};

use crate::App;

#[cfg(test)]
pub(crate) use column_picker::column_picker_area;
use column_picker::draw_column_picker;
pub(crate) use column_picker::{
    column_picker_close_button_area_for_screen, column_picker_index_at,
    column_picker_page_size_for_screen, column_picker_row_for_index,
    column_picker_scroll_max_for_page_size, column_picker_scrollbar_area,
};
use cpu_panel::draw_cpu_panel;
use details_panel::draw_details_panel;
use footer::draw_footer;
pub(crate) use format::fmt_bytes;
use header::draw_header;
use help::draw_help;
pub(crate) use help::{
    help_area, help_close_button_area, help_page_size_for_screen, help_scroll_max_for_page_size,
    help_scrollbar_area,
};
pub(crate) use layout::{
    GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, details_slot_areas_for_screen,
    details_slots_area_for_screen, process_table_area_for_screen, process_table_page_size,
    screen_layout,
};
#[cfg(test)]
pub(crate) use layout::{details_graph_area_for_screen, details_samples_area_for_screen};
use log_list::{draw_log_dir_dialog, draw_log_list};
pub(crate) use log_list::{
    log_dir_button_at, log_list_index_at, log_list_page_size_for_screen,
    log_list_total_rows_for_count,
};
use open_files::draw_open_files;
pub(crate) use open_files::{
    open_files_close_button_area_for_screen, open_files_page_size_for_screen,
    open_files_scrollbar_area_for_screen, open_files_total_rows,
};
use process_info_dialog::draw_process_info_dialog;
pub(crate) use process_info_dialog::process_info_close_button_area_for_screen;
use process_kill_confirm::draw_process_kill_confirm;
pub(crate) use process_kill_confirm::process_kill_button_at;
#[cfg(test)]
pub(crate) use process_kill_confirm::process_kill_dialog_area;
use process_table::draw_process_table;
#[cfg(test)]
pub(crate) use process_table::process_table_visible_column_count;
pub(crate) use process_table::{
    process_metric_column_index_at, process_table_visible_metric_range,
    process_tracked_only_checkbox_area,
};
use quit_confirm::draw_quit_confirm;
pub(crate) use quit_confirm::quit_confirm_button_at;
use recording_dialog::{
    draw_recording_no_tracked_warning, draw_recording_overwrite_confirm, draw_recording_path_dialog,
};
pub(crate) use recording_dialog::{
    recording_no_tracked_ok_button_area, recording_overwrite_button_at, recording_path_button_at,
};
use settings_dialog::draw_settings_dialog;
pub(crate) use settings_dialog::{settings_ok_button_area, settings_selection_at};
use system_panel::draw_system_panel;
#[cfg(test)]
pub(crate) use system_panel::{
    SummaryInfoStyle, optional_value_color, render_summary_info_line,
    render_summary_info_value_spans, render_summary_line,
};
pub(crate) use system_panel::{
    ram_vram_panel_area_for_screen, system_activity_panel_area_for_screen,
};
pub(crate) use theme::{THEMES, Theme, theme_index_by_name};
use tracked_remove_confirm::draw_tracked_remove_confirm;
pub(crate) use tracked_remove_confirm::tracked_remove_button_at;

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = frame.area();
    let theme = app.theme();

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    let layout = screen_layout(area);

    draw_header(frame, layout[0], app, theme);
    draw_body(frame, layout[1], app, theme);
    draw_footer(frame, layout[2], app, theme);

    if app.show_help {
        draw_help(frame, area, app, theme);
    }
    if app.show_column_picker {
        draw_column_picker(frame, area, app, theme);
    }
    if app.show_log_list {
        draw_log_list(frame, area, app, theme);
    }
    if app.show_log_dir_dialog {
        draw_log_dir_dialog(frame, area, app, theme);
    }
    if app.show_open_files {
        draw_open_files(frame, area, app, theme);
    }
    if app.show_process_info_dialog {
        draw_process_info_dialog(frame, area, app, theme);
    }
    if app.show_recording_no_tracked_warning {
        draw_recording_no_tracked_warning(frame, area, theme);
    }
    if app.show_recording_path_dialog {
        draw_recording_path_dialog(frame, area, app, theme);
    }
    if app.show_recording_overwrite_confirmation {
        draw_recording_overwrite_confirm(frame, area, app, theme);
    }
    if app.show_tracked_remove_confirmation {
        draw_tracked_remove_confirm(frame, area, app, theme);
    }
    if app.show_process_kill_confirmation {
        draw_process_kill_confirm(frame, area, app, theme);
    }
    if app.show_settings_dialog {
        draw_settings_dialog(frame, area, app, theme);
    }
    if app.show_display_area_warning {
        draw_display_area_warning(frame, area, theme);
    }
    if app.show_metric_column_warning {
        draw_metric_column_warning(frame, area, theme);
    }
    if app.show_no_graph_metrics_warning {
        draw_no_graph_metrics_warning(frame, area, theme);
    }
    if app.show_quit_confirmation {
        draw_quit_confirm(frame, area, app, theme);
    }
}

fn draw_body(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, app: &App, theme: Theme) {
    let sections = layout::body_sections(area);

    draw_system_panel(frame, sections[0], app, theme);
    draw_cpu_panel(frame, sections[1], app, theme);
    if app.show_details {
        let lower = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(13), Constraint::Min(20)])
            .split(sections[2]);
        draw_process_table(frame, lower[0], app, theme);
        draw_details_panel(frame, lower[1], app, theme);
    } else {
        draw_process_table(frame, sections[2], app, theme);
    }
}

fn draw_display_area_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 38, 4);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_message_dialog(
        "Warning",
        "Not enough display area.",
        widgets::confirm_dialog::button_line(&[(" OK ", true)], theme),
        theme,
    );
    frame.render_widget(dialog, popup);
}

pub(crate) fn display_area_warning_ok_button_area(
    area: ratatui::layout::Rect,
) -> Option<ratatui::layout::Rect> {
    warning_message_ok_button_area(area, 38, 4)
}

fn draw_metric_column_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 58, 4);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_message_dialog(
        "Warning",
        "Move to a metric cell before pressing 1-4.",
        widgets::confirm_dialog::button_line(&[(" OK ", true)], theme),
        theme,
    );
    frame.render_widget(dialog, popup);
}

pub(crate) fn metric_column_warning_ok_button_area(
    area: ratatui::layout::Rect,
) -> Option<ratatui::layout::Rect> {
    warning_message_ok_button_area(area, 58, 4)
}

fn draw_no_graph_metrics_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 82, 5);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_dialog(
        "Warning",
        "No metric is selected for graphing.",
        "Select a metric, then press 1-4 to show it in Graph#1-#4.",
        widgets::confirm_dialog::button_line(&[(" OK ", true)], theme),
        theme,
    );
    frame.render_widget(dialog, popup);
}

pub(crate) fn no_graph_metrics_warning_ok_button_area(
    area: ratatui::layout::Rect,
) -> Option<ratatui::layout::Rect> {
    warning_message_ok_button_area_at(area, 82, 5, 2)
}

fn warning_message_ok_button_area(
    area: ratatui::layout::Rect,
    width: u16,
    height: u16,
) -> Option<ratatui::layout::Rect> {
    warning_message_ok_button_area_at(area, width, height, 1)
}

fn warning_message_ok_button_area_at(
    area: ratatui::layout::Rect,
    width: u16,
    height: u16,
    row_from_content_top: u16,
) -> Option<ratatui::layout::Rect> {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, width, height);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    widgets::confirm_dialog::button_areas(content, row_from_content_top, &[" OK "])
        .into_iter()
        .next()
}
