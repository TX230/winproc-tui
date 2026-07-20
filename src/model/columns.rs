use std::{cmp::Ordering, str::FromStr};

use crate::model::ProcessRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortColumn {
    Pid,
    ProcessName,
    Metric(MetricColumn),
}

impl SortColumn {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::ProcessName => "Process",
            Self::Metric(column) => column.label(),
        }
    }

    pub(crate) fn default_direction(self) -> SortDirection {
        match self {
            Self::Pid | Self::ProcessName => SortDirection::Asc,
            Self::Metric(MetricColumn::FullPath) => SortDirection::Asc,
            Self::Metric(_) => SortDirection::Desc,
        }
    }

    fn compare_values(self, left: &ProcessRow, right: &ProcessRow) -> Ordering {
        match self {
            Self::Pid => left.pid.cmp(&right.pid),
            Self::ProcessName => compare_process_names(&left.name, &right.name),
            Self::Metric(column) => column.compare_values(left, right),
        }
    }

    fn has_value(self, row: &ProcessRow) -> bool {
        match self {
            Self::Pid | Self::ProcessName => {
                let _ = row;
                true
            }
            Self::Metric(column) => column.has_value(row),
        }
    }
}

impl From<MetricColumn> for SortColumn {
    fn from(column: MetricColumn) -> Self {
        Self::Metric(column)
    }
}

