use std::{
    collections::HashMap,
    ptr::{null, null_mut},
};

use anyhow::{Context, Result};
use winapi::um::pdh::{
    PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhOpenQueryW,
};

use crate::{model::ProcessExtraMetrics, platform::to_wide};

use super::pdh::{
    add_optional_pdh_counter, ensure_pdh_success, map_process_counter_instances_to_pids,
    normalize_process_cpu_percent, pdh_ok, read_named_counter_double_items,
    read_named_counter_items, read_optional_named_counter_values, read_optional_pdh_large_value,
    read_pdh_large_value, sum_optional_values,
};

pub(crate) struct SystemCounterSampler {
    query: PDH_HQUERY,
    available_counter: PDH_HCOUNTER,
    committed_counter: PDH_HCOUNTER,
    commit_limit_counter: PDH_HCOUNTER,
    cache_counter: Option<PDH_HCOUNTER>,
    standby_reserve_counter: Option<PDH_HCOUNTER>,
    standby_normal_counter: Option<PDH_HCOUNTER>,
    standby_core_counter: Option<PDH_HCOUNTER>,
    disk_read_counter: Option<PDH_HCOUNTER>,
    disk_write_counter: Option<PDH_HCOUNTER>,
    network_received_counter: Option<PDH_HCOUNTER>,
    network_sent_counter: Option<PDH_HCOUNTER>,
    cpu_frequency_counter: Option<PDH_HCOUNTER>,
    cpu_performance_counter: Option<PDH_HCOUNTER>,
}

pub(crate) struct ProcessCounterSampler {
    query: PDH_HQUERY,
    process_id_counter: PDH_HCOUNTER,
    cpu_counter: Option<PDH_HCOUNTER>,
    private_counter: Option<PDH_HCOUNTER>,
    working_set_counter: Option<PDH_HCOUNTER>,
    working_set_private_counter: Option<PDH_HCOUNTER>,
    io_read_counter: Option<PDH_HCOUNTER>,
    io_write_counter: Option<PDH_HCOUNTER>,
    dotnet_process_id_counter: Option<PDH_HCOUNTER>,
    dotnet_heap_counter: Option<PDH_HCOUNTER>,
}

impl SystemCounterSampler {
    pub(crate) fn new() -> Result<Self> {
        unsafe {
            let mut query: PDH_HQUERY = null_mut();
            let status = PdhOpenQueryW(null(), 0, &mut query);
            ensure_pdh_success(status, "opening system counter query")?;

            let mut available_counter: PDH_HCOUNTER = null_mut();
            let mut committed_counter: PDH_HCOUNTER = null_mut();
            let mut commit_limit_counter: PDH_HCOUNTER = null_mut();

            ensure_pdh_success(
                PdhAddEnglishCounterW(
                    query,
                    to_wide("\\Memory\\Available Bytes").as_ptr(),
                    0,
                    &mut available_counter,
                ),
                "adding \\Memory\\Available Bytes",
            )?;
            ensure_pdh_success(
                PdhAddEnglishCounterW(
                    query,
                    to_wide("\\Memory\\Committed Bytes").as_ptr(),
                    0,
                    &mut committed_counter,
                ),
                "adding \\Memory\\Committed Bytes",
            )?;
            ensure_pdh_success(
                PdhAddEnglishCounterW(
                    query,
                    to_wide("\\Memory\\Commit Limit").as_ptr(),
                    0,
                    &mut commit_limit_counter,
                ),
                "adding \\Memory\\Commit Limit",
            )?;

            let cache_counter = add_optional_pdh_counter(query, "\\Memory\\Cache Bytes");
            let standby_reserve_counter =
                add_optional_pdh_counter(query, "\\Memory\\Standby Cache Reserve Bytes");
            let standby_normal_counter =
                add_optional_pdh_counter(query, "\\Memory\\Standby Cache Normal Priority Bytes");
            let standby_core_counter =
                add_optional_pdh_counter(query, "\\Memory\\Standby Cache Core Bytes");
            let disk_read_counter =
                add_optional_pdh_counter(query, "\\PhysicalDisk(_Total)\\Disk Read Bytes/sec");
            let disk_write_counter =
                add_optional_pdh_counter(query, "\\PhysicalDisk(_Total)\\Disk Write Bytes/sec");
            let network_received_counter =
                add_optional_pdh_counter(query, "\\Network Interface(*)\\Bytes Received/sec");
            let network_sent_counter =
                add_optional_pdh_counter(query, "\\Network Interface(*)\\Bytes Sent/sec");
            let cpu_frequency_counter =
                add_optional_pdh_counter(query, "\\Processor Information(*)\\Processor Frequency");
            let cpu_performance_counter = add_optional_pdh_counter(
                query,
                "\\Processor Information(*)\\% Processor Performance",
            );

            ensure_pdh_success(PdhCollectQueryData(query), "priming system counter query")?;

            Ok(Self {
                query,
                available_counter,
                committed_counter,
                commit_limit_counter,
                cache_counter,
                standby_reserve_counter,
                standby_normal_counter,
                standby_core_counter,
                disk_read_counter,
                disk_write_counter,
                network_received_counter,
                network_sent_counter,
                cpu_frequency_counter,
                cpu_performance_counter,
            })
        }
    }

