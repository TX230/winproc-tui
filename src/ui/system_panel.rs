use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::{AppActivity, FocusedPanel, InfoPanelMode},
    model::{DiskUsageSample, SystemMetric, TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY},
    ui::{
        Theme,
        format::{
            format_frequency_mhz, format_integer, format_mb, format_mb_per_sec, format_mbps,
            ratio_optional,
        },
        layout::system_panel_area_for_screen,
        widgets::block::panel_block_focused,
    },
};

pub(crate) fn draw_system_panel(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let usage_lines = memory_usage_lines(app, theme);
    let memory_width = memory_panel_width_for_lines(area.width, &usage_lines);
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(memory_width), Constraint::Min(20)])
        .split(area);

    let memory_block = panel_block_focused(
        ram_vram_title(app, theme),
        theme,
        app.panel_has_focus(FocusedPanel::System),
    );
    let memory_inner = memory_block.inner(panels[0]);
    frame.render_widget(memory_block, panels[0]);

    let left = Paragraph::new(Text::from(usage_lines)).style(Style::default().bg(theme.panel));
    frame.render_widget(left, memory_inner);

    let info_title = match app.info_panel_mode {
        InfoPanelMode::SystemActivity => "System Activity",
        InfoPanelMode::SystemInfo => "System Info",
    };
    let info_block = panel_block_focused(
        info_title,
        theme,
        app.panel_has_focus(FocusedPanel::SystemActivity),
    );
    let info_inner = info_block.inner(panels[1]);
    frame.render_widget(info_block, panels[1]);

    let info_lines = match app.info_panel_mode {
        InfoPanelMode::SystemActivity => system_activity_lines(app, theme),
        InfoPanelMode::SystemInfo => system_info_lines(app, theme),
    };

    let right = Paragraph::new(Text::from(info_lines)).style(Style::default().bg(theme.panel));
    frame.render_widget(right, info_inner);
}

fn ram_vram_title(app: &App, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "RAM/VRAM",
            Style::default()
                .fg(theme.text)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            ram_vram_samples_label(app),
            Style::default()
                .fg(theme.text)
                .bg(theme.panel_alt)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ])
}

fn ram_vram_samples_label(app: &App) -> String {
    if app.activity() == AppActivity::Playback {
        format!(
            "[Samples: {}]",
            format_integer(app.display_system_history().len() as u64)
        )
    } else {
        format!("[Max samples: {TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY}]")
    }
}

