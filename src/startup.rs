use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Margin, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
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
            confirm_dialog::{button_areas, button_line_with_hover, centered_dialog_rect},
        },
    },
};

const DIALOG_WIDTH: u16 = 68;
const MAX_LIST_HEIGHT: u16 = 9;
const PANEL_CHROME_HEIGHT: u16 = 7;
const LIST_TOP_OFFSET: u16 = 2;
const LEAD_TEXT: &str = "Choose a Tracking List.";
const START_BUTTON_LABEL: &str = " Start ";
const QUIT_BUTTON_LABEL: &str = " Quit ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupOutcome {
    Start,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupFocus {
    List,
    StartButton,
    QuitButton,
}

impl StartupFocus {
    fn button(self) -> Option<StartupButton> {
        match self {
            Self::List => None,
            Self::StartButton => Some(StartupButton::Start),
            Self::QuitButton => Some(StartupButton::Quit),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupButton {
    Start,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupLayout {
    header: Rect,
    popup: Rect,
    lead: Rect,
    list: Rect,
    start_button: Option<Rect>,
    quit_button: Option<Rect>,
    footer: Rect,
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
    let mut focus = StartupFocus::List;
    let mut hovered_button = None;

    loop {
        terminal.draw(|frame| {
            draw_startup_choice(
                frame,
                &choices,
                selected,
                offset,
                focus,
                hovered_button,
                theme,
            )
        })?;
        let area = terminal.size()?;
        let screen = Rect::new(0, 0, area.width, area.height);
        let page_size = usize::from(startup_layout(screen, choices.len()).list.height).max(1);
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if let Some(outcome) = startup_outcome_for_key(&key.code, focus) {
                    if outcome == StartupOutcome::Start {
                        apply_startup_choice(config, choices[selected].clone());
                    }
                    return Ok(outcome);
                }
                if let Some(next_focus) = startup_focus_for_key(focus, &key.code) {
                    focus = next_focus;
                } else if focus == StartupFocus::List {
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
            }
            Event::Mouse(mouse) => {
                hovered_button = startup_button_at(screen, choices.len(), mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(index) = startup_choice_index_at(
                            screen,
                            mouse.column,
                            mouse.row,
                            offset,
                            choices.len(),
                        ) {
                            selected = index;
                            focus = StartupFocus::List;
                        } else if let Some(button) = hovered_button {
                            let outcome = startup_outcome_for_button(button);
                            if outcome == StartupOutcome::Start {
                                apply_startup_choice(config, choices[selected].clone());
                            }
                            return Ok(outcome);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        focus = StartupFocus::List;
                        selected = selected.saturating_sub(1);
                    }
                    MouseEventKind::ScrollDown => {
                        focus = StartupFocus::List;
                        selected = selected
                            .saturating_add(1)
                            .min(choices.len().saturating_sub(1))
                    }
                    _ => {}
                }
            }
            Event::Resize(_, _) => hovered_button = None,
            _ => {}
        }
        offset = ensure_visible_offset(selected, offset, choices.len(), page_size);
    }
}

fn startup_outcome_for_key(code: &KeyCode, focus: StartupFocus) -> Option<StartupOutcome> {
    match code {
        KeyCode::Enter => Some(match focus {
            StartupFocus::QuitButton => StartupOutcome::Quit,
            StartupFocus::List | StartupFocus::StartButton => StartupOutcome::Start,
        }),
        KeyCode::Esc => Some(StartupOutcome::Quit),
        _ => None,
    }
}

fn startup_outcome_for_button(button: StartupButton) -> StartupOutcome {
    match button {
        StartupButton::Start => StartupOutcome::Start,
        StartupButton::Quit => StartupOutcome::Quit,
    }
}

fn startup_focus_for_key(current: StartupFocus, code: &KeyCode) -> Option<StartupFocus> {
    match code {
        KeyCode::Tab => Some(match current {
            StartupFocus::List => StartupFocus::StartButton,
            StartupFocus::StartButton => StartupFocus::QuitButton,
            StartupFocus::QuitButton => StartupFocus::List,
        }),
        KeyCode::BackTab => Some(match current {
            StartupFocus::List => StartupFocus::QuitButton,
            StartupFocus::StartButton => StartupFocus::List,
            StartupFocus::QuitButton => StartupFocus::StartButton,
        }),
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
    focus: StartupFocus,
    hovered_button: Option<StartupButton>,
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
            let style = if is_selected && focus == StartupFocus::List {
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

    let highlighted_button = hovered_button.or(focus.button());
    for (button, label, area) in [
        (
            StartupButton::Start,
            START_BUTTON_LABEL,
            layout.start_button,
        ),
        (StartupButton::Quit, QUIT_BUTTON_LABEL, layout.quit_button),
    ] {
        if let Some(area) = area {
            frame.render_widget(
                Paragraph::new(button_line_with_hover(
                    &[(label, false)],
                    (highlighted_button == Some(button)).then_some(0),
                    theme,
                )),
                area,
            );
        }
    }

    let enter_action = if focus == StartupFocus::QuitButton {
        "Quit"
    } else {
        "Start"
    };
    let footer = Paragraph::new(Line::from(shortcut_spans(
        &[
            ("Up/Down", "Move"),
            ("Tab", "Focus"),
            ("Enter", enter_action),
            ("Esc", "Quit"),
        ],
        theme,
    )))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background)),
    );
    frame.render_widget(footer, layout.footer);
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
    let footer = screen[2];
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
    let buttons = (content.height >= 2)
        .then(|| {
            button_areas(
                content,
                content.height - 2,
                &[START_BUTTON_LABEL, QUIT_BUTTON_LABEL],
            )
        })
        .unwrap_or_default();
    let start_button = buttons.first().copied();
    let quit_button = buttons.get(1).copied();
    let list_bottom = start_button
        .or(quit_button)
        .map(|button| button.y.saturating_sub(1))
        .unwrap_or(content.bottom());
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
        start_button,
        quit_button,
        footer,
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

fn startup_button_area(area: Rect, choice_count: usize, button: StartupButton) -> Option<Rect> {
    let layout = startup_layout(area, choice_count);
    match button {
        StartupButton::Start => layout.start_button,
        StartupButton::Quit => layout.quit_button,
    }
}

fn startup_button_at(area: Rect, choice_count: usize, x: u16, y: u16) -> Option<StartupButton> {
    [StartupButton::Start, StartupButton::Quit]
        .into_iter()
        .find(|button| {
            startup_button_area(area, choice_count, *button)
                .is_some_and(|button_area| contains(button_area, x, y))
        })
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
            .draw(|frame| {
                draw_startup_choice(
                    frame,
                    &choices,
                    selected,
                    0,
                    StartupFocus::List,
                    None,
                    THEMES[0],
                )
            })
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
            rendered.contains("Up/Down Move  Tab Focus  Enter Start  Esc Quit"),
            "{rendered}"
        );
        assert!(rendered.contains("[ Start ]"), "{rendered}");
        assert!(rendered.contains("[ Quit ]"), "{rendered}");
        assert!(!rendered.contains("Keep the last working Tracking List."));
        assert!(!rendered.contains("Choose the Tracking List to apply"));
    }

    #[test]
    fn startup_panel_height_and_hit_tests_share_the_compact_layout() {
        let screen = Rect::new(0, 0, 80, 30);
        let layout = startup_layout(screen, 4);

        assert_eq!(layout.popup.height, 11);
        assert_eq!(layout.lead.height, 1);
        assert_eq!(layout.list.y, layout.lead.y + LIST_TOP_OFFSET);
        assert_eq!(layout.list.height, 4);
        assert_eq!(
            startup_choice_index_at(screen, layout.list.x + 1, layout.list.y + 2, 0, 4),
            Some(2)
        );
        assert_eq!(
            startup_button_area(screen, 4, StartupButton::Start),
            layout.start_button
        );
        assert_eq!(
            startup_button_area(screen, 4, StartupButton::Quit),
            layout.quit_button
        );
        assert!(
            layout.start_button.expect("Start should fit").x
                < layout.quit_button.expect("Quit should fit").x
        );
    }

    #[test]
    fn startup_buttons_use_common_focus_and_hover_style_and_mouse_hit_areas() {
        let config = AppConfig::default();
        let choices = startup_choices(&config);
        let screen = Rect::new(0, 0, 80, 30);
        let backend = TestBackend::new(screen.width, screen.height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| {
                draw_startup_choice(frame, &choices, 0, 0, StartupFocus::List, None, THEMES[0])
            })
            .expect("startup screen should render");

        let start_button = startup_button_area(screen, choices.len(), StartupButton::Start)
            .expect("start button should have a hit-test area");
        let quit_button = startup_button_area(screen, choices.len(), StartupButton::Quit)
            .expect("quit button should have a hit-test area");
        assert_eq!(
            startup_button_at(screen, choices.len(), start_button.x, start_button.y),
            Some(StartupButton::Start)
        );
        assert_eq!(
            startup_button_at(screen, choices.len(), quit_button.x, quit_button.y),
            Some(StartupButton::Quit)
        );
        assert_eq!(
            startup_button_at(
                screen,
                choices.len(),
                start_button.x.saturating_sub(1),
                start_button.y
            ),
            None
        );
        {
            let buffer = terminal.backend().buffer();
            for button in [start_button, quit_button] {
                let cell = &buffer[(button.x, button.y)];
                assert_eq!(cell.symbol(), "[");
                assert_eq!(cell.fg, THEMES[0].text);
                assert_eq!(cell.bg, THEMES[0].panel_alt);
                assert_ne!(cell.bg, THEMES[0].warning);
            }

            let list = startup_layout(screen, choices.len()).list;
            assert_eq!(buffer[(list.right() - 1, list.y)].bg, THEMES[0].highlight);
            assert_ne!(
                buffer[(list.right() - 1, list.y + 1)].bg,
                THEMES[0].highlight
            );
        }

        terminal
            .draw(|frame| {
                draw_startup_choice(
                    frame,
                    &choices,
                    0,
                    0,
                    StartupFocus::StartButton,
                    None,
                    THEMES[0],
                )
            })
            .expect("Start-focused startup screen should render");
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(start_button.x, start_button.y)].bg,
            THEMES[0].focus_surface
        );
        assert!(
            buffer[(start_button.x, start_button.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            buffer[(quit_button.x, quit_button.y)].bg,
            THEMES[0].panel_alt
        );
        let list = startup_layout(screen, choices.len()).list;
        assert_ne!(buffer[(list.right() - 1, list.y)].bg, THEMES[0].highlight);

        terminal
            .draw(|frame| {
                draw_startup_choice(
                    frame,
                    &choices,
                    0,
                    0,
                    StartupFocus::QuitButton,
                    None,
                    THEMES[0],
                )
            })
            .expect("Quit-focused startup screen should render");
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(start_button.x, start_button.y)].bg,
            THEMES[0].panel_alt
        );
        assert_eq!(
            buffer[(quit_button.x, quit_button.y)].bg,
            THEMES[0].focus_surface
        );
        assert!(
            buffer[(quit_button.x, quit_button.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Enter Quit"), "{rendered}");

        terminal
            .draw(|frame| {
                draw_startup_choice(
                    frame,
                    &choices,
                    0,
                    0,
                    StartupFocus::List,
                    Some(StartupButton::Quit),
                    THEMES[0],
                )
            })
            .expect("Quit-hovered startup screen should render");
        let hovered = &terminal.backend().buffer()[(quit_button.x, quit_button.y)];
        assert_eq!(hovered.bg, THEMES[0].focus_surface);
        assert!(hovered.modifier.contains(Modifier::BOLD));
        assert_ne!(hovered.bg, THEMES[0].warning);
    }

    #[test]
    fn startup_escape_and_quit_button_exit_without_starting_a_choice() {
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Esc, StartupFocus::List),
            Some(StartupOutcome::Quit)
        );
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Enter, StartupFocus::List),
            Some(StartupOutcome::Start)
        );
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Enter, StartupFocus::StartButton),
            Some(StartupOutcome::Start)
        );
        assert_eq!(
            startup_outcome_for_key(&KeyCode::Enter, StartupFocus::QuitButton),
            Some(StartupOutcome::Quit)
        );
        assert_eq!(
            startup_outcome_for_button(StartupButton::Start),
            StartupOutcome::Start
        );
        assert_eq!(
            startup_outcome_for_button(StartupButton::Quit),
            StartupOutcome::Quit
        );
    }

    #[test]
    fn startup_tab_and_backtab_cycle_list_start_and_quit_focus() {
        assert_eq!(
            startup_focus_for_key(StartupFocus::List, &KeyCode::Tab),
            Some(StartupFocus::StartButton)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::StartButton, &KeyCode::Tab),
            Some(StartupFocus::QuitButton)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::QuitButton, &KeyCode::Tab),
            Some(StartupFocus::List)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::List, &KeyCode::BackTab),
            Some(StartupFocus::QuitButton)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::QuitButton, &KeyCode::BackTab),
            Some(StartupFocus::StartButton)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::StartButton, &KeyCode::BackTab),
            Some(StartupFocus::List)
        );
        assert_eq!(
            startup_focus_for_key(StartupFocus::List, &KeyCode::Down),
            None
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
