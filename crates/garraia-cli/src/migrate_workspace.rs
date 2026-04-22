//! `garraia migrate workspace` — SQLite → Postgres migration (Stages 1–3).
//!
//! - Plan 0039 implemented §7.1 (users) + §7.2 (user_identities + atomic
//!   audit) of [plan 0034](../../plans/0034-gar-413-migrate-workspace-spec.md).
//! - Plan 0040 (this slice) adds §7.4 (groups + group_members): the
//!   legacy bucket is auto-created under `--target-group-name` /
//!   `--target-group-type`, the oldest migrated user becomes owner, the
//!   rest become members, and each write emits an atomic audit row in
//!   the same transaction.
//! - Subsequent stages (chats, messages, memory, sessions, api_keys,
//!   audit retrofit) land in follow-up slices.
//!
//! # Security invariants
//!
//! 1. **PHC reassembly correctness.** Legacy `mobile_users.password_hash` is a
//!    raw PBKDF2 digest base64-STANDARD encoded (with `=` padding). The
//!    Postgres `user_identities.password_hash` column expects the PHC string
//!    `$pbkdf2-sha256$i=600000,l=32$<salt-nopad>$<hash-nopad>` accepted by
//!    `garraia_auth::hashing::verify_pbkdf2`. Any drift → migrated users
//!    cannot log in. Guarded by
//!    [`tests::phc_roundtrip_with_legacy_fixture`].
//! 2. **Atomic audit row.** Each `user_identities` INSERT is paired with an
//!    `audit_events` INSERT in the **same transaction** (plan 0034 §7.2
//!    SEC-H-2 / LGPD art. 18). Rollback of the identity drops the audit; no
//!    orphan state.
//! 3. **BYPASSRLS re-check inside tx.** Plan 0034 §6.3 SEC-H-1: the catalog
//!    check runs once pre-flight and once inside the first data tx, on the
//!    same connection. A DBA that revokes the grant between T0 and T1 is
//!    caught.
//! 4. **Confirmation gate.** If Postgres already has any `users` rows and
//!    `--confirm-backup` is absent, abort with exit 78. `--dry-run` bypasses
//!    the gate because it cannot mutate state.
//! 5. **PII redaction.** `postgres_url` (may contain password) is never
//!    logged in the clear; tracing spans use `skip(postgres_url, sqlite_path)`.
//!    PHC strings and raw hash/salt bytes never enter `tracing` output.
//! 6. **Concurrent runs not supported.** Plan 0039 audit F-02: if two
//!    processes invoke this command against the same Postgres at the
//!    same time, the `WHERE NOT EXISTS` idempotency guard in the audit
//!    INSERT can race — both transactions pass the existence check
//!    before either commits, yielding duplicate audit rows. Operators
//!    MUST serialise invocations at the deploy layer (e.g. a single
//!    Kubernetes Job / systemd-run). Future slice may add a
//!    migration advisory lock via `pg_try_advisory_lock`.
//!
//! # Rollback
//!
//! Reversible by `git revert` of this module's commit (pure addition).
//! Operator-side rollback of migrated data:
//!
//! ```sql
//! -- Stage 3 artefacts first (must precede user deletion because of FK).
//! DELETE FROM group_members
//! WHERE user_id IN (SELECT id FROM users WHERE legacy_sqlite_id IS NOT NULL)
//!   AND group_id IN (
//!       SELECT id FROM groups
//!       WHERE name = 'Legacy Personal Workspace' AND type = 'personal'
//!   );
//! DELETE FROM groups
//! WHERE name = 'Legacy Personal Workspace' AND type = 'personal';
//!
//! -- Stages 1–2 artefacts.
//! DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL;
//! -- Cascades to user_identities (FK ON DELETE CASCADE).
//! -- audit_events rows persist by design (plan 0034 §8, LGPD art. 37).
//! ```

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::{info, instrument};
use uuid::Uuid;

/// Subset of sysexits.h codes used by `migrate workspace`. Kept local to
/// the module so the CLI entrypoint can `std::process::exit` without
/// guessing magic numbers. `IO_ERR` is the only code accessed from
/// `main.rs`; the rest are internal to the stage machinery.
#[allow(dead_code)] // DATA_ERR reserved for future use (stages 3+)
pub(crate) mod exit_codes {
    pub const OK: i32 = 0;
    /// Pre-flight check failed (schema version / missing table / bad auth).
    pub const USAGE: i32 = 64;
    /// SQLite corrupted or row violates destination constraint.
    pub const DATA_ERR: i32 = 65;
    /// `--to-postgres` user lacks BYPASSRLS / superuser.
    pub const NO_USER: i32 = 67;
    /// I/O error reading SQLite or writing Postgres.
    pub const IO_ERR: i32 = 74;
    /// Confirmation gate, conflict that requires manual intervention.
    pub const CONFIG: i32 = 78;
}

