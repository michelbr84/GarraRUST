//! `garraia migrate workspace` — SQLite → Postgres migration (Stages 1–3 + 5).
//!
//! - Plan 0039 implemented §7.1 (users) + §7.2 (user_identities + atomic
//!   audit) of [plan 0034](../../plans/0034-gar-413-migrate-workspace-spec.md).
//! - Plan 0040 adds §7.4 (groups + group_members): the legacy bucket is
//!   auto-created under `--target-group-name` / `--target-group-type`, the
//!   oldest migrated user becomes owner, the rest become members, and
//!   each write emits an atomic audit row in the same transaction.
//! - Plan 0045 (this slice) adds Stage 5 — `chats` + `chat_members` from
//!   SQLite `sessions`. Amends plan 0034 §7.5 (which referenced
//!   `conversations`): the legacy table is `sessions`. One `chats` row
//!   per session, one `chat_members` row per (session, migrated_user),
//!   atomic audit both for chats and members, mapping
//!   `legacy_session_id → new_chat_id` built in-memory for future Stage
//!   6 (messages) to consume.
//! - Subsequent stages (messages, memory, api_keys, audit retrofit) land
//!   in follow-up slices.
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
//! -- Stage 5 artefacts first — chat_members is not auto-cascaded by
//! -- user deletion in a way that removes the chat rows.
//! DELETE FROM chat_members
//! WHERE chat_id IN (
//!     SELECT resource_id::uuid FROM audit_events
//!     WHERE action = 'chats.imported_from_sqlite'
//! );
//! DELETE FROM chats
//! WHERE id IN (
//!     SELECT resource_id::uuid FROM audit_events
//!     WHERE action = 'chats.imported_from_sqlite'
//! );
//!
//! -- Stage 3 artefacts (must precede user deletion because of FK).
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

use std::collections::HashMap;
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
/// Covers stages 1 (users), 2 (identities + audit), 3 (groups +
/// group_members) and 5 (chats + chat_members). Stage-specific callers
/// are responsible for bumping only their own fields; the outer `run`
/// aggregates.
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
    // Stage 5 (plan 0045).
    pub chats_inserted: u64,
    pub chats_skipped_conflict: u64,
    pub chat_members_inserted: u64,
    pub chat_members_skipped_conflict: u64,
    pub chat_audit_events_inserted: u64,
    pub sessions_skipped_no_user: u64,
    pub dry_run: bool,
}

impl StageReport {
    pub fn print_summary(&self) {
        let mode = if self.dry_run { " (dry run)" } else { "" };
        println!("Workspace Migration Report — Stages 1–3 + 5{mode}");
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
            "  chats:            {} inserted, {} skipped (conflict), {} sessions skipped (no user)",
            self.chats_inserted, self.chats_skipped_conflict, self.sessions_skipped_no_user
        );
        println!(
            "  chat_members:     {} inserted, {} skipped (conflict)",
            self.chat_members_inserted, self.chat_members_skipped_conflict
        );
        println!(
            "  audit rows:       {} (users) + {} (groups/members) + {} (chats/members)",
            self.audit_events_inserted,
            self.group_audit_events_inserted,
            self.chat_audit_events_inserted
        );
    }
}

/// In-memory map from legacy SQLite `sessions.id` to newly minted
/// Postgres `chats.id`. Built by [`run_stage5_chats`]; a future slice
/// for Stage 6 (messages) will consume this to rewrite
/// `messages.session_id` references. Not persisted in any DB table —
/// the relationship is rebuilt on every migration run.
///
/// Invariant maintained by Stage 5: `session_to_chat.len()` equals
/// `StageReport.chats_inserted` after a successful run (guarded by an
/// integration-test assertion and a debug_assert inside the stage
/// function).
#[derive(Debug, Default)]
pub struct ChatMapping {
    pub session_to_chat: HashMap<String, Uuid>,
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
///
/// # Tracing
///
/// The `#[instrument]` `skip(...)` list drops `opts` and the two path
/// args from the span payload by default; the `fields(...)` clause
/// then re-emits only deterministic, non-PII values — `dry_run`,
/// `confirm_backup`, and `target_group_type` (validated by a CHECK
/// constraint, always one of `family`/`team`/`personal`).
/// `target_group_name` is **deliberately omitted** from `fields(...)`
/// because it is operator-supplied free text that could reach OTLP
/// exporters; security audit SEC-M-01 (plan 0040). Do not add it to
/// `fields()` in a refactor without re-evaluating the exporter surface.
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
        let upserted = insert_user(&mut tx, row).await?;
        if upserted {
            report.users_upserted += 1;
        } else {
            report.users_inserted += 1;
        }

