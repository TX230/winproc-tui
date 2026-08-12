use std::{
    collections::HashMap,
    mem::zeroed,
    ptr::{null, null_mut},
};

use winapi::{
    ctypes::c_void,
    shared::{
        dxgi::{
            CreateDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_REMOTE,
            DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory1, IID_IDXGIFactory1,
        },
        winerror::DXGI_ERROR_NOT_FOUND,
    },
    um::pdh::{
        PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhOpenQueryW,
    },
};

use crate::{
    model::{
        GpuAdapterId, GpuAdapterSample, GpuEngineSummary, GpuSample, ProcessExtraMetrics,
        ProcessGpuSample,
    },
    platform::{to_wide, wide_slice_to_string},
    samplers::pdh::{
        add_optional_pdh_counter, ensure_pdh_success, pdh_ok, read_named_counter_double_items,
        read_named_counter_items,
    },
};

pub(crate) struct GpuSampler {
    query: PDH_HQUERY,
    engine_counter: PDH_HCOUNTER,
    process_dedicated_counter: Option<PDH_HCOUNTER>,
    process_shared_counter: Option<PDH_HCOUNTER>,
    adapter_dedicated_counter: Option<PDH_HCOUNTER>,
    adapter_shared_counter: Option<PDH_HCOUNTER>,
    adapters: Vec<GpuAdapterSample>,
    sample_index: u64,
}

impl GpuSampler {
    pub(crate) fn new() -> anyhow::Result<Self> {
        unsafe {
            let mut query: PDH_HQUERY = null_mut();
            ensure_pdh_success(PdhOpenQueryW(null(), 0, &mut query), "opening GPU query")?;

            let mut engine_counter: PDH_HCOUNTER = null_mut();
            let result = (|| {
                ensure_pdh_success(
                    PdhAddEnglishCounterW(
                        query,
                        to_wide("\\GPU Engine(pid_*)\\Utilization Percentage").as_ptr(),
                        0,
                        &mut engine_counter,
                    ),
                    "adding GPU engine utilization counter",
                )?;
                let process_dedicated_counter =
                    add_optional_pdh_counter(query, "\\GPU Process Memory(pid_*)\\Local Usage");
                let process_shared_counter =
                    add_optional_pdh_counter(query, "\\GPU Process Memory(pid_*)\\Non Local Usage");
                let adapter_dedicated_counter =
                    add_optional_pdh_counter(query, "\\GPU Adapter Memory(*)\\Dedicated Usage");
                let adapter_shared_counter =
                    add_optional_pdh_counter(query, "\\GPU Adapter Memory(*)\\Shared Usage");
                ensure_pdh_success(PdhCollectQueryData(query), "priming GPU query")?;
                Ok(Self {
                    query,
                    engine_counter,
                    process_dedicated_counter,
                    process_shared_counter,
                    adapter_dedicated_counter,
                    adapter_shared_counter,
                    adapters: collect_gpu_adapters(),
                    sample_index: 0,
                })
            })();

            if result.is_err() {
                PdhCloseQuery(query);
            }
            result
        }
    }

    pub(crate) fn sample(&mut self) -> Option<GpuSample> {
        self.sample_index = self.sample_index.saturating_add(1);
        if self.sample_index.is_multiple_of(5) {
            let refreshed = collect_gpu_adapters();
            if !refreshed.is_empty() && !same_adapter_configuration(&self.adapters, &refreshed) {
                self.adapters = refreshed;
            }
        }
        unsafe {
            if !pdh_ok(PdhCollectQueryData(self.query)) {
                return Some(GpuSample {
                    adapters: self.adapters.clone(),
                    ..GpuSample::default()
                });
            }
        }

        let engine_items = read_named_counter_double_items(self.engine_counter).unwrap_or_default();
        let process_dedicated = read_u64_items(self.process_dedicated_counter);
        let process_shared = read_u64_items(self.process_shared_counter);
        let adapter_dedicated = read_u64_items(self.adapter_dedicated_counter);
        let adapter_shared = read_u64_items(self.adapter_shared_counter);
        Some(build_gpu_sample(
            &self.adapters,
            engine_items,
            process_dedicated,
            process_shared,
            adapter_dedicated,
            adapter_shared,
        ))
    }
}

fn same_adapter_configuration(left: &[GpuAdapterSample], right: &[GpuAdapterSample]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.id == right.id
                && left.name == right.name
                && left.dedicated_total == right.dedicated_total
                && left.shared_total == right.shared_total
        })
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

