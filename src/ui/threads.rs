use crate::{App, app::AppActivity};
use ratatui::text::{Line, Span};
pub(crate) fn lines(app: &App) -> Vec<Line<'static>> {
    if app.activity() == AppActivity::LogView {
        return vec![Line::raw(
            "This information is not included in recording logs.",
        )];
    }
    let mut lines = vec![
        Line::raw(if app.threads.pending {
            "Capturing threads...".into()
        } else if let Some(error) = &app.threads.error {
            error.clone()
        } else if let Some(report) = &app.threads.report {
            format!(
                "{} threads; {} rows have unavailable fields",
                report.rows.len(),
                report.unavailable
            )
        } else {
            "No thread capture.".into()
        }),
        Line::raw("Ideal CPU is a preference, not current CPU or affinity."),
        super::process_info_dialog::table_header(
            Line::from(vec![
                Span::raw("     TID"),
                Span::raw(format!(
                    " {:>12} {:>8} {:>10}  Name",
                    "CPU time (s)", "Base pri", "Ideal CPU"
                )),
            ]),
            super::process_info_dialog::table_has_focus(app),
            app.theme(),
        ),
    ];
    if let Some(report) = &app.threads.report {
        lines.extend(report.rows.iter().map(|row| {
            Line::raw(format!(
                "{:>8} {:>12} {:>8} {:>10}  {}",
                row.tid,
                row.cpu_seconds
                    .map_or_else(|| "--".into(), |value| format!("{value:.3}")),
                row.base_priority,
                row.ideal_cpu
                    .map_or_else(|| "--".into(), |(group, cpu)| format!("{group}:{cpu}")),
                row.name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("--")
                    .replace(['\r', '\n', '\t'], " ")
            ))
        }));
    }
    lines
}
