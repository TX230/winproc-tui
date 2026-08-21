use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    App,
    app::TrackedListsView,
    config::{EMPTY_TRACKED_LIST_NAME, TrackedListStartup},
    ui::{
        Theme,
        footer::{shortcut_spans, warning_shortcut_spans},
        widgets::{
            block::{panel_block, panel_block_focused, panel_title},
            confirm_dialog::{self, centered_dialog_rect},
        },
    },
};

const DIALOG_WIDTH: u16 = 74;
const DIALOG_HEIGHT: u16 = 24;
const LOAD_AREA_ROW: u16 = 0;
const LOAD_AREA_HEIGHT: u16 = 11;
const LOAD_INSTRUCTION_ROW: u16 = 0;
const LIST_ROW: u16 = 2;
const LIST_HEIGHT: u16 = 7;
const SAVE_AREA_ROW: u16 = 11;
const SAVE_AREA_HEIGHT: u16 = 6;
const SAVE_SUMMARY_ROW: u16 = 0;
const SAVE_INPUT_ROW: u16 = 1;
const SAVE_ERROR_ROW: u16 = 2;
const STARTUP_AREA_ROW: u16 = 17;
const STARTUP_AREA_HEIGHT: u16 = 3;
const SHORTCUT_ROW: u16 = 21;
const SAVE_NAME_LABEL: &str = "List name: ";
const LIST_NAME_WIDTH: usize = 22;
const MIN_PROCESS_PREVIEW_WIDTH: usize = 12;
const NAME_DIALOG_WIDTH: u16 = 58;
const NAME_DIALOG_HEIGHT: u16 = 8;
const NAME_INPUT_ROW: u16 = 2;
const NAME_ERROR_ROW: u16 = 3;
const NAME_SHORTCUT_ROW: u16 = 5;
const CONFIRM_DIALOG_WIDTH: u16 = 64;
const CONFIRM_DIALOG_HEIGHT: u16 = 8;
const CONFIRM_SHORTCUT_ROW: u16 = 5;

pub(crate) fn draw_tracked_lists(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(view) = app.tracked_lists_view() else {
        return;
    };
    match view {
        TrackedListsView::Browse => draw_browse(frame, area, app, theme),
        TrackedListsView::NameInput { .. } => {
            draw_browse(frame, area, app, theme);
            draw_name_input(frame, area, app, theme);
        }
        TrackedListsView::ConfirmDelete { .. } => {
            draw_browse(frame, area, app, theme);
            draw_delete_confirm(frame, area, app, theme);
        }
        TrackedListsView::ConfirmSwitch { .. } => {
            draw_browse(frame, area, app, theme);
            draw_switch_confirm(frame, area, app, theme);
        }
    }
}

fn draw_browse(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let popup = tracked_lists_dialog_area(area);
    let block = panel_block_focused(panel_title("TRACKING LISTS"), theme, true);
    let content = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let load_area = load_area(content);
    let load_block = panel_block(panel_title("LOAD TRACKING LIST"), theme);
    let load_content = load_block.inner(load_area);
    frame.render_widget(load_block, load_area);
    frame.render_widget(
        Paragraph::new("Select a Tracking List to load.").style(Style::default().fg(theme.muted)),
        row(load_content, LOAD_INSTRUCTION_ROW),
    );

    let list_area = Rect::new(
        load_content.x,
        load_content.y.saturating_add(LIST_ROW),
        load_content.width,
        LIST_HEIGHT.min(load_content.height.saturating_sub(LIST_ROW)),
    );
    let offset = app.tracked_lists_scroll_offset();
    let selected = app.tracked_lists_index();
    let list_focused =
        !app.tracked_lists_save_name_focused() && !app.tracked_lists_startup_focused();
    let lines = (offset..app.tracked_lists_entry_count())
        .take(list_area.height as usize)
        .map(|index| {
            let (name, processes, is_active) = if index == 0 {
                (
                    EMPTY_TRACKED_LIST_NAME,
                    &[][..],
                    app.empty_tracked_list_active(),
                )
            } else {
                let list = &app.runtime.saved_tracked_lists[index - 1];
                let is_active = app
                    .runtime
                    .active_tracked_list
                    .as_deref()
                    .is_some_and(|active| active.eq_ignore_ascii_case(&list.name));
                (list.name.as_str(), list.processes.as_slice(), is_active)
            };
            let is_selected = index == selected;
            let style = if is_selected && list_focused {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.highlight)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let cursor = if is_selected { ">" } else { " " };
            Line::from(Span::styled(
                tracked_list_row_text(cursor, name, processes, is_active, list_area.width as usize),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), list_area);

    let save_area = save_area(content);
    let save_block = panel_block(panel_title("SAVE CURRENT TRACKING LIST"), theme);
    let save_content = save_block.inner(save_area);
    frame.render_widget(save_block, save_area);
    let current = app
        .runtime
        .active_tracked_list
        .as_deref()
        .map(|name| {
            format!(
                "Current: {name}{} · {} process{}",
                if app.active_tracked_list_dirty() {
                    " (modified)"
                } else {
                    ""
                },
                app.watch_list.len(),
                if app.watch_list.len() == 1 { "" } else { "es" }
            )
        })
        .unwrap_or_else(|| {
            let name = if app.watch_list.is_empty() {
                EMPTY_TRACKED_LIST_NAME
            } else {
                "Unsaved"
            };
            format!(
                "Current: {name} · {} process{}",
                app.watch_list.len(),
                if app.watch_list.len() == 1 { "" } else { "es" }
            )
        });
    frame.render_widget(
        Paragraph::new(current).style(Style::default().fg(theme.muted)),
        row(save_content, SAVE_SUMMARY_ROW),
    );

    draw_save_name_input(frame, save_content, app, theme);

    let startup_area = startup_area(content);
    let startup_block = panel_block(panel_title("TRACKING LIST STARTUP"), theme);
    let startup_content = startup_block.inner(startup_area);
    frame.render_widget(startup_block, startup_area);
    frame.render_widget(
        Paragraph::new(startup_radio_line(app, theme)),
        row(startup_content, 0),
    );
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(
            &tracked_lists_shortcuts(app),
            theme,
        ))),
        row(content, SHORTCUT_ROW),
    );
}

