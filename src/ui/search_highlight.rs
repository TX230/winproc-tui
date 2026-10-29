use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::theme::Theme;

/// Highlight every occurrence using the same normalized terms as row filtering.
pub(crate) fn line(value: &str, query: &str, style: Style, theme: Theme) -> Line<'static> {
    let terms = crate::model::search::terms(query);
    matched_line(value, value.to_lowercase(), terms, style, theme)
}

pub(crate) fn path_line(value: &str, query: &str, style: Style, theme: Theme) -> Line<'static> {
    let terms = crate::model::search::terms(query)
        .iter()
        .map(|term| crate::model::file_users::normalize_search_path(term))
        .collect();
    matched_line(
        value,
        value.to_lowercase().replace('/', "\\"),
        terms,
        style,
        theme,
    )
}

fn matched_line(
    value: &str,
    lower: String,
    terms: Vec<String>,
    style: Style,
    theme: Theme,
) -> Line<'static> {
    if terms.iter().all(String::is_empty) {
        return Line::from(Span::styled(value.to_owned(), style)).style(style);
    }
    let mut matched = vec![false; lower.len()];
    for term in terms.into_iter().filter(|term| !term.is_empty()) {
        for (start, _) in lower.char_indices() {
            if lower[start..].starts_with(&term) {
                matched[start..start + term.len()].fill(true);
            }
        }
    }
    let mut offset = 0;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for ch in value.chars() {
        let len = ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        let hit = matched[offset..offset + len].iter().any(|hit| *hit);
        offset += len;
        let style = if hit { style.fg(theme.warning) } else { style };
        if let Some(last) = spans.last_mut().filter(|span| span.style == style) {
            last.content.to_mut().push(ch);
        } else {
            spans.push(Span::styled(ch.to_string(), style));
        }
    }
    Line::from(spans).style(style)
}

/// Fit after highlighting so a match crossing the clipping edge stays highlighted.
pub(crate) fn fit(
    line: Line<'static>,
    width: usize,
    tail: bool,
    marker: &str,
    pad: bool,
) -> Line<'static> {
    let style = line.style;
    let mut result = line;
    if result.width() > width {
        let marker: String = marker.chars().take(width).collect();
        let available = width.saturating_sub(Span::raw(&marker).width());
        let mut chars = result
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(move |ch| (ch, span.style)))
            .collect::<Vec<_>>();
        if tail {
            chars.reverse();
        }
        let mut used = 0;
        let mut kept = Vec::new();
        for (ch, style) in chars {
            let size = Span::raw(ch.to_string()).width();
            if used + size > available {
                break;
            }
            used += size;
            kept.push(Span::styled(ch.to_string(), style));
        }
        if tail {
            kept.reverse();
            kept.insert(0, Span::styled(marker, style));
        } else {
            kept.push(Span::styled(marker, style));
        }
        result = Line::from(kept).style(style);
    }
    if pad {
        result.spans.push(Span::styled(
            " ".repeat(width.saturating_sub(result.width())),
            style,
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terms_overlap_repeat_and_preserve_unicode_and_selection() {
        let theme = super::super::theme::THEMES[0];
        let style = Style::default().bg(theme.table_selection_surface);
        let result = line("İ日本 ababa ABABA", "İ 日本 aba bab", style, theme);
        assert_eq!(result.to_string(), "İ日本 ababa ABABA");
        for span in result
            .spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
        {
            assert_eq!(span.style.fg, Some(theme.warning));
            assert_eq!(span.style.bg, style.bg);
        }
    }
    #[test]
    fn clipped_phrase_keeps_visible_match_without_highlighting_marker() {
        let theme = super::super::theme::THEMES[0];
        let style = Style::default().fg(theme.text);
        let result = fit(
            line("C:\\Program Files\\app", "\"Program Files\"", style, theme),
            12,
            true,
            "...",
            true,
        );
        assert_eq!(result.to_string(), "...Files\\app");
        assert_eq!(result.spans[0].style, style);
        assert_eq!(result.spans[1].style.fg, Some(theme.warning));
        assert_eq!(result.width(), 12);
        assert!(
            line("日本", " \"\" ", style, theme)
                .spans
                .iter()
                .all(|span| span.style == style)
        );
    }
}