/// Minimum Postgres migration version expected for Stage 1.
///
/// Migration 003 (files) is the most recent forward-required schema for
/// slices 3+; Stage 1 strictly needs `users` (001), `audit_events` (002),
/// `user_identities` (001 + 009/hash_upgraded_at). We still assert the
/// full baseline 001–013 here because the migration tool should run
/// against a fully-migrated Postgres (matches plan 0034 §6.4).
const REQUIRED_MIGRATIONS: &[&str] = &[
    "001", "002", "003", "004", "005", "006", "007", "008", "009", "010", "011", "012", "013",
];

const PBKDF2_ITERATIONS: u32 = 600_000;
const PBKDF2_OUTPUT_LEN: u32 = 32;

/// Report produced by a successful `run` (or a failed one — fields
/// populated up to the point of failure).
///
/// Covers stages 1 (users), 2 (identities + audit) and 3 (groups +
/// group_members). Stage-specific callers are responsible for bumping
/// only their own fields; the outer `run` aggregates.
#[derive(Debug, Default)]
pub struct StageReport {
    // Stage 1.
    pub users_inserted: u64,
    pub users_upserted: u64,
    // Stage 2.
    pub identities_inserted: u64,
    pub identities_skipped_conflict: u64,
    pub audit_events_inserted: u64,
    // Stage 3 (plan 0040).
    pub groups_inserted: u64,
    pub groups_reused: u64,
    pub group_members_inserted: u64,
    pub group_members_skipped_conflict: u64,
    pub group_audit_events_inserted: u64,
    pub dry_run: bool,
}

impl StageReport {
    pub fn print_summary(&self) {
        let mode = if self.dry_run { " (dry run)" } else { "" };
        println!("Workspace Migration Report — Stages 1–3{mode}");
        println!("──────────────────────────────────────");
        println!(
            "  users:            {} inserted, {} upserted-on-existing",
            self.users_inserted, self.users_upserted
        );
        println!(
            "  identities:       {} inserted, {} skipped (conflict)",
            self.identities_inserted, self.identities_skipped_conflict
        );
        println!(
            "  groups:           {} inserted, {} reused",
            self.groups_inserted, self.groups_reused
        );
        println!(
            "  group_members:    {} inserted, {} skipped (conflict)",
            self.group_members_inserted, self.group_members_skipped_conflict
        );
        println!(
            "  audit rows:       {} (users) + {} (groups/members)",
            self.audit_events_inserted, self.group_audit_events_inserted
        );
    }
}

/// CLI options accepted by [`run`]. Grouping them in a struct keeps the
/// call-site stable as future slices add `--only`/`--skip`/`--batch-size`.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub dry_run: bool,
    pub confirm_backup: bool,
    /// Plan 0040 §5.5 — `--target-group-name`.
    pub target_group_name: String,
    /// Plan 0040 §5.5 — `--target-group-type`. Validated against
    /// `groups.type` CHECK (`'family' | 'team' | 'personal'`). Operator
    /// mistake bubbles up as exit 65 via SQLSTATE 23514 handling.
    pub target_group_type: String,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            confirm_backup: false,
            target_group_name: "Legacy Personal Workspace".to_string(),
            target_group_type: "personal".to_string(),
        }
    }
}

