use crate::{
    App,
    app::{AppActivity, ProcessInfoFocus},
    model::scheduling::{PriorityClass, PriorityOutcome},
    ui::Theme,
};
use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Priority(usize),
    Affinity(usize),
}

pub(crate) struct Cell {
    pub(crate) control: Control,
    pub(crate) area: Rect,
    text: String,
}
pub(crate) struct SchedulingLayout {
    pub(crate) cells: Vec<Cell>,
    affinity_heading: usize,
    summary_start: usize,
}

/// Content-relative cells are shared by rendering, keyboard navigation and hit testing.
pub(crate) fn layout(app: &App, width: u16) -> SchedulingLayout {
    let width = width.saturating_sub(1).max(1);
    let mut cells = Vec::new();
    let (mut x, mut y) = (9u16.min(width), 1u16);
    for (index, class) in PriorityClass::EDITABLE.into_iter().enumerate() {
        let text = format!(
            "({}) {}",
            if app.scheduling.report.as_ref().and_then(|r| r.current) == Some(class) {
                '●'
            } else {
                ' '
            },
            class.label()
        );
        let cell_width = Line::from(text.as_str()).width() as u16;
        if x > 0 && x.saturating_add(cell_width) > width {
            x = 0;
            y += 1;
        }
        cells.push(Cell {
            control: Control::Priority(index),
            area: Rect::new(x, y, cell_width.min(width), 1),
            text,
        });
        x = x.saturating_add(cell_width).saturating_add(2);
    }
    let affinity_heading = usize::from(y) + 2;
    let cpus = app
        .scheduling
        .affinity
        .as_ref()
        .map(|r| r.cpus())
        .unwrap_or_default();
    let labels: Vec<_> = cpus
        .iter()
        .map(|cpu| {
            let kind = match app
                .scheduling
                .affinity
                .as_ref()
                .and_then(|r| r.kinds.get(*cpu))
                .copied()
                .flatten()
            {
                Some(crate::model::CpuCoreKind::Performance) => "P",
                Some(crate::model::CpuCoreKind::Efficiency) => "E",
                None => "Unknown",
            };
            format!(
                "[{}] CPU {cpu:2} ({kind})",
                if app
                    .scheduling
                    .affinity
                    .as_ref()
                    .and_then(|r| r.current)
                    .unwrap_or(0)
                    & (1usize << cpu)
                    != 0
                {
                    'x'
                } else {
                    ' '
                }
            )
        })
        .collect();
    let cell_width = labels
        .iter()
        .map(|s| Line::from(s.as_str()).width() as u16 + 2)
        .max()
        .unwrap_or(1)
        .min(width);
    let columns = usize::from((width / cell_width).max(1));
    let grid_start = affinity_heading + 1;
    for (index, text) in labels.into_iter().enumerate() {
        cells.push(Cell {
            control: Control::Affinity(index),
            area: Rect::new(
                (index % columns) as u16 * cell_width,
                (grid_start + index / columns) as u16,
                cell_width,
                1,
            ),
            text,
        });
    }
    SchedulingLayout {
        cells,
        affinity_heading,
        summary_start: grid_start + cpus.len().div_ceil(columns) + 1,
    }
}

pub(crate) fn control_at(app: &App, area: Rect, x: u16, y: u16) -> Option<Control> {
    if !area.contains((x, y).into()) || app.activity() == AppActivity::LogView {
        return None;
    }
    let row = app.scheduling.scroll.offset + usize::from(y - area.y);
    layout(app, area.width)
        .cells
        .into_iter()
        .find(|cell| cell.area.contains((x - area.x, row as u16).into()))
        .map(|cell| cell.control)
}

pub(crate) fn lines(app: &App, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let width = width.saturating_sub(1).max(1);
    let wrap = |text: &str| {
        super::process_info_dialog::wrap_display_width(text, width as usize)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>()
    };
    if app.activity() == AppActivity::LogView {
        return wrap("This information is not included in recording logs.");
    }
    let geometry = layout(app, width.saturating_add(1));
    let mut result = vec![Line::default(); geometry.summary_start];
    result[1] = Line::from(Span::styled(
        "Priority ",
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    ));
    result[geometry.affinity_heading] = Line::from(vec![
        Span::styled(
            "Affinity",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" (Group 0)"),
    ]);
    for cell in geometry.cells {
        let line = &mut result[cell.area.y as usize];
        let gap = usize::from(cell.area.x).saturating_sub(line.width());
        line.spans.push(Span::raw(" ".repeat(gap)));
        let focused = match cell.control {
            Control::Priority(index) => {
                !app.scheduling.affinity_focused && index == app.scheduling.selected
            }
            Control::Affinity(index) => {
                app.scheduling.affinity_focused && index == app.scheduling.affinity_selected
            }
        } && app.process_info_focus == ProcessInfoFocus::Content;
        let style = if focused {
            Style::default()
                .fg(theme.text)
                .bg(theme.focus_surface)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        line.spans.push(Span::styled(cell.text, style));
    }
    let priority = app.scheduling.report.as_ref();
    let affinity = app.scheduling.affinity.as_ref();
    if let Some(current) = priority.and_then(|r| r.current).filter(|c| !c.editable()) {
        result.extend(wrap(&format!(
            "Priority: {} (not selectable)",
            current.label()
        )));
    }
    for outcome in [priority.map(|r| &r.outcome), affinity.map(|r| &r.outcome)] {
        let message = match outcome {
            Some(PriorityOutcome::Failed(error) | PriorityOutcome::AppliedUnverified(error)) => {
                Some(error.clone())
            }
            _ => None,
        };
        if let Some(message) = message {
            result.extend(wrap(&message));
        }
    }
    for message in [
        app.scheduling_disabled_reason(),
        app.scheduling.notice.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        result.extend(
            wrap(message)
                .into_iter()
                .map(|line| line.style(Style::default().fg(theme.warning))),
        );
    }
    result
}
