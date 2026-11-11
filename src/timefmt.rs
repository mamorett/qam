use std::time::Duration;

/// Returns `true` if `d` is zero (used for `#[serde(skip_serializing_if = "crate::timefmt::is_zero")]`).
pub fn is_zero(d: &Duration) -> bool {
    d.is_zero()
}

/// Parses a Go-style duration string (e.g., `"0"`, `"5s"`, `"1m30s"`, `"500ms"`, `"1.5s"`, `"2h45m"`).
/// Returns an error if the string is invalid or represents a negative duration.
pub fn parse_go_duration(input: &str) -> Result<Duration, String> {
    let nanos = parse_go_duration_signed_nanos(input)?;
    if nanos < 0 {
        return Err(format!("negative duration not allowed: {input}"));
    }
    let nanos_u128 = nanos as u128;
    let secs = u64::try_from(nanos_u128 / 1_000_000_000)
        .map_err(|_| format!("duration overflow: {input}"))?;
    let subsec_nanos = (nanos_u128 % 1_000_000_000) as u32;
    Ok(Duration::new(secs, subsec_nanos))
}

/// Parses a Go-style duration string into signed nanoseconds (compatible with `time.ParseDuration`).
pub fn parse_go_duration_signed_nanos(input: &str) -> Result<i128, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("time: invalid duration \"\"".to_string());
    }
    let (neg, mut rest) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (false, r)
    } else {
        (false, s)
    };

    if rest == "0" {
        return Ok(0);
    }
    if rest.is_empty() {
        return Err(format!("time: invalid duration {input:?}"));
    }

    let mut total_nanos: i128 = 0;
    while !rest.is_empty() {
        // Parse integer and optional fractional part: [0-9]*(\.[0-9]*)?
        let int_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let int_str = &rest[..int_end];
        rest = &rest[int_end..];

        let mut frac_str = "";
        if let Some(after_dot) = rest.strip_prefix('.') {
            let frac_end = after_dot
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_dot.len());
            frac_str = &after_dot[..frac_end];
            rest = &after_dot[frac_end..];
        }

        if int_str.is_empty() && frac_str.is_empty() {
            return Err(format!("time: invalid duration {input:?}"));
        }

        let int_val: i128 = if int_str.is_empty() {
            0
        } else {
            int_str
                .parse::<i128>()
                .map_err(|_| format!("time: invalid duration {input:?}"))?
        };

        // Parse unit
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let unit_str = &rest[..unit_end];
        rest = &rest[unit_end..];

        let unit_nanos: i128 = match unit_str {
            "ns" => 1,
            "us" | "µs" | "μs" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60 * 1_000_000_000,
            "h" => 3600 * 1_000_000_000,
            "" => return Err(format!("time: missing unit in duration {input:?}")),
            other => {
                return Err(format!(
                    "time: unknown unit {other:?} in duration {input:?}"
                ));
            }
        };

        let mut part_nanos = int_val
            .checked_mul(unit_nanos)
            .ok_or_else(|| format!("time: invalid duration {input:?}"))?;

        if !frac_str.is_empty() {
            // Compute fractional contribution up to 9 digits
            let mut scale: i128 = 1;
            let mut frac_val: i128 = 0;
            for ch in frac_str.chars().take(9) {
                frac_val = frac_val * 10 + (ch as i128 - '0' as i128);
                scale *= 10;
            }
            part_nanos = part_nanos
                .checked_add((frac_val * unit_nanos) / scale)
                .ok_or_else(|| format!("time: invalid duration {input:?}"))?;
        }

        total_nanos = total_nanos
            .checked_add(part_nanos)
            .ok_or_else(|| format!("time: invalid duration {input:?}"))?;
    }

    Ok(if neg { -total_nanos } else { total_nanos })
}

