use std::{
    collections::HashMap,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use winapi::{
    ctypes::c_void,
    shared::{minwindef::DWORD, ntdef::HANDLE, winerror::ERROR_BAD_LENGTH},
    um::{
        errhandlingapi::GetLastError,
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        pdh::{
            PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
            PdhOpenQueryW,
        },
        processthreadsapi::{GetProcessHandleCount, OpenProcess},
        psapi::QueryWorkingSet,
        sysinfoapi::{GetSystemInfo, SYSTEM_INFO},
        tlhelp32::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        winnt::{PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
    },
};

use crate::{
    model::{ProcessExtraMetrics, WorkingSetShareSample},
    platform::to_wide,
    samplers::{
        SamplingOptions,
        gpu::{GpuSampler, merge_gpu_usage},
        pdh::{map_process_counter_instances_to_pids, pdh_ok, read_named_counter_items},
    },
};

const GR_GDIOBJECTS: DWORD = 0;
const GR_USEROBJECTS: DWORD = 1;

unsafe extern "system" {
    fn GetGuiResources(hProcess: HANDLE, uiFlags: DWORD) -> DWORD;
}

pub(crate) fn collect_process_extras(
    mut extras: HashMap<u32, ProcessExtraMetrics>,
    collect_slow_metrics: bool,
    options: SamplingOptions,
    gpu_sampler: Option<&mut GpuSampler>,
    cached_slow_extras: &mut HashMap<u32, ProcessExtraMetrics>,
) -> HashMap<u32, ProcessExtraMetrics> {
    merge_process_threads(&mut extras);
    merge_handle_counts(&mut extras);
    if collect_slow_metrics {
        if options.collect_gui_resources {
            merge_gui_resource_counts(&mut extras);
        }
        if options.collect_ws_share {
            merge_working_set_shared_bytes(&mut extras);
        }
        if options.collect_gpu {
            merge_gpu_usage(&mut extras, gpu_sampler);
        }
        *cached_slow_extras = slow_process_extras(&extras);
    } else {
        merge_cached_slow_process_extras(&mut extras, cached_slow_extras);
    }
    extras
}

fn slow_process_extras(
    extras: &HashMap<u32, ProcessExtraMetrics>,
) -> HashMap<u32, ProcessExtraMetrics> {
    extras
        .iter()
        .map(|(pid, metric)| {
            let slow_metric = ProcessExtraMetrics {
                workset_shareable_bytes: metric.workset_shareable_bytes,
                workset_shared_bytes: metric.workset_shared_bytes,
                user_object_count: metric.user_object_count,
                gdi_object_count: metric.gdi_object_count,
                gpu_percent: metric.gpu_percent,
                gpu_dedicated_bytes: metric.gpu_dedicated_bytes,
                gpu_shared_bytes: metric.gpu_shared_bytes,
                ..ProcessExtraMetrics::default()
            };
            (*pid, slow_metric)
        })
        .collect()
}

fn merge_cached_slow_process_extras(
    extras: &mut HashMap<u32, ProcessExtraMetrics>,
    cached_slow_extras: &HashMap<u32, ProcessExtraMetrics>,
) {
    for (pid, metric) in extras.iter_mut() {
        let Some(cached) = cached_slow_extras.get(pid) else {
            continue;
        };
        metric.workset_shareable_bytes = cached.workset_shareable_bytes;
        metric.workset_shared_bytes = cached.workset_shared_bytes;
        metric.user_object_count = cached.user_object_count;
        metric.gdi_object_count = cached.gdi_object_count;
        metric.gpu_percent = cached.gpu_percent;
        metric.gpu_dedicated_bytes = cached.gpu_dedicated_bytes;
        metric.gpu_shared_bytes = cached.gpu_shared_bytes;
    }
}

fn merge_process_threads(extras: &mut HashMap<u32, ProcessExtraMetrics>) {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }

        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                extras.entry(entry.th32ProcessID).or_default().thread_count =
                    Some(entry.cntThreads as u64);

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }
}

fn merge_handle_counts(extras: &mut HashMap<u32, ProcessExtraMetrics>) {
    let mut missing_pids = Vec::new();

    if let Some(pdh_counts) = collect_process_handle_counts() {
        for (pid, metric) in extras.iter_mut() {
            if let Some(handle_count) = pdh_counts.get(pid) {
                metric.handle_count = Some(*handle_count);
            } else {
                missing_pids.push(*pid);
            }
        }
    } else {
        missing_pids.extend(extras.keys().copied());
    }

    for pid in missing_pids {
        let Some(metric) = extras.get_mut(&pid) else {
            continue;
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                continue;
            }

            let mut handle_count = 0u32;
            if GetProcessHandleCount(handle, &mut handle_count) != 0 {
                metric.handle_count = Some(handle_count as u64);
            }

            CloseHandle(handle);
        }
    }
}

