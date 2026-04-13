//! Integration smoke test for GAR-407 / migration 001.
//!
//! Starts `pgvector/pgvector:pg16` via testcontainers, connects via
//! `garraia_workspace::Workspace::connect` with `migrate_on_start = true`, and
//! verifies:
//!   1. All 7 tables exist.
//!   2. Critical indexes exist.
//!   3. A basic INSERT into `users` returns a non-nil UUID.
//!   4. A second INSERT with differently-cased email is rejected by the
//!      `citext` unique constraint.

use garraia_workspace::{Workspace, WorkspaceConfig};
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PgImage;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_001_applies_and_schema_is_sane() -> anyhow::Result<()> {
    // First run cold-pulls the image (~60s). Warm runs start in ~3-5s.
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;

    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let workspace = Workspace::connect(WorkspaceConfig {
        database_url,
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // 1. Verify all 7 tables exist.
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .fetch_all(workspace.pool())
    .await?;
    let names: Vec<&str> = tables.iter().map(|(n,)| n.as_str()).collect();
    for expected in &[
        "api_keys",
        "group_invites",
        "group_members",
        "groups",
        "sessions",
        "user_identities",
        "users",
    ] {
        assert!(
            names.contains(expected),
            "missing table: {expected} (have: {names:?})"
        );
    }

    // 2. Verify critical indexes exist (exact auto-generated names).
    let indexes: Vec<(String,)> = sqlx::query_as(
        "SELECT indexname FROM pg_indexes WHERE schemaname = 'public'",
    )
    .fetch_all(workspace.pool())
    .await?;
    let index_names: Vec<&str> = indexes.iter().map(|(n,)| n.as_str()).collect();
    for expected in &[
        "users_email_key",
        "user_identities_provider_provider_sub_key",
        "user_identities_user_id_idx",
        "sessions_user_id_idx",
        "sessions_active_expires_idx",
        "api_keys_key_hash_key",
        "api_keys_active_user_idx",
        "groups_created_by_idx",
        "group_members_user_id_idx",
        "group_members_active_by_group_idx",
        "group_invites_group_id_idx",
        "group_invites_pending_email_idx",
        "group_invites_token_hash_key",
    ] {
        assert!(
            index_names.contains(expected),
            "missing index: {expected} (have: {index_names:?})"
        );
    }

    // 3. Insert a fake user and verify UUID comes back.
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id",
    )
    .bind("test@example.com")
    .bind("Test User")
    .fetch_one(workspace.pool())
    .await?;
    assert!(!user_id.is_nil(), "expected non-nil UUID from INSERT");

    // 4. Second insert with differently-cased email must fail with the exact
    // Postgres unique_violation code (SQLSTATE 23505). Asserting just
    // `is_err()` would silently pass on any error (missing column, type
    // mismatch, etc.), producing a false green.
    let dup_err = sqlx::query("INSERT INTO users (email, display_name) VALUES ($1, $2)")
        .bind("TEST@example.com")
        .bind("Other User")
        .execute(workspace.pool())
        .await
        .expect_err("citext unique constraint should block case-insensitive dup");
    let db_err = dup_err
        .as_database_error()
        .expect("should be a database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23505"),
        "expected SQLSTATE 23505 (unique_violation), got: {db_err:?}"
    );

    Ok(())
}
