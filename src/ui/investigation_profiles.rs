use ratatui::{
    layout::{Alignment, Position, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    App,
    app::{AppActivity, InvestigationProfilesView, ProfileNameInputPurpose},
    config::InvestigationStartup,
    ui::{
        Theme,
        footer::{shortcut_spans, warning_shortcut_spans},
        widgets::{
            block::{modal_block_focused, modal_title},
            confirm_dialog::{self, centered_dialog_rect},
            modal_scrim::{ModalScrim, ModalScrimStrength},
        },
    },
};

const DIALOG_WIDTH: u16 = 88;
const INTRO_ROW: u16 = 0;
const LIST_LABEL_ROW: u16 = 2;
const LIST_ROW: u16 = 3;
const MAX_LIST_HEIGHT: u16 = 6;
const NAME_DIALOG_WIDTH: u16 = 60;
const NAME_DIALOG_HEIGHT: u16 = 8;
const CONFIRM_DIALOG_WIDTH: u16 = 68;
const STARTUP_DIALOG_WIDTH: u16 = 72;
const STARTUP_DIALOG_HEIGHT: u16 = 12;
const STARTUP_OPTION_ROW: u16 = 3;
const STARTUP_SHORTCUT_ROW: u16 = 9;

pub(crate) fn draw_investigation_profiles(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let Some(view) = app.investigation_profiles_view() else {
        return;
    };
    match view {
        InvestigationProfilesView::Browse => draw_browse(frame, area, app, theme),
        InvestigationProfilesView::Startup { .. } => draw_startup(frame, area, app, theme),
        InvestigationProfilesView::NameInput { .. } => draw_name_input(frame, area, app, theme),
        InvestigationProfilesView::ConfirmDelete { .. }
        | InvestigationProfilesView::ConfirmLoad { .. } => {
            draw_browse(frame, area, app, theme);
            frame.render_widget(ModalScrim::new(theme, ModalScrimStrength::Dialog), area);
            match view {
                InvestigationProfilesView::NameInput { .. } => {
                    draw_name_input(frame, area, app, theme)
                }
                InvestigationProfilesView::ConfirmDelete { .. } => {
                    draw_delete_confirm(frame, area, app, theme)
                }
                InvestigationProfilesView::ConfirmLoad { .. } => {
                    draw_load_confirm(frame, area, app, theme)
                }
                InvestigationProfilesView::Browse | InvestigationProfilesView::Startup { .. } => {}
            }
        }
    }
}

fn draw_browse(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let count = app.investigation_profiles_entry_count();
    let selected_tracked_count = app
        .selected_investigation_profile()
        .map(|profile| profile.tracked_names.len())
        .unwrap_or(0);
    let layout = browse_layout(area, count, selected_tracked_count);
    let popup = layout.popup;
    let block = modal_block_focused(modal_title("OPEN INVESTIGATION PROFILE", theme), theme);
    let content = layout.content;
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(match app.activity() {
            AppActivity::Live if count == 0 => {
                "Save the current tracking list as your first profile."
            }
            AppActivity::Live => {
                "Profiles save tracking lists only; current Graphs stay unchanged."
            }
            AppActivity::Recording => "Stop Recording to open a profile. Delete remains available.",
            AppActivity::LogView => "Return to Live to open a profile. Delete remains available.",
        })
        .style(Style::default().fg(theme.text)),
        row(content, INTRO_ROW),
    );

    draw_section_label(frame, content, LIST_LABEL_ROW, "SAVED PROFILES", theme);

    let list_area = layout.list;
    if count == 0 {
        frame.render_widget(
            Paragraph::new("(No saved profiles)").style(Style::default().fg(theme.muted)),
            list_area,
        );
    } else {
        let selected = app.investigation_profiles_index();
        let lines = (app.investigation_profiles_scroll_offset()..count)
            .take(list_area.height as usize)
            .map(|index| {
                let profile = &app.runtime.saved_investigation_profiles[index];
                let is_selected = index == selected;
                let active = app
                    .active_investigation_profile
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&profile.name));
                let style = if is_selected {
                    Style::default()
                        .fg(theme.text)
                        .bg(theme.highlight)
                        .add_modifier(Modifier::BOLD)
                } else if active {
                    Style::default()
                        .fg(theme.text)
                        .bg(theme.selection)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                Line::from(Span::styled(
                    profile_row_text(
                        if is_selected { ">" } else { " " },
                        &profile.name,
                        profile.tracked_names.len(),
                        active,
                        list_area.width as usize,
                    ),
                    style,
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), list_area);
    }

    if let Some(profile) = app.selected_investigation_profile() {
        draw_section_label(
            frame,
            content,
            layout.summary_label_row,
            &format!("SELECTED PROFILE · {}", profile.name),
            theme,
        );
        if profile.tracked_names.is_empty() {
            frame.render_widget(
                Paragraph::new("(Empty tracking list)").style(Style::default().fg(theme.muted)),
                row(content, layout.summary_row),
            );
        } else {
            for (offset, name) in profile
                .tracked_names
                .iter()
                .take(layout.summary_height as usize)
                .enumerate()
            {
                frame.render_widget(
                    Paragraph::new(name.as_str()).style(Style::default().fg(theme.text)),
                    row(content, layout.summary_row + offset as u16),
                );
            }
        }
    } else {
        draw_section_label(
            frame,
            content,
            layout.summary_label_row,
            "SELECTED PROFILE",
            theme,
        );
        frame.render_widget(
            Paragraph::new("Profiles do not include app settings or Graphs.")
                .style(Style::default().fg(theme.text)),
            row(content, layout.summary_row),
        );
    }

    let mut shortcuts = Vec::new();
    if count > 0 {
        shortcuts.push(("↑/↓", "Select"));
        if app.activity() == AppActivity::Live {
            shortcuts.push(("Enter", "Open"));
        }
        shortcuts.push(("Delete", "Delete"));
    }
    if app.activity() == AppActivity::Live {
        shortcuts.push(("Ctrl+S", "Save As"));
    }
    shortcuts.push(("Esc", "Close"));
    crate::ui::footer::register_shortcut_text(
        app,
        row(content, layout.shortcut_row),
        &ratatui::text::Text::from(Line::from(shortcut_spans(&shortcuts, theme))),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(&shortcuts, theme))),
        row(content, layout.shortcut_row),
    );
}

