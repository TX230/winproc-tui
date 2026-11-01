use crate::{
    app::App,
    ui::{
        Theme,
        footer::warning_shortcut_spans,
        widgets::{confirm_dialog, scrollable_modal::ScrollableModal},
    },
};
use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Text},
    widgets::Paragraph,
};
pub(crate) fn content(area: Rect, app: &App) -> (ScrollableModal, Text<'static>) {
    let width = area.width.min(90).saturating_sub(4).max(1) as usize;
    let lines: Vec<_> = app
        .mcp_state
        .confirmation
        .as_ref()
        .map(|(_, s)| s.as_str())
        .unwrap_or("")
        .split('\n')
        .flat_map(|s| super::process_info_dialog::wrap_display_width(s, width))
        .map(Line::from)
        .collect();
    (
        ScrollableModal::new(
            "MCP CONFIRMATION",
            86,
            lines.len().min(u16::MAX as usize) as u16,
            2,
        ),
        Text::from(lines),
    )
}
pub(super) fn draw(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some((action, _)) = &app.mcp_state.confirmation else {
        return;
    };
    let (modal, text) = content(area, app);
    let layout = modal.render(
        frame,
        area,
        text,
        app.mcp_state.confirmation_scroll,
        false,
        theme,
    );
    frame.render_widget(
        confirm_dialog::warning_block("MCP CONFIRMATION", theme),
        layout.area,
    );
    let keys = if action.method == "winproc_stop_recording" {
        vec![("y", "Stop"), ("Enter/Esc/n", "Continue")]
    } else {
        vec![("Enter", "Confirm"), ("Esc", "Cancel")]
    };
    let footer = Text::from(vec![
        Line::from(warning_shortcut_spans(
            &[
                ("↑/↓", "Scroll"),
                ("PgUp/PgDn", "Page"),
                ("Home/End", "First/Last"),
            ],
            theme,
        )),
        Line::from(warning_shortcut_spans(&keys, theme)),
    ]);
    crate::ui::footer::register_shortcut_text(app, layout.footer, &footer, Alignment::Left);
    frame.render_widget(Paragraph::new(footer), layout.footer);
}
