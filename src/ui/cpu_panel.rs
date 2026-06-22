use ratatui::{
    layout::Rect,
    prelude::{Color, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::{FocusedPanel, GraphSlot},
    model::{CpuCoreKind, CpuLogicalProcessorSample, Snapshot, SystemMetric},
    ui::{Theme, format::format_frequency_mhz, widgets::block::panel_block_focused},
};

pub(crate) fn draw_cpu_panel(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let block = panel_block_focused("CPUs", theme, app.panel_has_focus(FocusedPanel::Cpu));
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
    cpu_panel_lines(
        &cpu_average_graph_slot_numbers(app),
        app.display_snapshot(),
        theme,
        height,
    )
}

fn cpu_panel_lines(
    cpu_average_graph_slot_numbers: &str,
    snapshot: &Snapshot,
    theme: Theme,
    height: u16,
) -> Vec<Line<'static>> {
    if height <= 1 || snapshot.cpu_logical_processors.is_empty() {
        return vec![cpu_panel_line(
            cpu_average_graph_slot_numbers,
            snapshot,
            theme,
        )];
    }

    let mut lines = vec![cpu_panel_summary_line(
        cpu_average_graph_slot_numbers,
        snapshot,
        theme,
    )];
    let bar_height = usize::from(height.saturating_sub(1).min(3));
    lines.extend(cpu_core_bar_lines(
        &snapshot.cpu_logical_processors,
        theme,
        bar_height,
    ));
    lines
}

fn cpu_panel_line(
    cpu_average_graph_slot_numbers: &str,
    snapshot: &Snapshot,
    theme: Theme,
) -> Line<'static> {
    let mut spans = cpu_panel_summary_spans(cpu_average_graph_slot_numbers, snapshot, theme);

    if snapshot.cpu_logical_processors.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("--", Style::default().fg(theme.muted)));
    } else {
        spans.push(Span::raw("  "));
        spans.extend(cpu_core_spans(&snapshot.cpu_logical_processors, theme));
    }

    Line::from(spans)
}

fn cpu_panel_summary_line(
    cpu_average_graph_slot_numbers: &str,
    snapshot: &Snapshot,
    theme: Theme,
) -> Line<'static> {
    Line::from(cpu_panel_summary_spans(
        cpu_average_graph_slot_numbers,
        snapshot,
        theme,
    ))
}

fn cpu_panel_summary_spans(
    cpu_average_graph_slot_numbers: &str,
    snapshot: &Snapshot,
    theme: Theme,
) -> Vec<Span<'static>> {
    let mut spans = vec![
        cpu_average_graph_slot_span(cpu_average_graph_slot_numbers, theme),
        Span::styled("Avg ", Style::default().fg(theme.muted)),
        Span::styled(
            format_cpu_average(snapshot.cpu_total_usage_percent),
            Style::default().fg(theme.text),
        ),
        Span::raw("  "),
    ];
    spans.extend(cpu_frequency_spans(snapshot, theme));
    spans
}

fn cpu_core_bar_lines(
    cores: &[CpuLogicalProcessorSample],
    theme: Theme,
    height: usize,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    let classified = cores.iter().any(|core| core.kind.is_some());
    (0..height)
        .map(|row| {
            let mut spans = vec![Span::styled(
                if row == 0 { "  Load " } else { "       " },
                Style::default().fg(theme.muted),
            )];
            let threshold = ((height - row - 1) * 100 / height) as u8;
            let mut previous_kind = None;
            for (index, core) in cores.iter().enumerate() {
                if classified && (index == 0 || core.kind != previous_kind) {
                    if index > 0 {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(
                        if row == 0 {
                            cpu_core_marker(core.kind)
                        } else {
                            "   "
                        },
                        Style::default().fg(theme.muted),
                    ));
                    spans.push(Span::raw(" "));
                }
                let filled = core.usage_percent >= threshold;
                spans.push(Span::styled(
                    if filled { "█" } else { " " },
                    if filled {
                        cpu_usage_style(core.usage_percent)
                    } else {
                        Style::default().fg(theme.muted)
                    },
                ));
                previous_kind = core.kind;
            }
            Line::from(spans)
        })
        .collect()
}

fn cpu_average_graph_slot_span(numbers: &str, theme: Theme) -> Span<'static> {
    let label = format!("{numbers:<2}");
    let style = if numbers.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.text).bg(theme.warning)
    };
    Span::styled(label, style)
}

fn cpu_average_graph_slot_numbers(app: &App) -> String {
    app.graph_slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| match slot.as_ref() {
            Some(GraphSlot::System {
                metric: SystemMetric::CpuAverage,
            }) => Some((index + 1).to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn cpu_frequency_spans(snapshot: &Snapshot, theme: Theme) -> Vec<Span<'static>> {
    match (
        snapshot.cpu_p_core_frequency_mhz,
        snapshot.cpu_e_core_frequency_mhz,
    ) {
        (Some(p_frequency), Some(e_frequency)) => vec![
            Span::styled("P-core ", Style::default().fg(theme.muted)),
            Span::styled(
                format_frequency_mhz(Some(p_frequency)),
                Style::default().fg(theme.text),
            ),
            Span::raw("  "),
            Span::styled("E-core ", Style::default().fg(theme.muted)),
            Span::styled(
                format_frequency_mhz(Some(e_frequency)),
                Style::default().fg(theme.text),
            ),
        ],
        _ => vec![
            Span::styled("Clock ", Style::default().fg(theme.muted)),
            Span::styled(
                format_frequency_mhz(snapshot.cpu_current_frequency_mhz),
                Style::default().fg(theme.text),
            ),
        ],
    }
}

fn cpu_core_spans(cores: &[CpuLogicalProcessorSample], theme: Theme) -> Vec<Span<'static>> {
    let classified = cores.iter().any(|core| core.kind.is_some());
    let mut previous_kind = None;
    let mut spans = Vec::new();

    for (index, core) in cores.iter().enumerate() {
        if classified && (index == 0 || core.kind != previous_kind) {
            if index > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                cpu_core_marker(core.kind),
                Style::default().fg(theme.muted),
            ));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format_cpu_core_bar(core.usage_percent),
            cpu_usage_style(core.usage_percent),
        ));
        previous_kind = core.kind;
    }

    spans
}

