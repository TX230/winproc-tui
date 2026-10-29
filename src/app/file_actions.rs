use super::{App, AppActivity};
use crate::platform::shell::PathAction;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) struct FileActionView {
    pub(crate) path: PathBuf,
    pub(crate) selected: usize,
    pub(crate) message: String,
    pub(crate) failed: bool,
    pub(crate) scroll: crate::ui::widgets::scrollable_modal::ScrollableModalState,
    generation: u64,
    pending: bool,
}
impl FileActionView {
    pub(crate) fn busy(&self) -> bool {
        self.pending
    }
}
pub(crate) struct PendingFileAction {
    generation: u64,
    result: Receiver<Result<(), String>>,
}
impl App {
    pub(crate) fn open_file_actions(&mut self) {
        if self.activity() == AppActivity::LogView || self.process_info_filter_editing {
            return;
        }
        let Some(entry) = crate::ui::open_files::selected_entry(self) else {
            return;
        };
        let path = PathBuf::from(&entry.path);
        self.open_path_actions(path);
    }
    pub(crate) fn open_path_actions(&mut self, path: PathBuf) {
        self.file_action_generation = self.file_action_generation.wrapping_add(1);
        self.file_action = Some(FileActionView {
            path,
            selected: 0,
            message: String::new(),
            failed: false,
            scroll: Default::default(),
            generation: self.file_action_generation,
            pending: false,
        });
    }
    pub(crate) fn direct_file_action_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers != KeyModifiers::CONTROL || self.activity() == AppActivity::LogView {
            return false;
        }
        let action = match key.code {
            KeyCode::Char('e' | 'E') => 0,
            KeyCode::Char('o' | 'O') => 1,
            _ => return false,
        };
        let path = if self.show_process_info_dialog {
            if self.process_info_filter_editing
                || self.process_info_focus != super::ProcessInfoFocus::Content
            {
                return false;
            }
            match self.process_info_tab {
                super::ProcessInfoTab::Files => {
                    crate::ui::open_files::selected_entry(self).map(|entry| entry.path.clone())
                }
                super::ProcessInfoTab::Dlls if action == 0 => {
                    crate::ui::process_modules::selected_entry(self).map(|entry| entry.path.clone())
                }
                _ => return false,
            }
        } else if self.file_users.visible
            && self.file_users.focus == super::file_users::FileUsersFocus::Results
        {
            self.file_users
                .selected_entry()
                .map(|entry| entry.path.clone())
        } else {
            return false;
        };
        if let Some(path) = path {
            self.open_path_actions(PathBuf::from(path));
            self.launch_file_action(action);
        }
        true
    }
    pub(crate) fn launch_file_action(&mut self, selected: usize) {
        if self.activity() == AppActivity::LogView {
            return;
        }
        let Some(view) = self.file_action.as_mut() else {
            return;
        };
        if self.file_action_worker.is_some() || self.executable_location_worker.is_some() {
            if !view.pending {
                view.message =
                    "A previous file action is still pending. Try again after it completes.".into();
            }
            return;
        }
        view.selected = selected.min(1);
        let action = if view.selected == 0 {
            PathAction::Reveal
        } else {
            PathAction::Open
        };
        let path = view.path.clone();
        let (tx, rx) = mpsc::channel();
        view.failed = false;
        view.message = format!("{}: request pending...", action.label());
        match std::thread::Builder::new()
            .name("path-action".into())
            .spawn(move || {
                let _ = tx.send(crate::platform::shell::launch(&path, action));
            }) {
            Ok(_) => {
                view.pending = true;
                self.file_action_worker = Some(PendingFileAction {
                    generation: view.generation,
                    result: rx,
                });
            }
            Err(e) => {
                view.failed = true;
                view.message = format!("Cannot start launch worker: {e}");
            }
        }
    }
    pub(crate) fn poll_file_action(&mut self) -> bool {
        let Some(worker) = &self.file_action_worker else {
            return false;
        };
        let result = match worker.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                Err("Launch worker disconnected without a result".into())
            }
        };
        let generation = worker.generation;
        self.file_action_worker = None;
        let failed = result.is_err();
        let message = match result {
            Ok(()) => "Windows accepted the request.".into(),
            Err(e) => format!("Request failed: {e}"),
        };
        if let Some(view) = self
            .file_action
            .as_mut()
            .filter(|view| view.generation == generation)
        {
            view.pending = false;
            view.failed = failed;
            view.message = message;
        } else {
            self.status = format!("Earlier file action: {message}");
        }
        true
    }
    pub(crate) fn file_action_key(&mut self, key: KeyEvent) {
        let Some(view) = self.file_action.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.file_action = None;
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab if !view.busy() => {
                view.selected ^= 1;
                view.scroll.scroll_home();
            }
            KeyCode::Enter if !view.busy() => {
                let selected = view.selected;
                self.launch_file_action(selected);
            }
            KeyCode::Char('e' | 'E') if key.modifiers == KeyModifiers::CONTROL => {
                self.launch_file_action(0)
            }
            KeyCode::Char('o' | 'O') if key.modifiers == KeyModifiers::CONTROL => {
                self.launch_file_action(1)
            }
            KeyCode::Home | KeyCode::End => self.scroll_file_actions(key.code),
            _ => {}
        }
    }
}

#[cfg(test)]
impl App {
    pub(crate) fn inject_pending_file_action(&mut self) -> mpsc::Sender<Result<(), String>> {
        let view = self.file_action.as_mut().unwrap();
        view.pending = true;
        let (tx, rx) = mpsc::channel();
        self.file_action_worker = Some(PendingFileAction {
            generation: view.generation,
            result: rx,
        });
        tx
    }
}
