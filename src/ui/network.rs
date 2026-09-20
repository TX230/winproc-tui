use ratatui::{
    layout::{Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::{AppActivity, ProcessInfoFocus, network::NetworkView},
    model::network::NetworkEndpoint,
    ui::{
        Theme,
        widgets::{
            block::{modal_block_focused, modal_title},
            scrollable_modal::{ScrollableModal, ScrollableModalLayout},
        },
    },
};

const BROWSER: ScrollableModal = ScrollableModal::new("NETWORK ENDPOINTS", u16::MAX, u16::MAX, 2);

pub(crate) fn browser_layout(screen: Rect) -> ScrollableModalLayout {
    let mut layout = BROWSER.layout(super::screen_layout(screen)[1]);
    if layout.content.width >= 236 {
        layout.content.width -= 52;
    }
    layout
}

pub(crate) fn active_content_area(screen: Rect, global: bool) -> Rect {
    if global {
        browser_layout(screen).content
    } else {
        super::process_info_content_area_for_screen(screen)
    }
}

pub(crate) struct ContentLayout {
    pub(crate) status: Rect,
    pub(crate) filter: Rect,
    pub(crate) mode: Rect,
    pub(crate) notice: Rect,
    pub(crate) header: Rect,
    pub(crate) rows: Rect,
}

pub(crate) fn content_layout(area: Rect) -> ContentLayout {
    let row = |offset: u16| {
        Rect::new(
            area.x,
            area.y.saturating_add(offset),
            area.width,
            u16::from(offset < area.height),
        )
    };
    let controls = row(1);
    let mode_width = ("[Alt+A] Listening TCP + UDP".len() as u16).min(controls.width);
    ContentLayout {
        status: row(0),
        filter: Rect::new(
            controls.x,
            controls.y,
            controls.width.saturating_sub(mode_width),
            controls.height,
        ),
        mode: Rect::new(
            controls.right().saturating_sub(mode_width),
            controls.y,
            mode_width,
            controls.height,
        ),
        notice: row(2),
        header: row(3),
        rows: Rect::new(
            area.x,
            area.y.saturating_add(4).min(area.bottom()),
            area.width,
            area.height.saturating_sub(4),
        ),
    }
}

pub(crate) fn draw_browser(frame: &mut ratatui::Frame<'_>, screen: Rect, app: &App, theme: Theme) {
    let layout = browser_layout(screen);
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        modal_block_focused(modal_title("NETWORK ENDPOINTS", theme), theme),
        layout.area,
    );
    draw_content(
        frame,
        layout.content,
        &app.network_browser,
        false,
        !app.show_process_info_dialog,
        theme,
    );
    if layout.area.width >= 240 && !app.network_browser.detail {
        let inspector = Rect::new(
            layout.content.right() + 2,
            layout.content.y,
            50,
            layout.content.height,
        );
        frame.render_widget(
            Paragraph::new(
                detail_lines(&app.network_browser, inspector.width)
                    .into_iter()
                    .map(Line::raw)
                    .collect::<Vec<_>>(),
            )
            .style(Style::default().fg(theme.muted).bg(theme.panel_alt)),
            inspector,
        );
    }
    let secondary = if app.network_browser.editing {
        vec![("Backspace/Delete", "edit"), ("Home/End", "first/last")]
    } else if app.network_browser.detail {
        vec![("PgUp/PgDn", "page"), ("Home/End", "first/last")]
    } else {
        vec![
            ("↑/↓", "select"),
            ("PgUp/PgDn", "page"),
            ("Home/End", "first/last"),
            ("Ctrl+C", "copy"),
        ]
    };
    let lines = vec![
        Line::from(shortcuts(
            &app.network_browser,
            true,
            layout.footer.width,
            theme,
        )),
        Line::from(shortcut_line(&secondary, layout.footer.width, theme)),
    ];
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &ratatui::text::Text::from(lines.clone()),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.panel_alt)),
        layout.footer,
    );
}

pub(crate) fn draw_process_tab(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    draw_content(
        frame,
        area,
        &app.process_network,
        app.activity() == AppActivity::LogView,
        app.process_info_focus == ProcessInfoFocus::Content,
        theme,
    );
}

