use ratatui::{
    layout::{Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    App,
    app::{AppActivity, FocusedPanel, GraphSlot, GraphSourceState, ResourcePanel},
    model::{DiskUsageSample, SystemMetric},
    ui::{
        Theme,
        cpu_panel::{cpu_panel_lines_for_app, draw_cpu_panel},
        footer::shortcut_spans,
        format::{format_frequency_mhz, format_integer, format_mb, ratio_optional},
        graph_slot::graph_value_style,
        layout::system_panel_area_for_screen,
        widgets::block::{panel_block_focused, panel_title},
    },
};

const SUMMARY_ROW_LABEL_WIDTH: usize = 17;
const GPU_ROW_LABEL_WIDTH: usize = 10;
const MEMORY_COLUMN_GAP: u16 = 1;

pub(crate) fn draw_system_panel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let panels = top_panel_areas(area, app);
    let memory_columns = memory_usage_columns(app, theme);

    let memory_block = panel_block_focused(
        memory_title(app, theme),
        theme,
        app.panel_has_focus(FocusedPanel::System) && app.resource_panel == ResourcePanel::Memory,
    );
    let memory_inner = memory_block.inner(panels[0]);
    frame.render_widget(memory_block, panels[0]);

    let memory_column_areas = memory_column_areas(memory_inner, &memory_columns[0]);
    for (lines, column_area) in memory_columns.into_iter().zip(memory_column_areas) {
        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(theme.panel)),
            column_area,
        );
    }

    if panels[1].width > 0 {
        let gpu_block = panel_block_focused(
            gpu_title(app),
            theme,
            app.panel_has_focus(FocusedPanel::System) && app.resource_panel == ResourcePanel::Gpu,
        );
        let gpu_inner = gpu_block.inner(panels[1]);
        frame.render_widget(gpu_block, panels[1]);
        frame.render_widget(
            Paragraph::new(Text::from(gpu_usage_lines(app, theme)))
                .style(Style::default().bg(theme.panel)),
            gpu_inner,
        );
    }

    let activity_block = panel_block_focused(
        Line::from(Span::styled(
            "NW/DISK",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        theme,
        app.panel_has_focus(FocusedPanel::SystemActivity),
    );
    let activity_inner = activity_block.inner(panels[2]);
    frame.render_widget(activity_block, panels[2]);

    let right = Paragraph::new(Text::from(system_activity_lines(app, theme)))
        .style(Style::default().bg(theme.panel));
    frame.render_widget(right, activity_inner);

    draw_cpu_panel(frame, panels[3], app, theme);
}

pub(crate) fn draw_system_info_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = system_info_dialog_area(area);
    frame.render_widget(Clear, popup);
    let block = panel_block_focused(panel_title("SYSTEM INFO"), theme, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let content_height = inner.height.saturating_sub(2);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    let lines = system_info_dialog_lines(app, theme);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(theme.panel)),
        content,
    );

    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(&[("Enter/Esc", "Close")], theme))),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}

fn system_info_dialog_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    system_info_lines(app, theme)
}