fn draw_save_name_input(
    frame: &mut ratatui::Frame<'_>,
    save_content: Rect,
    app: &App,
    theme: Theme,
) {
    let Some((draft, cursor, error)) = app.tracked_lists_save_name() else {
        return;
    };
    let (label_area, input_area) = save_name_row_areas(save_content);
    frame.render_widget(
        Paragraph::new(SAVE_NAME_LABEL).style(Style::default().fg(theme.text)),
        label_area,
    );
    let input_width = input_area.width.saturating_sub(2) as usize;
    let (input, cursor_x) = input_view(draft, cursor, input_width);
    let padded = format!(" {input:<input_width$} ");
    let input_style = if app.tracked_lists_save_name_focused() {
        Style::default()
            .fg(theme.text)
            .bg(theme.focus_surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    };
    frame.render_widget(Paragraph::new(padded).style(input_style), input_area);
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(error).style(Style::default().fg(theme.danger)),
            row(save_content, SAVE_ERROR_ROW),
        );
    } else if let Some(feedback) = app.tracked_lists_save_feedback() {
        frame.render_widget(
            Paragraph::new(feedback).style(Style::default().fg(theme.success)),
            row(save_content, SAVE_ERROR_ROW),
        );
    }
    if app.tracked_lists_save_name_focused() {
        frame.set_cursor_position(Position::new(
            input_area
                .x
                .saturating_add(1)
                .saturating_add(cursor_x as u16),
            input_area.y,
        ));
    }
}

fn startup_radio_line(app: &App, theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, startup) in TrackedListStartup::ALL.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let selected = startup == app.runtime.tracked_list_startup;
        let marker = if selected { "(*)" } else { "( )" };
        let style = if selected && app.tracked_lists_startup_focused() {
            Style::default()
                .fg(theme.text)
                .bg(theme.focus_surface)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        spans.push(Span::styled(format!("{marker} {}", startup.label()), style));
    }
    Line::from(spans)
}

fn tracked_lists_shortcuts(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.tracked_lists_save_name_focused() {
        vec![("Enter", "Save"), ("Tab", "Focus"), ("Esc", "Close")]
    } else if app.tracked_lists_startup_focused() {
        vec![
            ("←/→", "Select"),
            ("Space", "Select"),
            ("Tab", "Focus"),
            ("Enter/Esc", "Close"),
        ]
    } else {
        vec![
            ("↑/↓", "Select"),
            ("Enter", "Load"),
            ("F2", "Rename"),
            ("Del", "Delete"),
            ("Tab", "Focus"),
            ("Esc", "Close"),
        ]
    }
}

fn tracked_list_row_text(
    cursor: &str,
    name: &str,
    processes: &[String],
    is_active: bool,
    width: usize,
) -> String {
    const PREFIX_WIDTH: usize = 2;
    const COLUMN_GAP: usize = 2;

    if width <= PREFIX_WIDTH + COLUMN_GAP {
        return truncate_end(name, width);
    }

    let available = width.saturating_sub(PREFIX_WIDTH + COLUMN_GAP);
    let name_width = LIST_NAME_WIDTH.min(available.saturating_sub(MIN_PROCESS_PREVIEW_WIDTH));
    let process_width = available.saturating_sub(name_width);
    if name_width == 0 || process_width == 0 {
        return truncate_end(name, width);
    }

    let name = tracked_list_name_label(name, is_active, name_width);
    let processes = process_name_summary(processes, process_width);
    format!("{cursor} {name:<name_width$}  {processes:<process_width$}")
}

