use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::file_users::{FileUsersFocus, FileUsersView},
    model::file_users::FileSearchEnd,
    ui::{
        Theme,
        widgets::{
            block::{modal_block_focused, modal_title},
            scrollable_modal::{ScrollableModal, ScrollableModalLayout},
        },
    },
};

const BROWSER: ScrollableModal =
    ScrollableModal::new("PROCESSES USING FILE", u16::MAX, u16::MAX, 2);

pub(crate) fn browser_layout(screen: Rect) -> ScrollableModalLayout {
    BROWSER.layout(super::screen_layout(screen)[1])
}

pub(crate) struct ContentLayout {
    pub(crate) query: Rect,
    pub(crate) mode: Rect,
    status: Rect,
    progress: Rect,
    notice: Rect,
    header: Rect,
    pub(crate) rows: Rect,
}

pub(crate) fn content_layout(area: Rect) -> ContentLayout {
    let row = |offset: u16| {
        Rect::new(
            area.x,
            area.y.saturating_add(offset).min(area.bottom()),
            area.width,
            u16::from(offset < area.height),
        )
    };
    ContentLayout {
        query: row(0),
        mode: row(1),
        status: row(2),
        progress: row(3),
        notice: row(4),
        header: row(5),
        rows: Rect::new(
            area.x,
            area.y.saturating_add(6).min(area.bottom()),
            area.width,
            area.height.saturating_sub(6),
        ),
    }
}

pub(crate) fn scrollbar_area(area: Rect, view: &FileUsersView) -> Option<Rect> {
    let rows = if view.detail {
        area
    } else {
        content_layout(area).rows
    };
    let total = if view.detail {
        detail_lines(view, area.width).len()
    } else {
        view.report.matches.len()
    };
    (rows.width > 0 && rows.height > 0 && total > rows.height as usize)
        .then(|| Rect::new(rows.right() - 1, rows.y, 1, rows.height))
}

fn status(view: &FileUsersView) -> String {
    let state = if view.verifying {
        "Verifying owner"
    } else if view.pending.is_some() {
        "Searching"
    } else {
        view.report
            .end
            .as_ref()
            .map_or("Ready", FileSearchEnd::label)
    };
    let partial = view.report.progress.incomplete()
        || view
            .report
            .end
            .as_ref()
            .is_some_and(|end| *end != FileSearchEnd::Complete);
    format!(
        "{state}{} · {} matches",
        if partial { " (partial results)" } else { "" },
        view.report.matches.len()
    )
}

pub(crate) fn detail_lines(view: &FileUsersView, width: u16) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(entry) = view.selected_entry() {
        lines.extend([
            entry.path.clone(),
            format!("PID: {}  Process: {}", entry.owner.pid, entry.owner.name),
            format!("Matched handles: {}", entry.handle_count),
            String::new(),
        ]);
    }
    lines.push(status(view));
    if let Some(query) = &view.searched {
        lines.push(format!("{}: {}", query.mode.label(), query.text));
    }
    let p = &view.report.progress;
    lines.extend([
        format!("Processes in handle table: {}", p.total_processes),
        format!("Processes inspected: {}", p.inspected_processes),
        format!("Access denied for {} processes", p.denied_processes),
        format!("Processes exited during scan: {}", p.exited_processes),
        format!(
            "Processes unavailable or unverified: {}",
            p.unavailable_processes
        ),
        format!("Handles in table: {}", p.total_handles),
        format!("Handles inspected: {}", p.inspected_handles),
        format!("Handles unreadable: {}", p.unreadable_handles),
        format!(
            "Disk-file handles without a readable path: {}",
            p.unnamed_disk_handles
        ),
        format!(
            "Handles skipped with unavailable processes: {}",
            p.skipped_handles
        ),
        format!(
            "Handles not reached: {}",
            p.total_handles
                .saturating_sub(p.inspected_handles + p.skipped_handles)
        ),
    ]);
    if let Some(FileSearchEnd::Failed(error)) = &view.report.end {
        lines.push(format!("Error: {error}"));
    }
    if let Some(warning) = &view.report.cleanup_warning {
        lines.push(warning.clone());
    }
    lines.push(
        "Limits: 30s overall, 5s without progress, 500,000 inspected handles, 5,000 result rows or 8 MiB."
            .into(),
    );
    lines.push("Coverage: disk-file handles only. Memory-mapped-only use and file aliases are not resolved. An open handle does not necessarily block deletion.".into());
    lines
        .into_iter()
        .flat_map(|line| {
            super::process_info_dialog::wrap_display_width(
                &line,
                width.saturating_sub(1).max(1) as usize,
            )
        })
        .collect()
}

