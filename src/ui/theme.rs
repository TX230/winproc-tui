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
    pub(crate) accent_alt: Color,
    pub(crate) graph_line: Color,
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
        background: Color::Rgb(10, 11, 13),
        panel: Color::Rgb(15, 17, 20),
        panel_alt: Color::Rgb(27, 30, 35),
        border: Color::Rgb(58, 64, 72),
        text: Color::Rgb(224, 228, 234),
        muted: Color::Rgb(134, 142, 153),
        accent: Color::Rgb(86, 166, 255),
        accent_alt: Color::Rgb(42, 91, 135),
        graph_line: Color::Rgb(244, 247, 251),
        success: Color::Rgb(93, 193, 120),
        warning: Color::Rgb(224, 170, 58),
        danger: Color::Rgb(218, 92, 99),
        tracked: Color::Rgb(231, 184, 69),
        exited: Color::Rgb(92, 98, 108),
        highlight: Color::Rgb(37, 43, 50),
        selection: Color::Rgb(36, 30, 22),
    },
    Theme {
        name: "Light",
        background: Color::Rgb(244, 247, 251),
        panel: Color::Rgb(255, 255, 255),
        panel_alt: Color::Rgb(230, 238, 247),
        border: Color::Rgb(148, 163, 184),
        text: Color::Rgb(15, 23, 42),
        muted: Color::Rgb(71, 85, 105),
        accent: Color::Rgb(8, 145, 178),
        accent_alt: Color::Rgb(37, 99, 235),
        graph_line: Color::Rgb(15, 23, 42),
        success: Color::Rgb(5, 150, 105),
        warning: Color::Rgb(180, 83, 9),
        danger: Color::Rgb(220, 38, 38),
        tracked: Color::Rgb(180, 83, 9),
        exited: Color::Rgb(100, 116, 139),
        highlight: Color::Rgb(203, 213, 225),
        selection: Color::Rgb(255, 237, 213),
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
    fn theme_lookup_keeps_legacy_names_compatible() {
        assert_eq!(theme_index_by_name("Dark"), 0);
        assert_eq!(theme_index_by_name("Light"), 1);
        assert_eq!(theme_index_by_name("Neutral Light"), 1);
        assert_eq!(theme_index_by_name("Ocean Pop"), 0);
        assert_eq!(theme_index_by_name("unknown"), 0);
    }
}
