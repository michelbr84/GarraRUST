//! Smoke test: compile-only guard for the `#[cfg(any(test, feature = "test-support"))]`
//! escape hatch on `LoginPool::raw()` and `SignupPool::raw()`.
//!
//! Requires the `test-support` feature:
//!
//! ```text
//! cargo test -p garraia-auth --features test-support --test raw_hatch_compile
//! ```
//!
//! Absorbed by the Task 3 harness; this file is deleted in Task 10 Step 10.1
//! once the RLS matrix suite consumes `raw()` as part of its normal flow.
//!
//! Plan 0013 path C — Task 1.

#![cfg(feature = "test-support")]

use garraia_auth::{LoginPool, SignupPool};
use sqlx::PgPool;

fn _assert_login_raw_returns_pool(p: &LoginPool) -> &PgPool {
    p.raw()
}

fn _assert_signup_raw_returns_pool(p: &SignupPool) -> &PgPool {
    p.raw()
}

#[test]
fn raw_hatches_compile() {
    // If this file compiles, the escape hatch is visible to integration tests
    // when `--features test-support` is enabled. Actual runtime behavior is
    // validated by the RLS matrix suite (Task 8).
}
