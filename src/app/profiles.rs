use crate::{
    app::{App, AppActivity, state::PendingTrackedListSwitch},
    config::{InvestigationStartup, InvestigationStateConfig, SavedInvestigationProfile},
    ui::widgets::scrollable_modal::ScrollableModalState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileNameInputPurpose {
    SaveAs,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingInvestigationProfileLoad {
    pub(crate) profile: SavedInvestigationProfile,
    pub(crate) tracking_switch: PendingTrackedListSwitch,
    pub(crate) unsaved_changes: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum InvestigationProfilesView {
    Browse,
    Startup {
        selected: InvestigationStartup,
    },
    NameInput {
        purpose: ProfileNameInputPurpose,
        draft: String,
        cursor: usize,
        error: Option<String>,
    },
    ConfirmDelete {
        name: String,
    },
    ConfirmLoad {
        pending: Box<PendingInvestigationProfileLoad>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct InvestigationProfilesDialog {
    pub(crate) index: usize,
    pub(crate) scroll: ScrollableModalState,
    pub(crate) view: InvestigationProfilesView,
}

impl App {
    pub(crate) fn open_investigation_profiles(&mut self) {
        let index = self
            .active_investigation_profile
            .as_deref()
            .and_then(|active| {
                self.runtime
                    .saved_investigation_profiles
                    .iter()
                    .position(|profile| profile.name.eq_ignore_ascii_case(active))
            })
            .unwrap_or(0)
            .min(
                self.runtime
                    .saved_investigation_profiles
                    .len()
                    .saturating_sub(1),
            );
        self.investigation_profiles_dialog = Some(InvestigationProfilesDialog {
            index,
            scroll: ScrollableModalState {
                page_size: 8,
                ..ScrollableModalState::default()
            },
            view: InvestigationProfilesView::Browse,
        });
        self.ensure_investigation_profile_selection_visible();
        self.status = "Investigation Profiles".to_string();
    }

    pub(crate) fn close_investigation_profiles(&mut self) {
        self.investigation_profiles_dialog = None;
        self.status = "Ready".to_string();
    }

    pub(crate) fn investigation_profiles_view(&self) -> Option<&InvestigationProfilesView> {
        self.investigation_profiles_dialog
            .as_ref()
            .map(|dialog| &dialog.view)
    }

    pub(crate) fn investigation_profiles_index(&self) -> usize {
        self.investigation_profiles_dialog
            .as_ref()
            .map(|dialog| dialog.index)
            .unwrap_or(0)
    }

    pub(crate) fn investigation_profiles_scroll_offset(&self) -> usize {
        self.investigation_profiles_dialog
            .as_ref()
            .map(|dialog| dialog.scroll.offset)
            .unwrap_or(0)
    }

    pub(crate) fn investigation_profiles_entry_count(&self) -> usize {
        self.runtime.saved_investigation_profiles.len()
    }

    pub(crate) fn open_investigation_startup(&mut self) {
        self.investigation_profiles_dialog = Some(InvestigationProfilesDialog {
            index: 0,
            scroll: ScrollableModalState::default(),
            view: InvestigationProfilesView::Startup {
                selected: self.runtime.investigation_startup,
            },
        });
        self.status = "Startup behavior".to_string();
    }

    pub(crate) fn select_next_investigation_startup(&mut self) {
        if let Some(InvestigationProfilesView::Startup { selected }) = self
            .investigation_profiles_dialog
            .as_mut()
            .map(|dialog| &mut dialog.view)
        {
            *selected = selected.next();
        }
    }

    pub(crate) fn select_previous_investigation_startup(&mut self) {
        if let Some(InvestigationProfilesView::Startup { selected }) = self
            .investigation_profiles_dialog
            .as_mut()
            .map(|dialog| &mut dialog.view)
        {
            *selected = selected.previous();
        }
    }

    pub(crate) fn select_investigation_startup(&mut self, startup: InvestigationStartup) {
        if let Some(InvestigationProfilesView::Startup { selected }) = self
            .investigation_profiles_dialog
            .as_mut()
            .map(|dialog| &mut dialog.view)
        {
            *selected = startup;
        }
    }

    pub(crate) fn apply_selected_investigation_startup(&mut self) {
        let Some(InvestigationProfilesView::Startup { selected }) =
            self.investigation_profiles_view().cloned()
        else {
            return;
        };
        if self.set_investigation_startup(selected) {
            self.investigation_profiles_dialog = None;
            self.status = format!("Startup behavior: {}", selected.label());
        }
    }

    fn set_investigation_startup(&mut self, startup: InvestigationStartup) -> bool {
        let previous = self.runtime.investigation_startup;
        self.runtime.investigation_startup = startup;
        if self.persist_investigation_profile_changes() {
            true
        } else {
            self.runtime.investigation_startup = previous;
            false
        }
    }

    pub(crate) fn set_investigation_profiles_page_size(&mut self, page_size: usize) {
        let total = self.investigation_profiles_entry_count();
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.scroll.set_page_size(page_size.max(1), total.max(1));
        }
        self.ensure_investigation_profile_selection_visible();
    }

    pub(crate) fn move_investigation_profile_selection_up(&mut self, amount: usize) {
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.index = dialog.index.saturating_sub(amount);
        }
        self.ensure_investigation_profile_selection_visible();
    }

    pub(crate) fn move_investigation_profile_selection_down(&mut self, amount: usize) {
        let last = self.investigation_profiles_entry_count().saturating_sub(1);
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.index = dialog.index.saturating_add(amount).min(last);
        }
        self.ensure_investigation_profile_selection_visible();
    }

    pub(crate) fn move_investigation_profile_selection_home(&mut self) {
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.index = 0;
        }
        self.ensure_investigation_profile_selection_visible();
    }

    pub(crate) fn move_investigation_profile_selection_end(&mut self) {
        let last = self.investigation_profiles_entry_count().saturating_sub(1);
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.index = last;
        }
        self.ensure_investigation_profile_selection_visible();
    }

    pub(crate) fn select_investigation_profile_index(&mut self, index: usize) {
        let last = self.investigation_profiles_entry_count().saturating_sub(1);
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.index = index.min(last);
        }
        self.ensure_investigation_profile_selection_visible();
    }

    fn ensure_investigation_profile_selection_visible(&mut self) {
        let total = self.investigation_profiles_entry_count();
        let Some(dialog) = self.investigation_profiles_dialog.as_mut() else {
            return;
        };
        dialog.index = dialog.index.min(total.saturating_sub(1));
        dialog.scroll.ensure_visible(dialog.index, total.max(1));
    }

    pub(crate) fn selected_investigation_profile(&self) -> Option<&SavedInvestigationProfile> {
        self.runtime
            .saved_investigation_profiles
            .get(self.investigation_profiles_index())
    }

    pub(crate) fn active_investigation_profile_dirty(&self) -> bool {
        let Some(active) = self.active_investigation_profile.as_deref() else {
            return true;
        };
        let Some(saved) = self
            .runtime
            .saved_investigation_profiles
            .iter()
            .find(|profile| profile.name.eq_ignore_ascii_case(active))
        else {
            return true;
        };
        !profiles_equivalent(
            saved,
            &self.capture_investigation_profile(saved.name.clone()),
        )
    }

    pub(crate) fn begin_save_investigation_profile_as(&mut self) {
        if !self.profile_workspace_available() {
            return;
        }
        self.investigation_profiles_dialog = Some(InvestigationProfilesDialog {
            index: 0,
            scroll: ScrollableModalState::default(),
            view: InvestigationProfilesView::NameInput {
                purpose: ProfileNameInputPurpose::SaveAs,
                draft: String::new(),
                cursor: 0,
                error: None,
            },
        });
        self.status = "Save Investigation Profile As".to_string();
    }

    pub(crate) fn save_active_investigation_profile(&mut self) {
        if !self.profile_workspace_available() {
            return;
        }
        let Some(active) = self.active_investigation_profile.clone() else {
            self.begin_save_investigation_profile_as();
            return;
        };
        let Some(index) = self
            .runtime
            .saved_investigation_profiles
            .iter()
            .position(|profile| profile.name.eq_ignore_ascii_case(&active))
        else {
            self.active_investigation_profile = None;
            self.begin_save_investigation_profile_as();
            return;
        };

        let previous = self.runtime.saved_investigation_profiles[index].clone();
        self.runtime.saved_investigation_profiles[index] =
            self.capture_investigation_profile(previous.name.clone());
        if self.persist_investigation_profile_changes() {
            self.status = format!("Saved Investigation Profile: {}", previous.name);
        } else {
            self.runtime.saved_investigation_profiles[index] = previous;
        }
    }

    pub(crate) fn cancel_investigation_profile_subdialog(&mut self) {
        let close_dialog = matches!(
            self.investigation_profiles_view(),
            Some(InvestigationProfilesView::Startup { .. })
                | Some(InvestigationProfilesView::NameInput { .. })
        );
        if close_dialog {
            self.close_investigation_profiles();
            return;
        }
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.view = InvestigationProfilesView::Browse;
            dialog.scroll.offset = 0;
        }
        self.ensure_investigation_profile_selection_visible();
        self.status = "Investigation Profiles".to_string();
    }

    pub(crate) fn push_investigation_profile_name_char(&mut self, ch: char) {
        if let Some((draft, cursor, error)) = self.profile_name_input_mut() {
            *error = None;
            *cursor = (*cursor).min(draft.len());
            draft.insert(*cursor, ch);
            *cursor += ch.len_utf8();
        }
    }

    pub(crate) fn pop_investigation_profile_name_char(&mut self) {
        let Some((draft, cursor, error)) = self.profile_name_input_mut() else {
            return;
        };
        if *cursor == 0 {
            return;
        }
        *error = None;
        let previous = draft[..*cursor]
            .char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0);
        draft.drain(previous..*cursor);
        *cursor = previous;
    }

    pub(crate) fn delete_investigation_profile_name_char(&mut self) {
        let Some((draft, cursor, error)) = self.profile_name_input_mut() else {
            return;
        };
        if *cursor >= draft.len() {
            return;
        }
        *error = None;
        let next = draft[*cursor..]
            .chars()
            .next()
            .map(|ch| *cursor + ch.len_utf8())
            .unwrap_or(draft.len());
        draft.drain(*cursor..next);
    }

    pub(crate) fn move_investigation_profile_name_cursor_left(&mut self) {
        let Some((draft, cursor, _)) = self.profile_name_input_mut() else {
            return;
        };
        if *cursor > 0 {
            *cursor = draft[..*cursor]
                .char_indices()
                .last()
                .map(|(index, _)| index)
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_investigation_profile_name_cursor_right(&mut self) {
        let Some((draft, cursor, _)) = self.profile_name_input_mut() else {
            return;
        };
        if *cursor < draft.len() {
            *cursor = draft[*cursor..]
                .chars()
                .next()
                .map(|ch| *cursor + ch.len_utf8())
                .unwrap_or(draft.len());
        }
    }

    pub(crate) fn move_investigation_profile_name_cursor_home(&mut self) {
        if let Some((_, cursor, _)) = self.profile_name_input_mut() {
            *cursor = 0;
        }
    }

    pub(crate) fn move_investigation_profile_name_cursor_end(&mut self) {
        if let Some((draft, cursor, _)) = self.profile_name_input_mut() {
            *cursor = draft.len();
        }
    }

    fn profile_name_input_mut(&mut self) -> Option<(&mut String, &mut usize, &mut Option<String>)> {
        let dialog = self.investigation_profiles_dialog.as_mut()?;
        let InvestigationProfilesView::NameInput {
            draft,
            cursor,
            error,
            ..
        } = &mut dialog.view
        else {
            return None;
        };
        Some((draft, cursor, error))
    }

    pub(crate) fn commit_investigation_profile_name_input(&mut self) {
        let Some((purpose, name)) =
            self.investigation_profiles_dialog
                .as_ref()
                .and_then(|dialog| {
                    let InvestigationProfilesView::NameInput { purpose, draft, .. } = &dialog.view
                    else {
                        return None;
                    };
                    Some((*purpose, draft.trim().to_string()))
                })
        else {
            return;
        };
        if name.is_empty() {
            self.set_investigation_profile_name_error("Enter a profile name.");
            return;
        }

        match purpose {
            ProfileNameInputPurpose::SaveAs => self.save_investigation_profile_as(name),
        }
    }

    fn set_investigation_profile_name_error(&mut self, message: &str) {
        if let Some((_, _, error)) = self.profile_name_input_mut() {
            *error = Some(message.to_string());
        }
        self.status = message.to_string();
    }

    fn save_investigation_profile_as(&mut self, name: String) {
        if self
            .runtime
            .saved_investigation_profiles
            .iter()
            .any(|profile| profile.name.eq_ignore_ascii_case(&name))
        {
            self.set_investigation_profile_name_error(
                "An Investigation Profile with that name already exists.",
            );
            return;
        }
        let previous_active = self.active_investigation_profile.clone();
        self.runtime
            .saved_investigation_profiles
            .push(self.capture_investigation_profile(name.clone()));
        self.active_investigation_profile = Some(name.clone());
        if self.persist_investigation_profile_changes() {
            self.investigation_profiles_dialog = None;
            self.status = format!("Saved Investigation Profile: {name}");
        } else {
            self.runtime.saved_investigation_profiles.pop();
            self.active_investigation_profile = previous_active;
            self.set_investigation_profile_name_error("Save failed.");
        }
    }

    pub(crate) fn request_delete_selected_investigation_profile(&mut self) {
        let Some(profile) = self.selected_investigation_profile() else {
            self.status = "No Investigation Profile selected".to_string();
            return;
        };
        let name = profile.name.clone();
        if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
            dialog.view = InvestigationProfilesView::ConfirmDelete { name };
        }
    }

    pub(crate) fn confirm_investigation_profile_action(&mut self) {
        let view = self
            .investigation_profiles_dialog
            .as_ref()
            .map(|dialog| dialog.view.clone());
        match view {
            Some(InvestigationProfilesView::ConfirmDelete { name }) => {
                self.delete_investigation_profile(name)
            }
            Some(InvestigationProfilesView::ConfirmLoad { pending }) => {
                self.apply_investigation_profile_load(*pending)
            }
            _ => {}
        }
    }

    fn delete_investigation_profile(&mut self, name: String) {
        let previous_profiles = self.runtime.saved_investigation_profiles.clone();
        let previous_active = self.active_investigation_profile.clone();
        self.runtime
            .saved_investigation_profiles
            .retain(|profile| !profile.name.eq_ignore_ascii_case(&name));
        if self
            .active_investigation_profile
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(&name))
        {
            self.active_investigation_profile = None;
        }
        if self.persist_investigation_profile_changes() {
            let last = self.investigation_profiles_entry_count().saturating_sub(1);
            if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
                dialog.index = dialog.index.min(last);
                dialog.view = InvestigationProfilesView::Browse;
            }
            self.ensure_investigation_profile_selection_visible();
            self.status = format!("Deleted Investigation Profile: {name}");
        } else {
            self.runtime.saved_investigation_profiles = previous_profiles;
            self.active_investigation_profile = previous_active;
        }
    }

    pub(crate) fn load_selected_investigation_profile(&mut self) {
        if !self.profile_workspace_available() {
            return;
        }
        let Some(profile) = self.selected_investigation_profile().cloned() else {
            self.status = "No Investigation Profile selected".to_string();
            return;
        };
        let tracking_switch = self.prepare_tracked_list_switch(profile.tracked_names.clone());
        let current: std::collections::HashSet<_> = self
            .watch_list
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let target: std::collections::HashSet<_> = profile
            .tracked_names
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let unsaved_changes = current != target
            && self.active_investigation_profile_dirty()
            && (!current.is_empty() || self.active_investigation_profile.is_some());
        let pending = PendingInvestigationProfileLoad {
            profile,
            tracking_switch,
            unsaved_changes,
        };
        if pending.unsaved_changes || pending.tracking_switch.discarded_sample_count > 0 {
            if let Some(dialog) = self.investigation_profiles_dialog.as_mut() {
                dialog.view = InvestigationProfilesView::ConfirmLoad {
                    pending: Box::new(pending),
                };
            }
            self.status = "Confirm replacing the current Tracking List".to_string();
        } else {
            self.apply_investigation_profile_load(pending);
        }
    }

    fn profile_workspace_available(&mut self) -> bool {
        match self.activity() {
            AppActivity::Live => true,
            AppActivity::Recording => {
                self.status =
                    "Investigation Profiles cannot save or load during Recording".to_string();
                false
            }
            AppActivity::LogView => {
                self.status = "Investigation Profiles cannot save or load in Log view".to_string();
                false
            }
        }
    }

    fn apply_investigation_profile_load(&mut self, pending: PendingInvestigationProfileLoad) {
        if !self.profile_workspace_available() {
            return;
        }
        let PendingInvestigationProfileLoad {
            profile,
            tracking_switch,
            ..
        } = pending;
        self.apply_tracked_list_switch(tracking_switch);
        self.active_investigation_profile = Some(profile.name.clone());
        if !self.persist_investigation_profile_changes() {
            return;
        }
        self.investigation_profiles_dialog = None;
        self.status = format!("Loaded Investigation Profile: {}", profile.name);
    }

    fn capture_investigation_profile(&self, name: String) -> SavedInvestigationProfile {
        SavedInvestigationProfile {
            name,
            investigation: self.capture_investigation_state(),
        }
    }

    pub(crate) fn capture_investigation_state(&self) -> InvestigationStateConfig {
        InvestigationStateConfig {
            tracked_names: self.watch_list.clone(),
            ..InvestigationStateConfig::default()
        }
    }

    fn persist_investigation_profile_changes(&mut self) -> bool {
        let Some(path) = self.runtime.config_path.clone() else {
            return true;
        };
        match crate::config::write_app_config(&path, self) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Failed to save Investigation Profiles: {error}");
                false
            }
        }
    }
}

fn profiles_equivalent(
    left: &SavedInvestigationProfile,
    right: &SavedInvestigationProfile,
) -> bool {
    left.name.eq_ignore_ascii_case(&right.name)
        && strings_equal_case_insensitively(&left.tracked_names, &right.tracked_names)
}

fn strings_equal_case_insensitively(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}
