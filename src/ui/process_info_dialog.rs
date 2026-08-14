use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    App,
    app::{ProcessInfoFocus, ProcessInfoTab},
    model::{InfoValue, ProcessInfo, ProcessRow},
    ui::{
        Theme,
        layout::screen_layout,
        open_files::{draw_open_files_tab, open_files_scrollbar_area, open_files_total_rows},
        process_environment::{
            draw_process_environment_tab, process_environment_scrollbar_area,
            process_environment_total_rows,
        },
        process_modules::{
            draw_process_modules_tab, process_modules_scrollbar_area, process_modules_total_rows,
        },
        widgets::{block::panel_block_focused, scrollable_modal::ScrollableModal},
    },
};

const PROCESS_INFO_MODAL: ScrollableModal = ScrollableModal::new("", 138, 20, 2);
const CLOSE_BUTTON: &str = "[ Close ]";
const TAB_HORIZONTAL_PADDING: u16 = 2;
const IMAGE_LABEL_WIDTH: usize = 16;
const METRIC_LABEL_WIDTH: usize = 22;
const METRIC_VALUE_WIDTH: usize = 20;
const METRIC_DELTA_WIDTH: usize = 24;
const METRIC_WIDE_LAYOUT_WIDTH: u16 =
    (METRIC_LABEL_WIDTH + METRIC_VALUE_WIDTH + METRIC_DELTA_WIDTH) as u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessInfoDialogLayout {
    pub(crate) area: Rect,
    pub(crate) tabs: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
}

pub(crate) fn draw_process_info_dialog(
    frame: &mut ratatui::Frame<'_>,
    screen: Rect,
    app: &App,
    theme: Theme,
) {
    let layout = process_info_dialog_layout_for_screen(screen);
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        panel_block_focused(process_info_title(app, theme), theme, true),
        layout.area,
    );
    draw_tabs(
        frame,
        layout,
        app.process_info_tab,
        app.process_info_focus == ProcessInfoFocus::Tabs,
        theme,
    );

    match app.process_info_tab {
        ProcessInfoTab::Metrics => render_scrollable_lines(
            frame,
            layout.content,
            process_info_metrics_lines(app, layout.content.width, theme),
            app.process_info_scroll_offset(),
            app.process_info_focus == ProcessInfoFocus::Content,
            theme,
        ),
        ProcessInfoTab::Image => render_scrollable_lines(
            frame,
            layout.content,
            process_info_image_lines(app, layout.content.width, theme),
            app.process_info_scroll_offset(),
            app.process_info_focus == ProcessInfoFocus::Content,
            theme,
        ),
        ProcessInfoTab::Files => draw_open_files_tab(frame, layout.content, app, theme),
        ProcessInfoTab::Dlls => draw_process_modules_tab(frame, layout.content, app, theme),
        ProcessInfoTab::Environment => {
            draw_process_environment_tab(frame, layout.content, app, theme)
        }
    }

    draw_footer(
        frame,
        layout.footer,
        app,
        app.process_info_focus == ProcessInfoFocus::Close,
        theme,
    );
}

pub(crate) fn process_info_dialog_layout_for_screen(screen: Rect) -> ProcessInfoDialogLayout {
    let screen_regions = screen_layout(screen);
    let available = screen_regions.get(1).copied().unwrap_or(screen);
    let modal = PROCESS_INFO_MODAL.layout(available);
    let tab_height = tab_row_count(modal.content.width).min(modal.content.height);
    let tabs = Rect::new(
        modal.content.x,
        modal.content.y,
        modal.content.width,
        tab_height,
    );
    let content = Rect::new(
        modal.content.x,
        tabs.bottom(),
        modal.content.width,
        modal.content.height.saturating_sub(tab_height),
    );
    ProcessInfoDialogLayout {
        area: modal.area,
        tabs,
        content,
        footer: modal.footer,
    }
}

pub(crate) fn process_info_content_area_for_screen(screen: Rect) -> Rect {
    process_info_dialog_layout_for_screen(screen).content
}

