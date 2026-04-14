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
//! | `RowsVisible(n)`       | query ok, >= 1 rows / affected              |
//! | `RlsFilteredZero`      | query ok, zero rows / affected              |
//!
//! Postgres 16 message text is stable across patch releases; if a future
//! upgrade changes the wording the matrix will surface the regression
//! immediately (a case flipping from `InsufficientPrivilege` to `None`
//! panics with the raw error so the new prefix can be added here).
//!
//! Plan 0013 path C — Task 8.

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
/// Any positive count collapses to `RowsVisible(1)`. The oracle's
/// semantic is "is any row visible under this GUC configuration vs.
/// filtered out" — not "exactly how many rows". Collapsing makes the
/// matrix robust against shared-harness state where BYPASSRLS roles
/// (`garraia_login`, `garraia_signup`) observe users / identities /
/// group_members accumulated by *other* test cases in the same process.
/// Negative or zero counts always classify as `RlsFilteredZero`.
pub fn classify_count(n: i64) -> RlsExpected {
    if n <= 0 {
        RlsExpected::RlsFilteredZero
    } else {
        RlsExpected::RowsVisible(1)
    }
}