pub(super) fn merge_process_gpu_metrics(
    extras: &mut HashMap<u32, ProcessExtraMetrics>,
    sample: &GpuSample,
) {
    for (pid, gpu) in &sample.processes {
        let metric = extras.entry(*pid).or_default();
        metric.gpu_percent = gpu.utilization_percent;
        metric.gpu_dedicated_bytes = gpu.dedicated_bytes;
        metric.gpu_shared_bytes = gpu.shared_bytes;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PhysicalEngineKey {
    adapter: GpuAdapterId,
    physical: u32,
    engine: u32,
    kind: String,
}

#[derive(Debug, Clone)]
struct EngineInstance {
    pid: u32,
    key: PhysicalEngineKey,
}

fn build_gpu_sample(
    capacities: &[GpuAdapterSample],
    engine_items: Vec<(String, f64)>,
    process_dedicated_items: Vec<(String, u64)>,
    process_shared_items: Vec<(String, u64)>,
    adapter_dedicated_items: Vec<(String, u64)>,
    adapter_shared_items: Vec<(String, u64)>,
) -> GpuSample {
    let mut physical_totals = HashMap::<PhysicalEngineKey, f64>::new();
    let mut process_engine_totals = HashMap::<u32, HashMap<PhysicalEngineKey, f64>>::new();
    for (name, value) in engine_items {
        let Some(instance) = parse_engine_instance(&name) else {
            continue;
        };
        let value = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        *physical_totals.entry(instance.key.clone()).or_default() += value;
        *process_engine_totals
            .entry(instance.pid)
            .or_default()
            .entry(instance.key)
            .or_default() += value;
    }

    for value in physical_totals.values_mut() {
        *value = value.clamp(0.0, 100.0);
    }
    for engines in process_engine_totals.values_mut() {
        for value in engines.values_mut() {
            *value = value.clamp(0.0, 100.0);
        }
    }

    let mut adapters = capacities.to_vec();
    let dedicated_by_adapter = adapter_memory_map(&adapter_dedicated_items);
    let shared_by_adapter = adapter_memory_map(&adapter_shared_items);

    for adapter in &mut adapters {
        let engine_values = physical_totals
            .iter()
            .filter(|(key, _)| key.adapter == adapter.id)
            .collect::<Vec<_>>();
        adapter.utilization_percent = engine_values
            .iter()
            .map(|(_, value)| **value)
            .reduce(f64::max);
        adapter.encode = summarize_engines(
            engine_values
                .iter()
                .filter(|(key, _)| key.kind.eq_ignore_ascii_case("VideoEncode"))
                .map(|(_, value)| **value),
        );
        adapter.decode = summarize_engines(
            engine_values
                .iter()
                .filter(|(key, _)| key.kind.eq_ignore_ascii_case("VideoDecode"))
                .map(|(_, value)| **value),
        );
        adapter.dedicated_used = dedicated_by_adapter.get(&adapter.id).copied();
        adapter.shared_used = shared_by_adapter.get(&adapter.id).copied();
    }

    let process_dedicated = process_memory_map(&process_dedicated_items);
    let process_shared = process_memory_map(&process_shared_items);
    let mut processes = HashMap::<u32, ProcessGpuSample>::new();
    for (pid, engines) in process_engine_totals {
        processes.entry(pid).or_default().utilization_percent =
            Some(engines.values().copied().sum::<f64>().clamp(0.0, 100.0));
    }
    for (pid, value) in process_dedicated {
        processes.entry(pid).or_default().dedicated_bytes = Some(value);
    }
    for (pid, value) in process_shared {
        processes.entry(pid).or_default().shared_bytes = Some(value);
    }

    GpuSample {
        adapters,
        processes,
    }
}

fn summarize_engines(values: impl Iterator<Item = f64>) -> GpuEngineSummary {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return GpuEngineSummary::default();
    }
    let count = values.len() as u32;
    let sum = values.iter().sum::<f64>();
    GpuEngineSummary {
        average_percent: Some((sum / f64::from(count)).clamp(0.0, 100.0)),
        max_percent: values.into_iter().reduce(f64::max),
        engine_count: count,
    }
}

fn read_u64_items(counter: Option<PDH_HCOUNTER>) -> Vec<(String, u64)> {
    counter
        .and_then(read_named_counter_items)
        .unwrap_or_default()
}

fn process_memory_map(items: &[(String, u64)]) -> HashMap<u32, u64> {
    let mut values = HashMap::new();
    for (name, value) in items {
        let Some(pid) = parse_pid_from_gpu_instance(name) else {
            continue;
        };
        let entry = values.entry(pid).or_insert(0u64);
        *entry = entry.saturating_add(*value);
    }
    values
}

fn adapter_memory_map(items: &[(String, u64)]) -> HashMap<GpuAdapterId, u64> {
    let mut values = HashMap::new();
    for (name, value) in items {
        let Some(id) = parse_luid(name) else {
            continue;
        };
        let entry = values.entry(id).or_insert(0u64);
        *entry = entry.saturating_add(*value);
    }
    values
}

fn parse_engine_instance(name: &str) -> Option<EngineInstance> {
    Some(EngineInstance {
        pid: parse_pid_from_gpu_instance(name)?,
        key: PhysicalEngineKey {
            adapter: parse_luid(name)?,
            physical: parse_decimal_token(name, "phys_")?,
            engine: parse_decimal_token(name, "eng_")?,
            kind: name.split("engtype_").nth(1)?.to_string(),
        },
    })
}

fn parse_luid(name: &str) -> Option<GpuAdapterId> {
    let tail = name.split("luid_").nth(1)?;
    let mut parts = tail.split('_');
    let high = parse_hex_token(parts.next()?)?;
    let low = parse_hex_token(parts.next()?)?;
    Some(GpuAdapterId { high, low })
}

fn parse_hex_token(value: &str) -> Option<u32> {
    u32::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16).ok()
}

