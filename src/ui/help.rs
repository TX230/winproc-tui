use ratatui::{
    layout::Rect,
    prelude::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    App,
    model::{GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY, TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY},
    ui::{
        Theme, footer::shortcut_spans, format::format_integer,
        widgets::scrollable_modal::ScrollableModal,
    },
};

const COLUMN_SEPARATOR: &str = "  │  ";
const KEY_LABEL_GAP: usize = 2;
const FOOTER_HEIGHT: u16 = 1;
const HELP_SHORTCUT_ITEMS: [(&str, &str); 6] = [
    ("s", "Sections"),
    ("←/→", "Section"),
    ("↑/↓", "Scroll"),
    ("PageUp/PageDown", "Page"),
    ("Home/End", "Jump"),
    ("Esc/Enter/F1/?", "Close"),
];

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

const FILE_USERS_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "/ (Mode / Results)",
        label: "Edit Query; opening file search focuses Query",
    },
    HelpItem {
        key: "F3 / F4 or Tools",
        label: "Network / Find processes by file (Live/REC)",
    },
    HelpItem {
        key: "Find processes by file: Tab/Shift+Tab",
        label: "Focus Query, Mode, or Results",
    },
    HelpItem {
        key: "Enter / Ctrl+U (file search)",
        label: "Search from Query / repeat search",
    },
    HelpItem {
        key: "←/→, Space/Enter (Mode)",
        label: "Filename / path substring / exact path",
    },
    HelpItem {
        key: "Enter (file results)",
        label: "Open Files for the process using this file",
    },
    HelpItem {
        key: "Space (file results)",
        label: "Show the full path and search coverage",
    },
    HelpItem {
        key: "Esc (file search)",
        label: "Cancel running scan; otherwise back/close",
    },
    HelpItem {
        key: "↑/↓, PgUp/PgDn, Home/End",
        label: "Navigate file results/details; Ctrl+C copies row",
    },
];

const GLOBAL_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Alt+S/P/V/O/T",
        label: "Session / Profile / View / Tools / Settings menus",
    },
    HelpItem {
        key: "Menu / Shift+F10",
        label: "Context actions for selected object; Esc or outside click closes",
    },
    HelpItem {
        key: "Click shortcut",
        label: "Run the displayed footer action; hover highlights its full group",
    },
    HelpItem {
        key: "Click heading / outside menu",
        label: "Toggle/switch category; outside closes; confirmations stay explicit",
    },
    HelpItem {
        key: "F2/F3/F4",
        label: "Processes / Network / Find processes by file view",
    },
    HelpItem {
        key: "Ctrl+↑/↓/←/→",
        label: "Focus neighboring visible panel (no wrap)",
    },
    HelpItem {
        key: "Ctrl+B (Log view)",
        label: "Return to Live; Space on a metric adds a Graph",
    },
    HelpItem {
        key: "Shift+T",
        label: "Toggle Tracked-only (any main panel)",
    },
    HelpItem {
        key: "q",
        label: "Quit",
    },
    HelpItem {
        key: "Esc",
        label: "Open Session menu (Quit is last)",
    },
    HelpItem {
        key: "↑/↓, ←/→, Enter",
        label: "Select item, switch menu (including Help), activate",
    },
    HelpItem {
        key: "Space",
        label: "Toggle selected main menu checkbox",
    },
    HelpItem {
        key: "F1/?",
        label: "F1: Help over any dialog at current task; ?: outside text input",
    },
    HelpItem {
        key: "F12",
        label: "Cycle theme; Settings selects theme and contrast directly",
    },
    HelpItem {
        key: "Tab/Shift+Tab",
        label: "Move focus",
    },
    HelpItem {
        key: "Ctrl+C",
        label: "Copy the focused row, or the contents of System Info",
    },
    HelpItem {
        key: "Ctrl+L",
        label: "Open log list",
    },
    HelpItem {
        key: "Ctrl+R",
        label: "Start recording / confirm stop",
    },
    HelpItem {
        key: "Ctrl+P",
        label: "Pause / Resume display",
    },
];