fn merge_gui_resource_counts(extras: &mut HashMap<u32, ProcessExtraMetrics>) {
    for (pid, metric) in extras.iter_mut() {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *pid);
            if handle.is_null() {
                continue;
            }

            metric.gdi_object_count = Some(GetGuiResources(handle, GR_GDIOBJECTS) as u64);
            metric.user_object_count = Some(GetGuiResources(handle, GR_USEROBJECTS) as u64);

            CloseHandle(handle);
        }
    }
}

fn merge_working_set_shared_bytes(extras: &mut HashMap<u32, ProcessExtraMetrics>) {
    let page_size = system_page_size();
    for (pid, metric) in extras.iter_mut() {
        let Some(workset_bytes) = metric.workset_bytes else {
            continue;
        };
        let Some(sample) = collect_working_set_share_bytes(*pid, workset_bytes, page_size) else {
            continue;
        };

        metric.workset_shareable_bytes = Some(sample.shareable_bytes);
        metric.workset_shared_bytes = Some(sample.shared_bytes);
    }
}

fn collect_working_set_share_bytes(
    pid: u32,
    workset_bytes: u64,
    page_size: u64,
) -> Option<WorkingSetShareSample> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let result = query_working_set_share_bytes(handle, workset_bytes, page_size);
    unsafe {
        CloseHandle(handle);
    }
    result
}

pub(crate) fn collect_working_set_share_bytes_for_process(
    pid: u32,
    workset_bytes: u64,
) -> Option<WorkingSetShareSample> {
    collect_working_set_share_bytes(pid, workset_bytes, system_page_size())
}

fn query_working_set_share_bytes(
    handle: HANDLE,
    workset_bytes: u64,
    page_size: u64,
) -> Option<WorkingSetShareSample> {
    let estimated_pages = (workset_bytes / page_size).saturating_add(1024);
    let mut entry_capacity = estimated_pages.clamp(1024, 1_000_000) as usize;

    for _ in 0..5 {
        let mut buffer = vec![0usize; entry_capacity.saturating_add(1)];
        let buffer_bytes = buffer
            .len()
            .checked_mul(size_of::<usize>())?
            .min(u32::MAX as usize) as u32;

        let status =
            unsafe { QueryWorkingSet(handle, buffer.as_mut_ptr() as *mut c_void, buffer_bytes) };
        if status == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_BAD_LENGTH {
                return None;
            }
            entry_capacity = entry_capacity.saturating_mul(2).min(1_000_000);
            continue;
        }

        let entry_count = buffer[0].min(entry_capacity);
        let mut shareable_pages = 0u64;
        let mut shared_pages = 0u64;
        for flags in buffer.iter().skip(1).take(entry_count) {
            if working_set_page_is_shareable(*flags) {
                shareable_pages += 1;
            }
            if working_set_page_is_shared(*flags) {
                shared_pages += 1;
            }
        }

        return Some(WorkingSetShareSample {
            shareable_bytes: shareable_pages.saturating_mul(page_size),
            shared_bytes: shared_pages.saturating_mul(page_size),
        });
    }

    None
}

#[cfg(test)]
pub(crate) fn working_set_page_is_shareable(flags: usize) -> bool {
    ((flags >> 8) & 0x1) != 0
}

#[cfg(not(test))]
fn working_set_page_is_shareable(flags: usize) -> bool {
    ((flags >> 8) & 0x1) != 0
}

#[cfg(test)]
pub(crate) fn working_set_page_is_shared(flags: usize) -> bool {
    ((flags >> 5) & 0x7) > 1
}

#[cfg(not(test))]
fn working_set_page_is_shared(flags: usize) -> bool {
    ((flags >> 5) & 0x7) > 1
}

fn system_page_size() -> u64 {
    unsafe {
        let mut info: SYSTEM_INFO = zeroed();
        GetSystemInfo(&mut info);
        u64::from(info.dwPageSize).max(1)
    }
}

fn collect_process_handle_counts() -> Option<HashMap<u32, u64>> {
    unsafe {
        let mut query: PDH_HQUERY = null_mut();
        if !pdh_ok(PdhOpenQueryW(null(), 0, &mut query)) {
            return None;
        }

        let result = (|| {
            let mut handle_count_counter: PDH_HCOUNTER = null_mut();
            if !pdh_ok(PdhAddEnglishCounterW(
                query,
                to_wide("\\Process(*)\\Handle Count").as_ptr(),
                0,
                &mut handle_count_counter,
            )) {
                return None;
            }

            let mut process_id_counter: PDH_HCOUNTER = null_mut();
            if !pdh_ok(PdhAddEnglishCounterW(
                query,
                to_wide("\\Process(*)\\ID Process").as_ptr(),
                0,
                &mut process_id_counter,
            )) {
                return None;
            }

            if !pdh_ok(PdhCollectQueryData(query)) {
                return None;
            }

            let handle_counts = read_named_counter_items(handle_count_counter)?;
            let process_ids = read_named_counter_items(process_id_counter)?;
            Some(map_process_counter_instances_to_pids(
                process_ids,
                handle_counts,
            ))
        })();

        PdhCloseQuery(query);
        result
    }
}
