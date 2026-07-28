use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::PROCESS_INFO_CONTENT_ROWS,
    model::{InfoValue, ProcessInfo, ProcessRow},
    ui::{Theme, widgets::scrollable_modal::ScrollableModal},
};

const PROCESS_INFO_MODAL: ScrollableModal =
    ScrollableModal::new("Process Info", 116, PROCESS_INFO_CONTENT_ROWS as u16, 2);
const CLOSE_BUTTON: &str = "[ Close ]";

pub(crate) fn draw_process_info_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let modal_layout = PROCESS_INFO_MODAL.layout(area);
    let layout = PROCESS_INFO_MODAL.render(
        frame,
        area,
        Text::from(process_info_lines(app, modal_layout.content.width, theme)),
        app.process_info_scroll.offset,
        false,
        theme,
    );

    if layout.footer.height >= 2 {
        let shortcuts = Line::from(vec![
            Span::styled(
                "Esc/Enter",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" close", Style::default().fg(theme.text)),
        ]);
        frame.render_widget(
            Paragraph::new(shortcuts).style(Style::default().bg(theme.panel)),
            Rect::new(layout.footer.x, layout.footer.y, layout.footer.width, 1),
        );
    }

    if let Some(area) = process_info_close_button_area_in_layout(layout.footer) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                CLOSE_BUTTON,
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))),
            area,
        );
    }
}

pub(crate) fn process_info_close_button_area_for_screen(area: Rect) -> Option<Rect> {
    process_info_close_button_area_in_layout(PROCESS_INFO_MODAL.layout(area).footer)
}

pub(crate) fn process_info_content_area_for_screen(area: Rect) -> Rect {
    PROCESS_INFO_MODAL.layout(area).content
}

pub(crate) fn process_info_page_size_for_screen(area: Rect) -> usize {
    PROCESS_INFO_MODAL.page_size(area)
}

fn process_info_lines(app: &App, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let info = app.process_info_for_selected();
    let process = app.process_info_target_process();
    let mut lines = static_process_info_lines(info, process, width, theme);
    lines.push(Line::default());

    let Some(metrics) = app.process_info_metrics_view() else {
        lines.push(Line::from(Span::styled(
            "Metrics --",
            Style::default().fg(theme.muted),
        )));
        return lines;
    };
    lines.push(Line::from(Span::styled(
        metrics.range,
        Style::default().fg(theme.accent),
    )));
    lines.push(metric_header_line(
        metrics.value_heading,
        metrics.delta_heading,
        theme,
    ));
    for row in metrics.rows {
        lines.push(metric_value_line(
            row.label,
            &row.value,
            row.delta.as_deref(),
            theme,
        ));
    }
    lines
}

fn static_process_info_lines(
    info: Option<&ProcessInfo>,
    process: Option<&ProcessRow>,
    width: u16,
    theme: Theme,
) -> Vec<Line<'static>> {
    let process_identity = info
        .map(format_process_identity)
        .or_else(|| process.map(|row| format!("{} / PID {}", row.name, row.pid)))
        .unwrap_or_else(|| "--".to_string());
    let parent = info
        .map(|info| value_text(&info.parent_process))
        .unwrap_or_else(|| "--".to_string());
    let started = info
        .map(format_process_started)
        .or_else(|| process.map(format_recorded_process_started))
        .unwrap_or_else(|| "--".to_string());
    let executable = info
        .map(|info| value_text(&info.executable))
        .or_else(|| process.and_then(|row| row.executable_path.clone()))
        .unwrap_or_else(|| "--".to_string());
    let command = info
        .map(|info| value_text(&info.command_line))
        .unwrap_or_else(|| "--".to_string());
    let file = info
        .map(format_process_file)
        .unwrap_or_else(|| "--".to_string());
    [
        ("Process", process_identity),
        ("Parent", parent),
        ("Started", started),
        ("Executable", executable),
        ("Command", command),
        ("File", file),
    ]
    .into_iter()
    .map(|(label, value)| render_process_info_line(label, &value, width, theme))
    .collect()
}

