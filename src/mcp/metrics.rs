//! Exact metric encoding at the external API boundary.
use crate::model::history::{ProcessSample, SystemSample};
use serde_json::{Value, json};
pub(crate) const PROCESS_METRICS: &[&str] = &[
    "cpu_percent",
    "private_bytes",
    "workset_bytes",
    "workset_private_bytes",
    "workset_shareable_bytes",
    "thread_count",
    "handle_count",
    "user_object_count",
    "gdi_object_count",
    "gpu_percent",
    "dotnet_heap_bytes",
    "dotnet_gc_gen0_heap_bytes",
    "dotnet_gc_gen1_heap_bytes",
    "dotnet_gc_gen2_heap_bytes",
    "dotnet_gc_loh_bytes",
    "dotnet_gc_poh_bytes",
    "dotnet_gc_committed_bytes",
    "dotnet_gc_fragmentation_bytes",
    "dotnet_allocation_bytes_per_sec",
    "gpu_dedicated_bytes",
    "gpu_shared_bytes",
    "io_read_bytes_per_sec",
    "io_write_bytes_per_sec",
];
pub(crate) fn process_value(sample: &ProcessSample, metric: &str) -> Option<Value> {
    match metric {
        "cpu_percent" => sample
            .cpu_percent
            .filter(|value| value.is_finite())
            .map(|value| json!(value)),
        "private_bytes" => sample.private_bytes.map(|value| json!(value.to_string())),
        "workset_bytes" => sample.workset_bytes.map(|value| json!(value.to_string())),
        "workset_private_bytes" => sample
            .workset_private_bytes
            .map(|value| json!(value.to_string())),
        "workset_shareable_bytes" => sample
            .workset_shareable_bytes
            .map(|value| json!(value.to_string())),
        "thread_count" => sample.thread_count.map(|value| json!(value.to_string())),
        "handle_count" => sample.handle_count.map(|value| json!(value.to_string())),
        "user_object_count" => sample
            .user_object_count
            .map(|value| json!(value.to_string())),
        "gdi_object_count" => sample
            .gdi_object_count
            .map(|value| json!(value.to_string())),
        "gpu_percent" => sample
            .gpu_percent
            .filter(|value| value.is_finite())
            .map(|value| json!(value)),
        "dotnet_heap_bytes" => sample
            .dotnet_heap_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_gen0_heap_bytes" => sample
            .dotnet_gc_gen0_heap_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_gen1_heap_bytes" => sample
            .dotnet_gc_gen1_heap_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_gen2_heap_bytes" => sample
            .dotnet_gc_gen2_heap_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_loh_bytes" => sample
            .dotnet_gc_loh_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_poh_bytes" => sample
            .dotnet_gc_poh_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_committed_bytes" => sample
            .dotnet_gc_committed_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_gc_fragmentation_bytes" => sample
            .dotnet_gc_fragmentation_bytes
            .map(|value| json!(value.to_string())),
        "dotnet_allocation_bytes_per_sec" => sample
            .dotnet_allocation_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "gpu_dedicated_bytes" => sample
            .gpu_dedicated_bytes
            .map(|value| json!(value.to_string())),
        "gpu_shared_bytes" => sample
            .gpu_shared_bytes
            .map(|value| json!(value.to_string())),
        "io_read_bytes_per_sec" => sample
            .io_read_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "io_write_bytes_per_sec" => sample
            .io_write_bytes_per_sec
            .map(|value| json!(value.to_string())),
        _ => None,
    }
}
pub(crate) const SYSTEM_METRICS: &[&str] = &[
    "cpu_average_percent",
    "physical_memory_bytes",
    "modified_memory_bytes",
    "standby_memory_bytes",
    "free_zeroed_memory_bytes",
    "committed_bytes",
    "paged_pool_bytes",
    "nonpaged_pool_bytes",
    "pages_input_per_sec",
    "pages_output_per_sec",
    "thread_count",
    "process_count",
    "network_received_bytes_per_sec",
    "network_sent_bytes_per_sec",
    "disk_read_bytes_per_sec",
    "disk_write_bytes_per_sec",
    "disk_queue_length",
];
pub(crate) fn system_value(sample: &SystemSample, metric: &str) -> Option<Value> {
    match metric {
        "cpu_average_percent" => sample.cpu_average_percent.map(|value| json!(value)),
        "physical_memory_bytes" => sample
            .physical_memory_bytes
            .map(|value| json!(value.to_string())),
        "modified_memory_bytes" => sample
            .modified_memory_bytes
            .map(|value| json!(value.to_string())),
        "standby_memory_bytes" => sample
            .standby_memory_bytes
            .map(|value| json!(value.to_string())),
        "free_zeroed_memory_bytes" => sample
            .free_zeroed_memory_bytes
            .map(|value| json!(value.to_string())),
        "committed_bytes" => sample.committed_bytes.map(|value| json!(value.to_string())),
        "paged_pool_bytes" => sample
            .paged_pool_bytes
            .map(|value| json!(value.to_string())),
        "nonpaged_pool_bytes" => sample
            .nonpaged_pool_bytes
            .map(|value| json!(value.to_string())),
        "pages_input_per_sec" => sample
            .pages_input_per_sec
            .map(|value| json!(value.to_string())),
        "pages_output_per_sec" => sample
            .pages_output_per_sec
            .map(|value| json!(value.to_string())),
        "thread_count" => sample.thread_count.map(|value| json!(value.to_string())),
        "process_count" => sample.process_count.map(|value| json!(value.to_string())),
        "network_received_bytes_per_sec" => sample
            .network_received_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "network_sent_bytes_per_sec" => sample
            .network_sent_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "disk_read_bytes_per_sec" => sample
            .disk_read_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "disk_write_bytes_per_sec" => sample
            .disk_write_bytes_per_sec
            .map(|value| json!(value.to_string())),
        "disk_queue_length" => sample
            .disk_queue_length
            .filter(|value| value.is_finite())
            .map(|value| json!(value)),
        _ => None,
    }
}

pub(crate) fn catalog(kind: Option<&str>) -> Value {
    let mut entries = Vec::new();
    for (target, ids) in [("process", PROCESS_METRICS), ("system", SYSTEM_METRICS)] {
        if kind.is_some_and(|kind| kind != target) {
            continue;
        }
        for id in ids {
            let (unit, representation) = if id.contains("percent") {
                ("percent", "number")
            } else if id.ends_with("bytes_per_sec") {
                ("bytes/second", "decimal_string")
            } else if id.ends_with("bytes") {
                ("bytes", "decimal_string")
            } else if *id == "disk_queue_length" {
                ("count", "number")
            } else if id.ends_with("per_sec") {
                ("count/second", "decimal_string")
            } else {
                ("count", "decimal_string")
            };
            entries.push(json!({"id":id,"target_kind":target,"unit":unit,"representation":representation,
                "source":"applied_samples","availability":"Omitted when the underlying collector has no value. .NET EventPipe requires explicit per-lifetime authorization."}));
        }
    }
    json!({"metrics":entries})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integers_are_exact_and_nonfinite_values_are_absent() {
        let row = crate::model::ProcessRow {
            private_bytes: Some(u64::MAX),
            cpu_percent: Some(f64::NAN),
            ..Default::default()
        };
        let sample = ProcessSample::from_row(chrono::Local::now(), &row);
        assert_eq!(
            process_value(&sample, "private_bytes"),
            Some(json!("18446744073709551615"))
        );
        assert_eq!(process_value(&sample, "cpu_percent"), None);
        assert_eq!(process_value(&sample, "unknown"), None);
    }
}
