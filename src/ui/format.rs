pub(crate) fn ratio_optional(value: Option<u64>, total: Option<u64>) -> Option<f64> {
    match (value, total) {
        (_, Some(0)) | (None, _) | (_, None) => None,
        (Some(value), Some(total)) => Some(value as f64 / total as f64),
    }
}

pub(crate) fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1000.0 && unit_index + 1 < UNITS.len() {
        value /= 1000.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        format!("{value:.0} {}", UNITS[unit_index])
    }
}

pub(crate) fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group_len = digits.len() % 3;

    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (index + 3 - first_group_len) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted
}

pub(crate) fn format_mb(bytes: u64) -> String {
    let megabytes = ((bytes as f64) / 1_000_000.0).round() as u64;
    format!("{} MB", format_integer(megabytes))
}

pub(crate) fn format_signed_integer(value: i128) -> String {
    let sign = if value >= 0 { "+" } else { "-" };
    let magnitude = value.unsigned_abs();
    format!("{sign}{}", format_unsigned_integer(magnitude))
}

fn format_unsigned_integer(value: u128) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group_len = digits.len() % 3;

    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (index + 3 - first_group_len) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted
}

pub(crate) fn format_frequency_mhz(frequency_mhz: Option<u64>) -> String {
    match frequency_mhz {
        Some(frequency_mhz) if frequency_mhz >= 1000 => {
            format!("{:.2} GHz", frequency_mhz as f64 / 1000.0)
        }
        Some(frequency_mhz) => format!("{frequency_mhz} MHz"),
        None => "--".to_string(),
    }
}

pub(crate) fn format_mbps(bytes_per_sec: u64) -> String {
    format!(
        "{} Mbps",
        ((bytes_per_sec as f64 * 8.0) / 1_000_000.0).round() as u64
    )
}