fn tracked_list_name_label(name: &str, is_active: bool, width: usize) -> String {
    const ACTIVE_LABEL: &str = " (*)";

    if !is_active {
        return truncate_end(name, width);
    }
    if width <= ACTIVE_LABEL.len() {
        return truncate_end(ACTIVE_LABEL.trim(), width);
    }
    let name = truncate_end(name, width.saturating_sub(ACTIVE_LABEL.len()));
    format!("{name}{ACTIVE_LABEL}")
}

fn process_name_summary(processes: &[String], width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if processes.is_empty() {
        return truncate_end("(empty)", width);
    }

    let full = processes.join(", ");
    if full.chars().count() <= width {
        return full;
    }

    for included in (1..processes.len()).rev() {
        let remaining = processes.len().saturating_sub(included);
        let candidate = format!("{}, ... (+{remaining})", processes[..included].join(", "));
        if candidate.chars().count() <= width {
            return candidate;
        }
    }

    if processes.len() == 1 {
        return truncate_end(&processes[0], width);
    }
    let suffix = format!(", ... (+{})", processes.len().saturating_sub(1));
    if suffix.chars().count() >= width {
        return truncate_end(&processes[0], width);
    }
    let first_width = width.saturating_sub(suffix.chars().count());
    format!("{}{}", truncate_end(&processes[0], first_width), suffix)
}

fn truncate_end(value: &str, width: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let head = value
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    format!("{head}...")
}

fn draw_name_input(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(TrackedListsView::NameInput {
        draft,
        cursor,
        error,
    }) = app.tracked_lists_view()
    else {
        return;
    };
    let popup = centered_dialog_rect(area, NAME_DIALOG_WIDTH, NAME_DIALOG_HEIGHT);
    let block = panel_block_focused(panel_title("RENAME TRACKING LIST"), theme, true);
    let content = block.inner(popup);
    let input_area = row(content, NAME_INPUT_ROW);
    let (input, cursor_x) = input_view(draft, *cursor, input_area.width as usize);

    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Enter a new name.").style(Style::default().fg(theme.text)),
        row(content, 0),
    );
    frame.render_widget(
        Paragraph::new("Name").style(Style::default().fg(theme.muted)),
        row(content, 1),
    );
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(theme.text).bg(theme.panel_alt)),
        input_area,
    );
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(theme.danger)),
            row(content, NAME_ERROR_ROW),
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(
            &[("Enter", "Save"), ("Esc", "Cancel")],
            theme,
        ))),
        row(content, NAME_SHORTCUT_ROW),
    );
    frame.set_cursor_position(Position::new(
        input_area.x.saturating_add(cursor_x as u16),
        input_area.y,
    ));
}

fn draw_delete_confirm(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(TrackedListsView::ConfirmDelete { name, .. }) = app.tracked_lists_view() else {
        return;
    };
    draw_confirm(
        frame,
        area,
        "DELETE SAVED TRACKING LIST?",
        &format!("Delete \"{name}\"? The working Tracking List is kept."),
        "This cannot be undone.",
        "Delete",
        theme,
    );
}

fn draw_switch_confirm(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(TrackedListsView::ConfirmSwitch { pending, .. }) = app.tracked_lists_view() else {
        return;
    };
    draw_confirm(
        frame,
        area,
        "LOAD TRACKING LIST?",
        &format!(
            "This removes {} tracked name{}.",
            pending.removed_name_count,
            if pending.removed_name_count == 1 {
                ""
            } else {
                "s"
            }
        ),
        &format!(
            "{} older sample{} across {} tracked name{} will be discarded.",
            pending.discarded_sample_count,
            if pending.discarded_sample_count == 1 {
                ""
            } else {
                "s"
            },
            pending.affected_name_count,
            if pending.affected_name_count == 1 {
                ""
            } else {
                "s"
            }
        ),
        "Load",
        theme,
    );
}

fn draw_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &'static str,
    message: &str,
    detail: &str,
    apply_shortcut_label: &'static str,
    theme: Theme,
) {
    let popup = centered_dialog_rect(area, CONFIRM_DIALOG_WIDTH, CONFIRM_DIALOG_HEIGHT);
    let block = confirm_dialog::warning_block(title, theme);
    let content = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(message).alignment(Alignment::Center),
        row(content, 1),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.warning)),
        row(content, 2),
    );
    frame.render_widget(
        Paragraph::new(Line::from(warning_shortcut_spans(
            &[("Enter/Esc/n", "Cancel"), ("y", apply_shortcut_label)],
            theme,
        )))
        .alignment(Alignment::Center),
        row(content, CONFIRM_SHORTCUT_ROW),
    );
}