fn system_activity_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let snapshot = app.display_snapshot();
    let rows = [
        (
            SystemMetric::NetworkReceived,
            render_summary_graph_slot_value_line(
                system_metric_graph_slot_numbers(app, SystemMetric::NetworkReceived),
                "Net In",
                &format_optional_mbps(snapshot.network_received_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::NetworkSent,
            render_summary_graph_slot_value_line(
                system_metric_graph_slot_numbers(app, SystemMetric::NetworkSent),
                "Net Out",
                &format_optional_mbps(snapshot.network_sent_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskRead,
            render_summary_graph_slot_value_line(
                system_metric_graph_slot_numbers(app, SystemMetric::DiskRead),
                "Disk R",
                &format_optional_mb_per_sec(snapshot.disk_read_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskWrite,
            render_summary_graph_slot_value_line(
                system_metric_graph_slot_numbers(app, SystemMetric::DiskWrite),
                "Disk W",
                &format_optional_mb_per_sec(snapshot.disk_write_bytes_per_sec),
                theme,
            ),
        ),
        (
            SystemMetric::DiskQueueLength,
            render_summary_graph_slot_value_line(
                system_metric_graph_slot_numbers(app, SystemMetric::DiskQueueLength),
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
                line.style(Style::default().bg(theme.highlight))
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
            &format_gpu_summary(
                snapshot.gpu_name.as_deref().unwrap_or("--"),
                snapshot.gpu_dedicated_total,
            ),
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
    slot_numbers: Option<String>,
    label: &'static str,
    value: &str,
    theme: Theme,
) -> Line<'static> {
    Line::from(vec![
        graph_slot_prefix_span(slot_numbers, theme),
        Span::styled(format!("{label:<8}"), Style::default().fg(theme.muted)),
        Span::styled(
            value.to_string(),
            Style::default()
                .fg(theme.text)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ])
}

fn graph_slot_prefix_span(graph_slot_numbers: Option<String>, theme: Theme) -> Span<'static> {
    let label = graph_slot_numbers.unwrap_or_default();
    Span::styled(
        format!("{label:<2}"),
        if label.is_empty() {
            Style::default().fg(theme.muted)
        } else {
            Style::default()
                .fg(ratatui::prelude::Color::Rgb(112, 74, 0))
                .bg(theme.warning)
                .add_modifier(ratatui::style::Modifier::BOLD)
        },
    )
}

pub(crate) fn ram_vram_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    let usage_lines = memory_usage_lines(app, app.theme());
    let memory_width = memory_panel_width_for_lines(area.width, &usage_lines);
    Rect::new(area.x, area.y, memory_width.min(area.width), area.height)
}

pub(crate) fn system_activity_panel_area_for_screen(screen_area: Rect, app: &App) -> Rect {
    let area = system_panel_area_for_screen(screen_area);
    let usage_lines = memory_usage_lines(app, app.theme());
    let memory_width = memory_panel_width_for_lines(area.width, &usage_lines).min(area.width);
    Rect::new(
        area.x.saturating_add(memory_width),
        area.y,
        area.width.saturating_sub(memory_width),
        area.height,
    )
}

fn memory_usage_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let snapshot = app.display_snapshot();
    let mut rows = vec![
        (
            Some(SystemMetric::PhysicalMemory),
            render_summary_graph_slot_line(
                system_metric_graph_slot_numbers(app, SystemMetric::PhysicalMemory),
                "Physical Memory",
                Some(snapshot.used_memory),
                Some(snapshot.total_memory),
                None,
                theme,
            ),
        ),
        (
            Some(SystemMetric::Committed),
            render_summary_graph_slot_line(
                system_metric_graph_slot_numbers(app, SystemMetric::Committed),
                "Committed",
                snapshot.committed_memory,
                snapshot.commit_limit,
                None,
                theme,
            ),
        ),
        (
            Some(SystemMetric::GpuDedicated),
            render_summary_graph_slot_line(
                system_metric_graph_slot_numbers(app, SystemMetric::GpuDedicated),
                "GPU Dedicated",
                snapshot.gpu_dedicated_used,
                snapshot.gpu_dedicated_total,
                None,
                theme,
            ),
        ),
        (
            Some(SystemMetric::GpuShared),
            render_summary_graph_slot_line(
                system_metric_graph_slot_numbers(app, SystemMetric::GpuShared),
                "GPU Shared",
                snapshot.gpu_shared_used,
                snapshot.gpu_shared_total,
                None,
                theme,
            ),
        ),
    ];
    let separator_width = rows
        .iter()
        .map(|(_, line)| line_width(line))
        .max()
        .unwrap_or(1)
        .max(1);
    rows.insert(
        2,
        (
            None,
            Line::from(Span::styled(
                "─".repeat(separator_width),
                Style::default().fg(theme.border),
            )),
        ),
    );

    let selected_metric = app.selected_system_metric();
    rows.into_iter()
        .map(|(metric, line)| {
            if app.panel_has_focus(FocusedPanel::System) && metric == Some(selected_metric) {
                line.style(Style::default().bg(theme.highlight))
            } else {
                line
            }
        })
        .collect()
}

fn system_metric_graph_slot_numbers(app: &App, metric: SystemMetric) -> Option<String> {
    let numbers = app
        .graph_slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            slot.as_ref()
                .is_some_and(|slot| slot.system_metric() == Some(metric))
                .then(|| char::from(b'1' + index as u8))
        })
        .collect::<String>();
    (!numbers.is_empty()).then_some(numbers)
}

fn memory_panel_width_for_lines(area_width: u16, lines: &[Line<'_>]) -> u16 {
    let content_width = lines.iter().map(line_width).max().unwrap_or(24) as u16;
    let desired = content_width.saturating_add(2);
    let min_width = 28.min(area_width.max(1));
    let max_width = area_width.saturating_sub(24).max(min_width);
    desired.max(min_width).min(max_width).min(area_width)
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
    value.map(format_mbps).unwrap_or_else(|| "--".to_string())
}

fn format_optional_mb_per_sec(value: Option<u64>) -> String {
    value
        .map(format_mb_per_sec)
        .unwrap_or_else(|| "--".to_string())
}

fn format_optional_queue_length(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.1}"))
        .unwrap_or_else(|| "--".to_string())
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
}

pub(crate) fn render_summary_line(
    title: &str,
    used: Option<u64>,
    total: Option<u64>,
    suffix: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    let ratio_value = ratio_optional(used, total);
    let stats = match (used, total) {
        (Some(used), Some(total)) => format!("{} / {}", format_mb(used), format_mb(total)),
        (Some(used), None) => format_mb(used),
        (None, Some(total)) => format!("-- / {}", format_mb(total)),
        (None, None) => "--".to_string(),
    };
    let suffix_text = suffix.unwrap_or("").to_string();
    let mut spans = vec![Span::styled(
        format!("{title:<16} "),
        Style::default().fg(theme.muted),
    )];
    spans.push(Span::styled(stats, Style::default().fg(theme.text)));
    if let Some(ratio_value) = ratio_value {
        spans.push(Span::styled(
            format!(" ({:>3.0}%)", ratio_value * 100.0),
            Style::default().fg(theme.text),
        ));
    }
    if !suffix_text.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(suffix_text, Style::default().fg(theme.muted)));
    }
    Line::from(spans)
}

fn render_summary_graph_slot_line(
    graph_slot_numbers: Option<String>,
    title: &str,
    used: Option<u64>,
    total: Option<u64>,
    suffix: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    let label = graph_slot_numbers.unwrap_or_default();
    let mut spans = vec![Span::styled(
        format!("{label:<2}"),
        if label.is_empty() {
            Style::default().fg(theme.muted)
        } else {
            Style::default()
                .fg(ratatui::prelude::Color::Rgb(112, 74, 0))
                .bg(theme.warning)
                .add_modifier(ratatui::style::Modifier::BOLD)
        },
    )];
    spans.extend(render_summary_line(title, used, total, suffix, theme).spans);
    Line::from(spans)
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
