use ratatui::{
    layout::Rect,
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
        theme::contrasting_foreground,
        widgets::{block::modal_block_focused, scrollable_modal::ScrollableModal},
    },
};

const PROCESS_INFO_MODAL: ScrollableModal = ScrollableModal::new("", 138, 20, 1);
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
    let dialog_theme = theme;
    frame.render_widget(
        modal_block_focused(process_info_title(app, dialog_theme), dialog_theme),
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
        ProcessInfoTab::Scheduling => render_scrollable_lines(
            frame,
            layout.content,
            super::scheduling::lines(app, layout.content.width, theme),
            app.scheduling.scroll.offset,
            app.process_info_focus == ProcessInfoFocus::Content,
            theme,
        ),
        ProcessInfoTab::Network => {
            super::network::draw_process_tab(frame, layout.content, app, theme)
        }
        ProcessInfoTab::Dlls => draw_process_modules_tab(frame, layout.content, app, theme),
        ProcessInfoTab::Environment => {
            draw_process_environment_tab(frame, layout.content, app, theme)
        }
    }

    draw_footer(frame, layout.footer, app, theme);
}

pub(crate) fn process_info_dialog_layout_for_screen(screen: Rect) -> ProcessInfoDialogLayout {
    let screen_regions = screen_layout(screen);
    let available = screen_regions.get(1).copied().unwrap_or(screen);
    let modal = PROCESS_INFO_MODAL.layout(available);
    let tab_height = tab_row_count(modal.content.width).min(modal.content.height);
    let top_gap = u16::from(modal.content.height.saturating_sub(tab_height) > 1);
    let tabs = Rect::new(
        modal.content.x,
        modal.content.y.saturating_add(top_gap),
        modal.content.width,
        tab_height,
    );
    let gap = u16::from(modal.content.height.saturating_sub(tab_height + top_gap) > 1);
    let content = Rect::new(
        modal.content.x,
        tabs.bottom().saturating_add(gap),
        modal.content.width,
        modal
            .content
            .height
            .saturating_sub(tab_height + top_gap + gap),
    );
    ProcessInfoDialogLayout {
        area: modal.area,
        tabs,
        content,
        footer: modal.footer,
    }
}

// Keep capture metadata, filters, and column headings outside the scrolling rows.
pub(crate) fn inspection_table_areas(area: Rect, header_rows: usize) -> (Rect, Rect) {
    let height = header_rows.min(area.height.saturating_sub(1) as usize) as u16;
    (
        Rect::new(area.x, area.y, area.width, height),
        Rect::new(area.x, area.y + height, area.width, area.height - height),
    )
}

pub(crate) fn inspection_header_rows(app: &App) -> usize {
    match app.process_info_tab {
        ProcessInfoTab::Files => super::open_files::fixed_header_rows(app),
        ProcessInfoTab::Dlls => super::process_modules::fixed_header_rows(app),
        ProcessInfoTab::Environment => super::process_environment::fixed_header_rows(app),
        _ => 0,
    }
}

pub(crate) fn draw_inspection_lines(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    mut lines: Vec<Line<'static>>,
    header_rows: usize,
    offset: usize,
    theme: Theme,
) {
    let body_lines = lines.split_off(header_rows.min(lines.len()));
    let (header, body) = inspection_table_areas(area, header_rows);
    let style = Style::default().fg(theme.text).bg(theme.panel_alt);
    let offset = offset.min(body_lines.len().saturating_sub(body.height as usize));
    frame.render_widget(Paragraph::new(lines).style(style), header);
    frame.render_widget(
        Paragraph::new(body_lines)
            .style(style)
            .scroll((offset as u16, 0)),
        body,
    );
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
        ProcessInfoTab::Scheduling => super::scheduling::lines(app, width, app.theme()).len(),
        ProcessInfoTab::Dlls => process_modules_total_rows(app, width),
        ProcessInfoTab::Environment => process_environment_total_rows(app, width),
        ProcessInfoTab::Network if app.process_network.detail => {
            super::network::detail_lines(&app.process_network, width).len()
        }
        ProcessInfoTab::Network => app.process_network.entries().len(),
    }
}

pub(crate) fn process_info_tab_at(screen: Rect, x: u16, y: u16) -> Option<ProcessInfoTab> {
    let layout = process_info_dialog_layout_for_screen(screen);
    ProcessInfoTab::ALL
        .into_iter()
        .zip(tab_areas(layout.tabs))
        .find_map(|(tab, area)| contains_point(area, x, y).then_some(tab))
}