impl FromStr for SortColumn {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "");
        match normalized.as_str() {
            "pid" | "processid" => Ok(Self::Pid),
            "process" | "name" | "processname" => Ok(Self::ProcessName),
            _ => value.parse::<MetricColumn>().map(Self::Metric),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricColumn {
    CpuPercent,
    PrivateBytes,
    WorksetBytes,
    WorksetPrivateBytes,
    WorksetShareableBytes,
    WorksetSharedBytes,
    ThreadCount,
    HandleCount,
    UserObjectCount,
    GdiObjectCount,
    GpuPercent,
    DotNetHeapBytes,
    GpuDedicatedBytes,
    GpuSharedBytes,
    IoReadBytesPerSec,
    IoWriteBytesPerSec,
    FullPath,
}

impl MetricColumn {
    pub(crate) const ALL: [Self; 15] = [
        Self::CpuPercent,
        Self::PrivateBytes,
        Self::WorksetBytes,
        Self::WorksetPrivateBytes,
        Self::ThreadCount,
        Self::HandleCount,
        Self::UserObjectCount,
        Self::GdiObjectCount,
        Self::GpuPercent,
        Self::DotNetHeapBytes,
        Self::GpuDedicatedBytes,
        Self::GpuSharedBytes,
        Self::IoReadBytesPerSec,
        Self::IoWriteBytesPerSec,
        Self::FullPath,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CpuPercent => "CPU%",
            Self::PrivateBytes => "Private",
            Self::WorksetBytes => "WS",
            Self::WorksetPrivateBytes => "WS Priv",
            Self::WorksetShareableBytes => "WS Shrbl",
            Self::WorksetSharedBytes => "WS Shrd",
            Self::ThreadCount => "Thrd",
            Self::HandleCount => "Hndl",
            Self::UserObjectCount => "USER",
            Self::GdiObjectCount => "GDI",
            Self::GpuPercent => "GPU%",
            Self::DotNetHeapBytes => ".NET Heap",
            Self::GpuDedicatedBytes => "GPU D",
            Self::GpuSharedBytes => "GPU S",
            Self::IoReadBytesPerSec => "IO Read/s",
            Self::IoWriteBytesPerSec => "IO Write/s",
            Self::FullPath => "Full Path",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::CpuPercent => "CPU usage as a percentage of total logical CPU capacity",
            Self::PrivateBytes => "Private committed memory, also known as Windows Commit size",
            Self::WorksetBytes => "Working set currently resident in RAM",
            Self::WorksetPrivateBytes => {
                "Private portion of the working set currently resident in RAM"
            }
            Self::WorksetShareableBytes => "Shareable pages in working set",
            Self::WorksetSharedBytes => "Actually shared pages in working set",
            Self::ThreadCount => "Number of threads in the process",
            Self::HandleCount => "Number of open handles in the process",
            Self::UserObjectCount => "USER objects such as windows and menus",
            Self::GdiObjectCount => "GDI objects such as pens, fonts, and bitmaps",
            Self::GpuPercent => "Per-process GPU engine utilization",
            Self::DotNetHeapBytes => ".NET CLR heap size, when available",
            Self::GpuDedicatedBytes => "Dedicated GPU memory used by the process",
            Self::GpuSharedBytes => "Shared system memory used by the process for GPU resources",
            Self::IoReadBytesPerSec => "I/O read throughput by the process (file/net/dev)",
            Self::IoWriteBytesPerSec => "I/O write throughput by the process (file/net/dev)",
            Self::FullPath => "Executable path, when available",
        }
    }

    pub(crate) fn width(self) -> u16 {
        match self {
            Self::CpuPercent => 7,
            Self::GpuPercent => 7,
            Self::ThreadCount
            | Self::HandleCount
            | Self::UserObjectCount
            | Self::GdiObjectCount => 8,
            Self::PrivateBytes
            | Self::WorksetBytes
            | Self::WorksetPrivateBytes
            | Self::WorksetShareableBytes
            | Self::WorksetSharedBytes
            | Self::DotNetHeapBytes
            | Self::GpuDedicatedBytes
            | Self::GpuSharedBytes => 13,
            Self::IoReadBytesPerSec | Self::IoWriteBytesPerSec => 12,
            Self::FullPath => 36,
        }
    }

    pub(crate) fn is_selectable(self) -> bool {
        Self::ALL.contains(&self)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn raw_value(self, row: &ProcessRow) -> Option<String> {
        match self {
            Self::CpuPercent => row.cpu_percent.map(|value| value.to_string()),
            Self::PrivateBytes => row.private_bytes.map(|value| value.to_string()),
            Self::WorksetBytes => row.workset_bytes.map(|value| value.to_string()),
            Self::WorksetPrivateBytes => row.workset_private_bytes.map(|value| value.to_string()),
            Self::WorksetShareableBytes => {
                row.workset_shareable_bytes.map(|value| value.to_string())
            }
            Self::WorksetSharedBytes => row.workset_shared_bytes.map(|value| value.to_string()),
            Self::ThreadCount => row.thread_count.map(|value| value.to_string()),
            Self::HandleCount => row.handle_count.map(|value| value.to_string()),
            Self::UserObjectCount => row.user_object_count.map(|value| value.to_string()),
            Self::GdiObjectCount => row.gdi_object_count.map(|value| value.to_string()),
            Self::GpuPercent => row.gpu_percent.map(|value| value.to_string()),
            Self::DotNetHeapBytes => row.dotnet_heap_bytes.map(|value| value.to_string()),
            Self::GpuDedicatedBytes => row.gpu_dedicated_bytes.map(|value| value.to_string()),
            Self::GpuSharedBytes => row.gpu_shared_bytes.map(|value| value.to_string()),
            Self::IoReadBytesPerSec => row.io_read_bytes_per_sec.map(|value| value.to_string()),
            Self::IoWriteBytesPerSec => row.io_write_bytes_per_sec.map(|value| value.to_string()),
            Self::FullPath => row.executable_path.clone(),
        }
    }

    fn compare_values(self, left: &ProcessRow, right: &ProcessRow) -> Ordering {
        match self {
            Self::CpuPercent => compare_optional_f64(left.cpu_percent, right.cpu_percent),
            Self::PrivateBytes => compare_optional_u64(left.private_bytes, right.private_bytes),
            Self::WorksetBytes => compare_optional_u64(left.workset_bytes, right.workset_bytes),
            Self::WorksetPrivateBytes => {
                compare_optional_u64(left.workset_private_bytes, right.workset_private_bytes)
            }
            Self::WorksetShareableBytes => {
                compare_optional_u64(left.workset_shareable_bytes, right.workset_shareable_bytes)
            }
            Self::WorksetSharedBytes => {
                compare_optional_u64(left.workset_shared_bytes, right.workset_shared_bytes)
            }
            Self::ThreadCount => compare_optional_u64(left.thread_count, right.thread_count),
            Self::HandleCount => compare_optional_u64(left.handle_count, right.handle_count),
            Self::UserObjectCount => {
                compare_optional_u64(left.user_object_count, right.user_object_count)
            }
            Self::GdiObjectCount => {
                compare_optional_u64(left.gdi_object_count, right.gdi_object_count)
            }
            Self::GpuPercent => compare_optional_f64(left.gpu_percent, right.gpu_percent),
            Self::DotNetHeapBytes => {
                compare_optional_u64(left.dotnet_heap_bytes, right.dotnet_heap_bytes)
            }
            Self::GpuDedicatedBytes => {
                compare_optional_u64(left.gpu_dedicated_bytes, right.gpu_dedicated_bytes)
            }
            Self::GpuSharedBytes => {
                compare_optional_u64(left.gpu_shared_bytes, right.gpu_shared_bytes)
            }
            Self::IoReadBytesPerSec => {
                compare_optional_u64(left.io_read_bytes_per_sec, right.io_read_bytes_per_sec)
            }
            Self::IoWriteBytesPerSec => {
                compare_optional_u64(left.io_write_bytes_per_sec, right.io_write_bytes_per_sec)
            }
            Self::FullPath => compare_optional_strings(
                left.executable_path.as_deref(),
                right.executable_path.as_deref(),
            ),
        }
    }

    fn has_value(self, row: &ProcessRow) -> bool {
        match self {
            Self::CpuPercent => row.cpu_percent.is_some(),
            Self::PrivateBytes => row.private_bytes.is_some(),
            Self::WorksetBytes => row.workset_bytes.is_some(),
            Self::WorksetPrivateBytes => row.workset_private_bytes.is_some(),
            Self::WorksetShareableBytes => row.workset_shareable_bytes.is_some(),
            Self::WorksetSharedBytes => row.workset_shared_bytes.is_some(),
            Self::ThreadCount => row.thread_count.is_some(),
            Self::HandleCount => row.handle_count.is_some(),
            Self::UserObjectCount => row.user_object_count.is_some(),
            Self::GdiObjectCount => row.gdi_object_count.is_some(),
            Self::GpuPercent => row.gpu_percent.is_some(),
            Self::DotNetHeapBytes => row.dotnet_heap_bytes.is_some(),
            Self::GpuDedicatedBytes => row.gpu_dedicated_bytes.is_some(),
            Self::GpuSharedBytes => row.gpu_shared_bytes.is_some(),
            Self::IoReadBytesPerSec => row.io_read_bytes_per_sec.is_some(),
            Self::IoWriteBytesPerSec => row.io_write_bytes_per_sec.is_some(),
            Self::FullPath => row.executable_path.is_some(),
        }
    }

    pub(crate) fn is_graphable(self) -> bool {
        !matches!(self, Self::FullPath)
    }
}

impl FromStr for MetricColumn {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "");
        match normalized.as_str() {
            "cpu%" | "cpu" | "cpupercent" => Ok(Self::CpuPercent),
            "private" | "privatebytes" => Ok(Self::PrivateBytes),
            "ws" | "workingset" | "workset" => Ok(Self::WorksetBytes),
            "wspriv" | "workingsetprivate" | "worksetprivate" => Ok(Self::WorksetPrivateBytes),
            "wsshrbl" | "wsshareable" | "worksetshareable" => Ok(Self::WorksetShareableBytes),
            "wsshrd" | "wsshared" | "worksetshared" => Ok(Self::WorksetSharedBytes),
            "thrd" | "thread" | "threads" | "threadcount" => Ok(Self::ThreadCount),
            "hndl" | "handle" | "handles" | "handlecount" => Ok(Self::HandleCount),
            "user" | "userobjects" | "userobjectcount" => Ok(Self::UserObjectCount),
            "gdi" | "gdiobjects" | "gdiobjectcount" => Ok(Self::GdiObjectCount),
            "gpu%" | "gpupercent" | "gpuutilization" | "gpuusage" => Ok(Self::GpuPercent),
            ".netheap" | "netheap" | "dotnetheap" => Ok(Self::DotNetHeapBytes),
            "gpud" | "gpudedicated" => Ok(Self::GpuDedicatedBytes),
            "gpus" | "gpushared" => Ok(Self::GpuSharedBytes),
            "ioread/s" | "ioreads" | "ioread" => Ok(Self::IoReadBytesPerSec),
            "iowrite/s" | "iowrites" | "iowrite" => Ok(Self::IoWriteBytesPerSec),
            "path" | "fullpath" | "exepath" | "executablepath" => Ok(Self::FullPath),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnPreset {
    Default,
    Memory,
    Resources,
    DotNet,
    Gpu,
    Io,
    Custom,
}

impl ColumnPreset {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Memory => "Memory",
            Self::Resources => "Resources",
            Self::DotNet => ".NET",
            Self::Gpu => "GPU",
            Self::Io => "IO",
            Self::Custom => "Custom",
        }
    }

    pub(crate) fn columns(self) -> &'static [MetricColumn] {
        match self {
            Self::Default => &MetricColumn::ALL,
            Self::Memory => &[
                MetricColumn::CpuPercent,
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetBytes,
                MetricColumn::WorksetPrivateBytes,
            ],
            Self::Resources => &[
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
                MetricColumn::ThreadCount,
                MetricColumn::HandleCount,
                MetricColumn::UserObjectCount,
                MetricColumn::GdiObjectCount,
            ],
            Self::DotNet => &[
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
                MetricColumn::DotNetHeapBytes,
                MetricColumn::HandleCount,
            ],
            Self::Gpu => &[
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
                MetricColumn::GpuPercent,
                MetricColumn::GpuDedicatedBytes,
                MetricColumn::GpuSharedBytes,
            ],
            Self::Io => &[
                MetricColumn::PrivateBytes,
                MetricColumn::WorksetPrivateBytes,
                MetricColumn::IoReadBytesPerSec,
                MetricColumn::IoWriteBytesPerSec,
            ],
            Self::Custom => &[],
        }
    }

    pub(crate) fn effective_columns(self) -> &'static [MetricColumn] {
        let columns = self.columns();
        if columns.is_empty() {
            Self::Default.columns()
        } else {
            columns
        }
    }
}

