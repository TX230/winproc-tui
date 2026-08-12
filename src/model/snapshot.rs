use chrono::{DateTime, Local};

use super::{CpuLogicalProcessorSample, DiskUsageSample, GpuAdapterSample, ProcessRow};

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) total_memory: u64,
    pub(crate) used_memory: u64,
    pub(crate) available_memory: Option<u64>,
    pub(crate) modified_memory: Option<u64>,
    pub(crate) standby_memory: Option<u64>,
    pub(crate) free_zeroed_memory: Option<u64>,
    pub(crate) committed_memory: Option<u64>,
    pub(crate) commit_limit: Option<u64>,
    pub(crate) paged_pool_memory: Option<u64>,
    pub(crate) nonpaged_pool_memory: Option<u64>,
    pub(crate) pages_input_per_sec: Option<u64>,
    pub(crate) pages_output_per_sec: Option<u64>,
    pub(crate) cpu_name: Option<String>,
    pub(crate) cpu_frequency_mhz: Option<u64>,
    pub(crate) cpu_current_frequency_mhz: Option<u64>,
    pub(crate) cpu_p_core_frequency_mhz: Option<u64>,
    pub(crate) cpu_e_core_frequency_mhz: Option<u64>,
    pub(crate) cpu_total_usage_percent: Option<u8>,
    pub(crate) cpu_logical_processors: Vec<CpuLogicalProcessorSample>,
    pub(crate) cpu_topology: Option<String>,
    pub(crate) cpu_cache: Option<String>,
    pub(crate) gpu_adapters: Vec<GpuAdapterSample>,
    pub(crate) disks: Vec<DiskUsageSample>,
    pub(crate) disk_read_bytes_per_sec: Option<u64>,
    pub(crate) disk_write_bytes_per_sec: Option<u64>,
    pub(crate) disk_queue_length: Option<f64>,
    pub(crate) network_received_bytes_per_sec: Option<u64>,
    pub(crate) network_sent_bytes_per_sec: Option<u64>,
    pub(crate) process_count: usize,
    pub(crate) thread_count: Option<u64>,
    pub(crate) processes: Vec<ProcessRow>,
}