pub(crate) fn process_info_scrollbar_area_for_screen(screen: Rect, app: &App) -> Option<Rect> {
    let content = process_info_content_area_for_screen(screen);
    if app.process_info_tab == ProcessInfoTab::Network {
        return super::network::scrollbar_area(content, &app.process_network);
    }
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
    let foreground = contrasting_foreground(theme.focus_border, theme);
    let title_style = Style::default().fg(foreground).bg(theme.focus_border);
    let mut spans = vec![Span::styled(
        " PROCESS INFO ",
        title_style.add_modifier(Modifier::BOLD),
    )];
    if let Some(process) = app.process_info_target_process() {
        spans.push(Span::styled(
            format!(" · {} · PID {} ", process.name, process.pid),
            Style::default().fg(theme.text).bg(theme.panel_alt),
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
            let style = Style::default()
                .fg(contrasting_foreground(theme.focus_border, theme))
                .bg(theme.focus_border)
                .add_modifier(Modifier::BOLD);
            if focused {
                style.add_modifier(Modifier::UNDERLINED)
            } else {
                style
            }
        } else {
            Style::default().fg(theme.text).bg(theme.panel_alt)
        };
        frame.render_widget(
            Paragraph::new(format!(" {} ", tab.label())).style(style),
            area,
        );
    }
}

fn tab_row_count(width: u16) -> u16 {
    let mut rows = 1;
    let mut used = 0;
    for tab in ProcessInfoTab::ALL {
        let size = tab.label().chars().count() as u16 + TAB_HORIZONTAL_PADDING;
        if used > 0 && used + size > width {
            rows += 1;
            used = 0;
        }
        used += size.min(width);
    }
    rows
}

fn tab_areas(area: Rect) -> [Rect; ProcessInfoTab::ALL.len()] {
    let mut result = [Rect::default(); ProcessInfoTab::ALL.len()];
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
    for row in metrics.rows {
        if row.label == ".NET Heap" {
            lines.push(Line::from(Span::styled(
                ".NET runtime · -- = unavailable",
                Style::default().fg(theme.muted),
            )));
        }
        lines.push(metric_value_line(
            row.label,
            &row.value,
            row.delta.as_deref(),
            width,
            theme,
        ));
    }
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
        (
            ".NET version",
            info_value(info.map(|info| &info.dotnet_version)),
        ),
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

pub(crate) fn wrap_display_width(value: &str, max_width: usize) -> Vec<String> {
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
            .style(Style::default().fg(theme.text).bg(theme.panel_alt))
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
        .style(Style::default().fg(theme.text).bg(theme.panel_alt))
        .thumb_style(
            Style::default()
                .fg(if focused {
                    theme.focus_border
                } else {
                    theme.muted
                })
                .bg(theme.panel_alt),
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

fn draw_footer(frame: &mut ratatui::Frame<'_>, footer: Rect, app: &App, theme: Theme) {
    if footer.is_empty() {
        return;
    }
    crate::ui::footer::register_shortcut_text(
        app,
        footer,
        &ratatui::text::Text::from(Line::from(shortcut_spans(app, footer.width, theme))),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(app, footer.width, theme)))
            .style(Style::default().bg(theme.panel_alt)),
        footer,
    );
}

fn shortcut_spans(app: &App, width: u16, theme: Theme) -> Vec<Span<'static>> {
    if app.process_info_filter_editing {
        return crate::ui::footer::shortcut_spans(
            crate::app::text_input::INSPECTION_FILTER_SHORTCUTS,
            theme,
        );
    }
    if app.process_info_tab == ProcessInfoTab::Network
        && app.process_info_focus == ProcessInfoFocus::Content
    {
        return super::network::shortcuts(&app.process_network, false, width, theme);
    }
    let items = if app.scheduling.applying {
        Vec::new()
    } else if app.process_info_focus == ProcessInfoFocus::Tabs
        && !app.process_info_tab.content_is_focusable()
    {
        vec![("←/→", "tabs"), ("↑/↓", "scroll"), ("Esc", "close")]
    } else if app.process_info_focus == ProcessInfoFocus::Tabs {
        let mut keys = vec![("←/→", "tabs"), ("Tab", "next"), ("Esc", "close")];
        if app.process_info_tab == ProcessInfoTab::Environment && !app.process_info_detail_is_open()
        {
            keys.insert(0, ("/", "filter"));
        }
        keys
    } else if app.process_info_detail_is_open() {
        let copy_label = match app.process_info_tab {
            ProcessInfoTab::Dlls => "copy path",
            ProcessInfoTab::Environment => "copy variable",
            ProcessInfoTab::Files => "copy row",
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
            ProcessInfoTab::Network => Vec::new(),
            ProcessInfoTab::Scheduling if app.scheduling.affinity_focused => vec![
                ("Space", "toggle"),
                ("Ctrl+D", "default"),
                ("p", "priority"),
                ("←/→/↑/↓", "move"),
                ("Tab", "next"),
                ("Ctrl+Z", "restore"),
                ("Ctrl+U", "refresh"),
                ("Esc", "tabs"),
            ],
            ProcessInfoTab::Scheduling => vec![
                ("a", "affinity"),
                ("←/→", "set"),
                ("Space", "select"),
                ("Ctrl+D", "default"),
                ("Ctrl+Z", "restore"),
                ("Ctrl+U", "refresh"),
                ("Tab", "next"),
                ("Esc", "tabs"),
            ],
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
                ("Enter", "details"),
                ("Ctrl+U", "refresh now"),
                ("Ctrl+C", "copy rows"),
                ("↑/↓", "select"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc", "tabs"),
            ],
            ProcessInfoTab::Dlls => vec![
                ("Enter", "details"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+C", "copy path"),
                ("↑/↓", "select"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc", "tabs"),
            ],
            ProcessInfoTab::Environment => vec![
                ("/", "filter"),
                ("Enter", "details"),
                ("Ctrl+U", "refresh"),
                ("Ctrl+C", "copy"),
                ("↑/↓", "select"),
                ("Ctrl+←/→", "tabs"),
                ("Tab", "next"),
                ("Esc", "tabs"),
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
    fn small_screen_keeps_tabs_content_and_shortcuts_separate() {
        let screen = Rect::new(0, 0, 60, 12);
        let layout = process_info_dialog_layout_for_screen(screen);

        assert!(layout.tabs.bottom() <= layout.content.y);
        assert_eq!(layout.content.bottom() + 1, layout.footer.y);
        assert_eq!(layout.footer.height, 1);
    }

    #[test]
    fn wrapped_image_value_keeps_all_text() {
        assert_eq!(wrap_display_width("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }
}