const PROCESSES_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "… in Tree indentation",
        label: "Deeper ancestors compressed; Enter opens the full process name",
    },
    HelpItem {
        key: "Stronger cell highlight",
        label: "The stronger cell highlight shows keyboard focus.",
    },
    HelpItem {
        key: "* before PID",
        label: "Multi-selected row; count stays in Processes title",
    },
    HelpItem {
        key: "Enter",
        label: "Open Process Info for the focused process",
    },
    HelpItem {
        key: "Ctrl+C",
        label: "Copy the focused row",
    },
    HelpItem {
        key: "d/Delete",
        label: "Kill selected processes, or the focused process if none are selected",
    },
    HelpItem {
        key: "v",
        label: "Toggle Flat / Tree view (Live and Recording)",
    },
    HelpItem {
        key: "e",
        label: "Expand/collapse Tree row (no filter)",
    },
    HelpItem {
        key: "Ctrl+F",
        label: "Edit filter: Enter apply, Esc clear",
    },
    HelpItem {
        key: "Ctrl+I/J",
        label: "Jump by name (next match)",
    },
    HelpItem {
        key: "↑/↓",
        label: "Move focus to the previous/next row; clear process selection.",
    },
    HelpItem {
        key: "Shift+↑/↓",
        label: "Select row range",
    },
    HelpItem {
        key: "Alt+↑/↓",
        label: "Move focus to the previous/next row; keep process selection.",
    },
    HelpItem {
        key: "Ctrl+A (Tracked-only)",
        label: "Select all listed process rows",
    },
    HelpItem {
        key: "Ctrl+Space",
        label: "Select/deselect the focused live process",
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
        key: "←/→",
        label: "Select column",
    },
    HelpItem {
        key: "Shift+←/→",
        label: "Move metric column",
    },
    HelpItem {
        key: "w/Shift+W",
        label: "Widen / narrow column",
    },
    HelpItem {
        key: "Space (Process/PID)",
        label: "Track/untrack by process name (Live only)",
    },
    HelpItem {
        key: "Space (metric)",
        label: "Add/remove Graph",
    },
    HelpItem {
        key: "s",
        label: "Sort by selected column",
    },
    HelpItem {
        key: "c",
        label: "Pick columns (also right-click header)",
    },
    HelpItem {
        key: "g",
        label: "Toggle Graphs panel",
    },
    HelpItem {
        key: "f",
        label: "Open Files for the focused process",
    },
    HelpItem {
        key: "i",
        label: "Open System Info",
    },
];

const PROCESS_INFO_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Ctrl+←/→",
        label: "Switch Info tabs",
    },
    HelpItem {
        key: "Tab / Shift+Tab",
        label: "Focus interactive Info content",
    },
    HelpItem {
        key: "←/→",
        label: "Switch focused Info tabs",
    },
    HelpItem {
        key: "Ctrl+U",
        label: "Refresh Info tab",
    },
    HelpItem {
        key: "Type (Files)",
        label: "Filter full paths; one row per handle",
    },
    HelpItem {
        key: "↑/↓, PgUp/PgDn (Files)",
        label: "Select handle; Home/End first/last",
    },
    HelpItem {
        key: "←/→, Backspace/Delete (Files)",
        label: "Move filter cursor / edit filter",
    },
    HelpItem {
        key: "Enter (Files)",
        label: "Full path, I/O attributes, raw masks",
    },
    HelpItem {
        key: "Esc/Enter (file detail)",
        label: "Return to handle list; Esc closes list",
    },
    HelpItem {
        key: "Ctrl+C (Files)",
        label: "Copy TSV rows; detail copies one handle",
    },
    HelpItem {
        key: "Files: Y/N",
        label: "Cached, Async, W-Thru; -- unavailable",
    },
    HelpItem {
        key: "Files: Access",
        label: "R read, W write, A append; - none; -- unknown",
    },
    HelpItem {
        key: "Files: Auto",
        label: "2–10s while visible; >1s/error stops; Ctrl+U retry",
    },
    HelpItem {
        key: "/ (Environment)",
        label: "Start Filter editing; / stays literal while editing",
    },
    HelpItem {
        key: "Ctrl+F (Info)",
        label: "Edit Files / DLL / Environment / Network filter",
    },
    HelpItem {
        key: "Home/End, ←/→ (filter)",
        label: "Move text cursor; Ctrl+U clears text",
    },
    HelpItem {
        key: "Type (DLL / Environment)",
        label: "Filter names and values; F1 opens Help without editing text",
    },
    HelpItem {
        key: "Enter / Esc",
        label: "Open selected row detail / return or close",
    },
    HelpItem {
        key: "Ctrl+C",
        label: "Copy current tab or selected detail",
    },
];

