use ratatui::{layout::Rect, widgets::Clear};

use crate::app::{App, QuitConfirmSelection};
use crate::ui::{
    Theme,
    widgets::confirm_dialog::{
        button_line, centered_dialog_rect, warning_dialog, warning_message_dialog,
    },
};

const QUIT_CONFIRM_WIDTH: u16 = 38;
const QUIT_CONFIRM_HEIGHT: u16 = 4;
const QUIT_CONFIRM_RECORDING_HEIGHT: u16 = 5;
const BUTTON_ROW_FROM_CONTENT_TOP: u16 = 1;
const RECORDING_BUTTON_ROW_FROM_CONTENT_TOP: u16 = 2;

pub(crate) fn draw_quit_confirm(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
) {
    let recording = app.recording_session.is_some();
    let popup = if recording {
        quit_confirm_recording_rect(area)
    } else {
        quit_confirm_rect(area)
    };
    let message = if app.recording_session.is_some() {
        "Stop recording and quit?"
    } else {
        "Quit winproc-tui?"
    };
    let buttons = button_line(
        &[
            (
                " Quit ",
                app.quit_confirm_selection == QuitConfirmSelection::Quit,
            ),
            (
                " Cancel ",
                app.quit_confirm_selection == QuitConfirmSelection::Cancel,
            ),
        ],
        theme,
    );

    frame.render_widget(Clear, popup);
    let dialog = if recording {
        warning_dialog(
            "Confirm",
            message,
            "The log will be flushed before exit.",
            buttons,
            theme,
        )
    } else {
        warning_message_dialog("Confirm", message, buttons, theme)
    };
    frame.render_widget(dialog, popup);
}

fn quit_confirm_rect(area: Rect) -> Rect {
    centered_dialog_rect(area, QUIT_CONFIRM_WIDTH, QUIT_CONFIRM_HEIGHT)
}

fn quit_confirm_recording_rect(area: Rect) -> Rect {
    centered_dialog_rect(area, QUIT_CONFIRM_WIDTH, QUIT_CONFIRM_RECORDING_HEIGHT)
}

pub(crate) fn quit_confirm_button_at(
    area: Rect,
    x: u16,
    y: u16,
    recording: bool,
) -> Option<QuitConfirmSelection> {
    let popup = if recording {
        quit_confirm_recording_rect(area)
    } else {
        quit_confirm_rect(area)
    };
    let content = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let button_row = if recording {
        RECORDING_BUTTON_ROW_FROM_CONTENT_TOP
    } else {
        BUTTON_ROW_FROM_CONTENT_TOP
    };
    let buttons = crate::ui::widgets::confirm_dialog::button_areas(
        content,
        button_row,
        &[" Quit ", " Cancel "],
    );
    buttons
        .into_iter()
        .enumerate()
        .find(|(_, area)| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
        .map(|(index, _)| {
            if index == 0 {
                QuitConfirmSelection::Quit
            } else {
                QuitConfirmSelection::Cancel
            }
        })
}
