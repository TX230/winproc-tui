use chrono::{DateTime, Local};

use super::{CpuLogicalProcessorSample, DiskUsageSample, ProcessRow};

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) total_memory: u64,
    pub(crate) used_memory: u64,
    pub(crate) committed_memory: Option<u64>,
    pub(crate) commit_limit: Option<u64>,
    pub(crate) gpu_dedicated_used: Option<u64>,
    pub(crate) gpu_dedicated_total: Option<u64>,
    pub(crate) gpu_shared_used: Option<u64>,
    pub(crate) gpu_shared_total: Option<u64>,
    pub(crate) cpu_name: Option<String>,
    pub(crate) cpu_frequency_mhz: Option<u64>,
    pub(crate) cpu_current_frequency_mhz: Option<u64>,
    pub(crate) cpu_p_core_frequency_mhz: Option<u64>,
    pub(crate) cpu_e_core_frequency_mhz: Option<u64>,
    pub(crate) cpu_total_usage_percent: Option<u8>,
    pub(crate) cpu_logical_processors: Vec<CpuLogicalProcessorSample>,
    pub(crate) cpu_topology: Option<String>,
    pub(crate) cpu_cache: Option<String>,
    pub(crate) gpu_name: Option<String>,
    pub(crate) disks: Vec<DiskUsageSample>,
    pub(crate) process_count: usize,
    pub(crate) processes: Vec<ProcessRow>,
}
