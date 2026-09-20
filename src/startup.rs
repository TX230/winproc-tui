use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Alignment, Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    config::{AppConfig, InvestigationStateConfig, SavedInvestigationProfile},
    terminal::{AppTerminal, draw_frame},
    ui::{
        THEMES,
        footer::shortcut_spans,
        layout::screen_layout,
        theme_index_by_name,
        widgets::{
            block::{panel_block_focused, panel_title},
            confirm_dialog::centered_dialog_rect,
        },
    },
};

const DIALOG_WIDTH: u16 = 68;
const MAX_LIST_HEIGHT: u16 = 9;
const PANEL_CHROME_HEIGHT: u16 = 6;
const LIST_TOP_OFFSET: u16 = 2;
const LEAD_TEXT: &str = "Choose the tracking list to start with.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupOutcome {
    Start,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupLayout {
    header: Rect,
    popup: Rect,
    lead: Rect,
    list: Rect,
    shortcuts: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupInvestigationChoice {
    ResumeLast,
    StartEmpty,
    Saved(SavedInvestigationProfile),
}

impl StartupInvestigationChoice {
    fn label(&self, width: usize) -> String {
        match self {
            Self::ResumeLast => "Previous tracking list".to_string(),
            Self::StartEmpty => "Empty tracking list".to_string(),
            Self::Saved(profile) => {
                let suffix = format!(
                    "  ({} process name{})",
                    profile.tracked_names.len(),
                    if profile.tracked_names.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
                let name = crate::ui::investigation_profiles::truncate(
                    &profile.name,
                    width.saturating_sub(suffix.len()),
                );
                format!("{name}{suffix}")
            }
        }
    }
}

pub(crate) fn choose_startup_investigation(
    terminal: &mut AppTerminal,
    config: &mut AppConfig,
) -> Result<StartupOutcome> {
    let choices = startup_choices(config);
    let theme = THEMES[theme_index_by_name(&config.general.theme)];
    let mut selected = initial_selection(config, &choices);
    let mut offset = selected.saturating_sub(MAX_LIST_HEIGHT as usize - 1);

    loop {
        draw_frame(terminal, |frame| {
            draw_startup_choice(frame, &choices, selected, offset, theme)
        })?;
        let area = terminal.size()?;
        let screen = Rect::new(0, 0, area.width, area.height);
        let page_size = usize::from(startup_layout(screen, choices.len()).list.height).max(1);
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if let Some(outcome) = startup_outcome_for_key(&key.code) {
                    if outcome == StartupOutcome::Start {
                        apply_startup_choice(config, choices[selected].clone());
                    }
                    return Ok(outcome);
                }
                match key.code {
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => {
                        selected = selected
                            .saturating_add(1)
                            .min(choices.len().saturating_sub(1))
                    }
                    KeyCode::PageUp => selected = selected.saturating_sub(page_size),
                    KeyCode::PageDown => {
                        selected = selected
                            .saturating_add(page_size)
                            .min(choices.len().saturating_sub(1))
                    }
                    KeyCode::Home => selected = 0,
                    KeyCode::End => selected = choices.len().saturating_sub(1),
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(index) = startup_choice_index_at(
                        screen,
                        mouse.column,
                        mouse.row,
                        offset,
                        choices.len(),
                    ) {
                        selected = index;
                    }
                }
                MouseEventKind::ScrollUp => {
                    selected = selected.saturating_sub(1);
                }
                MouseEventKind::ScrollDown => {
                    selected = selected
                        .saturating_add(1)
                        .min(choices.len().saturating_sub(1))
                }
                _ => {}
            },
            _ => {}
        }
        offset = ensure_visible_offset(selected, offset, choices.len(), page_size);
    }
}

fn startup_outcome_for_key(code: &KeyCode) -> Option<StartupOutcome> {
    match code {
        KeyCode::Enter => Some(StartupOutcome::Start),
        KeyCode::Esc => Some(StartupOutcome::Quit),
        _ => None,
    }
}

