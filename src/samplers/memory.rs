use anyhow::Result;

use crate::model::SystemCounterSample;

pub(crate) fn map_memory_counters(
    total_memory: u64,
    fallback_available_memory: u64,
    sampled_counters: Result<Option<SystemCounterSample>>,
) -> (
    u64,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<String>,
) {
    match sampled_counters {
        Ok(Some(sample)) => (
            sample.available_memory.min(total_memory),
            Some(sample.committed_memory),
            Some(sample.commit_limit),
            sample.cache_bytes,
            sample.standby_cache_bytes,
            sample.disk_read_bytes_per_sec,
            sample.disk_write_bytes_per_sec,
            sample.network_received_bytes_per_sec,
            sample.network_sent_bytes_per_sec,
            None,
        ),
        Ok(None) => (
            fallback_available_memory.min(total_memory),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("Warning: commit counters unavailable".to_string()),
        ),
        Err(error) => (
            fallback_available_memory.min(total_memory),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(format!("Warning: commit counters unavailable ({error})")),
        ),
    }
}