        // Stage 7.2 — identities with atomic audit.
        let phc = pbkdf2_legacy_to_phc(&row.password_hash_b64, &row.salt_b64)
            .with_context(|| format!("reassemble PHC for legacy user {}", row.id))?;

        let (id_ins, audit_ins) = insert_identity_with_audit(&mut tx, row, &phc).await?;
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
        &mut tx,
        &opts.target_group_name,
        &opts.target_group_type,
        &mut report,
    )
    .await?;

    // Stage 7.5 (plan 0045) — chats + chat_members from SQLite
    // `sessions`. Runs inside the same tx so any failure rolls back
    // stages 1–3 too. The returned ChatMapping is consumed only by a
    // future Stage 6 slice; ignored here after the smoke-check below.
    let chat_mapping = run_stage5_chats(sqlite_path, &mut tx, &mut report).await?;
    // Smoke invariant (plan 0045 acceptance criterion 16): the mapping
    // must have one entry per inserted chat. A divergence here signals
    // a bug in the INSERT/idempotency logic, not a data issue.
    debug_assert_eq!(
        chat_mapping.session_to_chat.len() as u64,
        report.chats_inserted,
        "ChatMapping length must equal chats_inserted"
    );

    if opts.dry_run {
        tx.rollback().await.context("rollback dry-run tx")?;
        info!("dry run: rolled back; no rows persisted");
    } else {
        tx.commit().await.context("commit stages 1..3 + 5 tx")?;
    }

    Ok((report, exit_codes::OK))
}

/// Narrow projection of `users` used by the Stage 3 membership loop.
/// Kept at module scope (not inside the async fn) so sqlx's derive
/// macros resolve cleanly and future refactors stay readable.
#[derive(sqlx::FromRow)]
struct MigratedUser {
    id: uuid::Uuid,
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

