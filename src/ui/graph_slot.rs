use ratatui::{prelude::Style, text::Span};

use crate::ui::Theme;

pub(crate) fn graph_slot_marker_span(numbers: &str, width: usize, theme: Theme) -> Span<'static> {
    let style = if numbers.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.warning)
    };
    Span::styled(format!("{numbers:<width$}"), style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn graph_slot_marker_uses_foreground_only() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_marker_span("13", 2, theme);

        assert_eq!(marker.content, "13");
        assert_eq!(marker.style.fg, Some(theme.warning));
        assert_eq!(marker.style.bg, None);
        assert!(!marker.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn empty_graph_slot_marker_reserves_width() {
        let theme = crate::ui::THEMES[0];
        let marker = graph_slot_marker_span("", 2, theme);

        assert_eq!(marker.content, "  ");
        assert_eq!(marker.style.fg, Some(theme.muted));
    }
}
