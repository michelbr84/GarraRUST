//! RFC 5545 recurrence for workspace tasks.
//!
//! `tasks.recurrence_rrule` has existed since migration 006 with a charset
//! CHECK and **no parser** — nothing ever expanded it. This module is that
//! engine: pure functions over an RRULE string plus a timezone, so the
//! semantics (DST, UNTIL/COUNT exhaustion, catch-up after downtime) are
//! unit-testable without a database.

use crate::error::{Result, WorkspaceError};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use rrule::{RRuleSet, Tz as RruleTz};

/// Default timezone when a task does not pin one. Matches the project's
/// narrative timezone convention.
pub const DEFAULT_TIMEZONE: &str = "America/New_York";

/// Upper bound on how many occurrences we walk when searching forward.
/// Guards against a pathological rule (e.g. `FREQ=SECONDLY`) turning one
/// tick into an unbounded loop.
const MAX_SCAN: u16 = 512;

/// Parse an IANA timezone, falling back to the project default.
pub fn parse_timezone(name: Option<&str>) -> Result<Tz> {
    let raw = name.unwrap_or(DEFAULT_TIMEZONE);
    raw.parse::<Tz>()
        .map_err(|_| WorkspaceError::Recurrence(format!("unknown timezone '{raw}'")))
}

/// Build an `RRuleSet` anchored at `dtstart`, evaluated in `tz`.
///
/// Accepts both a bare rule (`FREQ=WEEKLY;BYDAY=MO`) and the prefixed form
/// (`RRULE:FREQ=WEEKLY;BYDAY=MO`) — the column's CHECK allows both.
fn build_set(rrule: &str, tz: Tz, dtstart: DateTime<Utc>) -> Result<RRuleSet> {
    let body = rrule.trim();
    let body = body.strip_prefix("RRULE:").unwrap_or(body);
    let parsed: rrule::RRule<rrule::Unvalidated> = body
        .parse()
        .map_err(|e| WorkspaceError::Recurrence(format!("invalid RRULE '{rrule}': {e}")))?;

    let start = dtstart.with_timezone(&RruleTz::Tz(tz));
    let validated = parsed
        .validate(start)
        .map_err(|e| WorkspaceError::Recurrence(format!("invalid RRULE '{rrule}': {e}")))?;
    Ok(RRuleSet::new(start).rrule(validated))
}

/// First occurrence strictly after `after`.
///
/// Returns `None` when the rule is exhausted (`UNTIL`/`COUNT` reached) —
/// the caller should then stop recurring the task instead of treating it
/// as an error.
pub fn next_occurrence(
    rrule: &str,
    tz: Tz,
    dtstart: DateTime<Utc>,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let set = build_set(rrule, tz, dtstart)?;
    // `all` caps the walk at MAX_SCAN; `limited` tells us the rule has more
    // occurrences beyond the window, which only matters if we found none.
    let result = set.all(MAX_SCAN);
    let next = result
        .dates
        .into_iter()
        .map(|d: DateTime<RruleTz>| d.with_timezone(&Utc))
        .find(|d| *d > after);
    Ok(next)
}

/// Next occurrence after a run, skipping everything already in the past.
///
/// Same catch-up policy as the personal scheduler: a task whose window was
/// missed during downtime is scheduled once going forward rather than
/// replaying every occurrence it slept through.
pub fn next_after_run(
    rrule: &str,
    tz: Tz,
    dtstart: DateTime<Utc>,
    now: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let anchor = if scheduled_for > now {
        scheduled_for
    } else {
        now
    };
    next_occurrence(rrule, tz, dtstart, anchor)
}

