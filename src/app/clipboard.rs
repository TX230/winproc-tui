use anyhow::Result;

use crate::{
    App,
    app::{DetailsMetric, FocusedPanel},
    model::{MetricColumn, ProcessRow, history::SystemMetric},
    ui::format::{format_integer, format_mb, format_mbps, format_signed_integer, ratio_optional},
};

impl App {
    pub(crate) fn copy_focused_cell_to_clipboard(&mut self) -> Result<()> {
        match self.focused_panel {
            FocusedPanel::System => self.copy_selected_system_row_to_clipboard(),
            FocusedPanel::Cpu => self.copy_cpu_average_to_clipboard(),
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

    pub(crate) fn copy_open_files_to_clipboard(&mut self) -> Result<()> {
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
            .map(|entry| {
                if entry.handle_count > 1 {
                    format!("{}\t{}", entry.path, entry.handle_count)
                } else {
                    entry.path.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        match copy_text_to_clipboard(&text) {
            Ok(()) => {
                self.status = format!("Copied {} open file paths", entries.len());
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
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
                self.status = format!("Copied row: {process_name}");
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

    fn copy_cpu_average_to_clipboard(&mut self) -> Result<()> {
        let value = selected_system_row_text(self, SystemMetric::CpuAverage);

        match copy_text_to_clipboard(&value) {
            Ok(()) => {
                self.status = "Copied row: CPU Avg".to_string();
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn copy_selected_sample_row_to_clipboard(&mut self) -> Result<()> {
        let Some(row) = self.selected_sample_clipboard_row() else {
            self.status = "No sample selected".to_string();
            return Ok(());
        };
        let text = row.text();

        match copy_text_to_clipboard(&text) {
            Ok(()) => {
                self.status = format!(
                    "Copied row: {} {}={}",
                    row.time, row.metric_label, row.value
                );
            }
            Err(error) => {
                self.status = format!("Clipboard copy failed: {error}");
            }
        }

        Ok(())
    }

    fn selected_sample_clipboard_row(&self) -> Option<SampleClipboardRow> {
        let slot = self.active_graph_slot()?;
        let samples = self.graph_slot_samples(slot);
        let sample = samples.get(self.details_sample_selected)?;
        let metric = slot.value_format_metric();
        let value = format_graph_sample_value(sample.value, metric);
        let current_value = sample.value;
        let previous_value = self
            .details_sample_selected
            .checked_sub(1)
            .and_then(|index| samples.get(index))
            .and_then(|previous| previous.value);
        Some(SampleClipboardRow {
            marker: sample_ab_marker(self.active_ab_comparison(), sample.captured_at),
            time: sample.captured_at.format("%H:%M:%S").to_string(),
            metric_label: slot.metric_label(),
            value,
            delta: format_details_sample_delta(current_value, previous_value, metric),
        })
    }
}

struct SampleClipboardRow {
    marker: &'static str,
    time: String,
    metric_label: &'static str,
    value: String,
    delta: String,
}

impl SampleClipboardRow {
    fn text(&self) -> String {
        let mut fields = Vec::new();
        if !self.marker.is_empty() {
            fields.push(self.marker.to_string());
        }
        fields.push(self.time.clone());
        fields.push(self.value.clone());
        fields.push(self.delta.clone());
        fields.join("\t")
    }
}

fn selected_process_row_text(process: &ProcessRow, columns: &[MetricColumn]) -> String {
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
        SystemMetric::CpuAverage => snapshot
            .cpu_total_usage_percent
            .map(|value| format!("{}%", value.min(100)))
            .unwrap_or_else(|| "--".to_string()),
        SystemMetric::PhysicalMemory => {
            format_memory_row_value(Some(snapshot.used_memory), Some(snapshot.total_memory))
        }
        SystemMetric::Committed => {
            format_memory_row_value(snapshot.committed_memory, snapshot.commit_limit)
        }
        SystemMetric::GpuDedicated => {
            format_memory_row_value(snapshot.gpu_dedicated_used, snapshot.gpu_dedicated_total)
        }
        SystemMetric::GpuShared => {
            format_memory_row_value(snapshot.gpu_shared_used, snapshot.gpu_shared_total)
        }
    };
    format!("{}\t{value}", metric.label())
}

fn format_memory_row_value(used: Option<u64>, total: Option<u64>) -> String {
    let mut value = match (used, total) {
        (Some(used), Some(total)) => format!("{} / {}", format_mb(used), format_mb(total)),
        (Some(used), None) => format_mb(used),
        (None, Some(total)) => format!("-- / {}", format_mb(total)),
        (None, None) => "--".to_string(),
    };
    if let Some(ratio_value) = ratio_optional(used, total) {
        value.push_str(&format!(" ({:>3.0}%)", ratio_value * 100.0));
    }
    value
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
        MetricColumn::WorksetSharedBytes => format_optional_integer(process.workset_shared_bytes),
        MetricColumn::ThreadCount => format_optional_integer(process.thread_count),
        MetricColumn::HandleCount => format_optional_integer(process.handle_count),
        MetricColumn::UserObjectCount => format_optional_integer(process.user_object_count),
        MetricColumn::GdiObjectCount => format_optional_integer(process.gdi_object_count),
        MetricColumn::GpuPercent => process
            .gpu_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::DotNetHeapBytes => format_optional_integer(process.dotnet_heap_bytes),
        MetricColumn::GpuDedicatedBytes => format_optional_integer(process.gpu_dedicated_bytes),
        MetricColumn::GpuSharedBytes => format_optional_integer(process.gpu_shared_bytes),
        MetricColumn::IoReadBytesPerSec => process
            .io_read_bytes_per_sec
            .map(format_mbps)
            .unwrap_or_else(|| "--".to_string()),
        MetricColumn::IoWriteBytesPerSec => process
            .io_write_bytes_per_sec
            .map(format_mbps)
            .unwrap_or_else(|| "--".to_string()),
    }
}

fn format_graph_sample_value(value: Option<f64>, metric: DetailsMetric) -> String {
    let Some(value) = value else {
        return "--".to_string();
    };
    match metric {
        DetailsMetric::CpuPercent | DetailsMetric::GpuPercent => format!("{value:.1}%"),
        DetailsMetric::IoRead | DetailsMetric::IoWrite => {
            format_mbps(value.round().max(0.0) as u64)
        }
        _ => format_integer(value.round().max(0.0) as u64),
    }
}

fn format_details_sample_delta(
    value: Option<f64>,
    previous: Option<f64>,
    metric: DetailsMetric,
) -> String {
    let Some(value) = value else {
        return "--".to_string();
    };
    let Some(previous) = previous else {
        return "--".to_string();
    };
    format_details_delta(value - previous, metric)
}

fn format_details_delta(delta: f64, metric: DetailsMetric) -> String {
    match metric {
        DetailsMetric::CpuPercent => format!("{delta:+.1}%"),
        DetailsMetric::GpuPercent => format!("{delta:+.1}%"),
        DetailsMetric::IoRead | DetailsMetric::IoWrite => {
            let mbps = ((delta * 8.0) / 1_000_000.0).round() as i128;
            format_signed_integer(mbps) + " Mbps"
        }
        _ => format_signed_integer(delta.round() as i128),
    }
}

fn sample_ab_marker(
    comparison: Option<&crate::app::AbComparison>,
    captured_at: chrono::DateTime<chrono::Local>,
) -> &'static str {
    let Some(comparison) = comparison else {
        return "";
    };
    let is_a = comparison
        .a
        .is_some_and(|point| point.captured_at == captured_at);
    let is_b = comparison
        .b
        .is_some_and(|point| point.captured_at == captured_at);
    match (is_a, is_b) {
        (true, true) => "AB",
        (true, false) => "A",
        (false, true) => "B",
        (false, false) => "",
    }
}

fn format_optional_integer(value: Option<u64>) -> String {
    value
        .map(format_integer)
        .unwrap_or_else(|| "--".to_string())
}

#[cfg(not(test))]
fn copy_text_to_clipboard(value: &str) -> Result<()> {
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
fn copy_text_to_clipboard(value: &str) -> Result<()> {
    LAST_COPIED_TEXT.with(|last| {
        *last.borrow_mut() = Some(value.to_string());
    });
    Ok(())
}