fn system_info_dialog_area(area: Rect) -> Rect {
    let width = 100.min(area.width);
    let height = 21.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn memory_title(app: &App, theme: Theme) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "MEM",
        Style::default().add_modifier(ratatui::style::Modifier::BOLD),
    )];
    if app.activity() == AppActivity::LogView {
        spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        spans.push(Span::styled(
            format!(
                "[Samples: {}]",
                format_integer(app.display_system_history().len() as u64)
            ),
            Style::default()
                .fg(theme.text)
                .bg(theme.panel_alt)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn gpu_title(app: &App) -> Line<'static> {
    let (page, count) = app.gpu_adapter_page();
    Line::from(Span::styled(
        format!("GPU {page}/{count}"),
        Style::default().add_modifier(Modifier::BOLD),
    ))
}

fn system_activity_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let snapshot = app.display_snapshot();
    let rows = [
        (
            SystemMetric::NetworkReceived,
            render_summary_graph_slot_value_line(
                system_metric_graph_state(app, SystemMetric::NetworkReceived),
                "Net Rx",
                &format_optional_mbps(snapshot.network_received_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::NetworkSent,
            render_summary_graph_slot_value_line(
                system_metric_graph_state(app, SystemMetric::NetworkSent),
                "Net Tx",
                &format_optional_mbps(snapshot.network_sent_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskRead,
            render_summary_graph_slot_value_line(
                system_metric_graph_state(app, SystemMetric::DiskRead),
                "Disk R",
                &format_optional_whole_mb_per_sec(snapshot.disk_read_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskWrite,
            render_summary_graph_slot_value_line(
                system_metric_graph_state(app, SystemMetric::DiskWrite),
                "Disk W",
                &format_optional_whole_mb_per_sec(snapshot.disk_write_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskQueueLength,
            render_summary_graph_slot_value_line(
                system_metric_graph_state(app, SystemMetric::DiskQueueLength),
                "Disk Q",
                &format_optional_queue_length(snapshot.disk_queue_length),
                theme,
            ),
        ),
    ];
    let selected_metric = app.selected_system_activity_metric();
    rows.into_iter()
        .map(|(metric, line)| {
            if app.panel_has_focus(FocusedPanel::SystemActivity) && metric == selected_metric {
                line.style(Style::default().bg(theme.table_selection_surface))
            } else {
                line
            }
        })
        .collect()
}

fn system_info_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let snapshot = app.display_snapshot();
    vec![
        render_summary_info_line(
            "CPU",
            &format_cpu_summary(
                snapshot.cpu_name.as_deref().unwrap_or("--"),
                snapshot.cpu_frequency_mhz,
            ),
            SummaryInfoStyle::Plain,
            theme,
        ),
        render_summary_info_line(
            "Cores",
            snapshot.cpu_topology.as_deref().unwrap_or("--"),
            SummaryInfoStyle::Plain,
            theme,
        ),
        render_summary_info_line(
            "Cache",
            snapshot.cpu_cache.as_deref().unwrap_or("--"),
            SummaryInfoStyle::Measurement,
            theme,
        ),
        render_summary_info_line(
            "GPU",
            &format_gpu_adapters(&snapshot.gpu_adapters),
            SummaryInfoStyle::Plain,
            theme,
        ),
        render_summary_info_line(
            "Disk",
            &format_disk_summary(&snapshot.disks),
            SummaryInfoStyle::Measurement,
            theme,
        ),
    ]
}

fn render_summary_graph_slot_value_line(
    graph_state: Option<GraphSourceState>,
    label: &'static str,
    value: &str,
    theme: Theme,
) -> Line<'static> {
    render_summary_graph_slot_value_line_with_label_width(graph_state, label, 8, value, theme)
}

fn render_summary_graph_slot_value_line_with_label_width(
    graph_state: Option<GraphSourceState>,
    label: &'static str,
    label_width: usize,
    value: &str,
    theme: Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<label_width$}"),
            Style::default().fg(theme.muted),
        ),
        Span::styled(
            value.to_string(),
            graph_value_style(Style::default().fg(theme.text), graph_state, theme),
        ),
    ])
}

pub(crate) fn ram_vram_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    top_panel_areas(area, app)[0]
}

pub(crate) fn gpu_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    top_panel_areas(area, app)[1]
}

pub(crate) fn memory_metric_at_position(
    screen_area: Rect,
    app: &App,
    x: u16,
    y: u16,
) -> Option<SystemMetric> {
    let panel = ram_vram_panel_area_for_screen(screen_area, app);
    if panel.width < 2 || panel.height < 2 {
        return None;
    }
    let inner = panel.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let row = usize::from(y.checked_sub(inner.y)?);
    let columns = memory_usage_columns(app, app.theme());
    let column_areas = memory_column_areas(inner, &columns[0]);
    if point_in_rect(column_areas[0], x, y) {
        SystemMetric::MEMORY_OVERVIEW_PANEL.get(row).copied()
    } else if point_in_rect(column_areas[1], x, y) {
        SystemMetric::MEMORY_PRESSURE_PANEL.get(row).copied()
    } else {
        None
    }
}

pub(crate) fn cpu_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    top_panel_areas(area, app)[3]
}