const SCHEDULING_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Ctrl+D",
        label: "Reset focused setting to Normal / all allowed CPUs",
    },
    HelpItem {
        key: "p / a",
        label: "Focus priority / CPU affinity (single group only)",
    },
    HelpItem {
        key: "Space / click CPU",
        label: "Toggle and apply immediately",
    },
    HelpItem {
        key: "←/→/↑/↓, Home/End",
        label: "Priority arrows apply; CPU arrows only move focus",
    },
    HelpItem {
        key: "Enter / Esc",
        label: "Select focused priority / close; requires live display",
    },
    HelpItem {
        key: "Ctrl+U / Ctrl+Z",
        label: "Refresh / restore the previous value immediately",
    },
    HelpItem {
        key: "PgUp/PgDn",
        label: "Scroll details and notices",
    },
    HelpItem {
        key: "Tab / Ctrl+←/→",
        label: "Cycle tabs, priority, CPUs / change tab",
    },
];

const RAM_VRAM_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Tab / Ctrl+←/→",
        label: "Move focus between MEM and GPU panels",
    },
    HelpItem {
        key: "←/→",
        label: "Switch visible MEM column / GPU adapter",
    },
    HelpItem {
        key: "↑/↓",
        label: "Select the previous/next metric",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
    HelpItem {
        key: "Space",
        label: "Toggle selected metric Graph",
    },
    HelpItem {
        key: "g",
        label: "Toggle Graphs panel",
    },
];

const SYSTEM_ACTIVITY_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "↑/↓",
        label: "Select the previous/next metric",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
    HelpItem {
        key: "Space",
        label: "Toggle selected metric Graph",
    },
];

const CPU_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "↑/↓",
        label: "Select Usage / Threads / Processes / Per-core",
    },
    HelpItem {
        key: "Home/End",
        label: "Select Usage / Per-core",
    },
    HelpItem {
        key: "Space",
        label: "Toggle selected metric Graph",
    },
    HelpItem {
        key: "Enter",
        label: "Open selected Per-core usage grid",
    },
];

const TRACKING_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Tracking scope",
        label: "Tracking applies to all processes with the same process name, including those started later.",
    },
    HelpItem {
        key: "Profiles",
        label: "Save named tracking lists",
    },
    HelpItem {
        key: "Resume last",
        label: "Restore the previous tracking list",
    },
    HelpItem {
        key: "Graphs",
        label: "Start empty each time the app starts",
    },
    HelpItem {
        key: "Ctrl+S (Profiles)",
        label: "Save the current tracking list as a new profile",
    },
    HelpItem {
        key: "t",
        label: "Track/untrack the focused process name (Live only)",
    },
    HelpItem {
        key: "Ctrl+T",
        label: "Open an Investigation Profile",
    },
    HelpItem {
        key: "Ctrl+S",
        label: "Save the active profile; open Save As if none is active (Live only)",
    },
];

