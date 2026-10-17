use crate::{
    App,
    app::{AppActivity, SampleFreshness, state::MainMenuSection},
    ui::Theme,
};
use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

const PRODUCT_LABEL: &str = concat!(" winproc-tui ", env!("CARGO_PKG_VERSION"), " ");

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub(crate) fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let area = Rect {
        height: area.height.min(1),
        ..area
    };
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.menu_bar)),
        area,
    );
    let product_width = product_width(area);
    if product_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled("winproc-tui", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(concat!(" ", env!("CARGO_PKG_VERSION"), " ")),
            ]))
            .style(
                Style::default()
                    .fg(theme.background)
                    .bg(theme.active_series),
            ),
            Rect::new(area.x, area.y, product_width - 1, area.height),
        );
    }
    let actions = header_actions(area, app);
    for (action, rect) in &actions {
        let active = action
            .section()
            .is_some_and(|section| app.is_main_menu_open() && app.main_menu_section == section)
            || (*action == HeaderAction::More
                && app.is_main_menu_open()
                && !actions
                    .iter()
                    .any(|(action, _)| action.section() == Some(app.main_menu_section)));
        let selected = match action {
            HeaderAction::Processes => !app.network_browser.visible && !app.file_users.visible,
            HeaderAction::Network => app.network_browser.visible,
            HeaderAction::FileUsers => app.file_users.visible,
            _ => false,
        };
        let hovered = app.header_action_hovered == Some(*action);
        let style = Style::default()
            .fg(if selected {
                theme.focus_border
            } else {
                theme.text
            })
            .bg(if hovered {
                theme.focus_surface
            } else if active {
                theme.menu_active
            } else {
                theme.menu_bar
            })
            .add_modifier(if active || hovered || selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let compact = compact_menu(menu_area(area));
        let spans = action
            .label(compact)
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                Span::styled(
                    ch.to_string(),
                    if action.access_index(compact) == Some(index) {
                        style.fg(theme.key_hint).add_modifier(Modifier::UNDERLINED)
                    } else if ch == '▾' && !active && !hovered {
                        style.fg(theme.muted)
                    } else {
                        style
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), *rect);
    }
    let status_x = actions
        .last()
        .map_or(area.x, |(_, rect)| rect.right().saturating_add(1))
        .min(area.right());
    let status_area = Rect::new(
        status_x,
        area.y,
        area.right().saturating_sub(status_x),
        area.height,
    );
    let mut spans = header_status_spans(app, theme, usize::from(status_area.width));
    let budget = usize::from(status_area.width);
    let profile = if app.activity() == AppActivity::LogView {
        "PF: --".to_string()
    } else {
        format!(
            "PF: {}{}",
            app.active_investigation_profile
                .as_deref()
                .unwrap_or("none"),
            if app.active_investigation_profile.is_some()
                && app.active_investigation_profile_dirty()
            {
                "*"
            } else {
                ""
            }
        )
    };
    let profile_budget = budget.min(spans.iter().map(Span::width).sum::<usize>() + 30);
    let remaining = profile_budget.saturating_sub(spans.iter().map(Span::width).sum::<usize>());
    if remaining >= 8 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} ", truncate_name(&profile, remaining - 4)),
            Style::default().fg(theme.muted).bg(theme.menu_bar),
        ));
    }
    if let Some(path) = app.active_log_path() {
        append_fitting_label(
            &mut spans,
            &path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy(),
            budget,
            theme.muted,
        );
    }
    // Clip status by terminal cells, retaining activity before optional names.
    let mut remaining = budget;
    let spans = spans
        .into_iter()
        .map(|span| {
            let text = truncate_cells(&span.content, remaining);
            remaining = remaining.saturating_sub(Span::raw(&text).width());
            Span::styled(text, span.style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .alignment(Alignment::Right)
            .style(Style::default().bg(theme.menu_bar)),
        status_area,
    );
}

fn append_fitting_label(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    budget: usize,
    color: ratatui::style::Color,
) {
    let remaining = budget.saturating_sub(spans.iter().map(Span::width).sum::<usize>());
    if remaining >= 6 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            truncate_name(text, remaining - 2),
            Style::default().fg(color),
        ));
    }
}

pub(super) fn truncate_name(text: &str, width: usize) -> String {
    if Span::raw(text).width() <= width {
        return text.to_string();
    }
    if width < 5 {
        return truncate_cells(text, width);
    }
    let tail_budget = ((width - 1) / 2).max(4);
    let mut tail = String::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let cells = Span::raw(ch.to_string()).width();
        if used + cells > tail_budget {
            break;
        }
        tail.insert(0, ch);
        used += cells;
    }
    format!("{}{}", truncate_cells(text, width - used), tail)
}

