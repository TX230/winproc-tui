use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::{
    AbComparison, AbComparisonPoint, App, AppActivity, FocusedPanel, GraphId, GraphSample,
    GraphSlot, ProcessInfoTab, state::ProcessInfoDialogTarget,
};
use crate::model::network::{NetworkEndpoint, NetworkOwner};

#[derive(Debug, Clone)]
pub(crate) enum ContextAction {
    ShowGraph(GraphSlot),
    RemoveGraph(GraphId),
    ProcessInfo(Box<ProcessInfoDialogTarget>, ProcessInfoTab),
    Track(String),
    Copy(String),
    SetPoint(GraphId, DateTime<Local>, char),
    ClearPoints,
    Latest(GraphId),
    DisplayMode(GraphId),
    FitAll,
    MinZero,
    Owner(Option<NetworkOwner>, bool),
    Endpoint(NetworkEndpoint, bool),
    Refresh(bool),
}

#[derive(Debug, Clone)]
pub(crate) struct ContextItem {
    pub(crate) label: String,
    pub(crate) action: ContextAction,
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMenu {
    pub(crate) title: String,
    pub(crate) anchor: Position,
    pub(crate) items: Vec<ContextItem>,
    pub(crate) selected: usize,
    pub(crate) hovered: Option<usize>,
}

impl ContextMenu {
    fn new(title: String, anchor: Position) -> Self {
        Self {
            title,
            anchor,
            items: Vec::new(),
            selected: 0,
            hovered: None,
        }
    }
    fn add(&mut self, label: impl Into<String>, action: ContextAction, enabled: bool) {
        self.items.push(ContextItem {
            label: label.into(),
            action,
            enabled,
        });
    }
}

impl App {
    pub(crate) fn process_context_menu(
        &self,
        target: ProcessInfoDialogTarget,
        source: Option<GraphSlot>,
        anchor: Position,
    ) -> ContextMenu {
        let title = format!(
            "PID {}{} · {}",
            target.identity.pid,
            source
                .as_ref()
                .map_or(String::new(), |s| format!(" · {}", s.metric_label())),
            target.identity.name
        );
        let mut menu = ContextMenu::new(title, anchor);
        if let Some(source) = source {
            let existing = self.graph_id_for_source(&source);
            let available =
                existing.is_some() || self.graph_entries.len() < super::state::GRAPH_LIMIT;
            menu.add(
                if available {
                    "Show graph"
                } else {
                    "Show graph (16 Graph limit)"
                },
                ContextAction::ShowGraph(source),
                available,
            );
            if let Some(id) = existing {
                menu.add("Remove graph", ContextAction::RemoveGraph(id), true);
            }
        }
        menu.add(
            "Process Info",
            ContextAction::ProcessInfo(Box::new(target.clone()), ProcessInfoTab::Metrics),
            true,
        );
        let live = self.activity() != AppActivity::LogView;
        menu.add(
            "Files",
            ContextAction::ProcessInfo(Box::new(target.clone()), ProcessInfoTab::Files),
            live,
        );
        menu.add(
            "Network",
            ContextAction::ProcessInfo(Box::new(target.clone()), ProcessInfoTab::Network),
            live,
        );
        menu.add(
            if self.is_tracked_process_name(&target.identity.name) {
                "Untrack process name"
            } else {
                "Track process name"
            },
            ContextAction::Track(target.identity.name.clone()),
            self.activity() == AppActivity::Live,
        );
        menu.add(
            "Copy row",
            ContextAction::Copy(super::clipboard::selected_process_row_text(
                &target.process,
                &self.process_columns,
            )),
            true,
        );
        menu.add(
            "Copy PID",
            ContextAction::Copy(target.identity.pid.to_string()),
            true,
        );
        menu.add(
            "Copy process name",
            ContextAction::Copy(target.identity.name),
            true,
        );
        menu
    }

    pub(crate) fn graph_context_menu(
        &self,
        id: GraphId,
        sample: Option<GraphSample>,
        anchor: Position,
    ) -> Option<ContextMenu> {
        let entry = self.graph_entry_by_id(id)?;
        let mut menu = ContextMenu::new(
            format!(
                "{} · {}",
                entry.source.metric_label(),
                sample.as_ref().map_or_else(
                    || "No sample".into(),
                    |s| s.captured_at.format("%H:%M:%S").to_string()
                )
            ),
            anchor,
        );
        if let Some(sample) = sample {
            menu.add(
                "Set A",
                ContextAction::SetPoint(id, sample.captured_at, 'A'),
                sample.value.is_some(),
            );
            menu.add(
                "Set B",
                ContextAction::SetPoint(id, sample.captured_at, 'B'),
                sample.value.is_some(),
            );
            menu.add(
                "Copy sample",
                ContextAction::Copy(format!(
                    "{}\t{}",
                    sample.captured_at.format("%H:%M:%S"),
                    super::clipboard::format_graph_sample_value(
                        sample.value,
                        entry.source.value_format()
                    )
                )),
                true,
            );
        }
        menu.add(
            "Clear A/B",
            ContextAction::ClearPoints,
            self.ab_comparison.is_some(),
        );
        menu.add("Latest", ContextAction::Latest(id), true);
        menu.add("Raw / MA5", ContextAction::DisplayMode(id), true);
        menu.add("Fit all", ContextAction::FitAll, true);
        menu.add("Min 0", ContextAction::MinZero, true);
        menu.add("Remove graph", ContextAction::RemoveGraph(id), true);
        Some(menu)
    }

