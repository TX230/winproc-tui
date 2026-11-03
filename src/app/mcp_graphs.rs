//! Explicit Graph edits and application-owned CSV exports for MCP clients.
use super::{
    App, AppActivity, DetailsMetric, GraphDisplayMode, GraphId, GraphSlot, graph_csv::CsvRange,
    state::GraphEntry,
};
use crate::{
    mcp::{operations::Action, transport::error},
    model::{ProcessIdentity, history::SystemMetric},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

const SYSTEM: &[(&str, SystemMetric)] = &[
    ("cpu_average_percent", SystemMetric::CpuAverage),
    ("physical_memory_bytes", SystemMetric::PhysicalMemory),
    ("modified_memory_bytes", SystemMetric::ModifiedMemory),
    ("standby_memory_bytes", SystemMetric::StandbyMemory),
    ("free_zeroed_memory_bytes", SystemMetric::FreeZeroedMemory),
    ("committed_bytes", SystemMetric::Committed),
    ("paged_pool_bytes", SystemMetric::PagedPool),
    ("nonpaged_pool_bytes", SystemMetric::NonpagedPool),
    ("pages_input_per_sec", SystemMetric::PagesInput),
    ("pages_output_per_sec", SystemMetric::PagesOutput),
    ("thread_count", SystemMetric::ThreadCount),
    ("process_count", SystemMetric::ProcessCount),
    (
        "network_received_bytes_per_sec",
        SystemMetric::NetworkReceived,
    ),
    ("network_sent_bytes_per_sec", SystemMetric::NetworkSent),
    ("disk_read_bytes_per_sec", SystemMetric::DiskRead),
    ("disk_write_bytes_per_sec", SystemMetric::DiskWrite),
    ("disk_queue_length", SystemMetric::DiskQueueLength),
];
const PROCESS: &[(&str, DetailsMetric)] = &[
    ("cpu_percent", DetailsMetric::CpuPercent),
    ("private_bytes", DetailsMetric::Private),
    ("workset_bytes", DetailsMetric::Workset),
    ("workset_private_bytes", DetailsMetric::WorksetPrivate),
    ("workset_shareable_bytes", DetailsMetric::WorksetShareable),
    ("thread_count", DetailsMetric::ThreadCount),
    ("handle_count", DetailsMetric::HandleCount),
    ("user_object_count", DetailsMetric::UserObjectCount),
    ("gdi_object_count", DetailsMetric::GdiObjectCount),
    ("gpu_percent", DetailsMetric::GpuPercent),
    ("dotnet_heap_bytes", DetailsMetric::DotNetHeap),
    ("dotnet_gc_gen0_heap_bytes", DetailsMetric::DotNetGcGen0Heap),
    ("dotnet_gc_gen1_heap_bytes", DetailsMetric::DotNetGcGen1Heap),
    ("dotnet_gc_gen2_heap_bytes", DetailsMetric::DotNetGcGen2Heap),
    ("dotnet_gc_loh_bytes", DetailsMetric::DotNetGcLoh),
    ("dotnet_gc_poh_bytes", DetailsMetric::DotNetGcPoh),
    (
        "dotnet_gc_committed_bytes",
        DetailsMetric::DotNetGcCommitted,
    ),
    (
        "dotnet_gc_fragmentation_bytes",
        DetailsMetric::DotNetGcFragmentation,
    ),
    (
        "dotnet_allocation_bytes_per_sec",
        DetailsMetric::DotNetAllocation,
    ),
    ("gpu_dedicated_bytes", DetailsMetric::GpuDedicated),
    ("gpu_shared_bytes", DetailsMetric::GpuShared),
    ("io_read_bytes_per_sec", DetailsMetric::IoRead),
    ("io_write_bytes_per_sec", DetailsMetric::IoWrite),
];

pub(super) struct PendingCsv {
    id: String,
    receiver: Receiver<Result<usize, String>>,
    outcome: Value,
}

impl App {
    pub(super) fn mcp_graph_fingerprint(&self) -> Value {
        json!({
            "entries": format!("{:?}", self.graph_entries),
            "activity": format!("{:?}", self.activity()),
            "paused": self.is_display_paused(),
            "display_time": if self.is_display_paused() || self.activity() == AppActivity::LogView {
                Some(self.display_snapshot().captured_at)
            } else { None },
            "generation": self.mcp_state.display_generation,
            "span": self.graph_time_span_seconds,
            "offset": self.graph_time_offset_seconds,
            "right": self.graph_time_window_right_at,
            "fit_all": self.graph_show_all_samples,
            "ab": format!("{:?}", self.ab_comparison)
        })
    }

    pub(super) fn mcp_graph_state(&self) -> Value {
        let revision = self.mcp_revision();
        let epoch = revision
            .rsplit_once(':')
            .map_or(revision.as_str(), |(epoch, _)| epoch);
        let graphs: Vec<_> = self.graph_entries.iter().map(|entry| {
            let mut row = json!({
                "graph_id": format!("{}:graph:{}", epoch, entry.id.0),
                "label": entry.source.description(),
                "display_mode": if entry.display_mode == GraphDisplayMode::Raw { "raw" } else { "ma5" }
            });
            match &entry.source {
                GraphSlot::System { metric } => {
                    row["target"] = json!({"kind":"system"});
                    if let Some((id, _)) = SYSTEM.iter().find(|(_, m)| m == metric) {
                        row["metric_id"] = json!(id);
                    }
                }
                GraphSlot::Process { identity, metric } => {
                    row["target"] = identity_json(identity);
                    if let Some((id, _)) = PROCESS.iter().find(|(_, m)| m == metric) {
                        row["metric_id"] = json!(id);
                    }
                }
                GraphSlot::Gpu { adapter_id, adapter_name, .. } => {
                    row["target"] = json!({
                        "kind":"gpu", "adapter_high":adapter_id.high,
                        "adapter_low":adapter_id.low, "adapter_name":adapter_name
                    });
                }
            }
            row
        }).collect();
        json!({
            "revision": revision,
            "source": if self.activity() == AppActivity::LogView { "log" } else { "live" },
            "captured_at": self.display_snapshot().captured_at.with_timezone(&chrono::Utc).to_rfc3339(),
            "activity": match self.activity() {
                AppActivity::Live => "live", AppActivity::Recording => "recording", AppActivity::LogView => "log_view"
            },
            "display_paused": self.is_display_paused(),
            "graphs": graphs,
            "active_graph_id": self.active_graph_id.map(|id| format!("{}:graph:{}", epoch, id.0)),
            "workspace_visible": self.show_details,
            "capacity": 16,
            "supported_metrics": {
                "system": SYSTEM.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                "process": PROCESS.iter().map(|(id, _)| *id).collect::<Vec<_>>()
            },
            "export_ranges": {
                "all": true, "visible": self.graph_time_reference_at().is_some(), "ab": self.csv_ab_available()
            }
        })
    }

    pub(super) fn mcp_graph_targets(&self) -> Vec<ProcessIdentity> {
        let mut targets: Vec<_> = self
            .display_process_history()
            .identities()
            .cloned()
            .collect();
        targets.extend(
            self.display_snapshot()
                .processes
                .iter()
                .map(ProcessIdentity::from_row),
        );
        targets.sort_by(|a, b| (&a.name, a.pid, a.start_time).cmp(&(&b.name, b.pid, b.start_time)));
        targets.dedup();
        targets
    }
    fn resolve_graph_additions(&self, action: &Action) -> Result<Vec<GraphSlot>, Value> {
        if action
            .graph
            .revision
            .as_deref()
            .is_some_and(|r| r != self.mcp_revision())
        {
            return Err(error(
                "STATE_CONFLICT",
                "Displayed Graph source changed; read Graph targets again",
                false,
            ));
        }
        let displayed: std::collections::HashSet<_> = if action.graph.identities.is_empty() {
            Default::default()
        } else {
            self.mcp_graph_targets().into_iter().collect()
        };
        let mut additions = Vec::new();
        for spec in action.args["metrics"].as_array().into_iter().flatten() {
            let metric = spec["metric_id"].as_str().unwrap();
            let source = if let Some(reference) = spec["process_ref"].as_str() {
                let Some(identity) = action.graph.identities.get(reference) else {
                    return Err(error(
                        "REFERENCE_INVALID",
                        "Read a process reference before adding its Graph",
                        false,
                    ));
                };
                if identity.start_time.is_none() {
                    return Err(error(
                        "IDENTITY_UNVERIFIABLE",
                        "Graph target needs a verified creation time",
                        false,
                    ));
                }
                if self.activity() == AppActivity::LogView && action.graph.revision.is_none() {
                    return Err(error(
                        "ACTIVITY_CONFLICT",
                        "Use displayed Graph process references in Log view",
                        false,
                    ));
                }
                if !displayed.contains(identity) {
                    return Err(error(
                        "REFERENCE_INVALID",
                        "Process is not in the displayed snapshot or retained history",
                        false,
                    ));
                }
                let Some((_, metric)) = PROCESS.iter().find(|(id, _)| *id == metric) else {
                    return Err(error(
                        "INVALID_ARGUMENT",
                        "Choose a process metric from get_graphs",
                        false,
                    ));
                };
                GraphSlot::Process {
                    identity: identity.clone(),
                    metric: *metric,
                }
            } else {
                let Some((_, metric)) = SYSTEM.iter().find(|(id, _)| *id == metric) else {
                    return Err(error(
                        "INVALID_ARGUMENT",
                        "Choose a system metric or supply process_ref",
                        false,
                    ));
                };
                GraphSlot::System { metric: *metric }
            };
            if !self.graph_entries.iter().any(|e| e.source == source)
                && !additions.contains(&source)
            {
                additions.push(source);
            }
        }
        if self.graph_entries.len() + additions.len() > 16 {
            return Err(error(
                "RESOURCE_LIMIT",
                "Graph workspace has at most 16 unique sources",
                false,
            ));
        }
        Ok(additions)
    }
    fn append_mcp_graphs(&mut self, sources: Vec<GraphSlot>) {
        for source in sources {
            let id = GraphId(self.next_graph_id);
            self.next_graph_id = self
                .next_graph_id
                .checked_add(1)
                .expect("GraphId space exhausted");
            self.graph_entries.push(GraphEntry {
                id,
                source,
                display_mode: GraphDisplayMode::default(),
            });
            if self.active_graph_id.is_none() {
                self.active_graph_id = Some(id);
            }
        }
    }
    pub(super) fn apply_mcp_graph_action(&mut self, action: &Action) -> Value {
        let additions = match self.resolve_graph_additions(action) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if action.method == "winproc_add_graphs" {
            self.append_mcp_graphs(additions);
            self.refresh_mcp_control();
            return self.mcp_graph_state();
        }
        let destination = match export_destination(&action.args) {
            Ok(path) => path,
            Err(message) => return error("INVALID_ARGUMENT", &message, false),
        };
        let range = match action.args["range"].as_str().unwrap_or("all") {
            "all" => CsvRange::All,
            "visible" => CsvRange::Visible,
            "ab" => CsvRange::Ab,
            _ => return error("INVALID_ARGUMENT", "Unknown CSV range", false),
        };
        if self.graph_entries.is_empty() && additions.is_empty() {
            return error(
                "INVALID_ARGUMENT",
                "Add Graph metrics or include metrics in this export",
                false,
            );
        }
        let previous = self.graph_entries.clone();
        let active = self.active_graph_id;
        self.append_mcp_graphs(additions);
        let snapshot = match self.csv_snapshot(range) {
            Ok(s) => s,
            Err(message) => {
                self.graph_entries = previous;
                self.active_graph_id = active;
                return error("INVALID_ARGUMENT", &message, false);
            }
        };
        let (sender, receiver) = mpsc::channel();
        let target = destination.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("mcp-graph-csv".into())
            .spawn(move || {
                let _ = sender.send(snapshot.write_file(&target));
            })
        {
            self.graph_entries = previous;
            self.active_graph_id = active;
            return error("INTERNAL_ERROR", &e.to_string(), false);
        }
        self.refresh_mcp_control();
        let outcome = json!({"output_path":destination.to_string_lossy(),"range":action.args["range"].as_str().unwrap_or("all"),"workspace":self.mcp_graph_state()});
        self.mcp_state.csv = Some(PendingCsv {
            id: action.id.clone(),
            receiver,
            outcome,
        });
        json!({"csv_pending":true})
    }
    pub(super) fn poll_mcp_csv(&mut self) -> bool {
        let Some(pending) = self.mcp_state.csv.as_ref() else {
            return false;
        };
        let result = match pending.receiver.try_recv() {
            Ok(v) => v,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("CSV worker stopped".into()),
        };
        let mut pending = self.mcp_state.csv.take().unwrap();
        match result {
            Ok(rows) => {
                pending.outcome["rows"] = json!(rows);
                self.status = format!(
                    "Exported {rows} CSV rows: {}",
                    pending.outcome["output_path"].as_str().unwrap()
                );
            }
            Err(message) => {
                pending.outcome["error"] = error("EXPORT_FAILED", &message, false)["error"].clone();
                self.status = format!("CSV export failed: {message}");
            }
        }
        self.mcp_state
            .operations
            .lock()
            .unwrap()
            .finish(&pending.id, pending.outcome);
        true
    }
}
fn identity_json(identity: &ProcessIdentity) -> Value {
    json!({"kind":"process","pid":identity.pid,"name":identity.name,"start_time":identity.start_time.map(|time|time.to_string())})
}
fn export_destination(args: &Value) -> Result<PathBuf, String> {
    let directory = args["output_directory"].as_str().unwrap();
    if directory.chars().any(char::is_control)
        || directory.starts_with(r"\\.\")
        || directory
            .to_ascii_uppercase()
            .starts_with(r"\\?\GLOBALROOT")
    {
        return Err("Choose a normal filesystem directory".into());
    }
    let generated = format!("graphs-{}.csv", uuid::Uuid::new_v4());
    let name = args["filename"].as_str().unwrap_or(&generated);
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || device
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || !name.to_ascii_lowercase().ends_with(".csv")
        || Path::new(name).components().count() != 1
    {
        return Err("filename must be a single normal .csv filename".into());
    }
    std::path::absolute(Path::new(directory).join(name)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graph_metric_mappings_match_the_mcp_catalog() {
        let system: std::collections::HashSet<_> = SYSTEM.iter().map(|(id, _)| *id).collect();
        let process: std::collections::HashSet<_> = PROCESS.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            system,
            crate::mcp::metrics::SYSTEM_METRICS
                .iter()
                .copied()
                .collect()
        );
        assert_eq!(
            process,
            crate::mcp::metrics::PROCESS_METRICS
                .iter()
                .copied()
                .collect()
        );
        assert_eq!(system.len(), SYSTEM.len());
        assert_eq!(process.len(), PROCESS.len());
    }
}