/// Entry point invoked by the CLI. Returns `Ok(exit_code)` so the caller
/// forwards it to `std::process::exit` without anyhow context leaking in.
///
/// # Arguments
///
/// * `sqlite_path` — legacy SQLite file (read-only).
/// * `postgres_url` — target Postgres DSN (user must have BYPASSRLS or
///   superuser).
/// * `opts` — CLI flags (dry-run, confirmation, group target).
#[instrument(
    name = "migrate_workspace.run",
    skip(postgres_url, sqlite_path, opts),
    fields(
        dry_run = opts.dry_run,
        confirm_backup = opts.confirm_backup,
        target_group_type = %opts.target_group_type
    )
)]
pub async fn run(
    sqlite_path: &Path,
    postgres_url: &str,
    opts: RunOptions,
) -> Result<(StageReport, i32)> {
    if let Some(code) = preflight_sqlite(sqlite_path)? {
        return Ok((StageReport::default(), code));
    }

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(postgres_url)
        .await
        .context("connect to Postgres")?;

    if let Some(code) = preflight_bypassrls(&pool).await? {
        return Ok((StageReport::default(), code));
    }
    if let Some(code) = preflight_schema_version(&pool).await? {
        return Ok((StageReport::default(), code));
    }
    if let Some(code) =
        preflight_confirmation_gate(&pool, opts.dry_run, opts.confirm_backup).await?
    {
        return Ok((StageReport::default(), code));
    }

    let legacy_rows = load_mobile_users(sqlite_path)?;
    info!(
        count = legacy_rows.len(),
        "loaded mobile_users rows from SQLite"
    );

    let mut report = StageReport {
        dry_run: opts.dry_run,
        ..Default::default()
    };

    let mut tx = pool.begin().await.context("begin stages 1..3 tx")?;

    // SEC-H-1: re-check BYPASSRLS inside the first data tx on the same
    // connection pool (PgPool with max_connections=1 keeps us on the
    // same connection).
    if !bypass_rls_or_super(&mut *tx).await? {
        return Ok((StageReport::default(), exit_codes::NO_USER));
    }

    for row in &legacy_rows {
        // Stage 7.1 — users UPSERT.
        let upserted = insert_user(&mut *tx, row).await?;
        if upserted {
            report.users_upserted += 1;
        } else {
            report.users_inserted += 1;
        }

        // Stage 7.2 — identities with atomic audit.
        let phc = pbkdf2_legacy_to_phc(&row.password_hash_b64, &row.salt_b64)
            .with_context(|| format!("reassemble PHC for legacy user {}", row.id))?;

        let (id_ins, audit_ins) = insert_identity_with_audit(&mut *tx, row, &phc).await?;
        if id_ins {
            report.identities_inserted += 1;
        } else {
            report.identities_skipped_conflict += 1;
        }
        if audit_ins {
            report.audit_events_inserted += 1;
        }
    }

    // Stage 7.4 — groups + group_members. Runs inside the same tx so a
    // failure here rolls back the users/identities too. No-op when the
    // SQLite was empty.
    run_stage3_groups(
        &mut *tx,
        &opts.target_group_name,
        &opts.target_group_type,
        &mut report,
    )
    .await?;

    if opts.dry_run {
        tx.rollback().await.context("rollback dry-run tx")?;
        info!("dry run: rolled back; no rows persisted");
    } else {
        tx.commit().await.context("commit stages 1..3 tx")?;
    }

    Ok((report, exit_codes::OK))
}

#[derive(Debug, Clone)]
struct MobileUserRow {
    id: String,
    email: String,
    password_hash_b64: String,
    salt_b64: String,
    /// Parsed at load time from SQLite's ISO 8601 TEXT into
    /// `DateTime<Utc>`. Failing to parse aborts the load with a precise
    /// per-row error message instead of surfacing an opaque Postgres
    /// error at INSERT time (code review MEDIUM).
    created_at: DateTime<Utc>,
}

fn preflight_sqlite(path: &Path) -> Result<Option<i32>> {
    if !path.exists() {
        eprintln!("error: SQLite file not found: {}", path.display());
        return Ok(Some(exit_codes::IO_ERR));
    }
    let conn = match rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot open SQLite file: {e}");
            return Ok(Some(exit_codes::IO_ERR));
        }
    };
    let tbl_exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='mobile_users'",
            [],
            |r| r.get(0),
        )
        .optional()
        .context("check sqlite_master")?;
    if tbl_exists.is_none() {
        eprintln!(
            "error: `mobile_users` table not found in SQLite — is this the right file? \
             (expected a GarraIA CLI SQLite database)"
        );
        return Ok(Some(exit_codes::USAGE));
    }
    Ok(None)
}

async fn preflight_bypassrls(pool: &PgPool) -> Result<Option<i32>> {
    let mut conn = pool.acquire().await.context("acquire conn")?;
    if !bypass_rls_or_super(&mut *conn).await? {
        eprintln!(
            "error: `--to-postgres` user lacks BYPASSRLS or SUPERUSER — migration \
             requires unrestricted writes to tenant-scoped tables."
        );
        return Ok(Some(exit_codes::NO_USER));
    }
    Ok(None)
}

async fn bypass_rls_or_super<'c, E>(executor: E) -> Result<bool>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let row =
        sqlx::query("SELECT rolbypassrls, rolsuper FROM pg_roles WHERE rolname = current_user")
            .fetch_optional(executor)
            .await
            .context("query pg_roles")?;
    let Some(row) = row else {
        return Ok(false);
    };
    let bypass: bool = row
        .try_get("rolbypassrls")
        .context("read rolbypassrls from pg_roles")?;
    let sup: bool = row
        .try_get("rolsuper")
        .context("read rolsuper from pg_roles")?;
    Ok(bypass || sup)
}