const GRAPH_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "Up",
        label: "Select previous Graph",
    },
    HelpItem {
        key: "Down",
        label: "Select next Graph",
    },
    HelpItem {
        key: "Shift+↑/↓",
        label: "Move active Graph",
    },
    HelpItem {
        key: "s",
        label: "Open Graph reorder dialog",
    },
    HelpItem {
        key: "Delete",
        label: "Remove active Graph",
    },
    HelpItem {
        key: "m",
        label: "Toggle active Graph Raw / MA5",
    },
    HelpItem {
        key: "Left",
        label: "Select older sample time",
    },
    HelpItem {
        key: "Right",
        label: "Select newer sample time",
    },
    HelpItem {
        key: "Enter",
        label: "Open Process Info",
    },
    HelpItem {
        key: "Alt+←/→",
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
        key: "f/z",
        label: "Fit all / compact Min0",
    },
    HelpItem {
        key: "v/d/l",
        label: "Samples / Delta / layout",
    },
];

const SAMPLES_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "History / Follow latest",
        label: "Browse past samples; press End to follow the latest sample.",
    },
    HelpItem {
        key: "Shift+↑/↓",
        label: "Move active Graph",
    },
    HelpItem {
        key: "s",
        label: "Open Graph reorder dialog",
    },
    HelpItem {
        key: "Up/Left",
        label: "Select older sample",
    },
    HelpItem {
        key: "Down/Right",
        label: "Select newer sample",
    },
    HelpItem {
        key: "Delete",
        label: "Remove active Graph",
    },
    HelpItem {
        key: "m",
        label: "Toggle active Graph Raw / MA5",
    },
    HelpItem {
        key: "PageUp/PageDown",
        label: "Move sample selection by page",
    },
    HelpItem {
        key: "Home/End",
        label: "Move to top / bottom",
    },
];

const AB_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "a",
        label: "Set A range endpoint",
    },
    HelpItem {
        key: "b",
        label: "Set B; show range statistics",
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
        key: "Right-click column name",
        label: "Open Columns; click checkboxes to apply",
    },
    HelpItem {
        key: "Click column range arrows",
        label: "Reveal earlier/later columns",
    },
    HelpItem {
        key: "Click Width [-]/[+]",
        label: "Resize selected process column",
    },
    HelpItem {
        key: "Click menu heading",
        label: "Open category; click another heading to switch",
    },
    HelpItem {
        key: "Click panel",
        label: "Focus clicked panel",
    },
    HelpItem {
        key: "Click row",
        label: "Select clicked row",
    },
    HelpItem {
        key: "Click Tree disclosure",
        label: "Expand/collapse subtree (no filter)",
    },
    HelpItem {
        key: "Double-click metric",
        label: "Add or remove Graph",
    },
    HelpItem {
        key: "Double-click Process/PID",
        label: "Open Process Info for that process lifetime",
    },
    HelpItem {
        key: "Click Graph nav/card",
        label: "Select Graph",
    },
    HelpItem {
        key: "Click [x]",
        label: "Remove Graph",
    },
    HelpItem {
        key: "Click [-]/[+]",
        label: "Zoom time span out / in",
    },
    HelpItem {
        key: "Drag scrollbar",
        label: "Scroll",
    },
    HelpItem {
        key: "Drag Processes/Graphs border",
        label: "Resize Processes height",
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
        label: "Context actions for process, Graph, Samples, or endpoint",
    },
];

const PROCESS_GRAPH_SPLIT_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "h/Shift+H",
        label: "Increase / decrease Processes height",
    },
    HelpItem {
        key: "Alt+H",
        label: "Reset Processes height to Auto",
    },
];