pub(crate) fn system_activity_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    top_panel_areas(area, app)[2]
}

fn top_panel_areas(area: Rect, app: &App) -> [Rect; 4] {
    let memory_columns = memory_usage_columns(app, app.theme());
    let gpu_lines = gpu_usage_lines(app, app.theme());
    let activity_lines = system_activity_lines(app, app.theme());
    let cpu_lines = cpu_panel_lines_for_app(app, app.theme(), area.height.saturating_sub(2));
    let memory_desired = desired_memory_panel_width(&memory_columns, 28);
    let gpu_desired = desired_panel_width(&gpu_lines, 28);
    let activity_desired = desired_panel_width(&activity_lines, 14);
    let cpu_desired = desired_panel_width(&cpu_lines, 24);
    let wide = memory_desired
        .saturating_add(gpu_desired)
        .saturating_add(activity_desired)
        .saturating_add(cpu_desired)
        <= area.width;
    let (memory_width, gpu_width) = if wide {
        (memory_desired, gpu_desired)
    } else if app.resource_panel == ResourcePanel::Memory {
        (memory_desired.min(area.width), 0)
    } else {
        (0, gpu_desired.min(area.width))
    };
    let remaining = area
        .width
        .saturating_sub(memory_width)
        .saturating_sub(gpu_width);
    let activity_width = activity_desired.min(remaining.saturating_sub(cpu_desired.min(remaining)));
    let cpu_width = remaining.saturating_sub(activity_width);
    [
        Rect::new(area.x, area.y, memory_width, area.height),
        Rect::new(
            area.x.saturating_add(memory_width),
            area.y,
            gpu_width,
            area.height,
        ),
        Rect::new(
            area.x
                .saturating_add(memory_width)
                .saturating_add(gpu_width),
            area.y,
            activity_width,
            area.height,
        ),
        Rect::new(
            area.x
                .saturating_add(memory_width)
                .saturating_add(gpu_width)
                .saturating_add(activity_width),
            area.y,
            cpu_width,
            area.height,
        ),
    ]
}