    // Code review MEDIUM fix — owner selection survives a partial rerun
    // where the group row exists but `group_members` is empty (e.g. an
    // operator who DELETEd memberships, or a prior run that committed
    // the group but its membership inserts were reverted by a crash
    // between separate runs). Rather than tying the owner promotion to
    // `group_created_now`, check whether the group already has any
    // active owner; if none, the first migrated user takes the slot.
    // This preserves the reuse-a-pre-existing-group semantic (caller
    // provided a group that already has its own owner) without
    // collapsing the "group created earlier, members lost" case.
    let group_has_active_owner: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
             SELECT 1 FROM group_members
             WHERE group_id = $1 AND role = 'owner' AND status = 'active'
         )",
    )
    .bind(group_id)
    .fetch_one(&mut *tx)
    .await
    .context("probe existing owner")?;

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
    // order. The first user takes `owner` iff no active owner exists on
    // the group yet; otherwise everyone is `member`. This covers three
    // realistic scenarios at once:
    //   (a) freshly created group   → first legacy user is owner;
    //   (b) pre-existing group from an operator seed with its own owner
    //       → legacy users join as `member` (owner preservation);
    //   (c) partial-rerun / orphaned group where memberships were
    //       purged externally → first legacy user takes the empty
    //       owner slot (code review MEDIUM fix).
    //
    // Query deliberately scopes to `legacy_sqlite_id IS NOT NULL` so a
    // hypothetical signup user sharing the same email (via the stage 1
    // UPSERT) doesn't accidentally receive Stage 3 membership.
    let migrated: Vec<MigratedUser> = sqlx::query_as(
        "SELECT id FROM users
         WHERE legacy_sqlite_id IS NOT NULL
         ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(&mut *tx)
    .await
    .context("list migrated users for membership insert")?;

    for (idx, user) in migrated.iter().enumerate() {
        let role = if idx == 0 && !group_has_active_owner {
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

/// Narrow projection of SQLite `sessions` loaded for Stage 5. Only the
/// fields we need — metadata is kept as raw TEXT so
/// [`session_name_from_metadata`] can decide the name derivation path.
#[derive(Debug, Clone)]
struct SessionRow {
    id: String,
    channel_id: String,
    user_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    metadata_raw: String,
}

/// Derive the `chats.name` value from SQLite `sessions.metadata` +
/// `sessions.channel_id`, with the fallback chain described in plan
/// 0045 §5.3.
///
/// 1. `metadata.title` when present, non-empty, and the metadata parses
///    as JSON.
/// 2. `"Chat {channel_id}"` when `channel_id` is non-empty.
/// 3. `"Legacy chat"` otherwise.
///
/// The function is **fail-closed** w.r.t. SQLite integrity: malformed
/// JSON, missing keys, wrong value types, or a whitespace-only `title`
/// all fall through without returning an error. Stage 5 callers must
/// never propagate a parse error — they'd abort a migration over a
/// purely cosmetic field.
///
/// # Security
///
/// Plan 0045 §5.1 / §7: `sessions.metadata` may legitimately contain
/// channel-specific tokens (Telegram chat metadata, device IDs). The
/// caller MUST NOT log `metadata_raw`; only the extracted `title` is
/// safe-to-display because the operator writes it themselves via the
/// mobile client.
fn session_name_from_metadata(metadata_raw: &str, channel_id: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata_raw)
        && let Some(title) = value.get("title").and_then(|v| v.as_str())
    {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let trimmed_channel = channel_id.trim();
    if !trimmed_channel.is_empty() {
        return format!("Chat {trimmed_channel}");
    }
    "Legacy chat".to_string()
}

/// Load `sessions` rows from the SQLite legacy DB, ordered
/// deterministically (`created_at ASC, id ASC`). Returns `Ok(None)`
/// when the `sessions` table does not exist (old SQLite installs that
/// never ran `SessionStore::ensure_schema`). A missing table is NOT an
/// error — Stage 5 should skip gracefully with a WARN (plan 0045 §1.7).
fn load_sessions(path: &Path) -> Result<Option<Vec<SessionRow>>> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("reopen sqlite for stage 5")?;
    let table_exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |r| r.get(0),
        )
        .optional()
        .context("check sqlite_master for sessions")?;
    if table_exists.is_none() {
        return Ok(None);
    }

    let mut stmt = conn
        .prepare(
            "SELECT id, channel_id, user_id, created_at, updated_at,
                    COALESCE(metadata, '{}') AS metadata
             FROM sessions
             ORDER BY created_at ASC, id ASC",
        )
        .context("prepare sessions SELECT")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // id
                r.get::<_, String>(1)?, // channel_id
                r.get::<_, String>(2)?, // user_id
                r.get::<_, String>(3)?, // created_at
                r.get::<_, String>(4)?, // updated_at
                r.get::<_, String>(5)?, // metadata
            ))
        })
        .context("query sessions")?;
    let mut out = Vec::new();
    for row in rows {
        let (id, channel_id, user_id, created_raw, updated_raw, metadata_raw) =
            row.context("fetch sessions row")?;
        let created_at = parse_sqlite_timestamp(&created_raw)
            .with_context(|| format!("parse sessions.created_at for id={id}: `{created_raw}`"))?;
        let updated_at = parse_sqlite_timestamp(&updated_raw)
            .with_context(|| format!("parse sessions.updated_at for id={id}: `{updated_raw}`"))?;
        out.push(SessionRow {
            id,
            channel_id,
            user_id,
            created_at,
            updated_at,
            metadata_raw,
        });
    }
    Ok(Some(out))
}

