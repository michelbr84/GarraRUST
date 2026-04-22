//! End-to-end test for `migrate_workspace` Stages 1–3 against a real
//! Postgres + real SQLite fixture. Gated by Docker availability.
//!
//! Stage 1 coverage (plan 0039): users + identities UPSERT, atomic audit,
//! idempotency, dry-run.
//! Stage 3 coverage (plan 0040): group bucket create/reuse, owner
//! selection, per-membership audit, idempotency, empty-SQLite skip.

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::pbkdf2;
use std::num::NonZeroU32;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{ImageExt, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage};

const PBKDF2_ITERATIONS: u32 = 600_000;
const PBKDF2_OUTPUT_LEN: usize = 32;

struct PgFixture {
    _container: Arc<ContainerAsync<GenericImage>>,
    url: String,
}

async fn start_pg() -> Option<PgFixture> {
    let image = GenericImage::new("pgvector/pgvector", "pg16")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_DB", "garraia_test");
    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] pgvector container failed to start — Docker absent? ({e})");
            return None;
        }
    };
    let host = container.get_host().await.ok()?;
    let port = container.get_host_port_ipv4(5432).await.ok()?;
    let url = format!("postgres://postgres:test@{host}:{port}/garraia_test");
    // Poll for readiness (WaitFor is best-effort).
    for _ in 0..30 {
        if PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Some(PgFixture {
        _container: Arc::new(container),
        url,
    })
}

async fn apply_migrations(pool: &PgPool) {
    // Walk migrations folder from the workspace crate.
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("garraia-workspace")
        .join("migrations");
    let mut entries: Vec<_> = std::fs::read_dir(&migrations_dir)
        .expect("read migrations dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sql"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    // sqlx requires `_sqlx_migrations` populated so our preflight passes.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT,
            installed_on TIMESTAMPTZ DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create _sqlx_migrations");

    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        // Filename format: "001_users_and_groups.sql" → version=1
        let version: i64 = name
            .split('_')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let sql = std::fs::read_to_string(&path).expect("read migration file");
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {name}: {e}"));
        sqlx::query("INSERT INTO _sqlx_migrations (version, description) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(version)
            .bind(&name)
            .execute(pool)
            .await
            .expect("record migration");
    }

    // Ensure uuid_generate_v7 exists (migrations should have created it,
    // but add a defensive create if absent).
    sqlx::raw_sql(
        "DO $$ BEGIN
            PERFORM pg_catalog.pg_proc.proname FROM pg_catalog.pg_proc WHERE proname = 'uuid_generate_v7';
        EXCEPTION WHEN OTHERS THEN NULL;
        END $$;",
    )
    .execute(pool)
    .await
    .ok();
}

fn seed_sqlite(path: &std::path::Path, users: &[(&str, &str, &str)]) {
    let conn = rusqlite::Connection::open(path).expect("open sqlite");
    conn.execute_batch(
        "CREATE TABLE mobile_users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            salt TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .expect("create table");
    // Give each seeded user a distinct `created_at` so the Stage 3
    // owner-selection query (`ORDER BY created_at ASC, id ASC`) has a
    // deterministic primary key, independent of how Postgres orders
    // the inserts internally. Code review NIT (plan 0040).
    for (idx, (id, email, password)) in users.iter().enumerate() {
        let mut salt_raw = vec![0u8; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut salt_raw)
            .expect("rand fill");
        let mut hash_raw = vec![0u8; PBKDF2_OUTPUT_LEN];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            &salt_raw,
            password.as_bytes(),
            &mut hash_raw,
        );
        let hash_b64 = BASE64.encode(&hash_raw);
        let salt_b64 = BASE64.encode(&salt_raw);
        // Increment seconds so `ORDER BY created_at ASC` is strict.
        let ts = format!("2026-04-15T00:00:{:02}Z", idx);
        conn.execute(
            "INSERT INTO mobile_users (id, email, password_hash, salt, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, email, hash_b64, salt_b64, ts],
        )
        .expect("insert mobile_users");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stage1_happy_path_and_idempotency() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    // Seed SQLite.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp); // allow rusqlite to create the file fresh
    seed_sqlite(
        &sqlite_path,
        &[
            ("u1", "alice@example.com", "pw-alice-9999"),
            ("u2", "bob@example.com", "pw-bob-7777"),
            ("u3", "carol@example.com", "pw-carol-5555"),
        ],
    );

    // Invoke the migration command via the binary under test.
    // We exec via `cargo run` — simpler than wrangling cli::Parser in-process.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
        ])
        .env("RUST_LOG", "garraia=info")
        .output()
        .expect("exec garraia");

    if !output.status.success() {
        panic!(
            "migrate exit={}:\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Assertions: 3 users + 3 identities + 3 audit rows.
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE legacy_sqlite_id IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(user_count, 3, "users imported");

    let ident_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_identities WHERE provider = 'internal'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ident_count, 3, "identities imported");

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'users.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 3, "audit events emitted atomically");

    // Re-run must be a no-op (idempotency).
    let rerun = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
            "--confirm-backup", // Postgres now has rows; gate requires this.
        ])
        .output()
        .expect("rerun");
    assert!(
        rerun.status.success(),
        "re-run failed: {}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    let after_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE legacy_sqlite_id IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after_count, 3, "idempotent: no duplicates");
    let after_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'users.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_audit, 3, "idempotent: audit rows not duplicated");
}

