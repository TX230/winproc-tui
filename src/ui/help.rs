use ratatui::{
    layout::{Alignment, Rect},
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    ui::{Theme, widgets::scrollable_modal::ScrollableModal},
};

const HELP_TITLE: &str = "Keyboard shortcuts";
const CLOSE_BUTTON: &str = "[ Close ]";
const COLUMN_SEPARATOR: &str = "  │  ";
const KEY_LABEL_GAP: usize = 2;
const CLOSE_AREA_HEIGHT: u16 = 3;

#[derive(Clone, Copy)]
struct HelpItem {
    key: &'static str,
    label: &'static str,
}

struct HelpSection {
    title: &'static str,
    focus_hint: Option<&'static str>,
    rows: &'static [HelpItem],
}

const GLOBAL_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "q",
        label: "Quit",
    },
    HelpItem {
        key: "Esc",
        label: "Quit / Live in PLAY",
    },
    HelpItem {
        key: "?",
        label: "Toggle Help",
    },
    HelpItem {
        key: "Tab/Shift+Tab",
        label: "Move focus",
    },
    HelpItem {
        key: "F2",
        label: "Toggle theme",
    },
    HelpItem {
        key: "Ctrl+C",
        label: "Copy selected row",
    },
    HelpItem {
        key: "Ctrl+L",
        label: "Open log list",
    },
    HelpItem {
        key: "Ctrl+R",
        label: "Toggle recording",
    },
    HelpItem {
        key: "Ctrl+P",
        label: "Pause / Resume",
    },
    HelpItem {
        key: "Ctrl+O",
        label: "Open Settings",
    },
];

const PROCESSES_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Ctrl+F",
        label: "Edit name filter",
    },
    HelpItem {
        key: "Ctrl+I/J",
        label: "Jump by name (next match)",
    },
    HelpItem {
        key: "Up/Down",
        label: "Move selected row",
    },
    HelpItem {
        key: "PageUp/PageDown",
        label: "Move by page",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
    HelpItem {
        key: "Left/Right",
        label: "Select column",
    },
    HelpItem {
        key: "Shift+Left/Right",
        label: "Move metric column",
    },
    HelpItem {
        key: "1/2/3/4",
        label: "Set to Graph#n",
    },
    HelpItem {
        key: "0",
        label: "Clear all Graph slots",
    },
    HelpItem {
        key: "Space",
        label: "Track / Untrack process",
    },
    HelpItem {
        key: "t",
        label: "Toggle Tracked-only",
    },
    HelpItem {
        key: "s",
        label: "Sort by selected column",
    },
    HelpItem {
        key: "c",
        label: "Pick columns",
    },
    HelpItem {
        key: "g",
        label: "Toggle Graphs panel",
    },
    HelpItem {
        key: "i",
        label: "Toggle System/Process Info",
    },
    HelpItem {
        key: "f",
        label: "Open files of process",
    },
    HelpItem {
        key: "Delete",
        label: "Clear metric / hide row",
    },
    HelpItem {
        key: "Ctrl+U",
        label: "Refresh open-files list",
    },
];

const RAM_VRAM_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Up/Down",
        label: "Move selected metric",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
    HelpItem {
        key: "1/2/3/4",
        label: "Set to Graph#n",
    },
];

const CPU_ROWS: &[HelpItem] = &[HelpItem {
    key: "1/2/3/4",
    label: "Set CPU Avg to Graph#n",
}];

const GRAPH_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Left/Right",
        label: "Select sample",
    },
    HelpItem {
        key: "Ctrl+Left/Right",
        label: "Pan time range",
    },
    HelpItem {
        key: "Right/Ctrl+left drag",
        label: "Pan time range",
    },
    HelpItem {
        key: "PageUp/PageDown",
        label: "Change time span",
    },
    HelpItem {
        key: "z",
        label: "Toggle Y-axis Min 0",
    },
    HelpItem {
        key: "f",
        label: "Fit all samples",
    },
];

const SAMPLES_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Up/Down",
        label: "Move selected sample",
    },
    HelpItem {
        key: "PageUp/PageDown",
        label: "Move by page",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
];