fn startup_choices(config: &AppConfig) -> Vec<StartupInvestigationChoice> {
    let mut choices = vec![
        StartupInvestigationChoice::ResumeLast,
        StartupInvestigationChoice::StartEmpty,
    ];
    choices.extend(
        config
            .investigation_profiles
            .iter()
            .cloned()
            .map(StartupInvestigationChoice::Saved),
    );
    choices
}

fn initial_selection(config: &AppConfig, choices: &[StartupInvestigationChoice]) -> usize {
    let Some(active) = config
        .investigation
        .as_ref()
        .and_then(|investigation| investigation.active_profile.as_deref())
    else {
        return 0;
    };
    choices
        .iter()
        .position(|choice| {
            matches!(
                choice,
                StartupInvestigationChoice::Saved(profile)
                    if profile.name.eq_ignore_ascii_case(active)
            )
        })
        .unwrap_or(0)
}

fn apply_startup_choice(config: &mut AppConfig, choice: StartupInvestigationChoice) {
    let investigation = config
        .investigation
        .as_mut()
        .expect("startup config must be prepared");
    match choice {
        StartupInvestigationChoice::ResumeLast => {
            investigation.active_profile = None;
        }
        StartupInvestigationChoice::StartEmpty => {
            investigation.last = InvestigationStateConfig::default();
            investigation.active_profile = None;
        }
        StartupInvestigationChoice::Saved(profile) => {
            investigation.last = profile.investigation;
            investigation.active_profile = Some(profile.name);
        }
    }
}

fn draw_startup_choice(
    frame: &mut ratatui::Frame<'_>,
    choices: &[StartupInvestigationChoice],
    selected: usize,
    offset: usize,
    theme: crate::ui::Theme,
) {
    let area = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(theme.background)),
        area,
    );
    let layout = startup_layout(area, choices.len());
    draw_startup_header(frame, layout.header, theme);

    let block =
        panel_block_focused(panel_title("STARTUP"), theme, true).title_alignment(Alignment::Center);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(block, layout.popup);
    frame.render_widget(
        Paragraph::new(LEAD_TEXT).style(Style::default().fg(theme.text)),
        layout.lead,
    );

    let lines = choices
        .iter()
        .enumerate()
        .skip(offset)
        .take(layout.list.height as usize)
        .map(|(index, choice)| {
            let is_selected = index == selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.highlight)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let label = format!(
                "{} {}",
                if is_selected { ">" } else { " " },
                choice.label(layout.list.width.saturating_sub(2) as usize)
            );
            let padding =
                usize::from(layout.list.width).saturating_sub(Span::raw(label.as_str()).width());
            Line::from(Span::styled(
                format!("{label}{}", " ".repeat(padding)),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), layout.list);

    let shortcuts = Paragraph::new(Line::from(shortcut_spans(
        &[("↑/↓", "Move"), ("Enter", "Start"), ("Esc", "Quit")],
        theme,
    )))
    .alignment(Alignment::Center);
    frame.render_widget(shortcuts, layout.shortcuts);
}

fn draw_startup_header(frame: &mut ratatui::Frame<'_>, area: Rect, theme: crate::ui::Theme) {
    let product = format!("winproc-tui {}", env!("CARGO_PKG_VERSION"));
    let repository = env!("CARGO_PKG_REPOSITORY")
        .strip_prefix("https://")
        .unwrap_or(env!("CARGO_PKG_REPOSITORY"));
    let product_width = product.chars().count() as u16;
    let repository_width = repository.chars().count() as u16;

    frame.render_widget(
        Paragraph::new(product)
            .style(
                Style::default()
                    .fg(theme.text)
                    .bg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left),
        area,
    );

    if product_width
        .saturating_add(2)
        .saturating_add(repository_width)
        <= area.width
    {
        frame.render_widget(
            Paragraph::new(repository)
                .style(Style::default().fg(theme.muted).bg(theme.background))
                .alignment(Alignment::Right),
            Rect::new(
                area.right().saturating_sub(repository_width),
                area.y,
                repository_width,
                area.height,
            ),
        );
    }
}