fn format_cpu_average(value: Option<u8>) -> String {
    value
        .map(|value| format!("{}%", value.min(100)))
        .unwrap_or_else(|| "--".to_string())
}

fn format_cpu_core_bar(value: u8) -> &'static str {
    const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let index = usize::from(value.min(99)) * BARS.len() / 100;
    BARS[index]
}

fn cpu_core_marker(kind: Option<CpuCoreKind>) -> &'static str {
    match kind {
        Some(CpuCoreKind::Performance) => "(P)",
        Some(CpuCoreKind::Efficiency) => "(E)",
        None => "(CPU)",
    }
}

fn cpu_usage_color(value: u8) -> Color {
    let value = value.min(99) as u16;
    let red = 56 + (184 * value / 99);
    let green = 196 - (136 * value / 99);
    Color::Rgb(red as u8, green as u8, 80)
}

fn cpu_usage_style(value: u8) -> Style {
    Style::default().fg(cpu_usage_color(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn snapshot_with_cores(cores: Vec<CpuLogicalProcessorSample>) -> Snapshot {
        Snapshot {
            captured_at: Local::now(),
            total_memory: 0,
            used_memory: 0,
            committed_memory: None,
            commit_limit: None,
            gpu_dedicated_used: None,
            gpu_dedicated_total: None,
            gpu_shared_used: None,
            gpu_shared_total: None,
            cpu_name: None,
            cpu_frequency_mhz: Some(2_400),
            cpu_current_frequency_mhz: Some(3_000),
            cpu_p_core_frequency_mhz: Some(3_200),
            cpu_e_core_frequency_mhz: Some(1_800),
            cpu_total_usage_percent: Some(42),
            cpu_logical_processors: cores,
            cpu_topology: None,
            cpu_cache: None,
            gpu_name: None,
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            process_count: 0,
            processes: Vec::new(),
        }
    }

    #[test]
    fn cpu_core_bar_uses_eight_load_levels() {
        assert_eq!(format_cpu_core_bar(0), "▁");
        assert_eq!(format_cpu_core_bar(12), "▁");
        assert_eq!(format_cpu_core_bar(13), "▂");
        assert_eq!(format_cpu_core_bar(50), "▅");
        assert_eq!(format_cpu_core_bar(87), "▇");
        assert_eq!(format_cpu_core_bar(99), "█");
        assert_eq!(format_cpu_core_bar(100), "█");
    }

    #[test]
    fn cpu_core_spans_mark_p_and_e_boundaries() {
        let snapshot = snapshot_with_cores(vec![
            CpuLogicalProcessorSample {
                usage_percent: 1,
                kind: Some(CpuCoreKind::Performance),
            },
            CpuLogicalProcessorSample {
                usage_percent: 22,
                kind: Some(CpuCoreKind::Performance),
            },
            CpuLogicalProcessorSample {
                usage_percent: 99,
                kind: Some(CpuCoreKind::Efficiency),
            },
        ]);

        let text = cpu_panel_line("", &snapshot, crate::ui::THEMES[0])
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("Avg 42%"), "{text}");
        assert!(text.contains("P-core 3.20 GHz"), "{text}");
        assert!(text.contains("E-core 1.80 GHz"), "{text}");
        assert!(text.contains("(P) ▁▂ (E) █"), "{text}");
    }

    #[test]
    fn cpu_panel_fallback_clock_uses_current_frequency() {
        let mut snapshot = snapshot_with_cores(Vec::new());
        snapshot.cpu_frequency_mhz = Some(2_400);
        snapshot.cpu_current_frequency_mhz = Some(3_000);
        snapshot.cpu_p_core_frequency_mhz = None;
        snapshot.cpu_e_core_frequency_mhz = None;

        let text = cpu_panel_line("", &snapshot, crate::ui::THEMES[0])
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("Clock 3.00 GHz"), "{text}");
        assert!(!text.contains("Clock 2.40 GHz"), "{text}");
    }

    #[test]
    fn cpu_core_spans_use_foreground_color_for_usage_cells() {
        let snapshot = snapshot_with_cores(vec![CpuLogicalProcessorSample {
            usage_percent: 77,
            kind: None,
        }]);

        let line = cpu_panel_line("", &snapshot, crate::ui::THEMES[0]);
        let usage_span = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "▇")
            .expect("usage span");

        assert_eq!(usage_span.style.bg, None);
        assert_eq!(usage_span.style.fg, Some(cpu_usage_color(77)));
    }

    #[test]
    fn cpu_core_bar_lines_show_low_usage_cores() {
        let snapshot = snapshot_with_cores(vec![
            CpuLogicalProcessorSample {
                usage_percent: 1,
                kind: None,
            },
            CpuLogicalProcessorSample {
                usage_percent: 22,
                kind: None,
            },
            CpuLogicalProcessorSample {
                usage_percent: 99,
                kind: None,
            },
        ]);

        let text = cpu_panel_lines("", &snapshot, crate::ui::THEMES[0], 4)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Load   █"), "{text}");
        assert!(text.contains("       ███"), "{text}");
    }
}