const AB_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "a",
        label: "Set A at sample",
    },
    HelpItem {
        key: "b",
        label: "Set B at sample",
    },
    HelpItem {
        key: "Shift+A/B",
        label: "Jump to A or B",
    },
    HelpItem {
        key: "x",
        label: "Clear A/B comparison",
    },
];

const MOUSE_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Click panel",
        label: "Focus clicked panel",
    },
    HelpItem {
        key: "Click row",
        label: "Select clicked row",
    },
    HelpItem {
        key: "Click Graph item",
        label: "Match process row",
    },
    HelpItem {
        key: "Drag scrollbar",
        label: "Scroll",
    },
    HelpItem {
        key: "Wheel",
        label: "Scroll / Move selection",
    },
    HelpItem {
        key: "Ctrl+Wheel",
        label: "Terminal zoom",
    },
    HelpItem {
        key: "Right click",
        label: "Samples auto-scroll",
    },
    HelpItem {
        key: "Click [ Close ]",
        label: "Close dialog",
    },
];

const LEFT_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Global",
        focus_hint: Some("any focus"),
        rows: GLOBAL_ROWS,
    },
    HelpSection {
        title: "Processes",
        focus_hint: None,
        rows: PROCESSES_ROWS,
    },
];

const RIGHT_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "RAM/VRAM",
        focus_hint: None,
        rows: RAM_VRAM_ROWS,
    },
    HelpSection {
        title: "CPUs",
        focus_hint: None,
        rows: CPU_ROWS,
    },
    HelpSection {
        title: "Graph",
        focus_hint: None,
        rows: GRAPH_ROWS,
    },
    HelpSection {
        title: "Samples",
        focus_hint: None,
        rows: SAMPLES_ROWS,
    },
    HelpSection {
        title: "A/B comparison",
        focus_hint: Some("Graph or Samples"),
        rows: AB_ROWS,
    },
    HelpSection {
        title: "Mouse",
        focus_hint: None,
        rows: MOUSE_ROWS,
    },
];

pub(crate) fn draw_help(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let layout = help_modal().render(
        frame,
        area,
        Text::from(help_lines(theme)),
        app.help_scroll.offset,
        false,
        theme,
    );
    if let Some(area) = help_close_button_area_in_footer(layout.footer) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                CLOSE_BUTTON,
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            area,
        );
    }
}

pub(crate) fn help_area(area: Rect) -> Rect {
    help_modal().area(area)
}

pub(crate) fn help_close_button_area(popup: Rect) -> Option<Rect> {
    if popup.width < 11 || popup.height < 4 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        popup.x + popup.width.saturating_sub(width) / 2,
        popup.bottom().saturating_sub(3),
        width,
        1,
    ))
}

fn help_close_button_area_in_footer(footer: Rect) -> Option<Rect> {
    if footer.width < 11 || footer.height == 0 {
        return None;
    }
    let width = 11;
    Some(Rect::new(
        footer.x + footer.width.saturating_sub(width) / 2,
        footer.y.saturating_add(1),
        width,
        1,
    ))
}

pub(crate) fn help_page_size_for_screen(area: Rect) -> usize {
    help_modal().page_size(area)
}

pub(crate) fn help_scroll_max_for_page_size(page_size: usize) -> usize {
    help_modal().max_offset_for_page_size(page_size)
}

pub(crate) fn help_scrollbar_area(area: Rect, page_size: usize) -> Option<Rect> {
    help_modal().scrollbar_area(area, page_size)
}

#[derive(Clone)]
struct ColumnRow {
    spans: Vec<Span<'static>>,
    width: usize,
}

impl ColumnRow {
    fn blank() -> Self {
        Self {
            spans: Vec::new(),
            width: 0,
        }
    }
}