const NETWORK_ROWS: &[HelpItem] = &[
    HelpItem {
        key: "F3 / Tools > Network",
        label: "Open Network endpoints (Live / Recording)",
    },
    HelpItem {
        key: "Process Info > Network",
        label: "Inspect the fixed process target",
    },
    HelpItem {
        key: "a / Alt+A (Info)",
        label: "Listening TCP + UDP / All endpoints",
    },
    HelpItem {
        key: "Type (Info)",
        label: "Filter directly, including /, a, r, Space",
    },
    HelpItem {
        key: "/ or Ctrl+F (global list)",
        label: "Edit Filter; opening Network focuses Filter",
    },
    HelpItem {
        key: "Enter/Esc (global filter)",
        label: "Enter applies; Esc clears and exits; Ctrl+U clears text",
    },
    HelpItem {
        key: "←/→, Backspace/Delete",
        label: "Move filter cursor / edit filter",
    },
    HelpItem {
        key: "Home/End (global filter)",
        label: "Move filter cursor to first / last",
    },
    HelpItem {
        key: "↑/↓, PgUp/PgDn",
        label: "Select row / page (wheel also selects)",
    },
    HelpItem {
        key: "Home/End",
        label: "First / last row",
    },
    HelpItem {
        key: "Enter (global list)",
        label: "Open Process Info for the process using this endpoint",
    },
    HelpItem {
        key: "Space (global), Enter (Info)",
        label: "Full endpoint and capture details",
    },
    HelpItem {
        key: "↑/↓, PgUp/PgDn (detail)",
        label: "Scroll; Home/End moves first/last",
    },
    HelpItem {
        key: "Ctrl+C",
        label: "Copy selected endpoint, tab-separated",
    },
    HelpItem {
        key: "Ctrl+U / r (global)",
        label: "Refresh explicitly; no automatic refresh",
    },
    HelpItem {
        key: "Esc (detail/list)",
        label: "Back / close; global results are retained",
    },
    HelpItem {
        key: "Tab / Ctrl+←/→ (Info)",
        label: "Focus tabs / change tab",
    },
    HelpItem {
        key: "Partial / --",
        label: "Missing tables / unverified owner",
    },
    HelpItem {
        key: "Recording / Log view",
        label: "Endpoints are never recorded",
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
    HelpSection {
        title: "Process Info",
        focus_hint: Some("fixed process target"),
        rows: PROCESS_INFO_ROWS,
    },
    HelpSection {
        title: "Mouse",
        focus_hint: None,
        rows: MOUSE_ROWS,
    },
    HelpSection {
        title: "Network endpoints",
        focus_hint: Some("on demand"),
        rows: NETWORK_ROWS,
    },
    HelpSection {
        title: "Find processes by file",
        focus_hint: Some("on demand"),
        rows: FILE_USERS_ROWS,
    },
    HelpSection {
        title: "Scheduling",
        focus_hint: Some("Process Info"),
        rows: SCHEDULING_ROWS,
    },
];

const RIGHT_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "MEM/GPU",
        focus_hint: None,
        rows: RAM_VRAM_ROWS,
    },
    HelpSection {
        title: "NW/DISK",
        focus_hint: None,
        rows: SYSTEM_ACTIVITY_ROWS,
    },
    HelpSection {
        title: "CPU",
        focus_hint: None,
        rows: CPU_ROWS,
    },
    HelpSection {
        title: "Tracking",
        focus_hint: Some("Processes focus"),
        rows: TRACKING_ROWS,
    },
    HelpSection {
        title: "Graph Workspace",
        focus_hint: Some("Graph focus"),
        rows: GRAPH_ROWS,
    },
    HelpSection {
        title: "Processes / Graph split",
        focus_hint: Some("Processes, Graph, or Samples focus"),
        rows: PROCESS_GRAPH_SPLIT_ROWS,
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
];

