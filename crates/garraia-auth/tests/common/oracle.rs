//! SQLSTATE + MESSAGE prefix classifier for the GAR-392 RLS matrix.
//!
//! The oracle must distinguish three flavors of Postgres denial that all
//! share SQLSTATE 42501:
//!
//! | Outcome                | MESSAGE prefix                             |
//! |------------------------|--------------------------------------------|
//! | `InsufficientPrivilege`| `permission denied for (table\|relation)`  |
//! | `PermissionDenied`     | contains `row-level security policy`       |
//! | (neither)              | classifier returns `None` → caller panics  |
//!
//! Plus the success path:
//!
//! | Outcome                | Condition                                   |
//! |------------------------|---------------------------------------------|
//! | `RowsVisible(n)`       | query ok, returned/affected exactly `n`     |
//! | `RlsFilteredZero`      | query ok, zero rows / affected              |
//!
//! `classify_count` ALWAYS returns the exact count (strict). The matrix then
//! compares against the expected variant via `matches_expected`, which
//! supports a loose `RowsVisibleAny` variant for BYPASSRLS roles that
//! legitimately observe accumulated state across the shared harness. See
//! the type-level docs on `RlsExpected` and security audit finding H-1
//! (2026-04-14) for why this split exists.
//!
//! Postgres 16 message text is stable across patch releases; if a future
//! upgrade changes the wording the matrix will surface the regression
//! immediately (a case flipping from `InsufficientPrivilege` to `None`
//! panics with the raw error so the new prefix can be added here).
//!
//! Plan 0013 path C — Task 8 + H-1 fix.

use super::cases::RlsExpected;

/// Classify a `sqlx::Error` coming from Postgres into an `RlsExpected`
/// denial variant, or `None` if the error is not a 42501 and therefore
/// not an authorization denial.
pub fn classify_pg_error(err: &sqlx::Error) -> Option<RlsExpected> {
    let db_err = err.as_database_error()?;
    let code = db_err.code()?;
    if code != "42501" {
        return None;
    }
    let msg = db_err.message();

    if msg.starts_with("permission denied for table")
        || msg.starts_with("permission denied for relation")
    {
        Some(RlsExpected::InsufficientPrivilege)
    } else if msg.contains("row-level security policy")
        || msg.contains("row level security policy")
    {
        Some(RlsExpected::PermissionDenied)
    } else {
        // 42501 with an unrecognized prefix — surfacing it as
        // `InsufficientPrivilege` would mask a real oracle bug.
        None
    }
}

/// Classify a numeric result (row count from SELECT or `rows_affected`
/// from a write) into the appropriate success variant.
///
/// **Strict counting.** Returns the exact count as `RowsVisible(n)`. The
/// matrix uses `matches_expected` to decide whether a strict or loose
/// comparison is required for the specific case. See security audit
/// finding H-1 (2026-04-14): collapsing `n > 0` to `RowsVisible(1)`
/// unconditionally would hide cross-tenant leaks where a policy
/// regression made `garraia_app` visible N > 1 rows under `Correct` GUCs.
/// The strict count + per-case loose flag preserves both correctness
/// (strict for `garraia_app`) and robustness against accumulated state
/// (loose for BYPASSRLS roles).
pub fn classify_count(n: i64) -> RlsExpected {
    if n <= 0 {
        RlsExpected::RlsFilteredZero
    } else {
        RlsExpected::RowsVisible(n as usize)
    }
}

/// Decide whether an observed outcome satisfies the expected outcome.
///
/// For most variants this is exact equality. The exception is
/// `RowsVisibleAny`, which matches any `RowsVisible(n)` with `n >= 1`.
/// `RowsVisibleAny` exists only for `garraia_login` / `garraia_signup`
/// SELECT allow cases where accumulated state across the harness is
/// legitimate. See `RlsExpected` type docs.
pub fn matches_expected(outcome: RlsExpected, expected: RlsExpected) -> bool {
    match expected {
        RlsExpected::RowsVisibleAny => matches!(outcome, RlsExpected::RowsVisible(_)),
        other => outcome == other,
    }
}
