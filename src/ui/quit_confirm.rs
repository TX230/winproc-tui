use ratatui::{layout::Rect, text::Line, widgets::Clear};

use crate::app::App;
use crate::ui::{
    Theme,
    footer::warning_shortcut_spans,
    widgets::confirm_dialog::{centered_dialog_rect, warning_dialog, warning_message_dialog},
};

const QUIT_CONFIRM_WIDTH: u16 = 38;
const QUIT_CONFIRM_HEIGHT: u16 = 5;
const QUIT_CONFIRM_RECORDING_HEIGHT: u16 = 6;

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
    let shortcuts = Line::from(warning_shortcut_spans(
        &[("Enter/q", "Quit"), ("Esc", "Cancel")],
        theme,
    ));

    frame.render_widget(Clear, popup);
    let dialog = if recording {
        warning_dialog(
            "CONFIRM",
            message,
            "The log will be flushed before exit.",
            shortcuts,
            theme,
        )
    } else {
        warning_message_dialog("CONFIRM", message, shortcuts, theme)
    };
    frame.render_widget(dialog, popup);
}

fn quit_confirm_rect(area: Rect) -> Rect {
    centered_dialog_rect(area, QUIT_CONFIRM_WIDTH, QUIT_CONFIRM_HEIGHT)
}

fn quit_confirm_recording_rect(area: Rect) -> Rect {
    centered_dialog_rect(area, QUIT_CONFIRM_WIDTH, QUIT_CONFIRM_RECORDING_HEIGHT)
}
