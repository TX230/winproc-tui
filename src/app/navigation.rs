use crate::app::App;

impl App {
    pub(crate) fn move_selection_up(&mut self, amount: usize) {
        if self.visible_process_count() == 0 {
            return;
        }

        let current = self.process_table_state.selected().unwrap_or(0);
        let next = current.saturating_sub(amount);
        self.select_process_index(next);
        self.ensure_selected_row_visible();
    }

    pub(crate) fn move_selection_down(&mut self, amount: usize) {
        let visible_count = self.visible_process_count();
        if visible_count == 0 {
            return;
        }

        let last_index = visible_count.saturating_sub(1);
        let current = self.process_table_state.selected().unwrap_or(0);
        let next = current.saturating_add(amount).min(last_index);
        self.select_process_index(next);
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
        let identity = self.visible_process_identity_at(index);
        self.process_table_state.select(Some(index));
        self.selected_process_identity = identity;
        self.select_process_details_target();
        self.ensure_selected_process_info();
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
        let max_offset = self
            .visible_process_count()
            .saturating_sub(self.process_page_size.max(1));
        *self.process_table_state.offset_mut() = self.process_table_state.offset().min(max_offset);
        self.ensure_selected_row_visible();
        self.ensure_selected_process_info();
    }
}
