use super::{App, AppActivity};
use crate::platform::shell::PathAction;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) struct PendingFileAction {
    pub(crate) path: PathBuf,
    action: PathAction,
    result: Receiver<Result<(), String>>,
}
impl App {
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
            self.launch_file_action(
                PathBuf::from(path),
                if action == 0 {
                    PathAction::Reveal
                } else {
                    PathAction::Open
                },
            );
        }
        true
    }
    pub(crate) fn launch_file_action(&mut self, path: PathBuf, action: PathAction) {
        if self.activity() == AppActivity::LogView {
            return;
        }
        if self.file_action_worker.is_some() || self.executable_location_worker.is_some() {
            self.status = "A shell action is still pending; wait for it to complete".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let target = path.clone();
        self.status = format!("{}: request pending...", action.label());
        match std::thread::Builder::new()
            .name("path-action".into())
            .spawn(move || {
                let _ = tx.send(crate::platform::shell::launch(&target, action));
            }) {
            Ok(_) => {
                self.file_action_worker = Some(PendingFileAction {
                    path,
                    action,
                    result: rx,
                })
            }
            Err(error) => self.status = format!("Cannot start launch worker: {error}"),
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
        let message = match result {
            Ok(()) => "Windows accepted the request".into(),
            Err(error) => format!("Request failed: {error}"),
        };
        self.status = format!(
            "{}: {message} — {}",
            worker.action.label(),
            worker.path.display()
        );
        self.file_action_worker = None;
        true
    }
    #[cfg(test)]
    pub(crate) fn inject_pending_file_action(
        &mut self,
        path: PathBuf,
    ) -> mpsc::Sender<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        self.file_action_worker = Some(PendingFileAction {
            path,
            action: PathAction::Open,
            result: rx,
        });
        tx
    }
}
