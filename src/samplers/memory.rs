use anyhow::Result;

use crate::model::SystemCounterSample;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MappedSystemCounters {
    pub(crate) available_memory: u64,
    pub(crate) committed_memory: Option<u64>,
    pub(crate) commit_limit: Option<u64>,
    pub(crate) cache_bytes: Option<u64>,
    pub(crate) standby_cache_bytes: Option<u64>,
    pub(crate) disk_read_bytes_per_sec: Option<u64>,
    pub(crate) disk_write_bytes_per_sec: Option<u64>,
    pub(crate) disk_queue_length: Option<f64>,
    pub(crate) network_received_bytes_per_sec: Option<u64>,
    pub(crate) network_sent_bytes_per_sec: Option<u64>,
    pub(crate) warning: Option<String>,
}

pub(crate) fn map_memory_counters(
    total_memory: u64,
    fallback_available_memory: u64,
    sampled_counters: Result<Option<SystemCounterSample>>,
) -> MappedSystemCounters {
    match sampled_counters {
        Ok(Some(sample)) => MappedSystemCounters {
            available_memory: sample.available_memory.min(total_memory),
            committed_memory: Some(sample.committed_memory),
            commit_limit: Some(sample.commit_limit),
            cache_bytes: sample.cache_bytes,
            standby_cache_bytes: sample.standby_cache_bytes,
            disk_read_bytes_per_sec: sample.disk_read_bytes_per_sec,
            disk_write_bytes_per_sec: sample.disk_write_bytes_per_sec,
            disk_queue_length: sample.disk_queue_length,
            network_received_bytes_per_sec: sample.network_received_bytes_per_sec,
            network_sent_bytes_per_sec: sample.network_sent_bytes_per_sec,
            warning: None,
        },
        Ok(None) => MappedSystemCounters {
            available_memory: fallback_available_memory.min(total_memory),
            committed_memory: None,
            commit_limit: None,
            cache_bytes: None,
            standby_cache_bytes: None,
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            warning: Some("Warning: commit counters unavailable".to_string()),
        },
        Err(error) => MappedSystemCounters {
            available_memory: fallback_available_memory.min(total_memory),
            committed_memory: None,
            commit_limit: None,
            cache_bytes: None,
            standby_cache_bytes: None,
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            warning: Some(format!("Warning: commit counters unavailable ({error})")),
        },
    }
}
