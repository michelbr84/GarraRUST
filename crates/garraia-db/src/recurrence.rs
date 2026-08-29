//! Cron recurrence for scheduled tasks.
//!
//! Pure helpers around [`croner`] + [`chrono_tz`]: parsing, validation and
//! next-occurrence search. Kept free of DB access so the tricky parts
//! (timezone handling, DST boundaries, catch-up after downtime) are unit
//! testable without a store.

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use garraia_common::{Error, Result};
use std::str::FromStr;

/// Default timezone for recurring tasks. Matches the project's narrative
/// timezone convention (America/New_York), so "every day at 08:00" means
/// 08:00 where the operator lives, not 08:00 UTC.
pub const DEFAULT_TIMEZONE: &str = "America/New_York";

/// Parse a timezone name, falling back to the project default.
pub fn parse_timezone(name: Option<&str>) -> Result<Tz> {
    let raw = name.unwrap_or(DEFAULT_TIMEZONE);
    raw.parse::<Tz>()
        .map_err(|_| Error::Database(format!("unknown timezone '{raw}'")))
}

/// Parse and validate a cron expression (5 or 6 fields; seconds optional).
pub fn parse_cron(expr: &str) -> Result<Cron> {
    Cron::from_str(expr)
        .map_err(|e| Error::Database(format!("invalid cron expression '{expr}': {e}")))
}

/// First occurrence strictly after `after`, in `tz`, as UTC.
pub fn next_occurrence(expr: &str, tz: Tz, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let cron = parse_cron(expr)?;
    let local = after.with_timezone(&tz);
    let next = cron
        .find_next_occurrence(&local, false)
        .map_err(|e| Error::Database(format!("no future occurrence for '{expr}': {e}")))?;
    Ok(next.with_timezone(&Utc))
}

/// Next run after a fire, skipping every occurrence already in the past.
///
/// Catch-up policy: a gateway that was down for a week must run the task
/// **once** on return and then resume the normal cadence — replaying every
/// missed occurrence would flood the user's channel. `now` is passed in so
/// the decision is testable.
pub fn next_after_run(
    expr: &str,
    tz: Tz,
    now: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let anchor = if scheduled_for > now {
        scheduled_for
    } else {
        now
    };
    next_occurrence(expr, tz, anchor)
}

/// Backoff for a failed attempt: 1min, 4min, 9min… capped at 1h.
pub fn retry_delay_secs(attempts: u32) -> i64 {
    let n = attempts.max(1) as i64;
    (n * n * 60).min(3600)
}

/// Local wall-clock helper used by tests and diagnostics.
pub fn local_time(tz: Tz, at: DateTime<Utc>) -> String {
    tz.from_utc_datetime(&at.naive_utc())
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    #[test]
    fn rejects_invalid_cron_and_timezone() {
        assert!(parse_cron("not a cron").is_err());
        assert!(parse_cron("*/5 * * * *").is_ok());
        assert!(parse_timezone(Some("Mars/Olympus")).is_err());
        assert!(parse_timezone(None).is_ok(), "default must parse");
    }

    #[test]
    fn computes_next_daily_occurrence_in_local_time() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();
        // 2026-06-15 is EDT (UTC-4), so 08:00 local == 12:00 UTC.
        let now = utc(2026, 6, 15, 6, 0);
        let next = next_occurrence("0 8 * * *", tz, now).unwrap();
        assert_eq!(next, utc(2026, 6, 15, 12, 0));
    }

    /// The DST boundary is where a naive UTC implementation drifts by an
    /// hour: 08:00 local is 13:00 UTC in winter and 12:00 UTC in summer.
    #[test]
    fn daily_occurrence_follows_dst_shift() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();

        // Winter (EST, UTC-5): 08:00 local == 13:00 UTC.
        let winter = next_occurrence("0 8 * * *", tz, utc(2026, 1, 15, 6, 0)).unwrap();
        assert_eq!(winter, utc(2026, 1, 15, 13, 0));

        // US DST starts 2026-03-08 at 02:00 local (07:00 UTC). At 09:00 UTC
        // that day the clock is already EDT, so 08:00 local is 12:00 UTC —
        // an hour earlier than the same expression in winter. A naive
        // UTC-only implementation would still say 13:00 here.
        let after_switch = next_occurrence("0 8 * * *", tz, utc(2026, 3, 8, 9, 0)).unwrap();
        assert_eq!(after_switch, utc(2026, 3, 8, 12, 0));
        assert_eq!(local_time(tz, after_switch), "2026-03-08 08:00:00 EDT");
        assert_eq!(local_time(tz, winter), "2026-01-15 08:00:00 EST");
    }

    #[test]
    fn catch_up_runs_once_and_skips_missed_occurrences() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        // Hourly task whose last scheduled fire was 5 days ago (downtime).
        let scheduled_for = utc(2026, 6, 1, 0, 0);
        let now = utc(2026, 6, 6, 10, 30);
        let next = next_after_run("0 * * * *", tz, now, scheduled_for).unwrap();
        assert_eq!(
            next,
            utc(2026, 6, 6, 11, 0),
            "must resume from now, not replay 5 days of hourly fires"
        );
    }

    #[test]
    fn normal_cadence_advances_one_step() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let scheduled_for = utc(2026, 6, 6, 10, 0);
        // Fired on time.
        let next = next_after_run("0 * * * *", tz, utc(2026, 6, 6, 10, 0), scheduled_for).unwrap();
        assert_eq!(next, utc(2026, 6, 6, 11, 0));
    }

    #[test]
    fn retry_backoff_grows_and_caps() {
        assert_eq!(retry_delay_secs(1), 60);
        assert_eq!(retry_delay_secs(2), 240);
        assert_eq!(retry_delay_secs(3), 540);
        assert_eq!(retry_delay_secs(100), 3600, "must saturate at one hour");
    }
}