fn truncate_cells(text: &str, width: usize) -> String {
    if Span::raw(text).width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cells = Span::raw(ch.to_string()).width();
        if used + cells > width - 1 {
            break;
        }
        out.push(ch);
        used += cells;
    }
    out.push('…');
    out
}

fn header_status_spans(app: &App, theme: Theme, budget: usize) -> Vec<Span<'static>> {
    let activity = app.activity();
    let mut spans = vec![mode_span(
        activity_label(activity),
        match activity {
            AppActivity::Live => theme.active_series,
            AppActivity::Recording => theme.danger,
            AppActivity::LogView => theme.warning,
        },
        theme,
    )];
    if let Some(SampleFreshness::Stale { age_seconds }) = app.sample_freshness() {
        spans.push(stale_span(age_seconds, theme));
    }
    if app.is_display_paused() && activity != AppActivity::LogView {
        spans.push(Span::raw("  "));
        spans.push(mode_span("DISPLAY PAUSED", theme.warning, theme));
    }
    // Long stale durations can consume the reserved width. Drop badge padding and
    // redundant spacing before allowing any critical label to be clipped.
    if spans.iter().map(Span::width).sum::<usize>() > budget {
        for span in &mut spans {
            if span.content.trim().is_empty() {
                span.content = " ".into();
            } else if span.style.bg.is_some() {
                span.content = span.content.trim().to_owned().into();
            }
        }
    }
    let mut optional = Vec::new();
    match activity {
        AppActivity::Live => {}
        AppActivity::Recording => {
            if let Some(seconds) = app
                .active_recording_interval_seconds()
                .filter(|seconds| *seconds > 1)
            {
                append_recording_interval(&mut optional, seconds, theme);
            }
            optional.push(Span::styled(
                format!(" {}", SPINNER[app.recording_spinner_index % SPINNER.len()]),
                Style::default()
                    .fg(theme.danger)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        AppActivity::LogView => {
            if let Some(seconds) = app.log_view_interval_seconds {
                append_recording_interval(&mut optional, seconds, theme);
            }
        }
    }
    // Activity, stale and pause labels take priority over interval/spinner and names.
    if spans
        .iter()
        .chain(optional.iter())
        .map(Span::width)
        .sum::<usize>()
        <= budget
    {
        spans.extend(optional);
    }
    spans
}

fn activity_label(activity: AppActivity) -> &'static str {
    match activity {
        AppActivity::Live => "LIVE",
        AppActivity::Recording => "REC",
        AppActivity::LogView => "LOG",
    }
}

fn append_recording_interval(spans: &mut Vec<Span<'static>>, seconds: u64, theme: Theme) {
    spans.push(Span::raw(" · "));
    spans.push(Span::styled(
        if seconds > 1 {
            format!("{seconds}s AVG")
        } else {
            "1s".to_string()
        },
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    ));
}

fn stale_span(age_seconds: u64, theme: Theme) -> Span<'static> {
    Span::styled(
        format!(" STALE {age_seconds}s"),
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
    )
}

fn mode_span(label: &'static str, color: ratatui::prelude::Color, theme: Theme) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(theme.background)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderAction {
    File,
    Investigation,
    View,
    Settings,
    Help,
    Processes,
    Network,
    FileUsers,
    More,
}
impl HeaderAction {
    pub(crate) fn label(self, compact: bool) -> &'static str {
        match (self, compact) {
            (Self::File, false) => " File ▾ ",
            (Self::Investigation, false) => " Investigation ▾ ",
            (Self::View, false) => " View ▾ ",
            (Self::Settings, false) => " Settings ▾ ",
            (Self::Help, false) => " Help ",
            (Self::More, false) => " More ▾ ",
            (Self::File, true) => "File▾",
            (Self::Investigation, true) => "Investigation▾",
            (Self::View, true) => "View▾",
            (Self::Settings, true) => "Settings▾",
            (Self::Help, true) => "Help",
            (Self::More, true) => "More▾",
            _ => "",
        }
    }
    pub(crate) fn menu_label(self) -> &'static str {
        match self {
            Self::FileUsers => "Find processes by file",
            Self::File => "File",
            Self::Investigation => "Investigation",
            Self::View => "View",
            Self::Settings => "Settings",
            Self::Help => "Help",
            Self::Processes => "Processes",
            Self::Network => "Network endpoints",
            Self::More => "More",
        }
    }
    pub(crate) fn section(self) -> Option<MainMenuSection> {
        match self {
            Self::File => Some(MainMenuSection::File),
            Self::Investigation => Some(MainMenuSection::Investigation),
            Self::View => Some(MainMenuSection::View),
            Self::Settings => Some(MainMenuSection::Settings),
            Self::Help => Some(MainMenuSection::Help),
            Self::More => Some(MainMenuSection::More),
            _ => None,
        }
    }
    fn access_index(self, compact: bool) -> Option<usize> {
        let offset = usize::from(!compact);
        match self {
            Self::File | Self::Investigation | Self::View => Some(offset),
            Self::Settings => Some(offset + 2),
            _ => None,
        }
    }
    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Self::File => "Alt+F",
            Self::Investigation => "Alt+I",
            Self::View => "Alt+V",
            Self::Settings => "Alt+T",
            Self::Help => "F1",
            Self::Processes => "F2",
            Self::Network => "F3",
            Self::FileUsers => "F4",
            Self::More => "",
        }
    }
}