pub(crate) fn draw_help(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    if app.help_section_picker.is_some() {
        draw_section_picker(frame, area, app, theme);
        return;
    }
    let modal = help_modal(area);
    let layout = modal.render(
        frame,
        area,
        Text::from(help_lines(theme, modal.layout(area).content.width as usize)),
        app.help_scroll.offset,
        false,
        theme,
    );
    if !layout.footer.is_empty() {
        crate::ui::footer::register_shortcut_text(
            app,
            layout.footer,
            &ratatui::text::Text::from(Line::from(shortcut_spans(&HELP_SHORTCUT_ITEMS, theme))),
            ratatui::layout::Alignment::Left,
        );
        frame.render_widget(
            Paragraph::new(Line::from(shortcut_spans(&HELP_SHORTCUT_ITEMS, theme))),
            layout.footer,
        );
    }
}

#[cfg(test)]
pub(crate) fn help_area(area: Rect) -> Rect {
    help_modal(area).area(area)
}

pub(crate) fn help_page_size_for_screen(area: Rect) -> usize {
    help_modal(area).page_size(area)
}

pub(crate) fn help_scroll_max_for_page_size(page_size: usize, area: Rect) -> usize {
    help_modal(area).max_offset_for_page_size(page_size)
}

pub(crate) fn help_scrollbar_area(area: Rect, page_size: usize) -> Option<Rect> {
    help_modal(area).scrollbar_area(area, page_size)
}

pub(crate) fn section_titles() -> Vec<&'static str> {
    LEFT_SECTIONS
        .iter()
        .chain(RIGHT_SECTIONS)
        .map(|section| section.title)
        .collect()
}

pub(crate) fn section_index(title: &str) -> usize {
    section_titles()
        .iter()
        .position(|candidate| *candidate == title)
        .unwrap_or(0)
}

pub(crate) fn section_offset(index: usize, area: Rect) -> usize {
    let titles = section_titles();
    let title = titles.get(index).copied().unwrap_or(titles[0]);
    let theme = crate::ui::THEMES[0];
    let width = help_modal(area).layout(area).content.width as usize;
    help_lines(theme, width)
        .iter()
        .position(|line| {
            let headers: String = line
                .spans
                .iter()
                .filter(|span| {
                    span.style.fg == Some(theme.accent)
                        && span.style.add_modifier.contains(Modifier::BOLD)
                })
                .map(|span| span.content.as_ref())
                .collect();
            headers.contains(title)
        })
        .unwrap_or(0)
}

fn section_picker_modal() -> ScrollableModal {
    ScrollableModal::new("HELP SECTIONS", 44, section_titles().len() as u16, 1)
}

fn section_picker_offset(area: Rect, selected: usize) -> usize {
    let page = section_picker_modal().page_size(area);
    selected.saturating_sub(page.saturating_sub(1))
}

pub(crate) fn section_picker_row_at(
    area: Rect,
    selected: usize,
    x: u16,
    y: u16,
) -> Option<(usize, Rect)> {
    let content = section_picker_modal().layout(area).content;
    if !content.contains((x, y).into()) {
        return None;
    }
    let index = section_picker_offset(area, selected) + usize::from(y - content.y);
    (index < section_titles().len()).then_some((index, Rect::new(content.x, y, content.width, 1)))
}

pub(crate) fn section_picker_area(area: Rect) -> Rect {
    section_picker_modal().layout(area).area
}

