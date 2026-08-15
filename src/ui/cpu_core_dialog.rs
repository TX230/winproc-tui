use ratatui::{
    layout::Rect,
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    app::AppActivity,
    model::{CpuCoreKind, CpuLogicalProcessorSample},
    ui::{Theme, footer::shortcut_spans, widgets::scrollable_modal::ScrollableModal},
};

const FOOTER_ITEMS: [(&str, &str); 4] = [
    ("↑/↓", "Scroll"),
    ("PgUp/PgDn", "Page"),
    ("Home/End", "Edge"),
    ("Enter/Esc", "Close"),
];
const FOOTER_HEIGHT: u16 = 1;

pub(crate) fn draw_cpu_core_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let lines = cpu_core_dialog_lines(app, theme);
    let modal = cpu_core_dialog_modal(app);
    let layout = modal.render(
        frame,
        area,
        Text::from(lines),
        app.cpu_core_scroll.offset,
        false,
        theme,
    );
    if !layout.footer.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(&FOOTER_ITEMS, theme))),
            layout.footer,
        );
    }
}

pub(crate) fn cpu_core_dialog_page_size_for_screen(area: Rect, app: &App) -> usize {
    cpu_core_dialog_modal(app).page_size(area)
}

pub(crate) fn cpu_core_dialog_content_area(area: Rect, app: &App) -> Rect {
    cpu_core_dialog_modal(app).layout(area).content
}

pub(crate) fn cpu_core_dialog_scrollbar_area(
    area: Rect,
    app: &App,
    page_size: usize,
) -> Option<Rect> {
    cpu_core_dialog_modal(app).scrollbar_area(area, page_size)
}

pub(crate) fn cpu_core_dialog_total_rows(app: &App) -> usize {
    app.display_snapshot().cpu_logical_processors.len().max(1)
}

fn cpu_core_dialog_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let cores = &app.display_snapshot().cpu_logical_processors;
    if cores.is_empty() {
        let message = if app.activity() == AppActivity::LogView {
            "Per-core usage is not recorded in logs."
        } else {
            "Per-core usage is unavailable."
        };
        return vec![Line::from(Span::styled(
            message,
            ratatui::style::Style::default().fg(theme.muted),
        ))];
    }

    let index_width = cores.len().saturating_sub(1).to_string().len().max(1);
    cores
        .iter()
        .enumerate()
        .map(|(index, core)| cpu_core_line(index, index_width, *core, theme))
        .collect()
}

fn cpu_core_line(
    index: usize,
    index_width: usize,
    core: CpuLogicalProcessorSample,
    theme: Theme,
) -> Line<'static> {
    let kind = match core.kind {
        Some(CpuCoreKind::Performance) => "P",
        Some(CpuCoreKind::Efficiency) => "E",
        None => "-",
    };
    Line::from(vec![
        Span::styled(
            format!("CPU {index:>index_width$}"),
            ratatui::style::Style::default().fg(theme.text),
        ),
        Span::styled(
            format!(" ({kind})  "),
            ratatui::style::Style::default().fg(theme.muted),
        ),
        Span::styled(
            format!("{:>3}%", core.usage_percent.min(100)),
            ratatui::style::Style::default().fg(theme.text),
        ),
    ])
}

fn cpu_core_dialog_modal(app: &App) -> ScrollableModal {
    let content_height = cpu_core_dialog_total_rows(app).min(u16::MAX as usize) as u16;
    let content_width = footer_width().max(39) as u16;
    ScrollableModal::new(
        "PER-CORE CPU USAGE",
        content_width,
        content_height,
        FOOTER_HEIGHT,
    )
    .with_bold_title()
}

fn footer_width() -> usize {
    FOOTER_ITEMS
        .iter()
        .enumerate()
        .map(|(index, (key, label))| {
            usize::from(index > 0) * 2 + key.chars().count() + 1 + label.chars().count()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_cpu_row_uses_index_kind_and_percent() {
        let line = cpu_core_line(
            7,
            2,
            CpuLogicalProcessorSample {
                usage_percent: 42,
                kind: Some(CpuCoreKind::Efficiency),
            },
            crate::ui::THEMES[0],
        );
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(text, "CPU  7 (E)   42%");
    }
}