    pub(crate) fn sample(&mut self) -> Result<crate::model::SystemCounterSample> {
        unsafe {
            ensure_pdh_success(
                PdhCollectQueryData(self.query),
                "collecting system counters",
            )?;

            Ok(crate::model::SystemCounterSample {
                available_memory: read_pdh_large_value(self.available_counter)
                    .context("reading \\Memory\\Available Bytes")?,
                committed_memory: read_pdh_large_value(self.committed_counter)
                    .context("reading \\Memory\\Committed Bytes")?,
                commit_limit: read_pdh_large_value(self.commit_limit_counter)
                    .context("reading \\Memory\\Commit Limit")?,
                cache_bytes: read_optional_pdh_large_value(self.cache_counter),
                standby_cache_bytes: sum_optional_values([
                    read_optional_pdh_large_value(self.standby_reserve_counter),
                    read_optional_pdh_large_value(self.standby_normal_counter),
                    read_optional_pdh_large_value(self.standby_core_counter),
                ]),
                disk_read_bytes_per_sec: read_optional_pdh_large_value(self.disk_read_counter),
                disk_write_bytes_per_sec: read_optional_pdh_large_value(self.disk_write_counter),
                network_received_bytes_per_sec: read_optional_named_counter_values(
                    self.network_received_counter,
                ),
                network_sent_bytes_per_sec: read_optional_named_counter_values(
                    self.network_sent_counter,
                ),
                cpu_frequencies_mhz: read_cpu_current_frequency_items(
                    self.cpu_frequency_counter,
                    self.cpu_performance_counter,
                ),
            })
        }
    }
}

fn read_cpu_current_frequency_items(
    frequency_counter: Option<PDH_HCOUNTER>,
    performance_counter: Option<PDH_HCOUNTER>,
) -> Vec<(usize, u64)> {
    let (Some(frequency_counter), Some(performance_counter)) =
        (frequency_counter, performance_counter)
    else {
        return Vec::new();
    };
    let (Some(frequencies), Some(performances)) = (
        read_named_counter_items(frequency_counter),
        read_named_counter_double_items(performance_counter),
    ) else {
        return Vec::new();
    };
    current_cpu_frequency_items(frequencies, performances)
}

fn current_cpu_frequency_items(
    frequencies: Vec<(String, u64)>,
    performances: Vec<(String, f64)>,
) -> Vec<(usize, u64)> {
    let performances = performances
        .into_iter()
        .filter_map(|(name, performance)| {
            if !performance.is_finite() || performance < 0.0 {
                return None;
            }
            processor_information_instance_index(&name).map(|index| (index, performance))
        })
        .collect::<HashMap<_, _>>();

    frequencies
        .into_iter()
        .filter_map(|(name, frequency)| {
            if frequency == 0 {
                return None;
            }
            let index = processor_information_instance_index(&name)?;
            let performance = performances.get(&index)?;
            Some((
                index,
                ((*performance * frequency as f64) / 100.0).round() as u64,
            ))
        })
        .collect()
}

fn processor_information_instance_index(name: &str) -> Option<usize> {
    if name.eq_ignore_ascii_case("_Total") || name.to_ascii_lowercase().ends_with(",_total") {
        return None;
    }
    name.rsplit_once(',')
        .map(|(_, index)| index)
        .unwrap_or(name)
        .parse()
        .ok()
}

impl Drop for SystemCounterSampler {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

impl ProcessCounterSampler {
    pub(crate) fn new() -> Result<Self> {
        unsafe {
            let mut query: PDH_HQUERY = null_mut();
            ensure_pdh_success(
                PdhOpenQueryW(null(), 0, &mut query),
                "opening process query",
            )?;

            let mut process_id_counter: PDH_HCOUNTER = null_mut();
            ensure_pdh_success(
                PdhAddEnglishCounterW(
                    query,
                    to_wide("\\Process(*)\\ID Process").as_ptr(),
                    0,
                    &mut process_id_counter,
                ),
                "adding \\Process(*)\\ID Process",
            )?;

            let cpu_counter = add_optional_pdh_counter(query, "\\Process(*)\\% Processor Time");
            let private_counter = add_optional_pdh_counter(query, "\\Process(*)\\Private Bytes");
            let working_set_counter = add_optional_pdh_counter(query, "\\Process(*)\\Working Set");
            let working_set_private_counter =
                add_optional_pdh_counter(query, "\\Process(*)\\Working Set - Private");
            let io_read_counter =
                add_optional_pdh_counter(query, "\\Process(*)\\IO Read Bytes/sec");
            let io_write_counter =
                add_optional_pdh_counter(query, "\\Process(*)\\IO Write Bytes/sec");
            let dotnet_process_id_counter =
                add_optional_pdh_counter(query, "\\.NET CLR Memory(*)\\Process ID");
            let dotnet_heap_counter =
                add_optional_pdh_counter(query, "\\.NET CLR Memory(*)\\# Bytes in all Heaps");

            ensure_pdh_success(PdhCollectQueryData(query), "priming process query")?;

            Ok(Self {
                query,
                process_id_counter,
                cpu_counter,
                private_counter,
                working_set_counter,
                working_set_private_counter,
                io_read_counter,
                io_write_counter,
                dotnet_process_id_counter,
                dotnet_heap_counter,
            })
        }
    }

