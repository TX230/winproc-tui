use crate::{App, model::ProcessIdentity};

impl App {
    pub(crate) fn toggle_selected_dotnet(&mut self) {
        if self.activity() == super::AppActivity::LogView {
            self.status = ".NET collection is unavailable in Log view".into();
            return;
        }
        let Some(identity) = self.selected_live_process_identity() else {
            self.status = "Select a live process to toggle .NET metrics".into();
            return;
        };
        if self.sampling_worker.dotnet.allows(&identity) {
            self.sampling_worker.stop_dotnet_target(&identity);
            for row in &mut self.snapshot.processes {
                if ProcessIdentity::from_row(row) == identity {
                    crate::samplers::dotnet_control::clear_runtime_values(row);
                }
            }
            self.status = format!(
                ".NET collection stopped for {} ({})",
                identity.name, identity.pid
            );
            return;
        }
        if !self.sampling_worker.dotnet.supports_metrics(&identity) {
            self.status = "No supported .NET 8+ runtime detected for this process yet".into();
            return;
        }
        self.status = if self.sampling_worker.dotnet.enable_target(identity.clone()) {
            format!(
                ".NET collection requested for {} ({}); n stops collection",
                identity.name, identity.pid
            )
        } else {
            ".NET collection is still stopping; retry n after cleanup".into()
        };
    }

    pub(crate) fn toggle_dotnet(&mut self) {
        if self.activity() == super::AppActivity::LogView {
            self.status = ".NET collection is unavailable in Log view".into();
            return;
        }
        if self.sampling_worker.dotnet.stopping() {
            self.status = ".NET collection is stopping; wait before enabling it again".into();
            return;
        }
        if self.sampling_worker.dotnet.enabled() {
            self.stop_dotnet_collection();
        } else {
            let targets = self
                .snapshot
                .processes
                .iter()
                .filter(|row| self.is_tracked_process_name(&row.name))
                .map(ProcessIdentity::from_row)
                .filter(|id| id.start_time.is_some())
                .collect::<Vec<_>>();
            // An empty target set still opens visible guidance; a transient status
            // can be hidden by the compact footer and looks like an ignored input.
            self.dotnet_confirmation = Some(targets);
        }
    }

    fn stop_dotnet_collection(&mut self) {
        self.sampling_worker.suspend_dotnet();
        for row in &mut self.snapshot.processes {
            crate::samplers::dotnet_control::clear_runtime_values(row);
        }
        self.status = if self.sampling_worker.dotnet.stopping() {
            ".NET collection stopping"
        } else {
            ".NET collection disabled"
        }
        .into();
    }

    pub(crate) fn toggle_dotnet_collection(&mut self) {
        let Some(mut targets) = self.dotnet_confirmation.clone() else {
            return;
        };
        if self.sampling_worker.dotnet.enabled() {
            self.stop_dotnet_collection();
            return;
        }
        if self.sampling_worker.dotnet.stopping() {
            self.status = ".NET collection is stopping; wait before enabling it again".into();
            return;
        }
        targets.retain(|id| {
            self.is_tracked_process_name(&id.name)
                && self
                    .snapshot
                    .processes
                    .iter()
                    .any(|row| ProcessIdentity::from_row(row) == *id)
        });
        self.dotnet_confirmation = Some(targets.clone());
        let no_targets = targets.is_empty();
        self.status = if self.sampling_worker.dotnet.enable(targets) {
            if no_targets {
                ".NET metrics enabled with no authorized targets; no connections will be made"
                    .into()
            } else {
                ".NET collection enabled for tracked lifetimes; added/restarted processes require off/on"
                    .into()
            }
        } else {
            "No valid targets, or .NET collection is still stopping".into()
        };
    }
}