fn draw_startup(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(InvestigationProfilesView::Startup { selected }) = app.investigation_profiles_view()
    else {
        return;
    };
    let popup = startup_dialog_area(area);
    let block = modal_block_focused(modal_title("STARTUP BEHAVIOR", theme), theme);
    let content = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Choose how to start. Graphs start empty.")
            .style(Style::default().fg(theme.muted)),
        row(content, 0),
    );
    for (index, startup) in InvestigationStartup::ALL.into_iter().enumerate() {
        let is_selected = startup == *selected;
        let style = if is_selected {
            Style::default()
                .fg(theme.text)
                .bg(theme.highlight)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let text = format!(
            "{} {:<16} {}",
            if is_selected { ">" } else { " " },
            startup.label(),
            startup_description(startup)
        );
        frame.render_widget(
            Paragraph::new(text).style(style),
            row(content, STARTUP_OPTION_ROW + index as u16),
        );
    }
    crate::ui::footer::register_shortcut_text(
        app,
        row(content, STARTUP_SHORTCUT_ROW),
        &ratatui::text::Text::from(Line::from(shortcut_spans(
            &[("↑/↓", "Select"), ("Enter", "Apply"), ("Esc", "Close")],
            theme,
        ))),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(
            &[("↑/↓", "Select"), ("Enter", "Apply"), ("Esc", "Close")],
            theme,
        ))),
        row(content, STARTUP_SHORTCUT_ROW),
    );
}

fn draw_name_input(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(InvestigationProfilesView::NameInput {
        purpose,
        draft,
        cursor,
        error,
    }) = app.investigation_profiles_view()
    else {
        return;
    };
    let popup = centered_dialog_rect(area, NAME_DIALOG_WIDTH, NAME_DIALOG_HEIGHT);
    let title = match purpose {
        ProfileNameInputPurpose::SaveAs => "SAVE INVESTIGATION PROFILE AS",
    };
    let block = modal_block_focused(modal_title(title, theme), theme);
    let content = block.inner(popup);
    let input_area = row(content, 2);
    let (input, cursor_x) = input_view(draft, *cursor, input_area.width as usize);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new("Save the current tracking list as a profile.")
            .style(Style::default().fg(theme.text)),
        row(content, 0),
    );
    frame.render_widget(
        Paragraph::new("Profiles do not include app settings or Graphs.")
            .style(Style::default().fg(theme.muted)),
        row(content, 1),
    );
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(theme.text).bg(theme.panel)),
        input_area,
    );
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(theme.danger)),
            row(content, 3),
        );
    }
    crate::ui::footer::register_shortcut_text(
        app,
        row(content, 5),
        &ratatui::text::Text::from(Line::from(shortcut_spans(
            &[("Enter", "Save"), ("Esc", "Cancel")],
            theme,
        ))),
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(
        Paragraph::new(Line::from(shortcut_spans(
            &[("Enter", "Save"), ("Esc", "Cancel")],
            theme,
        ))),
        row(content, 5),
    );
    frame.set_cursor_position(Position::new(
        input_area.x.saturating_add(cursor_x as u16),
        input_area.y,
    ));
}