pub(crate) fn draw_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &NetworkView,
    not_recorded: bool,
    focused: bool,
    theme: Theme,
) {
    let normal = Style::default().fg(theme.text).bg(theme.panel_alt);
    if not_recorded {
        frame.render_widget(
            Paragraph::new("This information is not included in recording logs.")
                .style(normal.fg(theme.muted)),
            area,
        );
        return;
    }
    if view.detail {
        frame.render_widget(
            Paragraph::new(
                detail_lines(view, area.width)
                    .into_iter()
                    .map(Line::raw)
                    .collect::<Vec<_>>(),
            )
            .style(normal)
            .scroll((view.scroll.offset.min(u16::MAX as usize) as u16, 0)),
            area,
        );
        draw_scrollbar(frame, area, view, theme);
        return;
    }
    let layout = content_layout(area);
    let entries = view.entries();
    let status = match &view.report {
        Some(report) => format!(
            "{}{} · {} shown / {} captured endpoints · {}/4 tables",
            if view.pending.is_some() {
                "Updating · "
            } else {
                "Captured "
            },
            report.captured_at.format("%H:%M:%S"),
            entries.len(),
            report.endpoints.len(),
            report.successful_tables
        ),
        None if view.pending.is_some() => "Loading endpoints...".into(),
        None => "No capture available".into(),
    };
    frame.render_widget(
        Paragraph::new(status).style(normal.fg(theme.muted)),
        layout.status,
    );
    let filter_width = layout.filter.width.saturating_sub(8) as usize;
    let (filter_text, cursor_column) = filter_window(view, filter_width);
    let filter_active = focused && (view.editing || view.target.is_some());
    frame.render_widget(
        Paragraph::new(format!("Filter: {filter_text}")).style(if filter_active {
            normal.bg(theme.focus_surface)
        } else {
            normal
        }),
        layout.filter,
    );
    if filter_active && filter_width > 0 && layout.filter.height > 0 {
        frame.set_cursor_position(Position::new(
            layout.filter.x + 8 + cursor_column as u16,
            layout.filter.y,
        ));
    }
    frame.render_widget(
        Paragraph::new(match (view.target.is_some(), view.all) {
            (true, true) => "[Alt+A] All endpoints",
            (true, false) => "[Alt+A] Listening TCP + UDP",
            (false, true) => "[a] All endpoints",
            (false, false) => "[a] Listening TCP + UDP",
        })
        .style(normal.fg(theme.accent)),
        layout.mode,
    );
    let notice = view
        .notice
        .clone()
        .or_else(|| {
            view.report.as_ref().and_then(|report| {
                let unresolved = report.endpoints.iter().filter(|entry| entry.owner.is_none()).count();
                let shown_unresolved = entries.iter().filter(|entry| entry.owner.is_none()).count();
                let mut notices = Vec::new();
                if !report.failures.is_empty() {
                    notices.push(format!("Partial: {}/4 tables; details show capture errors", report.successful_tables));
                }
                if unresolved > 0 {
                    notices.push(format!("Owner unavailable/unverified: {shown_unresolved} shown / {unresolved} captured endpoint rows"));
                }
                (!notices.is_empty()).then(|| notices.join(" · "))
            })
        })
        .unwrap_or_else(|| {
            if layout.rows.width < 110 {
                if view.target.is_some() {
                    "Enter: full endpoint details (remote address included)".into()
                } else {
                    "Space: full endpoint details (remote address included)".into()
                }
            } else {
                String::new()
            }
        });
    frame.render_widget(
        Paragraph::new(notice).style(normal.fg(theme.warning)),
        layout.notice,
    );
    let width = layout.rows.width.saturating_sub(1);
    let columns = table_columns(&entries, width);
    frame.render_widget(
        Paragraph::new(table_line(
            &[
                "Proto",
                "Local address:port",
                "Remote address:port",
                "State",
                "PID",
                "Process",
            ],
            &columns,
            width,
        ))
        .style(normal.fg(theme.muted).add_modifier(Modifier::BOLD)),
        layout.header,
    );
    let offset = view.scroll.offset.min(
        entries
            .len()
            .saturating_sub(layout.rows.height.max(1) as usize),
    );
    if entries.is_empty() && view.report.is_some() {
        let text = if view
            .report
            .as_ref()
            .is_some_and(|report| !report.failures.is_empty())
        {
            "No matching endpoints in the available tables (partial capture)."
        } else {
            "No matching endpoints."
        };
        frame.render_widget(
            Paragraph::new(text).style(normal.fg(theme.muted)),
            layout.rows,
        );
    } else {
        let lines: Vec<Line<'static>> = entries
            .iter()
            .enumerate()
            .skip(offset)
            .take(layout.rows.height as usize)
            .map(|(index, entry)| {
                let local = entry.key.local.to_string();
                let remote = entry
                    .key
                    .remote
                    .map_or_else(|| "--".into(), |address| address.to_string());
                let pid = entry.key.pid.to_string();
                let line = table_line(
                    &[
                        entry.key.protocol.label(),
                        &local,
                        &remote,
                        entry.state_label(),
                        &pid,
                        entry.process_name(),
                    ],
                    &columns,
                    width,
                );
                let style = if index == view.selected {
                    normal.bg(theme.focus_surface).add_modifier(Modifier::BOLD)
                } else {
                    normal
                };
                Line::styled(line, style)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).style(normal), layout.rows);
    }
    draw_scrollbar(frame, area, view, theme);
}