fn parse_decimal_token(name: &str, prefix: &str) -> Option<u32> {
    name.split(prefix)
        .nth(1)?
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn parse_pid_from_gpu_instance(instance_name: &str) -> Option<u32> {
    parse_decimal_token(instance_name, "pid_")
}

fn collect_gpu_adapters() -> Vec<GpuAdapterSample> {
    unsafe {
        let mut factory: *mut IDXGIFactory1 = null_mut();
        let status = CreateDXGIFactory1(
            &IID_IDXGIFactory1,
            &mut factory as *mut _ as *mut *mut c_void,
        );
        if !hresult_succeeded(status) || factory.is_null() {
            return Vec::new();
        }

        let mut adapters = Vec::new();
        let mut index = 0u32;
        loop {
            let mut adapter: *mut IDXGIAdapter1 = null_mut();
            let status = (*factory).EnumAdapters1(index, &mut adapter);
            if status == DXGI_ERROR_NOT_FOUND {
                break;
            }
            if !hresult_succeeded(status) || adapter.is_null() {
                break;
            }

            let mut desc: DXGI_ADAPTER_DESC1 = zeroed();
            let got_desc = hresult_succeeded((*adapter).GetDesc1(&mut desc));
            (*adapter).Release();
            if got_desc && !is_filtered_dxgi_adapter(desc.Flags) {
                let name = wide_slice_to_string(&desc.Description);
                adapters.push(GpuAdapterSample {
                    id: GpuAdapterId {
                        high: desc.AdapterLuid.HighPart as u32,
                        low: desc.AdapterLuid.LowPart,
                    },
                    name: (!name.is_empty()).then_some(name),
                    dedicated_total: (desc.DedicatedVideoMemory > 0)
                        .then_some(desc.DedicatedVideoMemory as u64),
                    shared_total: (desc.SharedSystemMemory > 0)
                        .then_some(desc.SharedSystemMemory as u64),
                    ..GpuAdapterSample::default()
                });
            }
            index = index.saturating_add(1);
        }
        (*factory).Release();
        adapters
    }
}

#[cfg(test)]
pub(crate) fn is_filtered_dxgi_adapter(flags: u32) -> bool {
    (flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0 || (flags & DXGI_ADAPTER_FLAG_REMOTE) != 0
}

#[cfg(not(test))]
fn is_filtered_dxgi_adapter(flags: u32) -> bool {
    (flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0 || (flags & DXGI_ADAPTER_FLAG_REMOTE) != 0
}

fn hresult_succeeded(status: i32) -> bool {
    status >= 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_extracts_gpu_engine_identity() {
        let item = parse_engine_instance(
            "pid_1200_luid_0x00000001_0x00000002_phys_0_eng_3_engtype_VideoEncode",
        )
        .unwrap();
        assert_eq!(item.pid, 1200);
        assert_eq!(item.key.adapter, GpuAdapterId { high: 1, low: 2 });
        assert_eq!(item.key.physical, 0);
        assert_eq!(item.key.engine, 3);
        assert_eq!(item.key.kind, "VideoEncode");
    }

    #[test]
    fn gpu_sample_uses_busiest_engine_and_reports_encode_average_max_and_count() {
        let adapter_id = GpuAdapterId { high: 1, low: 2 };
        let sample = build_gpu_sample(
            &[GpuAdapterSample {
                id: adapter_id,
                name: Some("GPU".to_string()),
                ..GpuAdapterSample::default()
            }],
            vec![
                (
                    "pid_10_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                    70.0,
                ),
                (
                    "pid_20_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                    20.0,
                ),
                (
                    "pid_10_luid_0x1_0x2_phys_0_eng_1_engtype_VideoEncode".to_string(),
                    40.0,
                ),
                (
                    "pid_20_luid_0x1_0x2_phys_0_eng_2_engtype_VideoEncode".to_string(),
                    20.0,
                ),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let adapter = &sample.adapters[0];
        assert_eq!(adapter.utilization_percent, Some(90.0));
        assert_eq!(adapter.encode.average_percent, Some(30.0));
        assert_eq!(adapter.encode.max_percent, Some(40.0));
        assert_eq!(adapter.encode.engine_count, 2);
        assert_eq!(sample.processes[&10].utilization_percent, Some(100.0));
    }

    #[test]
    fn gpu_sample_keeps_adapters_separate_and_skips_untyped_instances() {
        let first = GpuAdapterId { high: 1, low: 2 };
        let second = GpuAdapterId { high: 3, low: 4 };
        let sample = build_gpu_sample(
            &[
                GpuAdapterSample {
                    id: first,
                    name: Some("GPU 0".to_string()),
                    dedicated_total: Some(8_000),
                    ..GpuAdapterSample::default()
                },
                GpuAdapterSample {
                    id: second,
                    name: Some("GPU 1".to_string()),
                    dedicated_total: Some(16_000),
                    ..GpuAdapterSample::default()
                },
            ],
            vec![
                (
                    "pid_10_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                    25.0,
                ),
                (
                    "pid_20_luid_0x3_0x4_phys_0_eng_0_engtype_3D".to_string(),
                    75.0,
                ),
                ("pid_30_luid_0x3_0x4_phys_0_eng_1".to_string(), 99.0),
            ],
            Vec::new(),
            Vec::new(),
            vec![
                ("luid_0x1_0x2_phys_0".to_string(), 1_000),
                ("luid_0x3_0x4_phys_0".to_string(), 2_000),
            ],
            Vec::new(),
        );

        assert_eq!(sample.adapters.len(), 2);
        assert_eq!(sample.adapters[0].utilization_percent, Some(25.0));
        assert_eq!(sample.adapters[0].dedicated_used, Some(1_000));
        assert_eq!(sample.adapters[0].dedicated_total, Some(8_000));
        assert_eq!(sample.adapters[1].utilization_percent, Some(75.0));
        assert_eq!(sample.adapters[1].dedicated_used, Some(2_000));
        assert_eq!(sample.adapters[1].dedicated_total, Some(16_000));
        assert!(!sample.processes.contains_key(&30));
    }

    #[test]
    fn gpu_sample_does_not_promote_pdh_only_adapters() {
        let hardware = GpuAdapterId { high: 1, low: 2 };
        let pdh_only = GpuAdapterId { high: 3, low: 4 };
        let sample = build_gpu_sample(
            &[GpuAdapterSample {
                id: hardware,
                name: Some("Hardware GPU".to_string()),
                dedicated_total: Some(8_000),
                ..GpuAdapterSample::default()
            }],
            vec![
                (
                    "pid_10_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                    25.0,
                ),
                (
                    "pid_20_luid_0x3_0x4_phys_0_eng_0_engtype_3D".to_string(),
                    75.0,
                ),
            ],
            Vec::new(),
            Vec::new(),
            vec![
                ("luid_0x1_0x2_phys_0".to_string(), 1_000),
                ("luid_0x3_0x4_phys_0".to_string(), 2_000),
            ],
            Vec::new(),
        );

        assert_eq!(sample.adapters.len(), 1);
        assert_eq!(sample.adapters[0].id, hardware);
        assert_eq!(sample.adapters[0].utilization_percent, Some(25.0));
        assert_eq!(sample.adapters[0].dedicated_used, Some(1_000));
        assert_eq!(sample.processes[&20].utilization_percent, Some(75.0));
        assert!(!sample.adapters.iter().any(|adapter| adapter.id == pdh_only));
    }

    #[test]
    fn gpu_sample_requires_dxgi_catalog_for_system_adapters() {
        let sample = build_gpu_sample(
            &[],
            vec![(
                "pid_20_luid_0x3_0x4_phys_0_eng_0_engtype_3D".to_string(),
                75.0,
            )],
            Vec::new(),
            Vec::new(),
            vec![("luid_0x3_0x4_phys_0".to_string(), 2_000)],
            Vec::new(),
        );

        assert!(sample.adapters.is_empty());
        assert_eq!(sample.processes[&20].utilization_percent, Some(75.0));
    }
}
