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
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW,
    },
};

use crate::{
    model::{GpuCapacitySample, GpuUsageSample, ProcessExtraMetrics},
    platform::{to_wide, wide_slice_to_string},
    samplers::pdh::{
        ensure_pdh_success, pdh_ok, read_named_counter_double_items, read_named_counter_items,
    },
};

const PDH_MORE_DATA_STATUS: u32 = 0x8000_07D2;

pub(crate) struct GpuSampler {
    utilization_query: PDH_HQUERY,
    utilization_counter: PDH_HCOUNTER,
}

impl GpuSampler {
    pub(crate) fn new() -> anyhow::Result<Self> {
        unsafe {
            let mut utilization_query: PDH_HQUERY = null_mut();
            ensure_pdh_success(
                PdhOpenQueryW(null(), 0, &mut utilization_query),
                "opening GPU utilization query",
            )?;

            let mut utilization_counter: PDH_HCOUNTER = null_mut();
            let result = (|| {
                ensure_pdh_success(
                    PdhAddEnglishCounterW(
                        utilization_query,
                        to_wide("\\GPU Engine(pid_*)\\Utilization Percentage").as_ptr(),
                        0,
                        &mut utilization_counter,
                    ),
                    "adding \\GPU Engine(pid_*)\\Utilization Percentage",
                )?;
                ensure_pdh_success(
                    PdhCollectQueryData(utilization_query),
                    "priming GPU utilization query",
                )?;
                Ok(Self {
                    utilization_query,
                    utilization_counter,
                })
            })();

            if result.is_err() {
                PdhCloseQuery(utilization_query);
            }
            result
        }
    }

    fn sample_utilization_map(&mut self) -> Option<HashMap<u32, f64>> {
        unsafe {
            if !pdh_ok(PdhCollectQueryData(self.utilization_query)) {
                return None;
            }
        }

        let items = read_named_counter_double_items(self.utilization_counter)?;
        Some(gpu_utilization_map_from_items(items))
    }
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.utilization_query);
        }
    }
}

pub(super) fn merge_gpu_usage(
    extras: &mut HashMap<u32, ProcessExtraMetrics>,
    gpu_sampler: Option<&mut GpuSampler>,
) {
    if let Some(dedicated) = collect_gpu_counter_map("\\GPU Process Memory(pid_*)\\Local Usage") {
        for (pid, value) in dedicated {
            extras.entry(pid).or_default().gpu_dedicated_bytes = Some(value);
        }
    }

    if let Some(shared) = collect_gpu_counter_map("\\GPU Process Memory(pid_*)\\Non Local Usage") {
        for (pid, value) in shared {
            extras.entry(pid).or_default().gpu_shared_bytes = Some(value);
        }
    }

    if let Some(utilization) = gpu_sampler.and_then(GpuSampler::sample_utilization_map) {
        for (pid, value) in utilization {
            extras.entry(pid).or_default().gpu_percent = Some(value);
        }
    }
}

pub(crate) fn collect_process_gpu_usage(
    extras: &HashMap<u32, ProcessExtraMetrics>,
) -> GpuUsageSample {
    GpuUsageSample {
        dedicated: sum_gpu_usage(extras.values().filter_map(|item| item.gpu_dedicated_bytes)),
        shared: sum_gpu_usage(extras.values().filter_map(|item| item.gpu_shared_bytes)),
    }
}

pub(crate) fn collect_gpu_summary_usage() -> GpuUsageSample {
    GpuUsageSample {
        dedicated: collect_gpu_total_counter("\\GPU Adapter Memory(*)\\Dedicated Usage"),
        shared: collect_gpu_total_counter("\\GPU Adapter Memory(*)\\Shared Usage"),
    }
}

pub(crate) fn collect_gpu_capacity() -> GpuCapacitySample {
    unsafe {
        let mut factory: *mut IDXGIFactory1 = null_mut();
        let status = CreateDXGIFactory1(
            &IID_IDXGIFactory1,
            &mut factory as *mut _ as *mut *mut c_void,
        );
        if !hresult_succeeded(status) || factory.is_null() {
            return GpuCapacitySample::default();
        }

        let result = (|| {
            let mut dedicated_total = 0u64;
            let mut shared_total = 0u64;
            let mut has_dedicated_total = false;
            let mut has_shared_total = false;
            let mut adapter_names = Vec::new();
            let mut index = 0u32;

            loop {
                let mut adapter: *mut IDXGIAdapter1 = null_mut();
                let status = (*factory).EnumAdapters1(index, &mut adapter);
                if status == DXGI_ERROR_NOT_FOUND {
                    break;
                }
                if !hresult_succeeded(status) || adapter.is_null() {
                    return GpuCapacitySample::default();
                }

                let mut desc: DXGI_ADAPTER_DESC1 = zeroed();
                let got_desc = hresult_succeeded((*adapter).GetDesc1(&mut desc));
                (*adapter).Release();

                if !got_desc || is_filtered_dxgi_adapter(desc.Flags) {
                    index = index.saturating_add(1);
                    continue;
                }

                let name = wide_slice_to_string(&desc.Description);
                if !name.is_empty() {
                    adapter_names.push(name);
                }

                let dedicated = desc.DedicatedVideoMemory as u64;
                if dedicated > 0 {
                    dedicated_total = dedicated_total.saturating_add(dedicated);
                    has_dedicated_total = true;
                }

                let shared = desc.SharedSystemMemory as u64;
                if shared > 0 {
                    shared_total = shared_total.max(shared);
                    has_shared_total = true;
                }

                index = index.saturating_add(1);
            }

            GpuCapacitySample {
                dedicated_total: has_dedicated_total.then_some(dedicated_total),
                shared_total: has_shared_total.then_some(shared_total),
                name: (!adapter_names.is_empty()).then_some(adapter_names.join(", ")),
            }
        })();

        (*factory).Release();
        result
    }
}

