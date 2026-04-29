//! Integration tests for `SessionStore::issue` / `verify_refresh` / `revoke`
//! against a real Postgres testcontainer (pgvector/pg16).
//!
//! GAR-463 Q6.1 (parent GAR-436) — kills these mutants from the
//! cargo-mutants baseline (run 25072579785, 85.04% killed):
//!
//!   - `crates/garraia-auth/src/sessions.rs:115` `verify_refresh → Ok(None)`
//!   - `crates/garraia-auth/src/sessions.rs:158` `revoke → Ok(())`
//!
//! Mirrors the boot pattern from `tests/verify_internal.rs` (per-test
//! container) rather than the process-wide `tests/common/harness.rs`
//! (which is feature-gated to `test-support` for the RLS matrix). One test
//! per mutant + a couple of negative-path companions to keep the audit
//! trail clean.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use garraia_auth::{
    JwtConfig, JwtIssuer, LoginConfig, LoginPool, SessionId, SessionStore,
};
use garraia_workspace::{Workspace, WorkspaceConfig};
use secrecy::{ExposeSecret, SecretString};
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

/// Per-test fixture: container handle (kept alive for the duration of the
/// test) + admin pool + the constructed `SessionStore` + a `JwtIssuer`
/// suitable for issuing/verifying refresh tokens against the store.
///
/// Holding `_container` here is what keeps the Postgres instance alive
/// across the test body — dropping `Fixture` mid-test would tear down the
/// container and turn every subsequent query into a connection error.
struct Fixture {
    _container: ContainerAsync<PgImage>,
    admin_pool: sqlx::PgPool,
    store: SessionStore,
    issuer: JwtIssuer,
}

async fn boot() -> anyhow::Result<Fixture> {
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let postgres_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // Apply migrations 001..010 (010 grants SELECT on sessions to garraia_login,
    // which is required for verify_refresh to return rows).
    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // Promote garraia_login to LOGIN with a known password (test-only).
    let admin_pool = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin_pool)
        .await?;

    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let login_pool = Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 4,
        })
        .await?,
    );

    let store = SessionStore::new(login_pool);
    // Build the issuer via the public `JwtConfig` constructor (same pattern
    // as the unit tests in `src/jwt.rs::tests::cfg`). The `new_for_test`
    // helper is feature-gated to `test-support` and not visible from
    // integration tests by default; using the public API keeps this file
    // free of Cargo.toml feature toggles.
    let issuer = JwtIssuer::new(JwtConfig {
        jwt_secret: SecretString::from("q6-1-test-jwt-secret-32-bytes!!!".to_owned()),
        refresh_hmac_secret: SecretString::from(
            "q6-1-test-refresh-hmac-32-bytes!".to_owned(),
        ),
    })?;

    Ok(Fixture {
        _container: container,
        admin_pool,
        store,
        issuer,
    })
}

/// Insert a `users` row directly via the admin pool (bypassing RLS/grants
/// the same way `verify_internal.rs::seed_user` does). Returns `user_id`.
///
/// Email uses the `.invalid` TLD (RFC 2606) plus a fresh UUID for safe
/// parallel test execution.
async fn seed_user(admin: &sqlx::PgPool) -> anyhow::Result<Uuid> {
    let user_id = Uuid::now_v7();
    let email = format!("session-test-{}@example.invalid", Uuid::now_v7());
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(&email)
    .bind(format!("Session Test {user_id}"))
    .execute(admin)
    .await?;
    Ok(user_id)
}

/// Issue a refresh-token pair via `JwtIssuer`, then persist a `sessions`
/// row via `SessionStore::issue`. Returns `(plaintext, session_id)`.
async fn issue_session_for(
    f: &Fixture,
    user_id: Uuid,
) -> anyhow::Result<(String, SessionId, DateTime<Utc>)> {
    let pair = f.issuer.issue_refresh()?;
    let (sid, expires_at) = f
        .store
        .issue(user_id, &pair.hmac_hash, None)
        .await?;
    Ok((pair.plaintext.expose_secret().to_string(), sid, expires_at))
}

// ───────────────────────────────────────────────────────────────────────────
// verify_refresh — kills sessions.rs:115 `Ok(None)`
// ───────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_refresh_returns_none_for_unknown_token() -> anyhow::Result<()> {
    let f = boot().await?;
    // No session ever issued → DB lookup returns 0 rows.
    let result = f
        .store
        .verify_refresh("definitely-not-a-real-token", &f.issuer)
        .await?;
    assert!(
        result.is_none(),
        "verify_refresh must return None for an unknown plaintext"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_refresh_returns_some_for_valid_token() -> anyhow::Result<()> {
    let f = boot().await?;
    let user_id = seed_user(&f.admin_pool).await?;
    let (plaintext, sid, _exp) = issue_session_for(&f, user_id).await?;

    let result = f.store.verify_refresh(&plaintext, &f.issuer).await?;
    let Some((found_sid, found_uid)) = result else {
        panic!(
            "verify_refresh returned Ok(None) for the just-issued token — \
             mutant `sessions.rs:115 → Ok(None)` triggers this panic"
        );
    };
    assert_eq!(found_sid, sid);
    assert_eq!(found_uid, user_id);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_refresh_returns_none_for_revoked_session() -> anyhow::Result<()> {
    let f = boot().await?;
    let user_id = seed_user(&f.admin_pool).await?;
    let (plaintext, sid, _exp) = issue_session_for(&f, user_id).await?;

    // Revoke first.
    f.store.revoke(sid).await?;

    // Then verify must return None — the row has revoked_at set.
    let result = f.store.verify_refresh(&plaintext, &f.issuer).await?;
    assert!(
        result.is_none(),
        "verify_refresh must return None after revoke — mutant \
         `sessions.rs:158 revoke → Ok(())` (no-op) leaves revoked_at NULL \
         and would make this assertion fail"
    );
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────────
// revoke — kills sessions.rs:158 `Ok(())` (also covered above as a chain;
// this is the focused, narrower test)
// ───────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoke_is_idempotent_and_persists() -> anyhow::Result<()> {
    let f = boot().await?;
    let user_id = seed_user(&f.admin_pool).await?;
    let (plaintext, sid, _exp) = issue_session_for(&f, user_id).await?;

    // First revoke succeeds and sets revoked_at = now().
    f.store.revoke(sid).await?;

    // Second revoke is a no-op (UPDATE ... AND revoked_at IS NULL matches 0
    // rows). Must still return Ok(()) — both the real impl AND the mutant
    // pass this assertion, BUT...
    f.store.revoke(sid).await?;

    // ...the verify_refresh below would return Some(...) under the mutant
    // because revoked_at would still be NULL. The real impl revoked the row
    // on the first call, so verify_refresh returns None.
    let result = f.store.verify_refresh(&plaintext, &f.issuer).await?;
    assert!(
        result.is_none(),
        "after revoke + idempotent re-revoke, verify_refresh must return None"
    );

    // Direct DB observation: revoked_at is non-NULL.
    let revoked_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT revoked_at FROM sessions WHERE id = $1",
    )
    .bind(sid.0)
    .fetch_one(&f.admin_pool)
    .await?;
    assert!(
        revoked_at.is_some(),
        "revoked_at must be populated after revoke — mutant `Ok(())` (no-op) \
         leaves it NULL"
    );
    Ok(())
}