async fn preflight_schema_version(pool: &PgPool) -> Result<Option<i32>> {
    // `_sqlx_migrations` is created by `sqlx migrate run`. Column is
    // `version BIGINT`; we match by the integer portion of the file
    // prefix (001 → 1, 013 → 13).
    let rows = sqlx::query("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .context(
            "read _sqlx_migrations — is Postgres initialised with `garraia-workspace` schema?",
        )?;
    let applied: Vec<i64> = rows
        .iter()
        .map(|r| r.try_get::<i64, _>("version"))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("read version column from _sqlx_migrations")?;
    let missing: Vec<&str> = REQUIRED_MIGRATIONS
        .iter()
        .copied()
        .filter(|m| {
            // `REQUIRED_MIGRATIONS` is a static list of numeric strings
            // (`"001"` … `"013"`). Leading zeros are benign to
            // `i64::parse`; a parse failure here is a programmer bug.
            let n: i64 = m
                .parse()
                .unwrap_or_else(|_| panic!("REQUIRED_MIGRATIONS entry `{m}` is not numeric"));
            !applied.contains(&n)
        })
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "error: Postgres is missing required schema migrations: {:?}. Run \
             `garraia-workspace` migrations before re-trying.",
            missing
        );
        return Ok(Some(exit_codes::USAGE));
    }
    Ok(None)
}

async fn preflight_confirmation_gate(
    pool: &PgPool,
    dry_run: bool,
    confirm_backup: bool,
) -> Result<Option<i32>> {
    if dry_run {
        return Ok(None);
    }
    let row = sqlx::query("SELECT COUNT(*) AS n FROM users")
        .fetch_one(pool)
        .await
        .context("count users")?;
    let n: i64 = row.try_get("n").context("read users count")?;
    if n > 0 && !confirm_backup {
        eprintln!(
            "error: Postgres `users` table already has {n} rows. Pass \
             `--confirm-backup` to proceed (evidence you have a backup of the \
             SQLite source per ADR 0003 §Migration)."
        );
        return Ok(Some(exit_codes::CONFIG));
    }
    Ok(None)
}

fn load_mobile_users(path: &Path) -> Result<Vec<MobileUserRow>> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("reopen sqlite")?;
    let mut stmt = conn
        .prepare("SELECT id, email, password_hash, salt, created_at FROM mobile_users")
        .context("prepare mobile_users SELECT")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // id
                r.get::<_, String>(1)?, // email
                r.get::<_, String>(2)?, // password_hash
                r.get::<_, String>(3)?, // salt
                r.get::<_, String>(4)?, // created_at as ISO 8601 TEXT
            ))
        })
        .context("query mobile_users")?;
    let mut out = Vec::new();
    for row in rows {
        let (id, email, password_hash_b64, salt_b64, created_at_raw) =
            row.context("fetch mobile_users row")?;
        let created_at = parse_sqlite_timestamp(&created_at_raw).with_context(|| {
            format!("parse created_at for mobile_users.id={id}: `{created_at_raw}`")
        })?;
        out.push(MobileUserRow {
            id,
            email,
            password_hash_b64,
            salt_b64,
            created_at,
        });
    }
    Ok(out)
}

/// Parse SQLite's two common timestamp TEXT formats:
///   - ISO 8601 with `T` separator and `Z` suffix  (`2026-04-15T00:00:00Z`)
///   - SQLite `CURRENT_TIMESTAMP` default           (`2026-04-15 00:00:00`)
///
/// The second form omits the timezone; we treat it as UTC (mobile_auth's
/// SQLite schema uses `DEFAULT CURRENT_TIMESTAMP` which is UTC-ish per
/// the SQLite date-and-time functions spec).
fn parse_sqlite_timestamp(raw: &str) -> Result<DateTime<Utc>> {
    // Try ISO 8601 first.
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc));
    }
    // SQLite's CURRENT_TIMESTAMP lacks a TZ; interpret as UTC.
    let naive = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|e| anyhow!("unrecognised timestamp format: {e}"))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

