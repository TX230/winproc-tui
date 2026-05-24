use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::ui::Theme;

pub(crate) const BUTTON_GAP_WIDTH: u16 = 3;

pub(crate) fn centered_dialog_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(crate) fn warning_dialog<'a>(
    title: &'static str,
    message: &'static str,
    detail: &'static str,
    buttons: Line<'a>,
    theme: Theme,
) -> Paragraph<'a> {
    let lines = Text::from(vec![
        Line::from(Span::styled(message, Style::default().fg(theme.text))),
        Line::from(Span::styled(detail, Style::default().fg(theme.text))),
        buttons,
    ]);

    Paragraph::new(lines)
        .block(warning_block(title, theme))
        .alignment(Alignment::Center)
}

pub(crate) fn warning_message_dialog<'a>(
    title: &'static str,
    message: &'static str,
    buttons: Line<'a>,
    theme: Theme,
) -> Paragraph<'a> {
    let lines = Text::from(vec![
        Line::from(Span::styled(message, Style::default().fg(theme.text))),
        buttons,
    ]);

    Paragraph::new(lines)
        .block(warning_block(title, theme))
        .alignment(Alignment::Center)
}

pub(crate) fn warning_block(title: &'static str, theme: Theme) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.panel))
}

pub(crate) fn button_line(buttons: &[(&'static str, bool)], theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, selected)) in buttons.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(button(label, *selected, theme));
    }
    Line::from(spans)
}

pub(crate) fn button_areas(
    content: Rect,
    row_from_content_top: u16,
    labels: &[&'static str],
) -> Vec<Rect> {
    if labels.is_empty() || row_from_content_top >= content.height {
        return Vec::new();
    }

    let total_width = labels
        .iter()
        .map(|label| button_width(label))
        .sum::<u16>()
        .saturating_add(BUTTON_GAP_WIDTH.saturating_mul(labels.len().saturating_sub(1) as u16));
    if total_width > content.width {
        return Vec::new();
    }

    let mut x = content.x + content.width.saturating_sub(total_width) / 2;
    let y = content.y.saturating_add(row_from_content_top);
    labels
        .iter()
        .map(|label| {
            let width = button_width(label);
            let area = Rect::new(x, y, width, 1);
            x = x.saturating_add(width).saturating_add(BUTTON_GAP_WIDTH);
            area
        })
        .collect()
}

pub(crate) fn right_aligned_button_areas(
    content: Rect,
    row_from_content_top: u16,
    labels: &[&'static str],
) -> Vec<Rect> {
    if labels.is_empty() || row_from_content_top >= content.height {
        return Vec::new();
    }

    let total_width = labels
        .iter()
        .map(|label| button_width(label))
        .sum::<u16>()
        .saturating_add(BUTTON_GAP_WIDTH.saturating_mul(labels.len().saturating_sub(1) as u16));
    if total_width > content.width {
        return Vec::new();
    }

    let mut x = content.right().saturating_sub(total_width);
    let y = content.y.saturating_add(row_from_content_top);
    labels
        .iter()
        .map(|label| {
            let width = button_width(label);
            let area = Rect::new(x, y, width, 1);
            x = x.saturating_add(width).saturating_add(BUTTON_GAP_WIDTH);
            area
        })
        .collect()
}

fn button_width(label: &'static str) -> u16 {
    label.chars().count().saturating_add(2) as u16
}

fn button(label: &'static str, selected: bool, theme: Theme) -> Span<'static> {
    if selected {
        Span::styled(
            format!("[{label}]"),
            Style::default()
                .fg(confirm_button_text(theme))
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!("[{label}]"),
            Style::default().fg(theme.text).bg(theme.panel_alt),
        )
    }
}

fn confirm_button_text(theme: Theme) -> Color {
    match theme.name {
        "Light" => theme.background,
        _ => Color::Black,
    }
}