fn fit(text: &str, width: usize, tail: bool) -> String {
    let text = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    if Span::raw(&text).width() <= width {
        return format!("{text}{}", " ".repeat(width - Span::raw(&text).width()));
    }
    if width == 0 {
        return String::new();
    }
    let mut chars = text.chars().collect::<Vec<_>>();
    if tail {
        chars.reverse();
    }
    let mut used = 0;
    let mut kept = chars
        .into_iter()
        .take_while(|ch| {
            used += Span::raw(ch.to_string()).width();
            used < width
        })
        .collect::<Vec<_>>();
    if tail {
        kept.reverse();
    }
    let part = kept.into_iter().collect::<String>();
    let text = if tail {
        format!("…{part}")
    } else {
        format!("{part}…")
    };
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(Span::raw(&text).width()))
    )
}

fn query_window(view: &FileUsersView, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let mut start = 0;
    while Span::raw(&view.draft[start..view.cursor]).width() >= width {
        start += view.draft[start..view.cursor]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
    }
    (
        fit(&view.draft[start..], width, false),
        Span::raw(&view.draft[start..view.cursor]).width(),
    )
}

fn table_line(pid: &str, process: &str, count: &str, path: &str, width: usize) -> String {
    let pid_width = 8.min(width);
    let process_width = (width / 4)
        .clamp(8, 24)
        .min(width.saturating_sub(pid_width + 2));
    let count_width = 7.min(width.saturating_sub(pid_width + process_width + 3));
    let path_width = width.saturating_sub(pid_width + process_width + count_width + 3);
    format!(
        "{} {} {} {}",
        fit(pid, pid_width, false),
        fit(process, process_width, false),
        fit(count, count_width, false),
        fit(path, path_width, true)
    )
}

