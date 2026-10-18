use super::*;

pub(crate) const TITLES: [&str; 4] = [
    "Investigate memory growth",
    "Find a process using a file",
    "Compare two times",
    "Record and inspect later",
];

const GUIDES: [&[&str]; 4] = [
    &[
        "Outcome: see how one process's private memory changes over time.",
        "Before starting: use Live with the process running. In LOG, File > Return to Live first. Stop Recording before changing tracking.",
        "1. Close Help. In Processes, select the process. Right-click its row > Track process (or Investigation > Add the selected name to Tracking List). Tracking List means process names: it retains longer history for all same-name instances, including future ones.",
        "2. Find the PrivBytes column; if hidden, View > Select columns. PrivBytes is private committed memory, not physical RAM usage. Right-click that process's PrivBytes value > Add to Graph Slot. If already added, choose Go to Graph Slot.",
        "3. Enable View > Graphs if the workspace is hidden. Let samples accumulate while exercising the process. A Graph belongs to this PID/lifetime; a restart needs a new Graph.",
        "4. Click an earlier point on the Graph, then a later one to read values and times. For an exact difference, use the Compare two times guide. Samples is the selected Graph's table of captured values; enable it in View when space permits.",
        "Result: a rising PrivBytes series shows growth during this interval; it does not by itself prove a leak. -- means unavailable, not zero.",
        "Return: Graph context menu > Jump to latest resumes following samples. Click Processes to continue. Reopen Help > Task guides whenever needed.",
    ],
    &[
        "Outcome: identify inspected processes with an open disk-file handle matching your query.",
        "Before starting: use Live or Recording; file search is unavailable in Log view. Have a filename or full path to search for.",
        "1. Close Help. Investigation > Find processes by file opens Query without starting a scan. Type the filename or path.",
        "2. Enter a filename or any part of its path in Query, then press Enter. Search always matches text anywhere in the full path.",
        "3. Read the capture status and coverage with the results. No matches means none in the inspected scope; inaccessible processes and mapped-only files can be missed.",
        "4. Select a matching result and use the displayed info action (Enter). Process Info opens its Files tab and lists that process's matching handles. Click a handle row (or Tab to the list), then use its detail action (Enter) for the full path and access attributes.",
        "Result: the captured process and matching file path are visible. This does not prove that the handle prevents deletion. If the process exits or closes the handle, retry the search.",
        "Return: Esc returns from detail to the list, then focuses the tab row, then closes Process Info back to the results. Esc cancels a running scan; when idle it returns. Use the displayed Esc close action when idle to return to the main workspace (F2 also returns to Processes).",
    ],
    &[
        "Outcome: read the exact B-A change between two captured times for a Graph.",
        "Before starting: add a metric to a Graph and collect at least two available samples, or open a recorded log. Right-click a Processes metric > Add to Graph Slot. Live, Recording, and Log view support comparison.",
        "1. Close Help. Enable View > Graphs if hidden, then select the Graph. On short terminals only the active Graph is shown; use its displayed navigation to select another.",
        "2. Click the earlier Graph point, then open its context menu > Set A. A marks the first comparison time. Left/Right moves between captured sample times; the footer's A/B range action is an alternative.",
        "3. Click the later point and open its context menu > Set B. B marks the second time. Read B-A in the Graph caption; Samples shows the A and B timestamps and values. View > Delta controls the additional Samples delta column.",
        "4. Enable View > Samples to see the active Graph's captured values and interval statistics. Enlarge the terminal if Samples is temporarily hidden. -- means this series lacks a value at that exact time; another time's value is never substituted.",
        "Result: B-A uses raw captured values even when the Graph shows MA5 (a five-sample moving average). A/B times are shared by all Graphs; each has its own values.",
        "Return: context menu > Clear A/B removes the comparison. Jump to latest follows new samples in Live/Recording, or selects the recorded end in LOG; it does not resume a paused display.",
    ],
    &[
        "Outcome: save process and system metrics, then inspect their recorded history.",
        "Before starting: use Live and track at least one process name (Processes row context menu > Track process). The Tracking List defines recording scope; filters and selected rows do not. No current matches is allowed: later matching processes are recorded.",
        "1. Close Help. File > Start Recording opens the path, interval, and fixed tracked names. Review the names and matching-process count; choose a new .log path and interval, then use the displayed Start action (Enter).",
        "2. Check REC in the header and let samples accumulate. Recording writes JSON Lines; intervals above 1s average available values. Tracking changes are locked until recording stops. DISPLAY PAUSED freezes only the display, not recording.",
        "3. File > Stop Recording opens confirmation. Use its Stop action (y); Enter/Esc/n continues recording. A confirmed stop flushes and closes the file and returns to LIVE.",
        "4. File > Open log opens the browser. Set its directory to the recording folder if needed, select the .log file, inspect the preview, and Open (Enter). If opening fails, dismiss the error to retry or choose another log.",
        "5. In LOG, right-click a recorded process metric > Add to Graph Slot. Use the Compare two times guide to inspect the history. Graphs start empty; the Processes table shows the final snapshot, not playback. Files and other live captures are not recorded.",
        "Result: Graphs and Samples inspect captured metrics across the saved interval; unavailable values remain gaps.",
        "Return: File > Return to Live resumes live investigation. This is separate from Quit. Recording is unavailable until you return to Live.",
    ],
];

