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
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
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
    let indexes: Vec<(String,)> =
        sqlx::query_as("SELECT indexname FROM pg_indexes WHERE schemaname = 'public'")
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
    let user_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id")
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
    assert_eq!(
        owner_perms, perms_count,
        "owner should have all permissions"
    );

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
    assert_eq!(
        member_perms, 11,
        "member should have exactly 11 permissions"
    );

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
    let user2_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id")
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

    let db_err = dup_owner.as_database_error().expect("database-layer error");
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
    let audit_action: String = sqlx::query_scalar("SELECT action FROM audit_events WHERE id = $1")
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

    let db_err = mismatch.as_database_error().expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23503"),
        "expected SQLSTATE 23503 (foreign_key_violation)"
    );

    // ─── Migration 005 validation ──────────────────────────────────────────

    // Extension `vector` is installed.
    let has_vector: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector')")
            .fetch_one(workspace.pool())
            .await?;
    assert!(
        has_vector,
        "pgvector extension must be installed by migration 005"
    );

    // New tables exist.
    for expected in &["memory_items", "memory_embeddings"] {
        assert!(
            names.contains(expected),
            "missing table from migration 005: {expected}"
        );
    }

    // HNSW index exists.
    assert!(
        index_names.contains(&"memory_embeddings_embedding_hnsw_idx"),
        "missing HNSW index from migration 005"
    );

    // Insert 3 memory_items (1 per scope) + embeddings.
    let memory_fact_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content) \
         VALUES ('group', $1, $2, $3, 'Test User', 'fact', 'A família gosta de churrasco aos domingos') \
         RETURNING id",
    )
    .bind(group_id)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let memory_pref_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content) \
         VALUES ('user', $1, NULL, $2, 'Test User', 'preference', 'Prefere respostas curtas') \
         RETURNING id",
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let memory_note_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content) \
         VALUES ('chat', $1, $2, $3, 'Test User', 'note', 'Combinamos churrasco dia 20') \
         RETURNING id",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    // Deterministic unit-normalized 768-d vectors. We use ChaCha8Rng
    // explicitly (not StdRng) so the bit-for-bit output is stable across
    // rand crate major versions — StdRng's backing algorithm is not
    // guaranteed by the rand contract.
    fn unit_vector(seed: u64) -> pgvector::Vector {
        use rand::{Rng, SeedableRng};
        use rand_chacha::ChaCha8Rng;
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut v: Vec<f32> = (0..768).map(|_| rng.gen_range(-1.0..1.0)).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
        pgvector::Vector::from(v)
    }

    // Generic 768-d helper for the wrong-dimension negative test below.
    fn vector_of_dim(dim: usize) -> pgvector::Vector {
        pgvector::Vector::from(vec![0.0_f32; dim])
    }

    for (item_id, seed) in [
        (memory_fact_id, 1u64),
        (memory_pref_id, 2u64),
        (memory_note_id, 3u64),
    ] {
        sqlx::query(
            "INSERT INTO memory_embeddings (memory_item_id, model, embedding) \
             VALUES ($1, $2, $3)",
        )
        .bind(item_id)
        .bind("mxbai-embed-large-v1")
        .bind(unit_vector(seed))
        .execute(workspace.pool())
        .await?;
    }

    // ANN query: query with seed=1 (same as first insert) should hit it first.
    let query_vec = unit_vector(1);
    let top_k: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT memory_item_id FROM memory_embeddings \
         ORDER BY embedding <=> $1 LIMIT 3",
    )
    .bind(query_vec)
    .fetch_all(workspace.pool())
    .await?;
    assert_eq!(top_k.len(), 3, "expected 3 ANN results");
    assert_eq!(
        top_k[0].0, memory_fact_id,
        "nearest neighbor should be seed=1 vector"
    );

    // Negative test: scope_type CHECK blocks invalid value.
    let bad_scope = sqlx::query(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content) \
         VALUES ('invalid_scope', $1, $2, $3, 'X', 'fact', 'bad')",
    )
    .bind(user_id)
    .bind(group_id)
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("scope_type CHECK should reject 'invalid_scope'");
    let db_err = bad_scope.as_database_error().expect("database error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23514"),
        "expected check_violation"
    );

    // Negative test: TTL in the past.
    let bad_ttl = sqlx::query(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content, ttl_expires_at) \
         VALUES ('user', $1, NULL, $2, 'X', 'fact', 'expired', now() - interval '1 day')",
    )
    .bind(user_id)
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("TTL in past should be rejected");
    let ttl_err = bad_ttl.as_database_error().expect("database-layer error");
    assert_eq!(
        ttl_err.code().as_deref(),
        Some("23514"),
        "expected check_violation for past TTL"
    );

    // Negative test: wrong vector dimension. vector(768) must reject a
    // 512-element vector. pgvector returns a DB-layer error (the exact
    // SQLSTATE varies by pgvector version — typically 22000 data_exception
    // or 22P02 invalid_text_representation). We assert it's a DB error
    // from the bind path, not a success.
    let wrong_dim = sqlx::query(
        "INSERT INTO memory_embeddings (memory_item_id, model, embedding) \
         VALUES ($1, $2, $3)",
    )
    .bind(memory_fact_id)
    .bind("wrong-dim-model")
    .bind(vector_of_dim(512))
    .execute(workspace.pool())
    .await
    .expect_err("vector(768) must reject a 512-dim embedding");
    assert!(
        wrong_dim.as_database_error().is_some(),
        "wrong-dim rejection must be a DB-layer error, got: {wrong_dim:?}"
    );

    // ─── Migration 007 validation (RLS FORCE on 10 tenant-scoped tables) ──
    //
    // Strategy: use `SET LOCAL ROLE garraia_app` to demote the superuser
    // connection to a non-owner role, then `SET LOCAL app.current_group_id`
    // and `SET LOCAL app.current_user_id` to establish the request scope.
    // Mirrors the B6 benchmark pattern in benches/database-poc/.

    // Helper: opens a transaction, demotes to garraia_app role, and
    // conditionally sets both app.current_group_id and app.current_user_id
    // for the duration of the transaction. Returns the transaction so the
    // caller can run queries and then rollback/commit.
    async fn rls_scope(
        pool: &sqlx::PgPool,
        group_id: Option<uuid::Uuid>,
        user_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL ROLE garraia_app")
            .execute(&mut *tx)
            .await?;
        if let Some(gid) = group_id {
            // Dynamic SET LOCAL via format! is intentional: SET LOCAL does
            // not support parameter binding, and the value is a typed
            // uuid::Uuid that sqlx already validated — no user input flows
            // through this path. Safe by construction.
            let stmt = format!("SET LOCAL app.current_group_id = '{gid}'");
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
        if let Some(uid) = user_id {
            let stmt = format!("SET LOCAL app.current_user_id = '{uid}'");
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
        Ok(tx)
    }

    // Metadata check: all 21 tables must have relforcerowsecurity = true.
    // 10 from migration 007 + 8 from migration 006 (tasks Tier 1) +
    // 3 from migration 003 (files/folders, GAR-387).
    // Complements Cenário 4 (empirical FORCE proof) with a direct pg_class
    // query — two orthogonal evidence paths.
    let forced_tables: Vec<(String,)> = sqlx::query_as(
        "SELECT relname::text FROM pg_class \
         WHERE relforcerowsecurity = true \
         AND relname IN ('messages','chats','chat_members','message_threads',\
                         'memory_items','memory_embeddings','audit_events',\
                         'sessions','api_keys','user_identities',\
                         'task_lists','tasks','task_assignees','task_labels',\
                         'task_label_assignments','task_comments',\
                         'task_subscriptions','task_activity',\
                         'folders','files','file_versions') \
         ORDER BY relname",
    )
    .fetch_all(workspace.pool())
    .await?;
    assert_eq!(
        forced_tables.len(),
        21,
        "expected all 21 tenant-scoped tables to have FORCE RLS (10 from migration 007 + 8 from migration 006 + 3 from migration 003), got: {forced_tables:?}"
    );

    // Migration 006 tables exist.
    for expected in &[
        "task_lists",
        "tasks",
        "task_assignees",
        "task_labels",
        "task_label_assignments",
        "task_comments",
        "task_subscriptions",
        "task_activity",
    ] {
        assert!(
            names.contains(expected),
            "missing table from migration 006: {expected}"
        );
    }

    // Migration 006 critical indexes.
    for expected in &[
        "tasks_list_status_idx",
        "tasks_group_status_idx",
        "tasks_due_idx",
        "tasks_parent_idx",
        "tasks_completed_idx",
        "task_lists_group_idx",
        "task_assignees_user_idx",
        "task_labels_group_idx",
        "task_label_assignments_label_idx",
        "task_comments_task_created_idx",
        "task_subscriptions_user_idx",
        "task_activity_task_created_idx",
        "task_activity_group_created_idx",
        "task_activity_kind_idx",
    ] {
        assert!(
            index_names.contains(expected),
            "missing index from migration 006: {expected}"
        );
    }

    // ── Cross-group fixtures (shared by scenarios 2, 5, 6, 7, 8) ──────────
    //
    // `other_group_id` already exists from the compound-FK test above
    // (type='team', owned by user_id). Reuse it as our "group B". We still
    // need: a second user (user_b), a chat in group B, a message in that
    // chat, a chat_member row in that chat, a personal memory owned by
    // user_b, an embedding for that memory, and 3 audit rows. All inserted
    // via the superuser pool — bypassing RLS for setup is intentional
    // (tests verify the policy, not the setup path).

    let user_b_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id")
            .bind("rls-user-b@example.com")
            .bind("RLS User B")
            .fetch_one(workspace.pool())
            .await?;

    let other_chat_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO chats (group_id, type, name, created_by) \
         VALUES ($1, 'channel', 'other-geral', $2) RETURNING id",
    )
    .bind(other_group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let other_message_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(other_chat_id)
    .bind(other_group_id)
    .bind(user_id)
    .bind("Test User")
    .bind("secret cross-group message")
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(other_chat_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    // Personal memory owned by user_b (scope_type=user, group_id=NULL).
    let other_user_memory_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
         created_by_label, kind, content) \
         VALUES ('user', $1, NULL, $1, 'RLS User B', 'preference', \
                 'user_b personal memory — must not leak') \
         RETURNING id",
    )
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query(
        "INSERT INTO memory_embeddings (memory_item_id, model, embedding) \
         VALUES ($1, $2, $3)",
    )
    .bind(other_user_memory_id)
    .bind("mxbai-embed-large-v1")
    .bind(unit_vector(999))
    .execute(workspace.pool())
    .await?;

    // 3 audit rows for scenario 8, all tagged with a distinctive action so
    // we can filter away the 2 audit rows inserted earlier by migration 002
    // validation block.
    // Row A: group-scoped, actor=user_A → visible under group branch.
    sqlx::query(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, \
         resource_type, resource_id) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(group_id)
    .bind(user_id)
    .bind("Test User")
    .bind("smoke_test_rls_audit")
    .bind("group")
    .bind(group_id.to_string())
    .execute(workspace.pool())
    .await?;
    // Row B: personal, actor=user_A → visible under user branch.
    sqlx::query(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, \
         resource_type, resource_id) VALUES (NULL, $1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind("Test User")
    .bind("smoke_test_rls_audit")
    .bind("user")
    .bind(user_id.to_string())
    .execute(workspace.pool())
    .await?;
    // Row C: personal, actor=user_b → NOT visible to user_A.
    sqlx::query(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, \
         resource_type, resource_id) VALUES (NULL, $1, $2, $3, $4, $5)",
    )
    .bind(user_b_id)
    .bind("RLS User B")
    .bind("smoke_test_rls_audit")
    .bind("user")
    .bind(user_b_id.to_string())
    .execute(workspace.pool())
    .await?;

    // ── Cenário 1 — Positive read ─────────────────────────────────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            count, 3,
            "cenário 1: positive read should see all 3 messages from migration 004"
        );
        tx.rollback().await?;
    }

    // ── Cenário 2 — Cross-group read blocked ──────────────────────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
            .bind(other_message_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            leaked, 0,
            "cenário 2: cross-group message must not be visible"
        );
        tx.rollback().await?;
    }

    // ── Cenário 3 — Unset settings fail-closed (ALL 10 tables) ─────────────
    //
    // Stronger than plan §7.2: we assert fail-closed on every one of the 10
    // RLS-protected tables, not just `messages`. A silent regression that
    // breaks one table's NULLIF would now surface immediately.
    {
        let mut tx = rls_scope(workspace.pool(), None, None).await?;
        // All 21 tenant-scoped tables must fail closed when app.current_*_id
        // is unset: 10 from migration 007 + 8 from migration 006 +
        // 3 from migration 003 (GAR-387 files/folders).
        // JOIN-class tables (chat_members, message_threads, task_assignees,
        // task_label_assignments, task_comments, task_subscriptions,
        // memory_embeddings) are implicitly covered via their recursive
        // subquery against tables that already fail closed, but we assert
        // them explicitly to catch any future regression where a policy
        // forgets the NULLIF wrapper or anchors on an unprotected column.
        //
        // format!("SELECT count(*) FROM {table}") is intentional and safe:
        // `table` comes from a hardcoded compile-time &[&str], NOT user
        // input. Same safety argument as the SET LOCAL format! in rls_scope.
        for table in &[
            // migration 007 (10)
            "messages",
            "chats",
            "chat_members",
            "message_threads",
            "memory_items",
            "memory_embeddings",
            "audit_events",
            "sessions",
            "api_keys",
            "user_identities",
            // migration 006 (8)
            "task_lists",
            "tasks",
            "task_assignees",
            "task_labels",
            "task_label_assignments",
            "task_comments",
            "task_subscriptions",
            "task_activity",
            // migration 003 (3)
            "folders",
            "files",
            "file_versions",
        ] {
            let sql = format!("SELECT count(*) FROM {table}");
            let count: i64 = sqlx::query_scalar(&sql).fetch_one(&mut *tx).await?;
            assert_eq!(
                count, 0,
                "cenário 3: unset settings must yield 0 rows on `{table}` (fail-closed)"
            );
        }
        tx.rollback().await?;
    }

    // ── Cenário 4 — FORCE RLS vs table owner ─────────────────────────────
    //
    // FORCE RLS guarantees that a table OWNER is still subject to policies.
    // It does NOT bypass the separate BYPASSRLS / SUPERUSER attribute — the
    // testcontainer 'postgres' user is a superuser, so it bypasses RLS for
    // that reason, independent of FORCE. To prove FORCE empirically, we
    // create a dedicated non-superuser role, transfer ownership of the
    // `messages` table to it, demote via SET LOCAL ROLE, and observe that
    // policies still apply.
    //
    // Ownership restoration is guarded by scopeguard::defer_lifetime_on_drop
    // so a panic/early-return inside the scenario block does NOT leave the
    // table owned by the test role. Test-correctness requirement per
    // @security-auditor H2 and @code-reviewer S1.
    sqlx::query(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_force_test_owner') THEN \
             CREATE ROLE garraia_force_test_owner NOLOGIN; \
           END IF; \
         END $$",
    )
    .execute(workspace.pool())
    .await?;
    sqlx::query("ALTER TABLE messages OWNER TO garraia_force_test_owner")
        .execute(workspace.pool())
        .await?;
    // Panic-safe ownership restore. scopeguard runs the closure when the
    // surrounding scope ends, whether via normal completion, ? propagation,
    // or panic. Uses std::thread blocking on the pool via a fresh async
    // runtime handle since Drop cannot be async — we use a sync SQL path
    // through the existing tokio runtime via `tokio::task::block_in_place`
    // + `Handle::current().block_on`. This is test-only code and the
    // blocking pattern is acceptable for teardown correctness.
    let pool_for_restore = workspace.pool().clone();
    let _restore_guard = scopeguard::guard((), move |_| {
        let pool = pool_for_restore.clone();
        // Best-effort restore. If the runtime is already torn down (rare),
        // the ownership stays with the test role until testcontainer drop —
        // acceptable fallback because the container is ephemeral.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("restore runtime");
            rt.block_on(async move {
                let _ = sqlx::query("ALTER TABLE messages OWNER TO postgres")
                    .execute(&pool)
                    .await;
            });
        })
        .join();
    });
    {
        let mut tx = workspace.pool().begin().await?;
        sqlx::query("SET LOCAL ROLE garraia_force_test_owner")
            .execute(&mut *tx)
            .await?;
        // Now we ARE the table owner, and NOT a superuser. FORCE RLS must
        // still apply. No SET LOCAL app.current_group_id → fail-closed.
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages")
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            count, 0,
            "cenário 4: FORCE RLS must block the non-superuser table owner when app.current_group_id is unset"
        );
        tx.rollback().await?;
    }
    // Restore ownership explicitly on the happy path. The scopeguard remains
    // as the panic-safety net.
    drop(_restore_guard);

    // ── Cenário 5 — chat_members JOIN policy ──────────────────────────────
    //
    // Setup (bypass) inserted exactly 1 chat_members row for `chat_id`
    // (migration 004 test block). Assert exact count, not loose lower bound —
    // a future fixture that accidentally adds a row must surface here.
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let own_members: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_members WHERE chat_id = $1")
                .bind(chat_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(
            own_members, 1,
            "cenário 5: should see exactly 1 chat member row for own chat"
        );
        let other_members: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_members WHERE chat_id = $1")
                .bind(other_chat_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(
            other_members, 0,
            "cenário 5: JOIN policy must block cross-group chat_members"
        );
        tx.rollback().await?;
    }

    // ── Cenário 6 — memory_items user-scope isolation (LGPD-critical) ────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let visible_personal_other: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memory_items \
             WHERE scope_type = 'user' AND created_by = $1",
        )
        .bind(user_b_id)
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(
            visible_personal_other, 0,
            "cenário 6: personal memory of another user must not leak"
        );
        tx.rollback().await?;
    }

    // ── Cenário 7 — memory_embeddings via recursive JOIN ──────────────────
    //
    // Count-based assertion (not negative-membership over top-k). The prior
    // form asserted `!hits.any(|id| id == other_user_memory_id)` under
    // LIMIT 10, which can pass vacuously if the cross-user embedding happens
    // to be outside the top-10 window — proving nothing about RLS. The
    // correct proof is: under the current scope, a DIRECT count of the
    // cross-user embedding row must be 0. This bypasses ANN ranking and
    // exercises only the RLS composition (memory_embeddings subquery →
    // memory_items RLS → user-scope branch).
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let leaked: i64 =
            sqlx::query_scalar("SELECT count(*) FROM memory_embeddings WHERE memory_item_id = $1")
                .bind(other_user_memory_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(
            leaked, 0,
            "cenário 7: memory_embeddings RLS must block cross-user embedding via recursive JOIN to memory_items"
        );
        tx.rollback().await?;
    }

    // ── Cenário 8 — audit_events dual policy ──────────────────────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let visible: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_events WHERE action = 'smoke_test_rls_audit'",
        )
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(
            visible, 2,
            "cenário 8: audit_events dual policy: group+self visible (2 rows), other user NOT"
        );
        tx.rollback().await?;
    }

    // ─── Migration 006 validation (Tasks Tier 1 + RLS FORCE) ──────────────
    //
    // Wave 1 fixtures for GAR-390. All setup goes through the superuser pool
    // (bypasses RLS) because we are testing the policies, not the setup path.

    // ── Fixture: task_list + parent task + 2 subtasks in group A ──────────
    let test_list_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_lists (group_id, name, type, created_by, created_by_label) \
         VALUES ($1, 'Sprint Alpha', 'board', $2, 'Test User') RETURNING id",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let parent_task_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, 'Parent task', $3, 'Test User') RETURNING id",
    )
    .bind(test_list_id)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let child1_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (list_id, group_id, parent_task_id, title, created_by, created_by_label) \
         VALUES ($1, $2, $3, 'Subtask 1', $4, 'Test User') RETURNING id",
    )
    .bind(test_list_id)
    .bind(group_id)
    .bind(parent_task_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let child2_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (list_id, group_id, parent_task_id, title, created_by, created_by_label) \
         VALUES ($1, $2, $3, 'Subtask 2', $4, 'Test User') RETURNING id",
    )
    .bind(test_list_id)
    .bind(group_id)
    .bind(parent_task_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    // ── Subtask cascade test (soft delete preserves, hard delete cascades) ─
    sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1")
        .bind(parent_task_id)
        .execute(workspace.pool())
        .await?;
    let surviving_soft: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tasks WHERE parent_task_id = $1")
            .bind(parent_task_id)
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(
        surviving_soft, 2,
        "migration 006: soft delete must not cascade to subtasks"
    );

    sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(parent_task_id)
        .execute(workspace.pool())
        .await?;
    let remaining_hard: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE id IN ($1, $2)")
        .bind(child1_id)
        .bind(child2_id)
        .fetch_one(workspace.pool())
        .await?;
    assert_eq!(
        remaining_hard, 0,
        "migration 006: hard delete must cascade via ON DELETE CASCADE to subtasks"
    );

    // ── Compound FK negative test: list from group A + group_id from B ────
    let bad_compound = sqlx::query(
        "INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, 'cross-group', $3, 'Test User')",
    )
    .bind(test_list_id) // belongs to group A
    .bind(other_group_id) // but we claim group B
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 006: compound FK must block cross-group drift");
    let db_err = bad_compound
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23503"),
        "expected SQLSTATE 23503 (foreign_key_violation) for compound FK drift"
    );

    // ── Enum CHECK negative test ──────────────────────────────────────────
    let bad_status = sqlx::query(
        "INSERT INTO tasks (list_id, group_id, title, status, created_by, created_by_label) \
         VALUES ($1, $2, 'x', 'invalid_status', $3, 'Test User')",
    )
    .bind(test_list_id)
    .bind(group_id)
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 006: invalid status must be rejected by CHECK");
    let db_err = bad_status
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23514"),
        "expected SQLSTATE 23514 (check_violation) for invalid status enum"
    );

    // ── Full positive fixture in group A (post-cascade, fresh) ────────────
    let positive_task_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, 'Positive task', $3, 'Test User') RETURNING id",
    )
    .bind(test_list_id)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_assignees (task_id, user_id, assigned_by) VALUES ($1, $2, $3)")
        .bind(positive_task_id)
        .bind(user_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    let positive_label_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_labels (group_id, name, color, created_by, created_by_label) \
         VALUES ($1, 'urgent', '#ff0000', $2, 'Test User') RETURNING id",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_label_assignments (task_id, label_id) VALUES ($1, $2)")
        .bind(positive_task_id)
        .bind(positive_label_id)
        .execute(workspace.pool())
        .await?;

    let positive_comment_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_comments (task_id, author_user_id, author_label, body_md) \
         VALUES ($1, $2, 'Test User', 'looks good') RETURNING id",
    )
    .bind(positive_task_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_subscriptions (task_id, user_id) VALUES ($1, $2)")
        .bind(positive_task_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    let positive_activity_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_activity (task_id, group_id, actor_user_id, actor_label, kind, payload) \
         VALUES ($1, $2, $3, 'Test User', 'created', '{}'::jsonb) RETURNING id",
    )
    .bind(positive_task_id)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    // ── Cross-group fixture (group B, bypass) ─────────────────────────────
    let other_list_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_lists (group_id, name, type, created_by, created_by_label) \
         VALUES ($1, 'Other board', 'board', $2, 'RLS User B') RETURNING id",
    )
    .bind(other_group_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    let other_task_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, 'Cross-group task', $3, 'Test User') RETURNING id",
    )
    .bind(other_list_id)
    .bind(other_group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_assignees (task_id, user_id, assigned_by) VALUES ($1, $2, $3)")
        .bind(other_task_id)
        .bind(user_b_id)
        .bind(user_id)
        .execute(workspace.pool())
        .await?;

    let other_label_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_labels (group_id, name, color, created_by, created_by_label) \
         VALUES ($1, 'secret', '#00ff00', $2, 'RLS User B') RETURNING id",
    )
    .bind(other_group_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_label_assignments (task_id, label_id) VALUES ($1, $2)")
        .bind(other_task_id)
        .bind(other_label_id)
        .execute(workspace.pool())
        .await?;

    let other_comment_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_comments (task_id, author_user_id, author_label, body_md) \
         VALUES ($1, $2, 'RLS User B', 'secret cross-group comment') RETURNING id",
    )
    .bind(other_task_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query("INSERT INTO task_subscriptions (task_id, user_id) VALUES ($1, $2)")
        .bind(other_task_id)
        .bind(user_b_id)
        .execute(workspace.pool())
        .await?;

    let other_activity_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO task_activity (task_id, group_id, actor_user_id, actor_label, kind, payload) \
         VALUES ($1, $2, $3, 'RLS User B', 'created', '{}'::jsonb) RETURNING id",
    )
    .bind(other_task_id)
    .bind(other_group_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    // ── Cenário 9 — RLS positive read across all 8 task tables ────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;

        let tl: i64 = sqlx::query_scalar("SELECT count(*) FROM task_lists WHERE id = $1")
            .bind(test_list_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tl, 1, "cenário 9: own task_list visible");

        let t: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE id = $1")
            .bind(positive_task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(t, 1, "cenário 9: own task visible");

        let ta: i64 = sqlx::query_scalar("SELECT count(*) FROM task_assignees WHERE task_id = $1")
            .bind(positive_task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(ta, 1, "cenário 9: own task_assignees visible");

        let tlab: i64 = sqlx::query_scalar("SELECT count(*) FROM task_labels WHERE id = $1")
            .bind(positive_label_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tlab, 1, "cenário 9: own task_labels visible");

        let tla: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_label_assignments WHERE task_id = $1")
                .bind(positive_task_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(tla, 1, "cenário 9: own task_label_assignments visible");

        let tc: i64 = sqlx::query_scalar("SELECT count(*) FROM task_comments WHERE id = $1")
            .bind(positive_comment_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tc, 1, "cenário 9: own task_comments visible");

        let ts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_subscriptions WHERE task_id = $1")
                .bind(positive_task_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(ts, 1, "cenário 9: own task_subscriptions visible");

        let tact: i64 = sqlx::query_scalar("SELECT count(*) FROM task_activity WHERE id = $1")
            .bind(positive_activity_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tact, 1, "cenário 9: own task_activity visible");

        tx.rollback().await?;
    }

    // ── Cenário 10 — RLS cross-group blocked across all 8 task tables ─────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;

        let tl: i64 = sqlx::query_scalar("SELECT count(*) FROM task_lists WHERE id = $1")
            .bind(other_list_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tl, 0, "cenário 10: cross-group task_list must not leak");

        let t: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE id = $1")
            .bind(other_task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(t, 0, "cenário 10: cross-group task must not leak");

        let ta: i64 = sqlx::query_scalar("SELECT count(*) FROM task_assignees WHERE task_id = $1")
            .bind(other_task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            ta, 0,
            "cenário 10: cross-group task_assignees must not leak (JOIN policy)"
        );

        let tlab: i64 = sqlx::query_scalar("SELECT count(*) FROM task_labels WHERE id = $1")
            .bind(other_label_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(tlab, 0, "cenário 10: cross-group task_labels must not leak");

        let tla: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_label_assignments WHERE task_id = $1")
                .bind(other_task_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(
            tla, 0,
            "cenário 10: cross-group task_label_assignments must not leak (JOIN policy)"
        );

        let tc: i64 = sqlx::query_scalar("SELECT count(*) FROM task_comments WHERE id = $1")
            .bind(other_comment_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            tc, 0,
            "cenário 10: cross-group task_comments must not leak (JOIN policy)"
        );

        let ts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_subscriptions WHERE task_id = $1")
                .bind(other_task_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(
            ts, 0,
            "cenário 10: cross-group task_subscriptions must not leak (JOIN policy)"
        );

        let tact: i64 = sqlx::query_scalar("SELECT count(*) FROM task_activity WHERE id = $1")
            .bind(other_activity_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(
            tact, 0,
            "cenário 10: cross-group task_activity must not leak"
        );

        tx.rollback().await?;
    }

    // ── Migration 008 validation ────────────────────────────────────────────
    //
    // garraia_login NOLOGIN BYPASSRLS dedicated role for the garraia-auth
    // login flow. See ADR 0005 §"Login role specification" and migration
    // 008_login_role.sql.

    let login_role: (bool, bool) = sqlx::query_as(
        "SELECT rolbypassrls, rolcanlogin FROM pg_roles WHERE rolname = 'garraia_login'",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert!(login_role.0, "garraia_login must have BYPASSRLS attribute");
    assert!(!login_role.1, "garraia_login must be NOLOGIN by default");

    // Positive grants — every privilege from ADR 0005 §"Login role specification".
    for (table, privs) in &[
        ("user_identities", "SELECT, UPDATE"),
        ("users", "SELECT"),
        ("sessions", "INSERT, UPDATE"),
        ("audit_events", "INSERT"),
    ] {
        let granted: bool =
            sqlx::query_scalar("SELECT has_table_privilege('garraia_login', $1, $2)")
                .bind(table)
                .bind(privs)
                .fetch_one(workspace.pool())
                .await?;
        assert!(granted, "garraia_login must have {privs} on {table}");
    }

    // Negative grants — login role MUST NOT have access to anything beyond
    // the four ADR 0005 §"Login role specification" tables. Coverage spans:
    //   - chat / memory / tasks (default-privileges leak from migration 007)
    //   - api_keys (separate auth surface — would broaden the credential blast radius)
    //   - roles / permissions / role_permissions (RBAC config, public lookup only)
    //   - groups / group_members / group_invites (tenant management)
    // GAR-391a security review H-2.
    for table in &[
        "messages",
        "memory_items",
        "tasks",
        "api_keys",
        "roles",
        "permissions",
        "role_permissions",
        "groups",
        // `group_members` REMOVED from this negative matrix in 391c —
        // migration 010 grants SELECT to garraia_login (Gap C fix). The
        // positive assertion lives in the migration 010 block below.
        "group_invites",
    ] {
        let leaked: bool =
            sqlx::query_scalar("SELECT has_table_privilege('garraia_login', $1, 'SELECT')")
                .bind(table)
                .fetch_one(workspace.pool())
                .await?;
        assert!(
            !leaked,
            "garraia_login MUST NOT have SELECT on {table} (privilege leak)"
        );
    }

    // Negative sequence assertion — no `GRANT USAGE ON ALL SEQUENCES` was
    // issued by migration 008, so `garraia_login` must not hold USAGE on any
    // sequence currently in the public schema. If a future migration needs
    // sequence access for the login role, it must do so on a per-sequence
    // basis with an accompanying security review (GAR-391a security review
    // C-1 + H-2).
    // Use a CTE so Postgres applies the relkind/namespace filter BEFORE
    // calling has_sequence_privilege — otherwise the planner may invoke the
    // privilege check on TOAST-related pg_class rows that look like
    // sequences from a different angle and fail with "X is not a sequence".
    // Use information_schema.usage_privileges directly — the standard view
    // is already filtered to grantable objects per role, so we don't have
    // to call has_sequence_privilege on raw pg_class rows (which can trip
    // on TOAST entries that the planner evaluates eagerly).
    let leaked_sequences: Vec<(String,)> = sqlx::query_as(
        "SELECT object_name::text \
         FROM information_schema.usage_privileges \
         WHERE grantee = 'garraia_login' \
           AND object_schema = 'public' \
           AND object_type = 'SEQUENCE'",
    )
    .fetch_all(workspace.pool())
    .await?;
    assert!(
        leaked_sequences.is_empty(),
        "garraia_login must not hold USAGE on any sequence (leaked: {leaked_sequences:?})"
    );

    // ── Migration 009 validation ────────────────────────────────────────────
    //
    // user_identities.hash_upgraded_at was added by migration 009 to support
    // the lazy upgrade path in garraia-auth::InternalProvider::verify_credential
    // (GAR-391b). See plan 0011.5.

    let hash_upgraded_at: (String, String, Option<String>) = sqlx::query_as(
        "SELECT data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = 'public' \
           AND table_name = 'user_identities' \
           AND column_name = 'hash_upgraded_at'",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert_eq!(hash_upgraded_at.0, "timestamp with time zone");
    assert_eq!(
        hash_upgraded_at.1, "YES",
        "hash_upgraded_at must be nullable"
    );
    assert!(
        hash_upgraded_at.2.is_none(),
        "hash_upgraded_at must NOT have a default (NULL is the only initial state)"
    );

    // Defensive regression: password_hash still exists with the original shape.
    let password_hash_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'user_identities' \
               AND column_name = 'password_hash' \
         )",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert!(password_hash_exists);

    // ── Migration 010 validation ────────────────────────────────────────────
    //
    // garraia_signup NOLOGIN BYPASSRLS dedicated role for the garraia-auth
    // signup flow + new SELECT on sessions for garraia_login (closes Gap A
    // from GAR-391b). See plan 0012 §3.1 and migration
    // 010_signup_role_and_session_select.sql.

    let signup_role: (bool, bool) = sqlx::query_as(
        "SELECT rolbypassrls, rolcanlogin FROM pg_roles WHERE rolname = 'garraia_signup'",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert!(
        signup_role.0,
        "garraia_signup must have BYPASSRLS attribute"
    );
    assert!(!signup_role.1, "garraia_signup must be NOLOGIN by default");

    // Positive grants — every privilege from plan 0012 §3.1.
    for (table, privs) in &[
        ("users", "SELECT, INSERT"),
        ("user_identities", "SELECT, INSERT"),
        ("audit_events", "INSERT"),
    ] {
        let granted: bool =
            sqlx::query_scalar("SELECT has_table_privilege('garraia_signup', $1, $2)")
                .bind(table)
                .bind(privs)
                .fetch_one(workspace.pool())
                .await?;
        assert!(granted, "garraia_signup must have {privs} on {table}");
    }

    // Negative grants — signup role MUST NOT have access to any tenant data
    // or session state. Narrower than the login role's blast radius.
    for table in &["sessions", "messages", "memory_items", "tasks", "groups"] {
        let leaked: bool =
            sqlx::query_scalar("SELECT has_table_privilege('garraia_signup', $1, 'SELECT')")
                .bind(table)
                .fetch_one(workspace.pool())
                .await?;
        assert!(
            !leaked,
            "garraia_signup MUST NOT have SELECT on {table} (privilege leak)"
        );
    }

    // New positive grant for garraia_login on sessions (Gap A fix from 391b).
    // INSERT ... RETURNING id + verify_refresh both require SELECT on sessions.
    let login_sessions_select: bool =
        sqlx::query_scalar("SELECT has_table_privilege('garraia_login', 'sessions', 'SELECT')")
            .fetch_one(workspace.pool())
            .await?;
    assert!(
        login_sessions_select,
        "garraia_login must have SELECT on sessions (Gap A fix, migration 010)"
    );

    // New positive grant for garraia_login on group_members (Gap C fix from 391c).
    // The Principal extractor needs to resolve membership via
    // `SELECT role FROM group_members WHERE group_id=$1 AND user_id=$2 AND status='active'`.
    let login_members_select: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('garraia_login', 'group_members', 'SELECT')",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert!(
        login_members_select,
        "garraia_login must have SELECT on group_members (Gap C fix, migration 010)"
    );

    // Negative regression: garraia_signup MUST NOT gain SELECT on group_members.
    // The signup pool's whole purpose is creating a new identity; reading
    // tenant membership is out of scope.
    let signup_members_leaked: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('garraia_signup', 'group_members', 'SELECT')",
    )
    .fetch_one(workspace.pool())
    .await?;
    assert!(
        !signup_members_leaked,
        "garraia_signup MUST NOT have SELECT on group_members"
    );

    // ─── Migration 003 validation (GAR-387: folders, files, file_versions) ──
    //
    // Three new tenant-scoped tables under FORCE RLS with direct policies on
    // a denormalized group_id column. Compound FK (folder_id, group_id) and
    // (file_id, group_id) prevent cross-tenant drift at the DB layer.

    // ── 003.1: tables exist ────────────────────────────────────────────────
    for expected in &["folders", "files", "file_versions"] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = $1 \
             )",
        )
        .bind(expected)
        .fetch_one(workspace.pool())
        .await?;
        assert!(exists, "missing table from migration 003: {expected}");
    }

    // ── 003.2: critical indexes exist ──────────────────────────────────────
    let m003_indexes: Vec<(String,)> =
        sqlx::query_as("SELECT indexname FROM pg_indexes WHERE schemaname = 'public'")
            .fetch_all(workspace.pool())
            .await?;
    let m003_index_names: Vec<&str> = m003_indexes.iter().map(|(n,)| n.as_str()).collect();
    for expected in &[
        "folders_group_parent_idx",
        "folders_parent_idx",
        "folders_unique_name_per_parent_idx",
        "files_group_folder_idx",
        "files_group_created_idx",
        "files_folder_idx",
        "file_versions_file_idx",
        "file_versions_group_created_idx",
    ] {
        assert!(
            m003_index_names.contains(expected),
            "missing index from migration 003: {expected}"
        );
    }

    // ── 003.3: policies exist AND have explicit WITH CHECK (migration 013 pattern)
    //
    // Any policy with qual <> with_check would be a regression: without
    // explicit WITH CHECK, a future conversion to AS RESTRICTIVE makes the
    // write-guard silently TRUE. Assert the two predicates are identical
    // for each of the 3 new policies.
    let policy_rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT tablename::text, policyname::text, qual::text, with_check::text \
         FROM pg_policies \
         WHERE schemaname = 'public' \
           AND tablename IN ('folders','files','file_versions') \
         ORDER BY tablename, policyname",
    )
    .fetch_all(workspace.pool())
    .await?;
    assert_eq!(
        policy_rows.len(),
        3,
        "expected exactly 3 policies on migration 003 tables (one per table), got: {policy_rows:?}"
    );
    for (table, policy, qual, with_check) in &policy_rows {
        assert!(
            qual.is_some() && with_check.is_some(),
            "migration 003 policy {policy} on {table} must have both USING and WITH CHECK"
        );
        assert_eq!(
            qual, with_check,
            "migration 003 policy {policy} on {table} must have identical USING and WITH CHECK (migration 013 pattern); qual={qual:?}, with_check={with_check:?}"
        );
    }

    // ── 003.4: garraia_app grants ──────────────────────────────────────────
    for table in &["folders", "files", "file_versions"] {
        let granted: bool = sqlx::query_scalar(
            "SELECT has_table_privilege('garraia_app', $1, 'SELECT, INSERT, UPDATE, DELETE')",
        )
        .bind(table)
        .fetch_one(workspace.pool())
        .await?;
        assert!(
            granted,
            "garraia_app must have SELECT/INSERT/UPDATE/DELETE on {table}"
        );
    }

    // ── 003.4b: garraia_login and garraia_signup MUST NOT have access ──────
    //
    // Regression guard — neither role needs to touch file metadata.
    // Migration 003 only grants to garraia_app. Any future migration that
    // widens default privileges or accidentally grants to login/signup
    // would surface here. Security review SEC-M-2 / code review I-3.
    for role in &["garraia_login", "garraia_signup"] {
        for table in &["folders", "files", "file_versions"] {
            for priv_ in &["SELECT", "INSERT", "UPDATE", "DELETE"] {
                let leaked: bool = sqlx::query_scalar(
                    "SELECT has_table_privilege($1, $2, $3)",
                )
                .bind(role)
                .bind(table)
                .bind(priv_)
                .fetch_one(workspace.pool())
                .await?;
                assert!(
                    !leaked,
                    "{role} MUST NOT have {priv_} on {table} (privilege leak)"
                );
            }
        }
    }

    // ── 003.5: fixtures in group A and group B (bypass via superuser) ──────
    let folder_a_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO folders (group_id, parent_id, name, created_by, created_by_label) \
         VALUES ($1, NULL, 'docs', $2, 'Test User') RETURNING id",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    let file_a_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO files (group_id, folder_id, name, current_version, total_versions, \
             size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, 'readme.md', 1, 1, 1024, 'text/markdown', $3, 'Test User') \
         RETURNING id",
    )
    .bind(group_id)
    .bind(folder_a_id)
    .bind(user_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query(
        "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
             checksum_sha256, integrity_hmac, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, $4, $5, $6, 1024, 'text/markdown', $7, 'Test User')",
    )
    .bind(file_a_id)
    .bind(group_id)
    .bind(format!("{group_id}/{file_a_id}/v1"))
    .bind("abc123")
    .bind("a".repeat(64)) // 64 lowercase hex chars
    .bind("b".repeat(64))
    .bind(user_id)
    .execute(workspace.pool())
    .await?;

    let folder_b_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO folders (group_id, parent_id, name, created_by, created_by_label) \
         VALUES ($1, NULL, 'other-docs', $2, 'RLS User B') RETURNING id",
    )
    .bind(other_group_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    let file_b_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO files (group_id, folder_id, name, current_version, total_versions, \
             size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, 'secret.pdf', 1, 1, 2048, 'application/pdf', $3, 'RLS User B') \
         RETURNING id",
    )
    .bind(other_group_id)
    .bind(folder_b_id)
    .bind(user_b_id)
    .fetch_one(workspace.pool())
    .await?;

    sqlx::query(
        "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
             checksum_sha256, integrity_hmac, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, $4, $5, $6, 2048, 'application/pdf', $7, 'RLS User B')",
    )
    .bind(file_b_id)
    .bind(other_group_id)
    .bind(format!("{other_group_id}/{file_b_id}/v1"))
    .bind("def456")
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .bind(user_b_id)
    .execute(workspace.pool())
    .await?;

    // ── Cenário 11 — positive read (same group) ────────────────────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let f: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE id = $1")
            .bind(folder_a_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(f, 1, "cenário 11: own folder visible");
        let fl: i64 = sqlx::query_scalar("SELECT count(*) FROM files WHERE id = $1")
            .bind(file_a_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(fl, 1, "cenário 11: own file visible");
        let fv: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM file_versions WHERE file_id = $1 AND version = 1",
        )
        .bind(file_a_id)
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(fv, 1, "cenário 11: own file_version visible");
        tx.rollback().await?;
    }

    // ── Cenário 12 — cross-group blocked ───────────────────────────────────
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let f: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE id = $1")
            .bind(folder_b_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(f, 0, "cenário 12: cross-group folder must not leak");
        let fl: i64 = sqlx::query_scalar("SELECT count(*) FROM files WHERE id = $1")
            .bind(file_b_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_eq!(fl, 0, "cenário 12: cross-group file must not leak");
        let fv: i64 =
            sqlx::query_scalar("SELECT count(*) FROM file_versions WHERE file_id = $1")
                .bind(file_b_id)
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!(fv, 0, "cenário 12: cross-group file_version must not leak");
        tx.rollback().await?;
    }

    // ── Cenário 13 — compound FK drift on files (folder_id in other group) ─
    let drift_file = sqlx::query(
        "INSERT INTO files (group_id, folder_id, name, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 'drift.txt', 10, 'text/plain', $3, 'Test User')",
    )
    .bind(group_id) // claims group A
    .bind(folder_b_id) // but folder lives in group B
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 003: compound FK must block cross-group drift on files");
    let db_err = drift_file.as_database_error().expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23503"),
        "expected SQLSTATE 23503 (foreign_key_violation) for files cross-group drift"
    );

    // ── Cenário 14 — compound FK drift on file_versions ────────────────────
    let drift_version = sqlx::query(
        "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
             checksum_sha256, integrity_hmac, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 2, $3, $4, $5, $6, 10, 'text/plain', $7, 'Test User')",
    )
    .bind(file_a_id) // file lives in group A
    .bind(other_group_id) // claims group B
    .bind(format!("drift/{file_a_id}/v2"))
    .bind("drift")
    .bind("e".repeat(64))
    .bind("f".repeat(64))
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 003: compound FK must block cross-group drift on file_versions");
    let db_err = drift_version
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23503"),
        "expected SQLSTATE 23503 for file_versions cross-group drift"
    );

    // ── Cenário 15 — files.deleted_at (soft delete) does NOT cascade ───────
    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1")
        .bind(file_a_id)
        .execute(workspace.pool())
        .await?;
    let surviving_versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_versions WHERE file_id = $1")
            .bind(file_a_id)
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(
        surviving_versions, 1,
        "migration 003: soft-delete of files must NOT cascade to file_versions (audit invariant)"
    );
    // Restore for subsequent scenarios that still expect this file.
    sqlx::query("UPDATE files SET deleted_at = NULL WHERE id = $1")
        .bind(file_a_id)
        .execute(workspace.pool())
        .await?;

    // ── Cenário 16 — DELETE files cascades to file_versions (hard delete) ──
    // Use file_b_id so we don't destroy fixtures for other scenarios above.
    sqlx::query("DELETE FROM files WHERE id = $1")
        .bind(file_b_id)
        .execute(workspace.pool())
        .await?;
    let remaining_versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_versions WHERE file_id = $1")
            .bind(file_b_id)
            .fetch_one(workspace.pool())
            .await?;
    assert_eq!(
        remaining_versions, 0,
        "migration 003: hard DELETE of files must cascade to file_versions via compound FK ON DELETE CASCADE"
    );

    // ── Cenário 17 — CHECK rejects malformed checksum_sha256 ───────────────
    for bad_checksum in &[
        "A".repeat(64), // uppercase hex (regex is lowercase-only)
        "0".repeat(63), // too short
        "0".repeat(65), // too long
        "z".repeat(64), // not hex
    ] {
        let bad = sqlx::query(
            "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
                 checksum_sha256, integrity_hmac, size_bytes, mime_type, \
                 created_by, created_by_label) \
             VALUES ($1, $2, 99, $3, 'e', $4, $5, 10, 'text/plain', $6, 'Test User')",
        )
        .bind(file_a_id)
        .bind(group_id)
        .bind(format!("bad-checksum/{bad_checksum}"))
        .bind(bad_checksum.as_str())
        .bind("f".repeat(64))
        .bind(user_id)
        .execute(workspace.pool())
        .await
        .expect_err("migration 003: invalid checksum_sha256 must be rejected by CHECK");
        let db_err = bad.as_database_error().expect("database-layer error");
        assert_eq!(
            db_err.code().as_deref(),
            Some("23514"),
            "expected SQLSTATE 23514 (check_violation) for checksum_sha256 = {bad_checksum}"
        );
    }

    // ── Cenário 17b — CHECK rejects malformed integrity_hmac (symmetry) ────
    //
    // The schema uses the same regex for both checksum_sha256 and
    // integrity_hmac (security review SEC-L-1). Exercise the hmac CHECK
    // independently so a future ALTER that desyncs the two constraints
    // would surface here.
    for bad_hmac in &[
        "A".repeat(64), // uppercase
        "0".repeat(63), // short
        "0".repeat(65), // long
        "z".repeat(64), // non-hex
    ] {
        let bad = sqlx::query(
            "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
                 checksum_sha256, integrity_hmac, size_bytes, mime_type, \
                 created_by, created_by_label) \
             VALUES ($1, $2, 99, $3, 'e', $4, $5, 10, 'text/plain', $6, 'Test User')",
        )
        .bind(file_a_id)
        .bind(group_id)
        .bind(format!("bad-hmac/{bad_hmac}"))
        .bind("a".repeat(64)) // valid checksum so only hmac CHECK fires
        .bind(bad_hmac.as_str())
        .bind(user_id)
        .execute(workspace.pool())
        .await
        .expect_err("migration 003: invalid integrity_hmac must be rejected by CHECK");
        let db_err = bad.as_database_error().expect("database-layer error");
        assert_eq!(
            db_err.code().as_deref(),
            Some("23514"),
            "expected SQLSTATE 23514 (check_violation) for integrity_hmac = {bad_hmac}"
        );
    }

    // ── Cenário 18 — object_key UNIQUE collision ───────────────────────────
    // Insert a second version row that reuses the existing v1 object_key of
    // file_a. The PRIMARY KEY (file_id, version) allows a different version
    // number, but the UNIQUE constraint on object_key must still reject it.
    let existing_key: String = sqlx::query_scalar(
        "SELECT object_key FROM file_versions WHERE file_id = $1 AND version = 1",
    )
    .bind(file_a_id)
    .fetch_one(workspace.pool())
    .await?;
    let collision = sqlx::query(
        "INSERT INTO file_versions (file_id, group_id, version, object_key, etag, \
             checksum_sha256, integrity_hmac, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 2, $3, 'e', $4, $5, 10, 'text/plain', $6, 'Test User')",
    )
    .bind(file_a_id)
    .bind(group_id)
    .bind(existing_key.as_str())
    .bind("0".repeat(64))
    .bind("1".repeat(64))
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 003: duplicate object_key must be rejected by UNIQUE");
    let db_err = collision
        .as_database_error()
        .expect("database-layer error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23505"),
        "expected SQLSTATE 23505 (unique_violation) for object_key collision"
    );

    // ── Cenário 18b — RLS WITH CHECK blocks cross-group INSERT ─────────────
    //
    // Migration 013 pattern: WITH CHECK is explicit and identical to USING,
    // so an INSERT from garraia_app scoped to group_A that claims
    // group_id = group_B must fail even though the row would be "visible"
    // to group_B readers. This exercises the write-side of the policy that
    // cenários 11-12 (read-side) leave implicit. Code review S-1.
    {
        let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
        let blocked = sqlx::query(
            "INSERT INTO folders (group_id, parent_id, name, created_by, created_by_label) \
             VALUES ($1, NULL, 'rls-write-block', $2, 'Test User')",
        )
        .bind(other_group_id) // claim group B while scope = group A
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .expect_err("migration 003: RLS WITH CHECK must block cross-group folder INSERT");
        let db_err = blocked
            .as_database_error()
            .expect("database-layer error");
        // 42501 = insufficient_privilege (RLS policy WITH CHECK refused).
        assert_eq!(
            db_err.code().as_deref(),
            Some("42501"),
            "expected SQLSTATE 42501 (RLS WITH CHECK refusal) for cross-group folder INSERT"
        );
        tx.rollback().await?;
    }

    // ── Cenário 19 — size_bytes CHECK boundaries ───────────────────────────
    // Reject negative size.
    let negative = sqlx::query(
        "INSERT INTO files (group_id, folder_id, name, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 'neg.txt', -1, 'text/plain', $3, 'Test User')",
    )
    .bind(group_id)
    .bind(folder_a_id)
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 003: negative size_bytes must be rejected");
    assert_eq!(
        negative
            .as_database_error()
            .and_then(|e| e.code().map(|c| c.into_owned())),
        Some("23514".to_string()),
        "expected CHECK violation for negative size_bytes"
    );
    // Reject size > 5 GiB (5368709120 bytes).
    let oversize = sqlx::query(
        "INSERT INTO files (group_id, folder_id, name, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 'huge.bin', 5368709121, 'application/octet-stream', $3, 'Test User')",
    )
    .bind(group_id)
    .bind(folder_a_id)
    .bind(user_id)
    .execute(workspace.pool())
    .await
    .expect_err("migration 003: oversize size_bytes must be rejected");
    assert_eq!(
        oversize
            .as_database_error()
            .and_then(|e| e.code().map(|c| c.into_owned())),
        Some("23514".to_string()),
        "expected CHECK violation for size_bytes > 5 GiB"
    );

    Ok(())
}
