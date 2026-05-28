use std::{io, mem::size_of, ptr::null_mut};

use sysinfo::System;
use winapi::{
    shared::winerror::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
    um::{
        sysinfoapi::GetLogicalProcessorInformationEx,
        winnt::{
            LTP_PC_SMT, RelationAll, RelationCache, RelationProcessorCore,
            SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
        },
        winreg::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
    },
};

use crate::{
    model::{CpuCoreKind, CpuLogicalProcessorSample, CpuSummarySample},
    platform::{to_wide, wide_slice_to_string},
    ui::fmt_bytes,
};

pub(crate) fn collect_cpu_summary(
    system: &System,
    current_frequencies_mhz: &[(usize, u64)],
) -> CpuSummarySample {
    let cpu = system.cpus().first();
    let name = cpu.map(|cpu| cpu.brand().trim()).unwrap_or_default();
    let topology = collect_cpu_topology();
    let current_frequency_mhz = average_current_frequency_mhz(current_frequencies_mhz, |_, _| true);

    CpuSummarySample {
        name: (!name.is_empty())
            .then_some(name.to_string())
            .or_else(collect_cpu_name_from_registry),
        frequency_mhz: collect_cpu_frequency_mhz().or_else(|| {
            let frequency = cpu.map(|cpu| cpu.frequency()).unwrap_or_default();
            (frequency > 0).then_some(frequency)
        }),
        current_frequency_mhz,
        p_core_frequency_mhz: average_current_frequency_mhz(current_frequencies_mhz, |index, _| {
            cpu_core_kind(index, &topology.logical_efficiency_classes)
                == Some(CpuCoreKind::Performance)
        }),
        e_core_frequency_mhz: average_current_frequency_mhz(current_frequencies_mhz, |index, _| {
            cpu_core_kind(index, &topology.logical_efficiency_classes)
                == Some(CpuCoreKind::Efficiency)
        }),
        total_usage_percent: average_cpu_usage_percent(system),
        logical_processors: collect_logical_processor_usage(
            system,
            &topology.logical_efficiency_classes,
        ),
        topology: format_cpu_topology(
            topology.physical_cores,
            topology.logical_threads,
            topology.smt_enabled,
        ),
        caches: format_cpu_caches(
            topology.l1_cache_bytes,
            topology.l2_cache_bytes,
            topology.l3_cache_bytes,
        ),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CpuTopologySample {
    physical_cores: Option<u32>,
    logical_threads: Option<u32>,
    smt_enabled: Option<bool>,
    l1_cache_bytes: Option<u64>,
    l2_cache_bytes: Option<u64>,
    l3_cache_bytes: Option<u64>,
    logical_efficiency_classes: Vec<Option<u8>>,
}

fn collect_cpu_topology() -> CpuTopologySample {
    unsafe {
        let mut buffer_size = 0u32;
        let first_status =
            GetLogicalProcessorInformationEx(RelationAll, null_mut(), &mut buffer_size);
        if first_status != 0 || buffer_size == 0 {
            return CpuTopologySample::default();
        }
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return CpuTopologySample::default();
        }

        let mut buffer = vec![0u8; buffer_size as usize];
        let status = GetLogicalProcessorInformationEx(
            RelationAll,
            buffer.as_mut_ptr() as *mut _,
            &mut buffer_size,
        );
        if status == 0 {
            return CpuTopologySample::default();
        }

        let mut sample = CpuTopologySample::default();
        let mut physical_cores = 0u32;
        let mut logical_threads = 0u32;
        let mut smt_enabled = false;
        let mut offset = 0usize;

        while offset + size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>() <= buffer.len() {
            let info =
                &*(buffer.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            if info.Size == 0 {
                break;
            }

            let relationship = info.Relationship;
            if relationship == RelationProcessorCore {
                physical_cores = physical_cores.saturating_add(1);
                let processor = info.u.Processor();
                let group_masks = std::slice::from_raw_parts(
                    (*processor).GroupMask.as_ptr(),
                    (*processor).GroupCount as usize,
                );
                let efficiency_class = (*processor).EfficiencyClass;
                let thread_count = group_masks
                    .iter()
                    .map(|group| group.Mask.count_ones())
                    .sum::<u32>();
                logical_threads = logical_threads.saturating_add(thread_count.max(1));
                smt_enabled |= ((*processor).Flags & LTP_PC_SMT) != 0;
                for group in group_masks {
                    push_efficiency_class_bits(
                        &mut sample.logical_efficiency_classes,
                        group.Group as usize,
                        group.Mask,
                        efficiency_class,
                    );
                }
            } else if relationship == RelationCache {
                let cache = info.u.Cache();
                let cache_size = (*cache).CacheSize as u64;
                match (*cache).Level {
                    1 => {
                        sample.l1_cache_bytes = Some(
                            sample
                                .l1_cache_bytes
                                .unwrap_or(0)
                                .saturating_add(cache_size),
                        );
                    }
                    2 => {
                        sample.l2_cache_bytes = Some(
                            sample
                                .l2_cache_bytes
                                .unwrap_or(0)
                                .saturating_add(cache_size),
                        );
                    }
                    3 => {
                        sample.l3_cache_bytes = Some(
                            sample
                                .l3_cache_bytes
                                .unwrap_or(0)
                                .saturating_add(cache_size),
                        );
                    }
                    _ => {}
                }
            }

            offset = offset.saturating_add(info.Size as usize);
        }

        sample.physical_cores = (physical_cores > 0).then_some(physical_cores);
        sample.logical_threads = (logical_threads > 0).then_some(logical_threads);
        sample.smt_enabled = sample
            .physical_cores
            .zip(sample.logical_threads)
            .map(|(cores, threads)| smt_enabled || threads > cores);
        sample
    }
}

fn push_efficiency_class_bits(
    classes: &mut Vec<Option<u8>>,
    group_index: usize,
    mask: usize,
    efficiency_class: u8,
) {
    for bit in 0..usize::BITS as usize {
        if (mask & (1usize << bit)) == 0 {
            continue;
        }
        let index = group_index
            .saturating_mul(usize::BITS as usize)
            .saturating_add(bit);
        if classes.len() <= index {
            classes.resize(index + 1, None);
        }
        classes[index] = Some(efficiency_class);
    }
}

fn collect_logical_processor_usage(
    system: &System,
    efficiency_classes: &[Option<u8>],
) -> Vec<CpuLogicalProcessorSample> {
    system
        .cpus()
        .iter()
        .enumerate()
        .map(|(index, cpu)| CpuLogicalProcessorSample {
            usage_percent: two_digit_cpu_percent(cpu.cpu_usage()),
            kind: cpu_core_kind(index, efficiency_classes),
        })
        .collect()
}

fn average_cpu_usage_percent(system: &System) -> Option<u8> {
    let cpus = system.cpus();
    if cpus.is_empty() {
        return None;
    }
    let total = cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>();
    Some(cpu_percent_u8(total / cpus.len() as f32))
}

fn average_current_frequency_mhz(
    frequencies: &[(usize, u64)],
    include: impl Fn(usize, u64) -> bool,
) -> Option<u64> {
    let mut total = 0u64;
    let mut count = 0u64;
    for &(index, frequency) in frequencies {
        if frequency == 0 || !include(index, frequency) {
            continue;
        }
        total = total.saturating_add(frequency);
        count = count.saturating_add(1);
    }
    if count > 0 { Some(total / count) } else { None }
}

fn cpu_core_kind(index: usize, efficiency_classes: &[Option<u8>]) -> Option<CpuCoreKind> {
    let Some(Some(efficiency_class)) = efficiency_classes.get(index) else {
        return None;
    };
    let present_classes = efficiency_classes
        .iter()
        .filter_map(|value| *value)
        .collect::<Vec<_>>();
    let min_class = present_classes.iter().min()?;
    let max_class = present_classes.iter().max()?;
    if min_class == max_class {
        None
    } else if efficiency_class == max_class {
        Some(CpuCoreKind::Performance)
    } else {
        Some(CpuCoreKind::Efficiency)
    }
}

fn two_digit_cpu_percent(value: f32) -> u8 {
    cpu_percent_u8(value).min(99)
}

fn cpu_percent_u8(value: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().clamp(0.0, 100.0) as u8
    }
}

fn collect_cpu_name_from_registry() -> Option<String> {
    read_registry_string(
        "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",
        "ProcessorNameString",
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn collect_cpu_frequency_mhz() -> Option<u64> {
    read_registry_dword("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0", "~MHz").map(u64::from)
}

fn read_registry_string(sub_key: &str, value_name: &str) -> Option<String> {
    unsafe {
        let sub_key_wide = to_wide(sub_key);
        let value_name_wide = to_wide(value_name);
        let mut size = 0u32;
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub_key_wide.as_ptr(),
            value_name_wide.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        );
        if status != ERROR_SUCCESS as i32 || size < 2 {
            return None;
        }

        let mut buffer = vec![0u16; (size as usize + 1) / 2];
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub_key_wide.as_ptr(),
            value_name_wide.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr() as *mut _,
            &mut size,
        );
        if status != ERROR_SUCCESS as i32 {
            return None;
        }

        Some(wide_slice_to_string(&buffer))
    }
}

fn read_registry_dword(sub_key: &str, value_name: &str) -> Option<u32> {
    unsafe {
        let sub_key_wide = to_wide(sub_key);
        let value_name_wide = to_wide(value_name);
        let mut data = 0u32;
        let mut size = size_of::<u32>() as u32;
        let status = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub_key_wide.as_ptr(),
            value_name_wide.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        );
        (status == ERROR_SUCCESS as i32).then_some(data)
    }
}