pub(crate) fn process_info_page_size_for_screen(screen: Rect) -> usize {
    process_info_content_area_for_screen(screen).height.max(1) as usize
}

pub(crate) fn process_info_total_rows(app: &App) -> usize {
    let width = process_info_content_area_for_screen(app.last_screen_area).width;
    match app.process_info_tab {
        ProcessInfoTab::Metrics => process_info_metrics_lines(app, width, app.theme()).len(),
        ProcessInfoTab::Image => process_info_image_lines(app, width, app.theme()).len(),
        ProcessInfoTab::Files => open_files_total_rows(app),
        ProcessInfoTab::Dlls => process_modules_total_rows(app, width),
        ProcessInfoTab::Environment => process_environment_total_rows(app, width),
    }
}

pub(crate) fn process_info_tab_at(screen: Rect, x: u16, y: u16) -> Option<ProcessInfoTab> {
    let layout = process_info_dialog_layout_for_screen(screen);
    ProcessInfoTab::ALL
        .into_iter()
        .zip(tab_areas(layout.tabs))
        .find_map(|(tab, area)| contains_point(area, x, y).then_some(tab))
}

pub(crate) fn process_info_close_button_area_for_screen(screen: Rect) -> Option<Rect> {
    close_button_area(process_info_dialog_layout_for_screen(screen).footer)
}

pub(crate) fn process_info_scrollbar_area_for_screen(screen: Rect, app: &App) -> Option<Rect> {
    let content = process_info_content_area_for_screen(screen);
    if app.process_info_tab == ProcessInfoTab::Files {
        return open_files_scrollbar_area(content, app);
    }
    if app.process_info_tab == ProcessInfoTab::Dlls {
        return process_modules_scrollbar_area(content, app);
    }
    if app.process_info_tab == ProcessInfoTab::Environment {
        return process_environment_scrollbar_area(content, app);
    }
    generic_scrollbar_area(
        content,
        process_info_total_rows(app),
        app.process_info_page_size(),
    )
}

