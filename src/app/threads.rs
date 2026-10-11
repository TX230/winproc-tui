use super::{App, AppActivity};
use crate::{model::threads::ThreadReport, ui::widgets::scrollable_modal::ScrollableModalState};
#[derive(Default)]
pub(crate) struct ThreadsView {
    pub(crate) report: Option<ThreadReport>,
    pub(crate) pending: bool,
    pub(crate) error: Option<String>,
    pub(crate) scroll: ScrollableModalState,
}
impl App {
    pub(crate) fn ensure_threads(&mut self) {
        if self.threads.report.is_none() && self.threads.error.is_none() {
            self.refresh_threads();
        }
    }
    pub(crate) fn refresh_threads(&mut self) {
        if !self.show_process_info_dialog
            || self.activity() == AppActivity::LogView
            || self.threads.pending
        {
            return;
        }
        if !self.process_info_target_is_currently_live() {
            self.threads.error = Some("Process exited or no live process is selected.".into());
            return;
        }
        let Some(target) = &self.process_info_target else {
            return;
        };
        match self
            .threads_worker
            .request(self.process_info_generation, target.identity.clone())
        {
            Ok(()) => {
                self.threads.pending = true;
                self.threads.error = None;
            }
            Err(error) => self.threads.error = Some(error),
        }
    }
    pub(crate) fn poll_threads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.threads_worker.try_recv() {
            if !self.show_process_info_dialog
                || self.activity() == AppActivity::LogView
                || !self.threads.pending
                || result.generation != self.process_info_generation
                || self
                    .process_info_target
                    .as_ref()
                    .map(|target| &target.identity)
                    != Some(&result.identity)
            {
                continue;
            }
            self.threads.pending = false;
            match result.outcome {
                Ok(report) => {
                    self.threads.report = Some(report);
                    self.threads.error = None;
                }
                Err(error) => self.threads.error = Some(error),
            }
            changed = true;
        }
        changed
    }
}