pub(crate) fn draw_browser(frame: &mut ratatui::Frame<'_>, screen: Rect, app: &App, theme: Theme) {
    let layout = browser_layout(screen);
    let view = &app.file_users;
    let area = layout.content;
    let normal = Style::default().fg(theme.text).bg(theme.panel_alt);
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        modal_block_focused(modal_title("PROCESSES USING FILE", theme), theme),
        layout.area,
    );
    if view.detail {
        let lines = detail_lines(view, area.width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .style(normal)
                .scroll((view.scroll.offset as u16, 0)),
            area,
        );
    } else {
        let content = content_layout(area);
        let control_style = |focus| {
            if view.focus == focus && !app.show_process_info_dialog {
                normal.bg(theme.focus_surface).add_modifier(Modifier::BOLD)
            } else {
                normal
            }
        };
        let (query, cursor) = query_window(view, content.query.width.saturating_sub(7) as usize);
        frame.render_widget(
            Paragraph::new(format!("Query: {query}")).style(control_style(FileUsersFocus::Query)),
            content.query,
        );
        frame.render_widget(
            Paragraph::new(format!("Mode: {}", view.mode.label()))
                .style(control_style(FileUsersFocus::Mode)),
            content.mode,
        );
        if view.focus == FileUsersFocus::Query
            && !app.show_process_info_dialog
            && content.query.width > 7
            && content.query.height > 0
        {
            frame.set_cursor_position(Position::new(
                content.query.x + 7 + cursor as u16,
                content.query.y,
            ));
        }
        frame.render_widget(Paragraph::new(status(view)).style(normal), content.status);
        let p = &view.report.progress;
        frame.render_widget(
            Paragraph::new(format!(
                "Processes {}/{}  Handles {}/{}",
                p.inspected_processes, p.total_processes, p.inspected_handles, p.total_handles
            ))
            .style(normal.fg(theme.muted)),
            content.progress,
        );
        let notice = view
            .notice
            .as_deref()
            .or(view.report.cleanup_warning.as_deref())
            .or(match &view.report.end {
                Some(FileSearchEnd::Failed(error)) => Some(error.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| {
                if view
                    .searched
                    .as_ref()
                    .is_some_and(|query| query.text != view.draft || query.mode != view.mode)
                {
                    "Query changed; start a search to update results"
                } else {
                    ""
                }
            });
        frame.render_widget(
            Paragraph::new(notice).style(normal.fg(theme.warning)),
            content.notice,
        );
        let width = content.rows.width.saturating_sub(1) as usize;
        frame.render_widget(
            Paragraph::new(table_line(
                "PID",
                "Process",
                "Handles",
                "Matched path",
                width,
            ))
            .style(
                normal
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            content.header,
        );
        if view.report.matches.is_empty() {
            let text = if view.searched.is_none() {
                "Enter a filename or path, then start a search."
            } else if view.pending.is_some() {
                "No matches yet in the inspected scope."
            } else {
                "No matches found in the inspected scope."
            };
            frame.render_widget(
                Paragraph::new(text).style(normal.fg(theme.muted)),
                content.rows,
            );
        } else {
            let rows = view
                .report
                .matches
                .iter()
                .enumerate()
                .skip(view.scroll.offset)
                .take(content.rows.height as usize)
                .map(|(i, entry)| {
                    let style = if i == view.selected {
                        normal
                            .bg(
                                if view.focus == FileUsersFocus::Results
                                    && !app.show_process_info_dialog
                                {
                                    theme.focus_surface
                                } else {
                                    theme.table_selection_surface
                                },
                            )
                            .add_modifier(Modifier::BOLD)
                    } else {
                        normal
                    };
                    Line::styled(
                        table_line(
                            &entry.owner.pid.to_string(),
                            &entry.owner.name,
                            &entry.handle_count.to_string(),
                            &entry.path,
                            width,
                        ),
                        style,
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(rows), content.rows);
        }
    }
    if let Some(bar) = scrollbar_area(area, view) {
        let total = if view.detail {
            detail_lines(view, area.width).len()
        } else {
            view.report.matches.len()
        };
        let max_offset = total.saturating_sub(bar.height as usize);
        let position = (view.scroll.offset.min(max_offset) * total.saturating_sub(1))
            .checked_div(max_offset)
            .unwrap_or(0);
        let mut state = ScrollbarState::new(total)
            .position(position)
            .viewport_content_length(bar.height as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .thumb_symbol("█")
                .track_symbol(Some("│"))
                .style(normal.fg(theme.muted)),
            bar,
            &mut state,
        );
    }
    let enter = if view.detail {
        "back"
    } else {
        match view.focus {
            FileUsersFocus::Query => "search",
            FileUsersFocus::Mode => "mode",
            FileUsersFocus::Results => "info",
        }
    };
    let escape = if view.detail {
        "back"
    } else if view.pending.is_some() {
        "cancel"
    } else {
        "close"
    };
    let first = super::footer::shortcut_spans(
        &[
            ("Enter", enter),
            ("Esc", escape),
            ("Tab", "focus"),
            ("Ctrl+U", "search"),
        ],
        theme,
    );
    let second = if view.detail {
        super::footer::shortcut_spans(
            &[("↑/↓", "scroll"), ("PgUp/PgDn", "page"), ("Ctrl+C", "copy")],
            theme,
        )
    } else if view.focus == FileUsersFocus::Query {
        super::footer::shortcut_spans(&[("←/→", "cursor"), ("Backspace/Delete", "edit")], theme)
    } else if view.focus == FileUsersFocus::Mode {
        super::footer::shortcut_spans(
            &[("/", "query"), ("←/→", "mode"), ("Tab", "results")],
            theme,
        )
    } else {
        super::footer::shortcut_spans(
            &[
                ("/", "query"),
                ("↑/↓", "select"),
                ("Space", "details"),
                ("Ctrl+C", "copy"),
            ],
            theme,
        )
    };
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &ratatui::text::Text::from(vec![Line::from(first.clone()), Line::from(second.clone())]),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(vec![Line::from(first), Line::from(second)]).style(normal),
        layout.footer,
    );
}