fn process_info_title(app: &App, theme: Theme) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "PROCESS INFO",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    if let Some(process) = app.process_info_target_process() {
        spans.push(Span::styled(
            format!(" · {} · PID {}", process.name, process.pid),
            Style::default()
                .fg(theme.muted)
                .remove_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn draw_tabs(
    frame: &mut ratatui::Frame<'_>,
    layout: ProcessInfoDialogLayout,
    active: ProcessInfoTab,
    focused: bool,
    theme: Theme,
) {
    for (tab, area) in ProcessInfoTab::ALL.into_iter().zip(tab_areas(layout.tabs)) {
        if area.is_empty() {
            continue;
        }
        let style = if tab == active {
            let style = if focused {
                Style::default()
                    .fg(theme.focus_border)
                    .bg(theme.focus_surface)
            } else {
                Style::default().fg(theme.accent).bg(theme.panel)
            };
            style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted).bg(theme.panel)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", tab.label())).style(style),
            area,
        );
    }
}

fn tab_row_count(width: u16) -> u16 {
    let required = ProcessInfoTab::ALL
        .iter()
        .map(|tab| tab.label().chars().count() as u16 + TAB_HORIZONTAL_PADDING)
        .sum::<u16>();
    if width >= required { 1 } else { 2 }
}

fn tab_areas(area: Rect) -> [Rect; 5] {
    let mut result = [Rect::default(); 5];
    if area.is_empty() {
        return result;
    }
    let mut x = area.x;
    let mut row = 0u16;
    for (index, tab) in ProcessInfoTab::ALL.into_iter().enumerate() {
        let desired_width = tab.label().chars().count() as u16 + TAB_HORIZONTAL_PADDING;
        if x > area.x
            && x.saturating_add(desired_width) > area.right()
            && row.saturating_add(1) < area.height
        {
            row = row.saturating_add(1);
            x = area.x;
        }
        if row >= area.height || x >= area.right() {
            continue;
        }
        let width = desired_width.min(area.right().saturating_sub(x));
        result[index] = Rect::new(x, area.y.saturating_add(row), width, 1);
        x = x.saturating_add(width);
    }
    result
}

fn process_info_metrics_lines(app: &App, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let Some(metrics) = app.process_info_metrics_view() else {
        return vec![Line::from(Span::styled(
            "Metrics --",
            Style::default().fg(theme.muted),
        ))];
    };
    let mut lines = vec![Line::from(Span::styled(
        metrics.range,
        Style::default().fg(theme.accent),
    ))];
    lines.push(metric_header_line(
        metrics.value_heading,
        metrics.delta_heading,
        width,
        theme,
    ));
    lines.extend(
        metrics.rows.into_iter().map(|row| {
            metric_value_line(row.label, &row.value, row.delta.as_deref(), width, theme)
        }),
    );
    lines
}

fn process_info_image_lines(app: &App, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let info = app.process_info_for_selected();
    let process = app.process_info_target_process();
    let process_identity = info
        .map(format_process_identity)
        .or_else(|| process.map(|row| format!("{} / PID {}", row.name, row.pid)))
        .unwrap_or_else(|| "--".to_string());
    let started = info
        .map(format_process_started)
        .or_else(|| process.map(format_recorded_process_started))
        .unwrap_or_else(|| "--".to_string());
    let executable = info
        .map(|info| value_text(&info.executable))
        .or_else(|| process.and_then(|row| row.executable_path.clone()))
        .unwrap_or_else(|| "--".to_string());
    let rows = [
        ("Process", process_identity),
        ("User", info_value(info.map(|info| &info.user))),
        ("Architecture", info_value(info.map(|info| &info.arch))),
        ("Parent", info_value(info.map(|info| &info.parent_process))),
        ("Started", started),
        ("Executable", executable),
        (
            "Command line",
            info_value(info.map(|info| &info.command_line)),
        ),
        ("Company", info_value(info.map(|info| &info.company_name))),
        ("Product", info_value(info.map(|info| &info.product_name))),
        (
            "Product version",
            info_value(info.map(|info| &info.product_version)),
        ),
        (
            "File version",
            info_value(info.map(|info| &info.file_version)),
        ),
        ("Modified", info_value(info.map(|info| &info.file_modified))),
        ("Size", info_value(info.map(|info| &info.file_size))),
    ];
    rows.into_iter()
        .flat_map(|(label, value)| labeled_wrapped_lines(label, &value, width, theme))
        .collect()
}

fn labeled_wrapped_lines(label: &str, value: &str, width: u16, theme: Theme) -> Vec<Line<'static>> {
    let width = width as usize;
    if width == 0 {
        return Vec::new();
    }
    let label_width = IMAGE_LABEL_WIDTH.min(width.saturating_sub(1)).max(1);
    let value_width = width.saturating_sub(label_width).max(1);
    let wrapped = wrap_display_width(value, value_width);
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let label = if index == 0 { label } else { "" };
            Line::from(vec![
                Span::styled(
                    format!("{label:<label_width$}"),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(value, Style::default().fg(theme.text)),
            ])
        })
        .collect()
}

fn wrap_display_width(value: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    if value.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut width = 0usize;
    for ch in value.chars() {
        let ch_width = Span::raw(ch.to_string()).width().max(1);
        if width > 0 && width.saturating_add(ch_width) > max_width {
            lines.push(std::mem::take(&mut line));
            width = 0;
        }
        line.push(ch);
        width = width.saturating_add(ch_width);
    }
    lines.push(line);
    lines
}

fn info_value(value: Option<&InfoValue>) -> String {
    value
        .map(|value| value.text().to_string())
        .unwrap_or_else(|| "--".to_string())
}