fn collect_named_counter_map(counter_path: &str) -> Option<HashMap<String, u64>> {
    unsafe {
        let mut query: PDH_HQUERY = null_mut();
        if !pdh_ok(PdhOpenQueryW(null(), 0, &mut query)) {
            return None;
        }

        let result = (|| {
            let mut counter: PDH_HCOUNTER = null_mut();
            let wide_path = to_wide(counter_path);
            if !pdh_ok(PdhAddEnglishCounterW(
                query,
                wide_path.as_ptr(),
                0,
                &mut counter,
            )) {
                return None;
            }

            if !pdh_ok(PdhCollectQueryData(query)) {
                return None;
            }

            read_named_counter_map(counter)
        })();

        PdhCloseQuery(query);
        result
    }
}

fn read_named_counter_map(counter: PDH_HCOUNTER) -> Option<HashMap<String, u64>> {
    let items = read_named_counter_items(counter)?;
    let mut values = HashMap::new();
    for (name, value) in items {
        values.insert(name, value);
    }
    Some(values)
}

fn gpu_utilization_map_from_items(items: Vec<(String, f64)>) -> HashMap<u32, f64> {
    let mut values = HashMap::new();

    for (instance_name, value) in items {
        let Some(pid) = parse_pid_from_gpu_instance(&instance_name) else {
            continue;
        };
        let entry = values.entry(pid).or_insert(0.0);
        *entry += value;
    }

    for value in values.values_mut() {
        *value = value.clamp(0.0, 100.0);
    }

    values
}

fn collect_gpu_counter_map(counter_path: &str) -> Option<HashMap<u32, u64>> {
    let items = collect_named_counter_map(counter_path)?;
    let mut values = HashMap::new();

    for (instance_name, value) in items {
        let Some(pid) = parse_pid_from_gpu_instance(&instance_name) else {
            continue;
        };
        *values.entry(pid).or_insert(0) += value;
    }

    Some(values)
}

fn collect_gpu_total_counter(counter_path: &str) -> Option<u64> {
    unsafe {
        let mut query: PDH_HQUERY = null_mut();
        if !pdh_ok(PdhOpenQueryW(null(), 0, &mut query)) {
            return None;
        }

        let result = (|| {
            let mut counter: PDH_HCOUNTER = null_mut();
            let wide_path = to_wide(counter_path);
            if !pdh_ok(PdhAddEnglishCounterW(
                query,
                wide_path.as_ptr(),
                0,
                &mut counter,
            )) {
                return None;
            }

            if !pdh_ok(PdhCollectQueryData(query)) {
                return None;
            }

            let mut buffer_size = 0u32;
            let mut item_count = 0u32;
            let first_status = PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut buffer_size,
                &mut item_count,
                null_mut(),
            );
            if first_status as u32 != PDH_MORE_DATA_STATUS || buffer_size == 0 {
                return None;
            }

            let mut buffer = vec![0u8; buffer_size as usize];
            let item_ptr = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if !pdh_ok(PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut buffer_size,
                &mut item_count,
                item_ptr,
            )) {
                return None;
            }

            let items = std::slice::from_raw_parts(item_ptr, item_count as usize);
            let mut total = 0u64;
            let mut found = false;
            for item in items {
                if item.szName.is_null() {
                    continue;
                }
                let value = *item.FmtValue.u.largeValue();
                if value < 0 {
                    continue;
                }
                total = total.saturating_add(value as u64);
                found = true;
            }

            found.then_some(total)
        })();

        PdhCloseQuery(query);
        result
    }
}

fn sum_gpu_usage(values: impl Iterator<Item = u64>) -> Option<u64> {
    let mut total = 0u64;
    let mut found = false;
    for value in values {
        total = total.saturating_add(value);
        found = true;
    }
    found.then_some(total)
}

#[cfg(test)]
pub(crate) fn is_filtered_dxgi_adapter(flags: u32) -> bool {
    (flags & DXGI_ADAPTER_FLAG_SOFTWARE as u32) != 0
        || (flags & DXGI_ADAPTER_FLAG_REMOTE as u32) != 0
}

#[cfg(not(test))]
fn is_filtered_dxgi_adapter(flags: u32) -> bool {
    (flags & DXGI_ADAPTER_FLAG_SOFTWARE as u32) != 0
        || (flags & DXGI_ADAPTER_FLAG_REMOTE as u32) != 0
}

fn hresult_succeeded(status: i32) -> bool {
    status >= 0
}

fn parse_pid_from_gpu_instance(instance_name: &str) -> Option<u32> {
    let start = instance_name.find("pid_")? + 4;
    let digits = instance_name[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::gpu_utilization_map_from_items;

    #[test]
    fn gpu_utilization_sums_engine_instances_by_pid() {
        let values = gpu_utilization_map_from_items(vec![
            (
                "pid_1200_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                12.5,
            ),
            (
                "pid_1200_luid_0x1_0x2_phys_0_eng_1_engtype_Copy".to_string(),
                3.25,
            ),
            (
                "pid_1300_luid_0x1_0x2_phys_0_eng_0_engtype_3D".to_string(),
                125.0,
            ),
            ("engtype_3D".to_string(), 50.0),
        ]);

        assert_eq!(values.get(&1200), Some(&15.75));
        assert_eq!(values.get(&1300), Some(&100.0));
        assert!(!values.contains_key(&0));
    }
}