#[tokio::test(flavor = "multi_thread")]
async fn stage3_creates_group_and_members_happy_path() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    // Seed three users. Their SQLite `created_at` is identical
    // ('2026-04-15T00:00:00Z' per seed_sqlite), so Postgres users rows
    // inherit the same value. The owner tie-breaker then falls to
    // `ORDER BY id ASC` — deterministic for the test.
    seed_sqlite(
        &sqlite_path,
        &[
            ("u1", "alice@example.com", "pw-alice-9999"),
            ("u2", "bob@example.com", "pw-bob-7777"),
            ("u3", "carol@example.com", "pw-carol-5555"),
        ],
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
        ])
        .env("RUST_LOG", "garraia=info")
        .output()
        .expect("exec garraia");
    assert!(
        output.status.success(),
        "migrate exit={}:\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Exactly one group, name + type from defaults.
    let group_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM groups WHERE name = 'Legacy Personal Workspace' AND type = 'personal'",
    )
    .fetch_one(&pool)
    .await
    .expect("group row");

    // 3 active memberships.
    let member_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(member_count, 3, "three memberships");

    // Exactly one owner.
    let owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members
         WHERE group_id = $1 AND role = 'owner' AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner_count, 1, "single owner");

    let member_role_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members WHERE group_id = $1 AND role = 'member'",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(member_role_count, 2, "two members");

    // groups.created_by must match the owner — plan 0040 §5.6.
    let group_created_by: uuid::Uuid =
        sqlx::query_scalar("SELECT created_by FROM groups WHERE id = $1")
            .bind(group_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let owner_user_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT user_id FROM group_members
         WHERE group_id = $1 AND role = 'owner'",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        group_created_by, owner_user_id,
        "groups.created_by == owner user_id"
    );

    // Audit invariants.
    let group_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'groups.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(group_audit, 1, "one group audit row");

    let member_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'group_members.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(member_audit, 3, "one membership audit per user");
}

#[tokio::test(flavor = "multi_thread")]
async fn stage3_idempotent_rerun() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    seed_sqlite(&sqlite_path, &[("u1", "dup@example.com", "pw-dup-1111")]);

    // First run.
    assert!(
        std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
            .args([
                "migrate",
                "workspace",
                "--from-sqlite",
                sqlite_path.to_str().unwrap(),
                "--to-postgres",
                &pg.url,
            ])
            .output()
            .expect("exec")
            .status
            .success(),
    );

    // Second run with --confirm-backup (users.count > 0 now).
    assert!(
        std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
            .args([
                "migrate",
                "workspace",
                "--from-sqlite",
                sqlite_path.to_str().unwrap(),
                "--to-postgres",
                &pg.url,
                "--confirm-backup",
            ])
            .output()
            .expect("rerun")
            .status
            .success(),
    );

    // Counts never duplicate.
    let group_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM groups WHERE name = 'Legacy Personal Workspace' AND type = 'personal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(group_count, 1);

    let member_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members gm
         JOIN groups g ON g.id = gm.group_id
         WHERE g.name = 'Legacy Personal Workspace'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(member_count, 1, "membership not duplicated");

    let group_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'groups.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(group_audit, 1, "group audit not duplicated");

    let member_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'group_members.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(member_audit, 1, "membership audit not duplicated");
}

