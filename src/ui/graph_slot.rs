use ratatui::{prelude::Style, text::Span};

use crate::{app::GraphSourceState, ui::Theme};

pub(crate) const GRAPH_SLOT_NUMBER_WIDTH: usize = 2;
pub(crate) const GRAPH_SLOT_NUMBER_GAP: usize = 1;

pub(crate) fn graph_slot_number_span(
    state: Option<GraphSourceState>,
    width: usize,
    theme: Theme,
) -> Span<'static> {
    let (number, style) = match state {
        Some(state) => (
            (state.ordinal + 1).to_string(),
            Style::default()
                .fg(if state.active {
                    theme.active_series
                } else {
                    theme.graph_line
                })
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        None => (String::new(), Style::default().fg(theme.muted)),
    };
    Span::styled(format!("{number:<width$}"), style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn active_graph_number_uses_active_series_color() {
        for theme in crate::ui::THEMES {
            let marker = graph_slot_number_span(
                Some(GraphSourceState {
                    ordinal: 0,
                    active: true,
                }),
                2,
                theme,
            );

            assert_eq!(marker.content, "1 ");
            assert_eq!(marker.style.fg, Some(theme.active_series));
            assert_eq!(marker.style.bg, None);
            assert!(marker.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn inactive_graph_number_uses_bold_high_contrast_color_and_fixed_width() {
        for theme in crate::ui::THEMES {
            let marker = graph_slot_number_span(
                Some(GraphSourceState {
                    ordinal: 15,
                    active: false,
                }),
                2,
                theme,
            );

            assert_eq!(marker.content, "16");
            assert_eq!(marker.style.fg, Some(theme.graph_line));
            assert!(marker.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn two_digit_graph_number_keeps_a_gap_before_the_label() {
        for theme in crate::ui::THEMES {
            let marker = graph_slot_number_span(
                Some(GraphSourceState {
                    ordinal: 9,
                    active: false,
                }),
                GRAPH_SLOT_NUMBER_WIDTH + GRAPH_SLOT_NUMBER_GAP,
                theme,
            );

            assert_eq!(marker.content, "10 ");
        }
    }

    #[test]
    fn empty_graph_number_reserves_width() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_number_span(None, 2, theme);

        assert_eq!(marker.content, "  ");
        assert_eq!(marker.style.fg, Some(theme.muted));
        assert!(!marker.style.add_modifier.contains(Modifier::BOLD));
    }
}
