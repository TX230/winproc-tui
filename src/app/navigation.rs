use crate::app::App;

impl App {
    pub(crate) fn move_selection_up(&mut self, amount: usize) {
        self.move_selection_up_with_mode(amount, SelectionMoveMode::Single);
    }

    pub(crate) fn move_selection_down(&mut self, amount: usize) {
        self.move_selection_down_with_mode(amount, SelectionMoveMode::Single);
    }

    pub(crate) fn move_selection_cursor_up(&mut self, amount: usize) {
        self.move_selection_up_with_mode(amount, SelectionMoveMode::CursorOnly);
    }

    pub(crate) fn move_selection_cursor_down(&mut self, amount: usize) {
        self.move_selection_down_with_mode(amount, SelectionMoveMode::CursorOnly);
    }

    pub(crate) fn extend_process_selection_up(&mut self, amount: usize) {
        self.move_selection_up_with_mode(amount, SelectionMoveMode::Extend);
    }

    pub(crate) fn extend_process_selection_down(&mut self, amount: usize) {
        self.move_selection_down_with_mode(amount, SelectionMoveMode::Extend);
    }

    fn move_selection_up_with_mode(&mut self, amount: usize, mode: SelectionMoveMode) {
        if self.visible_process_count() == 0 {
            return;
        }

        let current = self.process_table_state.selected().unwrap_or(0);
        let next = current.saturating_sub(amount);
        self.select_process_index_with_mode(next, mode);
        self.ensure_selected_row_visible();
    }

    fn move_selection_down_with_mode(&mut self, amount: usize, mode: SelectionMoveMode) {
        let visible_count = self.visible_process_count();
        if visible_count == 0 {
            return;
        }

        let last_index = visible_count.saturating_sub(1);
        let current = self.process_table_state.selected().unwrap_or(0);
        let next = current.saturating_add(amount).min(last_index);
        self.select_process_index_with_mode(next, mode);
        self.ensure_selected_row_visible();
    }

    pub(crate) fn select_first_row(&mut self) {
        if self.visible_process_count() == 0 {
            return;
        }
        self.select_process_index(self.first_selectable_process_index().unwrap_or(0));
        *self.process_table_state.offset_mut() = 0;
    }

    pub(crate) fn select_last_row(&mut self) {
        let visible_count = self.visible_process_count();
        if visible_count == 0 {
            return;
        }
        let last_index = visible_count.saturating_sub(1);
        self.select_process_index(last_index);
        self.ensure_selected_row_visible();
    }

    pub(crate) fn select_process_index(&mut self, index: usize) {
        self.select_process_index_with_mode(index, SelectionMoveMode::Single);
    }

    fn select_process_index_with_mode(&mut self, index: usize, mode: SelectionMoveMode) {
        let anchor = match mode {
            SelectionMoveMode::Extend => self
                .process_selection_anchor
                .clone()
                .or_else(|| self.selected_live_process_identity()),
            SelectionMoveMode::Single | SelectionMoveMode::CursorOnly => None,
        };
        let identity = self.visible_process_identity_at(index);
        self.process_table_state.select(Some(index));
        self.selected_process_identity = identity;
        self.hold_process_order_during_navigation();
        self.select_process_details_target();
        self.ensure_selected_process_info();
        match mode {
            SelectionMoveMode::Single => {
                self.clear_process_multi_selection();
                self.process_selection_anchor = None;
            }
            SelectionMoveMode::CursorOnly => {}
            SelectionMoveMode::Extend => {
                self.apply_process_selection_range(anchor, index);
            }
        }
    }

    pub(crate) fn select_process_identity(
        &mut self,
        identity: &crate::model::ProcessIdentity,
    ) -> bool {
        let Some(index) = self.visible_process_position(identity) else {
            self.status = "Graph item is not visible in Processes".to_string();
            return false;
        };
        self.select_process_index(index);
        self.ensure_selected_row_visible();
        self.status = format!("Process selected: {}", identity.name);
        true
    }

    fn apply_process_selection_range(
        &mut self,
        anchor: Option<crate::model::ProcessIdentity>,
        cursor_index: usize,
    ) {
        let Some(anchor) = anchor else {
            self.clear_process_multi_selection();
            return;
        };
        let Some(anchor_index) = self.visible_process_position(&anchor) else {
            self.clear_process_multi_selection();
            self.process_selection_anchor = None;
            return;
        };

        self.process_selection_anchor = Some(anchor);
        self.selected_process_identities.clear();
        let start = anchor_index.min(cursor_index);
        let end = anchor_index.max(cursor_index);
        for index in start..=end {
            if let Some(identity) = self.live_process_identity_at(index) {
                self.selected_process_identities.insert(identity);
            }
        }
        let count = self.selected_process_identities.len();
        self.status = if count == 0 {
            "No live process rows selected".to_string()
        } else {
            format!("Selected {count} live process rows")
        };
    }

    fn live_process_identity_at(&self, index: usize) -> Option<crate::model::ProcessIdentity> {
        self.visible_process_entries
            .get(index)
            .and_then(|entry| self.live_identity_for_visible_entry(entry))
    }

    pub(crate) fn ensure_selected_row_visible(&mut self) {
        let Some(selected) = self.process_table_state.selected() else {
            return;
        };
        let page_size = self.process_page_size.max(1);
        let total_rows = self.visible_process_count();
        let max_offset = total_rows.saturating_sub(page_size);
        let offset = self.process_table_state.offset();
        let visible_end = offset.saturating_add(page_size.saturating_sub(1));

        let next_offset = if selected < offset {
            selected
        } else if selected > visible_end {
            selected.saturating_sub(page_size.saturating_sub(1))
        } else {
            offset
        };

        *self.process_table_state.offset_mut() = next_offset.min(max_offset);
    }

    pub(crate) fn clamp_process_table_state(&mut self) {
        let visible_count = self.visible_process_count();
        if visible_count == 0 {
            self.process_table_state.select(None);
            self.selected_process_identity = None;
            self.process_selection_anchor = None;
            self.clear_process_multi_selection();
            *self.process_table_state.offset_mut() = 0;
            return;
        }

        let last_index = visible_count.saturating_sub(1);
        let (selected, selected_identity) = {
            let selected = self
                .selected_process_identity
                .as_ref()
                .and_then(|identity| self.visible_process_position(identity))
                .or_else(|| {
                    self.process_table_state
                        .selected()
                        .map(|selected| selected.min(last_index))
                })
                .unwrap_or(0);
            let identity = self.visible_process_identity_at(selected);
            let selected = if identity.is_none() {
                self.first_selectable_process_index().unwrap_or(selected)
            } else {
                selected
            };
            let identity = self.visible_process_identity_at(selected);
            (selected, identity)
        };
        self.process_table_state.select(Some(selected));
        self.selected_process_identity = selected_identity;
        self.prune_process_selection_to_visible_live_rows();
        let max_offset = self
            .visible_process_count()
            .saturating_sub(self.process_page_size.max(1));
        *self.process_table_state.offset_mut() = self.process_table_state.offset().min(max_offset);
        self.ensure_selected_row_visible();
        self.ensure_selected_process_info();
    }
}

#[derive(Debug, Clone, Copy)]
enum SelectionMoveMode {
    Single,
    CursorOnly,
    Extend,
}