/// Formats a `Duration` using Go's `time.Duration.String()` representation.
pub fn format_go_duration(d: Duration) -> String {
    let total_nanos = d.as_nanos();
    if total_nanos == 0 {
        return "0s".to_string();
    }
    if total_nanos < 1_000 {
        return format!("{total_nanos}ns");
    }
    if total_nanos < 1_000_000 {
        return format_subunit(total_nanos, 1_000, "µs");
    }
    if total_nanos < 1_000_000_000 {
        return format_subunit(total_nanos, 1_000_000, "ms");
    }

    let total_secs = d.as_secs();
    let subsec_nanos = d.subsec_nanos();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    let mut out = String::new();
    if hours > 0 {
        out.push_str(&format!("{hours}h"));
        out.push_str(&format!("{mins}m"));
    } else if mins > 0 {
        out.push_str(&format!("{mins}m"));
    }

    if subsec_nanos == 0 {
        out.push_str(&format!("{secs}s"));
    } else {
        let mut frac = format!("{subsec_nanos:09}");
        while frac.ends_with('0') {
            frac.pop();
        }
        out.push_str(&format!("{secs}.{frac}s"));
    }
    out
}

fn format_subunit(total_nanos: u128, unit_nanos: u128, suffix: &str) -> String {
    let whole = total_nanos / unit_nanos;
    let rem = total_nanos % unit_nanos;
    if rem == 0 {
        format!("{whole}{suffix}")
    } else {
        let width = if unit_nanos == 1_000_000 { 6 } else { 3 };
        let mut frac = format!("{rem:0width$}");
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("{whole}.{frac}{suffix}")
    }
}

pub mod serde_duration {
    use super::{format_go_duration, parse_go_duration};
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::time::Duration;

    pub fn serialize<S>(d: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_go_duration(*d))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawDuration {
            Str(String),
            U64(u64),
            I64(i64),
        }

        match Option::<RawDuration>::deserialize(deserializer)? {
            None => Ok(Duration::ZERO),
            Some(RawDuration::Str(s)) => {
                if s.trim().is_empty() {
                    Ok(Duration::ZERO)
                } else {
                    parse_go_duration(&s).map_err(de::Error::custom)
                }
            }
            Some(RawDuration::U64(ns)) => Ok(Duration::from_nanos(ns)),
            Some(RawDuration::I64(ns)) => {
                if ns < 0 {
                    Err(de::Error::custom("negative duration"))
                } else {
                    Ok(Duration::from_nanos(ns as u64))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_and_format_basic_vectors() {
        assert_eq!(parse_go_duration("0").unwrap(), Duration::ZERO);
        assert_eq!(parse_go_duration("0s").unwrap(), Duration::ZERO);
        assert_eq!(parse_go_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_go_duration("1m30s").unwrap(), Duration::from_secs(90));
        assert_eq!(
            parse_go_duration("500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            parse_go_duration("1.5s").unwrap(),
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_go_duration("2h45m").unwrap(),
            Duration::from_secs(2 * 3600 + 45 * 60)
        );
        assert_eq!(
            parse_go_duration("250us").unwrap(),
            Duration::from_micros(250)
        );
        assert_eq!(
            parse_go_duration("250µs").unwrap(),
            Duration::from_micros(250)
        );
        assert_eq!(
            parse_go_duration("100ns").unwrap(),
            Duration::from_nanos(100)
        );

        assert_eq!(format_go_duration(Duration::ZERO), "0s");
        assert_eq!(format_go_duration(Duration::from_secs(5)), "5s");
        assert_eq!(format_go_duration(Duration::from_secs(60)), "1m0s");
        assert_eq!(format_go_duration(Duration::from_secs(90)), "1m30s");
        assert_eq!(format_go_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_go_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_go_duration(Duration::from_micros(250)), "250µs");
        assert_eq!(format_go_duration(Duration::from_nanos(42)), "42ns");
    }

    #[test]
    fn rejects_invalid_and_negative_durations() {
        assert!(parse_go_duration("").is_err());
        assert!(parse_go_duration("abc").is_err());
        assert!(parse_go_duration("10").is_err());
        assert!(parse_go_duration("10days").is_err());
        assert!(parse_go_duration("-5s").is_err());
        assert_eq!(
            parse_go_duration_signed_nanos("-5s").unwrap(),
            -5_000_000_000
        );
    }

    proptest! {
        #[test]
        fn roundtrip_go_duration(millis in 0u64..86_400_000u64) {
            let d = Duration::from_millis(millis);
            let formatted = format_go_duration(d);
            let parsed = parse_go_duration(&formatted).unwrap();
            prop_assert_eq!(parsed, d);
        }
    }
}
