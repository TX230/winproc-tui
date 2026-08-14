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
    pub(crate) table_column_surface: Color,
    pub(crate) table_intersection_surface: Color,
    pub(crate) graph_line: Color,
    pub(crate) active_series: Color,
    pub(crate) cursor_guide: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    #[allow(dead_code)]
    pub(crate) danger: Color,
    pub(crate) tracked: Color,
    pub(crate) exited: Color,
    pub(crate) highlight: Color,
    pub(crate) selection: Color,
}

pub(crate) const THEMES: [Theme; 2] = [
    Theme {
        name: "Dark",
        background: Color::Rgb(12, 13, 14),
        panel: Color::Rgb(17, 19, 21),
        panel_alt: Color::Rgb(26, 29, 32),
        border: Color::Rgb(53, 58, 64),
        text: Color::Rgb(230, 226, 218),
        muted: Color::Rgb(154, 152, 146),
        accent: Color::Rgb(201, 206, 214),
        focus_border: Color::Rgb(104, 196, 164),
        focus_surface: Color::Rgb(48, 52, 58),
        key_hint: Color::Rgb(83, 151, 128),
        table_selection_surface: Color::Rgb(19, 51, 48),
        table_column_surface: Color::Rgb(29, 38, 42),
        table_intersection_surface: Color::Rgb(32, 79, 73),
        graph_line: Color::Rgb(139, 144, 150),
        active_series: Color::Rgb(72, 190, 151),
        cursor_guide: Color::Rgb(101, 106, 112),
        success: Color::Rgb(120, 194, 139),
        warning: Color::Rgb(214, 170, 94),
        danger: Color::Rgb(224, 108, 117),
        tracked: Color::Rgb(185, 160, 106),
        exited: Color::Rgb(109, 114, 122),
        highlight: Color::Rgb(34, 37, 41),
        selection: Color::Rgb(27, 30, 33),
    },
    Theme {
        name: "Light",
        background: Color::Rgb(242, 241, 237),
        panel: Color::Rgb(250, 249, 246),
        panel_alt: Color::Rgb(231, 229, 223),
        border: Color::Rgb(169, 165, 157),
        text: Color::Rgb(37, 36, 33),
        muted: Color::Rgb(103, 99, 93),
        accent: Color::Rgb(66, 70, 76),
        focus_border: Color::Rgb(32, 130, 94),
        focus_surface: Color::Rgb(212, 209, 202),
        key_hint: Color::Rgb(54, 112, 91),
        table_selection_surface: Color::Rgb(218, 239, 235),
        table_column_surface: Color::Rgb(232, 234, 232),
        table_intersection_surface: Color::Rgb(184, 222, 214),
        graph_line: Color::Rgb(104, 109, 114),
        active_series: Color::Rgb(16, 119, 79),
        cursor_guide: Color::Rgb(154, 150, 142),
        success: Color::Rgb(47, 114, 68),
        warning: Color::Rgb(147, 98, 20),
        danger: Color::Rgb(179, 58, 71),
        tracked: Color::Rgb(122, 103, 65),
        exited: Color::Rgb(111, 106, 100),
        highlight: Color::Rgb(221, 218, 211),
        selection: Color::Rgb(236, 234, 229),
    },
];

pub(crate) fn theme_index_by_name(name: &str) -> usize {
    if name.eq_ignore_ascii_case("Light") || name.eq_ignore_ascii_case("Neutral Light") {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_are_dark_and_light_only() {
        assert_eq!(THEMES.len(), 2);
        assert_eq!(THEMES[0].name, "Dark");
        assert_eq!(THEMES[1].name, "Light");
    }

    #[test]
    fn built_in_themes_separate_focus_selection_and_semantic_status_colors() {
        let dark = THEMES[0];
        assert_eq!(dark.accent, Color::Rgb(201, 206, 214));
        assert_eq!(dark.focus_border, Color::Rgb(104, 196, 164));
        assert_ne!(dark.focus_border, dark.accent);
        assert_eq!(dark.focus_surface, Color::Rgb(48, 52, 58));
        assert_eq!(dark.key_hint, Color::Rgb(83, 151, 128));
        assert_ne!(dark.key_hint, dark.focus_border);
        assert_eq!(dark.table_selection_surface, Color::Rgb(19, 51, 48));
        assert_eq!(dark.table_column_surface, Color::Rgb(29, 38, 42));
        assert_eq!(dark.table_intersection_surface, Color::Rgb(32, 79, 73));
        assert_ne!(dark.table_selection_surface, dark.table_column_surface);
        assert_ne!(
            dark.table_selection_surface,
            dark.table_intersection_surface
        );
        assert_ne!(dark.table_column_surface, dark.table_intersection_surface);
        assert_eq!(dark.cursor_guide, Color::Rgb(101, 106, 112));
        assert_eq!(dark.graph_line, Color::Rgb(139, 144, 150));
        assert_eq!(dark.active_series, Color::Rgb(72, 190, 151));
        assert_ne!(dark.focus_border, dark.active_series);
        assert_eq!(dark.success, Color::Rgb(120, 194, 139));
        assert_eq!(dark.tracked, Color::Rgb(185, 160, 106));
        assert_ne!(dark.warning, dark.tracked);

        let light = THEMES[1];
        assert_eq!(light.accent, Color::Rgb(66, 70, 76));
        assert_eq!(light.focus_border, Color::Rgb(32, 130, 94));
        assert_ne!(light.focus_border, light.accent);
        assert_eq!(light.focus_surface, Color::Rgb(212, 209, 202));
        assert_eq!(light.key_hint, Color::Rgb(54, 112, 91));
        assert_ne!(light.key_hint, light.focus_border);
        assert_eq!(light.table_selection_surface, Color::Rgb(218, 239, 235));
        assert_eq!(light.table_column_surface, Color::Rgb(232, 234, 232));
        assert_eq!(light.table_intersection_surface, Color::Rgb(184, 222, 214));
        assert_ne!(light.table_selection_surface, light.table_column_surface);
        assert_ne!(
            light.table_selection_surface,
            light.table_intersection_surface
        );
        assert_ne!(light.table_column_surface, light.table_intersection_surface);
        assert_eq!(light.cursor_guide, Color::Rgb(154, 150, 142));
        assert_eq!(light.graph_line, Color::Rgb(104, 109, 114));
        assert_eq!(light.active_series, Color::Rgb(16, 119, 79));
        assert_ne!(light.focus_border, light.active_series);
        assert_eq!(light.success, Color::Rgb(47, 114, 68));
        assert_eq!(light.tracked, Color::Rgb(122, 103, 65));
        assert_ne!(light.warning, light.tracked);
    }

    #[test]
    fn theme_lookup_keeps_legacy_names_compatible() {
        assert_eq!(theme_index_by_name("Dark"), 0);
        assert_eq!(theme_index_by_name("Light"), 1);
        assert_eq!(theme_index_by_name("Neutral Light"), 1);
        assert_eq!(theme_index_by_name("Ocean Pop"), 0);
        assert_eq!(theme_index_by_name("unknown"), 0);
    }
}