impl Default for ColumnPreset {
    fn default() -> Self {
        Self::Default
    }
}

impl FromStr for ColumnPreset {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "memory" => Ok(Self::Memory),
            "resources" => Ok(Self::Resources),
            ".net" | "dotnet" | "net" => Ok(Self::DotNet),
            "gpu" => Ok(Self::Gpu),
            "io" | "i/o" => Ok(Self::Io),
            "custom" => Ok(Self::Custom),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

impl FromStr for SortDirection {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "asc" | "ascending" => Ok(Self::Asc),
            "desc" | "descending" => Ok(Self::Desc),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortSpec {
    pub(crate) column: SortColumn,
    pub(crate) direction: SortDirection,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            column: SortColumn::Metric(MetricColumn::WorksetPrivateBytes),
            direction: SortDirection::Desc,
        }
    }
}

pub(crate) fn sort_process_rows(rows: &mut [ProcessRow], sort: SortSpec) {
    rows.sort_by(|left, right| {
        let value_ordering = match (sort.column.has_value(left), sort.column.has_value(right)) {
            (true, true) => {
                let ordering = sort.column.compare_values(left, right);
                match sort.direction {
                    SortDirection::Asc => ordering,
                    SortDirection::Desc => ordering.reverse(),
                }
            }
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => Ordering::Equal,
        };

        value_ordering
            .then_with(|| compare_process_names(&left.name, &right.name))
            .then_with(|| left.pid.cmp(&right.pid))
    });
}

