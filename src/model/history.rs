use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Local};

use crate::model::{ProcessRow, Snapshot};

pub(crate) const GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY: usize = 120;
pub(crate) const TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY: usize = 7_200;
const SYSTEM_HISTORY_SAMPLE_CAPACITY: usize = TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SystemMetric {
    CpuAverage,
    PhysicalMemory,
    ModifiedMemory,
    StandbyMemory,
    FreeZeroedMemory,
    Committed,
    PagedPool,
    NonpagedPool,
    PagesInput,
    PagesOutput,
    ThreadCount,
    ProcessCount,
    GpuUtilization,
    GpuEncode,
    GpuDecode,
    GpuDedicated,
    GpuShared,
    NetworkReceived,
    NetworkSent,
    DiskRead,
    DiskWrite,
    DiskQueueLength,
}

impl SystemMetric {
    pub(crate) const MEMORY_OVERVIEW_PANEL: [Self; 5] = [
        Self::PhysicalMemory,
        Self::ModifiedMemory,
        Self::StandbyMemory,
        Self::FreeZeroedMemory,
        Self::Committed,
    ];
    pub(crate) const MEMORY_PRESSURE_PANEL: [Self; 4] = [
        Self::PagedPool,
        Self::NonpagedPool,
        Self::PagesInput,
        Self::PagesOutput,
    ];
    pub(crate) const MEMORY_PANEL: [Self; 9] = [
        Self::PhysicalMemory,
        Self::ModifiedMemory,
        Self::StandbyMemory,
        Self::FreeZeroedMemory,
        Self::Committed,
        Self::PagedPool,
        Self::NonpagedPool,
        Self::PagesInput,
        Self::PagesOutput,
    ];
    pub(crate) const CPU_PANEL: [Self; 3] =
        [Self::CpuAverage, Self::ThreadCount, Self::ProcessCount];
    pub(crate) const GPU_PANEL: [Self; 5] = [
        Self::GpuUtilization,
        Self::GpuEncode,
        Self::GpuDecode,
        Self::GpuDedicated,
        Self::GpuShared,
    ];
    pub(crate) const SYSTEM_ACTIVITY_PANEL: [Self; 5] = [
        Self::NetworkReceived,
        Self::NetworkSent,
        Self::DiskRead,
        Self::DiskWrite,
        Self::DiskQueueLength,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CpuAverage => "CPU Usage",
            Self::PhysicalMemory => "In use",
            Self::ModifiedMemory => "Modified",
            Self::StandbyMemory => "Standby",
            Self::FreeZeroedMemory => "Free + Zeroed",
            Self::Committed => "Commit charge",
            Self::PagedPool => "Paged Pool",
            Self::NonpagedPool => "Nonpaged Pool",
            Self::PagesInput => "Pages In/s",
            Self::PagesOutput => "Pages Out/s",
            Self::ThreadCount => "Threads",
            Self::ProcessCount => "Processes",
            Self::GpuUtilization => "Usage",
            Self::GpuEncode => "Encode",
            Self::GpuDecode => "Decode",
            Self::GpuDedicated => "GPU Dedicated",
            Self::GpuShared => "GPU Shared",
            Self::NetworkReceived => "Net Rx",
            Self::NetworkSent => "Net Tx",
            Self::DiskRead => "Disk R",
            Self::DiskWrite => "Disk W",
            Self::DiskQueueLength => "Disk Q",
        }
    }

    pub(crate) fn panel_label(self) -> &'static str {
        match self {
            Self::CpuAverage | Self::ThreadCount | Self::ProcessCount => "CPU",
            Self::PhysicalMemory
            | Self::ModifiedMemory
            | Self::StandbyMemory
            | Self::FreeZeroedMemory
            | Self::Committed
            | Self::PagedPool
            | Self::NonpagedPool
            | Self::PagesInput
            | Self::PagesOutput => "MEM",
            Self::GpuUtilization
            | Self::GpuEncode
            | Self::GpuDecode
            | Self::GpuDedicated
            | Self::GpuShared => "GPU",
            Self::NetworkReceived
            | Self::NetworkSent
            | Self::DiskRead
            | Self::DiskWrite
            | Self::DiskQueueLength => "System Activity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) start_time: Option<u64>,
}