fn memory_usage_columns(app: &App, theme: Theme) -> [Vec<Line<'static>>; 2] {
    let snapshot = app.display_snapshot();
    let overview_rows = vec![
        (
            SystemMetric::PhysicalMemory,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::PhysicalMemory),
                "In use",
                Some(snapshot.used_memory),
                Some(snapshot.total_memory),
                None,
                theme,
            ),
        ),
        (
            SystemMetric::ModifiedMemory,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::ModifiedMemory),
                "Modified",
                snapshot.modified_memory,
                None,
                None,
                theme,
            ),
        ),
        (
            SystemMetric::StandbyMemory,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::StandbyMemory),
                "Standby",
                snapshot.standby_memory,
                None,
                None,
                theme,
            ),
        ),
        (
            SystemMetric::FreeZeroedMemory,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::FreeZeroedMemory),
                "Free + Zeroed",
                snapshot.free_zeroed_memory,
                None,
                None,
                theme,
            ),
        ),
        (
            SystemMetric::Committed,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::Committed),
                "Commit charge",
                snapshot.committed_memory,
                snapshot.commit_limit,
                None,
                theme,
            ),
        ),
    ];
    let pressure_rows = vec![
        (
            SystemMetric::PagedPool,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::PagedPool),
                "Paged Pool",
                snapshot.paged_pool_memory,
                None,
                None,
                theme,
            ),
        ),
        (
            SystemMetric::NonpagedPool,
            render_summary_graph_slot_line(
                system_metric_graph_state(app, SystemMetric::NonpagedPool),
                "Nonpaged Pool",
                snapshot.nonpaged_pool_memory,
                None,
                None,
                theme,
            ),
        ),
        (
            SystemMetric::PagesInput,
            render_summary_graph_slot_value_line_with_label_width(
                system_metric_graph_state(app, SystemMetric::PagesInput),
                "Pages In/s",
                SUMMARY_ROW_LABEL_WIDTH,
                &format_optional_integer(snapshot.pages_input_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::PagesOutput,
            render_summary_graph_slot_value_line_with_label_width(
                system_metric_graph_state(app, SystemMetric::PagesOutput),
                "Pages Out/s",
                SUMMARY_ROW_LABEL_WIDTH,
                &format_optional_integer(snapshot.pages_output_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::ThreadCount,
            render_summary_graph_slot_value_line_with_label_width(
                system_metric_graph_state(app, SystemMetric::ThreadCount),
                "Threads",
                SUMMARY_ROW_LABEL_WIDTH,
                &format_optional_integer(snapshot.thread_count),
                theme,
            ),
        ),
    ];

    let selected_metric = app.selected_system_metric();
    let style_rows = |rows: Vec<(SystemMetric, Line<'static>)>| {
        rows.into_iter()
            .map(|(metric, line)| {
                if app.panel_has_focus(FocusedPanel::System)
                    && app.resource_panel == ResourcePanel::Memory
                    && metric == selected_metric
                {
                    line.style(Style::default().bg(theme.table_selection_surface))
                } else {
                    line
                }
            })
            .collect()
    };
    [style_rows(overview_rows), style_rows(pressure_rows)]
}

fn gpu_usage_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let adapter = app.selected_gpu_adapter();
    let rows = [
        (
            SystemMetric::GpuUtilization,
            render_gpu_percent_line(
                app,
                adapter,
                SystemMetric::GpuUtilization,
                "Usage",
                adapter.and_then(|value| value.utilization_percent),
                None,
                theme,
            ),
        ),
        (
            SystemMetric::GpuEncode,
            render_gpu_percent_line(
                app,
                adapter,
                SystemMetric::GpuEncode,
                "Encode",
                adapter.and_then(|value| value.encode.average_percent),
                adapter.map(|value| value.encode),
                theme,
            ),
        ),
        (
            SystemMetric::GpuDecode,
            render_gpu_percent_line(
                app,
                adapter,
                SystemMetric::GpuDecode,
                "Decode",
                adapter.and_then(|value| value.decode.average_percent),
                adapter.map(|value| value.decode),
                theme,
            ),
        ),
        (
            SystemMetric::GpuDedicated,
            render_gpu_memory_line(
                app,
                adapter,
                SystemMetric::GpuDedicated,
                "Dedicated",
                adapter.and_then(|value| value.dedicated_used),
                adapter.and_then(|value| value.dedicated_total),
                theme,
            ),
        ),
        (
            SystemMetric::GpuShared,
            render_gpu_memory_line(
                app,
                adapter,
                SystemMetric::GpuShared,
                "Shared",
                adapter.and_then(|value| value.shared_used),
                adapter.and_then(|value| value.shared_total),
                theme,
            ),
        ),
    ];
    let selected = app.selected_system_metric();
    rows.into_iter()
        .map(|(metric, line)| {
            if app.panel_has_focus(FocusedPanel::System)
                && app.resource_panel == ResourcePanel::Gpu
                && metric == selected
            {
                line.style(Style::default().bg(theme.table_selection_surface))
            } else {
                line
            }
        })
        .collect()
}

fn render_gpu_percent_line(
    app: &App,
    adapter: Option<&crate::model::GpuAdapterSample>,
    metric: SystemMetric,
    label: &'static str,
    average: Option<f64>,
    detail: Option<crate::model::GpuEngineSummary>,
    theme: Theme,
) -> Line<'static> {
    let value = average
        .map(|value| format!("{value:>3.0}%"))
        .unwrap_or_else(|| " --".to_string());
    let suffix = detail
        .filter(|value| value.engine_count > 0)
        .map(|value| {
            format!(
                " max {:>3.0}% {}E",
                value.max_percent.unwrap_or_default(),
                value.engine_count
            )
        })
        .unwrap_or_default();
    let slot = adapter.map(|adapter| {
        GraphSlot::gpu(adapter.id, adapter.name.as_deref().unwrap_or("GPU"), metric)
    });
    let graph_state = slot.as_ref().and_then(|slot| app.graph_source_state(slot));
    Line::from(vec![
        Span::styled(
            format!("{label:<GPU_ROW_LABEL_WIDTH$}"),
            Style::default().fg(theme.muted),
        ),
        Span::styled(
            value,
            graph_value_style(Style::default().fg(theme.text), graph_state, theme),
        ),
        Span::styled(suffix, Style::default().fg(theme.muted)),
    ])
}

