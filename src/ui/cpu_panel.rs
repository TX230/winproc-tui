use ratatui::{
    layout::{Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::{FocusedPanel, GraphSlot, GraphSourceState},
    model::{CpuCoreKind, Snapshot, SystemMetric},
    ui::{
        Theme, format::format_integer, graph_slot::graph_value_style,
        widgets::block::panel_block_focused,
    },
};

const LABEL_WIDTH: usize = 11;
const USAGE_ROW: u16 = 0;
const THREADS_ROW: u16 = 2;
const PROCESSES_ROW: u16 = 3;
const PER_CORE_ROW: u16 = 4;
pub(crate) const PER_CORE_BUTTON_LABEL: &str = "[Per-core Usage (P/E)]";

pub(crate) fn draw_cpu_panel(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let block = panel_block_focused(
        Line::from(Span::styled(
            "CPU",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        theme,
        app.panel_has_focus(FocusedPanel::Cpu),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(Text::from(cpu_panel_lines_for_app(
        app,
        theme,
        inner.height,
    )))
    .style(Style::default().bg(theme.panel));
    frame.render_widget(paragraph, inner);
}

pub(crate) fn cpu_panel_lines_for_app(app: &App, theme: Theme, height: u16) -> Vec<Line<'static>> {
    let snapshot = app.display_snapshot();
    let mut lines = vec![
        cpu_usage_line(app, snapshot, theme),
        cpu_frequency_line(snapshot, theme),
        cpu_count_line(
            "Threads",
            snapshot.thread_count.map(format_integer),
            cpu_graph_state(app, SystemMetric::ThreadCount),
            theme,
        ),
        cpu_count_line(
            "Processes",
            Some(format_integer(snapshot.process_count as u64)),
            cpu_graph_state(app, SystemMetric::ProcessCount),
            theme,
        ),
        per_core_button_line(app, theme),
    ];

    if app.panel_has_focus(FocusedPanel::Cpu) {
        let selected_row = match app.selected_cpu_metric() {
            Some(SystemMetric::ThreadCount) => THREADS_ROW,
            Some(SystemMetric::ProcessCount) => PROCESSES_ROW,
            Some(_) => USAGE_ROW,
            None => PER_CORE_ROW,
        };
        if selected_row != PER_CORE_ROW
            && let Some(line) = lines.get_mut(selected_row as usize)
        {
            *line = line
                .clone()
                .style(Style::default().bg(theme.table_selection_surface));
        }
    }

    lines.truncate(height as usize);
    lines
}

pub(crate) fn cpu_metric_at_position(panel: Rect, x: u16, y: u16) -> Option<SystemMetric> {
    let inner = panel.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if !contains_point(inner, x, y) {
        return None;
    }
    match y.saturating_sub(inner.y) {
        USAGE_ROW => Some(SystemMetric::CpuAverage),
        THREADS_ROW => Some(SystemMetric::ThreadCount),
        PROCESSES_ROW => Some(SystemMetric::ProcessCount),
        _ => None,
    }
}

pub(crate) fn cpu_per_core_button_area(panel: Rect) -> Option<Rect> {
    let inner = panel.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.height <= PER_CORE_ROW || inner.width == 0 {
        return None;
    }
    Some(Rect::new(
        inner.x,
        inner.y.saturating_add(PER_CORE_ROW),
        (PER_CORE_BUTTON_LABEL.chars().count() as u16).min(inner.width),
        1,
    ))
}

fn cpu_usage_line(app: &App, snapshot: &Snapshot, theme: Theme) -> Line<'static> {
    Line::from(vec![
        metric_label("Usage", theme),
        Span::styled(
            format_cpu_average(snapshot.cpu_total_usage_percent),
            graph_value_style(
                Style::default().fg(theme.text),
                cpu_graph_state(app, SystemMetric::CpuAverage),
                theme,
            ),
        ),
        Span::styled(
            format!(
                " (U {}, K {})",
                format_cpu_part(snapshot.cpu_user_usage_percent),
                format_cpu_part(snapshot.cpu_kernel_usage_percent)
            ),
            Style::default().fg(theme.muted),
        ),
    ])
}

fn cpu_frequency_line(snapshot: &Snapshot, theme: Theme) -> Line<'static> {
    let has_e_core = snapshot
        .cpu_logical_processors
        .iter()
        .any(|core| core.kind == Some(CpuCoreKind::Efficiency));
    let primary = snapshot
        .cpu_p_core_frequency_mhz
        .or(snapshot.cpu_current_frequency_mhz);
    let value = if has_e_core {
        format!(
            "{} / {}",
            format_cpu_panel_frequency_mhz(primary),
            format_cpu_panel_frequency_mhz(snapshot.cpu_e_core_frequency_mhz)
        )
    } else {
        format_cpu_panel_frequency_mhz(primary)
    };
    Line::from(vec![
        metric_label("Freq(P/E)", theme),
        Span::styled(value, Style::default().fg(theme.text)),
    ])
}

fn per_core_button_line(app: &App, theme: Theme) -> Line<'static> {
    let style = if app.cpu_per_core_hovered {
        Style::default()
            .fg(theme.text)
            .bg(theme.focus_surface)
            .add_modifier(Modifier::BOLD)
    } else if app.panel_has_focus(FocusedPanel::Cpu) && app.cpu_per_core_selected() {
        Style::default()
            .fg(theme.text)
            .bg(theme.table_selection_surface)
    } else {
        Style::default().fg(theme.text)
    };
    Line::from(Span::styled(PER_CORE_BUTTON_LABEL, style))
}

