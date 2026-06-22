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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GpuUsageSample {
    pub(crate) dedicated: Option<u64>,
    pub(crate) shared: Option<u64>,
}

impl GpuUsageSample {
    pub(crate) fn merge(self, fallback: Self) -> Self {
        Self {
            dedicated: self.dedicated.or(fallback.dedicated),
            shared: self.shared.or(fallback.shared),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GpuCapacitySample {
    pub(crate) dedicated_total: Option<u64>,
    pub(crate) shared_total: Option<u64>,
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SystemCounterSample {
    pub(crate) available_memory: u64,
    pub(crate) committed_memory: u64,
    pub(crate) commit_limit: u64,
    pub(crate) cache_bytes: Option<u64>,
    pub(crate) standby_cache_bytes: Option<u64>,
    pub(crate) disk_read_bytes_per_sec: Option<u64>,
    pub(crate) disk_write_bytes_per_sec: Option<u64>,
    pub(crate) disk_queue_length: Option<f64>,
    pub(crate) network_received_bytes_per_sec: Option<u64>,
    pub(crate) network_sent_bytes_per_sec: Option<u64>,
    pub(crate) cpu_frequencies_mhz: Vec<(usize, u64)>,
}