    pub(crate) fn endpoint_context_menu(
        &self,
        entry: NetworkEndpoint,
        global: bool,
        anchor: Position,
    ) -> ContextMenu {
        let mut menu = ContextMenu::new(
            format!(
                "{} {} · PID {}",
                entry.key.protocol.label(),
                entry.key.local,
                entry.key.pid
            ),
            anchor,
        );
        menu.add(
            "Open Process Info",
            ContextAction::Owner(entry.owner.clone(), global),
            entry.owner.is_some() && self.network_view(global).pending.is_none(),
        );
        menu.add(
            "Endpoint details",
            ContextAction::Endpoint(entry.clone(), global),
            true,
        );
        menu.add(
            "Copy endpoint",
            ContextAction::Copy(entry.plain_text()),
            true,
        );
        menu.add("Refresh", ContextAction::Refresh(global), true);
        menu
    }

    pub(crate) fn open_context_menu(&mut self, menu: ContextMenu) {
        self.clear_source_cell_click();
        self.shortcut_map.borrow_mut().regions.clear();
        self.context_menu = Some(menu);
    }

    pub(crate) fn context_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        let Some(menu) = self.context_menu.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Esc => self.context_menu = None,
            KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Down => {
                menu.selected = (menu.selected + 1).min(menu.items.len().saturating_sub(1))
            }
            KeyCode::Home => menu.selected = 0,
            KeyCode::End => menu.selected = menu.items.len().saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_context_item()?,
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn context_menu_mouse(&mut self, mouse: MouseEvent, screen: Rect) {
        let Some(menu) = self.context_menu.as_ref() else {
            return;
        };
        let layout = crate::ui::context_menu::layout(screen, menu);
        let index = crate::ui::context_menu::item_at(screen, menu, mouse.column, mouse.row);
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(index) = index {
                self.context_menu.as_mut().unwrap().selected = index;
                if let Err(error) = self.activate_context_item() {
                    self.status = format!("Action failed: {error}");
                }
            } else if !layout.area.contains(Position::new(mouse.column, mouse.row)) {
                self.context_menu = None;
            }
        } else if mouse.kind == MouseEventKind::Moved {
            self.context_menu.as_mut().unwrap().hovered = index;
        } else if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let code = if mouse.kind == MouseEventKind::ScrollUp {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            let _ =
                self.context_menu_key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
        }
    }

    pub(crate) fn activate_context_item(&mut self) -> Result<()> {
        let Some(item) = self
            .context_menu
            .as_ref()
            .and_then(|menu| menu.items.get(menu.selected))
            .cloned()
        else {
            return Ok(());
        };
        if !item.enabled {
            return Ok(());
        }
        self.context_menu = None;
        match item.action {
            ContextAction::ShowGraph(source) => {
                if self.add_or_reveal_graph_source(source.clone(), FocusedPanel::Processes)
                    && let Some(id) = self.graph_id_for_source(&source)
                {
                    self.select_graph(id);
                    self.focused_panel = FocusedPanel::DetailsGraph;
                }
            }
            ContextAction::RemoveGraph(id) => {
                self.remove_graph(id);
            }
            ContextAction::ProcessInfo(target, tab) => {
                self.open_process_info_dialog(*target, tab)?
            }
            ContextAction::Track(name) => self.toggle_process_name_tracking(name),
            ContextAction::Copy(text) => {
                self.status = match super::clipboard::copy_text_to_clipboard(&text) {
                    Ok(()) => "Copied selection".into(),
                    Err(error) => format!("Copy failed: {error}"),
                }
            }
            ContextAction::SetPoint(id, captured_at, label) => {
                if self.select_graph(id) {
                    let point = AbComparisonPoint { captured_at };
                    let comparison = self
                        .ab_comparison
                        .get_or_insert(AbComparison { a: None, b: None });
                    if label == 'A' {
                        comparison.a = Some(point);
                    } else {
                        comparison.b = Some(point);
                    }
                    self.status = format!("{label} point set: {}", captured_at.format("%H:%M:%S"));
                }
            }
            ContextAction::ClearPoints => self.clear_ab_comparison_with_status(),
            ContextAction::Latest(id) => {
                if self.select_graph(id) {
                    self.enter_details_live_mode();
                }
            }
            ContextAction::DisplayMode(id) => {
                self.toggle_graph_display_mode(id);
            }
            ContextAction::FitAll => self.toggle_graph_all_samples(),
            ContextAction::MinZero => self.toggle_graph_y_axis_zero_min(),
            ContextAction::Owner(owner, global) => {
                if let Some(owner) = owner {
                    self.verify_network_owner(owner, global);
                }
            }
            ContextAction::Endpoint(entry, global) => {
                let view = self.network_view_mut(global);
                view.detail_entry = Some(entry);
                view.detail = true;
                view.editing = false;
                view.scroll.reset();
                if !global {
                    self.process_info_focus = super::ProcessInfoFocus::Content;
                }
            }
            ContextAction::Refresh(global) => self.refresh_network(global),
        }
        Ok(())
    }
}
