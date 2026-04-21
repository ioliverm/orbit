//! Daily-at-17:00-Europe/Madrid scheduler (Slice 3 T29).
//!
//! ADR-017 §4 picks the simplest shape that works: on each tick, sleep
//! until the next 17:00 local. Europe/Madrid's offset is `CET (+1)` /
//! `CEST (+2)`; we approximate this with a **fixed +01:00 offset** so
//! the worker has zero dependencies on `chrono-tz`. Four hours of drift
//! per year during DST transitions is acceptable at Slice-3 scale (one
//! scheduled fetch per day; ECB publishes at ~16:00 CET regardless, so
//! the 17:00 trigger window comfortably covers both the DST edges).
//!
//! If Slice 9's ops review wants strict Europe/Madrid (including DST),
//! swap the offset for `chrono_tz::Europe::Madrid` — the rest of the
//! module is offset-agnostic.

use chrono::{DateTime, Duration, FixedOffset, NaiveTime, TimeZone, Utc};
use std::time::Duration as StdDuration;

/// Compute how long to sleep from `now` until the next 17:00-Madrid tick.
/// Exported (crate-public) for unit testing.
pub fn duration_until_next_tick(now: DateTime<Utc>) -> StdDuration {
    let next = next_tick_after(now);
    let delta = next - now;
    let secs = delta.num_seconds().max(0) as u64;
    StdDuration::from_secs(secs)
}

/// The next 17:00 local (Europe/Madrid, CET +01:00 approximation) after
/// `now`.
pub fn next_tick_after(now: DateTime<Utc>) -> DateTime<Utc> {
    let madrid = FixedOffset::east_opt(3600).expect("+01:00 is representable");
    let now_madrid = now.with_timezone(&madrid);
    let tick_time = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
    let today_tick = madrid
        .from_local_datetime(&now_madrid.date_naive().and_time(tick_time))
        .single()
        .unwrap_or_else(|| madrid.with_ymd_and_hms(2026, 1, 1, 17, 0, 0).unwrap());

    let next = if today_tick > now_madrid {
        today_tick
    } else {
        today_tick + Duration::days(1)
    };
    next.with_timezone(&Utc)
}

/// Async helper: `tokio::time::sleep` for [`duration_until_next_tick`].
pub async fn sleep_until_next_tick(now: DateTime<Utc>) {
    tokio::time::sleep(duration_until_next_tick(now)).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn morning_ticks_today() {
        // 10:00 UTC = 11:00 Madrid, so next tick is today 17:00 Madrid
        // = 16:00 UTC, 6 hours away.
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap();
        let next = next_tick_after(now);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 15, 16, 0, 0).unwrap());
    }

    #[test]
    fn evening_ticks_tomorrow() {
        // 20:00 UTC = 21:00 Madrid, past the 17:00 tick.
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 20, 0, 0).unwrap();
        let next = next_tick_after(now);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 16, 16, 0, 0).unwrap());
    }

    #[test]
    fn at_the_tick_ticks_tomorrow() {
        // Exactly 16:00 UTC = 17:00 Madrid; we want the *next* tick,
        // so tomorrow.
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 16, 0, 0).unwrap();
        let next = next_tick_after(now);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 16, 16, 0, 0).unwrap());
    }

    #[test]
    fn duration_is_non_negative() {
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap();
        let d = duration_until_next_tick(now);
        assert!(d.as_secs() > 0);
    }
}