pub(crate) fn tracked_lists_page_size_for_screen(area: Rect) -> usize {
    let popup = tracked_lists_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let load = load_area(content).inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    LIST_HEIGHT.min(load.height.saturating_sub(LIST_ROW)).max(1) as usize
}

pub(crate) fn tracked_list_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll_offset: usize,
    list_count: usize,
) -> Option<usize> {
    let popup = tracked_lists_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let load = load_area(content).inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let list = Rect::new(
        load.x,
        load.y.saturating_add(LIST_ROW),
        load.width,
        LIST_HEIGHT.min(load.height.saturating_sub(LIST_ROW)),
    );
    if x < list.x || x >= list.right() || y < list.y || y >= list.bottom() {
        return None;
    }
    let index = scroll_offset.saturating_add(y.saturating_sub(list.y) as usize);
    (index < list_count).then_some(index)
}

pub(crate) fn tracked_list_save_name_area_for_screen(area: Rect) -> Option<Rect> {
    let popup = tracked_lists_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let save = save_area(content).inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (_, input) = save_name_row_areas(save);
    (input.width > 0).then_some(input)
}

pub(crate) fn tracked_list_startup_at_for_screen(
    area: Rect,
    x: u16,
    y: u16,
) -> Option<TrackedListStartup> {
    let popup = tracked_lists_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let startup_content = startup_area(content).inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    startup_option_areas(row(startup_content, 0))
        .into_iter()
        .find_map(|(startup, option)| contains(option, x, y).then_some(startup))
}

fn tracked_lists_dialog_area(area: Rect) -> Rect {
    centered_dialog_rect(area, DIALOG_WIDTH, DIALOG_HEIGHT)
}

fn load_area(content: Rect) -> Rect {
    Rect::new(
        content.x,
        content.y.saturating_add(LOAD_AREA_ROW),
        content.width,
        LOAD_AREA_HEIGHT.min(content.height.saturating_sub(LOAD_AREA_ROW)),
    )
}

fn save_area(content: Rect) -> Rect {
    Rect::new(
        content.x,
        content.y.saturating_add(SAVE_AREA_ROW),
        content.width,
        SAVE_AREA_HEIGHT.min(content.height.saturating_sub(SAVE_AREA_ROW)),
    )
}

fn startup_area(content: Rect) -> Rect {
    Rect::new(
        content.x,
        content.y.saturating_add(STARTUP_AREA_ROW),
        content.width,
        STARTUP_AREA_HEIGHT.min(content.height.saturating_sub(STARTUP_AREA_ROW)),
    )
}

fn save_name_row_areas(save_content: Rect) -> (Rect, Rect) {
    let label_width = (SAVE_NAME_LABEL.chars().count() as u16).min(save_content.width);
    let input_width = save_content.width.saturating_sub(label_width);
    let y = save_content.y.saturating_add(SAVE_INPUT_ROW);
    let label = Rect::new(save_content.x, y, label_width, 1);
    let input = Rect::new(
        save_content.x.saturating_add(label_width),
        y,
        input_width,
        1,
    );
    (label, input)
}

fn startup_option_areas(area: Rect) -> Vec<(TrackedListStartup, Rect)> {
    let mut x = area.x;
    TrackedListStartup::ALL
        .into_iter()
        .map(|startup| {
            let width = (4 + startup.label().chars().count()) as u16;
            let available = area.right().saturating_sub(x);
            let option = Rect::new(x, area.y, width.min(available), area.height.min(1));
            x = x.saturating_add(width).saturating_add(2);
            (startup, option)
        })
        .collect()
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

fn input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    let cursor = cursor.min(value.len());
    let cursor_char = value[..cursor].chars().count();
    let char_count = value.chars().count();
    let start_char = cursor_char.saturating_sub(width.saturating_sub(1));
    let end_char = start_char.saturating_add(width).min(char_count);
    let visible = value
        .chars()
        .skip(start_char)
        .take(end_char.saturating_sub(start_char))
        .collect::<String>();
    (visible, cursor_char.saturating_sub(start_char))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn process_name_summary_lists_process_names() {
        assert_eq!(
            process_name_summary(&names(&["chrome.exe", "node.exe"]), 40),
            "chrome.exe, node.exe"
        );
    }

    #[test]
    fn process_name_summary_shows_remaining_count_when_truncated() {
        assert_eq!(
            process_name_summary(&names(&["chrome.exe", "node.exe", "worker.exe"]), 20),
            "chrome.exe, ... (+2)"
        );
    }

    #[test]
    fn process_name_summary_marks_empty_lists() {
        assert_eq!(process_name_summary(&[], 20), "(empty)");
    }
}
