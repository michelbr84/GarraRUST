//! End-to-end test for `migrate_workspace` Stage 1 against a real
//! Postgres + real SQLite fixture. Gated by Docker availability.

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
    for (id, email, password) in users {
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
        conn.execute(
            "INSERT INTO mobile_users (id, email, password_hash, salt, created_at) VALUES (?1, ?2, ?3, ?4, '2026-04-15T00:00:00Z')",
            rusqlite::params![id, email, hash_b64, salt_b64],
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