fn draw_delete_confirm(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, _theme: Theme) {
    let Some(InvestigationProfilesView::ConfirmDelete { name }) = app.investigation_profiles_view()
    else {
        return;
    };
    draw_confirm(
        frame,
        area,
        "DELETE INVESTIGATION PROFILE?",
        &format!("Delete \"{name}\"? The current setup is kept."),
        "This cannot be undone.",
        &[("Enter", "Delete"), ("Esc", "Cancel")],
        app,
    );
}

fn draw_load_confirm(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, _theme: Theme) {
    let Some(InvestigationProfilesView::ConfirmLoad { pending }) =
        app.investigation_profiles_view()
    else {
        return;
    };
    draw_confirm(
        frame,
        area,
        "OPEN INVESTIGATION PROFILE?",
        &format!(
            "{}\nRemoves {} {} from the tracking list.",
            if pending.unsaved_changes {
                "Discard unsaved tracking changes and open this profile?"
            } else {
                "Open this profile?"
            },
            pending.tracking_switch.removed_name_count,
            if pending.tracking_switch.removed_name_count == 1 {
                "entry"
            } else {
                "entries"
            }
        ),
        &format!(
            "{} older sample{} for {} process name{} will be discarded.\nRemoved: {}",
            pending.tracking_switch.discarded_sample_count,
            if pending.tracking_switch.discarded_sample_count == 1 {
                ""
            } else {
                "s"
            },
            pending.tracking_switch.affected_name_count,
            if pending.tracking_switch.affected_name_count == 1 {
                ""
            } else {
                "s"
            },
            truncate(
                &app.watch_list
                    .iter()
                    .filter(|name| !pending
                        .profile
                        .tracked_names
                        .iter()
                        .any(|target| name.eq_ignore_ascii_case(target)))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                52
            )
        ),
        &[("Enter", "Open"), ("Esc", "Cancel")],
        app,
    );
}

fn draw_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &'static str,
    message: &str,
    detail: &str,
    shortcuts: &[(&'static str, &'static str)],
    app: &App,
) {
    let theme = app.theme();
    let width = CONFIRM_DIALOG_WIDTH.min(area.width);
    let wrap = |value: &str| {
        value
            .lines()
            .flat_map(|line| {
                super::process_info_dialog::wrap_display_width(
                    line,
                    width.saturating_sub(2) as usize,
                )
            })
            .map(Line::raw)
            .collect::<Vec<_>>()
    };
    let message = wrap(message);
    let detail = wrap(detail);
    let message_height = message.len() as u16;
    let detail_height = detail.len() as u16;
    let popup = centered_dialog_rect(area, width, message_height + detail_height + 5);
    let block = confirm_dialog::warning_block(title, theme);
    let content = block.inner(popup);
    let shortcuts_area = row(content, content.height.saturating_sub(1));
    let body = Rect {
        height: content.height.saturating_sub(2),
        ..content
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(message).alignment(Alignment::Center),
        Rect {
            height: message_height,
            ..row(content, 1)
        }
        .intersection(body),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.warning)),
        Rect {
            height: detail_height,
            ..row(content, 1 + message_height)
        }
        .intersection(body),
    );
    crate::ui::footer::register_shortcut_text(
        app,
        shortcuts_area,
        &ratatui::text::Text::from(Line::from(warning_shortcut_spans(shortcuts, theme))),
        ratatui::layout::Alignment::Center,
    );
    frame.render_widget(
        Paragraph::new(Line::from(warning_shortcut_spans(shortcuts, theme)))
            .alignment(Alignment::Center),
        shortcuts_area,
    );
}