fn compare_process_names(left: &str, right: &str) -> Ordering {
    left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
}

fn compare_optional_u64(left: Option<u64>, right: Option<u64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_f64(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_strings(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        pid: u32,
        name: &str,
        private_bytes: Option<u64>,
        ws_private: Option<u64>,
    ) -> ProcessRow {
        ProcessRow {
            pid,
            name: name.to_string(),
            executable_path: None,
            start_time: Some(1_700_000_000 + u64::from(pid)),
            cpu_percent: None,
            private_bytes,
            workset_bytes: None,
            workset_private_bytes: ws_private,
            workset_shareable_bytes: None,
            workset_shared_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }

    #[test]
    fn default_preset_selects_all_columns() {
        assert_eq!(
            ColumnPreset::Default.effective_columns(),
            &MetricColumn::ALL
        );
    }

    #[test]
    fn custom_preset_effective_columns_fall_back_to_default() {
        assert_eq!(
            ColumnPreset::Custom.effective_columns(),
            ColumnPreset::Default.columns()
        );
    }

    #[test]
    fn selectable_columns_exclude_working_set_share_columns() {
        assert!(!MetricColumn::ALL.contains(&MetricColumn::WorksetShareableBytes));
        assert!(!MetricColumn::ALL.contains(&MetricColumn::WorksetSharedBytes));
        assert!(
            !ColumnPreset::Memory
                .columns()
                .contains(&MetricColumn::WorksetShareableBytes)
        );
        assert!(
            !ColumnPreset::Memory
                .columns()
                .contains(&MetricColumn::WorksetSharedBytes)
        );
    }

    #[test]
    fn full_path_column_is_selectable_but_not_graphable() {
        assert!(MetricColumn::ALL.contains(&MetricColumn::FullPath));
        assert!(MetricColumn::FullPath.is_selectable());
        assert!(!MetricColumn::FullPath.is_graphable());
        assert_eq!(
            SortColumn::Metric(MetricColumn::FullPath).default_direction(),
            SortDirection::Asc
        );
    }

    #[test]
    fn raw_value_returns_unformatted_metric_values() {
        let row = ProcessRow {
            pid: 1,
            name: "app.exe".to_string(),
            executable_path: Some(r"C:\work\app.exe".to_string()),
            start_time: Some(1_700_000_000),
            cpu_percent: Some(12.3),
            private_bytes: Some(1001),
            workset_bytes: Some(1002),
            workset_private_bytes: Some(1003),
            workset_shareable_bytes: Some(1004),
            workset_shared_bytes: Some(1005),
            thread_count: Some(6),
            handle_count: Some(7),
            user_object_count: Some(8),
            gdi_object_count: Some(9),
            gpu_percent: Some(10.5),
            dotnet_heap_bytes: Some(1011),
            gpu_dedicated_bytes: Some(1012),
            gpu_shared_bytes: Some(1013),
            io_read_bytes_per_sec: Some(1014),
            io_write_bytes_per_sec: Some(1015),
        };

        assert_eq!(
            MetricColumn::CpuPercent.raw_value(&row).as_deref(),
            Some("12.3")
        );
        assert_eq!(
            MetricColumn::PrivateBytes.raw_value(&row).as_deref(),
            Some("1001")
        );
        assert_eq!(
            MetricColumn::WorksetBytes.raw_value(&row).as_deref(),
            Some("1002")
        );
        assert_eq!(
            MetricColumn::WorksetPrivateBytes.raw_value(&row).as_deref(),
            Some("1003")
        );
        assert_eq!(
            MetricColumn::WorksetShareableBytes
                .raw_value(&row)
                .as_deref(),
            Some("1004")
        );
        assert_eq!(
            MetricColumn::WorksetSharedBytes.raw_value(&row).as_deref(),
            Some("1005")
        );
        assert_eq!(
            MetricColumn::ThreadCount.raw_value(&row).as_deref(),
            Some("6")
        );
        assert_eq!(
            MetricColumn::HandleCount.raw_value(&row).as_deref(),
            Some("7")
        );
        assert_eq!(
            MetricColumn::UserObjectCount.raw_value(&row).as_deref(),
            Some("8")
        );
        assert_eq!(
            MetricColumn::GdiObjectCount.raw_value(&row).as_deref(),
            Some("9")
        );
        assert_eq!(
            MetricColumn::GpuPercent.raw_value(&row).as_deref(),
            Some("10.5")
        );
        assert_eq!(
            MetricColumn::DotNetHeapBytes.raw_value(&row).as_deref(),
            Some("1011")
        );
        assert_eq!(
            MetricColumn::GpuDedicatedBytes.raw_value(&row).as_deref(),
            Some("1012")
        );
        assert_eq!(
            MetricColumn::GpuSharedBytes.raw_value(&row).as_deref(),
            Some("1013")
        );
        assert_eq!(
            MetricColumn::IoReadBytesPerSec.raw_value(&row).as_deref(),
            Some("1014")
        );
        assert_eq!(
            MetricColumn::IoWriteBytesPerSec.raw_value(&row).as_deref(),
            Some("1015")
        );
        assert_eq!(
            MetricColumn::FullPath.raw_value(&row).as_deref(),
            Some(r"C:\work\app.exe")
        );
    }

    #[test]
    fn raw_value_returns_none_for_missing_metric_value() {
        let row = row(1, "app.exe", Some(10), None);

        assert_eq!(MetricColumn::WorksetPrivateBytes.raw_value(&row), None);
    }

    #[test]
    fn sort_process_rows_keeps_missing_values_last_in_desc_order() {
        let mut rows = vec![
            row(1, "none", Some(10), None),
            row(2, "small", Some(20), Some(100)),
            row(3, "large", Some(30), Some(300)),
        ];

        sort_process_rows(&mut rows, SortSpec::default());

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["large", "small", "none"]
        );
    }

    #[test]
    fn sort_process_rows_ignores_case_for_process_names() {
        let mut rows = vec![
            row(3, "beta.exe", Some(10), Some(10)),
            row(2, "Alpha.exe", Some(20), Some(20)),
            row(1, "alpha.exe", Some(30), Some(30)),
        ];

        sort_process_rows(
            &mut rows,
            SortSpec {
                column: SortColumn::ProcessName,
                direction: SortDirection::Asc,
            },
        );

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha.exe", "Alpha.exe", "beta.exe"]
        );
    }

    #[test]
    fn sort_process_rows_uses_case_insensitive_name_tie_breaker() {
        let mut rows = vec![
            row(3, "beta.exe", Some(30), Some(100)),
            row(2, "Alpha.exe", Some(20), Some(100)),
            row(1, "alpha.exe", Some(10), Some(100)),
        ];

        sort_process_rows(
            &mut rows,
            SortSpec {
                column: SortColumn::Metric(MetricColumn::WorksetPrivateBytes),
                direction: SortDirection::Desc,
            },
        );

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha.exe", "Alpha.exe", "beta.exe"]
        );
    }
}
