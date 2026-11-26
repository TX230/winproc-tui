//! Local display and wire formats shared by recording, CSV, and MCP.
use jiff::{Timestamp, fmt::strtime, tz::TimeZone};

pub(crate) trait TimestampExt {
    fn format_local(self, format: &str) -> strtime::Display<'_>;
    fn local_rfc3339(self) -> String;
    fn utc_rfc3339(self) -> String;
    fn epoch_millis(self) -> i64;
}

impl TimestampExt for Timestamp {
    fn format_local(self, format: &str) -> strtime::Display<'_> {
        self.to_zoned_datetime(TimeZone::system()).strftime(format)
    }

    fn local_rfc3339(self) -> String {
        self.format_local(wire_format(self)).to_string()
    }

    fn utc_rfc3339(self) -> String {
        self.strftime(wire_format(self)).to_string()
    }

    fn epoch_millis(self) -> i64 {
        // Schema v3 rounds down, including fractional milliseconds before the Unix epoch.
        self.as_nanosecond().div_euclid(1_000_000) as i64
    }
}

// Existing wire timestamps use groups of three fractional digits and an explicit UTC offset.
fn wire_format(timestamp: Timestamp) -> &'static str {
    let nanos = timestamp.subsec_nanosecond();
    if nanos == 0 {
        "%Y-%m-%dT%H:%M:%S%:z"
    } else if nanos % 1_000_000 == 0 {
        "%Y-%m-%dT%H:%M:%S%.3f%:z"
    } else if nanos % 1_000 == 0 {
        "%Y-%m-%dT%H:%M:%S%.6f%:z"
    } else {
        "%Y-%m-%dT%H:%M:%S%.9f%:z"
    }
}

#[cfg(test)]
pub(crate) fn local_test_time(
    year: i16,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
) -> Timestamp {
    let civil = jiff::civil::DateTime::new(year, month, day, hour, minute, second, 0).unwrap();
    TimeZone::system()
        .to_zoned_datetime(civil)
        .unwrap()
        .timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_wire_times_keep_explicit_offsets_and_fractional_digit_groups() {
        for (nanos, fraction) in [
            (0, ""),
            (100_000_000, ".100"),
            (100_100_000, ".100100"),
            (10, ".000000010"),
        ] {
            let timestamp = Timestamp::new(0, nanos).unwrap();
            assert_eq!(
                timestamp.utc_rfc3339(),
                format!("1970-01-01T00:00:00{fraction}+00:00")
            );
            assert_eq!(
                timestamp.local_rfc3339().parse::<Timestamp>().unwrap(),
                timestamp
            );
        }
    }

    #[test]
    fn recording_milliseconds_floor_negative_instants() {
        assert_eq!(Timestamp::new(0, -1).unwrap().epoch_millis(), -1);
        assert_eq!(Timestamp::new(0, 999_999).unwrap().epoch_millis(), 0);
    }
}
