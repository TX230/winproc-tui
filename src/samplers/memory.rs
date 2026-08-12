use anyhow::Result;

use std::mem::{size_of, zeroed};

use winapi::um::psapi::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

use crate::model::{PerformanceSample, SystemCounterSample};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MappedSystemCounters {
    pub(crate) available_memory: u64,
    pub(crate) committed_memory: Option<u64>,
    pub(crate) commit_limit: Option<u64>,
    pub(crate) cache_bytes: Option<u64>,
    pub(crate) modified_page_list_bytes: Option<u64>,
    pub(crate) standby_cache_bytes: Option<u64>,
    pub(crate) free_zeroed_bytes: Option<u64>,
    pub(crate) pages_input_per_sec: Option<u64>,
    pub(crate) pages_output_per_sec: Option<u64>,
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
            modified_page_list_bytes: sample.modified_page_list_bytes,
            standby_cache_bytes: sample.standby_cache_bytes,
            free_zeroed_bytes: sample.free_zeroed_bytes,
            pages_input_per_sec: sample.pages_input_per_sec,
            pages_output_per_sec: sample.pages_output_per_sec,
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
            modified_page_list_bytes: None,
            standby_cache_bytes: None,
            free_zeroed_bytes: None,
            pages_input_per_sec: None,
            pages_output_per_sec: None,
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
            modified_page_list_bytes: None,
            standby_cache_bytes: None,
            free_zeroed_bytes: None,
            pages_input_per_sec: None,
            pages_output_per_sec: None,
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            disk_queue_length: None,
            network_received_bytes_per_sec: None,
            network_sent_bytes_per_sec: None,
            warning: Some(format!("Warning: commit counters unavailable ({error})")),
        },
    }
}

pub(crate) fn collect_performance_info() -> PerformanceSample {
    unsafe {
        let mut info: PERFORMANCE_INFORMATION = zeroed();
        info.cb = size_of::<PERFORMANCE_INFORMATION>() as u32;
        if GetPerformanceInfo(&mut info, info.cb) == 0 {
            return PerformanceSample::default();
        }

        let page_size = info.PageSize as u64;
        PerformanceSample {
            physical_total_bytes: Some((info.PhysicalTotal as u64).saturating_mul(page_size)),
            physical_available_bytes: Some(
                (info.PhysicalAvailable as u64).saturating_mul(page_size),
            ),
            paged_pool_bytes: Some((info.KernelPaged as u64).saturating_mul(page_size)),
            nonpaged_pool_bytes: Some((info.KernelNonpaged as u64).saturating_mul(page_size)),
            process_count: Some(info.ProcessCount as u64),
            thread_count: Some(info.ThreadCount as u64),
        }
    }
}