fn help_lines(theme: Theme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            HELP_TITLE,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    let left = render_column(LEFT_SECTIONS, theme);
    let right = render_column(RIGHT_SECTIONS, theme);
    let left_width = column_max_width(&left);
    let max_rows = left.len().max(right.len());

    for i in 0..max_rows {
        let left_row = left.get(i).cloned().unwrap_or_else(ColumnRow::blank);
        let right_row = right.get(i).cloned().unwrap_or_else(ColumnRow::blank);
        let mut spans = left_row.spans;
        let pad = left_width.saturating_sub(left_row.width);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans.push(Span::styled(
            COLUMN_SEPARATOR,
            Style::default().fg(theme.muted),
        ));
        spans.extend(right_row.spans);
        lines.push(Line::from(spans));
    }

    lines
}

fn render_column(sections: &[HelpSection], theme: Theme) -> Vec<ColumnRow> {
    let mut rows = Vec::new();
    for (idx, section) in sections.iter().enumerate() {
        if idx > 0 {
            rows.push(ColumnRow::blank());
        }
        rows.push(section_header_row(section, theme));
        let key_width = section_key_width(section);
        for item in section.rows {
            rows.push(shortcut_row(item, key_width, theme));
        }
    }
    rows
}

fn section_header_row(section: &HelpSection, theme: Theme) -> ColumnRow {
    let mut width = section.title.chars().count();
    let mut spans = vec![Span::styled(
        section.title,
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(hint) = section.focus_hint {
        let gap = "  ";
        let hint_text = format!("({hint})");
        width += gap.chars().count() + hint_text.chars().count();
        spans.push(Span::raw(gap));
        spans.push(Span::styled(hint_text, Style::default().fg(theme.muted)));
    }
    ColumnRow { spans, width }
}

fn shortcut_row(item: &HelpItem, key_width: usize, theme: Theme) -> ColumnRow {
    let key_len = item.key.chars().count();
    let pad = key_width.saturating_sub(key_len) + KEY_LABEL_GAP;
    let label_len = item.label.chars().count();
    let width = key_len + pad + label_len;
    let spans = vec![
        Span::styled(
            item.key,
            Style::default()
                .fg(theme.text)
                .bg(theme.panel_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(pad)),
        Span::styled(item.label, Style::default().fg(theme.text)),
    ];
    ColumnRow { spans, width }
}

fn section_key_width(section: &HelpSection) -> usize {
    section
        .rows
        .iter()
        .map(|item| item.key.chars().count())
        .max()
        .unwrap_or(0)
}

fn column_max_width(rows: &[ColumnRow]) -> usize {
    rows.iter().map(|row| row.width).max().unwrap_or(0)
}

fn help_content_width() -> u16 {
    let left = render_column_widths(LEFT_SECTIONS);
    let right = render_column_widths(RIGHT_SECTIONS);
    let title_width = HELP_TITLE.chars().count().max(CLOSE_BUTTON.chars().count());
    let body_width = left + COLUMN_SEPARATOR.chars().count() + right;
    body_width.max(title_width) as u16
}

fn render_column_widths(sections: &[HelpSection]) -> usize {
    let mut max_width = 0usize;
    for section in sections {
        let header_width = section_header_width(section);
        max_width = max_width.max(header_width);
        let key_width = section_key_width(section);
        for item in section.rows {
            let row_width = key_width + KEY_LABEL_GAP + item.label.chars().count();
            max_width = max_width.max(row_width);
        }
    }
    max_width
}

fn section_header_width(section: &HelpSection) -> usize {
    let mut width = section.title.chars().count();
    if let Some(hint) = section.focus_hint {
        width += 2 + 1 + hint.chars().count() + 1;
    }
    width
}

fn help_content_line_count() -> u16 {
    let left = column_line_count(LEFT_SECTIONS);
    let right = column_line_count(RIGHT_SECTIONS);
    (2 + left.max(right)) as u16
}

fn column_line_count(sections: &[HelpSection]) -> usize {
    let mut lines = 0;
    for (idx, section) in sections.iter().enumerate() {
        if idx > 0 {
            lines += 1;
        }
        lines += 1 + section.rows.len();
    }
    lines
}

fn help_modal() -> ScrollableModal {
    ScrollableModal::new(
        "Help",
        help_content_width(),
        help_content_line_count(),
        CLOSE_AREA_HEIGHT,
    )
}
