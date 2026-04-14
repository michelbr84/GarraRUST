//! Smoke test: `common/cases.rs` compiles and its types are constructible
//! as plain literal values. Validates the reduced Task 2 scope (plan 0013
//! path C).

#![cfg(feature = "test-support")]

mod common;

use common::cases::{DbRole, RlsCase, RlsExpected, SqlOp, TenantCtx};

#[test]
fn case_types_are_usable() {
    let _: RlsCase = RlsCase {
        case_id: "rls_smoke",
        db_role: DbRole::App,
        table: "chats",
        op: SqlOp::Select,
        tenant_ctx: TenantCtx::Correct,
        expected: RlsExpected::RowsVisible(1),
    };
    let _ = RlsExpected::InsufficientPrivilege;
    let _ = RlsExpected::PermissionDenied;
    let _ = RlsExpected::RlsFilteredZero;
    let _ = TenantCtx::WrongGroupCorrectUser;
    let _ = TenantCtx::BothUnset;
    let _ = TenantCtx::CorrectRoleWrongTenant;
    let _ = SqlOp::Insert;
    let _ = SqlOp::Update;
    let _ = SqlOp::Delete;
    assert_eq!(DbRole::App.as_str(), "garraia_app");
    assert_eq!(DbRole::Login.as_str(), "garraia_login");
    assert_eq!(DbRole::Signup.as_str(), "garraia_signup");
}