/// Stage 5 — populate `chats` + `chat_members` from SQLite `sessions`
/// per plan 0045. Runs inside the caller's transaction so any failure
/// rolls back the entire migration (stages 1–3 included). Emits audit
/// rows (`chats.imported_from_sqlite` once per created chat +
/// `chat_members.imported_from_sqlite` once per membership) in the
/// same transaction.
///
/// Skip conditions (plan 0045 §1.7):
///
/// - SQLite has no `sessions` table → WARN + early return, no DB
///   touch.
/// - SQLite has zero rows in `sessions` → silent early return.
/// - Postgres has no migrated users (stage 3 skipped with WARN too) →
///   early return because `chats.created_by` would have no owner.
/// - A specific session's `user_id` does not resolve to
///   `users.legacy_sqlite_id` → skip that session, bump
///   `sessions_skipped_no_user`, log WARN with
///   legacy_session_id + legacy_user_id only (both are internal
///   identifiers, not PII).
///
/// # Side effects
///
/// - Emits at most one `chats` row per session.
/// - Emits at most one `chat_members` row per session (role=`owner`).
/// - Emits at most one `chats.imported_from_sqlite` audit row per
///   new chat.
/// - Emits at most one `chat_members.imported_from_sqlite` audit row
///   per new membership.
///
/// # Returns
///
/// A [`ChatMapping`] from legacy `sessions.id` → new `chats.id`. The
/// mapping tracks **only chats that were newly inserted in this run**;
/// idempotent re-runs that detect an existing audit row do NOT populate
/// the mapping. Plan 0045 §4 criterion 16 asserts the invariant
/// `ChatMapping.len() == report.chats_inserted` (debug_assert in the
/// caller). A future Stage 6 (messages) slice running in the same tx
/// will therefore see exactly the chats just created — consistent with
/// its own audit-row idempotency skipping messages already imported.
#[instrument(name = "migrate_workspace.stage5_chats", skip(sqlite_path, tx, report))]
async fn run_stage5_chats(
    sqlite_path: &Path,
    tx: &mut sqlx::PgConnection,
    report: &mut StageReport,
) -> Result<ChatMapping> {
    let mut mapping = ChatMapping::default();

    let sessions = match load_sessions(sqlite_path)? {
        Some(s) => s,
        None => {
            tracing::warn!(
                target: "garraia_cli::migrate_workspace",
                "SQLite `sessions` table absent; skipping stage 5"
            );
            return Ok(mapping);
        }
    };

    if sessions.is_empty() {
        info!("no sessions in SQLite; skipping stage 5");
        return Ok(mapping);
    }

    // Owner attribution for `chats.created_by` — the group owner
    // (stage 3). If stage 3 found no migrated users, stage 5 can't
    // write either because `created_by` is NOT NULL.
    let owner_row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT u.id, gm.group_id FROM users u
         JOIN group_members gm ON gm.user_id = u.id
         WHERE u.legacy_sqlite_id IS NOT NULL
           AND gm.role = 'owner' AND gm.status = 'active'
         ORDER BY u.created_at ASC, u.id ASC
         LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await
    .context("select owner user + group for stage 5")?;
    let Some((owner_user_id, legacy_group_id)) = owner_row else {
        tracing::warn!(
            target: "garraia_cli::migrate_workspace",
            "no migrated owner found; skipping stage 5"
        );
        return Ok(mapping);
    };

    for session in &sessions {
        // Resolve `sessions.user_id` → pg user.id via legacy_sqlite_id.
        let pg_user_row: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM users WHERE legacy_sqlite_id = $1")
                .bind(&session.user_id)
                .fetch_optional(&mut *tx)
                .await
                .context("lookup user by legacy_sqlite_id for stage 5")?;
        let Some(pg_user_id) = pg_user_row else {
            report.sessions_skipped_no_user += 1;
            tracing::warn!(
                target: "garraia_cli::migrate_workspace",
                legacy_session_id = %session.id,
                legacy_user_id = %session.user_id,
                "session user not migrated; skipping session"
            );
            continue;
        };

        let chat_name = session_name_from_metadata(&session.metadata_raw, &session.channel_id);

        // Look for an existing chat already imported from this legacy
        // session (idempotency). The ground truth is the audit row,
        // because there is no UNIQUE constraint on `chats` we can
        // ON CONFLICT against.
        let existing_chat_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT resource_id::uuid FROM audit_events
             WHERE action = 'chats.imported_from_sqlite'
               AND metadata->>'legacy_session_id' = $1
             LIMIT 1",
        )
        .bind(&session.id)
        .fetch_optional(&mut *tx)
        .await
        .context("lookup existing stage 5 chat via audit")?;

        let (chat_id, chat_created_now) = if let Some(existing) = existing_chat_id {
            report.chats_skipped_conflict += 1;
            (existing, false)
        } else {
            let new_chat_id = Uuid::now_v7();
            sqlx::query(
                r#"
                INSERT INTO chats (id, group_id, type, name, created_by, settings, created_at, updated_at)
                VALUES ($1, $2, 'channel', $3, $4, '{}'::jsonb, $5, $6)
                "#,
            )
            .bind(new_chat_id)
            .bind(legacy_group_id)
            .bind(&chat_name)
            .bind(owner_user_id)
            .bind(session.created_at)
            .bind(session.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlstate_error)
            .context("insert chats")?;
            report.chats_inserted += 1;
            // Only record newly inserted chats in the mapping — plan
            // 0045 §4 criterion 16 invariant `mapping.len() ==
            // chats_inserted`. Idempotent reruns that find an existing
            // audit row do NOT populate the mapping; a future Stage 6
            // slice will skip messages already imported via its own
            // audit-row idempotency check.
            mapping
                .session_to_chat
                .insert(session.id.clone(), new_chat_id);
            (new_chat_id, true)
        };

        if chat_created_now {
            // Audit row: chats.imported_from_sqlite. Zero PII in
            // metadata — `legacy_session_id` + `channel_id` are
            // internal categorical tokens.
            let audit_id = Uuid::now_v7();
            let inserted = sqlx::query(
                r#"
                INSERT INTO audit_events
                    (id, group_id, actor_user_id, actor_label, action,
                     resource_type, resource_id, metadata, created_at)
                SELECT $1, $2, NULL, 'system.migrate_workspace',
                       'chats.imported_from_sqlite', 'chat', $3::text,
                       jsonb_build_object(
                           'source', 'sessions',
                           'legacy_session_id', $4::text,
                           'chat_type', 'channel',
                           'channel_id', $5::text),
                       NOW()
                WHERE NOT EXISTS (
                    SELECT 1 FROM audit_events
                    WHERE action = 'chats.imported_from_sqlite'
                      AND resource_id = $3::text
                )
                "#,
            )
            .bind(audit_id)
            .bind(legacy_group_id)
            .bind(chat_id.to_string())
            .bind(&session.id)
            .bind(&session.channel_id)
            .execute(&mut *tx)
            .await
            .context("insert chats.imported_from_sqlite audit row")?
            .rows_affected();
            if inserted > 0 {
                report.chat_audit_events_inserted += 1;
            }
        }

        // chat_members — one row for the session's user, role='owner'.
        let member_res = sqlx::query(
            "INSERT INTO chat_members (chat_id, user_id, role, joined_at)
             VALUES ($1, $2, 'owner', $3)
             ON CONFLICT (chat_id, user_id) DO NOTHING",
        )
        .bind(chat_id)
        .bind(pg_user_id)
        .bind(session.created_at)
        .execute(&mut *tx)
        .await
        .context("insert chat_members row")?;

        if member_res.rows_affected() > 0 {
            report.chat_members_inserted += 1;

            // Audit: chat_members.imported_from_sqlite, resource_id
            // = `{chat_id}:{user_id}` (mirrors stage 3).
            let audit_id = Uuid::now_v7();
            let resource_id = format!("{chat_id}:{pg_user_id}");
            let inserted = sqlx::query(
                r#"
                INSERT INTO audit_events
                    (id, group_id, actor_user_id, actor_label, action,
                     resource_type, resource_id, metadata, created_at)
                SELECT $1, $2, NULL, 'system.migrate_workspace',
                       'chat_members.imported_from_sqlite',
                       'chat_member', $3::text,
                       jsonb_build_object(
                           'source', 'sessions',
                           'legacy_session_id', $4::text,
                           'role', 'owner',
                           'user_id', $5::text,
                           'chat_id', $6::text),
                       NOW()
                WHERE NOT EXISTS (
                    SELECT 1 FROM audit_events
                    WHERE action = 'chat_members.imported_from_sqlite'
                      AND resource_id = $3::text
                )
                "#,
            )
            .bind(audit_id)
            .bind(legacy_group_id)
            .bind(&resource_id)
            .bind(&session.id)
            .bind(pg_user_id.to_string())
            .bind(chat_id.to_string())
            .execute(&mut *tx)
            .await
            .context("insert chat_members.imported_from_sqlite audit row")?
            .rows_affected();
            if inserted > 0 {
                report.chat_audit_events_inserted += 1;
            }
        } else {
            report.chat_members_skipped_conflict += 1;
        }
    }

    info!(
        chats_new = report.chats_inserted,
        chats_existing = report.chats_skipped_conflict,
        members_new = report.chat_members_inserted,
        members_existing = report.chat_members_skipped_conflict,
        skipped_no_user = report.sessions_skipped_no_user,
        mapping_size = mapping.session_to_chat.len(),
        "stage 5 complete"
    );

    Ok(mapping)
}