pub(crate) fn investigation_profiles_page_size_for_screen(area: Rect, app: &App) -> usize {
    browse_layout(
        area,
        app.investigation_profiles_entry_count(),
        app.selected_investigation_profile()
            .map(|profile| profile.tracked_names.len())
            .unwrap_or(0),
    )
    .list
    .height
    .max(1) as usize
}

pub(crate) fn investigation_profile_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll_offset: usize,
    profile_count: usize,
    selected_tracked_count: usize,
) -> Option<usize> {
    let list = browse_layout(area, profile_count, selected_tracked_count).list;
    if !contains(list, x, y) {
        return None;
    }
    let index = scroll_offset.saturating_add(y.saturating_sub(list.y) as usize);
    (index < profile_count).then_some(index)
}

pub(crate) fn investigation_profile_startup_at_for_screen(
    area: Rect,
    x: u16,
    y: u16,
) -> Option<InvestigationStartup> {
    let popup = startup_dialog_area(area);
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    InvestigationStartup::ALL
        .into_iter()
        .enumerate()
        .find_map(|(index, startup)| {
            contains(row(content, STARTUP_OPTION_ROW + index as u16), x, y).then_some(startup)
        })
}

fn profile_row_text(
    cursor: &str,
    name: &str,
    tracked_count: usize,
    active: bool,
    width: usize,
) -> String {
    let suffix = format!(
        "  {:>2} process name{}{}",
        tracked_count,
        if tracked_count == 1 { "" } else { "s" },
        if active { "  [current]" } else { "" }
    );
    let name_width = width.saturating_sub(cursor.chars().count() + 1 + suffix.chars().count());
    let name = truncate(name, name_width);
    let padding = name_width.saturating_sub(Span::raw(&name).width());
    format!("{cursor} {name}{}{suffix}", " ".repeat(padding))
}

fn startup_description(startup: InvestigationStartup) -> &'static str {
    match startup {
        InvestigationStartup::ResumeLast => "Restore the previous tracking list",
        InvestigationStartup::ChooseProfile => "Choose a tracking list at startup.",
        InvestigationStartup::StartEmpty => "Start with an empty tracking list",
    }
}

fn draw_section_label(
    frame: &mut ratatui::Frame<'_>,
    content: Rect,
    offset: u16,
    label: &str,
    theme: Theme,
) {
    frame.render_widget(
        Paragraph::new(label.to_string()).style(
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ),
        row(content, offset),
    );
}

pub(crate) fn truncate(value: &str, width: usize) -> String {
    let len = Span::raw(value).width();
    if len <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let cells = Span::raw(ch.to_string()).width();
        if used + cells > width - 3 {
            break;
        }
        result.push(ch);
        used += cells;
    }
    result.push_str("...");
    result
}

#[derive(Clone, Copy)]
struct BrowseLayout {
    popup: Rect,
    content: Rect,
    list: Rect,
    summary_label_row: u16,
    summary_row: u16,
    summary_height: u16,
    shortcut_row: u16,
}

fn browse_layout(area: Rect, profile_count: usize, selected_tracked_count: usize) -> BrowseLayout {
    let list_height = (profile_count as u16).clamp(1, MAX_LIST_HEIGHT);
    let summary_label_row = LIST_ROW.saturating_add(list_height).saturating_add(1);
    let summary_row = summary_label_row.saturating_add(1);
    let max_summary_height = area
        .height
        .saturating_sub(summary_row.saturating_add(4))
        .max(1);
    let summary_height = (selected_tracked_count as u16)
        .max(1)
        .min(max_summary_height);
    let shortcut_row = summary_row.saturating_add(summary_height).saturating_add(1);
    let popup = centered_dialog_rect(area, DIALOG_WIDTH, shortcut_row.saturating_add(3));
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let list = Rect::new(
        content.x,
        content.y.saturating_add(LIST_ROW),
        content.width,
        list_height.min(content.height.saturating_sub(LIST_ROW)),
    );
    BrowseLayout {
        popup,
        content,
        list,
        summary_label_row,
        summary_row,
        summary_height,
        shortcut_row,
    }
}

fn startup_dialog_area(area: Rect) -> Rect {
    centered_dialog_rect(area, STARTUP_DIALOG_WIDTH, STARTUP_DIALOG_HEIGHT)
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