/// Returns `true` if the row existed (UPSERT updated `legacy_sqlite_id`),
/// `false` if a fresh row was inserted.
async fn insert_user(tx: &mut sqlx::PgConnection, row: &MobileUserRow) -> Result<bool> {
    let display = row
        .email
        .split_once('@')
        .map(|(local, _)| local.to_string())
        .unwrap_or_else(|| row.email.clone());
    // `uuid_generate_v7()` is not installed in Postgres (migration 001
    // only installs `pgcrypto` for v4). Generate v7 in Rust and bind —
    // code review HIGH-1.
    let new_user_id = Uuid::now_v7();
    let result = sqlx::query(
        r#"
        INSERT INTO users (id, email, display_name, status, legacy_sqlite_id, created_at, updated_at)
        VALUES ($1, LOWER($2), $3, 'active', $4, $5, $5)
        ON CONFLICT (email) DO UPDATE SET
            legacy_sqlite_id = COALESCE(users.legacy_sqlite_id, EXCLUDED.legacy_sqlite_id)
        RETURNING (xmax = 0) AS inserted
        "#,
    )
    .bind(new_user_id)
    .bind(&row.email)
    .bind(&display)
    .bind(&row.id)
    .bind(row.created_at)
    .fetch_one(&mut *tx)
    .await
    .context("insert_user")?;
    let inserted: bool = result
        .try_get("inserted")
        .context("read `inserted` flag from RETURNING")?;
    Ok(!inserted)
}

/// Insert identity + audit in the same tx. Returns `(identity_inserted,
/// audit_inserted)` — `false` on conflict path (idempotent re-run).
async fn insert_identity_with_audit(
    tx: &mut sqlx::PgConnection,
    row: &MobileUserRow,
    phc: &str,
) -> Result<(bool, bool)> {
    // Resolve the pg user_id that the stage-users step produced.
    let user_id_row = sqlx::query("SELECT id FROM users WHERE legacy_sqlite_id = $1")
        .bind(&row.id)
        .fetch_optional(&mut *tx)
        .await
        .context("lookup user by legacy_sqlite_id")?;
    let user_id_row = user_id_row.ok_or_else(|| {
        anyhow!(
            "user row for legacy_sqlite_id {} not found in-tx — stage 7.1 skipped?",
            row.id
        )
    })?;
    let user_uuid: uuid::Uuid = user_id_row.try_get("id")?;

    let new_identity_id = Uuid::now_v7();
    let id_inserted = sqlx::query(
        r#"
        INSERT INTO user_identities
            (id, user_id, provider, provider_sub, password_hash, created_at, hash_upgraded_at)
        VALUES
            ($1, $2, 'internal', $3, $4, NOW(), NULL)
        ON CONFLICT (provider, provider_sub) DO NOTHING
        "#,
    )
    .bind(new_identity_id)
    .bind(user_uuid)
    .bind(user_uuid.to_string())
    .bind(phc)
    .execute(&mut *tx)
    .await
    .context("insert user_identities")?
    .rows_affected()
        > 0;

    let new_audit_id = Uuid::now_v7();
    let audit_inserted = sqlx::query(
        r#"
        INSERT INTO audit_events
            (id, group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata, created_at)
        SELECT $1, NULL, NULL, 'system.migrate_workspace',
               'users.imported_from_sqlite', 'user', $2::text,
               jsonb_build_object(
                   'source', 'mobile_users',
                   'legacy_id', $3::text,
                   'hash_algorithm', 'pbkdf2-sha256',
                   'iterations', $4::bigint,
                   'lazy_upgrade_pending', true),
               NOW()
        WHERE NOT EXISTS (
            SELECT 1 FROM audit_events
            WHERE action = 'users.imported_from_sqlite'
              AND resource_id = $2::text
        )
        "#,
    )
    .bind(new_audit_id)
    .bind(user_uuid)
    .bind(&row.id)
    .bind(PBKDF2_ITERATIONS as i64)
    .execute(&mut *tx)
    .await
    .context("insert audit_events")?
    .rows_affected()
        > 0;

    Ok((id_inserted, audit_inserted))
}

