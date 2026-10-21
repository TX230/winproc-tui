use std::{
    io,
    mem::{align_of, offset_of, size_of},
    ptr::null_mut,
};

use sysinfo::System;
use winapi::{
    shared::winerror::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
    um::{
        sysinfoapi::GetLogicalProcessorInformationEx,
        winnt::{
            CACHE_RELATIONSHIP, GROUP_AFFINITY, LTP_PC_SMT, PROCESSOR_RELATIONSHIP, RelationAll,
            RelationCache, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
        },
        winreg::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
    },
};

use crate::{
    model::{CpuCoreKind, CpuLogicalProcessorSample, CpuSummarySample},
    platform::{to_wide, wide_slice_to_string},
    ui::fmt_bytes,
};

const _: () = assert!(
    align_of::<usize>() >= align_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
    "the Windows x64 word buffer must align processor information"
);

pub(crate) fn collect_cpu_summary(
    system: &System,
    current_frequencies_mhz: &[(usize, u64)],
    total_usage_percent: Option<f64>,
    user_usage_percent: Option<f64>,
    kernel_usage_percent: Option<f64>,
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
        total_usage_percent: optional_cpu_percent_u8(total_usage_percent)
            .or_else(|| average_cpu_usage_percent(system)),
        user_usage_percent: optional_cpu_percent_u8(user_usage_percent),
        kernel_usage_percent: optional_cpu_percent_u8(kernel_usage_percent),
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

pub(super) fn affinity_core_kinds() -> Vec<Option<CpuCoreKind>> {
    let topology = collect_cpu_topology();
    (0..usize::BITS as usize)
        .map(|index| cpu_core_kind(index, &topology.logical_efficiency_classes))
        .collect()
}

fn collect_cpu_topology() -> CpuTopologySample {
    // SAFETY: the size probe supplies a valid output pointer and no data buffer. The second call
    // receives word-aligned storage at least as large as the reported byte count. Parsing below
    // checks byte ranges and decodes fields without forming typed record references.
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

        let mut buffer = vec![0usize; (buffer_size as usize).div_ceil(size_of::<usize>())];
        let buffer_capacity = buffer.len().saturating_mul(size_of::<usize>());
        let status = GetLogicalProcessorInformationEx(
            RelationAll,
            buffer.as_mut_ptr() as *mut _,
            &mut buffer_size,
        );
        let valid_bytes = buffer_size as usize;
        if status == 0 || valid_bytes > buffer_capacity {
            return CpuTopologySample::default();
        }

        // Decode only the returned bytes; a record can be smaller than the outer union.
        let bytes = std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), valid_bytes);
        parse_cpu_topology(bytes).unwrap_or_default()
    }
}

fn parse_cpu_topology(bytes: &[u8]) -> Option<CpuTopologySample> {
    const HEADER: usize = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, u);
    let mut sample = CpuTopologySample::default();
    let mut physical_cores = 0u32;
    let mut logical_threads = 0u32;
    let mut smt_enabled = false;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.checked_add(HEADER)?)?;
        let relationship = u32::from_ne_bytes(header.get(..4)?.try_into().ok()?);
        let record_size = u32::from_ne_bytes(header.get(4..8)?.try_into().ok()?) as usize;
        if record_size < HEADER {
            return None;
        }
        let record = bytes.get(offset..offset.checked_add(record_size)?)?;
        let payload = record.get(HEADER..)?;
        if relationship == RelationProcessorCore {
            let flags = *payload.get(offset_of!(PROCESSOR_RELATIONSHIP, Flags))?;
            let efficiency_class =
                *payload.get(offset_of!(PROCESSOR_RELATIONSHIP, EfficiencyClass))?;
            let count_offset = offset_of!(PROCESSOR_RELATIONSHIP, GroupCount);
            let group_count = u16::from_ne_bytes(
                payload
                    .get(count_offset..count_offset + 2)?
                    .try_into()
                    .ok()?,
            ) as usize;
            if group_count == 0 {
                return None;
            }
            let masks_offset = offset_of!(PROCESSOR_RELATIONSHIP, GroupMask);
            let masks_size = group_count.checked_mul(size_of::<GROUP_AFFINITY>())?;
            let masks = payload.get(masks_offset..masks_offset.checked_add(masks_size)?)?;
            let mut thread_count = 0u32;
            for group in masks.chunks_exact(size_of::<GROUP_AFFINITY>()) {
                let mask = usize::from_ne_bytes(group.get(..size_of::<usize>())?.try_into().ok()?);
                let group_offset = offset_of!(GROUP_AFFINITY, Group);
                let group_index =
                    u16::from_ne_bytes(group.get(group_offset..group_offset + 2)?.try_into().ok()?)
                        as usize;
                thread_count = thread_count.saturating_add(mask.count_ones());
                push_efficiency_class_bits(
                    &mut sample.logical_efficiency_classes,
                    group_index,
                    mask,
                    efficiency_class,
                );
            }
            physical_cores = physical_cores.saturating_add(1);
            logical_threads = logical_threads.saturating_add(thread_count);
            smt_enabled |= (flags & LTP_PC_SMT) != 0;
        } else if relationship == RelationCache {
            let level = *payload.get(offset_of!(CACHE_RELATIONSHIP, Level))?;
            let size_offset = offset_of!(CACHE_RELATIONSHIP, CacheSize);
            let cache_size =
                u32::from_ne_bytes(payload.get(size_offset..size_offset + 4)?.try_into().ok()?)
                    as u64;
            let cache = match level {
                1 => Some(&mut sample.l1_cache_bytes),
                2 => Some(&mut sample.l2_cache_bytes),
                3 => Some(&mut sample.l3_cache_bytes),
                _ => None,
            };
            if let Some(cache) = cache {
                *cache = Some(cache.unwrap_or(0).saturating_add(cache_size));
            }
        }
        offset += record_size;
    }
    sample.physical_cores = (physical_cores > 0).then_some(physical_cores);
    sample.logical_threads = (logical_threads > 0).then_some(logical_threads);
    sample.smt_enabled = sample
        .physical_cores
        .zip(sample.logical_threads)
        .map(|(cores, threads)| smt_enabled || threads > cores);
    Some(sample)
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
            usage_percent: cpu_percent_u8(cpu.cpu_usage()),
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
    total.checked_div(count)
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