fn metric_header_line(
    value_heading: &str,
    delta_heading: Option<&str>,
    width: u16,
    theme: Theme,
) -> Line<'static> {
    let header_style = Style::default()
        .fg(theme.text)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    if width >= METRIC_WIDE_LAYOUT_WIDTH {
        let mut spans = vec![Span::styled("Metrics", header_style)];
        spans.push(Span::raw(" ".repeat(METRIC_LABEL_WIDTH - "Metrics".len())));
        spans.push(Span::raw(
            " ".repeat(METRIC_VALUE_WIDTH.saturating_sub(value_heading.len())),
        ));
        spans.push(Span::styled(value_heading.to_string(), header_style));
        if let Some(delta_heading) = delta_heading {
            spans.push(Span::raw(
                " ".repeat(METRIC_DELTA_WIDTH.saturating_sub(delta_heading.len())),
            ));
            spans.push(Span::styled(delta_heading.to_string(), header_style));
        }
        return Line::from(spans);
    }
    let mut spans = vec![Span::styled("Metrics", header_style), Span::raw("  ")];
    spans.push(Span::styled(value_heading.to_string(), header_style));
    if let Some(delta_heading) = delta_heading {
        spans.push(Span::raw(" / "));
        spans.push(Span::styled(delta_heading.to_string(), header_style));
    }
    Line::from(spans)
}

fn metric_value_line(
    label: &str,
    value: &str,
    delta: Option<&str>,
    width: u16,
    theme: Theme,
) -> Line<'static> {
    let label_width = METRIC_LABEL_WIDTH;
    let text = if width >= METRIC_WIDE_LAYOUT_WIDTH {
        let value_width = METRIC_VALUE_WIDTH;
        let delta_width = METRIC_DELTA_WIDTH;
        match delta {
            Some(delta) => {
                let delta = format!("({delta})");
                format!("{label:<label_width$}{value:>value_width$}{delta:>delta_width$}")
            }
            None => format!("{label:<label_width$}{value:>value_width$}"),
        }
    } else {
        match delta {
            Some(delta) => format!("{label:<label_width$}{value} ({delta})"),
            None => format!("{label:<label_width$}{value}"),
        }
    };
    Line::from(Span::styled(text, Style::default().fg(theme.text)))
}

fn render_scrollable_lines(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    lines: Vec<Line<'static>>,
    offset: usize,
    focused: bool,
    theme: Theme,
) {
    let total = lines.len();
    let rows = area.height.max(1) as usize;
    let offset = offset.min(total.saturating_sub(rows));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.panel))
            .scroll((offset as u16, 0)),
        area,
    );
    let Some(scrollbar_area) = generic_scrollbar_area(area, total, rows) else {
        return;
    };
    let mut state = ScrollbarState::new(total)
        .position(scrollbar_position(total, rows, offset))
        .viewport_content_length(rows);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(theme.muted).bg(theme.panel))
        .thumb_style(
            Style::default()
                .fg(if focused {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel),
        );
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut state);
}

fn generic_scrollbar_area(area: Rect, total: usize, rows: usize) -> Option<Rect> {
    if area.is_empty() || total <= rows.max(1) {
        return None;
    }
    Some(Rect::new(
        area.right().saturating_sub(1),
        area.y,
        1,
        area.height,
    ))
}

fn scrollbar_position(total: usize, rows: usize, offset: usize) -> usize {
    let rows = rows.max(1).min(total);
    let max_offset = total.saturating_sub(rows);
    if total == 0 || max_offset == 0 {
        return 0;
    }
    (offset.min(max_offset) * total.saturating_sub(1) + max_offset / 2) / max_offset
}

fn draw_footer(
    frame: &mut ratatui::Frame<'_>,
    footer: Rect,
    app: &App,
    close_focused: bool,
    theme: Theme,
) {
    if footer.is_empty() {
        return;
    }
    if footer.height >= 2 {
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(app, footer.width, theme)))
                .style(Style::default().bg(theme.panel)),
            Rect::new(footer.x, footer.y, footer.width, 1),
        );
    }
    if let Some(area) = close_button_area(footer) {
        let style = if close_focused {
            Style::default()
                .fg(theme.text)
                .bg(theme.focus_surface)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text).bg(theme.panel_alt)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(CLOSE_BUTTON, style)))
                .alignment(Alignment::Center),
            area,
        );
    }
}