fn filter_window(view: &NetworkView, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let before = &view.filter[..view.cursor];
    let mut start = 0;
    while Span::raw(&before[start..]).width() >= width {
        let Some(ch) = before[start..].chars().next() else {
            break;
        };
        start += ch.len_utf8();
    }
    (
        fit(&view.filter[start..], width, false),
        Span::raw(&before[start..]).width(),
    )
}

fn fit(text: &str, width: usize, pad: bool) -> String {
    let truncated = Span::raw(text).width() > width;
    let limit = width.saturating_sub(usize::from(truncated));
    let mut output = String::new();
    for ch in text.chars() {
        if Span::raw(&output).width() + Span::raw(ch.to_string()).width() > limit {
            break;
        }
        output.push(if ch.is_control() { ' ' } else { ch });
    }
    if truncated && width > 0 {
        output.push('…');
    }
    if pad {
        output.push_str(&" ".repeat(width.saturating_sub(Span::raw(&output).width())));
    }
    output
}

fn table_columns(entries: &[&NetworkEndpoint], width: u16) -> [usize; 6] {
    let remote_visible = width >= 109;
    let mut columns = [5, 18, 19, 12, 3, 7];
    for entry in entries {
        columns[1] = columns[1].max(entry.key.local.to_string().len());
        if let Some(remote) = entry.key.remote {
            columns[2] = columns[2].max(remote.to_string().len());
        }
        columns[4] = columns[4].max(entry.key.pid.to_string().len());
        columns[5] = columns[5].max(Span::raw(entry.process_name()).width());
    }
    if !remote_visible {
        columns[2] = 0;
    }
    let separators = if remote_visible { 5 } else { 4 };
    let required = columns.iter().sum::<usize>() + separators;
    let mut excess = required.saturating_sub(width as usize);
    // Give full numeric endpoints priority over long executable names on smaller screens.
    let name_reduction = excess.min(columns[5].saturating_sub(16));
    columns[5] -= name_reduction;
    excess -= name_reduction;
    while excess > 0 {
        let index = if columns[1] >= columns[2] { 1 } else { 2 };
        if columns[index] <= 10 {
            break;
        }
        columns[index] -= 1;
        excess -= 1;
    }
    // Let the final column use the remaining surface rather than leaving unused cells.
    columns[5] += (width as usize).saturating_sub(columns.iter().sum::<usize>() + separators);
    columns
}

fn table_line(values: &[&str; 6], columns: &[usize; 6], width: u16) -> String {
    let mut line = String::new();
    for (value, column_width) in values.iter().zip(columns) {
        if *column_width == 0 {
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&fit(value, *column_width, true));
    }
    fit(&line, width as usize, false)
}