/// Stage 3 — populate `groups` + `group_members` per plan 0034 §7.4 and
/// plan 0040 §5. Runs inside the caller's transaction so any failure
/// rolls back the entire migration (including stages 1–2). Emits audit
/// rows (`groups.imported_from_sqlite` once per created group +
/// `group_members.imported_from_sqlite` once per membership) in the same
/// transaction.
///
/// The first user migrated (ordered by `users.created_at ASC`) becomes
/// `role='owner'`; every other migrated user becomes `role='member'`.
/// Both bucket resolution and membership INSERT are idempotent:
///
/// - Group is located by `(name, type)` with `SELECT … FOR UPDATE` so
///   the second concurrent run sees the just-inserted row.
/// - Memberships use `ON CONFLICT (group_id, user_id) DO NOTHING`.
/// - Audit rows use `WHERE NOT EXISTS` (audit_events has no unique
///   index we can pivot on).
///
/// No-op when zero users carry `legacy_sqlite_id` (SQLite source was
/// empty) — emits WARN and returns early without touching any tables.
#[instrument(
    name = "migrate_workspace.stage3_groups",
    skip(tx, report),
    fields(target_group_type = %target_group_type)
)]
async fn run_stage3_groups(
    tx: &mut sqlx::PgConnection,
    target_group_name: &str,
    target_group_type: &str,
    report: &mut StageReport,
) -> Result<()> {
    // Plan 0040 risk table — "Primeiro user migrado não existe em
    // Postgres". If legacy users missing, skip silently with WARN and
    // exit 0.
    let owner_candidate: Option<(uuid::Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, created_at FROM users
         WHERE legacy_sqlite_id IS NOT NULL
         ORDER BY created_at ASC, id ASC
         LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await
    .context("select owner candidate")?;

    let Some((owner_user_id, _owner_created_at)) = owner_candidate else {
        tracing::warn!(
            target: "garraia_cli::migrate_workspace",
            "no migrated users; skipping stage 3 (groups/group_members)"
        );
        return Ok(());
    };

    // Resolve group — SELECT … FOR UPDATE so we serialise with any
    // concurrent would-be creator on the same (name, type) pair. If it
    // exists, reuse; otherwise INSERT with `created_by = owner_user_id`.
    // Note on concurrency: plan 0040 §5.2 documents that `READ
    // COMMITTED` cannot fully serialise INSERT-after-missing in absence
    // of a UNIQUE constraint. Known limitation; runs are non-concurrent
    // per plan 0039 F-02.
    let existing_group: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM groups
         WHERE name = $1 AND type = $2
         FOR UPDATE",
    )
    .bind(target_group_name)
    .bind(target_group_type)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlstate_error)
    .context("resolve target group")?;

    let (group_id, group_created_now) = if let Some(gid) = existing_group {
        report.groups_reused += 1;
        (gid, false)
    } else {
        let new_group_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO groups (id, name, type, created_by, settings)
             VALUES ($1, $2, $3, $4, '{}'::jsonb)",
        )
        .bind(new_group_id)
        .bind(target_group_name)
        .bind(target_group_type)
        .bind(owner_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlstate_error)
        .context("insert group")?;
        report.groups_inserted += 1;
        (new_group_id, true)
    };

    if group_created_now {
        // Audit row for the group itself. `WHERE NOT EXISTS` idempotency
        // mirrors the stage 2 pattern.
        let audit_id = uuid::Uuid::now_v7();
        let inserted = sqlx::query(
            r#"
            INSERT INTO audit_events
                (id, group_id, actor_user_id, actor_label, action,
                 resource_type, resource_id, metadata, created_at)
            SELECT $1, $2, NULL, 'system.migrate_workspace',
                   'groups.imported_from_sqlite', 'group', $2::text,
                   jsonb_build_object(
                       'source', 'mobile_users',
                       'target_group_name', $3::text,
                       'target_group_type', $4::text,
                       'created_by_user_id', $5::text),
                   NOW()
            WHERE NOT EXISTS (
                SELECT 1 FROM audit_events
                WHERE action = 'groups.imported_from_sqlite'
                  AND resource_id = $2::text
            )
            "#,
        )
        .bind(audit_id)
        .bind(group_id)
        .bind(target_group_name)
        .bind(target_group_type)
        .bind(owner_user_id.to_string())
        .execute(&mut *tx)
        .await
        .context("insert groups.imported_from_sqlite audit row")?
        .rows_affected();
        if inserted > 0 {
            report.group_audit_events_inserted += 1;
        }
    }

    // Memberships — iterate over every migrated user in deterministic
    // order. First-seen becomes owner; rest become member.
    //
    // Query deliberately scopes to `legacy_sqlite_id IS NOT NULL` so a
    // hypothetical signup user sharing the same email (via the stage 1
    // UPSERT) doesn't accidentally receive Stage 3 membership.
    #[derive(sqlx::FromRow)]
    struct MigratedUser {
        id: uuid::Uuid,
    }
    let migrated: Vec<MigratedUser> = sqlx::query_as(
        "SELECT id FROM users
         WHERE legacy_sqlite_id IS NOT NULL
         ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(&mut *tx)
    .await
    .context("list migrated users for membership insert")?;

    for (idx, user) in migrated.iter().enumerate() {
        let role = if idx == 0 && group_created_now {
            // Only the freshly-created group gets the migration's owner;
            // a reused group keeps its own owner to avoid stomping on
            // an operator-crafted bucket.
            "owner"
        } else {
            "member"
        };

        let res = sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, status)
             VALUES ($1, $2, $3, 'active')
             ON CONFLICT (group_id, user_id) DO NOTHING",
        )
        .bind(group_id)
        .bind(user.id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .context("insert group_members row")?;

        if res.rows_affected() > 0 {
            report.group_members_inserted += 1;

            // Audit the membership in the same tx.
            let audit_id = uuid::Uuid::now_v7();
            // Compose the audit resource_id as `{group_id}:{user_id}`
            // so `WHERE NOT EXISTS` stays per-membership.
            let resource_id = format!("{group_id}:{}", user.id);
            let inserted = sqlx::query(
                r#"
                INSERT INTO audit_events
                    (id, group_id, actor_user_id, actor_label, action,
                     resource_type, resource_id, metadata, created_at)
                SELECT $1, $2, NULL, 'system.migrate_workspace',
                       'group_members.imported_from_sqlite',
                       'group_member', $3::text,
                       jsonb_build_object(
                           'source', 'mobile_users',
                           'role', $4::text,
                           'user_id', $5::text,
                           'group_id', $2::text),
                       NOW()
                WHERE NOT EXISTS (
                    SELECT 1 FROM audit_events
                    WHERE action = 'group_members.imported_from_sqlite'
                      AND resource_id = $3::text
                )
                "#,
            )
            .bind(audit_id)
            .bind(group_id)
            .bind(&resource_id)
            .bind(role)
            .bind(user.id.to_string())
            .execute(&mut *tx)
            .await
            .context("insert group_members.imported_from_sqlite audit row")?
            .rows_affected();
            if inserted > 0 {
                report.group_audit_events_inserted += 1;
            }
        } else {
            report.group_members_skipped_conflict += 1;
        }
    }

    info!(
        group_id = %group_id,
        members_new = report.group_members_inserted,
        members_reused = report.group_members_skipped_conflict,
        "stage 3 complete"
    );
    Ok(())
}

