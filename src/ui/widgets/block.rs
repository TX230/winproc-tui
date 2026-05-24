use ratatui::{
    prelude::Style,
    text::Line,
    widgets::{Block, BorderType, Borders},
};

use crate::ui::Theme;

pub(crate) fn panel_block<'a>(title: impl Into<Line<'a>>, theme: Theme) -> Block<'a> {
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.panel))
}

pub(crate) fn panel_block_focused<'a>(
    title: impl Into<Line<'a>>,
    theme: Theme,
    focused: bool,
) -> Block<'a> {
    let block = panel_block(title, theme);
    if focused {
        block.border_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(ratatui::prelude::Modifier::BOLD),
        )
    } else {
        block
    }
}
