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
/// Uses the harness's **shared** `admin_pool` (built once in
/// `Harness::boot`) instead of opening a fresh pool per call.
/// Opening a fresh `PgPool` per fixture call would exhaust
/// Postgres `max_connections` in parallel test suites — a bug
/// discovered during plan 0016 M3-T3 when 5 `#[tokio::test]`
/// functions each opened their own fixture pool on top of the
/// three harness pools (48 connections).
pub async fn seed_user_with_group(
    h: &Harness,
    email: &str,
) -> anyhow::Result<(Uuid, Uuid, String)> {
    let user_id = Uuid::new_v4();
    let group_id = Uuid::new_v4();

    // Single transaction: acquires ONE connection for all 3 inserts
    // and either commits all or rolls back all. Crucial for parallel
    // test suites — 5 `#[tokio::test]` × 3 sequential inserts against
    // a small shared admin_pool hits the `acquire_timeout` otherwise.
    // Discovered during plan 0016 M3-T3 debugging.
    let mut tx = h.admin_pool.begin().await.context("fixture tx begin")?;

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&mut *tx)
    .await
    .context("fixture insert users")?;

    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by) \
         VALUES ($1, $2, 'team', $3)",
    )
    .bind(group_id)
    .bind(format!("Test group for {}", email))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("fixture insert groups")?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'owner', 'active')",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("fixture insert group_members")?;

    tx.commit().await.context("fixture tx commit")?;

    let token = h.jwt.issue_access_for_test(user_id);

    Ok((user_id, group_id, token))
}

/// Seed an authenticated user with NO group membership, then mint a
/// JWT for that user via `Harness::jwt::issue_access_for_test`.
///
/// Returns `(user_id, jwt_token)`.
///
/// Used by the GAR-391d authz matrix (plan 0014) to exercise the
/// "authenticated but not a member of any group" vector. Cross-group
/// authorization cannot be validated without this actor: the
/// `Principal` extractor's 403 path on `GET /v1/groups/{id}` only
/// fires when the caller has a valid JWT but no matching row in
/// `group_members` for the requested `X-Group-Id`.
///
/// Follows the same single-transaction pattern as
/// `seed_user_with_group` so the test suite does not exhaust
/// Postgres connections under parallel scenarios (lesson from
/// plan 0016 M3-T3 pool exhaustion).
pub async fn seed_user_without_group(
    h: &Harness,
    email: &str,
) -> anyhow::Result<(Uuid, String)> {
    let user_id = Uuid::new_v4();

    let mut tx = h
        .admin_pool
        .begin()
        .await
        .context("seed_user_without_group: tx begin")?;

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&mut *tx)
    .await
    .context("seed_user_without_group: insert users")?;

    tx.commit()
        .await
        .context("seed_user_without_group: tx commit")?;

    let token = h.jwt.issue_access_for_test(user_id);

    Ok((user_id, token))
}
