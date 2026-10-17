use ratatui::{
    prelude::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

use crate::ui::{Theme, theme::contrasting_foreground};

pub(crate) const PANEL_NAME_PADDING_WIDTH: usize = 2;

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
    let block = panel_block(styled_panel_name(title.into(), theme, focused), theme);
    if focused {
        block
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(theme.focus_border))
    } else {
        block
    }
}

fn styled_panel_name<'a>(mut title: Line<'a>, theme: Theme, focused: bool) -> Line<'a> {
    if let Some(first) = title.spans.first_mut() {
        // Only the leading panel name gets focus emphasis; counts and controls retain their styles.
        let name_len = first.content.find(' ').unwrap_or(first.content.len());
        if name_len == 0 {
            return title;
        }
        let suffix = Span::styled(first.content[name_len..].to_string(), first.style);
        first.content = format!(" {} ", &first.content[..name_len]).into();
        first.style = Style::default()
            .fg(if focused {
                theme.focus_border
            } else {
                theme.muted
            })
            .bg(theme.panel)
            .add_modifier(Modifier::BOLD);
        if !suffix.content.is_empty() {
            title.spans.insert(1, suffix);
        }
    }
    title
}

pub(crate) fn modal_title(title: impl Into<String>, theme: Theme) -> Line<'static> {
    semantic_modal_title(title, theme.focus_border, theme)
}

pub(crate) fn semantic_modal_title(
    title: impl Into<String>,
    background: Color,
    theme: Theme,
) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {} ", title.into().trim()),
        Style::default()
            .fg(contrasting_foreground(background, theme))
            .bg(background)
            .add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn modal_block_focused<'a>(title: impl Into<Line<'a>>, theme: Theme) -> Block<'a> {
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.focus_border))
        .style(Style::default().bg(theme.panel_alt))
}

pub(crate) fn graph_workspace_block<'a>(
    title: impl Into<Line<'a>>,
    theme: Theme,
    focused: bool,
) -> Block<'a> {
    panel_block_focused(title, theme, focused)
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
    fn focused_panel_uses_thick_pale_green_border() {
        let theme = crate::ui::THEMES[0];
        let (symbol, style) = rendered_corner(panel_block_focused("Panel", theme, true));

        assert_eq!(symbol, "┏");
        assert_eq!(style.fg, Some(theme.focus_border));
        assert_ne!(style.fg, Some(theme.accent));
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
    fn graph_workspace_uses_a_complete_thick_focus_border() {
        let theme = crate::ui::THEMES[0];
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        graph_workspace_block("", theme, true).render(area, &mut buffer);

        assert_eq!(buffer[(0, 0)].symbol(), "┏");
        assert_eq!(buffer[(0, 0)].fg, theme.focus_border);
        assert_eq!(buffer[(0, 1)].symbol(), "┃");
        assert_eq!(buffer[(0, 2)].symbol(), "┗");
    }

    #[test]
    fn inactive_graph_workspace_uses_a_complete_muted_border() {
        let theme = crate::ui::THEMES[0];
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        graph_workspace_block("", theme, false).render(area, &mut buffer);

        assert_eq!(buffer[(0, 0)].symbol(), "╭");
        assert_eq!(buffer[(0, 0)].fg, theme.border);
        assert_eq!(buffer[(0, 1)].symbol(), "│");
        assert_eq!(buffer[(0, 2)].symbol(), "╰");
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

    #[test]
    fn panel_names_use_panel_background_without_recoloring_metadata_or_content() {
        for theme in crate::ui::THEMES {
            for focused in [false, true] {
                let area = Rect::new(0, 0, 32, 4);
                let mut buffer = Buffer::empty(area);
                panel_block_focused("GPU 1/2", theme, focused).render(area, &mut buffer);
                let name_fg = if focused {
                    theme.focus_border
                } else {
                    theme.muted
                };
                for x in 1..=5 {
                    assert_eq!(buffer[(x, 0)].fg, name_fg);
                    assert_eq!(buffer[(x, 0)].bg, theme.panel);
                    assert!(buffer[(x, 0)].modifier.contains(Modifier::BOLD));
                }
                assert_eq!(buffer[(1, 0)].symbol(), " ");
                assert_eq!(buffer[(5, 0)].symbol(), " ");
                assert_eq!(buffer[(7, 0)].symbol(), "1");
                assert_eq!(buffer[(7, 0)].bg, theme.panel);
                assert_eq!(buffer[(1, 1)].bg, theme.panel);

                let title = Line::from(vec![
                    Span::raw("GRAPHS · 2 Slots"),
                    Span::styled(" [-]", Style::default().fg(theme.key_hint)),
                ]);
                graph_workspace_block(title, theme, focused).render(area, &mut buffer);
                assert_eq!(buffer[(1, 0)].fg, name_fg);
                assert_eq!(buffer[(1, 0)].bg, theme.panel);
                assert_eq!(buffer[(1, 0)].symbol(), " ");
                assert_eq!(buffer[(8, 0)].symbol(), " ");
                assert_eq!(buffer[(8, 0)].bg, theme.panel);
                assert_eq!(buffer[(10, 0)].bg, theme.panel);
                assert_eq!(buffer[(20, 0)].fg, theme.key_hint);
            }
        }
    }
}