fn draw_section_picker(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let selected = app.help_section_picker.unwrap_or(app.help_section);
    let hovered = app
        .shortcut_hovered
        .and_then(|row| section_picker_row_at(area, selected, row.x, row.y))
        .map(|(index, _)| index);
    let lines = section_titles()
        .iter()
        .enumerate()
        .map(|(index, title)| {
            let style = if index == selected || hovered == Some(index) {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.focus_surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            Line::from(Span::styled(
                format!("{} {title}", if index == selected { ">" } else { " " }),
                style,
            ))
        })
        .collect::<Vec<_>>();
    let layout = section_picker_modal().render(
        frame,
        area,
        Text::from(lines),
        section_picker_offset(area, selected),
        false,
        theme,
    );
    let footer = Text::from(Line::from(shortcut_spans(
        &[("↑/↓", "Select"), ("Enter", "Open"), ("Esc", "Back")],
        theme,
    )));
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &footer,
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(Paragraph::new(footer), layout.footer);
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

fn help_lines(theme: Theme, width: usize) -> Vec<Line<'static>> {
    let title = help_title();
    let mut lines = vec![
        Line::from(Span::styled(
            title,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(help_hint(), Style::default().fg(theme.muted))),
        Line::from(""),
    ];

    let left = render_column(LEFT_SECTIONS, theme);
    let right = render_column(RIGHT_SECTIONS, theme);
    let left_width = column_max_width(&left);
    if left_width + COLUMN_SEPARATOR.chars().count() + column_max_width(&right) > width {
        lines.extend(left.into_iter().map(|row| Line::from(row.spans)));
        lines.push(Line::default());
        lines.extend(right.into_iter().map(|row| Line::from(row.spans)));
        return wrap_lines(lines, width);
    }
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

    wrap_lines(lines, width)
}

fn help_hint() -> String {
    format!(
        "Footer: fits width. History: {}/{} normal/tracked. Scheme colors mark active items; T marks tracked.",
        format_integer(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY as u64),
        format_integer(TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY as u64)
    )
}

fn help_title() -> String {
    format!(
        "winproc-tui {} · Keyboard shortcuts",
        env!("CARGO_PKG_VERSION")
    )
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
        Span::styled(item.key, Style::default().fg(theme.key_hint)),
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
    let title_width = help_title()
        .chars()
        .count()
        .max(help_hint().chars().count())
        .max(shortcut_width(&HELP_SHORTCUT_ITEMS));
    let body_width = left + COLUMN_SEPARATOR.chars().count() + right;
    body_width.max(title_width) as u16
}

fn shortcut_width(items: &[(&str, &str)]) -> usize {
    items
        .iter()
        .enumerate()
        .map(|(index, (key, label))| {
            usize::from(index > 0) * 2 + key.chars().count() + 1 + label.chars().count()
        })
        .sum()
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

// Wrap styled spans explicitly so drawing, paging, and the scrollbar count the same rows.
fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for line in lines {
        let mut row = Line::default();
        for span in line.spans {
            for word in span.content.split_inclusive(' ') {
                let word_width = Span::raw(word).width();
                if row.width() > 0 && row.width() + word_width > width {
                    wrapped.push(row);
                    row = Line::default();
                }
                if word_width <= width {
                    row.spans.push(Span::styled(word.to_owned(), span.style));
                    continue;
                }
                for ch in word.chars() {
                    let cell = Span::styled(ch.to_string(), span.style);
                    if row.width() + cell.width() > width {
                        wrapped.push(row);
                        row = Line::default();
                    }
                    row.spans.push(cell);
                }
            }
        }
        wrapped.push(row);
    }
    wrapped
}

fn help_modal(area: Rect) -> ScrollableModal {
    let width = help_content_width().min(area.width.saturating_sub(4));
    ScrollableModal::new(
        "HELP",
        width,
        help_lines(crate::ui::THEMES[0], width as usize).len() as u16,
        FOOTER_HEIGHT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_help_item_remains_readable_at_supported_widths() {
        for width in [76, 116, 176] {
            let lines = help_lines(crate::ui::THEMES[0], width);
            assert!(lines.iter().all(|line| line.width() <= width));
            let text: String = lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .flat_map(|s| s.content.chars())
                .filter(|c| !c.is_whitespace())
                .collect();
            for section in LEFT_SECTIONS.iter().chain(RIGHT_SECTIONS) {
                for item in section.rows {
                    let label: String = item.label.chars().filter(|c| !c.is_whitespace()).collect();
                    assert!(text.contains(&label), "{} at width {width}", item.label);
                }
            }
        }
    }
}
