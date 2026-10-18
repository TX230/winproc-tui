use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    ui::{Theme, footer::warning_shortcut_spans, widgets::confirm_dialog},
};

const POPUP_WIDTH: u16 = 64;
const POPUP_HEIGHT: u16 = 9;

pub(crate) fn draw_process_kill_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    if app.process_kill_job.is_some() {
        draw_results(frame, area, app, theme);
        return;
    }
    let popup = process_kill_dialog_area(area);
    let pids = app
        .process_kill_targets
        .iter()
        .map(|target| target.pid)
        .collect::<Vec<_>>();
    let pid_list = compact_pid_list(&pids, 54);
    let lines = Text::from(vec![
        Line::from(Span::styled(
            "Kill Selected Processes?",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Selected rows: {}", app.process_kill_targets.len()),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            format!("PIDs: {pid_list}"),
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled("Continue?", Style::default().fg(theme.text))),
        Line::from(""),
        Line::from(warning_shortcut_spans(
            &[("Enter", "Kill"), ("Esc", "Cancel")],
            theme,
        )),
    ]);

    frame.render_widget(Clear, popup);
    crate::ui::footer::register_shortcut_text(
        app,
        popup.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        }),
        &lines,
        Alignment::Center,
    );
    let dialog = Paragraph::new(lines)
        .block(confirm_dialog::warning_block("CONFIRM", theme))
        .alignment(Alignment::Center);
    frame.render_widget(dialog, popup);
}

pub(crate) fn process_kill_dialog_area(area: Rect) -> Rect {
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn compact_pid_list(pids: &[u32], max_chars: usize) -> String {
    let joined = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let shown = joined
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{shown}+")
}

fn draw_results(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    use ratatui::{
        layout::{Constraint, Layout, Margin},
        widgets::Wrap,
    };
    let job = app.process_kill_job.as_ref().unwrap();
    let popup = process_kill_results_area(area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        confirm_dialog::warning_block(
            if job.finished() {
                "KILL RESULTS"
            } else {
                "KILL IN PROGRESS"
            },
            theme,
        ),
        popup,
    );
    let inner = popup.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let regions = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(job.summary())
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false }),
        regions[0],
    );
    let mut lines = Vec::new();
    for (index, target) in app
        .process_kill_targets
        .iter()
        .enumerate()
        .skip(app.process_kill_scroll)
    {
        lines.push(Line::from(format!("{} ({})", target.pid, target.name)));
        lines.push(Line::from(
            job.outcomes[index]
                .as_ref()
                .map_or_else(|| "Pending".into(), |outcome| outcome.text()),
        ));
        lines.push(Line::from(""));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false }),
        regions[1],
    );
    let shortcuts = if job.finished() {
        vec![
            ("↑/↓", "Scroll"),
            ("Home/End", "First/Last"),
            ("Enter/Esc", "Close"),
        ]
    } else {
        vec![("↑/↓", "Scroll"), ("Home/End", "First/Last")]
    };
    let guidance = Text::from(vec![
        Line::from(""),
        Line::from(warning_shortcut_spans(&shortcuts, theme)),
    ]);
    crate::ui::footer::register_shortcut_text(app, regions[2], &guidance, Alignment::Center);
    frame.render_widget(
        Paragraph::new(guidance).alignment(Alignment::Center),
        regions[2],
    );
}

pub(crate) fn process_kill_results_area(area: Rect) -> Rect {
    let width = area.width.min(100);
    let height = area.height.min(22);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