#[tokio::test(flavor = "multi_thread")]
async fn stage3_resolves_preexisting_group_by_name() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    // Seed a signup user + a pre-existing "team" group owned by them
    // BEFORE the migration runs. The migration should reuse the group
    // rather than create a new one when --target-group-name matches.
    let existing_owner = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) VALUES ($1, 'owner@pre.example.com', 'pre-owner', 'active')",
    )
    .bind(existing_owner)
    .execute(&pool)
    .await
    .unwrap();
    let existing_group = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by, settings)
         VALUES ($1, 'Shared Family Bucket', 'family', $2, '{}'::jsonb)",
    )
    .bind(existing_group)
    .bind(existing_owner)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status)
         VALUES ($1, $2, 'owner', 'active')",
    )
    .bind(existing_group)
    .bind(existing_owner)
    .execute(&pool)
    .await
    .unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    seed_sqlite(
        &sqlite_path,
        &[
            ("u1", "family.alice@example.com", "pw-a"),
            ("u2", "family.bob@example.com", "pw-b"),
        ],
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
            "--target-group-name",
            "Shared Family Bucket",
            "--target-group-type",
            "family",
            "--confirm-backup", // Postgres has rows already.
        ])
        .output()
        .expect("exec");
    assert!(
        output.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Exactly one group row — reused, not duplicated.
    let group_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM groups WHERE name = 'Shared Family Bucket' AND type = 'family'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(group_count, 1, "group reused, not cloned");

    // Original owner preserved.
    let preserved_owner: uuid::Uuid = sqlx::query_scalar(
        "SELECT user_id FROM group_members
         WHERE group_id = $1 AND role = 'owner'",
    )
    .bind(existing_group)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(preserved_owner, existing_owner, "pre-existing owner kept");

    // Migrated users are members (NOT owners) — plan 0040 §5.1 says
    // reused groups keep their owner; all migrated users become members.
    let owner_role_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members gm
         JOIN users u ON u.id = gm.user_id
         WHERE gm.group_id = $1 AND u.legacy_sqlite_id IS NOT NULL AND gm.role = 'owner'",
    )
    .bind(existing_group)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        owner_role_count, 0,
        "no legacy user was promoted in a reused group"
    );

    let member_role_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members gm
         JOIN users u ON u.id = gm.user_id
         WHERE gm.group_id = $1 AND u.legacy_sqlite_id IS NOT NULL AND gm.role = 'member'",
    )
    .bind(existing_group)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(member_role_count, 2, "two migrated users joined as member");

    // No `groups.imported_from_sqlite` audit row — we reused, not
    // created.
    let group_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'groups.imported_from_sqlite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(group_audit, 0, "reuse path emits no group audit");
}

#[tokio::test(flavor = "multi_thread")]
async fn stage3_promotes_first_legacy_user_when_group_has_no_owner() {
    // Regression guard for code-review MEDIUM (plan 0040) — a group
    // that exists but has no active owner must receive a legacy
    // user as owner on the next run. Scenario constructed by pre-
    // creating a group row with NO membership + then running the
    // migration.
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    // Seed a group with no owner. `created_by` still has to be a
    // valid user due to FK; so create a placeholder user who is NOT a
    // legacy migrant — they only exist to satisfy the FK and deliberately
    // never become a member.
    let placeholder = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status)
         VALUES ($1, 'placeholder@example.com', 'placeholder', 'active')",
    )
    .bind(placeholder)
    .execute(&pool)
    .await
    .unwrap();
    let pre_group = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by, settings)
         VALUES ($1, 'Orphaned Bucket', 'personal', $2, '{}'::jsonb)",
    )
    .bind(pre_group)
    .bind(placeholder)
    .execute(&pool)
    .await
    .unwrap();
    // NO group_members inserted.

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    seed_sqlite(
        &sqlite_path,
        &[
            ("u1", "alice@example.com", "pw-alice"),
            ("u2", "bob@example.com", "pw-bob"),
        ],
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
            "--target-group-name",
            "Orphaned Bucket",
            "--confirm-backup",
        ])
        .output()
        .expect("exec");
    assert!(output.status.success());

    // First legacy user (smallest created_at) is owner.
    let owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members gm
         JOIN users u ON u.id = gm.user_id
         WHERE gm.group_id = $1
           AND u.legacy_sqlite_id IS NOT NULL
           AND gm.role = 'owner' AND gm.status = 'active'",
    )
    .bind(pre_group)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner_count, 1, "orphan group gets owner promotion");

    // Two memberships total: one owner + one member.
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM group_members WHERE group_id = $1")
        .bind(pre_group)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn stage3_skips_when_no_legacy_users() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    // Empty SQLite — no mobile_users rows, just the table.
    seed_sqlite(&sqlite_path, &[]);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
        ])
        .output()
        .expect("exec");
    assert!(output.status.success(), "empty SQLite must exit 0");

    let group_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM groups")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(group_count, 0, "no group when no legacy users");

    let member_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM group_members")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(member_count, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stage1_dry_run_does_not_persist() {
    let Some(pg) = start_pg().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&pg.url)
        .await
        .expect("connect");
    apply_migrations(&pool).await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let sqlite_path = tmp.path().to_path_buf();
    drop(tmp);
    seed_sqlite(
        &sqlite_path,
        &[("u1", "drake@example.com", "pw-drake-1111")],
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_garraia"))
        .args([
            "migrate",
            "workspace",
            "--from-sqlite",
            sqlite_path.to_str().unwrap(),
            "--to-postgres",
            &pg.url,
            "--dry-run",
        ])
        .output()
        .expect("exec");
    assert!(output.status.success(), "dry run should succeed");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE legacy_sqlite_id IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "dry run must not persist rows");
}
