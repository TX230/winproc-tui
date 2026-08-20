use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{GpuAdapterSample, ProcessRow, Snapshot};

pub(crate) const CURRENT_LOG_SCHEMA_VERSION: u64 = 3;
pub(crate) const LEGACY_LOG_SCHEMA_VERSION: u64 = 2;

pub(crate) const SYSTEM_U64_FIELD_COUNT: usize = 21;
pub(crate) const PROCESS_F64_FIELD_COUNT: usize = 5;
pub(crate) const PROCESS_U64_FIELD_COUNT: usize = 21;
pub(crate) const GPU_F64_FIELD_COUNT: usize = 5;
pub(crate) const GPU_U64_FIELD_COUNT: usize = 6;

pub(crate) mod system_u64 {
    pub(crate) const PHYSICAL_MEMORY: usize = 0;
    pub(crate) const TOTAL_MEMORY: usize = 1;
    pub(crate) const AVAILABLE_MEMORY: usize = 2;
    pub(crate) const MODIFIED_MEMORY: usize = 3;
    pub(crate) const STANDBY_MEMORY: usize = 4;
    pub(crate) const FREE_ZEROED_MEMORY: usize = 5;
    pub(crate) const COMMITTED_MEMORY: usize = 6;
    pub(crate) const COMMIT_LIMIT: usize = 7;
    pub(crate) const PAGED_POOL: usize = 8;
    pub(crate) const NONPAGED_POOL: usize = 9;
    pub(crate) const PAGES_INPUT: usize = 10;
    pub(crate) const PAGES_OUTPUT: usize = 11;
    pub(crate) const PROCESS_COUNT: usize = 12;
    pub(crate) const THREAD_COUNT: usize = 13;
    pub(crate) const CPU_TOTAL: usize = 14;
    pub(crate) const CPU_USER: usize = 15;
    pub(crate) const CPU_KERNEL: usize = 16;
    pub(crate) const DISK_READ: usize = 17;
    pub(crate) const DISK_WRITE: usize = 18;
    pub(crate) const NETWORK_RECEIVED: usize = 19;
    pub(crate) const NETWORK_SENT: usize = 20;
}

pub(crate) mod process_f64 {
    pub(crate) const CPU_PERCENT: usize = 0;
    pub(crate) const GPU_PERCENT: usize = 1;
    // Schema v3 positions 2..=4 are reserved for removed .NET GC-rate metrics.
}

pub(crate) mod process_u64 {
    pub(crate) const PRIVATE_BYTES: usize = 0;
    pub(crate) const WORKSET_BYTES: usize = 1;
    pub(crate) const WORKSET_PRIVATE_BYTES: usize = 2;
    pub(crate) const WORKSET_SHAREABLE_BYTES: usize = 3;
    pub(crate) const THREAD_COUNT: usize = 4;
    pub(crate) const HANDLE_COUNT: usize = 5;
    pub(crate) const USER_OBJECT_COUNT: usize = 6;
    pub(crate) const GDI_OBJECT_COUNT: usize = 7;
    pub(crate) const GPU_DEDICATED_BYTES: usize = 8;
    pub(crate) const GPU_SHARED_BYTES: usize = 9;
    pub(crate) const DOTNET_HEAP_BYTES: usize = 10;
    pub(crate) const IO_READ_BYTES_PER_SEC: usize = 11;
    pub(crate) const IO_WRITE_BYTES_PER_SEC: usize = 12;
    pub(crate) const DOTNET_GC_COMMITTED_BYTES: usize = 13;
    pub(crate) const DOTNET_GC_FRAGMENTATION_BYTES: usize = 14;
    pub(crate) const DOTNET_ALLOCATION_BYTES_PER_SEC: usize = 15;
    pub(crate) const DOTNET_GC_GEN0_HEAP_BYTES: usize = 16;
    pub(crate) const DOTNET_GC_GEN1_HEAP_BYTES: usize = 17;
    pub(crate) const DOTNET_GC_GEN2_HEAP_BYTES: usize = 18;
    pub(crate) const DOTNET_GC_LOH_BYTES: usize = 19;
    pub(crate) const DOTNET_GC_POH_BYTES: usize = 20;
}

pub(crate) mod gpu_f64 {
    pub(crate) const UTILIZATION_PERCENT: usize = 0;
    pub(crate) const ENCODE_AVERAGE_PERCENT: usize = 1;
    pub(crate) const ENCODE_MAX_PERCENT: usize = 2;
    pub(crate) const DECODE_AVERAGE_PERCENT: usize = 3;
    pub(crate) const DECODE_MAX_PERCENT: usize = 4;
}