const CATEGORIES: [HeaderAction; 5] = [
    HeaderAction::File,
    HeaderAction::Investigation,
    HeaderAction::View,
    HeaderAction::Settings,
    HeaderAction::Help,
];

// Reserve critical state independently of names and current freshness/pause state.
// Product branding yields to essential controls below the supported 80-column width.
const IMPORTANT_STATUS_WIDTH: u16 = 38;

fn product_width(area: Rect) -> u16 {
    if area.width >= 80 {
        PRODUCT_LABEL.len() as u16 + 1
    } else {
        0
    }
}

fn menu_area(area: Rect) -> Rect {
    let width = product_width(area);
    Rect::new(area.x + width, area.y, area.width - width, area.height)
}

fn menu_budget(area: Rect) -> u16 {
    area.width
        .saturating_sub(IMPORTANT_STATUS_WIDTH.min(area.width.saturating_sub(12)) + 1)
}

fn compact_menu(area: Rect) -> bool {
    let regular_width = CATEGORIES
        .iter()
        .map(|action| Span::raw(action.label(false)).width() as u16)
        .sum::<u16>()
        + 4;
    regular_width > menu_budget(area)
}

pub(crate) fn menu_sections(screen: Rect, app: &App) -> Vec<MainMenuSection> {
    let mut sections = CATEGORIES
        .into_iter()
        .filter_map(HeaderAction::section)
        .collect::<Vec<_>>();
    if !overflow_actions(screen, app).is_empty() {
        sections.push(MainMenuSection::More);
    }
    sections
}

pub(crate) fn header_actions(area: Rect, _app: &App) -> Vec<(HeaderAction, Rect)> {
    if area.is_empty() {
        return Vec::new();
    }
    let area = menu_area(area);
    let compact = compact_menu(area);
    let budget = menu_budget(area);
    let total = CATEGORIES
        .iter()
        .map(|action| Span::raw(action.label(compact)).width() as u16)
        .sum::<u16>()
        + 4;
    let overflow = total > budget;
    let more_width = Span::raw(HeaderAction::More.label(compact)).width() as u16;
    let limit = if overflow {
        budget.saturating_sub(more_width + 1)
    } else {
        budget
    };
    let mut x = area.x;
    let mut actions = Vec::new();
    for action in CATEGORIES {
        let width = Span::raw(action.label(compact)).width() as u16;
        if x.saturating_sub(area.x) + width > limit && action != HeaderAction::File {
            break;
        }
        let width = width.min(area.width);
        actions.push((action, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(width + 1);
    }
    if overflow && x.saturating_sub(area.x) + more_width <= budget {
        actions.push((HeaderAction::More, Rect::new(x, area.y, more_width, 1)));
    }
    actions
}

pub(crate) fn overflow_actions(screen: Rect, app: &App) -> Vec<HeaderAction> {
    let visible = header_actions(crate::ui::screen_layout(screen)[0], app);
    CATEGORIES
        .into_iter()
        .filter(|action| !visible.iter().any(|(shown, _)| shown == action))
        .collect()
}

pub(crate) fn header_action_at(screen: Rect, app: &App, x: u16, y: u16) -> Option<HeaderAction> {
    header_actions(crate::ui::screen_layout(screen)[0], app)
        .into_iter()
        .find(|(_, rect)| rect.contains(ratatui::layout::Position::new(x, y)))
        .map(|(action, _)| action)
}