    pub(crate) fn sample(
        &mut self,
        logical_processor_count: usize,
    ) -> std::collections::HashMap<u32, ProcessExtraMetrics> {
        unsafe {
            if !pdh_ok(PdhCollectQueryData(self.query)) {
                return std::collections::HashMap::new();
            }
        }

        let Some(process_ids) = read_named_counter_items(self.process_id_counter) else {
            return std::collections::HashMap::new();
        };

        let mut metrics = std::collections::HashMap::<u32, ProcessExtraMetrics>::new();
        if let Some(cpu_counter) = self.cpu_counter {
            if let Some(cpu_items) = read_named_counter_double_items(cpu_counter) {
                let cpu_by_pid =
                    map_process_counter_instances_to_pids(process_ids.clone(), cpu_items);
                for (pid, cpu_percent) in cpu_by_pid {
                    metrics.entry(pid).or_default().cpu_percent =
                        normalize_process_cpu_percent(cpu_percent, logical_processor_count);
                }
            }
        }

        self.merge_u64_counter(
            &mut metrics,
            process_ids.clone(),
            self.private_counter,
            |metric, value| metric.private_bytes = Some(value),
        );
        self.merge_u64_counter(
            &mut metrics,
            process_ids.clone(),
            self.working_set_counter,
            |metric, value| metric.workset_bytes = Some(value),
        );
        self.merge_u64_counter(
            &mut metrics,
            process_ids.clone(),
            self.working_set_private_counter,
            |metric, value| metric.workset_private_bytes = Some(value),
        );
        self.merge_u64_counter(
            &mut metrics,
            process_ids.clone(),
            self.io_read_counter,
            |metric, value| metric.io_read_bytes_per_sec = Some(value),
        );
        self.merge_u64_counter(
            &mut metrics,
            process_ids,
            self.io_write_counter,
            |metric, value| metric.io_write_bytes_per_sec = Some(value),
        );

        if let (Some(dotnet_process_id_counter), Some(dotnet_heap_counter)) =
            (self.dotnet_process_id_counter, self.dotnet_heap_counter)
        {
            if let (Some(dotnet_pids), Some(dotnet_heaps)) = (
                read_named_counter_items(dotnet_process_id_counter),
                read_named_counter_items(dotnet_heap_counter),
            ) {
                let heaps_by_pid = map_process_counter_instances_to_pids(dotnet_pids, dotnet_heaps);
                for (pid, heap_bytes) in heaps_by_pid {
                    metrics.entry(pid).or_default().dotnet_heap_bytes = Some(heap_bytes);
                }
            }
        }

        metrics
    }

    fn merge_u64_counter(
        &self,
        metrics: &mut std::collections::HashMap<u32, ProcessExtraMetrics>,
        process_ids: Vec<(String, u64)>,
        counter: Option<PDH_HCOUNTER>,
        apply: impl Fn(&mut ProcessExtraMetrics, u64),
    ) {
        let Some(counter) = counter else {
            return;
        };
        let Some(items) = read_named_counter_items(counter) else {
            return;
        };

        for (pid, value) in map_process_counter_instances_to_pids(process_ids, items) {
            apply(metrics.entry(pid).or_default(), value);
        }
    }
}

impl Drop for ProcessCounterSampler {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_information_instance_index_uses_logical_processor_suffix() {
        assert_eq!(processor_information_instance_index("0,0"), Some(0));
        assert_eq!(processor_information_instance_index("0,15"), Some(15));
        assert_eq!(processor_information_instance_index("12"), Some(12));
        assert_eq!(processor_information_instance_index("_Total"), None);
        assert_eq!(processor_information_instance_index("0,_Total"), None);
        assert_eq!(processor_information_instance_index("0,_total"), None);
    }

    #[test]
    fn current_cpu_frequency_items_scales_base_frequency_by_processor_performance() {
        let frequencies = vec![
            ("0,0".to_string(), 2_100),
            ("0,1".to_string(), 2_100),
            ("0,_Total".to_string(), 2_100),
            ("missing-performance".to_string(), 2_100),
        ];
        let performances = vec![
            ("0,0".to_string(), 183.0),
            ("0,1".to_string(), 151.5),
            ("0,_Total".to_string(), 175.0),
        ];

        assert_eq!(
            current_cpu_frequency_items(frequencies, performances),
            vec![(0, 3_843), (1, 3_182)]
        );
    }
}
