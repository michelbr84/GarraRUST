//! Test fixtures for the gateway integration harness (plan 0016 M3-T2).
//!
//! `seed_user_with_group` is the one-stop setup helper used by the
//! authed `/v1/me` tests. It bypasses RLS by connecting as the
//! container superuser (`postgres`) and inserting rows directly into
//! `users`, `groups`, and `group_members`. Token minting uses
//! `h.jwt.issue_access_for_test(user_id)` so every seeded user has
//! a valid bearer JWT for the same `JwtIssuer` that the router
//! verifies against.
//!
//! ## Boundary note
//!
//! Fixture setup is the one sanctioned place where a test may read
//! from the superuser URL. Assertions and handler-driven queries
//! must NOT use the admin pool — they run through the `garraia_app`
//! RLS-enforced pool via `h.app_pool` or through the HTTP router.
//!
//! This mirrors the pattern already validated in
//! `crates/garraia-auth/tests/common/harness.rs` (plan 0013 path C).

use anyhow::Context;
use uuid::Uuid;

use super::Harness;

/// Seed one user + one group + one `group_members` row with role
/// `owner`, then mint a JWT for that user via
/// `Harness::jwt::issue_access_for_test`.
///
/// Returns `(user_id, group_id, jwt_token)`.
///
/// A fresh admin pool is opened on every call and closed on drop —
/// same pattern as `garraia-auth/tests/common/harness.rs` uses for
/// role promotion (opens, runs ALTER ROLE, closes). Connection pools
/// against a local testcontainer are cheap enough for this.
pub async fn seed_user_with_group(
    h: &Harness,
    email: &str,
) -> anyhow::Result<(Uuid, Uuid, String)> {
    let admin_pool = sqlx::PgPool::connect(&h.admin_url)
        .await
        .context("fixture admin_pool connect")?;

    let user_id = Uuid::new_v4();
    let group_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&admin_pool)
    .await
    .context("fixture insert users")?;

    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by) \
         VALUES ($1, $2, 'team', $3)",
    )
    .bind(group_id)
    .bind(format!("Test group for {}", email))
    .bind(user_id)
    .execute(&admin_pool)
    .await
    .context("fixture insert groups")?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'owner', 'active')",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&admin_pool)
    .await
    .context("fixture insert group_members")?;

    admin_pool.close().await;

    let token = h.jwt.issue_access_for_test(user_id);

    Ok((user_id, group_id, token))
}
