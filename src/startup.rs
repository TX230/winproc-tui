use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    config::{AppConfig, SavedTrackedList, TrackedConfig},
    ui::{
        THEMES, theme_index_by_name,
        widgets::{
            block::panel_block_focused,
            confirm_dialog::{button_areas, button_line, centered_dialog_rect},
        },
    },
};

const DIALOG_WIDTH: u16 = 68;
const DIALOG_HEIGHT: u16 = 18;
const HEADER_ROWS: u16 = 3;
const LIST_HEIGHT: u16 = 9;
const BUTTON_ROW: u16 = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupTrackedListChoice {
    ResumeLast,
    StartEmpty,
    Saved(SavedTrackedList),
}

impl StartupTrackedListChoice {
    fn label(&self) -> String {
        match self {
            Self::ResumeLast => "Resume last session".to_string(),
            Self::StartEmpty => "Start empty".to_string(),
            Self::Saved(list) => format!(
                "{}  ({} process{})",
                list.name,
                list.processes.len(),
                if list.processes.len() == 1 { "" } else { "es" }
            ),
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::ResumeLast => "Keep the last working Tracking List.",
            Self::StartEmpty => "Start without tracked process names.",
            Self::Saved(_) => "Load this saved Tracking List.",
        }
    }
}

pub(crate) fn choose_startup_tracked_list(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    config: &mut AppConfig,
) -> Result<()> {
    let choices = startup_choices(config);
    let theme = THEMES[theme_index_by_name(&config.general.theme)];
    let mut selected = initial_selection(config, &choices);
    let mut offset = selected.saturating_sub(LIST_HEIGHT as usize - 1);

    loop {
        terminal.draw(|frame| draw_startup_choice(frame, &choices, selected, offset, theme))?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => {
                    apply_startup_choice(config, StartupTrackedListChoice::ResumeLast);
                    return Ok(());
                }
                KeyCode::Enter => {
                    apply_startup_choice(config, choices[selected].clone());
                    return Ok(());
                }
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    selected = selected
                        .saturating_add(1)
                        .min(choices.len().saturating_sub(1))
                }
                KeyCode::PageUp => selected = selected.saturating_sub(LIST_HEIGHT as usize),
                KeyCode::PageDown => {
                    selected = selected
                        .saturating_add(LIST_HEIGHT as usize)
                        .min(choices.len().saturating_sub(1))
                }
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = choices.len().saturating_sub(1),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let area = terminal.size()?;
                    let screen = Rect::new(0, 0, area.width, area.height);
                    if let Some(index) = startup_choice_index_at(
                        screen,
                        mouse.column,
                        mouse.row,
                        offset,
                        choices.len(),
                    ) {
                        selected = index;
                    } else if startup_start_button_area(screen)
                        .is_some_and(|button| contains(button, mouse.column, mouse.row))
                    {
                        apply_startup_choice(config, choices[selected].clone());
                        return Ok(());
                    }
                }
                MouseEventKind::ScrollUp => selected = selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    selected = selected
                        .saturating_add(1)
                        .min(choices.len().saturating_sub(1))
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
        offset = ensure_visible_offset(selected, offset, choices.len());
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
    let popup = startup_dialog_area(area);
    let block = panel_block_focused("Choose Tracking List", theme, true);
    let content = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Choose the Tracking List to apply before the first sample.")
            .style(Style::default().fg(theme.muted)),
        row(content, 0),
    );
    frame.render_widget(
        Paragraph::new(choices[selected].description()).style(Style::default().fg(theme.text)),
        row(content, 1),
    );
    frame.render_widget(
        Paragraph::new("Up/Down move · Enter start · Esc resume last")
            .style(Style::default().fg(theme.muted)),
        row(content, 2),
    );

    let lines = choices
        .iter()
        .enumerate()
        .skip(offset)
        .take(LIST_HEIGHT as usize)
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
            Line::from(Span::styled(
                format!("{} {}", if is_selected { ">" } else { " " }, choice.label()),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            content.x,
            content.y.saturating_add(HEADER_ROWS),
            content.width,
            LIST_HEIGHT.min(content.height.saturating_sub(HEADER_ROWS)),
        ),
    );
    frame.render_widget(
        Paragraph::new(button_line(&[(" Start ", true)], theme)).alignment(Alignment::Center),
        row(content, BUTTON_ROW),
    );
}

fn startup_choice_index_at(
    area: Rect,
    x: u16,
    y: u16,
    offset: usize,
    count: usize,
) -> Option<usize> {
    let popup = startup_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let list = Rect::new(
        content.x,
        content.y.saturating_add(HEADER_ROWS),
        content.width,
        LIST_HEIGHT.min(content.height.saturating_sub(HEADER_ROWS)),
    );
    if !contains(list, x, y) {
        return None;
    }
    let index = offset.saturating_add(y.saturating_sub(list.y) as usize);
    (index < count).then_some(index)
}

fn startup_start_button_area(area: Rect) -> Option<Rect> {
    let popup = startup_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    button_areas(content, BUTTON_ROW, &[" Start "])
        .into_iter()
        .next()
}

fn startup_dialog_area(area: Rect) -> Rect {
    centered_dialog_rect(area, DIALOG_WIDTH, DIALOG_HEIGHT)
}

fn ensure_visible_offset(selected: usize, offset: usize, total: usize) -> usize {
    let page_size = LIST_HEIGHT as usize;
    let mut offset = offset.min(total.saturating_sub(page_size));
    if selected < offset {
        offset = selected;
    } else if selected >= offset.saturating_add(page_size) {
        offset = selected.saturating_add(1).saturating_sub(page_size);
    }
    offset.min(total.saturating_sub(page_size))
}

fn row(content: Rect, offset: u16) -> Rect {
    Rect::new(
        content.x,
        content.y.saturating_add(offset),
        content.width,
        1,
    )
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
    fn startup_choice_explains_resume_and_empty_effects() {
        let resume = render_startup_choice(0);
        assert!(resume.contains("Keep the last working Tracking List."));

        let empty = render_startup_choice(1);
        assert!(empty.contains("Start without tracked process names."));
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
