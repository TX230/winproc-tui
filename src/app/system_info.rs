use sysinfo::System;

use crate::{
    App,
    model::{DiskUsageSample, GpuAdapterSample},
    ui::format::{format_compact_bytes, format_frequency_mhz},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SystemInfoHost {
    pub(crate) windows_version: Option<String>,
    pub(crate) windows_build: Option<String>,
    pub(crate) architecture: Option<String>,
}

impl SystemInfoHost {
    pub(crate) fn collect() -> Self {
        Self {
            windows_version: System::long_os_version(),
            windows_build: System::kernel_version(),
            architecture: normalize_architecture(System::cpu_arch()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemInfoValueStyle {
    Plain,
    Measurement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SystemInfoField {
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) value_style: SystemInfoValueStyle,
}

impl SystemInfoField {
    fn plain(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style: SystemInfoValueStyle::Plain,
        }
    }

    fn measurement(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style: SystemInfoValueStyle::Measurement,
        }
    }

    pub(crate) fn text(&self) -> String {
        format!("{}: {}", self.label, self.value)
    }
}

pub(crate) fn system_info_fields(app: &App) -> Vec<SystemInfoField> {
    // System Info describes the current host, not the paused or loaded-log display. The latest
    // live snapshot remains available in all activities and is refreshed only by the sampler.
    let snapshot = &app.snapshot;
    let mut fields = vec![
        SystemInfoField::plain("winproc-tui", env!("CARGO_PKG_VERSION")),
        SystemInfoField::plain(
            "Windows",
            optional_text(app.system_info_host.windows_version.as_deref()),
        ),
        SystemInfoField::measurement(
            "Build",
            optional_text(app.system_info_host.windows_build.as_deref()),
        ),
        SystemInfoField::plain(
            "Architecture",
            optional_text(app.system_info_host.architecture.as_deref()),
        ),
        SystemInfoField::plain(
            "CPU",
            format_cpu_summary(
                snapshot.cpu_name.as_deref().unwrap_or("--"),
                snapshot.cpu_frequency_mhz,
            ),
        ),
        SystemInfoField::plain("Cores", snapshot.cpu_topology.as_deref().unwrap_or("--")),
        SystemInfoField::measurement("Cache", snapshot.cpu_cache.as_deref().unwrap_or("--")),
        SystemInfoField::measurement(
            "Physical memory",
            format_optional_capacity((snapshot.total_memory > 0).then_some(snapshot.total_memory)),
        ),
        SystemInfoField::measurement(
            "Commit limit",
            format_optional_capacity(snapshot.commit_limit),
        ),
    ];
    push_gpu_fields(&mut fields, &snapshot.gpu_adapters);
    push_disk_fields(&mut fields, &snapshot.disks);
    fields
}

pub(crate) fn system_info_plain_text(app: &App) -> String {
    system_info_fields(app)
        .iter()
        .map(SystemInfoField::text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_gpu_fields(fields: &mut Vec<SystemInfoField>, adapters: &[GpuAdapterSample]) {
    if adapters.is_empty() {
        fields.push(SystemInfoField::plain("GPU", "--"));
        return;
    }

    for (index, adapter) in adapters.iter().enumerate() {
        fields.push(SystemInfoField::measurement(
            format!("GPU {}", index + 1),
            format!(
                "{} / Dedicated {} / Shared {}",
                adapter.name.as_deref().unwrap_or("--"),
                format_optional_capacity(adapter.dedicated_total),
                format_optional_capacity(adapter.shared_total),
            ),
        ));
    }
}

fn push_disk_fields(fields: &mut Vec<SystemInfoField>, disks: &[DiskUsageSample]) {
    if disks.is_empty() {
        fields.push(SystemInfoField::measurement("Disk", "--"));
        return;
    }

    for disk in disks {
        fields.push(SystemInfoField::measurement(
            format!("Disk {}", disk.name.trim_end_matches(':')),
            format!(
                "{} free / {} total",
                format_compact_bytes(disk.free_bytes),
                format_compact_bytes(disk.total_bytes),
            ),
        ));
    }
}

fn format_cpu_summary(name: &str, frequency_mhz: Option<u64>) -> String {
    match frequency_mhz {
        Some(_) => format!("{name} / {}", format_frequency_mhz(frequency_mhz)),
        None => name.to_string(),
    }
}

fn format_optional_capacity(value: Option<u64>) -> String {
    value
        .map(format_compact_bytes)
        .unwrap_or_else(|| "--".to_string())
}

fn optional_text(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("--")
        .to_string()
}

fn normalize_architecture(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        match value.to_ascii_lowercase().as_str() {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "ARM64",
            "x86" | "i386" | "i686" | "ia32" => "x86",
            _ => value,
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_names_use_windows_facing_labels() {
        assert_eq!(
            normalize_architecture("x86_64".to_string()).as_deref(),
            Some("x64")
        );
        assert_eq!(
            normalize_architecture("arm64".to_string()).as_deref(),
            Some("ARM64")
        );
        assert_eq!(normalize_architecture(String::new()), None);
    }
}
