use super::{App, AppActivity, ProcessLifecycle, state::ProcessInfoDialogTarget};
use crate::model::ProcessIdentity;
use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use std::sync::mpsc::{self, TryRecvError};

pub(crate) struct LocationView {
    pub(crate) target: ProcessIdentity,
    pub(crate) message: String,
    pub(crate) failed: bool,
    pub(crate) offset: usize,
}
impl App {
    pub(crate) fn open_executable_location(&mut self, target: ProcessInfoDialogTarget) {
        if self.activity() == AppActivity::LogView || target.lifecycle != ProcessLifecycle::Live {
            self.status = "Executable location is unavailable for Ghost Rows and Log view".into();
            return;
        }
        if self.file_action_worker.is_some() || self.executable_location_worker.is_some() {
            self.status = "A shell action is still pending; wait for it to complete".into();
            return;
        }
        self.executable_location = Some(LocationView {
            target: target.identity.clone(),
            message: "Resolving executable location...".into(),
            failed: false,
            offset: 0,
        });
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("executable-location".into())
            .spawn(move || {
                let _ = tx.send(crate::platform::executable_location::reveal(
                    &target.identity,
                ));
            }) {
            Ok(_) => self.executable_location_worker = Some(rx),
            Err(error) => {
                let view = self.executable_location.as_mut().unwrap();
                view.failed = true;
                view.message = format!("Cannot start worker: {error}");
            }
        }
    }
    pub(crate) fn poll_executable_location(&mut self) -> bool {
        let Some(rx) = &self.executable_location_worker else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("Worker disconnected without a result".into()),
        };
        self.executable_location_worker = None;
        let failed = result.is_err();
        let message = match result {
            Ok(path) => format!("Windows accepted the Explorer selection request.\n{path}"),
            Err(error) => format!("Request failed: {error}"),
        };
        if let Some(view) = &mut self.executable_location {
            view.failed = failed;
            view.message = message;
        } else {
            self.status = format!("Earlier executable location action: {message}");
        }
        true
    }
    pub(crate) fn executable_location_key(&mut self, key: KeyEvent) {
        let Some(view) = &mut self.executable_location else {
            return;
        };
        let layout = crate::ui::executable_location::layout(self.last_screen_area, view);
        let max_offset = layout.max_offset();
        view.offset = view.offset.min(max_offset);
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.executable_location = None,
            KeyCode::Down => view.offset = view.offset.saturating_add(1),
            KeyCode::Up => view.offset = view.offset.saturating_sub(1),
            KeyCode::PageDown => {
                view.offset = view
                    .offset
                    .saturating_add(layout.content.height.max(1) as usize)
            }
            KeyCode::PageUp => {
                view.offset = view
                    .offset
                    .saturating_sub(layout.content.height.max(1) as usize)
            }
            KeyCode::Home => view.offset = 0,
            KeyCode::End => view.offset = max_offset,
            _ => {}
        }
    }
    pub(crate) fn executable_location_mouse(&mut self, mouse: MouseEvent) {
        let Some(view) = &self.executable_location else {
            return;
        };
        if !crate::ui::executable_location::layout(self.last_screen_area, view)
            .area
            .contains((mouse.column, mouse.row).into())
        {
            return;
        }
        let code = match mouse.kind {
            MouseEventKind::ScrollUp => KeyCode::Up,
            MouseEventKind::ScrollDown => KeyCode::Down,
            _ => return,
        };
        self.executable_location_key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
    }
}
