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

    // ─── Migration 002 validation ──────────────────────────────────────────
    //
    // `names` and `index_names` were populated earlier via a single query after
    // `Workspace::connect` returned. Because `migrate_on_start = true` applies
    // migrations 001 AND 002 atomically before the first query, those snapshots
    // already include everything migration 002 creates. If a future refactor
    // moves schema queries above `connect()`, these assertions will silently
    // regress — keep the query calls downstream of `connect()`.

    // New tables exist.
    for expected in &["roles", "permissions", "role_permissions", "audit_events"] {
        assert!(
            names.contains(expected),
            "missing table from migration 002: {expected}"
        );
    }

    // Partial unique index exists.
    assert!(
        index_names.contains(&"group_members_single_owner_idx"),
        "missing partial unique index group_members_single_owner_idx"
    );

    // Seed counts — pinned exact values. Loose bounds would let a silent
    // regression drop rows without failing; `==` surfaces any change.
    let roles_count: i64 = sqlx::query_scalar("SELECT count(*) FROM roles")
        .fetch_one(workspace.pool())
        .await?;
    assert_eq!(roles_count, 5, "expected exactly 5 seeded roles");

    let perms_count: i64 = sqlx::query_scalar("SELECT count(*) FROM permissions")
        .fetch_one(workspace.pool())
        .await?;
    assert_eq!(
        perms_count, 22,
        "expected exactly 22 seeded permissions, got {perms_count}"
    );

    let owner_perms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'owner'")
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(owner_perms, perms_count, "owner should have all permissions");

    let admin_perms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'admin'")
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(
        admin_perms, 20,
        "admin should have 20 permissions (all except group.delete + export.group)"
    );

    let member_perms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'member'")
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(member_perms, 11, "member should have exactly 11 permissions");

    let guest_perms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'guest'")
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(guest_perms, 6, "guest should have exactly 6 permissions");

    let child_perms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'child'")
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(
        child_perms, 4,
        "child should have exactly 4 permissions (chats read/write + tasks read/write)"
    );

    // Single-owner constraint violation.
    // Setup: create a group owned by the test user, try to add a second owner.
    let group_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO groups (name, type, created_by) VALUES ($1, 'family', $2) RETURNING id",
    )
    .bind("Test Family")
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(group_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    // Create a second user and try to add them as another owner of the same group.
    let user2_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id",
    )
    .bind("second@example.com")
    .bind("Second User")
    .fetch_one(workspace.pool())
    .await?;

    let dup_owner =
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')")
            .bind(group_id)
            .bind(user2_id)
            .execute(workspace.pool())
            .await
            .expect_err("second owner for same group must be rejected");

    let db_err = dup_owner
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23505"),
        "expected unique_violation for single-owner constraint"
    );

    // Audit event insert + read-back. Exercises both a regular row (with
    // actor_user_id) and a NULL-actor row (post-erasure survival path per
    // LGPD art. 8 §5 / GDPR art. 17(1)).
    let audit_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb) RETURNING id",
    )
    .bind(group_id)
    .bind(user_id)
    .bind("Test User")
    .bind("group.create")
    .bind("group")
    .bind(group_id.to_string())
    .bind(r#"{"source":"smoke_test"}"#)
    .fetch_one(workspace.pool())
    .await?;
    assert!(!audit_id.is_nil());

    // Read-back exercises jsonb + uuid deserialization and proves the row is
    // queryable, not just writable.
    let audit_action: String =
        sqlx::query_scalar("SELECT action FROM audit_events WHERE id = $1")
            .bind(audit_id)
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(audit_action, "group.create");

    // NULL-actor row: documents the explicit design that audit rows survive
    // hard user deletion. actor_label is still set so the audit remains
    // readable post-erasure.
    let null_actor_audit: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata) \
         VALUES ($1, NULL, $2, $3, $4, $5, $6::jsonb) RETURNING id",
    )
    .bind(group_id)
    .bind("deleted-user@example.com")
    .bind("users.delete")
    .bind("user")
    .bind("legacy-id-placeholder")
    .bind(r#"{"source":"smoke_test","reason":"erasure_survival"}"#)
    .fetch_one(workspace.pool())
    .await?;
    assert!(!null_actor_audit.is_nil());

    // ─── Migration 004 validation ──────────────────────────────────────────
    //
    // Same snapshot semantics as migration 002: `names` and `index_names` were
    // populated after `Workspace::connect` applied all migrations atomically.

    // New tables exist.
    for expected in &["chats", "chat_members", "messages", "message_threads"] {
        assert!(
            names.contains(expected),
            "missing table from migration 004: {expected}"
        );
    }

    // Critical FTS + pagination + performance indexes exist. All 8 indexes
    // migration 004 creates are asserted here — silent removal of any would
    // degrade query performance without a test catching it.
    for expected in &[
        "messages_body_tsv_idx",
        "messages_chat_created_idx",
        "messages_group_created_idx",
        "messages_thread_id_idx",
        "messages_sender_idx",
        "chats_group_id_idx",
        "chats_group_type_idx",
        "chat_members_user_id_idx",
        "chat_members_unread_idx",
        "message_threads_chat_idx",
    ] {
        assert!(
            index_names.contains(expected),
            "missing index from migration 004: {expected}"
        );
    }

    // Verify body_tsv is a STORED generated column.
    let attgenerated: String = sqlx::query_scalar(
        "SELECT attgenerated::text FROM pg_attribute \
         WHERE attrelid = 'messages'::regclass AND attname = 'body_tsv'",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert_eq!(
        attgenerated, "s",
        "body_tsv must be STORED (attgenerated='s')"
    );

    // Create a chat and add the test user as owner.
    let chat_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO chats (group_id, type, name, created_by) \
         VALUES ($1, 'channel', 'geral', $2) RETURNING id",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(chat_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    // Insert 3 messages with known tokens.
    for (body, label) in [
        (
            "Bom dia pessoal tudo certo para o churrasco no Brasil",
            "msg-brasil",
        ),
        (
            "Vou trazer carne e bebidas para a festa amanhã",
            "msg-festa",
        ),
        ("Confirma presença até amanhã por favor", "msg-confirma"),
    ] {
        sqlx::query(
            "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(chat_id)
        .bind(group_id)
        .bind(user_id)
        .bind(label)
        .bind(body)
        .execute(workspace.pool())
        .await?;
    }

    // FTS query: positive match (body contains "brasil").
    let hits_positive: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages \
         WHERE chat_id = $1 AND body_tsv @@ plainto_tsquery('portuguese', 'brasil') \
         AND deleted_at IS NULL",
    )
    .bind(chat_id)
    .fetch_all(workspace.pool())
    .await?;
    assert_eq!(
        hits_positive.len(),
        1,
        "expected exactly 1 FTS match for 'brasil'"
    );

    // FTS query: negative match (token not in any body).
    let hits_negative: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages \
         WHERE body_tsv @@ plainto_tsquery('portuguese', 'helicoptero') \
         AND deleted_at IS NULL",
    )
    .fetch_all(workspace.pool())
    .await?;
    assert_eq!(
        hits_negative.len(),
        0,
        "expected 0 FTS matches for 'helicoptero'"
    );

    // Compound FK test: message with mismatched group_id must fail.
    // Create a second group to force the mismatch.
    let other_group_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO groups (name, type, created_by) VALUES ('Other', 'team', $1) RETURNING id",
    )
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let mismatch = sqlx::query(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(chat_id) // chat belongs to `group_id`
    .bind(other_group_id) // but we claim `other_group_id`
    .bind(user_id)
    .bind("Test User")
    .bind("should fail")
    .execute(workspace.pool())
    .await
    .expect_err("compound FK should reject cross-group message");

    let db_err = mismatch
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23503"),
        "expected SQLSTATE 23503 (foreign_key_violation)"
    );

    Ok(())
}