/// Map common Postgres SQLSTATE codes to actionable CLI hints via
/// structured tracing. Currently narrow: only 23514 (CHECK violation)
/// is surfaced because the caller is most likely to mis-type
/// `--target-group-type`. The error is returned unchanged so the outer
/// `anyhow` chain still carries the original context plus Postgres
/// diagnostics for the CLI wrapper in `main.rs`.
fn map_sqlstate_error(err: sqlx::Error) -> sqlx::Error {
    if let sqlx::Error::Database(ref db_err) = err
        && db_err.code().as_deref() == Some("23514")
    {
        tracing::error!(
            target: "garraia_cli::migrate_workspace",
            pg_message = %db_err.message(),
            "CHECK constraint violated — verify --target-group-type is one of 'family', 'team', 'personal'"
        );
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
        // Stage 5 (plan 0045) counters.
        assert_eq!(r.chats_inserted, 0);
        assert_eq!(r.chats_skipped_conflict, 0);
        assert_eq!(r.chat_members_inserted, 0);
        assert_eq!(r.chat_members_skipped_conflict, 0);
        assert_eq!(r.chat_audit_events_inserted, 0);
        assert_eq!(r.sessions_skipped_no_user, 0);
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

    // ─── Plan 0045 — Stage 5 pure helpers ────────────────────────────

    #[test]
    fn chat_mapping_default_is_empty() {
        let m = ChatMapping::default();
        assert!(m.session_to_chat.is_empty());
        assert_eq!(m.session_to_chat.len(), 0);
    }

    #[test]
    fn session_name_uses_metadata_title_when_present() {
        let name = session_name_from_metadata(r#"{"title":"Hello"}"#, "mobile");
        assert_eq!(name, "Hello");
    }

    #[test]
    fn session_name_trims_whitespace_around_title() {
        let name = session_name_from_metadata(r#"{"title":"  Hello  "}"#, "mobile");
        assert_eq!(name, "Hello");
    }

    #[test]
    fn session_name_falls_back_to_channel_when_title_empty_string() {
        let name = session_name_from_metadata(r#"{"title":""}"#, "telegram");
        assert_eq!(name, "Chat telegram");
    }

    #[test]
    fn session_name_falls_back_to_channel_when_title_whitespace_only() {
        let name = session_name_from_metadata(r#"{"title":"   "}"#, "discord");
        assert_eq!(name, "Chat discord");
    }

    #[test]
    fn session_name_falls_back_to_channel_on_missing_title_key() {
        let name = session_name_from_metadata(r#"{"other":"x"}"#, "mobile");
        assert_eq!(name, "Chat mobile");
    }

    #[test]
    fn session_name_falls_back_to_channel_on_non_string_title() {
        let name = session_name_from_metadata(r#"{"title":42}"#, "mobile");
        assert_eq!(name, "Chat mobile");
    }

    #[test]
    fn session_name_falls_back_to_channel_on_empty_metadata_object() {
        let name = session_name_from_metadata("{}", "mobile");
        assert_eq!(name, "Chat mobile");
    }

    #[test]
    fn session_name_fails_closed_on_malformed_json() {
        // Not JSON at all → fallback chain.
        let name = session_name_from_metadata("not even json {[", "mobile");
        assert_eq!(name, "Chat mobile");
    }

    #[test]
    fn session_name_absolute_fallback_when_channel_empty() {
        let name = session_name_from_metadata("", "");
        assert_eq!(name, "Legacy chat");
    }

    #[test]
    fn session_name_absolute_fallback_when_channel_whitespace_only() {
        let name = session_name_from_metadata("not-json", "   ");
        assert_eq!(name, "Legacy chat");
    }

    #[test]
    fn session_name_title_wins_over_channel_even_when_both_present() {
        let name = session_name_from_metadata(r#"{"title":"Project Apollo"}"#, "mobile");
        assert_eq!(name, "Project Apollo");
    }

    #[test]
    fn session_name_fail_closed_never_panics_on_pathological_inputs() {
        // Ensure zero panics on weird inputs that a legacy SQLite
        // might contain (empty strings, unicode, deeply nested JSON).
        let _ = session_name_from_metadata("", "mobile");
        let _ = session_name_from_metadata(r#"{"title":"🎉 αβγ"}"#, "mobile");
        let _ = session_name_from_metadata(
            r#"{"title": null, "other": {"nested": [1, 2, 3]}}"#,
            "mobile",
        );
        let _ = session_name_from_metadata("\0\u{FFFE}", "mobile");
    }
}
