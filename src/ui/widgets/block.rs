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

pub(crate) fn graph_workspace_block<'a>(
    title: impl Into<Line<'a>>,
    theme: Theme,
    focused: bool,
) -> Block<'a> {
    Block::default()
        .title(title.into())
        .borders(Borders::TOP)
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(Style::default().fg(if focused {
            theme.focus_border
        } else {
            theme.border
        }))
        .style(Style::default().bg(theme.panel))
}

pub(crate) fn graph_card_block<'a>(
    title: impl Into<Line<'a>>,
    theme: Theme,
    active: bool,
) -> Block<'a> {
    let block = panel_block(title, theme);
    if active {
        block.border_type(BorderType::Rounded).border_style(
            Style::default()
                .fg(theme.active_series)
                .add_modifier(Modifier::BOLD),
        )
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
    fn focused_panel_uses_thick_high_contrast_neutral_border() {
        let theme = crate::ui::THEMES[0];
        let (symbol, style) = rendered_corner(panel_block_focused("Panel", theme, true));

        assert_eq!(symbol, "┏");
        assert_eq!(style.fg, Some(theme.focus_border));
        assert_eq!(style.fg, Some(theme.accent));
    }

    #[test]
    fn active_graph_card_uses_single_green_border() {
        for theme in crate::ui::THEMES {
            let (symbol, style) = rendered_corner(graph_card_block("Slot#1", theme, true));

            assert_eq!(symbol, "╭");
            assert_eq!(style.fg, Some(theme.active_series));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn graph_workspace_uses_only_a_thick_focused_top_rule() {
        let theme = crate::ui::THEMES[0];
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        graph_workspace_block("", theme, true).render(area, &mut buffer);

        assert_eq!(buffer[(0, 0)].symbol(), "━");
        assert_eq!(buffer[(0, 0)].fg, theme.focus_border);
        assert_eq!(buffer[(0, 1)].symbol(), " ");
        assert_eq!(buffer[(0, 2)].symbol(), " ");
    }

    #[test]
    fn inactive_graph_workspace_uses_a_thin_muted_top_rule() {
        let theme = crate::ui::THEMES[0];
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        graph_workspace_block("", theme, false).render(area, &mut buffer);

        assert_eq!(buffer[(0, 0)].symbol(), "─");
        assert_eq!(buffer[(0, 0)].fg, theme.border);
        assert_eq!(buffer[(0, 1)].symbol(), " ");
        assert_eq!(buffer[(0, 2)].symbol(), " ");
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
