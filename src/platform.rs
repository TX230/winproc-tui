use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

pub(crate) fn wide_slice_to_string(value: &[u16]) -> String {
    let len = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

pub(crate) fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
