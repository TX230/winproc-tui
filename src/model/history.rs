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
    Committed,
    GpuDedicated,
    GpuShared,
}

impl SystemMetric {
    pub(crate) const RAM_VRAM_PANEL: [Self; 4] = [
        Self::PhysicalMemory,
        Self::Committed,
        Self::GpuDedicated,
        Self::GpuShared,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CpuAverage => "CPU Avg",
            Self::PhysicalMemory => "Physical Memory",
            Self::Committed => "Committed",
            Self::GpuDedicated => "GPU Dedicated",
            Self::GpuShared => "GPU Shared",
        }
    }

    pub(crate) fn panel_label(self) -> &'static str {
        match self {
            Self::CpuAverage => "CPUs",
            Self::PhysicalMemory | Self::Committed | Self::GpuDedicated | Self::GpuShared => {
                "RAM/VRAM"
            }
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
    pub(crate) workset_shared_bytes: Option<u64>,
    pub(crate) thread_count: Option<u64>,
    pub(crate) handle_count: Option<u64>,
    pub(crate) user_object_count: Option<u64>,
    pub(crate) gdi_object_count: Option<u64>,
    pub(crate) gpu_percent: Option<f64>,
    pub(crate) dotnet_heap_bytes: Option<u64>,
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
            workset_shared_bytes: row.workset_shared_bytes,
            thread_count: row.thread_count,
            handle_count: row.handle_count,
            user_object_count: row.user_object_count,
            gdi_object_count: row.gdi_object_count,
            gpu_percent: row.gpu_percent,
            dotnet_heap_bytes: row.dotnet_heap_bytes,
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

    pub(crate) fn peak_for(&self, identity: &ProcessIdentity) -> Option<&ProcessPeak> {
        self.peaks.get(identity)
    }

    pub(crate) fn max_sample_count(&self) -> usize {
        self.samples.values().map(VecDeque::len).max().unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.samples.values().map(VecDeque::len).sum()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SystemSample {
    #[allow(dead_code)]
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) cpu_average_percent: Option<u64>,
    pub(crate) physical_memory_bytes: Option<u64>,
    pub(crate) committed_bytes: Option<u64>,
    pub(crate) gpu_dedicated_bytes: Option<u64>,
    pub(crate) gpu_shared_bytes: Option<u64>,
}

impl SystemSample {
    pub(crate) fn from_snapshot(snapshot: &Snapshot) -> Self {
        Self {
            captured_at: snapshot.captured_at,
            cpu_average_percent: snapshot.cpu_total_usage_percent.map(u64::from),
            physical_memory_bytes: Some(snapshot.used_memory),
            committed_bytes: snapshot.committed_memory,
            gpu_dedicated_bytes: snapshot.gpu_dedicated_used,
            gpu_shared_bytes: snapshot.gpu_shared_used,
        }
    }

    pub(crate) fn value(&self, metric: SystemMetric) -> Option<u64> {
        match metric {
            SystemMetric::CpuAverage => self.cpu_average_percent,
            SystemMetric::PhysicalMemory => self.physical_memory_bytes,
            SystemMetric::Committed => self.committed_bytes,
            SystemMetric::GpuDedicated => self.gpu_dedicated_bytes,
            SystemMetric::GpuShared => self.gpu_shared_bytes,
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

    fn prune(&mut self) {
        let excess = self
            .samples
            .len()
            .saturating_sub(SYSTEM_HISTORY_SAMPLE_CAPACITY);
        if excess > 0 {
            self.samples.drain(0..excess);
        }
    }

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
            committed_memory: None,
            commit_limit: None,
            gpu_dedicated_used: None,
            gpu_dedicated_total: None,
            gpu_shared_used: None,
            gpu_shared_total: None,
            cpu_name: None,
            cpu_frequency_mhz: None,
            cpu_current_frequency_mhz: None,
            cpu_p_core_frequency_mhz: None,
            cpu_e_core_frequency_mhz: None,
            cpu_total_usage_percent: None,
            cpu_logical_processors: Vec::new(),
            cpu_topology: None,
            cpu_cache: None,
            gpu_name: None,
            disks: Vec::new(),
            process_count: 0,
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
            Some(1)
        );
    }
}
