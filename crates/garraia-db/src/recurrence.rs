//! Cron recurrence for scheduled tasks.
//!
//! Pure helpers around [`croner`] (its `jiff` backend): parsing, validation
//! and next-occurrence search. Kept free of DB access so the tricky parts
//! (timezone handling, DST boundaries, catch-up after downtime) are unit
//! testable without a store.
//!
//! Time zones come from the IANA database **embedded** in `jiff`
//! (`tzdb-bundle-always`), never from the host's copy. That is the guarantee
//! `chrono-tz` gave before #1938: a task's schedule does not depend on the
//! host having `tzdata` (slim container, Termux) or `tzdata-legacy`
//! (`US/Eastern`), and `jiff` picks a single database per process, so a
//! name missing from the host's copy would not fall back to the bundle.
//!
//! The public boundary stays `chrono::DateTime<Utc>`: the store and its
//! callers keep their types; only this module talks to `jiff`.

use chrono::{DateTime, Utc};
use croner::Cron;
use garraia_common::{Error, Result};
use jiff::tz::{TimeZone, TimeZoneDatabase};
use jiff::{Timestamp, ZonedDateTime};
use std::str::FromStr;

/// Default timezone for recurring tasks. Matches the project's narrative
/// timezone convention (America/New_York), so "every day at 08:00" means
/// 08:00 where the operator lives, not 08:00 UTC.
pub const DEFAULT_TIMEZONE: &str = "America/New_York";

/// The time zone of a recurring task.
///
/// Built only by [`parse_timezone`], so it always carries an IANA name:
/// [`Self::name`] is what goes into `scheduled_tasks.timezone`.
#[derive(Debug, Clone)]
pub struct TaskTimezone {
    tz: TimeZone,
    name: String,
}

impl TaskTimezone {
    /// Canonical IANA name (`america/new_york` comes back as
    /// `America/New_York`), the value stored with the task.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Parse a timezone name, falling back to the project default.
///
/// The lookup ignores ASCII case. `Etc/Unknown` is refused: `jiff` maps it
/// to an unnamed zone that behaves as UTC, and storing it would hide a bad
/// input behind UTC.
pub fn parse_timezone(name: Option<&str>) -> Result<TaskTimezone> {
    let raw = name.unwrap_or(DEFAULT_TIMEZONE);
    let unknown = || Error::Database(format!("unknown timezone '{raw}'"));
    let tz = TimeZoneDatabase::bundled()
        .get(raw)
        .map_err(|_| unknown())?;
    let name = tz.iana_name().ok_or_else(unknown)?.to_owned();
    Ok(TaskTimezone { tz, name })
}

/// Parse and validate a cron expression (5 or 6 fields; seconds optional).
pub fn parse_cron(expr: &str) -> Result<Cron> {
    Cron::from_str(expr)
        .map_err(|e| Error::Database(format!("invalid cron expression '{expr}': {e}")))
}

/// First occurrence strictly after `after`, in `tz`, as UTC.
pub fn next_occurrence(
    expr: &str,
    tz: &TaskTimezone,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let cron = parse_cron(expr)?;
    let local = zoned(tz, after)?;
    let next = cron
        .find_next_occurrence(&local, false)
        .map_err(|e| Error::Database(format!("no future occurrence for '{expr}': {e}")))?;
    from_timestamp(next.timestamp())
}

/// `at` seen in `tz`.
fn zoned(tz: &TaskTimezone, at: DateTime<Utc>) -> Result<ZonedDateTime> {
    let out_of_range = || Error::Database(format!("instant out of range: {at}"));
    let nanos = i32::try_from(at.timestamp_subsec_nanos()).map_err(|_| out_of_range())?;
    let ts = Timestamp::new(at.timestamp(), nanos).map_err(|_| out_of_range())?;
    Ok(ts.to_zoned_datetime(tz.tz.clone()))
}

/// The UTC `chrono` instant for a `jiff` timestamp.
fn from_timestamp(ts: Timestamp) -> Result<DateTime<Utc>> {
    let out_of_range = || Error::Database(format!("instant out of range: {ts}"));
    let nanos = u32::try_from(ts.subsec_nanosecond()).map_err(|_| out_of_range())?;
    DateTime::from_timestamp(ts.as_second(), nanos).ok_or_else(out_of_range)
}

/// Next run after a fire, skipping every occurrence already in the past.
///
/// Catch-up policy: a gateway that was down for a week must run the task
/// **once** on return and then resume the normal cadence — replaying every
/// missed occurrence would flood the user's channel. `now` is passed in so
/// the decision is testable.
pub fn next_after_run(
    expr: &str,
    tz: &TaskTimezone,
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
pub fn local_time(tz: &TaskTimezone, at: DateTime<Utc>) -> Result<String> {
    Ok(zoned(tz, at)?.strftime("%Y-%m-%d %H:%M:%S %Z").to_string())
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
        let next = next_occurrence("0 8 * * *", &tz, now).unwrap();
        assert_eq!(next, utc(2026, 6, 15, 12, 0));
    }

    /// The DST boundary is where a naive UTC implementation drifts by an
    /// hour: 08:00 local is 13:00 UTC in winter and 12:00 UTC in summer.
    #[test]
    fn daily_occurrence_follows_dst_shift() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();

        // Winter (EST, UTC-5): 08:00 local == 13:00 UTC.
        let winter = next_occurrence("0 8 * * *", &tz, utc(2026, 1, 15, 6, 0)).unwrap();
        assert_eq!(winter, utc(2026, 1, 15, 13, 0));

        // US DST starts 2026-03-08 at 02:00 local (07:00 UTC). At 09:00 UTC
        // that day the clock is already EDT, so 08:00 local is 12:00 UTC —
        // an hour earlier than the same expression in winter. A naive
        // UTC-only implementation would still say 13:00 here.
        let after_switch = next_occurrence("0 8 * * *", &tz, utc(2026, 3, 8, 9, 0)).unwrap();
        assert_eq!(after_switch, utc(2026, 3, 8, 12, 0));
        assert_eq!(
            local_time(&tz, after_switch).unwrap(),
            "2026-03-08 08:00:00 EDT"
        );
        assert_eq!(local_time(&tz, winter).unwrap(), "2026-01-15 08:00:00 EST");
    }

