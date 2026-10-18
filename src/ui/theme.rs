use ratatui::prelude::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) name: &'static str,
    pub(crate) background: Color,
    pub(crate) panel: Color,
    pub(crate) panel_alt: Color,
    pub(crate) menu_bar: Color,
    pub(crate) menu_active: Color,
    pub(crate) border: Color,
    pub(crate) text: Color,
    pub(crate) muted: Color,
    pub(crate) accent: Color,
    pub(crate) focus_border: Color,
    // Focused controls and pointer hover; never a replacement for a selected table row.
    pub(crate) focus_surface: Color,
    pub(crate) key_hint: Color,
    // Persistent row selection in this theme, independent of keyboard focus.
    // Column intersections may use table_intersection_surface on top of this base.
    pub(crate) table_selection_surface: Color,
    pub(crate) table_column_surface: Color,
    pub(crate) table_intersection_surface: Color,
    pub(crate) graph_line: Color,
    pub(crate) active_series: Color,
    pub(crate) cursor_guide: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    pub(crate) tracked: Color,
    pub(crate) exited: Color,
    pub(crate) highlight: Color,
    pub(crate) selection: Color,
}

impl Theme {
    pub(crate) fn with_high_contrast(mut self, enabled: bool) -> Self {
        if enabled {
            self.text = Color::Rgb(255, 255, 255);
            self.muted = Color::Rgb(195, 198, 202);
            self.border = Color::Rgb(132, 140, 150);
            self.key_hint = self.focus_border;
            self.menu_bar = Color::Rgb(48, 51, 56);
            self.menu_active = Color::Rgb(82, 87, 94);
            self.graph_line = Color::Rgb(195, 198, 202);
            self.cursor_guide = Color::Rgb(169, 176, 184);
            self.exited = Color::Rgb(174, 182, 190);
        }
        self
    }
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
        menu_bar: Color::Rgb(38, 41, 46),
        menu_active: Color::Rgb(67, 72, 80),
        border: Color::Rgb(53, 58, 64),
        text: Color::Rgb(230, 226, 218),
        muted: Color::Rgb(154, 152, 146),
        accent,
        focus_border,
        focus_surface: Color::Rgb(48, 52, 58),
        key_hint,
        table_selection_surface,
        table_column_surface,
        table_intersection_surface,
        graph_line: Color::Rgb(139, 144, 150),
        active_series,
        cursor_guide: Color::Rgb(101, 106, 112),
        warning,
        danger: Color::Rgb(224, 108, 117),
        tracked,
        exited: Color::Rgb(145, 150, 158),
        highlight: Color::Rgb(34, 37, 41),
        selection: Color::Rgb(27, 30, 33),
    }
}

pub(crate) const THEMES: [Theme; 4] = [
    dark_theme(
        "Green",
        Color::Rgb(201, 206, 214),
        Color::Rgb(104, 196, 164),
        Color::Rgb(100, 174, 149),
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
        Color::Rgb(197, 170, 94),
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
        Color::Rgb(199, 135, 91),
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

pub(crate) fn contrasting_foreground(background: Color, theme: Theme) -> Color {
    let light_contrast = contrast_ratio(background, theme.text);
    let dark_contrast = contrast_ratio(background, theme.background);
    if dark_contrast >= light_contrast {
        theme.background
    } else {
        theme.text
    }
}

fn contrast_ratio(first: Color, second: Color) -> f64 {
    let lighter = relative_luminance(first).max(relative_luminance(second));
    let darker = relative_luminance(first).min(relative_luminance(second));
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Color) -> f64 {
    let Color::Rgb(red, green, blue) = color else {
        return 0.0;
    };
    0.2126 * linear_channel(red) + 0.7152 * linear_channel(green) + 0.0722 * linear_channel(blue)
}

fn linear_channel(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
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
            assert_ne!(theme.table_selection_surface, theme.panel);
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

    #[test]
    fn text_roles_meet_local_contrast_in_normal_and_high_contrast_dark_palettes() {
        for normal in THEMES {
            let high = normal.with_high_contrast(true);
            for theme in [normal, high] {
                for surface in [theme.background, theme.panel, theme.panel_alt] {
                    for foreground in [theme.text, theme.muted, theme.key_hint, theme.exited] {
                        assert!(
                            contrast_ratio(foreground, surface) >= 4.5,
                            "{} {foreground:?} on {surface:?}",
                            theme.name
                        );
                    }
                }
            }
            assert!(
                contrast_ratio(high.muted, high.panel) > contrast_ratio(normal.muted, normal.panel)
            );
        }
    }

    #[test]
    fn semantic_title_fills_choose_the_higher_contrast_foreground() {
        for theme in THEMES {
            for background in [theme.focus_border, theme.warning, theme.danger] {
                let foreground = contrasting_foreground(background, theme);
                assert_eq!(foreground, theme.background);
                assert!(
                    contrast_ratio(background, foreground) >= 4.5,
                    "theme={}, background={background:?}",
                    theme.name
                );
            }
        }
    }
}
