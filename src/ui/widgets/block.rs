use ratatui::{
    prelude::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

use crate::ui::Theme;

pub(crate) fn panel_title(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::default().add_modifier(Modifier::BOLD),
    ))
}

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
        block
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(theme.focus_border))
    } else {
        block
    }
}

pub(crate) fn graph_card_block<'a>(
    title: impl Into<Line<'a>>,
    theme: Theme,
    active: bool,
) -> Block<'a> {
    let block = panel_block(title, theme);
    if active {
        block
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(theme.accent))
    } else {
        block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    fn rendered_corner(block: Block<'_>) -> (String, Style) {
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        block.render(area, &mut buffer);
        let cell = &buffer[(0, 0)];
        (cell.symbol().to_string(), cell.style())
    }

    #[test]
    fn focused_panel_uses_thick_green_border() {
        let theme = crate::ui::THEMES[0];
        let (symbol, style) = rendered_corner(panel_block_focused("Panel", theme, true));

        assert_eq!(symbol, "┏");
        assert_eq!(style.fg, Some(theme.focus_border));
    }

    #[test]
    fn active_graph_card_uses_double_neutral_border() {
        let theme = crate::ui::THEMES[0];
        let (symbol, style) = rendered_corner(graph_card_block("Slot#1", theme, true));

        assert_eq!(symbol, "╔");
        assert_eq!(style.fg, Some(theme.accent));
        assert_ne!(style.fg, Some(theme.focus_border));
    }

    #[test]
    fn inactive_blocks_keep_rounded_border() {
        let theme = crate::ui::THEMES[0];
        assert_eq!(
            rendered_corner(panel_block_focused("Panel", theme, false)).0,
            "╭"
        );
        assert_eq!(
            rendered_corner(graph_card_block("Slot#1", theme, false)).0,
            "╭"
        );
    }
}