fn startup_layout(area: Rect, choice_count: usize) -> StartupLayout {
    let screen = screen_layout(area);
    let header = screen[0];
    let body = screen[1];
    let desired_list_height = (choice_count.max(1) as u16).min(MAX_LIST_HEIGHT);
    let popup = centered_dialog_rect(
        body,
        DIALOG_WIDTH,
        desired_list_height.saturating_add(PANEL_CHROME_HEIGHT),
    );
    let content = popup.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let lead = Rect::new(content.x, content.y, content.width, content.height.min(1));
    let shortcuts = Rect::new(
        content.x,
        content.bottom().saturating_sub(1),
        content.width,
        content.height.min(1),
    );
    let list_bottom = shortcuts.y.saturating_sub(1);
    let list_y = content
        .y
        .saturating_add(LIST_TOP_OFFSET)
        .min(content.bottom());
    let list = Rect::new(
        content.x,
        list_y,
        content.width,
        list_bottom.saturating_sub(list_y).min(desired_list_height),
    );

    StartupLayout {
        header,
        popup,
        lead,
        list,
        shortcuts,
    }
}

fn startup_choice_index_at(
    area: Rect,
    x: u16,
    y: u16,
    offset: usize,
    count: usize,
) -> Option<usize> {
    let list = startup_layout(area, count).list;
    if !contains(list, x, y) {
        return None;
    }
    let index = offset.saturating_add(y.saturating_sub(list.y) as usize);
    (index < count).then_some(index)
}

