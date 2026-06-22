use std::{collections::VecDeque, mem::zeroed, ptr::null_mut};

use anyhow::Result;
use winapi::{
    shared::winerror::ERROR_SUCCESS,
    um::pdh::{
        PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE,
        PDH_FMT_NOCAP100, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
        PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    },
};

use crate::platform::to_wide;

const PDH_MORE_DATA_STATUS: u32 = 0x8000_07D2;

pub(crate) fn map_process_counter_instances_to_pids<T: Copy>(
    process_ids: Vec<(String, u64)>,
    counter_values: Vec<(String, T)>,
) -> std::collections::HashMap<u32, T> {
    let mut values = std::collections::HashMap::new();
    let mut counters_by_instance = std::collections::HashMap::<String, VecDeque<T>>::new();

    for (instance_name, counter_value) in counter_values {
        counters_by_instance
            .entry(instance_name)
            .or_default()
            .push_back(counter_value);
    }

    for (instance_name, pid_value) in process_ids {
        if instance_name == "_Total" || pid_value == 0 || pid_value > u32::MAX as u64 {
            continue;
        }

        let Some(counter_values) = counters_by_instance.get_mut(&instance_name) else {
            continue;
        };

        let Some(counter_value) = counter_values.pop_front() else {
            continue;
        };

        values.insert(pid_value as u32, counter_value);
    }

    values
}

pub(crate) fn read_named_counter_items(counter: PDH_HCOUNTER) -> Option<Vec<(String, u64)>> {
    unsafe {
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
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            if item.szName.is_null() {
                continue;
            }

            let c_status = item.FmtValue.CStatus as i32;
            if c_status != ERROR_SUCCESS as i32 {
                continue;
            }

            let value = *item.FmtValue.u.largeValue();
            if value < 0 {
                continue;
            }

            let name = wide_ptr_to_string(item.szName);
            values.push((name, value as u64));
        }

        Some(values)
    }
}

pub(crate) fn read_named_counter_double_items(counter: PDH_HCOUNTER) -> Option<Vec<(String, f64)>> {
    unsafe {
        let format = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100;
        let mut buffer_size = 0u32;
        let mut item_count = 0u32;
        let first_status = PdhGetFormattedCounterArrayW(
            counter,
            format,
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
            format,
            &mut buffer_size,
            &mut item_count,
            item_ptr,
        )) {
            return None;
        }

        let items = std::slice::from_raw_parts(item_ptr, item_count as usize);
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            if item.szName.is_null() {
                continue;
            }

            let c_status = item.FmtValue.CStatus as i32;
            if c_status != ERROR_SUCCESS as i32 {
                continue;
            }

            let value = *item.FmtValue.u.doubleValue();
            if !value.is_finite() || value < 0.0 {
                continue;
            }

            let name = wide_ptr_to_string(item.szName);
            values.push((name, value));
        }

        Some(values)
    }
}

pub(crate) fn read_pdh_large_value(counter: PDH_HCOUNTER) -> Result<u64> {
    unsafe {
        let mut counter_type = 0u32;
        let mut value: PDH_FMT_COUNTERVALUE = zeroed();
        ensure_pdh_success(
            PdhGetFormattedCounterValue(counter, PDH_FMT_LARGE, &mut counter_type, &mut value),
            "reading formatted counter value",
        )?;
        let c_status = value.CStatus as i32;
        if c_status != ERROR_SUCCESS as i32 {
            anyhow::bail!("counter status {c_status:#x}");
        }

        let value = *value.u.largeValue();
        if value < 0 {
            anyhow::bail!("counter returned negative value {value}");
        }
        Ok(value as u64)
    }
}

pub(crate) fn read_pdh_double_value(counter: PDH_HCOUNTER) -> Result<f64> {
    unsafe {
        let mut counter_type = 0u32;
        let mut value: PDH_FMT_COUNTERVALUE = zeroed();
        ensure_pdh_success(
            PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, &mut counter_type, &mut value),
            "reading formatted counter value",
        )?;
        let c_status = value.CStatus as i32;
        if c_status != ERROR_SUCCESS as i32 {
            anyhow::bail!("counter status {c_status:#x}");
        }

        let value = *value.u.doubleValue();
        if !value.is_finite() || value < 0.0 {
            anyhow::bail!("counter returned invalid value {value}");
        }
        Ok(value)
    }
}

pub(crate) fn read_optional_pdh_large_value(counter: Option<PDH_HCOUNTER>) -> Option<u64> {
    counter.and_then(|counter| read_pdh_large_value(counter).ok())
}

pub(crate) fn read_optional_pdh_double_value(counter: Option<PDH_HCOUNTER>) -> Option<f64> {
    counter.and_then(|counter| read_pdh_double_value(counter).ok())
}

pub(crate) fn read_optional_named_counter_values(counter: Option<PDH_HCOUNTER>) -> Option<u64> {
    counter.and_then(|counter| {
        let items = read_named_counter_items(counter)?;
        let mut found = false;
        let mut total = 0u64;
        for (name, value) in items {
            if name == "_Total" {
                continue;
            }
            total = total.saturating_add(value);
            found = true;
        }
        found.then_some(total)
    })
}

pub(crate) fn add_optional_pdh_counter(
    query: PDH_HQUERY,
    counter_path: &str,
) -> Option<PDH_HCOUNTER> {
    unsafe {
        let mut counter: PDH_HCOUNTER = null_mut();
        let wide_path = to_wide(counter_path);
        pdh_ok(PdhAddEnglishCounterW(
            query,
            wide_path.as_ptr(),
            0,
            &mut counter,
        ))
        .then_some(counter)
    }
}

pub(crate) fn sum_optional_values(values: impl IntoIterator<Item = Option<u64>>) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for value in values.into_iter().flatten() {
        total = total.saturating_add(value);
        found = true;
    }
    found.then_some(total)
}

pub(crate) fn normalize_process_cpu_percent(
    value: f64,
    logical_processor_count: usize,
) -> Option<f64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let normalized = value / logical_processor_count.max(1) as f64;
    Some(normalized.clamp(0.0, 100.0))
}

pub(crate) fn ensure_pdh_success(status: i32, context: &str) -> Result<()> {
    if pdh_ok(status) {
        Ok(())
    } else {
        anyhow::bail!("{context} failed with {status:#x}");
    }
}

pub(crate) fn pdh_ok(status: i32) -> bool {
    status == ERROR_SUCCESS as i32
}

fn wide_ptr_to_string(ptr: *mut u16) -> String {
    unsafe {
        let mut len = 0usize;
        while !ptr.is_null() && *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}