fn render_gpu_memory_line(
    app: &App,
    adapter: Option<&crate::model::GpuAdapterSample>,
    metric: SystemMetric,
    label: &'static str,
    used: Option<u64>,
    total: Option<u64>,
    theme: Theme,
) -> Line<'static> {
    let slot = adapter.map(|adapter| {
        GraphSlot::gpu(adapter.id, adapter.name.as_deref().unwrap_or("GPU"), metric)
    });
    let graph_state = slot.as_ref().and_then(|slot| app.graph_source_state(slot));
    render_summary_line_with_label_width(
        label,
        GPU_ROW_LABEL_WIDTH,
        used,
        total,
        None,
        false,
        graph_value_style(Style::default().fg(theme.text), graph_state, theme),
        theme,
    )
}

fn format_optional_integer(value: Option<u64>) -> String {
    value
        .map(format_integer)
        .unwrap_or_else(|| "--".to_string())
}

fn system_metric_graph_state(app: &App, metric: SystemMetric) -> Option<GraphSourceState> {
    app.graph_source_state(&crate::app::GraphSlot::system(metric))
}

fn desired_panel_width(lines: &[Line<'_>], minimum: u16) -> u16 {
    (lines.iter().map(line_width).max().unwrap_or(1) as u16)
        .saturating_add(2)
        .max(minimum)
}

fn desired_memory_panel_width(columns: &[Vec<Line<'_>>; 2], minimum: u16) -> u16 {
    let content_width = columns
        .iter()
        .map(|lines| lines.iter().map(line_width).max().unwrap_or(1) as u16)
        .sum::<u16>()
        .saturating_add(MEMORY_COLUMN_GAP);
    content_width.saturating_add(2).max(minimum)
}

fn memory_column_areas(inner: Rect, left_lines: &[Line<'_>]) -> [Rect; 2] {
    let left_width = (left_lines.iter().map(line_width).max().unwrap_or(1) as u16).min(inner.width);
    let gap = MEMORY_COLUMN_GAP.min(inner.width.saturating_sub(left_width));
    let right_x = inner.x.saturating_add(left_width).saturating_add(gap);
    [
        Rect::new(inner.x, inner.y, left_width, inner.height),
        Rect::new(
            right_x,
            inner.y,
            inner.right().saturating_sub(right_x),
            inner.height,
        ),
    ]
}

fn point_in_rect(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn line_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum()
}

fn format_cpu_summary(name: &str, frequency_mhz: Option<u64>) -> String {
    match frequency_mhz {
        Some(_) => format!("{name} / {}", format_frequency_mhz(frequency_mhz)),
        None => name.to_string(),
    }
}

fn format_gpu_summary(name: &str, dedicated_total: Option<u64>) -> String {
    match dedicated_total {
        Some(total) => format!("{name} / {} GB VRAM", format_gb_number(total)),
        None => name.to_string(),
    }
}

fn format_gpu_adapters(adapters: &[crate::model::GpuAdapterSample]) -> String {
    if adapters.is_empty() {
        return "--".to_string();
    }
    adapters
        .iter()
        .enumerate()
        .map(|(index, adapter)| {
            format!(
                "{}: {}",
                index,
                format_gpu_summary(
                    adapter.name.as_deref().unwrap_or("--"),
                    adapter.dedicated_total
                )
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_disk_summary(disks: &[DiskUsageSample]) -> String {
    if disks.is_empty() {
        return "--".to_string();
    }

    disks
        .iter()
        .map(|disk| {
            let used_bytes = disk.total_bytes.saturating_sub(disk.free_bytes);
            format!(
                "{} {}/{} GB",
                disk.name,
                format_gb_number(used_bytes),
                format_gb_number(disk.total_bytes)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_optional_mbps(value: Option<u64>) -> String {
    value
        .map(|value| {
            format!(
                "{:>4} Mbps",
                ((value as f64 * 8.0) / 1_000_000.0).round() as u64
            )
        })
        .unwrap_or_else(|| format!("{:>4}", "--"))
}

fn format_optional_whole_mb_per_sec(value: Option<u64>) -> String {
    value
        .map(|value| format!("{:>4} MB/s", ((value as f64) / 1_000_000.0).round() as u64))
        .unwrap_or_else(|| format!("{:>4}", "--"))
}

fn format_optional_queue_length(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{:>4}", value.round().max(0.0) as u64))
        .unwrap_or_else(|| format!("{:>4}", "--"))
}

fn format_gb_number(bytes: u64) -> String {
    format_integer(((bytes as f64) / 1_000_000_000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::THEMES;

    #[test]
    fn disk_summary_formats_all_disks_on_one_line_without_percent() {
        let disks = vec![
            DiskUsageSample {
                name: "C:".to_string(),
                free_bytes: 17_000_000_000,
                total_bytes: 999_000_000_000,
            },
            DiskUsageSample {
                name: "X:".to_string(),
                free_bytes: 1_783_000_000_000,
                total_bytes: 2_000_000_000_000,
            },
        ];

        assert_eq!(
            format_disk_summary(&disks),
            "C: 982/999 GB, X: 217/2,000 GB"
        );
    }

    #[test]
    fn cpu_summary_places_clock_after_cpu_name() {
        assert_eq!(
            format_cpu_summary("Intel CPU", Some(2_100)),
            "Intel CPU / 2.10 GHz"
        );
    }

    #[test]
    fn gpu_summary_appends_vram_capacity() {
        assert_eq!(
            format_gpu_summary("NVIDIA GPU", Some(8_406_000_000)),
            "NVIDIA GPU / 8 GB VRAM"
        );
    }

    #[test]
    fn cache_and_standby_lines_show_single_value_without_empty_total() {
        let line = render_summary_line("Cache", Some(1_714_000_000), None, None, THEMES[0]);
        let joined = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(joined.trim(), "Cache            1,714 MB");
        assert!(!joined.contains('/'));
    }

    #[test]
    fn memory_value_with_commas_uses_one_text_color_span() {
        let line = render_summary_line(
            "Physical Memory",
            Some(14_915_000_000),
            Some(34_089_000_000),
            None,
            THEMES[0],
        );

        assert_eq!(line.spans[1].content.as_ref(), "14,915 MB / 34,089 MB");
        assert_eq!(line.spans[1].style.fg, Some(THEMES[0].text));
    }

    #[test]
    fn system_activity_formatters_use_whole_number_panel_units() {
        assert_eq!(format_optional_mbps(Some(30_000_000)), " 240 Mbps");
        assert_eq!(
            format_optional_whole_mb_per_sec(Some(10_400_000)),
            "  10 MB/s"
        );
        assert_eq!(format_optional_queue_length(Some(1.5)), "   2");
        assert_eq!(format_optional_queue_length(None), "  --");
    }
}

#[cfg(test)]
pub(crate) fn render_summary_line(
    title: &str,
    used: Option<u64>,
    total: Option<u64>,
    suffix: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    render_summary_line_with_label_width(
        title,
        SUMMARY_ROW_LABEL_WIDTH,
        used,
        total,
        suffix,
        true,
        Style::default().fg(theme.text),
        theme,
    )
}

fn render_summary_line_with_label_width(
    title: &str,
    label_width: usize,
    used: Option<u64>,
    total: Option<u64>,
    suffix: Option<&str>,
    show_ratio: bool,
    value_style: Style,
    theme: Theme,
) -> Line<'static> {
    let ratio_value = show_ratio.then(|| ratio_optional(used, total)).flatten();
    let stats = match (used, total) {
        (Some(used), Some(total)) => format!("{} / {}", format_mb(used), format_mb(total)),
        (Some(used), None) => format_mb(used),
        (None, Some(total)) => format!("-- / {}", format_mb(total)),
        (None, None) => "--".to_string(),
    };
    let suffix_text = suffix.unwrap_or("").to_string();
    let mut spans = vec![Span::styled(
        format!("{title:<label_width$}"),
        Style::default().fg(theme.muted),
    )];
    spans.push(Span::styled(stats, value_style));
    if let Some(ratio_value) = ratio_value {
        spans.push(Span::styled(
            format!(" ({:>3.0}%)", ratio_value * 100.0),
            value_style,
        ));
    }
    if !suffix_text.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(suffix_text, Style::default().fg(theme.muted)));
    }
    Line::from(spans)
}

fn render_summary_graph_slot_line(
    graph_state: Option<GraphSourceState>,
    title: &str,
    used: Option<u64>,
    total: Option<u64>,
    suffix: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    render_summary_line_with_label_width(
        title,
        SUMMARY_ROW_LABEL_WIDTH,
        used,
        total,
        suffix,
        false,
        graph_value_style(Style::default().fg(theme.text), graph_state, theme),
        theme,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SummaryInfoStyle {
    Plain,
    Measurement,
}

pub(crate) fn render_summary_info_line(
    title: &str,
    value: &str,
    value_style: SummaryInfoStyle,
    theme: Theme,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{title:<7} "),
        Style::default().fg(theme.muted),
    )];
    match value_style {
        SummaryInfoStyle::Plain => spans.push(Span::styled(
            value.to_string(),
            Style::default().fg(theme.text),
        )),
        SummaryInfoStyle::Measurement => {
            spans.extend(render_summary_info_value_spans(value, theme))
        }
    }
    Line::from(spans)
}

pub(crate) fn render_summary_info_value_spans(value: &str, theme: Theme) -> Vec<Span<'static>> {
    if value.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_is_numeric = None;
    let mut previous_char = None;

    for ch in value.chars() {
        let is_numeric = if current_is_numeric == Some(true) {
            ch.is_ascii_digit() || ch == '.' || ch == ','
        } else {
            starts_numeric_value_span(previous_char, ch)
        };
        if current_is_numeric == Some(is_numeric) {
            current.push(ch);
            previous_char = Some(ch);
            continue;
        }

        if !current.is_empty() {
            spans.push(Span::styled(
                current.clone(),
                Style::default().fg(if current_is_numeric == Some(true) {
                    theme.text
                } else {
                    theme.muted
                }),
            ));
            current.clear();
        }

        current.push(ch);
        current_is_numeric = Some(is_numeric);
        previous_char = Some(ch);
    }

    if !current.is_empty() {
        spans.push(Span::styled(
            current,
            Style::default().fg(if current_is_numeric == Some(true) {
                theme.text
            } else {
                theme.muted
            }),
        ));
    }

    spans
}

fn starts_numeric_value_span(previous_char: Option<char>, current_char: char) -> bool {
    (current_char.is_ascii_digit() || current_char == '.')
        && match previous_char {
            None => true,
            Some(ch) if ch.is_ascii_whitespace() => true,
            Some('(' | '[' | '/' | ':') => true,
            _ => false,
        }
}

#[cfg(test)]
pub(crate) fn optional_value_color(value: Option<u64>, theme: Theme) -> ratatui::prelude::Color {
    match value {
        Some(_) => theme.text,
        None => theme.muted,
    }
}
