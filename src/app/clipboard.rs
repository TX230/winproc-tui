use anyhow::Result;

use crate::{
    App,
    app::system_info::{system_info_fields, system_info_plain_text},
    app::{FocusedPanel, GraphValueFormat},
    model::{MetricColumn, ProcessRow, history::SystemMetric},
    ui::format::{
        format_integer, format_io_rate, format_mb, format_mb_per_sec, format_mbps,
        format_signed_integer, format_signed_io_rate,
    },
};

impl App {
    pub(crate) fn copy_focused_cell_to_clipboard(&mut self) -> Result<()> {
        match self.focused_panel {
            FocusedPanel::System => self.copy_selected_system_row_to_clipboard(),
            FocusedPanel::SystemActivity => self.copy_selected_system_activity_row_to_clipboard(),
            FocusedPanel::Cpu => self.copy_selected_cpu_row_to_clipboard(),
            FocusedPanel::Processes => self.copy_selected_process_row_to_clipboard(),
            FocusedPanel::DetailsSamples if self.show_details => {
                self.copy_selected_sample_row_to_clipboard()
            }
            _ => {
                self.status = "No row to copy".to_string();
                Ok(())
            }
        }
    }

    pub(crate) fn copy_system_info_to_clipboard(&mut self) -> Result<()> {
        let field_count = system_info_fields(self).len();
        let text = system_info_plain_text(self);
        match copy_text_to_clipboard(&text) {
            Ok(()) => self.status = format!("Copied System Info ({field_count} fields)"),
            Err(error) => self.status = format!("Clipboard copy failed: {error}"),
        }
        Ok(())
    }

