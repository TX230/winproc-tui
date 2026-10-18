use std::time::{Duration, Instant};

use super::{App, FocusedPanel, ResourcePanel};

const ACTION_FEEDBACK_LIFETIME: Duration = Duration::from_secs(6);
const FOCUS_FEEDBACK_LIFETIME: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct StatusFeedback {
    observed: String,
    expires_at: Option<Instant>,
    context: (bool, bool, bool),
    panel: Option<(FocusedPanel, ResourcePanel)>,
}

impl App {
    // The event loop observes action results independently of sampling, including
    // while display-paused or in Log view. Durable failures live in dialog state.
    pub(crate) fn refresh_status_feedback(&mut self, now: Instant) -> bool {
        let context = (
            self.has_workspace_overlay(),
            self.network_browser.visible,
            self.file_users.visible,
        );
        let panel = (self.focused_panel, self.resource_panel);
        let selection_feedback = [
            "MEM row: ",
            "GPU row: ",
            "GPU adapter ",
            "NW/DISK row: ",
            "CPU row: ",
        ]
        .iter()
        .any(|prefix| self.status.starts_with(prefix));
        let focus_feedback = selection_feedback || self.status.starts_with("Focus:");
        let feedback = &mut self.status_feedback;
        let mut changed = false;
        if (context != feedback.context
            || (focus_feedback && feedback.panel.is_some_and(|previous| previous != panel)))
            && self.status == feedback.observed
        {
            changed = !self.status.is_empty();
            self.status.clear();
        }
        feedback.context = context;
        feedback.panel = Some(panel);
        if self.status != feedback.observed {
            let lifetime = if focus_feedback {
                FOCUS_FEEDBACK_LIFETIME
            } else {
                ACTION_FEEDBACK_LIFETIME
            };
            feedback.expires_at = (!self.status.is_empty()).then_some(now + lifetime);
            feedback.observed.clone_from(&self.status);
            changed = true;
        }
        if feedback.expires_at.is_some_and(|deadline| now >= deadline) {
            self.status.clear();
            feedback.observed.clear();
            feedback.expires_at = None;
            changed = true;
        }
        changed
    }
}
