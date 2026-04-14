//! Meta tripwires — compile-time counters and coverage invariants that
//! ring an alarm when the RLS matrix degrades silently.
//!
//! Runs as pure Rust unit-style asserts; no container, no Postgres, no
//! tenant — reads `RLS_MATRIX` from `common::matrix` as static data.
//!
//! Plan 0013 path C — Task 9.

#![cfg(feature = "test-support")]

mod common;

use common::cases::{DbRole, RlsCase, RlsExpected, TenantCtx};
use common::matrix::RLS_MATRIX;
use std::collections::HashSet;

/// Tripwire 1: total case count must stay at or above the amendment
/// threshold (80). A shrinking matrix is a silent regression that
/// would otherwise only surface in a distant future audit.
///
/// The threshold comes from plan 0013 path C §Revised targets:
/// "cargo test -p garraia-auth --test rls_matrix passes with >= 80 cases".
#[test]
fn total_case_count_at_least_80() {
    let total = RLS_MATRIX.len();
    assert!(
        total >= 80,
        "RLS matrix degraded: {total} cases < 80 minimum. \
         See plan 0013 path C Success Criteria §6.3.",
    );
}

/// Tripwire 2: every `DbRole` variant must appear in the matrix at
/// least once AND each role must exercise at least 2 distinct
/// `RlsExpected` outcomes. This catches a regression where a block
/// collapses to only "allow" or only "deny" cases (degenerate
/// coverage).
///
/// See design doc §4.3 for the rule: `garraia_app` exercises all four
/// `TenantCtx` variants; `garraia_login`/`garraia_signup` exercise both
/// positive and negative GRANT-layer paths.
#[test]
fn rls_role_coverage_check() {
    let mut seen_roles: HashSet<DbRole> = HashSet::new();
    let mut outcomes_by_role: std::collections::HashMap<DbRole, HashSet<std::mem::Discriminant<RlsExpected>>> =
        Default::default();

    for case in RLS_MATRIX {
        seen_roles.insert(case.db_role);
        outcomes_by_role
            .entry(case.db_role)
            .or_default()
            .insert(std::mem::discriminant(&case.expected));
    }

    for expected_role in [DbRole::App, DbRole::Login, DbRole::Signup] {
        assert!(
            seen_roles.contains(&expected_role),
            "coverage gap: DbRole::{expected_role:?} has zero cases in RLS_MATRIX",
        );
        let outcomes = outcomes_by_role.get(&expected_role).cloned().unwrap_or_default();
        assert!(
            outcomes.len() >= 2,
            "coverage gap: DbRole::{expected_role:?} exercises only {} RlsExpected variant(s), \
             need >= 2 (at least one allow + one deny path)",
            outcomes.len(),
        );
    }
}

/// Tripwire 3: every case must have a unique `case_id`. Duplicate ids
/// would make failure triage ambiguous (two different cases sharing a
/// label).
#[test]
fn case_ids_are_unique() {
    let mut seen: HashSet<&'static str> = HashSet::new();
    for case in RLS_MATRIX {
        assert!(
            seen.insert(case.case_id),
            "duplicate case_id in RLS_MATRIX: {}",
            case.case_id,
        );
    }
}

/// Tripwire 4: every `RlsExpected` variant is exercised by at least
/// one case. This guards against the oracle gaining a variant that the
/// matrix never hits (dead code on the oracle side).
#[test]
fn all_oracle_variants_exercised() {
    let mut seen: HashSet<std::mem::Discriminant<RlsExpected>> = HashSet::new();
    for case in RLS_MATRIX {
        seen.insert(std::mem::discriminant(&case.expected));
    }

    let required = [
        RlsExpected::RowsVisible(1),
        RlsExpected::RowsVisibleAny,
        RlsExpected::InsufficientPrivilege,
        RlsExpected::PermissionDenied,
        RlsExpected::RlsFilteredZero,
    ];
    for variant in &required {
        assert!(
            seen.contains(&std::mem::discriminant(variant)),
            "oracle variant {variant:?} has no corresponding case in RLS_MATRIX",
        );
    }
}

/// Tripwire 5: `garraia_app` cases must exercise all 4 `TenantCtx`
/// variants across the matrix. This validates the design doc §4.3
/// "tenant_ctx is semantically relevant for garraia_app" rule — if a
/// refactor accidentally dropped CorrectRoleWrongTenant cases, this
/// would fire immediately.
#[test]
fn app_role_exercises_all_tenant_ctx_variants() {
    let mut seen: HashSet<std::mem::Discriminant<TenantCtx>> = HashSet::new();
    for case in RLS_MATRIX {
        if case.db_role == DbRole::App {
            seen.insert(std::mem::discriminant(&case.tenant_ctx));
        }
    }

    let required = [
        TenantCtx::Correct,
        TenantCtx::WrongGroupCorrectUser,
        TenantCtx::BothUnset,
        TenantCtx::CorrectRoleWrongTenant,
    ];
    for variant in &required {
        assert!(
            seen.contains(&std::mem::discriminant(variant)),
            "garraia_app coverage gap: TenantCtx::{variant:?} has no cases",
        );
    }
}

// Keep the binding live so the unused-imports lint does not fire if a
// future change stops referencing `RlsCase` directly.
const _: &[RlsCase] = RLS_MATRIX;
