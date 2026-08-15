use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    config::{AppConfig, SavedTrackedList, TrackedConfig},
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
const LEAD_TEXT: &str = "Choose a Tracking List.";

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
enum StartupTrackedListChoice {
    ResumeLast,
    StartEmpty,
    Saved(SavedTrackedList),
}

impl StartupTrackedListChoice {
    fn label(&self) -> String {
        match self {
            Self::ResumeLast => "Last used Tracking List".to_string(),
            Self::StartEmpty => "Empty Tracking List".to_string(),
            Self::Saved(list) => format!(
                "{}  ({} process{})",
                list.name,
                list.processes.len(),
                if list.processes.len() == 1 { "" } else { "es" }
            ),
        }
    }
}

pub(crate) fn choose_startup_tracked_list(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    config: &mut AppConfig,
) -> Result<StartupOutcome> {
    let choices = startup_choices(config);
    let theme = THEMES[theme_index_by_name(&config.general.theme)];
    let mut selected = initial_selection(config, &choices);
    let mut offset = selected.saturating_sub(MAX_LIST_HEIGHT as usize - 1);

    loop {
        terminal.draw(|frame| draw_startup_choice(frame, &choices, selected, offset, theme))?;
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

fn startup_choices(config: &AppConfig) -> Vec<StartupTrackedListChoice> {
    let mut choices = vec![
        StartupTrackedListChoice::ResumeLast,
        StartupTrackedListChoice::StartEmpty,
    ];
    choices.extend(
        config
            .tracked_lists
            .iter()
            .cloned()
            .map(StartupTrackedListChoice::Saved),
    );
    choices
}

fn initial_selection(config: &AppConfig, choices: &[StartupTrackedListChoice]) -> usize {
    let Some(active) = config.tracking.active_list.as_deref() else {
        return 0;
    };
    choices
        .iter()
        .position(|choice| {
            matches!(
                choice,
                StartupTrackedListChoice::Saved(list)
                    if list.name.eq_ignore_ascii_case(active)
            )
        })
        .unwrap_or(0)
}

fn apply_startup_choice(config: &mut AppConfig, choice: StartupTrackedListChoice) {
    match choice {
        StartupTrackedListChoice::ResumeLast => {}
        StartupTrackedListChoice::StartEmpty => {
            config.tracked.clear();
            config.tracking.active_list = None;
        }
        StartupTrackedListChoice::Saved(list) => {
            config.tracked = list
                .processes
                .into_iter()
                .map(|name| TrackedConfig { name })
                .collect();
            config.tracking.active_list = Some(list.name);
        }
    }
}

fn draw_startup_choice(
    frame: &mut ratatui::Frame<'_>,
    choices: &[StartupTrackedListChoice],
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
            let label = format!("{} {}", if is_selected { ">" } else { " " }, choice.label());
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
        let config = AppConfig::default();
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
        assert!(rendered.contains("Last used Tracking List"), "{rendered}");
        assert!(rendered.contains("Empty Tracking List"), "{rendered}");
        assert!(!rendered.contains("Resume last Tracking List"));
        assert!(!rendered.contains("Start with empty Tracking List"));
        assert!(!rendered.contains("CHOOSE TRACKING LIST"));
        assert!(!rendered.contains("START MENU"));
        assert!(!rendered.contains("Last working Tracking List"));
        assert!(
            rendered.contains("↑/↓ Move  Enter Start  Esc Quit"),
            "{rendered}"
        );
        assert!(!rendered.contains("[ Start ]"), "{rendered}");
        assert!(!rendered.contains("[ Quit ]"), "{rendered}");
        assert!(!rendered.contains("Keep the last working Tracking List."));
        assert!(!rendered.contains("Choose the Tracking List to apply"));
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
        assert!(layout.shortcuts.bottom() <= layout.popup.bottom() - 1);
        assert_eq!(
            startup_choice_index_at(screen, layout.list.x + 1, layout.list.y + 2, 0, 4),
            Some(2)
        );
    }

    #[test]
    fn startup_selection_uses_highlight_and_list_hit_testing() {
        let config = AppConfig::default();
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
    fn saved_startup_choice_replaces_active_working_list() {
        let mut config = AppConfig::default();
        config.tracked.push(TrackedConfig {
            name: "old.exe".to_string(),
        });
        apply_startup_choice(
            &mut config,
            StartupTrackedListChoice::Saved(SavedTrackedList {
                name: "API".to_string(),
                processes: vec!["api.exe".to_string(), "worker.exe".to_string()],
            }),
        );

        assert_eq!(config.tracking.active_list.as_deref(), Some("API"));
        assert_eq!(
            config
                .tracked
                .iter()
                .map(|process| process.name.as_str())
                .collect::<Vec<_>>(),
            vec!["api.exe", "worker.exe"]
        );
    }

    #[test]
    fn empty_startup_choice_clears_active_working_list() {
        let mut config = AppConfig::default();
        config.tracking.active_list = Some("API".to_string());
        config.tracked.push(TrackedConfig {
            name: "api.exe".to_string(),
        });

        apply_startup_choice(&mut config, StartupTrackedListChoice::StartEmpty);

        assert!(config.tracked.is_empty());
        assert_eq!(config.tracking.active_list, None);
    }
}