    pub(crate) fn copy_open_files_to_clipboard(&mut self) -> Result<()> {
        if self.open_files_show_detail {
            let Some(entry) = crate::ui::open_files::selected_entry(self) else {
                return Ok(());
            };
            let text = entry.plain_text();
            self.status = match copy_text_to_clipboard(&text) {
                Ok(()) => "Copied file handle attributes".into(),
                Err(error) => format!("Clipboard copy failed: {error}"),
            };
            return Ok(());
        }
        if self.open_files_result.is_none() {
            self.status = "No open file paths to copy".to_string();
            return Ok(());
        }
        let entries = crate::ui::open_files::filtered_entries(self);
        if entries.is_empty() {
            self.status = "No open file paths to copy".to_string();
            return Ok(());
        }

        let text = entries
            .iter()
            .map(|entry| entry.plain_text())
            .collect::<Vec<_>>()
            .join("\n");

        match copy_text_to_clipboard(&text) {
            Ok(()) => {
                self.status = format!("Copied {} file handle rows", entries.len());
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    pub(crate) fn copy_selected_process_module_to_clipboard(&mut self) -> Result<()> {
        let Some(entry) = crate::ui::process_modules::selected_entry(self) else {
            self.status = "No DLL selected".to_string();
            return Ok(());
        };
        let path = entry.path.clone();
        match copy_text_to_clipboard(&path) {
            Ok(()) => self.status = "Copied DLL path".to_string(),
            Err(error) => self.status = format!("Clipboard copy failed: {error}"),
        }
        Ok(())
    }

    pub(crate) fn copy_selected_process_environment_to_clipboard(&mut self) -> Result<()> {
        let Some(entry) = crate::ui::process_environment::selected_entry(self) else {
            self.status = "No environment variable selected".to_string();
            return Ok(());
        };
        let value = entry.raw();
        match copy_text_to_clipboard(&value) {
            Ok(()) => self.status = "Copied environment variable".to_string(),
            Err(error) => self.status = format!("Clipboard copy failed: {error}"),
        }
        Ok(())
    }

    pub(crate) fn copy_selected_process_row_to_clipboard(&mut self) -> Result<()> {
        let Some(process) = self.selected_visible_process() else {
            self.status = "No process selected".to_string();
            return Ok(());
        };

        let process_name = process.name.clone();
        let value = selected_process_row_text(process, &self.process_columns);

        match copy_text_to_clipboard(&value) {
            Ok(()) => {
                self.status = format!("Copied process row: {process_name}");
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn copy_selected_system_row_to_clipboard(&mut self) -> Result<()> {
        let metric = self.selected_system_metric();
        let value = selected_system_row_text(self, metric);

        match copy_text_to_clipboard(&value) {
            Ok(()) => {
                self.status = format!("Copied row: {}", metric.label());
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn copy_selected_system_activity_row_to_clipboard(&mut self) -> Result<()> {
        let metric = self.selected_system_activity_metric();
        let value = selected_system_row_text(self, metric);

        match copy_text_to_clipboard(&value) {
            Ok(()) => {
                self.status = format!("Copied row: {}", metric.label());
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn copy_selected_cpu_row_to_clipboard(&mut self) -> Result<()> {
        let Some(metric) = self.selected_cpu_metric() else {
            self.status = "Per-core Usage has no single value to copy".to_string();
            return Ok(());
        };
        let value = selected_system_row_text(self, metric);

        match copy_text_to_clipboard(&value) {
            Ok(()) => {
                self.status = format!("Copied row: {}", metric.label());
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn copy_selected_sample_row_to_clipboard(&mut self) -> Result<()> {
        let Some(text) = self.samples_clipboard_text() else {
            self.status = "No retained sample selection to copy".into();
            return Ok(());
        };
        self.status = match copy_text_to_clipboard(&text) {
            Ok(()) => format!("Copied {} sample rows", text.lines().count()),
            Err(error) => format!("Clipboard copy failed: {error}"),
        };
        Ok(())
    }

    pub(crate) fn samples_clipboard_text(&self) -> Option<String> {
        let slot = self.active_graph_slot()?;
        let samples = self.graph_slot_samples(slot);
        let (start, end) = if let Some((id, anchor, end)) = self.sample_selection {
            if self.active_graph_id != Some(id) {
                return None;
            }
            let a = samples.iter().position(|s| s.captured_at == anchor)?;
            let b = samples.iter().position(|s| s.captured_at == end)?;
            (a.min(b), a.max(b))
        } else {
            (self.details_sample_selected, self.details_sample_selected)
        };
        samples.get(start..=end)?;
        Some(
            (start..=end)
                .map(|index| {
                    sample_row_text(&samples, index, slot.value_format(), self.show_sample_delta)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    pub(crate) fn sample_time_is_selected(&self, time: chrono::DateTime<chrono::Local>) -> bool {
        self.sample_selection.is_some_and(|(id, a, b)| {
            self.active_graph_id == Some(id) && time >= a.min(b) && time <= a.max(b)
        })
    }

    pub(crate) fn extend_sample_selection(&mut self, older: bool, by_page: bool) {
        let Some(id) = self.active_graph_id else {
            return;
        };
        self.clamp_details_sample_selection();
        let Some(current) = self.selected_details_sample_time() else {
            return;
        };
        let anchor = self
            .sample_selection
            .filter(|(graph, _, _)| *graph == id)
            .map_or(current, |(_, a, _)| a);
        match (older, by_page) {
            (true, true) => self.select_details_sample_page_older(),
            (false, true) => self.select_details_sample_page_newer(),
            (true, false) => self.select_details_sample_older(1),
            (false, false) => self.select_details_sample_newer(1),
        }
        if let Some(end) = self.selected_details_sample_time() {
            self.sample_selection = Some((id, anchor, end));
            self.details_live = false;
        }
    }
}

pub(super) fn sample_row_text(
    samples: &[super::GraphSample],
    index: usize,
    format: GraphValueFormat,
    delta: bool,
) -> String {
    let sample = &samples[index];
    let mut fields = vec![
        sample.captured_at.format("%H:%M:%S").to_string(),
        format_graph_sample_value(sample.value, format),
    ];
    if delta {
        fields.push(format_details_sample_delta(
            sample.value,
            index.checked_sub(1).and_then(|i| samples[i].value),
            format,
        ));
    }
    fields.join("\t")
}

pub(super) fn selected_process_row_text(process: &ProcessRow, columns: &[MetricColumn]) -> String {
    let mut fields = vec![process.pid.to_string(), process.name.clone()];
    fields.extend(
        columns
            .iter()
            .map(|column| format_process_metric_column(process, *column)),
    );
    fields.join("\t")
}

fn selected_system_row_text(app: &App, metric: SystemMetric) -> String {
    let snapshot = app.display_snapshot();
    let value = match metric {
        SystemMetric::CpuAverage => format!(
            "{} (U {}, K {})",
            snapshot
                .cpu_total_usage_percent
                .map(|value| format!("{}%", value.min(100)))
                .unwrap_or_else(|| "--".to_string()),
            snapshot
                .cpu_user_usage_percent
                .map(|value| format!("{}%", value.min(100)))
                .unwrap_or_else(|| "--".to_string()),
            snapshot
                .cpu_kernel_usage_percent
                .map(|value| format!("{}%", value.min(100)))
                .unwrap_or_else(|| "--".to_string())
        ),
        SystemMetric::PhysicalMemory => {
            format_memory_row_value(Some(snapshot.used_memory), Some(snapshot.total_memory))
        }
        SystemMetric::ModifiedMemory => format_memory_row_value(snapshot.modified_memory, None),
        SystemMetric::StandbyMemory => format_memory_row_value(snapshot.standby_memory, None),
        SystemMetric::FreeZeroedMemory => {
            format_memory_row_value(snapshot.free_zeroed_memory, None)
        }
        SystemMetric::Committed => {
            format_memory_row_value(snapshot.committed_memory, snapshot.commit_limit)
        }
        SystemMetric::PagedPool => format_memory_row_value(snapshot.paged_pool_memory, None),
        SystemMetric::NonpagedPool => format_memory_row_value(snapshot.nonpaged_pool_memory, None),
        SystemMetric::PagesInput => snapshot
            .pages_input_per_sec
            .map(format_integer)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::PagesOutput => snapshot
            .pages_output_per_sec
            .map(format_integer)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::ThreadCount => snapshot
            .thread_count
            .map(format_integer)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::ProcessCount => format_integer(snapshot.process_count as u64),
        SystemMetric::GpuUtilization | SystemMetric::GpuEncode | SystemMetric::GpuDecode => app
            .selected_gpu_adapter()
            .and_then(|adapter| match metric {
                SystemMetric::GpuUtilization => adapter.utilization_percent,
                SystemMetric::GpuEncode => adapter.encode.average_percent,
                SystemMetric::GpuDecode => adapter.decode.average_percent,
                _ => None,
            })
            .map(|value| format!("{value:.0}%"))
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::GpuDedicated => app.selected_gpu_adapter().map_or_else(
            || "--".to_string(),
            |adapter| format_memory_row_value(adapter.dedicated_used, adapter.dedicated_total),
        ),
        SystemMetric::GpuShared => app.selected_gpu_adapter().map_or_else(
            || "--".to_string(),
            |adapter| format_memory_row_value(adapter.shared_used, adapter.shared_total),
        ),
        SystemMetric::NetworkReceived => snapshot
            .network_received_bytes_per_sec
            .map(format_mbps)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::NetworkSent => snapshot
            .network_sent_bytes_per_sec
            .map(format_mbps)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::DiskRead => snapshot
            .disk_read_bytes_per_sec
            .map(format_mb_per_sec)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::DiskWrite => snapshot
            .disk_write_bytes_per_sec
            .map(format_mb_per_sec)
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::DiskQueueLength => snapshot
            .disk_queue_length
            .filter(|value| value.is_finite())
            .map(|value| format!("{value:.1}"))
            .unwrap_or_else(|| "--".to_string()),
    };
    format!("{}\t{value}", metric.label())
}

fn format_memory_row_value(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(used), Some(total)) => format!("{} / {}", format_mb(used), format_mb(total)),
        (Some(used), None) => format_mb(used),
        (None, Some(total)) => format!("-- / {}", format_mb(total)),
        (None, None) => "--".to_string(),
    }
}

fn format_process_metric_column(process: &ProcessRow, column: MetricColumn) -> String {
    match column {
        MetricColumn::CpuPercent => process
            .cpu_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::PrivateBytes => format_optional_integer(process.private_bytes),
        MetricColumn::WorksetBytes => format_optional_integer(process.workset_bytes),
        MetricColumn::WorksetPrivateBytes => format_optional_integer(process.workset_private_bytes),
        MetricColumn::WorksetShareableBytes => {
            format_optional_integer(process.workset_shareable_bytes)
        }
        MetricColumn::ThreadCount => format_optional_integer(process.thread_count),
        MetricColumn::HandleCount => format_optional_integer(process.handle_count),
        MetricColumn::UserObjectCount => format_optional_integer(process.user_object_count),
        MetricColumn::GdiObjectCount => format_optional_integer(process.gdi_object_count),
        MetricColumn::GpuPercent => process
            .gpu_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::DotNetHeapBytes => format_optional_integer(process.dotnet_heap_bytes),
        MetricColumn::DotNetGcGen0HeapBytes => {
            format_optional_integer(process.dotnet_gc_gen0_heap_bytes)
        }
        MetricColumn::DotNetGcGen1HeapBytes => {
            format_optional_integer(process.dotnet_gc_gen1_heap_bytes)
        }
        MetricColumn::DotNetGcGen2HeapBytes => {
            format_optional_integer(process.dotnet_gc_gen2_heap_bytes)
        }
        MetricColumn::DotNetGcLohBytes => format_optional_integer(process.dotnet_gc_loh_bytes),
        MetricColumn::DotNetGcPohBytes => format_optional_integer(process.dotnet_gc_poh_bytes),
        MetricColumn::DotNetGcCommittedBytes => {
            format_optional_integer(process.dotnet_gc_committed_bytes)
        }
        MetricColumn::DotNetGcFragmentationBytes => {
            format_optional_integer(process.dotnet_gc_fragmentation_bytes)
        }
        MetricColumn::DotNetAllocationBytesPerSec => {
            format_optional_integer(process.dotnet_allocation_bytes_per_sec)
        }
        MetricColumn::GpuDedicatedBytes => format_optional_integer(process.gpu_dedicated_bytes),
        MetricColumn::GpuSharedBytes => format_optional_integer(process.gpu_shared_bytes),
        MetricColumn::IoReadBytesPerSec => process
            .io_read_bytes_per_sec
            .map(format_io_rate)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::IoWriteBytesPerSec => process
            .io_write_bytes_per_sec
            .map(format_io_rate)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::FullPath => process
            .executable_path
            .clone()
            .unwrap_or_else(|| "--".to_string()),
    }
}

pub(super) fn format_graph_sample_value(
    value: Option<f64>,
    value_format: GraphValueFormat,
) -> String {
    let Some(value) = value else {
        return "--".to_string();
    };
    match value_format {
        GraphValueFormat::Percent => format!("{value:.1}%"),
        GraphValueFormat::AdaptiveBitsPerSec => format_io_rate(value.round().max(0.0) as u64),
        GraphValueFormat::MegabitsPerSec => format_mbps(value.round().max(0.0) as u64),
        GraphValueFormat::MegabytesPerSec => format_mb_per_sec(value.round().max(0.0) as u64),
        GraphValueFormat::QueueLength => format!("{value:.1}"),
        GraphValueFormat::BytesPerSec => {
            format!("{}/s", format_integer(value.round().max(0.0) as u64))
        }
        GraphValueFormat::Bytes | GraphValueFormat::Count => {
            format_integer(value.round().max(0.0) as u64)
        }
    }
}

fn format_details_sample_delta(
    value: Option<f64>,
    previous: Option<f64>,
    value_format: GraphValueFormat,
) -> String {
    let Some(value) = value else {
        return "--".to_string();
    };
    let Some(previous) = previous else {
        return "--".to_string();
    };
    format_details_delta(value - previous, value_format)
}

fn format_details_delta(delta: f64, value_format: GraphValueFormat) -> String {
    match value_format {
        GraphValueFormat::Percent => format!("{delta:+.1}%"),
        GraphValueFormat::AdaptiveBitsPerSec => format_signed_io_rate(delta.round() as i128),
        GraphValueFormat::MegabitsPerSec => {
            let mbps = ((delta * 8.0) / 1_000_000.0).round() as i128;
            format_signed_integer(mbps) + " Mbps"
        }
        GraphValueFormat::MegabytesPerSec => {
            let mb_per_sec = (delta / 1_000_000.0).round() as i128;
            format_signed_integer(mb_per_sec) + " MB/s"
        }
        GraphValueFormat::QueueLength => format!("{delta:+.1}"),
        GraphValueFormat::BytesPerSec => {
            format!("{}/s", format_signed_integer(delta.round() as i128))
        }
        GraphValueFormat::Bytes | GraphValueFormat::Count => {
            format_signed_integer(delta.round() as i128)
        }
    }
}

fn format_optional_integer(value: Option<u64>) -> String {
    value
        .map(format_integer)
        .unwrap_or_else(|| "--".to_string())
}

#[cfg(not(test))]
pub(super) fn copy_text_to_clipboard(value: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(value.to_string())?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    static LAST_COPIED_TEXT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn last_copied_text() -> Option<String> {
    LAST_COPIED_TEXT.with(|value| value.borrow().clone())
}

#[cfg(test)]
pub(super) fn copy_text_to_clipboard(value: &str) -> Result<()> {
    LAST_COPIED_TEXT.with(|last| {
        *last.borrow_mut() = Some(value.to_string());
    });
    Ok(())
}