/// Validate a rule without computing anything — used to reject a bad RRULE
/// at write time rather than silently never firing.
pub fn validate(rrule: &str, tz: Tz, dtstart: DateTime<Utc>) -> Result<()> {
    build_set(rrule, tz, dtstart).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    #[test]
    fn rejects_garbage_rules() {
        let tz = parse_timezone(None).unwrap();
        assert!(validate("NOT A RULE", tz, utc(2026, 6, 1, 9, 0)).is_err());
        assert!(parse_timezone(Some("Mars/Olympus")).is_err());
    }

    #[test]
    fn accepts_bare_and_prefixed_forms() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let start = utc(2026, 6, 1, 9, 0);
        assert!(validate("FREQ=DAILY", tz, start).is_ok());
        assert!(validate("RRULE:FREQ=DAILY", tz, start).is_ok());
    }

    #[test]
    fn daily_rule_advances_one_day() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let start = utc(2026, 6, 1, 9, 0);
        let next = next_occurrence("FREQ=DAILY", tz, start, start)
            .unwrap()
            .expect("daily rule never exhausts");
        assert_eq!(next, utc(2026, 6, 2, 9, 0));
    }

    #[test]
    fn weekly_byday_picks_the_next_weekday() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        // 2026-06-01 is a Monday.
        let start = utc(2026, 6, 1, 9, 0);
        let next = next_occurrence("FREQ=WEEKLY;BYDAY=MO,WE", tz, start, start)
            .unwrap()
            .unwrap();
        assert_eq!(next, utc(2026, 6, 3, 9, 0), "Wednesday comes next");
    }

    /// The whole reason for a timezone-aware engine: a 09:00 local task keeps
    /// firing at 09:00 local across the DST switch, which is a different UTC
    /// instant on either side.
    #[test]
    fn daily_rule_holds_local_time_across_dst() {
        let tz = parse_timezone(Some("America/New_York")).unwrap();
        // Start in EST (UTC-5): 09:00 local == 14:00 UTC.
        let start = utc(2026, 3, 6, 14, 0);
        assert_eq!(
            next_occurrence("FREQ=DAILY", tz, start, start)
                .unwrap()
                .unwrap(),
            utc(2026, 3, 7, 14, 0),
            "still EST the next day"
        );
        // DST starts 2026-03-08; after it, 09:00 local == 13:00 UTC.
        let after_switch = next_occurrence("FREQ=DAILY", tz, start, utc(2026, 3, 8, 20, 0))
            .unwrap()
            .unwrap();
        assert_eq!(after_switch, utc(2026, 3, 9, 13, 0));
    }

    #[test]
    fn count_limited_rule_reports_exhaustion() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let start = utc(2026, 6, 1, 9, 0);
        // COUNT includes DTSTART, so there are exactly two more occurrences.
        let second = next_occurrence("FREQ=DAILY;COUNT=3", tz, start, start)
            .unwrap()
            .unwrap();
        let third = next_occurrence("FREQ=DAILY;COUNT=3", tz, start, second)
            .unwrap()
            .unwrap();
        assert_eq!(third, utc(2026, 6, 3, 9, 0));
        assert!(
            next_occurrence("FREQ=DAILY;COUNT=3", tz, start, third)
                .unwrap()
                .is_none(),
            "exhausted rule must report None, not an error"
        );
    }

    #[test]
    fn until_limited_rule_reports_exhaustion() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let start = utc(2026, 6, 1, 9, 0);
        let rule = "FREQ=DAILY;UNTIL=20260603T090000Z";
        assert!(next_occurrence(rule, tz, start, start).unwrap().is_some());
        assert!(
            next_occurrence(rule, tz, start, utc(2026, 6, 3, 9, 0))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn catch_up_skips_occurrences_missed_during_downtime() {
        let tz = parse_timezone(Some("UTC")).unwrap();
        let start = utc(2026, 6, 1, 9, 0);
        // Was due a week ago; the gateway is only back now.
        let next = next_after_run(
            "FREQ=DAILY",
            tz,
            start,
            utc(2026, 6, 8, 10, 0),
            utc(2026, 6, 1, 9, 0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            next,
            utc(2026, 6, 9, 9, 0),
            "resume from now instead of replaying a week of dailies"
        );
    }
}
