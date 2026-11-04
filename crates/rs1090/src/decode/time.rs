/**
 * Timestamps are encoded in various formats that may require some conversions.
 *
 * - Unix timestamps (from epoch), may be expressed in s, ms, us, ns
 *   This is provided by std::time::SystemTime
 * - nanoseconds since midnight UTC (Beast format)
 * - nanoseconds since GPT time of week
 */
use std::time::{SystemTime, UNIX_EPOCH};

static GPS_TO_UNIX_OFFSET: u64 = 315964800; // GPS epoch to Unix epoch in seconds

static LEAP_SECONDS_SINCE_2017: u64 = 18;

pub fn now_in_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before unix epoch")
        .as_nanos()
}

pub fn now_in_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before unix epoch")
        .as_secs()
}

pub fn today_in_s(now_s: u128) -> u128 {
    86_400 * (now_s / 86_400)
}

pub fn gps_week_in_s(now_s: u64) -> u64 {
    (86_400 * 7)
        * ((now_s - GPS_TO_UNIX_OFFSET + LEAP_SECONDS_SINCE_2017) / 86_400 / 7)
        + GPS_TO_UNIX_OFFSET
        - LEAP_SECONDS_SINCE_2017
}

pub fn since_today_to_nanos(nanos: u128) -> u128 {
    today_in_s(now_in_ns() / 1_000_000_000) * 1_000_000_000 + nanos
}

pub fn since_gps_week_to_since_today(gps_ns: u64) -> u64 {
    (gps_ns - LEAP_SECONDS_SINCE_2017 * 1_000_000_000) % 86_400_000_000_000
}

pub fn since_gps_week_to_unix_s(gps_ns: u64) -> f64 {
    gps_week_in_s(now_in_s()) as f64 + (gps_ns as f64 * 1e-9)
}

/// Resolve declared GPS time-of-week to the nearest week around an observation.
/// This is epoch resolution for a known GPS clock, not validation of its quality.
#[cfg(any(feature = "sero", test))]
pub(crate) fn checked_gps_week_to_unix_ns(
    gps_ns: u64,
    observation_ns: u128,
) -> Option<u128> {
    const NS_PER_SECOND: i128 = 1_000_000_000;
    const NS_PER_WEEK: i128 = 604_800 * NS_PER_SECOND;
    if i128::from(gps_ns) >= NS_PER_WEEK {
        return None;
    }
    let observation = i128::try_from(observation_ns).ok()?;
    let epoch = i128::from(GPS_TO_UNIX_OFFSET - LEAP_SECONDS_SINCE_2017)
        * NS_PER_SECOND;
    if observation < epoch {
        return None;
    }
    let week_start =
        epoch + ((observation - epoch) / NS_PER_WEEK) * NS_PER_WEEK;
    [-NS_PER_WEEK, 0, NS_PER_WEEK]
        .into_iter()
        .filter_map(|offset| {
            week_start
                .checked_add(offset)?
                .checked_add(i128::from(gps_ns))
        })
        .filter(|candidate| *candidate >= epoch)
        .min_by_key(|candidate| candidate.abs_diff(observation))
        .and_then(|candidate| u128::try_from(candidate).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: u128 = 1_000_000_000;
    const WEEK: u128 = 604_800 * NS;

    fn week_start() -> u128 {
        u128::from(gps_week_in_s(1_792_944_000)) * NS
    }

    #[test]
    fn gps_week_conversion_preserves_subsecond_time() {
        let start = week_start();
        let offset = 12_345_678_901;
        assert_eq!(
            checked_gps_week_to_unix_ns(offset, start + 20 * NS),
            Some(start + u128::from(offset))
        );
    }

    #[test]
    fn gps_week_conversion_handles_rollover_and_leap_offset() {
        let start = week_start();
        assert_eq!(checked_gps_week_to_unix_ns(0, start + NS), Some(start));
        assert_eq!(start % (86_400 * NS), 86_382 * NS);
        assert_eq!(
            checked_gps_week_to_unix_ns(18_000_000_000, start + 19 * NS),
            Some(start + 18 * NS)
        );
        assert_eq!(
            checked_gps_week_to_unix_ns((WEEK - NS) as u64, start + NS),
            Some(start - NS)
        );
        assert_eq!(
            checked_gps_week_to_unix_ns(0, start + WEEK - NS),
            Some(start + WEEK)
        );
    }

    #[test]
    fn gps_week_conversion_rejects_unrepresentable_values() {
        assert!(
            checked_gps_week_to_unix_ns(WEEK as u64, week_start()).is_none()
        );
        assert!(checked_gps_week_to_unix_ns(u64::MAX, week_start()).is_none());
        assert!(checked_gps_week_to_unix_ns(0, u128::MAX).is_none());
        assert!(checked_gps_week_to_unix_ns(0, 0).is_none());
    }
}
