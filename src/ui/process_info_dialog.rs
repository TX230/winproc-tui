use ratatui::{
    layout::{Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    App,
    model::{InfoValue, ProcessInfo},
    ui::{Theme, widgets::block::panel_block_focused},
};

const DIALOG_WIDTH: u16 = 120;
const DIALOG_HEIGHT: u16 = 10;
const CLOSE_BUTTON: &str = "[ Close ]";

pub(crate) fn draw_process_info_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let popup = process_info_dialog_area(area);
    frame.render_widget(Clear, popup);
    let block = panel_block_focused("Process Info", theme, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let content_height = inner.height.saturating_sub(2);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    let lines = process_info_lines(app, content.width, theme);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(theme.panel)),
        content,
    );

    if inner.height >= 2 {
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
            Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 1),
        );
    }

    if let Some(area) = process_info_close_button_area_in_popup(popup) {
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
    process_info_close_button_area_in_popup(process_info_dialog_area(area))
}

fn process_info_dialog_area(area: Rect) -> Rect {
    centered_rect(area, DIALOG_WIDTH, DIALOG_HEIGHT)
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn process_info_lines(app: &App, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let Some(info) = app.process_info_for_selected() else {
        let value = if app.pending_process_info.is_some() || app.process_info_in_flight.is_some() {
            "Loading..."
        } else {
            "--"
        };
        return vec![render_process_info_line("Process", value, width, theme)];
    };

    [
        ("Process", format_process_identity(info)),
        ("Parent", value_text(&info.parent_process)),
        ("Started", format_process_started(info)),
        ("Executable", value_text(&info.executable)),
        ("Command", value_text(&info.command_line)),
        ("File", format_process_file(info)),
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

fn process_info_close_button_area_in_popup(popup: Rect) -> Option<Rect> {
    if popup.width < CLOSE_BUTTON.len() as u16 + 2 || popup.height < 4 {
        return None;
    }
    let inner = popup.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let width = CLOSE_BUTTON.len() as u16;
    Some(Rect::new(
        inner.x + inner.width.saturating_sub(width) / 2,
        inner.bottom().saturating_sub(1),
        width,
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_start_keeps_tail() {
        assert_eq!(
            truncate_start(r"C:\very\long\path\app.exe", 15),
            r"...path\app.exe"
        );
    }
}
