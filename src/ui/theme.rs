use ratatui::prelude::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) name: &'static str,
    pub(crate) background: Color,
    pub(crate) panel: Color,
    pub(crate) panel_alt: Color,
    pub(crate) border: Color,
    pub(crate) text: Color,
    pub(crate) muted: Color,
    pub(crate) accent: Color,
    pub(crate) focus_border: Color,
    pub(crate) focus_surface: Color,
    pub(crate) key_hint: Color,
    pub(crate) table_selection_surface: Color,
    pub(crate) table_multi_selection_surface: Color,
    pub(crate) table_column_surface: Color,
    pub(crate) table_intersection_surface: Color,
    pub(crate) graph_line: Color,
    pub(crate) active_series: Color,
    pub(crate) cursor_guide: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    pub(crate) tracked: Color,
    pub(crate) exited: Color,
    pub(crate) highlight: Color,
    pub(crate) selection: Color,
}

// Each theme varies this complete set of semantic colors while sharing neutral surfaces.
#[allow(clippy::too_many_arguments)]
const fn dark_theme(
    name: &'static str,
    accent: Color,
    focus_border: Color,
    key_hint: Color,
    table_selection_surface: Color,
    table_column_surface: Color,
    table_intersection_surface: Color,
    active_series: Color,
    warning: Color,
    tracked: Color,
) -> Theme {
    Theme {
        name,
        background: Color::Rgb(12, 13, 14),
        panel: Color::Rgb(17, 19, 21),
        panel_alt: Color::Rgb(26, 29, 32),
        border: Color::Rgb(53, 58, 64),
        text: Color::Rgb(230, 226, 218),
        muted: Color::Rgb(154, 152, 146),
        accent,
        focus_border,
        focus_surface: Color::Rgb(48, 52, 58),
        key_hint,
        table_selection_surface,
        table_multi_selection_surface: Color::Rgb(45, 48, 52),
        table_column_surface,
        table_intersection_surface,
        graph_line: Color::Rgb(139, 144, 150),
        active_series,
        cursor_guide: Color::Rgb(101, 106, 112),
        success: Color::Rgb(120, 194, 139),
        warning,
        danger: Color::Rgb(224, 108, 117),
        tracked,
        exited: Color::Rgb(109, 114, 122),
        highlight: Color::Rgb(34, 37, 41),
        selection: Color::Rgb(27, 30, 33),
    }
}

pub(crate) const THEMES: [Theme; 4] = [
    dark_theme(
        "Green",
        Color::Rgb(201, 206, 214),
        Color::Rgb(104, 196, 164),
        Color::Rgb(83, 151, 128),
        Color::Rgb(19, 51, 48),
        Color::Rgb(39, 49, 54),
        Color::Rgb(32, 79, 73),
        Color::Rgb(72, 190, 151),
        Color::Rgb(214, 170, 94),
        Color::Rgb(72, 190, 151),
    ),
    dark_theme(
        "Yellow",
        Color::Rgb(226, 200, 111),
        Color::Rgb(239, 209, 116),
        Color::Rgb(169, 144, 77),
        Color::Rgb(63, 54, 28),
        Color::Rgb(55, 51, 41),
        Color::Rgb(92, 77, 33),
        Color::Rgb(224, 196, 108),
        Color::Rgb(229, 139, 82),
        Color::Rgb(224, 196, 108),
    ),
    dark_theme(
        "Orange",
        Color::Rgb(238, 157, 99),
        Color::Rgb(242, 163, 111),
        Color::Rgb(181, 111, 67),
        Color::Rgb(67, 39, 25),
        Color::Rgb(56, 46, 40),
        Color::Rgb(97, 54, 31),
        Color::Rgb(229, 139, 82),
        Color::Rgb(214, 170, 94),
        Color::Rgb(229, 139, 82),
    ),
    dark_theme(
        "Cyan",
        Color::Rgb(121, 216, 232),
        Color::Rgb(116, 220, 235),
        Color::Rgb(84, 167, 181),
        Color::Rgb(23, 58, 64),
        Color::Rgb(41, 54, 58),
        Color::Rgb(32, 88, 97),
        Color::Rgb(69, 199, 214),
        Color::Rgb(214, 170, 94),
        Color::Rgb(69, 199, 214),
    ),
];

pub(crate) fn theme_index_by_name(name: &str) -> usize {
    if name.eq_ignore_ascii_case("Multi") {
        return 3;
    }
    THEMES
        .iter()
        .position(|theme| theme.name.eq_ignore_ascii_case(name))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_are_four_dark_color_schemes() {
        assert_eq!(THEMES.len(), 4);
        assert_eq!(
            THEMES.map(|theme| theme.name),
            ["Green", "Yellow", "Orange", "Cyan"]
        );
        for theme in THEMES {
            assert_eq!(theme.background, Color::Rgb(12, 13, 14));
            assert_eq!(theme.panel, Color::Rgb(17, 19, 21));
            assert_eq!(theme.text, Color::Rgb(230, 226, 218));
        }
    }

    #[test]
    fn built_in_themes_separate_focus_selection_and_semantic_status_colors() {
        for theme in THEMES {
            assert_ne!(theme.focus_border, theme.active_series);
            assert_ne!(theme.key_hint, theme.focus_border);
            assert_ne!(theme.table_multi_selection_surface, theme.panel);
            assert_ne!(
                theme.table_multi_selection_surface,
                theme.table_column_surface
            );
            assert_ne!(theme.table_selection_surface, theme.table_column_surface);
            assert_ne!(
                theme.table_selection_surface,
                theme.table_intersection_surface
            );
            assert_ne!(theme.table_column_surface, theme.table_intersection_surface);
            assert_ne!(theme.warning, theme.tracked);
        }

        for theme in THEMES {
            assert_eq!(theme.active_series, theme.tracked);
        }
    }

    #[test]
    fn theme_lookup_keeps_legacy_names_compatible() {
        assert_eq!(theme_index_by_name("Green"), 0);
        assert_eq!(theme_index_by_name("yellow"), 1);
        assert_eq!(theme_index_by_name("ORANGE"), 2);
        assert_eq!(theme_index_by_name("Cyan"), 3);
        assert_eq!(theme_index_by_name("Multi"), 3);
        assert_eq!(theme_index_by_name("Dark"), 0);
        assert_eq!(theme_index_by_name("Light"), 0);
        assert_eq!(theme_index_by_name("Neutral Light"), 0);
        assert_eq!(theme_index_by_name("Ocean Pop"), 0);
        assert_eq!(theme_index_by_name("unknown"), 0);
    }
}