fn format_cpu_topology(
    physical_cores: Option<u32>,
    logical_threads: Option<u32>,
    smt_enabled: Option<bool>,
) -> Option<String> {
    match (physical_cores, logical_threads) {
        (Some(physical_cores), Some(logical_threads)) => {
            let smt = match smt_enabled {
                Some(true) => "HT on",
                Some(false) => "HT off",
                None => "HT --",
            };
            Some(format!("{physical_cores}C / {logical_threads}T ({smt})"))
        }
        _ => None,
    }
}

fn format_cpu_caches(
    l1_cache_bytes: Option<u64>,
    l2_cache_bytes: Option<u64>,
    l3_cache_bytes: Option<u64>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(l1_cache_bytes) = l1_cache_bytes {
        parts.push(format!("L1 {}", fmt_bytes(l1_cache_bytes)));
    }
    if let Some(l2_cache_bytes) = l2_cache_bytes {
        parts.push(format!("L2 {}", fmt_bytes(l2_cache_bytes)));
    }
    if let Some(l3_cache_bytes) = l3_cache_bytes {
        parts.push(format!("L3 {}", fmt_bytes(l3_cache_bytes)));
    }

    (!parts.is_empty()).then_some(parts.join("  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_current_frequency_mhz_uses_matching_processors_only() {
        let frequencies = [(0, 4_100), (1, 4_300), (2, 0), (3, 2_200)];

        assert_eq!(
            average_current_frequency_mhz(&frequencies, |index, _| index < 2),
            Some(4_200)
        );
        assert_eq!(
            average_current_frequency_mhz(&frequencies, |index, _| index >= 2),
            Some(2_200)
        );
        assert_eq!(
            average_current_frequency_mhz(&frequencies, |index, _| index > 3),
            None
        );
    }
}