impl ProcessIdentity {
    pub(crate) fn from_row(row: &ProcessRow) -> Self {
        Self {
            pid: row.pid,
            name: row.name.clone(),
            start_time: row.start_time,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessSample {
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) private_bytes: Option<u64>,
    pub(crate) workset_bytes: Option<u64>,
    pub(crate) workset_private_bytes: Option<u64>,
    pub(crate) workset_shareable_bytes: Option<u64>,
    pub(crate) thread_count: Option<u64>,
    pub(crate) handle_count: Option<u64>,
    pub(crate) user_object_count: Option<u64>,
    pub(crate) gdi_object_count: Option<u64>,
    pub(crate) gpu_percent: Option<f64>,
    pub(crate) dotnet_heap_bytes: Option<u64>,
    pub(crate) dotnet_gc_gen0_heap_bytes: Option<u64>,
    pub(crate) dotnet_gc_gen1_heap_bytes: Option<u64>,
    pub(crate) dotnet_gc_gen2_heap_bytes: Option<u64>,
    pub(crate) dotnet_gc_loh_bytes: Option<u64>,
    pub(crate) dotnet_gc_poh_bytes: Option<u64>,
    pub(crate) dotnet_gc_committed_bytes: Option<u64>,
    pub(crate) dotnet_gc_fragmentation_bytes: Option<u64>,
    pub(crate) dotnet_allocation_bytes_per_sec: Option<u64>,
    pub(crate) gpu_dedicated_bytes: Option<u64>,
    pub(crate) gpu_shared_bytes: Option<u64>,
    pub(crate) io_read_bytes_per_sec: Option<u64>,
    pub(crate) io_write_bytes_per_sec: Option<u64>,
}

impl ProcessSample {
    pub(crate) fn from_row(captured_at: DateTime<Local>, row: &ProcessRow) -> Self {
        Self {
            captured_at,
            cpu_percent: row.cpu_percent,
            private_bytes: row.private_bytes,
            workset_bytes: row.workset_bytes,
            workset_private_bytes: row.workset_private_bytes,
            workset_shareable_bytes: row.workset_shareable_bytes,
            thread_count: row.thread_count,
            handle_count: row.handle_count,
            user_object_count: row.user_object_count,
            gdi_object_count: row.gdi_object_count,
            gpu_percent: row.gpu_percent,
            dotnet_heap_bytes: row.dotnet_heap_bytes,
            dotnet_gc_gen0_heap_bytes: row.dotnet_gc_gen0_heap_bytes,
            dotnet_gc_gen1_heap_bytes: row.dotnet_gc_gen1_heap_bytes,
            dotnet_gc_gen2_heap_bytes: row.dotnet_gc_gen2_heap_bytes,
            dotnet_gc_loh_bytes: row.dotnet_gc_loh_bytes,
            dotnet_gc_poh_bytes: row.dotnet_gc_poh_bytes,
            dotnet_gc_committed_bytes: row.dotnet_gc_committed_bytes,
            dotnet_gc_fragmentation_bytes: row.dotnet_gc_fragmentation_bytes,
            dotnet_allocation_bytes_per_sec: row.dotnet_allocation_bytes_per_sec,
            gpu_dedicated_bytes: row.gpu_dedicated_bytes,
            gpu_shared_bytes: row.gpu_shared_bytes,
            io_read_bytes_per_sec: row.io_read_bytes_per_sec,
            io_write_bytes_per_sec: row.io_write_bytes_per_sec,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessPeak {
    pub(crate) private_bytes: Option<u64>,
    pub(crate) workset_private_bytes: Option<u64>,
}

impl ProcessPeak {
    fn record(&mut self, sample: &ProcessSample) {
        self.private_bytes = max_option(self.private_bytes, sample.private_bytes);
        self.workset_private_bytes =
            max_option(self.workset_private_bytes, sample.workset_private_bytes);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessHistory {
    samples: HashMap<ProcessIdentity, VecDeque<ProcessSample>>,
    peaks: HashMap<ProcessIdentity, ProcessPeak>,
}

impl ProcessHistory {
    pub(crate) fn record_snapshot(
        &mut self,
        captured_at: DateTime<Local>,
        processes: &[ProcessRow],
        tracked_names: &HashSet<String>,
    ) {
        for process in processes {
            let capacity = if tracked_names.contains(&process.name.to_ascii_lowercase()) {
                TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY
            } else {
                GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY
            };
            self.record_process_sample(captured_at, process, Some(capacity));
        }
    }

    pub(crate) fn record_snapshot_unbounded(
        &mut self,
        captured_at: DateTime<Local>,
        processes: &[ProcessRow],
    ) {
        for process in processes {
            self.record_process_sample(captured_at, process, None);
        }
    }

    fn record_process_sample(
        &mut self,
        captured_at: DateTime<Local>,
        process: &ProcessRow,
        capacity: Option<usize>,
    ) {
        let identity = ProcessIdentity::from_row(process);
        let sample = ProcessSample::from_row(captured_at, process);
        self.peaks
            .entry(identity.clone())
            .or_default()
            .record(&sample);
        let samples = self.samples.entry(identity).or_default();
        samples.push_back(sample);
        if let Some(capacity) = capacity {
            while samples.len() > capacity {
                samples.pop_front();
            }
        }
    }

    pub(crate) fn samples_for(&self, identity: &ProcessIdentity) -> Vec<&ProcessSample> {
        self.samples
            .get(identity)
            .map(|samples| samples.iter().collect())
            .unwrap_or_default()
    }

    pub(crate) fn time_range_for(
        &self,
        identity: &ProcessIdentity,
    ) -> Option<(DateTime<Local>, DateTime<Local>)> {
        let samples = self.samples.get(identity)?;
        Some((samples.front()?.captured_at, samples.back()?.captured_at))
    }

    pub(crate) fn sample_at(
        &self,
        identity: &ProcessIdentity,
        captured_at: DateTime<Local>,
    ) -> Option<&ProcessSample> {
        self.samples
            .get(identity)?
            .iter()
            .find(|sample| sample.captured_at == captured_at)
    }

    #[cfg(test)]
    pub(crate) fn sample_count_for(&self, identity: &ProcessIdentity) -> usize {
        self.samples
            .get(identity)
            .map(VecDeque::len)
            .unwrap_or_default()
    }

    pub(crate) fn prune_summary_for_name(
        &self,
        name: &str,
        retained_samples: usize,
    ) -> (usize, usize) {
        let normalized = name.to_ascii_lowercase();
        self.samples
            .iter()
            .filter(|(identity, _)| identity.name.eq_ignore_ascii_case(&normalized))
            .fold((0, 0), |(total, discarded), (_, samples)| {
                (
                    total + samples.len(),
                    discarded + samples.len().saturating_sub(retained_samples),
                )
            })
    }

    pub(crate) fn prune_name_to_latest(&mut self, name: &str, retained_samples: usize) -> usize {
        let normalized = name.to_ascii_lowercase();
        let mut discarded = 0;
        for (identity, samples) in &mut self.samples {
            if !identity.name.eq_ignore_ascii_case(&normalized) {
                continue;
            }
            let excess = samples.len().saturating_sub(retained_samples);
            if excess > 0 {
                samples.drain(0..excess);
                discarded += excess;
            }
        }
        discarded
    }

    pub(crate) fn retain_identities_or_recent(
        &mut self,
        retained: &HashSet<ProcessIdentity>,
        recent_after: DateTime<Local>,
    ) {
        self.samples.retain(|identity, samples| {
            retained.contains(identity)
                || samples
                    .back()
                    .is_some_and(|sample| sample.captured_at > recent_after)
        });
        self.peaks
            .retain(|identity, _| self.samples.contains_key(identity));
    }

    pub(crate) fn peak_for(&self, identity: &ProcessIdentity) -> Option<&ProcessPeak> {
        self.peaks.get(identity)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.samples.values().map(VecDeque::len).sum()
    }

    #[cfg(test)]
    pub(crate) fn identity_count(&self) -> usize {
        self.samples.len()
    }

    #[cfg(test)]
    pub(crate) fn peak_count(&self) -> usize {
        self.peaks.len()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SystemSample {
    #[allow(dead_code)]
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) cpu_average_percent: Option<u64>,
    pub(crate) physical_memory_bytes: Option<u64>,
    pub(crate) modified_memory_bytes: Option<u64>,
    pub(crate) standby_memory_bytes: Option<u64>,
    pub(crate) free_zeroed_memory_bytes: Option<u64>,
    pub(crate) committed_bytes: Option<u64>,
    pub(crate) paged_pool_bytes: Option<u64>,
    pub(crate) nonpaged_pool_bytes: Option<u64>,
    pub(crate) pages_input_per_sec: Option<u64>,
    pub(crate) pages_output_per_sec: Option<u64>,
    pub(crate) thread_count: Option<u64>,
    pub(crate) process_count: Option<u64>,
    pub(crate) gpu_adapters: Vec<crate::model::GpuAdapterSample>,
    pub(crate) network_received_bytes_per_sec: Option<u64>,
    pub(crate) network_sent_bytes_per_sec: Option<u64>,
    pub(crate) disk_read_bytes_per_sec: Option<u64>,
    pub(crate) disk_write_bytes_per_sec: Option<u64>,
    pub(crate) disk_queue_length: Option<f64>,
}

impl SystemSample {
    pub(crate) fn from_snapshot(snapshot: &Snapshot) -> Self {
        Self {
            captured_at: snapshot.captured_at,
            cpu_average_percent: snapshot.cpu_total_usage_percent.map(u64::from),
            physical_memory_bytes: Some(snapshot.used_memory),
            modified_memory_bytes: snapshot.modified_memory,
            standby_memory_bytes: snapshot.standby_memory,
            free_zeroed_memory_bytes: snapshot.free_zeroed_memory,
            committed_bytes: snapshot.committed_memory,
            paged_pool_bytes: snapshot.paged_pool_memory,
            nonpaged_pool_bytes: snapshot.nonpaged_pool_memory,
            pages_input_per_sec: snapshot.pages_input_per_sec,
            pages_output_per_sec: snapshot.pages_output_per_sec,
            thread_count: snapshot.thread_count,
            process_count: u64::try_from(snapshot.process_count).ok(),
            gpu_adapters: snapshot.gpu_adapters.clone(),
            network_received_bytes_per_sec: snapshot.network_received_bytes_per_sec,
            network_sent_bytes_per_sec: snapshot.network_sent_bytes_per_sec,
            disk_read_bytes_per_sec: snapshot.disk_read_bytes_per_sec,
            disk_write_bytes_per_sec: snapshot.disk_write_bytes_per_sec,
            disk_queue_length: snapshot.disk_queue_length,
        }
    }

    pub(crate) fn value(&self, metric: SystemMetric) -> Option<f64> {
        match metric {
            SystemMetric::CpuAverage => self.cpu_average_percent.map(|value| value as f64),
            SystemMetric::PhysicalMemory => self.physical_memory_bytes.map(|value| value as f64),
            SystemMetric::ModifiedMemory => self.modified_memory_bytes.map(|value| value as f64),
            SystemMetric::StandbyMemory => self.standby_memory_bytes.map(|value| value as f64),
            SystemMetric::FreeZeroedMemory => {
                self.free_zeroed_memory_bytes.map(|value| value as f64)
            }
            SystemMetric::Committed => self.committed_bytes.map(|value| value as f64),
            SystemMetric::PagedPool => self.paged_pool_bytes.map(|value| value as f64),
            SystemMetric::NonpagedPool => self.nonpaged_pool_bytes.map(|value| value as f64),
            SystemMetric::PagesInput => self.pages_input_per_sec.map(|value| value as f64),
            SystemMetric::PagesOutput => self.pages_output_per_sec.map(|value| value as f64),
            SystemMetric::ThreadCount => self.thread_count.map(|value| value as f64),
            SystemMetric::ProcessCount => self.process_count.map(|value| value as f64),
            SystemMetric::GpuUtilization
            | SystemMetric::GpuEncode
            | SystemMetric::GpuDecode
            | SystemMetric::GpuDedicated
            | SystemMetric::GpuShared => None,
            SystemMetric::NetworkReceived => self
                .network_received_bytes_per_sec
                .map(|value| value as f64),
            SystemMetric::NetworkSent => self.network_sent_bytes_per_sec.map(|value| value as f64),
            SystemMetric::DiskRead => self.disk_read_bytes_per_sec.map(|value| value as f64),
            SystemMetric::DiskWrite => self.disk_write_bytes_per_sec.map(|value| value as f64),
            SystemMetric::DiskQueueLength => self.disk_queue_length,
        }
    }

    pub(crate) fn gpu_value(
        &self,
        adapter_id: crate::model::GpuAdapterId,
        metric: SystemMetric,
    ) -> Option<f64> {
        let adapter = self
            .gpu_adapters
            .iter()
            .find(|adapter| adapter.id == adapter_id)?;
        match metric {
            SystemMetric::GpuUtilization => adapter.utilization_percent,
            SystemMetric::GpuEncode => adapter.encode.average_percent,
            SystemMetric::GpuDecode => adapter.decode.average_percent,
            SystemMetric::GpuDedicated => adapter.dedicated_used.map(|value| value as f64),
            SystemMetric::GpuShared => adapter.shared_used.map(|value| value as f64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SystemHistory {
    samples: Vec<SystemSample>,
}

impl SystemHistory {
    pub(crate) fn record_snapshot(&mut self, snapshot: &Snapshot) {
        self.samples.push(SystemSample::from_snapshot(snapshot));
        self.prune();
    }

    pub(crate) fn record_snapshot_unbounded(&mut self, snapshot: &Snapshot) {
        self.samples.push(SystemSample::from_snapshot(snapshot));
    }

    pub(crate) fn samples(&self) -> &[SystemSample] {
        &self.samples
    }

    pub(crate) fn time_range(&self) -> Option<(DateTime<Local>, DateTime<Local>)> {
        Some((
            self.samples.first()?.captured_at,
            self.samples.last()?.captured_at,
        ))
    }

    fn prune(&mut self) {
        let excess = self
            .samples
            .len()
            .saturating_sub(SYSTEM_HISTORY_SAMPLE_CAPACITY);
        if excess > 0 {
            self.samples.drain(0..excess);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }
}

fn max_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_tracked_names() -> HashSet<String> {
        HashSet::new()
    }

    fn tracked_names(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn row(pid: u32, name: &str, private_bytes: u64) -> ProcessRow {
        ProcessRow {
            pid,
            name: name.to_string(),
            executable_path: None,
            start_time: Some(1_700_000_000 + u64::from(pid)),
            cpu_percent: None,
            private_bytes: Some(private_bytes),
            workset_bytes: None,
            workset_private_bytes: Some(private_bytes / 2),
            workset_shareable_bytes: None,
            thread_count: None,
            handle_count: None,
            user_object_count: None,
            gdi_object_count: None,
            gpu_percent: None,
            gpu_dedicated_bytes: None,
            gpu_shared_bytes: None,
            dotnet_heap_bytes: None,
            dotnet_gc_gen0_heap_bytes: None,
            dotnet_gc_gen1_heap_bytes: None,
            dotnet_gc_gen2_heap_bytes: None,
            dotnet_gc_loh_bytes: None,
            dotnet_gc_poh_bytes: None,
            dotnet_gc_committed_bytes: None,
            dotnet_gc_fragmentation_bytes: None,
            dotnet_allocation_bytes_per_sec: None,
            io_read_bytes_per_sec: None,
            io_write_bytes_per_sec: None,
        }
    }

    #[test]
    fn process_history_keeps_last_120_samples_for_general_processes() {
        let now = Local::now();
        let mut history = ProcessHistory::default();

        for offset in 0..(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY + 1) {
            history.record_snapshot(
                now + chrono::Duration::seconds(offset as i64),
                &[row(1, "app.exe", offset as u64)],
                &empty_tracked_names(),
            );
        }

        assert_eq!(history.len(), GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
        let samples = history.samples_for(&ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_001),
        });
        assert_eq!(samples[0].private_bytes, Some(1));
    }

    #[test]
    fn process_history_keeps_last_7200_samples_for_tracked_processes() {
        let now = Local::now();
        let mut history = ProcessHistory::default();

        for offset in 0..7_201 {
            history.record_snapshot(
                now + chrono::Duration::seconds(offset),
                &[row(1, "app.exe", offset as u64)],
                &tracked_names(&["app.exe"]),
            );
        }

        assert_eq!(history.len(), TRACKED_PROCESS_HISTORY_SAMPLE_CAPACITY);
        let samples = history.samples_for(&ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_001),
        });
        assert_eq!(samples[0].private_bytes, Some(1));
    }

    #[test]
    fn process_history_uses_pid_and_name_identity() {
        let now = Local::now();
        let mut history = ProcessHistory::default();
        history.record_snapshot(
            now,
            &[row(1, "app.exe", 10), row(1, "other.exe", 20)],
            &empty_tracked_names(),
        );

        let samples = history.samples_for(&ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_001),
        });

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].private_bytes, Some(10));
    }

    #[test]
    fn process_history_keeps_peak_after_sample_prune() {
        let now = Local::now();
        let identity = ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(1_700_000_001),
        };
        let mut history = ProcessHistory::default();

        history.record_snapshot(
            now - chrono::Duration::seconds(61),
            &[row(1, "app.exe", 40)],
            &empty_tracked_names(),
        );
        history.record_snapshot(now, &[row(1, "app.exe", 20)], &empty_tracked_names());

        let peak = history.peak_for(&identity).expect("peak should be tracked");
        assert_eq!(peak.private_bytes, Some(40));
        assert_eq!(peak.workset_private_bytes, Some(20));
    }

    #[test]
    fn process_history_separates_pid_reuse_by_start_time() {
        let now = Local::now();
        let mut first = row(1, "app.exe", 10);
        first.start_time = Some(100);
        let mut second = row(1, "app.exe", 20);
        second.start_time = Some(200);
        let mut history = ProcessHistory::default();

        history.record_snapshot(now, &[first, second], &empty_tracked_names());

        let samples = history.samples_for(&ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(100),
        });
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].private_bytes, Some(10));
    }

    #[test]
    fn process_history_exact_lookup_requires_time_and_identity_match() {
        let now = Local::now();
        let later = now + chrono::Duration::seconds(1);
        let mut first = row(1, "app.exe", 10);
        first.start_time = Some(100);
        let mut restarted = row(1, "app.exe", 20);
        restarted.start_time = Some(200);
        let mut history = ProcessHistory::default();
        history.record_snapshot(now, &[first], &empty_tracked_names());
        history.record_snapshot(later, &[restarted], &empty_tracked_names());

        let first_identity = ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(100),
        };
        let restarted_identity = ProcessIdentity {
            start_time: Some(200),
            ..first_identity.clone()
        };

        assert_eq!(
            history
                .sample_at(&first_identity, now)
                .and_then(|sample| sample.private_bytes),
            Some(10)
        );
        assert!(history.sample_at(&first_identity, later).is_none());
        assert!(history.sample_at(&restarted_identity, now).is_none());
        assert!(
            history
                .sample_at(&first_identity, now + chrono::Duration::milliseconds(1))
                .is_none()
        );
    }

    #[test]
    fn process_history_retain_keeps_explicit_and_recent_identities() {
        let now = Local::now();
        let retained = ProcessIdentity {
            pid: 1,
            name: "keep.exe".to_string(),
            start_time: Some(100),
        };
        let recent = ProcessIdentity {
            pid: 2,
            name: "recent.exe".to_string(),
            start_time: Some(200),
        };
        let stale = ProcessIdentity {
            pid: 3,
            name: "stale.exe".to_string(),
            start_time: Some(300),
        };
        let mut retained_row = row(1, "keep.exe", 10);
        retained_row.start_time = retained.start_time;
        let mut recent_row = row(2, "recent.exe", 20);
        recent_row.start_time = recent.start_time;
        let mut stale_row = row(3, "stale.exe", 30);
        stale_row.start_time = stale.start_time;
        let mut history = ProcessHistory::default();
        history.record_snapshot(
            now - chrono::Duration::seconds(121),
            &[retained_row, stale_row],
            &empty_tracked_names(),
        );
        history.record_snapshot(
            now - chrono::Duration::seconds(119),
            &[recent_row],
            &empty_tracked_names(),
        );

        history.retain_identities_or_recent(
            &HashSet::from([retained.clone()]),
            now - chrono::Duration::seconds(120),
        );

        assert_eq!(history.identity_count(), 2);
        assert_eq!(history.peak_count(), 2);
        assert_eq!(history.sample_count_for(&retained), 1);
        assert!(history.peak_for(&retained).is_some());
        assert_eq!(history.sample_count_for(&recent), 1);
        assert!(history.peak_for(&recent).is_some());
        assert_eq!(history.sample_count_for(&stale), 0);
        assert!(history.peak_for(&stale).is_none());
    }

    #[test]
    fn process_history_summarizes_and_prunes_name_to_latest_samples() {
        let now = Local::now();
        let mut first = row(1, "app.exe", 10);
        first.start_time = Some(100);
        let mut second = row(2, "APP.EXE", 20);
        second.start_time = Some(200);
        let mut history = ProcessHistory::default();

        for offset in 0..(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY + 2) {
            first.private_bytes = Some(offset as u64);
            history.record_snapshot(
                now + chrono::Duration::seconds(offset as i64),
                &[first.clone()],
                &tracked_names(&["app.exe"]),
            );
        }
        for offset in 0..(GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY + 1) {
            second.private_bytes = Some(offset as u64);
            history.record_snapshot(
                now + chrono::Duration::seconds(offset as i64),
                &[second.clone()],
                &tracked_names(&["app.exe"]),
            );
        }

        assert_eq!(
            history.prune_summary_for_name("app.exe", GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY),
            (GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY * 2 + 3, 3)
        );
        assert_eq!(
            history.prune_name_to_latest("app.exe", GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY),
            3
        );

        let first_samples = history.samples_for(&ProcessIdentity {
            pid: 1,
            name: "app.exe".to_string(),
            start_time: Some(100),
        });
        let second_samples = history.samples_for(&ProcessIdentity {
            pid: 2,
            name: "APP.EXE".to_string(),
            start_time: Some(200),
        });
        assert_eq!(first_samples.len(), GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY);
        assert_eq!(
            second_samples.len(),
            GENERAL_PROCESS_HISTORY_SAMPLE_CAPACITY
        );
        assert_eq!(first_samples[0].private_bytes, Some(2));
        assert_eq!(second_samples[0].private_bytes, Some(1));
    }

    #[test]
    fn system_history_keeps_last_7200_samples() {
        let now = Local::now();
        let mut snapshot = Snapshot {
            captured_at: now,
            total_memory: 0,
            used_memory: 0,
            available_memory: None,
            modified_memory: None,
            standby_memory: None,
            free_zeroed_memory: None,
            committed_memory: None,
            commit_limit: None,
            paged_pool_memory: None,
            nonpaged_pool_memory: None,
            pages_input_per_sec: None,
            pages_output_per_sec: None,
            cpu_name: None,
            cpu_frequency_mhz: None,
            cpu_current_frequency_mhz: None,
            cpu_p_core_frequency_mhz: None,
            cpu_e_core_frequency_mhz: None,
            cpu_total_usage_percent: None,
            cpu_user_usage_percent: None,
            cpu_kernel_usage_percent: None,
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_adapters: Vec::new(),
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            process_count: 0,
            thread_count: None,
            processes: Vec::new(),
        };
        let mut history = SystemHistory::default();

        for offset in 0..(SYSTEM_HISTORY_SAMPLE_CAPACITY + 1) {
            snapshot.captured_at = now + chrono::Duration::seconds(offset as i64);
            snapshot.used_memory = offset as u64;
            history.record_snapshot(&snapshot);
        }

        assert_eq!(history.len(), SYSTEM_HISTORY_SAMPLE_CAPACITY);
        assert_eq!(
            history.samples()[0].value(SystemMetric::PhysicalMemory),
            Some(1.0)
        );
    }
}