fn footer_items(open: bool) -> Vec<(&'static str, &'static str)> {
    if open {
        vec![
            ("Esc", "Back"),
            ("F1", "Close"),
            ("r", "Reference"),
            ("↑/↓", "Scroll"),
            ("PageUp/PageDown", "Page"),
            ("Home/End", "Jump"),
        ]
    } else {
        vec![
            ("Esc/F1", "Close"),
            ("↑/↓", "Select"),
            ("Enter", "Open"),
            ("r", "Reference"),
        ]
    }
}
fn footer(theme: Theme, width: u16, open: bool) -> Text<'static> {
    let mut rows = Vec::new();
    let mut items = Vec::new();
    for item in footer_items(open) {
        let mut next = items.clone();
        next.push(item);
        if !items.is_empty() && Line::from(shortcut_spans(&next, theme)).width() > width as usize {
            rows.push(Line::from(shortcut_spans(&items, theme)));
            items.clear();
        }
        items.push(item);
    }
    rows.push(Line::from(shortcut_spans(&items, theme)));
    Text::from(rows)
}
fn lines(index: usize, open: bool, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if open {
        for paragraph in GUIDES[index] {
            rows.push(Line::from(Span::styled(
                *paragraph,
                Style::default().fg(theme.text),
            )));
            rows.push(Line::default());
        }
        rows.pop();
    } else {
        rows.push(Line::from("Choose a goal. Close Help to follow its steps."));
        rows.push(Line::default());
        for (i, title) in TITLES.iter().enumerate() {
            let style = if i == index {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.focus_surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            rows.push(Line::from(Span::styled(
                format!("{} {title}", if i == index { ">" } else { " " }),
                style,
            )));
        }
        rows.push(Line::default());
        rows.push(Line::from(
            "Reference: complete shortcuts, grouped by panel or dialog.",
        ));
        rows.push(Line::from(
            "F1 from your workspace opens the contextual reference.",
        ));
    }
    wrap_lines(rows, width)
}
pub(crate) fn modal(area: Rect, index: usize, open: bool) -> ScrollableModal {
    let width = 84.min(area.width.saturating_sub(4));
    ScrollableModal::new(
        if open { TITLES[index] } else { "Task guides" },
        width,
        lines(index, open, width as usize, crate::ui::THEMES[0]).len() as u16,
        footer(crate::ui::THEMES[0], width + 2, open).height() as u16,
    )
}
fn chooser_ranges(width: usize) -> Vec<std::ops::Range<usize>> {
    let mut start = wrap_lines(
        vec![
            Line::from("Choose a goal. Close Help to follow its steps."),
            Line::default(),
        ],
        width,
    )
    .len();
    TITLES
        .iter()
        .map(|title| {
            let end = start + wrap_lines(vec![Line::from(format!("> {title}"))], width).len();
            let range = start..end;
            start = end;
            range
        })
        .collect()
}

pub(crate) fn row_at(area: Rect, x: u16, y: u16) -> Option<usize> {
    let content = modal(area, 0, false).layout(area).content;
    if !content.contains((x, y).into()) {
        return None;
    }
    let row = usize::from(y - content.y);
    chooser_ranges(content.width as usize)
        .iter()
        .position(|range| range.contains(&row))
}
pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let (index, open) = app.task_guide.unwrap();
    let modal = modal(area, index, open);
    let mut content_lines = lines(
        index,
        open,
        modal.layout(area).content.width as usize,
        theme,
    );
    if !open && let Some(hovered) = app.shortcut_hovered.and_then(|r| row_at(area, r.x, r.y)) {
        for row in chooser_ranges(modal.layout(area).content.width as usize)[hovered].clone() {
            for span in &mut content_lines[row].spans {
                span.style = Style::default()
                    .fg(theme.text)
                    .bg(theme.focus_surface)
                    .add_modifier(Modifier::BOLD);
            }
        }
    }
    let layout = modal.render(
        frame,
        area,
        Text::from(content_lines),
        app.task_scroll.offset,
        false,
        theme,
    );
    let footer = footer(theme, layout.footer.width, open);
    crate::ui::footer::register_shortcut_text(
        app,
        layout.footer,
        &footer,
        ratatui::layout::Alignment::Left,
    );
    frame.render_widget(Paragraph::new(footer), layout.footer);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chooser_hit_rows_follow_wrapped_text() {
        for width in [30, 48, 80, 120] {
            let area = Rect::new(0, 0, width, 40);
            let content = modal(area, 0, false).layout(area).content;
            let rows = lines(0, false, content.width as usize, crate::ui::THEMES[0]);
            for (i, range) in chooser_ranges(content.width as usize).iter().enumerate() {
                let text: String = rows[range.clone()]
                    .iter()
                    .flat_map(|l| l.spans.iter())
                    .map(|s| s.content.as_ref())
                    .collect();
                assert!(text.contains(TITLES[i]));
                for row in range.clone() {
                    assert_eq!(row_at(area, content.x, content.y + row as u16), Some(i));
                }
            }
            assert_eq!(row_at(area, content.x, content.y), None);
        }
    }
    #[test]
    fn guide_wrapping_preserves_every_paragraph() {
        for width in [76, 84] {
            for (i, guide) in GUIDES.iter().enumerate() {
                let rows = lines(i, true, width, crate::ui::THEMES[0]);
                assert!(rows.iter().all(|l| l.width() <= width));
                let text: String = rows
                    .iter()
                    .flat_map(|l| l.spans.iter())
                    .map(|s| s.content.as_ref())
                    .collect();
                for paragraph in *guide {
                    assert!(text.contains(paragraph));
                }
            }
        }
    }
}