fn shortcut_spans(app: &App, width: u16, theme: Theme) -> Vec<Span<'static>> {
    let items = if app.process_info_focus == ProcessInfoFocus::Tabs {
        vec![("←/→", "tabs"), ("Tab", "next"), ("Esc", "close")]
    } else if app.process_info_focus == ProcessInfoFocus::Close {
        vec![("Enter", "close"), ("Tab", "next"), ("Esc", "close")]
    } else if app.process_info_detail_is_open() {
        let copy_label = match app.process_info_tab {
            ProcessInfoTab::Dlls => "copy path",
            ProcessInfoTab::Environment => "copy variable",
            _ => "copy",
        };
        vec![
            ("↑/↓", "scroll"),
            ("Ctrl+C", copy_label),
            ("Esc/Enter", "back"),
            ("Ctrl+←/→", "tabs"),
            ("Tab", "next"),
        ]
    } else {
        match app.process_info_tab {
            ProcessInfoTab::Metrics => vec![
                ("↑/↓", "scroll"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc/Enter", "close"),
            ],
            ProcessInfoTab::Image => vec![
                ("↑/↓", "scroll"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc/Enter", "close"),
            ],
            ProcessInfoTab::Files => vec![
                ("↑/↓", "scroll"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+C", "copy paths"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc/Enter", "close"),
            ],
            ProcessInfoTab::Dlls => vec![
                ("Enter", "details"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+C", "copy path"),
                ("↑/↓", "select"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc", "close"),
            ],
            ProcessInfoTab::Environment => vec![
                ("Enter", "details"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+C", "copy variable"),
                ("↑/↓", "select"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc", "close"),
            ],
        }
    };
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (key, label) in items {
        let separator = usize::from(!spans.is_empty()) * 2;
        let item_width = Span::raw(format!("{key} {label}")).width();
        if used.saturating_add(separator).saturating_add(item_width) > width as usize {
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
        used = used.saturating_add(separator).saturating_add(item_width);
    }
    spans
}

fn close_button_area(footer: Rect) -> Option<Rect> {
    let width = CLOSE_BUTTON.len() as u16;
    if footer.width < width || footer.height == 0 {
        return None;
    }
    Some(Rect::new(
        footer.x + footer.width.saturating_sub(width) / 2,
        footer.bottom().saturating_sub(1),
        width,
        1,
    ))
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

fn contains_point(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_screen_uses_centered_maximum_outer_size_below_header() {
        let layout = process_info_dialog_layout_for_screen(Rect::new(0, 0, 200, 60));

        assert_eq!(layout.area.width, 142);
        assert_eq!(layout.area.height, 24);
        assert!(layout.area.y >= 1);
        assert_eq!(layout.area.x, 29);
    }

    #[test]
    fn narrow_or_short_screen_shrinks_only_that_dimension() {
        let narrow = process_info_dialog_layout_for_screen(Rect::new(0, 0, 100, 60));
        assert_eq!((narrow.area.width, narrow.area.height), (100, 24));

        let short = process_info_dialog_layout_for_screen(Rect::new(0, 0, 200, 24));
        assert_eq!((short.area.width, short.area.height), (142, 21));
    }

    #[test]
    fn small_screen_keeps_tabs_content_and_close_button_separate() {
        let screen = Rect::new(0, 0, 60, 12);
        let layout = process_info_dialog_layout_for_screen(screen);
        let close = process_info_close_button_area_for_screen(screen)
            .expect("small dialog should keep a close button");

        assert!(layout.tabs.bottom() <= layout.content.y);
        assert!(layout.content.bottom() <= layout.footer.y);
        assert!(layout.content.bottom() <= close.y);
    }

    #[test]
    fn wrapped_image_value_keeps_all_text() {
        assert_eq!(wrap_display_width("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }
}
