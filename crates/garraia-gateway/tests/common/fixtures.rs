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

/// Seed a **second** user and insert them as an additional `owner`
/// of the given `group_id`, then mint a JWT for them.
///
/// Used by plan 0020 scenarios M5/D5 which need a group with two
/// owners so a self-demote / self-leave of the first owner stays
/// within the last-owner invariant. The product API deliberately
/// does NOT expose a way to promote someone to `owner` (setRole
/// rejects `role = "owner"` with 400), so this helper is the only
/// sanctioned way for tests to reach the "two-owners" state.
///
/// ## Index warning — **caller-owned cleanup**
///
/// The partial UNIQUE index `group_members_single_owner_idx`
/// (migration 002:146) forbids two active `'owner'` rows for a
/// given `group_id`. This fixture drops that index before the
/// INSERT and **does not recreate it** — recreating while the
/// group still has two owners would fail immediately.
///
/// The caller is responsible for restoring single-owner state
/// (via a setRole-to-admin or admin_pool UPDATE) and then
/// recreating the index. See [`restore_single_owner_idx`] for the
/// companion helper. Tests that forget to restore the index will
/// leak the dropped state into the shared test harness and break
/// subsequent scenarios — so the contract here is strict.
///
/// Returns `(user_id, jwt_token)`.
pub async fn seed_second_owner_via_admin(
    h: &Harness,
    group_id: Uuid,
    email: &str,
) -> anyhow::Result<(Uuid, String)> {
    let user_id = Uuid::new_v4();

    let mut tx = h
        .admin_pool
        .begin()
        .await
        .context("seed_second_owner: tx begin")?;

    // Insert the second user first.
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&mut *tx)
    .await
    .context("seed_second_owner: insert users")?;

    // Drop the single-owner partial unique index. The caller is
    // responsible for recreating it once the group is back to a
    // single active owner (see `restore_single_owner_idx`). We do
    // NOT recreate here because the post-INSERT state has two
    // owners and the UNIQUE partial index would fail to build.
    sqlx::query("DROP INDEX IF EXISTS group_members_single_owner_idx")
        .execute(&mut *tx)
        .await
        .context("seed_second_owner: drop idx")?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'owner', 'active')",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("seed_second_owner: insert group_members")?;

    tx.commit()
        .await
        .context("seed_second_owner: tx commit")?;

    let token = h.jwt.issue_access_for_test(user_id);
    Ok((user_id, token))
}

/// Recreate `group_members_single_owner_idx` — the companion helper
/// to [`seed_second_owner_via_admin`]. Call after the test has put
/// the group back into a single-active-owner state.
///
/// Idempotent: uses `CREATE UNIQUE INDEX IF NOT EXISTS` so callers
/// don't need to track whether the index was actually dropped.
pub async fn restore_single_owner_idx(h: &Harness) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS group_members_single_owner_idx \
         ON group_members(group_id) WHERE role = 'owner'",
    )
    .execute(&h.admin_pool)
    .await
    .context("restore_single_owner_idx: CREATE UNIQUE INDEX")?;
    Ok(())
}

/// Seed a user + insert them as a member of the given `group_id` with
/// the requested `role` and `status = 'active'`. Mints a JWT.
///
/// Used by plan 0020 scenarios that need specific hierarchy actors
/// (an admin, a member, a guest) inside the same group without
/// creating multiple groups. The role must be one of:
/// `admin`, `member`, `guest`, `child`. For `owner` use
/// [`seed_second_owner_via_admin`] (index workaround).
///
/// Returns `(user_id, jwt_token)`.
pub async fn seed_member_via_admin(
    h: &Harness,
    group_id: Uuid,
    role: &str,
    email: &str,
) -> anyhow::Result<(Uuid, String)> {
    assert!(
        ["admin", "member", "guest", "child"].contains(&role),
        "seed_member_via_admin: role '{role}' must be one of admin/member/guest/child; \
         use seed_second_owner_via_admin for owner"
    );

    let user_id = Uuid::new_v4();

    let mut tx = h
        .admin_pool
        .begin()
        .await
        .context("seed_member: tx begin")?;

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&mut *tx)
    .await
    .context("seed_member: insert users")?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(group_id)
    .bind(user_id)
    .bind(role)
    .execute(&mut *tx)
    .await
    .context("seed_member: insert group_members")?;

    tx.commit()
        .await
        .context("seed_member: tx commit")?;

    let token = h.jwt.issue_access_for_test(user_id);
    Ok((user_id, token))
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
pub async fn seed_user_without_group(h: &Harness, email: &str) -> anyhow::Result<(Uuid, String)> {
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
