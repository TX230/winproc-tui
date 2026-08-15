use ratatui::prelude::{Modifier, Style};

use crate::{app::GraphSourceState, ui::Theme};

pub(crate) fn graph_value_style(
    base: Style,
    state: Option<GraphSourceState>,
    theme: Theme,
) -> Style {
    let Some(state) = state else {
        return base;
    };
    let style = base.fg(theme.active_series).remove_modifier(Modifier::BOLD);
    if state.active {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn registered_values_use_green_with_bold_reserved_for_the_active_graph() {
        for theme in crate::ui::THEMES {
            let inactive = graph_value_style(
                Style::default().add_modifier(Modifier::BOLD),
                Some(GraphSourceState {
                    ordinal: 0,
                    active: false,
                }),
                theme,
            );
            let active = graph_value_style(
                Style::default(),
                Some(GraphSourceState {
                    ordinal: 1,
                    active: true,
                }),
                theme,
            );

            assert_eq!(inactive.fg, Some(theme.active_series));
            assert!(!inactive.add_modifier.contains(Modifier::BOLD));
            assert_eq!(active.fg, Some(theme.active_series));
            assert!(active.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn unregistered_value_keeps_its_base_style() {
        for theme in crate::ui::THEMES {
            let base = Style::default()
                .fg(theme.text)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::UNDERLINED);
            let style = graph_value_style(base, None, theme);

            assert_eq!(style, base);
        }
    }
}