pub(crate) fn detail_lines(view: &NetworkView, width: u16) -> Vec<String> {
    let mut lines =
        if let Some(entry) = view.detail_entry.as_ref().or_else(|| view.selected_entry()) {
            vec![
                format!("Protocol: {}", entry.key.protocol.label()),
                format!("Local: {}", entry.key.local),
                format!(
                    "Remote: {}",
                    entry
                        .key
                        .remote
                        .map_or_else(|| "--".into(), |address| address.to_string())
                ),
                format!("State: {}", entry.state_label()),
                format!("PID: {}", entry.key.pid),
                format!("Process: {}", entry.process_name()),
                format!(
                    "Owner: {}",
                    if entry.owner.is_some() {
                        "Verified during capture"
                    } else {
                        "Unavailable or unverified"
                    }
                ),
            ]
        } else {
            vec!["No endpoint selected.".into()]
        };
    if let Some(report) = &view.report {
        lines.push(format!(
            "Capture: {} - {}",
            report.started_at.format("%Y-%m-%d %H:%M:%S%.3f"),
            report.captured_at.format("%H:%M:%S%.3f")
        ));
        lines.extend(
            report
                .failures
                .iter()
                .map(|failure| format!("Partial: {failure}")),
        );
    }
    if let Some(notice) = &view.notice {
        lines.push(notice.clone());
    }
    let width = width.saturating_sub(1).max(1) as usize;
    lines
        .into_iter()
        .flat_map(|line| {
            let mut rows = Vec::new();
            let mut row = String::new();
            let mut used = 0;
            for ch in line.chars() {
                let ch = if ch.is_control() { ' ' } else { ch };
                let size = Span::raw(ch.to_string()).width();
                if used + size > width && !row.is_empty() {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                row.push(ch);
                used += size;
            }
            rows.push(row);
            rows
        })
        .collect()
}

pub(crate) fn scrollbar_area(area: Rect, view: &NetworkView) -> Option<Rect> {
    let rows = if view.detail {
        area
    } else {
        content_layout(area).rows
    };
    let total = if view.detail {
        detail_lines(view, area.width).len()
    } else {
        view.entries().len()
    };
    (total > rows.height as usize && !rows.is_empty())
        .then(|| Rect::new(rows.right().saturating_sub(1), rows.y, 1, rows.height))
}

fn draw_scrollbar(frame: &mut ratatui::Frame<'_>, area: Rect, view: &NetworkView, theme: Theme) {
    let Some(bar) = scrollbar_area(area, view) else {
        return;
    };
    let total = if view.detail {
        detail_lines(view, area.width).len()
    } else {
        view.entries().len()
    };
    let rows = bar.height.max(1) as usize;
    let max_offset = total.saturating_sub(rows);
    let position = (view.scroll.offset.min(max_offset) * total.saturating_sub(1) + max_offset / 2)
        / max_offset.max(1);
    let mut state = ScrollbarState::new(total)
        .position(position)
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_symbol("█")
            .track_symbol(Some("│"))
            .style(Style::default().fg(theme.muted).bg(theme.panel_alt))
            .thumb_style(Style::default().fg(theme.accent)),
        bar,
        &mut state,
    );
}

pub(crate) fn shortcuts(
    view: &NetworkView,
    global: bool,
    width: u16,
    theme: Theme,
) -> Vec<Span<'static>> {
    let items = if view.editing {
        vec![
            ("Enter", "Apply"),
            ("Esc", "Clear"),
            ("Home/End", "Edge"),
            ("Ctrl+U", "clear"),
            ("←/→", "cursor"),
        ]
    } else if view.detail {
        vec![("Esc/Enter", "back"), ("↑/↓", "scroll"), ("Ctrl+C", "copy")]
    } else if global {
        vec![
            ("Esc", "close"),
            ("Enter", "info"),
            ("Space", "details"),
            ("/", "filter"),
            ("a", "mode"),
            ("r", "refresh"),
        ]
    } else {
        vec![
            ("Esc", "tabs"),
            ("Ctrl+←/→", "tabs"),
            ("Enter", "details"),
            ("Ctrl+U", "refresh"),
            ("Ctrl+C", "copy"),
            ("Tab", "tabs"),
            ("Alt+A", "mode"),
            ("↑/↓", "select"),
        ]
    };
    shortcut_line(&items, width, theme)
}

fn shortcut_line(items: &[(&str, &str)], width: u16, theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, label) in items {
        let size =
            Span::raw(format!("{key} {label}")).width() + if spans.is_empty() { 0 } else { 2 };
        if used + size > width as usize {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(theme.key_hint),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text),
        ));
        used += size;
    }
    spans
}