fn render_process_info_line(title: &str, value: &str, width: u16, theme: Theme) -> Line<'static> {
    let label_width = 11usize;
    let value_width = (width as usize).saturating_sub(label_width);
    Line::from(vec![
        Span::styled(format!("{title:<10} "), Style::default().fg(theme.muted)),
        Span::styled(
            truncate_start(value, value_width),
            Style::default().fg(theme.text),
        ),
    ])
}

fn metric_header_line(
    value_heading: &str,
    delta_heading: Option<&str>,
    theme: Theme,
) -> Line<'static> {
    let header_style = Style::default()
        .fg(theme.text)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut spans = vec![Span::styled("Metrics", header_style)];
    push_header_padding(&mut spans, "Metrics", 18);
    push_right_aligned_header(&mut spans, value_heading, 20, header_style);
    if let Some(delta_heading) = delta_heading {
        push_right_aligned_header(&mut spans, delta_heading, 24, header_style);
    }
    Line::from(spans)
}

fn push_header_padding(spans: &mut Vec<Span<'static>>, heading: &str, width: usize) {
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(heading.chars().count())),
    ));
}

fn push_right_aligned_header(
    spans: &mut Vec<Span<'static>>,
    heading: &str,
    width: usize,
    style: Style,
) {
    push_header_padding(spans, heading, width);
    spans.push(Span::styled(heading.to_string(), style));
}

fn metric_value_line(label: &str, value: &str, delta: Option<&str>, theme: Theme) -> Line<'static> {
    let text = match delta {
        Some(delta) => format!("{label:<18}{value:>20}{:>24}", format!("({delta})")),
        None => format!("{label:<18}{value:>20}"),
    };
    Line::from(Span::styled(text, Style::default().fg(theme.text)))
}

fn format_process_identity(info: &ProcessInfo) -> String {
    format!("{} / PID {}", info.name, info.pid)
}

fn format_process_started(info: &ProcessInfo) -> String {
    let Some(start_time) = info.start_time else {
        return "--".to_string();
    };
    let Some(started_utc) = chrono::DateTime::from_timestamp(start_time as i64, 0) else {
        return start_time.to_string();
    };
    let started = started_utc.with_timezone(&chrono::Local);
    let uptime = chrono::Local::now()
        .signed_duration_since(started)
        .max(chrono::Duration::zero());
    format!(
        "{} / Uptime {}",
        started.format("%Y-%m-%d %H:%M:%S"),
        format_duration(uptime)
    )
}

fn format_recorded_process_started(process: &ProcessRow) -> String {
    let Some(start_time) = process.start_time else {
        return "--".to_string();
    };
    chrono::DateTime::from_timestamp(start_time as i64, 0)
        .map(|started| {
            started
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| start_time.to_string())
}

fn format_process_file(info: &ProcessInfo) -> String {
    format!(
        "Modified {} / Size {} / Product {}",
        info.file_modified.text(),
        info.file_size.text(),
        info.product_version.text()
    )
}

fn value_text(value: &InfoValue) -> String {
    value.text().to_string()
}

fn format_duration(duration: chrono::Duration) -> String {
    let total_seconds = duration.num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn truncate_start(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if value.chars().count() <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let tail = value
        .chars()
        .rev()
        .take(max_width.saturating_sub(3))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{tail}")
}

fn process_info_close_button_area_in_layout(footer: Rect) -> Option<Rect> {
    if footer.width < CLOSE_BUTTON.len() as u16 || footer.height < 1 {
        return None;
    }
    let width = CLOSE_BUTTON.len() as u16;
    Some(Rect::new(
        footer.x + footer.width.saturating_sub(width) / 2,
        footer.bottom().saturating_sub(1),
        width,
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_screen_keeps_content_and_close_button_separate() {
        let screen = Rect::new(0, 0, 60, 12);
        let content = process_info_content_area_for_screen(screen);
        let close = process_info_close_button_area_for_screen(screen)
            .expect("small dialog should keep a close button");

        assert!(content.bottom() <= close.y);
        assert_eq!(process_info_page_size_for_screen(screen), 8);
    }
}