fn cpu_percent_u8(value: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().clamp(0.0, 100.0) as u8
    }
}

fn optional_cpu_percent_u8(value: Option<f64>) -> Option<u8> {
    value.and_then(|value| {
        value
            .is_finite()
            .then(|| value.round().clamp(0.0, 100.0) as u8)
    })
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
    // SAFETY: both registry names are live NUL-terminated UTF-16 buffers. The first call probes a
    // byte size; the second receives a UTF-16 allocation large enough for that size and a live size
    // pointer, and the result is consumed only on success.
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

        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
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
    // SAFETY: both registry names are live NUL-terminated UTF-16 buffers, and the data and size
    // pointers describe exactly one initialized `u32`; the value is returned only on success.
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

    fn core_record(class: u8, mask: u64) -> Vec<u8> {
        let mut record = vec![0u8; 48];
        record[4..8].copy_from_slice(&48u32.to_ne_bytes());
        record[8] = if mask.count_ones() > 1 { LTP_PC_SMT } else { 0 };
        record[9] = class;
        record[30..32].copy_from_slice(&1u16.to_ne_bytes());
        record[32..40].copy_from_slice(&mask.to_ne_bytes());
        record
    }

    #[test]
    fn topology_accepts_short_hybrid_records_and_classifies_clocks() {
        let mut bytes = core_record(1, 3);
        bytes.extend(core_record(0, 4));
        let topology = parse_cpu_topology(&bytes).unwrap();
        assert_eq!(topology.physical_cores, Some(2));
        assert_eq!(topology.logical_threads, Some(3));
        assert_eq!(topology.smt_enabled, Some(true));
        assert_eq!(
            topology.logical_efficiency_classes,
            vec![Some(1), Some(1), Some(0)]
        );
        let frequencies = [(0, 4000), (1, 4200), (2, 1800)];
        for (kind, expected) in [
            (CpuCoreKind::Performance, 4100),
            (CpuCoreKind::Efficiency, 1800),
        ] {
            assert_eq!(
                average_current_frequency_mhz(&frequencies, |index, _| cpu_core_kind(
                    index,
                    &topology.logical_efficiency_classes
                ) == Some(kind)),
                Some(expected)
            );
        }
    }

    #[test]
    fn topology_handles_homogeneous_cores_and_processor_groups() {
        let mut bytes = core_record(0, 1);
        let mut other = core_record(0, 2);
        other[40..42].copy_from_slice(&1u16.to_ne_bytes());
        bytes.extend(other);
        let topology = parse_cpu_topology(&bytes).unwrap();
        assert_eq!(topology.logical_efficiency_classes[65], Some(0));
        assert_eq!(cpu_core_kind(0, &topology.logical_efficiency_classes), None);
        assert_eq!(topology.logical_threads, Some(2));
    }

    #[test]
    fn topology_rejects_truncation_and_invalid_lengths() {
        let bytes = core_record(1, 3);
        for length in 1..bytes.len() {
            assert!(
                parse_cpu_topology(&bytes[..length]).is_none(),
                "length {length}"
            );
        }
        for size in [0u32, 7, 32, 49, u32::MAX] {
            let mut invalid = bytes.clone();
            invalid[4..8].copy_from_slice(&size.to_ne_bytes());
            assert!(parse_cpu_topology(&invalid).is_none(), "size {size}");
        }
        for count in [0u16, 2, u16::MAX] {
            let mut invalid = bytes.clone();
            invalid[30..32].copy_from_slice(&count.to_ne_bytes());
            assert!(parse_cpu_topology(&invalid).is_none(), "count {count}");
        }
    }

    #[test]
    fn topology_preserves_caches_and_skips_unknown_records() {
        let mut bytes = core_record(0, 1);
        let mut cache = vec![0u8; 56];
        cache[..4].copy_from_slice(&RelationCache.to_ne_bytes());
        cache[4..8].copy_from_slice(&56u32.to_ne_bytes());
        cache[8] = 3;
        cache[12..16].copy_from_slice(&8192u32.to_ne_bytes());
        bytes.extend(cache);
        bytes.extend(99u32.to_ne_bytes());
        bytes.extend(8u32.to_ne_bytes());
        assert_eq!(
            parse_cpu_topology(&bytes).unwrap().l3_cache_bytes,
            Some(8192)
        );
    }

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

    #[test]
    fn logical_cpu_usage_preserves_full_utilization() {
        assert_eq!(cpu_percent_u8(100.0), 100);
        assert_eq!(cpu_percent_u8(101.0), 100);
    }
}