fn ensure_visible_offset(selected: usize, offset: usize, total: usize, page_size: usize) -> usize {
    let page_size = page_size.max(1);
    let mut offset = offset.min(total.saturating_sub(page_size));
    if selected < offset {
        offset = selected;
    } else if selected >= offset.saturating_add(page_size) {
        offset = selected.saturating_add(1).saturating_sub(page_size);
    }
    offset.min(total.saturating_sub(page_size))
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_startup_choice(selected: usize) -> String {
        let mut config = AppConfig::default();
        crate::config::prepare_app_config(&mut config);
        let choices = startup_choices(&config);
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| draw_startup_choice(frame, &choices, selected, 0, THEMES[0]))
            .expect("startup dialog should render");
        let buffer = terminal.backend().buffer();

        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn long_startup_profile_names_keep_counts_visible() {
        for name in ["Long profile ".repeat(20), "調査プロファイル".repeat(20)] {
            let saved = SavedInvestigationProfile {
                name,
                investigation: InvestigationStateConfig {
                    tracked_names: vec!["api.exe".into()],
                    ..Default::default()
                },
            };
            for width in [80, 120, 180] {
                let choices = vec![StartupInvestigationChoice::Saved(saved.clone())];
                let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                terminal
                    .draw(|frame| draw_startup_choice(frame, &choices, 0, 0, THEMES[0]))
                    .unwrap();
                let buffer = terminal.backend().buffer();
                assert!((0..buffer.area.height).any(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .contains("(1 process name)")
                }));
            }
        }
    }

    #[test]
    fn startup_screen_shows_identity_choices_and_footer_without_redundant_copy() {
        let rendered = render_startup_choice(0);

        assert!(
            rendered.contains(&format!("winproc-tui {}", env!("CARGO_PKG_VERSION"))),
            "{rendered}"
        );
        assert!(
            rendered.contains("github.com/TX230/winproc-tui"),
            "{rendered}"
        );
        assert!(rendered.contains("STARTUP"), "{rendered}");
        assert!(rendered.contains(LEAD_TEXT), "{rendered}");
        assert!(rendered.contains("Previous tracking list"), "{rendered}");
        assert!(rendered.contains("Empty tracking list"), "{rendered}");
        assert!(!rendered.contains("Choose a Tracking List"));
        assert!(!rendered.contains("START MENU"));
        assert!(
            rendered.contains("↑/↓ Move  Enter Start  Esc Quit"),
            "{rendered}"
        );
        assert!(!rendered.contains("[ Start ]"), "{rendered}");
        assert!(!rendered.contains("[ Quit ]"), "{rendered}");
    }

    #[test]
    fn startup_panel_height_and_hit_tests_share_the_compact_layout() {
        let screen = Rect::new(0, 0, 80, 30);
        let layout = startup_layout(screen, 4);

        assert_eq!(layout.popup.height, 10);
        assert_eq!(layout.lead.height, 1);
        assert_eq!(layout.list.y, layout.lead.y + LIST_TOP_OFFSET);
        assert_eq!(layout.list.height, 4);
        assert_eq!(layout.shortcuts.y, layout.popup.bottom() - 2);
        assert_eq!(layout.shortcuts.y, layout.list.bottom() + 1);
        assert!(layout.shortcuts.bottom() < layout.popup.bottom());
        assert_eq!(
            startup_choice_index_at(screen, layout.list.x + 1, layout.list.y + 2, 0, 4),
            Some(2)
        );
    }

    #[test]
    fn startup_selection_uses_highlight_and_list_hit_testing() {
        let mut config = AppConfig::default();
        crate::config::prepare_app_config(&mut config);
        let choices = startup_choices(&config);
        let screen = Rect::new(0, 0, 80, 30);
        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| draw_startup_choice(frame, &choices, 0, 0, THEMES[0]))
            .expect("startup screen should render");

        let buffer = terminal.backend().buffer();
        let list = startup_layout(screen, choices.len()).list;
        assert_eq!(buffer[(list.right() - 1, list.y)].bg, THEMES[0].highlight);
        assert!(buffer[(list.x, list.y)].modifier.contains(Modifier::BOLD));
        assert_eq!(
            startup_choice_index_at(screen, list.x, list.y + 1, 0, choices.len()),
            Some(1)
        );
    }

    #[test]
    fn startup_enter_starts_and_escape_quits() {
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Esc),
            Some(StartupOutcome::Quit)
        );
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Enter),
            Some(StartupOutcome::Start)
        );
    }

    #[test]
    fn saved_startup_choice_replaces_only_the_last_tracking_list() {
        let mut config = AppConfig::default();
        crate::config::prepare_app_config(&mut config);
        config.graphs.time_span_seconds = Some(300);
        let saved = SavedInvestigationProfile {
            name: "API".to_string(),
            investigation: InvestigationStateConfig {
                tracked_names: vec!["api.exe".to_string(), "worker.exe".to_string()],
                ..InvestigationStateConfig::default()
            },
        };
        assert_eq!(
            StartupInvestigationChoice::Saved(saved.clone()).label(usize::MAX),
            "API  (2 process names)"
        );
        apply_startup_choice(&mut config, StartupInvestigationChoice::Saved(saved));

        let investigation = config.investigation.as_ref().unwrap();
        assert_eq!(investigation.active_profile.as_deref(), Some("API"));
        assert_eq!(
            investigation
                .last
                .tracked_names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["api.exe", "worker.exe"]
        );
        assert_eq!(config.graphs.time_span_seconds, Some(300));
    }

    #[test]
    fn empty_startup_choice_resets_only_profile_owned_state() {
        let mut config = AppConfig::default();
        crate::config::prepare_app_config(&mut config);
        let investigation = config.investigation.as_mut().unwrap();
        investigation.active_profile = Some("API".to_string());
        investigation.last.tracked_names = vec!["api.exe".to_string()];
        config.graphs.time_span_seconds = Some(300);

        apply_startup_choice(&mut config, StartupInvestigationChoice::StartEmpty);

        let investigation = config.investigation.as_ref().unwrap();
        assert_eq!(investigation.last, InvestigationStateConfig::default());
        assert_eq!(investigation.active_profile, None);
        assert_eq!(config.graphs.time_span_seconds, Some(300));
    }

    #[test]
    fn resume_last_keeps_state_without_binding_a_profile() {
        let mut config = AppConfig::default();
        crate::config::prepare_app_config(&mut config);
        let investigation = config.investigation.as_mut().unwrap();
        investigation.active_profile = Some("API".to_string());
        investigation.last.tracked_names = vec!["api.exe".to_string()];
        let expected_state = investigation.last.clone();

        apply_startup_choice(&mut config, StartupInvestigationChoice::ResumeLast);

        let investigation = config.investigation.as_ref().unwrap();
        assert_eq!(investigation.last, expected_state);
        assert_eq!(investigation.active_profile, None);
    }
}
