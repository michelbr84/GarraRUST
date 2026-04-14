//! Task 3 smoke test — boot the shared Harness, verify:
//!   1. `OnceCell` returns the same `Arc` across calls.
//!   2. The three pools can execute `SELECT 1` / `SELECT current_user`.
//!   3. Each pool reports the expected `current_user` (verifying that the
//!      role-promotion step in `Harness::boot` worked and that the typed
//!      newtypes wrap the role they claim to wrap).
//!   4. Post-migration schema has at least the expected number of tables.
//!
//! Plan 0013 path C — Task 3.

#![cfg(feature = "test-support")]

mod common;

use common::harness::Harness;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_boots_once_and_pools_are_typed() -> anyhow::Result<()> {
    let h1 = Harness::get().await;
    let h2 = Harness::get().await;

    // OnceCell shares the same Arc across calls.
    assert!(
        Arc::ptr_eq(&h1, &h2),
        "Harness::get() must return the same Arc across invocations",
    );

    // Post-migration schema exists. Migrations 001..010 create 25 public
    // tables per CLAUDE.md §garraia-workspace. We assert >= 20 to leave
    // room for any future migration that adds auxiliary tables without
    // over-coupling this smoke test to the exact count.
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_one(&h1.app_pool)
    .await?;
    assert!(
        tables >= 20,
        "expected >= 20 public tables after migrations 001..010, got {tables}",
    );

    // `app_pool` connects as garraia_app.
    let app_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&h1.app_pool)
        .await?;
    assert_eq!(app_user, "garraia_app");

    // `login_pool` connects as garraia_login (via the test-only raw() hatch).
    let login_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(h1.login_pool.raw())
        .await?;
    assert_eq!(login_user, "garraia_login");

    // `signup_pool` connects as garraia_signup.
    let signup_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(h1.signup_pool.raw())
        .await?;
    assert_eq!(signup_user, "garraia_signup");

    Ok(())
}