/// Map common Postgres SQLSTATE codes to actionable CLI exit paths.
/// Currently narrow: only 23514 (CHECK violation) is surfaced because
/// the caller is most likely to mis-type `--target-group-type`.
/// `anyhow::Error` preserves the original context for the user.
fn map_sqlstate_error(err: sqlx::Error) -> sqlx::Error {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.code().as_deref() == Some("23514") {
            eprintln!(
                "error: Postgres CHECK constraint violated — \
                 verify --target-group-type is one of 'family', \
                 'team', 'personal' ({})",
                db_err.message()
            );
        }
    }
    err
}

/// Reassemble a legacy PBKDF2 `(hash_b64_standard, salt_b64_standard)`
/// pair into a PHC string accepted by `password-hash`'s `pbkdf2` crate.
///
/// The SQLite legacy format (plan 0034 §7.2 empirical check) encodes 32
/// raw bytes each with `base64::STANDARD` (padded, `=`-terminated). The
/// PHC format (RFC-ish) expects the same bytes re-encoded with
/// `base64::STANDARD_NO_PAD` (no `=` suffix). Encoding divergence is
/// the single most common migration footgun — therefore this helper
/// carries extensive tests.
pub fn pbkdf2_legacy_to_phc(hash_b64_std: &str, salt_b64_std: &str) -> Result<String> {
    let hash_bytes = STANDARD
        .decode(hash_b64_std.trim())
        .with_context(|| "decode legacy hash base64-STANDARD")?;
    let salt_bytes = STANDARD
        .decode(salt_b64_std.trim())
        .with_context(|| "decode legacy salt base64-STANDARD")?;
    if hash_bytes.len() != PBKDF2_OUTPUT_LEN as usize {
        return Err(anyhow!(
            "legacy hash length = {} bytes; expected {}",
            hash_bytes.len(),
            PBKDF2_OUTPUT_LEN
        ));
    }
    if salt_bytes.is_empty() {
        return Err(anyhow!("legacy salt is empty"));
    }
    let salt_nopad = STANDARD_NO_PAD.encode(&salt_bytes);
    let hash_nopad = STANDARD_NO_PAD.encode(&hash_bytes);
    Ok(format!(
        "$pbkdf2-sha256$i={PBKDF2_ITERATIONS},l={PBKDF2_OUTPUT_LEN}${salt_nopad}${hash_nopad}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use ring::pbkdf2;
    use std::num::NonZeroU32;

    /// Replicate `mobile_auth::legacy_hash_password_for_tests` locally so
    /// the unit test does not need to depend on `garraia-gateway`'s
    /// `test-helpers` feature (which would pull the entire Axum stack).
    fn legacy_hash(password: &str, salt: &[u8]) -> String {
        let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
        let mut hash = vec![0u8; PBKDF2_OUTPUT_LEN as usize];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            password.as_bytes(),
            &mut hash,
        );
        BASE64.encode(&hash)
    }

    #[test]
    fn phc_roundtrip_with_legacy_fixture() {
        use garraia_auth::hashing::verify_pbkdf2;
        use secrecy::SecretString;

        let password = "test-password-1234-αβγ";
        let salt_raw = [0x42u8; 32];
        let salt_b64 = BASE64.encode(salt_raw);
        let hash_b64 = legacy_hash(password, &salt_raw);

        let phc = pbkdf2_legacy_to_phc(&hash_b64, &salt_b64).expect("reassemble PHC");
        assert!(phc.starts_with("$pbkdf2-sha256$i=600000,l=32$"));
        // The params block legitimately contains `=` in `i=600000,l=32`;
        // the base64 body (salt + hash) must NOT carry `=` padding.
        let (_params, body) = phc.rsplit_once("$i=600000,l=32$").unwrap();
        assert!(
            !body.contains('='),
            "PHC body (salt + hash) must not contain base64 padding; got `{body}`"
        );

        // The critical assertion: the reassembled PHC verifies with the
        // same password via garraia-auth's verify_pbkdf2.
        let verified =
            verify_pbkdf2(&phc, &SecretString::new(password.into())).expect("verify did not error");
        assert!(
            verified,
            "reassembled PHC must verify the original password"
        );

        let bad = verify_pbkdf2(&phc, &SecretString::new("wrong-password".into()))
            .expect("verify did not error");
        assert!(!bad, "reassembled PHC must reject wrong password");
    }

    #[test]
    fn phc_roundtrip_with_random_salt_and_password() {
        use garraia_auth::hashing::verify_pbkdf2;
        use ring::rand::{SecureRandom, SystemRandom};
        use secrecy::SecretString;

        let rng = SystemRandom::new();
        let mut salt = vec![0u8; 32];
        rng.fill(&mut salt).unwrap();
        let salt_b64 = BASE64.encode(&salt);
        let password = "another-password!@#";
        let hash_b64 = legacy_hash(password, &salt);

        let phc = pbkdf2_legacy_to_phc(&hash_b64, &salt_b64).unwrap();
        let verified = verify_pbkdf2(&phc, &SecretString::new(password.into())).unwrap();
        assert!(verified);

        // Paired rejection check (plan 0039 audit F-08).
        let rejected = verify_pbkdf2(&phc, &SecretString::new("totally-different".into())).unwrap();
        assert!(!rejected, "wrong password must not verify");
    }

    #[test]
    fn phc_rejects_bad_base64() {
        let err = pbkdf2_legacy_to_phc("not-valid-base64!!!", "==").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("decode legacy hash"), "got: {msg}");
    }

    #[test]
    fn phc_rejects_wrong_hash_length() {
        let short_hash = BASE64.encode([0x10u8; 8]); // 8 bytes, not 32
        let salt = BASE64.encode([0x20u8; 32]);
        let err = pbkdf2_legacy_to_phc(&short_hash, &salt).unwrap_err();
        assert!(err.to_string().contains("legacy hash length"));
    }

    #[test]
    fn phc_rejects_empty_salt() {
        let hash = BASE64.encode([0x10u8; 32]);
        let err = pbkdf2_legacy_to_phc(&hash, "").unwrap_err();
        assert!(err.to_string().contains("legacy salt is empty"));
    }

    #[test]
    fn stage_report_default_is_zero_and_dry_run_false() {
        let r = StageReport::default();
        assert_eq!(r.users_inserted, 0);
        assert_eq!(r.identities_inserted, 0);
        assert_eq!(r.audit_events_inserted, 0);
        assert_eq!(r.groups_inserted, 0);
        assert_eq!(r.groups_reused, 0);
        assert_eq!(r.group_members_inserted, 0);
        assert_eq!(r.group_members_skipped_conflict, 0);
        assert_eq!(r.group_audit_events_inserted, 0);
        assert!(!r.dry_run);
    }

    #[test]
    fn run_options_defaults_match_plan_0040() {
        let o = RunOptions::default();
        assert_eq!(o.target_group_name, "Legacy Personal Workspace");
        assert_eq!(o.target_group_type, "personal");
        assert!(!o.dry_run);
        assert!(!o.confirm_backup);
    }
}
