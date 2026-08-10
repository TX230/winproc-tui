use ratatui::{prelude::Style, text::Span};

use crate::{app::GraphSourceState, ui::Theme};

pub(crate) fn graph_slot_number_span(
    state: Option<GraphSourceState>,
    width: usize,
    theme: Theme,
) -> Span<'static> {
    let style = if state.is_some_and(|state| state.active) {
        Style::default()
            .fg(theme.accent)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    let number = state
        .map(|state| (state.ordinal + 1).to_string())
        .unwrap_or_default();
    Span::styled(format!("{number:<width$}"), style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn active_graph_number_uses_accent() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_number_span(
            Some(GraphSourceState {
                ordinal: 0,
                active: true,
            }),
            2,
            theme,
        );

        assert_eq!(marker.content, "1 ");
        assert_eq!(marker.style.fg, Some(theme.accent));
        assert_eq!(marker.style.bg, None);
        assert!(marker.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn two_digit_graph_number_uses_the_fixed_width() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_number_span(
            Some(GraphSourceState {
                ordinal: 15,
                active: false,
            }),
            2,
            theme,
        );

        assert_eq!(marker.content, "16");
        assert_eq!(marker.style.fg, Some(theme.muted));
    }

    #[test]
    fn empty_graph_number_reserves_width() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_number_span(None, 2, theme);

        assert_eq!(marker.content, "  ");
        assert_eq!(marker.style.fg, Some(theme.muted));
    }
}
