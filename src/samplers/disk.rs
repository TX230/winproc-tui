use std::ptr::null_mut;

use winapi::um::fileapi::{GetDiskFreeSpaceExW, GetLogicalDrives};

use crate::{model::DiskUsageSample, platform::to_wide};

pub(crate) fn collect_disk_usages() -> Vec<DiskUsageSample> {
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Vec::new();
    }

    let mut disks = Vec::new();
    for index in 0..26u32 {
        if (mask & (1u32 << index)) == 0 {
            continue;
        }

        let drive_letter = (b'A' + index as u8) as char;
        let root_path = format!("{drive_letter}:\\");
        let wide_path = to_wide(&root_path);
        let mut free_bytes = 0u64;
        let mut total_bytes = 0u64;

        let status = unsafe {
            GetDiskFreeSpaceExW(
                wide_path.as_ptr(),
                null_mut(),
                &mut total_bytes as *mut u64 as *mut _,
                &mut free_bytes as *mut u64 as *mut _,
            )
        };
        if status == 0 || total_bytes == 0 {
            continue;
        }

        disks.push(DiskUsageSample {
            name: format!("{drive_letter}:"),
            free_bytes,
            total_bytes,
        });
    }

    disks.sort_by(|left, right| left.name.cmp(&right.name));
    disks
}
