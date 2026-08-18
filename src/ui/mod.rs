pub(crate) mod column_picker;
mod cpu_core_dialog;
mod cpu_panel;
pub(crate) mod details_panel;
pub(crate) mod footer;
pub(crate) mod format;
pub(crate) mod graph_reorder;
pub(crate) mod graph_slot;
pub(crate) mod header;
pub(crate) mod help;
pub(crate) mod layout;
pub(crate) mod log_list;
pub(crate) mod open_files;
pub(crate) mod process_environment;
pub(crate) mod process_info_dialog;
pub(crate) mod process_kill_confirm;
pub(crate) mod process_modules;
pub(crate) mod process_table;
pub(crate) mod quit_confirm;
pub(crate) mod recording_dialog;
pub(crate) mod system_panel;
pub(crate) mod theme;
pub(crate) mod tracked_lists;
pub(crate) mod tracked_remove_confirm;
pub(crate) mod widgets;

use ratatui::{
    prelude::Style,
    widgets::{Block, Clear},
};

use crate::App;

#[cfg(test)]
pub(crate) use column_picker::column_picker_area;
use column_picker::draw_column_picker;
pub(crate) use column_picker::{
    column_picker_index_at, column_picker_page_size_for_screen, column_picker_row_for_index,
    column_picker_scroll_max_for_page_size, column_picker_scrollbar_area,
};
use cpu_core_dialog::draw_cpu_core_dialog;
pub(crate) use cpu_core_dialog::{
    cpu_core_dialog_content_area, cpu_core_dialog_page_size_for_screen,
    cpu_core_dialog_scrollbar_area, cpu_core_dialog_total_rows,
};
pub(crate) use cpu_panel::{cpu_metric_at_position, cpu_per_core_button_area};
use details_panel::draw_details_panel;
use footer::draw_footer;
pub(crate) use format::fmt_bytes;
use graph_reorder::draw_graph_reorder_dialog;
pub(crate) use graph_reorder::{
    graph_reorder_index_at, graph_reorder_page_size_for_screen, graph_reorder_row_for_index,
    graph_reorder_scrollbar_area,
};
use header::draw_header;
use help::draw_help;
#[cfg(test)]
pub(crate) use help::help_area;
pub(crate) use help::{
    help_page_size_for_screen, help_scroll_max_for_page_size, help_scrollbar_area,
};
#[cfg(test)]
pub(crate) use layout::main_panel_areas;
#[cfg(test)]
pub(crate) use layout::{
    GRAPH_ALL_SAMPLES_TOGGLE_WIDTH, GRAPH_Y_AXIS_TOGGLE_WIDTH, details_graph_area_for_app,
    details_samples_area_for_app, details_shared_controls_area_for_app,
};
pub(crate) use layout::{main_panel_areas_for_app, screen_layout};
use log_list::{draw_log_dir_dialog, draw_log_list};
pub(crate) use log_list::{
    log_list_index_at, log_list_page_size_for_screen, log_list_total_rows_for_count,
};
pub(crate) use open_files::open_files_total_rows;
use process_info_dialog::draw_process_info_dialog;
pub(crate) use process_info_dialog::{
    process_info_content_area_for_screen, process_info_page_size_for_screen,
    process_info_scrollbar_area_for_screen, process_info_tab_at, process_info_total_rows,
};
use process_kill_confirm::draw_process_kill_confirm;
#[cfg(test)]
pub(crate) use process_kill_confirm::process_kill_dialog_area;
use process_table::draw_process_table;
#[cfg(test)]
pub(crate) use process_table::process_table_visible_column_count;
pub(crate) use process_table::{
    process_metric_column_index_at, process_table_visible_metric_range,
    process_tracked_only_control_area,
};
use quit_confirm::draw_quit_confirm;
use recording_dialog::{
    draw_recording_error, draw_recording_no_tracked_warning, draw_recording_overwrite_confirm,
    draw_recording_path_dialog, draw_recording_stop_confirm, draw_recording_tracking_fixed,
};
pub(crate) use recording_dialog::{
    recording_interval_option_at, recording_interval_selector_area, recording_path_input_area,
};
#[cfg(test)]
pub(crate) use system_panel::{
    SummaryInfoStyle, optional_value_color, render_summary_info_line,
    render_summary_info_value_spans, render_summary_line,
};
pub(crate) use system_panel::{
    cpu_panel_area_for_screen, gpu_panel_area_for_screen, memory_metric_at_position,
    ram_vram_panel_area_for_screen, system_activity_panel_area_for_screen,
};
use system_panel::{draw_system_info_dialog, draw_system_panel};
pub(crate) use theme::{THEMES, Theme, theme_index_by_name};
use tracked_lists::draw_tracked_lists;
pub(crate) use tracked_lists::{
    tracked_list_index_at, tracked_list_save_name_area_for_screen,
    tracked_list_startup_at_for_screen, tracked_lists_page_size_for_screen,
};
use tracked_remove_confirm::draw_tracked_remove_confirm;
#[cfg(test)]
pub(crate) use tracked_remove_confirm::tracked_remove_dialog_area;

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = frame.area();
    let theme = app.theme();

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    let layout = screen_layout(area);
    let panels = layout::main_panel_areas_for_app(area, app);

    draw_header(frame, layout[0], app, theme);
    draw_body(frame, panels, app, theme);
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
    if app.show_process_info_dialog {
        draw_process_info_dialog(frame, area, app, theme);
    }
    if app.show_cpu_core_dialog {
        draw_cpu_core_dialog(frame, area, app, theme);
    }
    if app.show_system_info_dialog {
        draw_system_info_dialog(frame, area, app, theme);
    }
    if app.graph_reorder_dialog.is_some() {
        draw_graph_reorder_dialog(frame, area, app, theme);
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
    if app.show_recording_tracking_fixed {
        draw_recording_tracking_fixed(frame, area, theme);
    }
    if app.show_recording_stop_confirmation {
        draw_recording_stop_confirm(frame, area, theme);
    }
    if app.show_tracked_remove_confirmation {
        draw_tracked_remove_confirm(frame, area, app, theme);
    }
    if app.show_process_kill_confirmation {
        draw_process_kill_confirm(frame, area, app, theme);
    }
    if app.tracked_lists_dialog.is_some() {
        draw_tracked_lists(frame, area, app, theme);
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
    if app.recording_error.is_some() {
        draw_recording_error(frame, area, app, theme);
    }
}

fn draw_body(
    frame: &mut ratatui::Frame<'_>,
    panels: layout::MainPanelAreas,
    app: &App,
    theme: Theme,
) {
    draw_system_panel(frame, panels.system, app, theme);
    draw_process_table(frame, panels.processes, app, theme);
    if let Some(details) = panels.details {
        draw_details_panel(frame, details, app, theme);
    }
}

fn draw_display_area_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 38, 5);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_message_dialog(
        "WARNING",
        "Not enough display area.",
        ratatui::text::Line::from(footer::warning_shortcut_spans(
            &[("Enter/Esc", "Close")],
            theme,
        )),
        theme,
    );
    frame.render_widget(dialog, popup);
}

fn draw_metric_column_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 58, 5);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_message_dialog(
        "WARNING",
        "Select a graphable metric cell.",
        ratatui::text::Line::from(footer::warning_shortcut_spans(
            &[("Enter/Esc", "Close")],
            theme,
        )),
        theme,
    );
    frame.render_widget(dialog, popup);
}

fn draw_no_graph_metrics_warning(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    theme: Theme,
) {
    let popup = widgets::confirm_dialog::centered_dialog_rect(area, 82, 6);
    frame.render_widget(Clear, popup);
    let dialog = widgets::confirm_dialog::warning_dialog(
        "WARNING",
        "No metric is selected for graphing.",
        "Select a metric, then press Space or double-click it.",
        ratatui::text::Line::from(footer::warning_shortcut_spans(
            &[("Enter/Esc", "Close")],
            theme,
        )),
        theme,
    );
    frame.render_widget(dialog, popup);
}