fn cpu_count_line(
    label: &str,
    value: Option<String>,
    graph_state: Option<GraphSourceState>,
    theme: Theme,
) -> Line<'static> {
    Line::from(vec![
        metric_label(label, theme),
        Span::styled(
            value.unwrap_or_else(|| "--".to_string()),
            graph_value_style(Style::default().fg(theme.text), graph_state, theme),
        ),
    ])
}

fn metric_label(label: &str, theme: Theme) -> Span<'static> {
    Span::styled(
        format!("{label:<LABEL_WIDTH$}"),
        Style::default().fg(theme.muted),
    )
}

fn cpu_graph_state(app: &App, metric: SystemMetric) -> Option<GraphSourceState> {
    app.graph_source_state(&GraphSlot::system(metric))
}

fn format_cpu_average(value: Option<u8>) -> String {
    value
        .map(|value| format!("{:>3}%", value.min(100)))
        .unwrap_or_else(|| " --".to_string())
}

fn format_cpu_part(value: Option<u8>) -> String {
    value
        .map(|value| format!("{}%", value.min(100)))
        .unwrap_or_else(|| "--".to_string())
}

fn format_cpu_panel_frequency_mhz(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} MHz"))
        .unwrap_or_else(|| "--".to_string())
}

fn contains_point(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn snapshot_with_cores(has_e_core: bool) -> Snapshot {
        Snapshot {
            captured_at: Local::now(),
            total_memory: 0,
            used_memory: 0,
            available_memory: None,
            modified_memory: None,
            standby_memory: None,
            free_zeroed_memory: None,
            committed_memory: None,
            commit_limit: None,
            paged_pool_memory: None,
            nonpaged_pool_memory: None,
            pages_input_per_sec: None,
            pages_output_per_sec: None,
            cpu_name: None,
            cpu_frequency_mhz: Some(2_400),
            cpu_current_frequency_mhz: Some(3_000),
            cpu_p_core_frequency_mhz: Some(3_200),
            cpu_e_core_frequency_mhz: Some(1_800),
            cpu_total_usage_percent: Some(42),
            cpu_user_usage_percent: Some(31),
            cpu_kernel_usage_percent: Some(11),
            cpu_logical_processors: has_e_core
                .then_some(crate::model::CpuLogicalProcessorSample {
                    usage_percent: 10,
                    kind: Some(CpuCoreKind::Efficiency),
                })
                .into_iter()
                .collect(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_adapters: Vec::new(),
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            process_count: 214,
            thread_count: Some(4_335),
            processes: Vec::new(),
        }
    }

    #[test]
    fn cpu_percent_formats_are_compact() {
        assert_eq!(format_cpu_average(Some(42)), " 42%");
        assert_eq!(format_cpu_part(Some(31)), "31%");
        assert_eq!(format_cpu_part(None), "--");
    }

    #[test]
    fn cpu_frequency_omits_e_segment_without_e_cores() {
        let without_e = cpu_frequency_line(&snapshot_with_cores(false), crate::ui::THEMES[0]);
        let with_e = cpu_frequency_line(&snapshot_with_cores(true), crate::ui::THEMES[0]);
        let join = |line: Line<'_>| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };

        assert_eq!(join(without_e), "Freq(P/E)  3200 MHz");
        assert_eq!(join(with_e), "Freq(P/E)  3200 MHz / 1800 MHz");
    }
}