    #[test]
    fn catch_up_runs_once_and_skips_missed_occurrences() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        // Hourly task whose last scheduled fire was 5 days ago (downtime).
        let scheduled_for = utc(2026, 6, 1, 0, 0);
        let now = utc(2026, 6, 6, 10, 30);
        let next = next_after_run("0 * * * *", &tz, now, scheduled_for).unwrap();
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
        let next = next_after_run("0 * * * *", &tz, utc(2026, 6, 6, 10, 0), scheduled_for).unwrap();
        assert_eq!(next, utc(2026, 6, 6, 11, 0));
    }

    /// Fall-back 2026-11-01 in New York: 01:30 happens twice (05:30 UTC on
    /// EDT, 06:30 UTC on EST). A fixed-time job runs on the first pass only;
    /// the next occurrence is the following day, not the repeated hour.
    #[test]
    fn fall_back_runs_fixed_time_job_once() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();
        let first = next_occurrence("30 1 * * *", &tz, utc(2026, 11, 1, 4, 0)).unwrap();
        assert_eq!(first, utc(2026, 11, 1, 5, 30));
        let second = next_occurrence("30 1 * * *", &tz, first).unwrap();
        assert_eq!(
            second,
            utc(2026, 11, 2, 6, 30),
            "the repeated 01:30 (06:30 UTC) must not fire again"
        );
    }

    /// Spring-forward 2026-03-08 in New York: 02:30 does not exist. A
    /// fixed-time job scheduled in the gap runs at the first valid instant
    /// after it, 03:00 EDT (07:00 UTC), instead of being skipped for the day.
    #[test]
    fn spring_forward_gap_moves_fixed_time_job_past_the_gap() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();
        let next = next_occurrence("30 2 * * *", &tz, utc(2026, 3, 8, 5, 0)).unwrap();
        assert_eq!(next, utc(2026, 3, 8, 7, 0));
    }

    /// Legacy IANA links stay valid: tasks stored with them must keep
    /// resolving whatever tz database the host ships (Debian trixie moved
    /// these names to `tzdata-legacy`).
    #[test]
    fn legacy_timezone_links_still_resolve() {
        let tz = parse_timezone(Some("US/Eastern")).unwrap();
        assert_eq!(tz.name(), "US/Eastern");
        // Same instant rule as America/New_York.
        let next = next_occurrence("0 8 * * *", &tz, utc(2026, 6, 15, 6, 0)).unwrap();
        assert_eq!(next, utc(2026, 6, 15, 12, 0));
    }

    /// `Etc/Unknown` is not a real zone: storing it would be UTC in disguise.
    #[test]
    fn rejects_unknown_placeholder_timezone() {
        assert!(parse_timezone(Some("Etc/Unknown")).is_err());
        assert!(parse_timezone(Some("")).is_err());
    }

    /// The name written to `scheduled_tasks.timezone` is the canonical IANA
    /// spelling, so re-parsing it later never depends on the input's case.
    #[test]
    fn stored_timezone_name_is_canonical() {
        assert_eq!(
            parse_timezone(Some("America/New_York")).unwrap().name(),
            "America/New_York"
        );
        assert_eq!(
            parse_timezone(Some("america/new_york")).unwrap().name(),
            "America/New_York"
        );
        assert_eq!(parse_timezone(Some("UTC")).unwrap().name(), "UTC");
    }

    /// croner < 5 read a month name in the weekday field as a number
    /// (`jan` = 1 = Monday) and a weekday name in the minute field as its
    /// number, so these schedules ran on days the user never wrote. They
    /// must be refused when the task is created.
    #[test]
    fn rejects_names_outside_their_own_field() {
        assert!(parse_cron("0 0 * * jan").is_err());
        assert!(parse_cron("mon * * * *").is_err());
        // Names in their own fields are still fine.
        assert!(parse_cron("0 9 * * mon-fri").is_ok());
        assert!(parse_cron("0 9 1 jan *").is_ok());
    }

    /// `SUN-SUN` means Sundays. croner < 5.0.1 turned it into `0-7` and ran
    /// the task every day.
    #[test]
    fn sunday_to_sunday_range_is_sundays_only() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        // 2026-06-15 is a Monday; the next Sunday is 2026-06-21.
        let next = next_occurrence("0 0 * * SUN-SUN", &tz, utc(2026, 6, 15, 0, 30)).unwrap();
        assert_eq!(next, utc(2026, 6, 21, 0, 0));
    }

    #[test]
    fn retry_backoff_grows_and_caps() {
        assert_eq!(retry_delay_secs(1), 60);
        assert_eq!(retry_delay_secs(2), 240);
        assert_eq!(retry_delay_secs(3), 540);
        assert_eq!(retry_delay_secs(100), 3600, "must saturate at one hour");
    }
}
