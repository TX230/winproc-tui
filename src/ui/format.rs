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

pub(crate) fn format_compact_bytes(bytes: u64) -> String {
    format_compact_bytes_magnitude(u128::from(bytes), 1)
}

pub(crate) fn format_compact_bytes_with_precision(bytes: u64, precision: usize) -> String {
    format_compact_bytes_magnitude(u128::from(bytes), precision)
}

pub(crate) fn format_signed_compact_bytes(bytes: i128) -> String {
    let sign = if bytes >= 0 { "+" } else { "-" };
    format!(
        "{sign}{}",
        format_compact_bytes_magnitude(bytes.unsigned_abs(), 1)
    )
}

fn format_compact_bytes_magnitude(bytes: u128, precision: usize) -> String {
    const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1000.0 && unit_index + 1 < UNITS.len() {
        value /= 1000.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        let factor = 10_f64.powi(precision as i32);
        value = (value * factor).round() / factor;
        if value >= 1000.0 && unit_index + 1 < UNITS.len() {
            value /= 1000.0;
            unit_index += 1;
        }
        format!("{value:.precision$} {}", UNITS[unit_index])
    }
}

pub(crate) fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group_len = digits.len() % 3;

    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (index + 3 - first_group_len).is_multiple_of(3) {
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
        if index > 0 && (index + 3 - first_group_len).is_multiple_of(3) {
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

pub(crate) fn format_io_rate(bytes_per_sec: u64) -> String {
    if bytes_per_sec < 125_000 {
        let kbps = ((bytes_per_sec as f64 * 8.0) / 1_000.0).round() as u64;
        format!("{} Kbps", format_integer(kbps))
    } else {
        format_mbps(bytes_per_sec)
    }
}

pub(crate) fn format_kb_per_sec(bytes_per_sec: u64) -> String {
    let whole_kb = bytes_per_sec / 1_000 + u64::from(bytes_per_sec % 1_000 >= 500);
    format!("{} KB/s", format_integer(whole_kb))
}

pub(crate) fn format_signed_mbps(bytes_per_sec: i128) -> String {
    let mbps = ((bytes_per_sec as f64 * 8.0) / 1_000_000.0).round() as i128;
    format!("{} Mbps", format_signed_integer(mbps))
}

pub(crate) fn format_signed_io_rate(bytes_per_sec: i128) -> String {
    if bytes_per_sec.unsigned_abs() < 125_000 {
        let kbps = ((bytes_per_sec as f64 * 8.0) / 1_000.0).round() as i128;
        format!("{} Kbps", format_signed_integer(kbps))
    } else {
        format_signed_mbps(bytes_per_sec)
    }
}

pub(crate) fn format_mb_per_sec(bytes_per_sec: u64) -> String {
    format!("{:.1} MB/s", bytes_per_sec as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_bytes_uses_adaptive_decimal_units() {
        assert_eq!(format_compact_bytes(0), "0 B");
        assert_eq!(format_compact_bytes(999), "999 B");
        assert_eq!(format_compact_bytes(1_000), "1.0 KB");
        assert_eq!(format_compact_bytes(388_067_328), "388.1 MB");
        assert_eq!(format_compact_bytes(999_950_000), "1.0 GB");
        assert_eq!(format_compact_bytes(1_500_000_000), "1.5 GB");
        assert_eq!(format_compact_bytes(u64::MAX), "18.4 EB");
        assert_eq!(
            format_compact_bytes_with_precision(2_860_000_000, 2),
            "2.86 GB"
        );
    }

    #[test]
    fn signed_compact_values_keep_sign_for_decreases_and_zero() {
        assert_eq!(format_signed_compact_bytes(-12_300_000), "-12.3 MB");
        assert_eq!(format_signed_compact_bytes(0), "+0 B");
        assert_eq!(format_signed_mbps(-1_000_000), "-8 Mbps");
        assert_eq!(format_signed_mbps(0), "+0 Mbps");
    }

    #[test]
    fn io_rates_use_kbps_below_one_mbps() {
        assert_eq!(format_io_rate(62_500), "500 Kbps");
        assert_eq!(format_io_rate(124_999), "1,000 Kbps");
        assert_eq!(format_io_rate(125_000), "1 Mbps");
        assert_eq!(format_signed_io_rate(-62_500), "-500 Kbps");
        assert_eq!(format_signed_io_rate(0), "+0 Kbps");
        assert_eq!(format_signed_io_rate(125_000), "+1 Mbps");
    }

    #[test]
    fn byte_rates_use_whole_decimal_kilobytes_per_second() {
        assert_eq!(format_kb_per_sec(499), "0 KB/s");
        assert_eq!(format_kb_per_sec(500), "1 KB/s");
        assert_eq!(format_kb_per_sec(12_345_678), "12,346 KB/s");
    }
}

/// Fit a summary row without ever exposing only a prefix of a measurement.
/// The original row is retained at wider widths; an ellipsis requests more room.
pub(crate) fn bounded_summary_line(
    mut line: ratatui::text::Line<'static>,
    width: u16,
) -> ratatui::text::Line<'static> {
    use ratatui::text::Span;
    let width = usize::from(width);
    if line.width() <= width {
        return line;
    }
    if line.spans.len() == 1 {
        if line.spans[0].content.starts_with("[Per-core") {
            line.spans[0].content = "[Per-core]".into();
        }
        if line.width() > width {
            line.spans = vec![Span::raw(if width > 0 { "…" } else { "" })];
        }
        return line;
    }
    line.spans.retain(|span| !span.content.trim().is_empty());
    if line.spans.len() < 2 {
        return line;
    }
    let label = line.spans[0].content.trim();
    let label = match label {
        "Free + Zeroed" => "Free+0",
        "Commit charge" => "Commit",
        "Dedicated" => "Ded",
        "Encode" => "Enc",
        "Decode" => "Dec",
        "Net Rx" => "Rx",
        "Net Tx" => "Tx",
        "Disk R" => "R",
        "Disk W" => "W",
        "Disk Q" => "Queue",
        "Freq(P/E)" => "P/E",
        "Processes" => "Procs",
        "Nonpaged Pool" => "Nonpaged",
        "Paged Pool" => "Paged",
        other => other,
    }
    .to_string();
    line.spans[0].content = format!("{label} ").into();
    // Compact each complete numeric token and its unit, preserving semantic styles.
    for span in line.spans.iter_mut().skip(1) {
        let supplementary = span.content.starts_with(" (") || span.content.starts_with(" max");
        span.content = format!(
            "{}{}",
            if supplementary { " " } else { "" },
            compact_summary_value(&span.content)
        )
        .into();
    }
    if line.width() <= width {
        return line;
    }
    // Drop supplementary spans (codec max/engine count, CPU U/K), explicitly.
    while line.spans.len() > 2 && line.width() + 1 > width {
        line.spans.pop();
    }
    let budget = width.saturating_sub(line.spans[0].width() + 1);
    if let Some(value) = line.spans.get_mut(1) {
        let primary = value
            .content
            .split_once('/')
            .filter(|(_, rest)| rest.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-'))
            .map(|(first, _)| first)
            .unwrap_or(&value.content)
            .trim();
        if value.width() > budget {
            value.content = if ratatui::text::Span::raw(primary).width() <= budget {
                primary.to_string()
            } else {
                String::new()
            }
            .into();
        }
    }
    line.spans.push(Span::raw("…"));
    if line.width() > width {
        line.spans = vec![Span::raw(if width > 0 { "…" } else { "" })];
    }
    line
}

fn compact_summary_value(value: &str) -> String {
    let tokens: Vec<_> = value.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].replace(',', "");
        if let Ok(mut number) = token.parse::<f64>() {
            let unit = tokens.get(i + 1).copied().unwrap_or("");
            let units: &[&str] = match unit {
                "MB" => &["MB", "GB", "TB", "PB", "EB"],
                "MB/s" => &["MB/s", "GB/s", "TB/s", "PB/s", "EB/s"],
                "Mbps" => &["Mbps", "Gbps", "Tbps", "Pbps", "Ebps"],
                "MHz" => &["MHz", "GHz", "THz"],
                _ => &["", "k", "M", "G", "T", "P", "E"],
            };
            let mut index = 0;
            while number >= 1000.0 && index + 1 < units.len() {
                number /= 1000.0;
                index += 1;
            }
            let digits = if index == 0 {
                format!("{number:.0}")
            } else {
                format!("{number:.1}")
            };
            result.push(format!("{digits}{}", units[index]));
            if !units[0].is_empty() {
                i += 1;
            }
        } else {
            result.push(token);
        }
        i += 1;
    }
    result.join(" ").replace(" / ", "/")
}

#[cfg(test)]
mod bounded_summary_tests {
    use super::*;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        text::{Line, Span},
        widgets::Paragraph,
    };
    #[test]
    fn compact_summary_units_preserve_quantity_and_rate_meanings() {
        for (input, expected) in [
            ("123,456 MB/s", "123.5GB/s"),
            ("12,345 Mbps", "12.3Gbps"),
            ("--", "--"),
            ("12,345 MB / 64,000 MB", "12.3GB/64.0GB"),
            ("-- / 64,000 MB", "--/64.0GB"),
            ("3,850 MHz / 2,500 MHz", "3.9GHz/2.5GHz"),
            ("999 MB/s", "999MB/s"),
            ("12,345", "12.3k"),
        ] {
            assert_eq!(compact_summary_value(input), expected, "{input}");
        }
    }

    #[test]
    fn overflow_never_renders_a_partial_number_or_rate_unit() {
        for value in ["123,456 MB/s", "--", "18,446,744,073,709 MB", "123%"] {
            for width in 1..32 {
                let line = bounded_summary_line(
                    Line::from(vec![Span::raw("Disk R  "), Span::raw(value)]),
                    width,
                );
                assert!(line.width() <= usize::from(width));
                let text = line.to_string();
                assert!(!text.ends_with("MB") || !value.ends_with("MB/s"));
                let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
                terminal
                    .draw(|f| f.render_widget(Paragraph::new(line.clone()), f.area()))
                    .unwrap();
                let rendered: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                assert_eq!(rendered.trim_end(), text.trim_end());
            }
        }
    }
}