pub(crate) mod gpu_u64 {
    pub(crate) const ENCODE_ENGINE_COUNT: usize = 0;
    pub(crate) const DECODE_ENGINE_COUNT: usize = 1;
    pub(crate) const DEDICATED_BYTES: usize = 2;
    pub(crate) const DEDICATED_TOTAL_BYTES: usize = 3;
    pub(crate) const SHARED_BYTES: usize = 4;
    pub(crate) const SHARED_TOTAL_BYTES: usize = 5;
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum V3Record {
    #[serde(rename = "s")]
    Session(V3SessionRecord),
    #[serde(rename = "p")]
    Process(V3ProcessDefinition),
    #[serde(rename = "g")]
    Gpu(V3GpuDefinition),
    #[serde(rename = "f")]
    Frame(V3FrameRecord),
    #[serde(rename = "e")]
    End(V3EndRecord),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3SessionRecord {
    #[serde(rename = "v")]
    pub(crate) schema_version: u64,
    #[serde(rename = "id")]
    pub(crate) session_id: String,
    #[serde(rename = "app")]
    pub(crate) app_version: String,
    #[serde(rename = "host")]
    pub(crate) host: String,
    #[serde(rename = "start")]
    pub(crate) started_at_ms: i64,
    #[serde(rename = "interval")]
    pub(crate) interval_seconds: u64,
    #[serde(rename = "tracked")]
    pub(crate) tracked_names: Vec<String>,
    #[serde(rename = "columns")]
    pub(crate) columns: Vec<String>,
    #[serde(rename = "sort")]
    pub(crate) sort: [String; 2],
    #[serde(rename = "system")]
    pub(crate) system: V3SessionSystem,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct V3SessionSystem {
    #[serde(rename = "cpu", default, skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_name: Option<String>,
    #[serde(rename = "cpu_mhz", default, skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_frequency_mhz: Option<u64>,
    #[serde(rename = "topology", default, skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_topology: Option<String>,
    #[serde(rename = "cache", default, skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_cache: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct V3ProcessDefinition(
    pub(crate) u32,
    pub(crate) u32,
    pub(crate) String,
    pub(crate) Option<u64>,
    pub(crate) Option<String>,
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct V3GpuDefinition(
    pub(crate) u32,
    pub(crate) u32,
    pub(crate) u32,
    pub(crate) Option<String>,
);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3FrameRecord(
    pub(crate) i64,
    pub(crate) V3SystemMetrics,
    pub(crate) Vec<V3ProcessSample>,
);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3EndRecord(pub(crate) i64, pub(crate) String);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3SystemMetrics(
    pub(crate) [Option<u64>; SYSTEM_U64_FIELD_COUNT],
    pub(crate) Option<f64>,
    pub(crate) Vec<V3GpuSample>,
);

impl V3SystemMetrics {
    pub(crate) fn from_snapshot(snapshot: &Snapshot, gpu_samples: Vec<V3GpuSample>) -> Self {
        let mut values = [None; SYSTEM_U64_FIELD_COUNT];
        values[system_u64::PHYSICAL_MEMORY] = Some(snapshot.used_memory);
        values[system_u64::TOTAL_MEMORY] = Some(snapshot.total_memory);
        values[system_u64::AVAILABLE_MEMORY] = snapshot.available_memory;
        values[system_u64::MODIFIED_MEMORY] = snapshot.modified_memory;
        values[system_u64::STANDBY_MEMORY] = snapshot.standby_memory;
        values[system_u64::FREE_ZEROED_MEMORY] = snapshot.free_zeroed_memory;
        values[system_u64::COMMITTED_MEMORY] = snapshot.committed_memory;
        values[system_u64::COMMIT_LIMIT] = snapshot.commit_limit;
        values[system_u64::PAGED_POOL] = snapshot.paged_pool_memory;
        values[system_u64::NONPAGED_POOL] = snapshot.nonpaged_pool_memory;
        values[system_u64::PAGES_INPUT] = snapshot.pages_input_per_sec;
        values[system_u64::PAGES_OUTPUT] = snapshot.pages_output_per_sec;
        values[system_u64::PROCESS_COUNT] = u64::try_from(snapshot.process_count).ok();
        values[system_u64::THREAD_COUNT] = snapshot.thread_count;
        values[system_u64::CPU_TOTAL] = snapshot.cpu_total_usage_percent.map(u64::from);
        values[system_u64::CPU_USER] = snapshot.cpu_user_usage_percent.map(u64::from);
        values[system_u64::CPU_KERNEL] = snapshot.cpu_kernel_usage_percent.map(u64::from);
        values[system_u64::DISK_READ] = snapshot.disk_read_bytes_per_sec;
        values[system_u64::DISK_WRITE] = snapshot.disk_write_bytes_per_sec;
        values[system_u64::NETWORK_RECEIVED] = snapshot.network_received_bytes_per_sec;
        values[system_u64::NETWORK_SENT] = snapshot.network_sent_bytes_per_sec;
        Self(
            values,
            snapshot.disk_queue_length.filter(|value| value.is_finite()),
            gpu_samples,
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3ProcessSample(
    pub(crate) u32,
    #[serde(deserialize_with = "deserialize_process_f64_values")]
    pub(crate)  [Option<f64>; PROCESS_F64_FIELD_COUNT],
    #[serde(deserialize_with = "deserialize_process_u64_values")]
    pub(crate)  [Option<u64>; PROCESS_U64_FIELD_COUNT],
);

fn deserialize_process_f64_values<'de, D>(
    deserializer: D,
) -> Result<[Option<f64>; PROCESS_F64_FIELD_COUNT], D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<Option<f64>>::deserialize(deserializer)?;
    let mut result = [None; PROCESS_F64_FIELD_COUNT];
    for (target, value) in result.iter_mut().zip(values) {
        *target = value;
    }
    Ok(result)
}

fn deserialize_process_u64_values<'de, D>(
    deserializer: D,
) -> Result<[Option<u64>; PROCESS_U64_FIELD_COUNT], D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<Option<u64>>::deserialize(deserializer)?;
    let mut result = [None; PROCESS_U64_FIELD_COUNT];
    for (target, value) in result.iter_mut().zip(values) {
        *target = value;
    }
    Ok(result)
}

impl V3ProcessSample {
    pub(crate) fn from_row(process_id: u32, process: &ProcessRow) -> Self {
        let mut floats = [None; PROCESS_F64_FIELD_COUNT];
        floats[process_f64::CPU_PERCENT] = process.cpu_percent.filter(|value| value.is_finite());
        floats[process_f64::GPU_PERCENT] = process.gpu_percent.filter(|value| value.is_finite());

        let mut integers = [None; PROCESS_U64_FIELD_COUNT];
        integers[process_u64::PRIVATE_BYTES] = process.private_bytes;
        integers[process_u64::WORKSET_BYTES] = process.workset_bytes;
        integers[process_u64::WORKSET_PRIVATE_BYTES] = process.workset_private_bytes;
        integers[process_u64::WORKSET_SHAREABLE_BYTES] = process.workset_shareable_bytes;
        integers[process_u64::THREAD_COUNT] = process.thread_count;
        integers[process_u64::HANDLE_COUNT] = process.handle_count;
        integers[process_u64::USER_OBJECT_COUNT] = process.user_object_count;
        integers[process_u64::GDI_OBJECT_COUNT] = process.gdi_object_count;
        integers[process_u64::GPU_DEDICATED_BYTES] = process.gpu_dedicated_bytes;
        integers[process_u64::GPU_SHARED_BYTES] = process.gpu_shared_bytes;
        integers[process_u64::DOTNET_HEAP_BYTES] = process.dotnet_heap_bytes;
        integers[process_u64::IO_READ_BYTES_PER_SEC] = process.io_read_bytes_per_sec;
        integers[process_u64::IO_WRITE_BYTES_PER_SEC] = process.io_write_bytes_per_sec;
        integers[process_u64::DOTNET_GC_COMMITTED_BYTES] = process.dotnet_gc_committed_bytes;
        integers[process_u64::DOTNET_GC_FRAGMENTATION_BYTES] =
            process.dotnet_gc_fragmentation_bytes;
        integers[process_u64::DOTNET_ALLOCATION_BYTES_PER_SEC] =
            process.dotnet_allocation_bytes_per_sec;
        integers[process_u64::DOTNET_GC_GEN0_HEAP_BYTES] = process.dotnet_gc_gen0_heap_bytes;
        integers[process_u64::DOTNET_GC_GEN1_HEAP_BYTES] = process.dotnet_gc_gen1_heap_bytes;
        integers[process_u64::DOTNET_GC_GEN2_HEAP_BYTES] = process.dotnet_gc_gen2_heap_bytes;
        integers[process_u64::DOTNET_GC_LOH_BYTES] = process.dotnet_gc_loh_bytes;
        integers[process_u64::DOTNET_GC_POH_BYTES] = process.dotnet_gc_poh_bytes;
        Self(process_id, floats, integers)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct V3GpuSample(
    pub(crate) u32,
    pub(crate) [Option<f64>; GPU_F64_FIELD_COUNT],
    pub(crate) [Option<u64>; GPU_U64_FIELD_COUNT],
);

impl V3GpuSample {
    pub(crate) fn from_adapter(adapter_id: u32, adapter: &GpuAdapterSample) -> Self {
        let mut floats = [None; GPU_F64_FIELD_COUNT];
        floats[gpu_f64::UTILIZATION_PERCENT] = adapter
            .utilization_percent
            .filter(|value| value.is_finite());
        floats[gpu_f64::ENCODE_AVERAGE_PERCENT] = adapter
            .encode
            .average_percent
            .filter(|value| value.is_finite());
        floats[gpu_f64::ENCODE_MAX_PERCENT] =
            adapter.encode.max_percent.filter(|value| value.is_finite());
        floats[gpu_f64::DECODE_AVERAGE_PERCENT] = adapter
            .decode
            .average_percent
            .filter(|value| value.is_finite());
        floats[gpu_f64::DECODE_MAX_PERCENT] =
            adapter.decode.max_percent.filter(|value| value.is_finite());

        let mut integers = [None; GPU_U64_FIELD_COUNT];
        integers[gpu_u64::ENCODE_ENGINE_COUNT] = Some(u64::from(adapter.encode.engine_count));
        integers[gpu_u64::DECODE_ENGINE_COUNT] = Some(u64::from(adapter.decode.engine_count));
        integers[gpu_u64::DEDICATED_BYTES] = adapter.dedicated_used;
        integers[gpu_u64::DEDICATED_TOTAL_BYTES] = adapter.dedicated_total;
        integers[gpu_u64::SHARED_BYTES] = adapter.shared_used;
        integers[gpu_u64::SHARED_TOTAL_BYTES] = adapter.shared_total;
        Self(adapter_id, floats, integers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_process_sample_pads_older_shorter_metric_arrays() {
        let sample: V3ProcessSample =
            serde_json::from_str("[7,[12.5,5.5],[1,2,3,4,5,6,7,8,9,10,11,12,13]]").unwrap();

        assert_eq!(sample.0, 7);
        assert_eq!(sample.1[process_f64::CPU_PERCENT], Some(12.5));
        assert_eq!(sample.1[2], None);
        assert_eq!(sample.2[process_u64::IO_WRITE_BYTES_PER_SEC], Some(13));
        assert_eq!(sample.2[process_u64::DOTNET_GC_COMMITTED_BYTES], None);
        assert_eq!(sample.2[process_u64::DOTNET_GC_GEN0_HEAP_BYTES], None);
        assert_eq!(sample.2[process_u64::DOTNET_GC_POH_BYTES], None);
    }

    #[test]
    fn documented_v3_schema_matches_compact_field_counts() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../docs/schemas/recording-v3-line.schema.json"
        ))
        .expect("recording-v3-line.schema.json must be valid JSON");

        assert_eq!(
            schema.pointer("/$defs/session/properties/v/const"),
            Some(&serde_json::json!(CURRENT_LOG_SCHEMA_VERSION))
        );

        for (definition, expected) in [
            ("systemU64Values", SYSTEM_U64_FIELD_COUNT),
            ("processF64Values", PROCESS_F64_FIELD_COUNT),
            ("processU64Values", PROCESS_U64_FIELD_COUNT),
            ("gpuF64Values", GPU_F64_FIELD_COUNT),
            ("gpuU64Values", GPU_U64_FIELD_COUNT),
        ] {
            let pointer = format!("/$defs/{definition}");
            let field_schema = schema
                .pointer(&pointer)
                .unwrap_or_else(|| panic!("missing schema definition {definition}"));
            assert_eq!(
                field_schema
                    .get("maxItems")
                    .and_then(serde_json::Value::as_u64),
                Some(expected as u64),
                "schema maxItems drifted for {definition}"
            );
            assert_eq!(
                field_schema
                    .get("prefixItems")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len),
                Some(expected),
                "schema prefixItems drifted for {definition}"
            );
        }
    }
}
