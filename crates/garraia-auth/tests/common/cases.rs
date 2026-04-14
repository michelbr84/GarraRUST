//! Case types for the GAR-392 RLS matrix.
//!
//! This module is intentionally narrow: only the types needed by
//! `rls_matrix.rs` live here. The app-layer types (`AppCase`,
//! `Relationship`, `Expected`, `DenyKind`) were removed by plan 0013
//! path C amendment and will return in plan 0014 when Phase 3.4
//! materializes the REST handlers that GAR-391d targets.
//!
//! `Role` and `Action` from `garraia_auth` are deliberately NOT re-exported
//! here — the pure RLS matrix does not touch `fn can()` and re-exporting
//! them would emit `unused_import` warnings until plan 0014 lands.

/// Which dedicated Postgres role a case connects as.
///
/// All three are defined by migrations 008/010 in `garraia-workspace`:
/// `garraia_app` (RLS-enforced), `garraia_login` (BYPASSRLS, login path),
/// `garraia_signup` (BYPASSRLS, signup path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbRole {
    App,
    Login,
    Signup,
}

impl DbRole {
    pub fn as_str(self) -> &'static str {
        match self {
            DbRole::App => "garraia_app",
            DbRole::Login => "garraia_login",
            DbRole::Signup => "garraia_signup",
        }
    }
}

/// SQL op exercised by a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SqlOp {
    Select,
    Insert,
    Update,
    Delete,
}

/// How the session GUCs `app.current_user_id` and `app.current_group_id`
/// are configured before the op runs. See design doc §2.6 and §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenantCtx {
    /// Both GUCs set to the tenant's real member + group.
    Correct,
    /// `current_user_id` correct, `current_group_id` points at another group.
    WrongGroupCorrectUser,
    /// Neither GUC set — exercises the NULLIF fail-closed policy.
    BothUnset,
    /// Role correct for the op, but both GUCs point at an unrelated tenant.
    CorrectRoleWrongTenant,
}

/// Oracle outcome for an RLS case. The executor classifies the Postgres
/// response into exactly one of these and the matrix compares against the
/// expected variant.
///
/// See design doc §4.1 for the rule distinguishing `InsufficientPrivilege`
/// (42501 at the GRANT layer), `PermissionDenied` (42501 from a WITH CHECK
/// RLS write policy), and `RlsFilteredZero` (USING clause silently filtered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RlsExpected {
    /// SELECT returned exactly `n` rows, or a write affected exactly `n`.
    RowsVisible(usize),
    /// SQLSTATE 42501 with message prefix `permission denied for (table|relation)`.
    InsufficientPrivilege,
    /// SQLSTATE 42501 with message `new row violates row-level security policy`.
    PermissionDenied,
    /// Query succeeded without error but returned zero rows / affected zero.
    RlsFilteredZero,
}

/// One case of the RLS matrix. All fields are static so the matrix can
/// live in a `const` slice.
#[derive(Debug, Clone, Copy)]
pub struct RlsCase {
    pub case_id: &'static str,
    pub db_role: DbRole,
    pub table: &'static str,
    pub op: SqlOp,
    pub tenant_ctx: TenantCtx,
    pub expected: RlsExpected,
}
