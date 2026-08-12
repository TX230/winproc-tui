#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskUsageSample {
    pub(crate) name: String,
    pub(crate) free_bytes: u64,
    pub(crate) total_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CpuSummarySample {
    pub(crate) name: Option<String>,
    pub(crate) frequency_mhz: Option<u64>,
    pub(crate) current_frequency_mhz: Option<u64>,
    pub(crate) p_core_frequency_mhz: Option<u64>,
    pub(crate) e_core_frequency_mhz: Option<u64>,
    pub(crate) total_usage_percent: Option<u8>,
    pub(crate) logical_processors: Vec<CpuLogicalProcessorSample>,
    pub(crate) topology: Option<String>,
    pub(crate) caches: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CpuLogicalProcessorSample {
    pub(crate) usage_percent: u8,
    pub(crate) kind: Option<CpuCoreKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CpuCoreKind {
    Performance,
    Efficiency,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct GpuAdapterId {
    pub(crate) high: u32,
    pub(crate) low: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct GpuEngineSummary {
    pub(crate) average_percent: Option<f64>,
    pub(crate) max_percent: Option<f64>,
    pub(crate) engine_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GpuAdapterSample {
    pub(crate) id: GpuAdapterId,
    pub(crate) name: Option<String>,
    pub(crate) utilization_percent: Option<f64>,
    pub(crate) encode: GpuEngineSummary,
    pub(crate) decode: GpuEngineSummary,
    pub(crate) dedicated_used: Option<u64>,
    pub(crate) dedicated_total: Option<u64>,
    pub(crate) shared_used: Option<u64>,
    pub(crate) shared_total: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GpuSample {
    pub(crate) adapters: Vec<GpuAdapterSample>,
    pub(crate) processes: std::collections::HashMap<u32, ProcessGpuSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ProcessGpuSample {
    pub(crate) utilization_percent: Option<f64>,
    pub(crate) dedicated_bytes: Option<u64>,
    pub(crate) shared_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PerformanceSample {
    pub(crate) physical_total_bytes: Option<u64>,
    pub(crate) physical_available_bytes: Option<u64>,
    pub(crate) paged_pool_bytes: Option<u64>,
    pub(crate) nonpaged_pool_bytes: Option<u64>,
    pub(crate) process_count: Option<u64>,
    pub(crate) thread_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SystemCounterSample {
    pub(crate) available_memory: u64,
    pub(crate) committed_memory: u64,
    pub(crate) commit_limit: u64,
    pub(crate) cache_bytes: Option<u64>,
    pub(crate) standby_cache_bytes: Option<u64>,
    pub(crate) free_zeroed_bytes: Option<u64>,
    pub(crate) pages_input_per_sec: Option<u64>,
    pub(crate) disk_read_bytes_per_sec: Option<u64>,
    pub(crate) disk_write_bytes_per_sec: Option<u64>,
    pub(crate) disk_queue_length: Option<f64>,
    pub(crate) network_received_bytes_per_sec: Option<u64>,
    pub(crate) network_sent_bytes_per_sec: Option<u64>,
    pub(crate) cpu_frequencies_mhz: Vec<(usize, u64)>,
}
