//! `garraia migrate workspace` — SQLite → Postgres migration (Stage 1).
//!
//! Plan 0039 — implements §7.1 (users) + §7.2 (user_identities + atomic
//! audit) of [plan 0034](../../plans/0034-gar-413-migrate-workspace-spec.md).
//! Subsequent stages (groups, chats, messages, memory, sessions, api_keys,
//! audit retrofit) land in follow-up slices.
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
//!
//! # Rollback
//!
//! Reversible by `git revert` of this module's commit (pure addition).
//! Operator-side rollback of migrated data:
//!
//! ```sql
//! DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL;
//! -- Cascades to user_identities (FK ON DELETE CASCADE).
//! -- audit_events rows persist by design (plan 0034 §8, LGPD art. 37).
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use rusqlite::OptionalExtension;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::{info, instrument};

/// Subset of sysexits.h codes used by `migrate workspace`. Kept local to
/// the module so the CLI entrypoint can `std::process::exit` without
/// guessing magic numbers.
#[allow(dead_code)] // DATA_ERR reserved for future use (stages 3+)
pub mod exit_codes {
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
#[derive(Debug, Default)]
pub struct Stage1Report {
    pub users_inserted: u64,
    pub users_upserted: u64,
    pub identities_inserted: u64,
    pub identities_skipped_conflict: u64,
    pub audit_events_inserted: u64,
    pub dry_run: bool,
}

impl Stage1Report {
    pub fn print_summary(&self) {
        let mode = if self.dry_run { " (dry run)" } else { "" };
        println!("Workspace Migration Report — Stage 1{mode}");
        println!("──────────────────────────────────────");
        println!(
            "  users:       {} inserted, {} upserted-on-existing",
            self.users_inserted, self.users_upserted
        );
        println!(
            "  identities:  {} inserted, {} skipped (conflict)",
            self.identities_inserted, self.identities_skipped_conflict
        );
        println!("  audit rows:  {}", self.audit_events_inserted);
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
/// * `dry_run` — when `true`, rolls back the transaction at the end.
/// * `confirm_backup` — gate against running against a non-empty Postgres
///   without explicit operator opt-in.
#[instrument(
    name = "migrate_workspace.run",
    skip(postgres_url, sqlite_path),
    fields(dry_run = dry_run, confirm_backup = confirm_backup)
)]
pub async fn run(
    sqlite_path: &Path,
    postgres_url: &str,
    dry_run: bool,
    confirm_backup: bool,
) -> Result<(Stage1Report, i32)> {
    if let Some(code) = preflight_sqlite(sqlite_path)? {
        return Ok((Stage1Report::default(), code));
    }

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(postgres_url)
        .await
        .context("connect to Postgres")?;

    if let Some(code) = preflight_bypassrls(&pool).await? {
        return Ok((Stage1Report::default(), code));
    }
    if let Some(code) = preflight_schema_version(&pool).await? {
        return Ok((Stage1Report::default(), code));
    }
    if let Some(code) = preflight_confirmation_gate(&pool, dry_run, confirm_backup).await? {
        return Ok((Stage1Report::default(), code));
    }

    let legacy_rows = load_mobile_users(sqlite_path)?;
    info!(
        count = legacy_rows.len(),
        "loaded mobile_users rows from SQLite"
    );

    let mut report = Stage1Report {
        dry_run,
        ..Default::default()
    };

    let mut tx = pool.begin().await.context("begin stage1 tx")?;

    // SEC-H-1: re-check BYPASSRLS inside the first data tx on the same
    // connection pool (PgPool with max_connections=1 keeps us on the
    // same connection).
    if !bypass_rls_or_super(&mut *tx).await? {
        return Ok((Stage1Report::default(), exit_codes::NO_USER));
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

    if dry_run {
        tx.rollback().await.context("rollback dry-run tx")?;
        info!("dry run: rolled back; no rows persisted");
    } else {
        tx.commit().await.context("commit stage1 tx")?;
    }

    Ok((report, exit_codes::OK))
}

#[derive(Debug, Clone)]
struct MobileUserRow {
    id: String,
    email: String,
    password_hash_b64: String,
    salt_b64: String,
    /// SQLite stores created_at as TEXT in ISO 8601. Passed to Postgres
    /// as `$N::timestamptz`; if parsing fails Postgres raises and the
    /// stage tx rolls back — the operator gets DATA_ERR.
    #[allow(dead_code)] // read via bind in insert_user
    created_at: String,
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
    let bypass: bool = row.try_get("rolbypassrls").unwrap_or(false);
    let sup: bool = row.try_get("rolsuper").unwrap_or(false);
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
        .map(|r| r.try_get::<i64, _>("version").unwrap_or(0))
        .collect();
    let missing: Vec<&str> = REQUIRED_MIGRATIONS
        .iter()
        .copied()
        .filter(|m| {
            let n: i64 = m.parse().unwrap_or(-1);
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
    let n: i64 = row.try_get("n").unwrap_or(0);
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
            Ok(MobileUserRow {
                id: r.get(0)?,
                email: r.get(1)?,
                password_hash_b64: r.get(2)?,
                salt_b64: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .context("query mobile_users")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("fetch mobile_users row")?);
    }
    Ok(out)
}

/// Returns `true` if the row existed (UPSERT updated `legacy_sqlite_id`),
/// `false` if a fresh row was inserted.
async fn insert_user(tx: &mut sqlx::PgConnection, row: &MobileUserRow) -> Result<bool> {
    let display = row
        .email
        .split_once('@')
        .map(|(local, _)| local.to_string())
        .unwrap_or_else(|| row.email.clone());
    let result = sqlx::query(
        r#"
        INSERT INTO users (id, email, display_name, status, legacy_sqlite_id, created_at, updated_at)
        VALUES (uuid_generate_v7(), LOWER($1), $2, 'active', $3, $4::timestamptz, $4::timestamptz)
        ON CONFLICT (email) DO UPDATE SET
            legacy_sqlite_id = COALESCE(users.legacy_sqlite_id, EXCLUDED.legacy_sqlite_id)
        RETURNING (xmax = 0) AS inserted
        "#,
    )
    .bind(&row.email)
    .bind(&display)
    .bind(&row.id)
    .bind(&row.created_at)
    .fetch_one(&mut *tx)
    .await
    .context("insert_user")?;
    let inserted: bool = result.try_get("inserted").unwrap_or(true);
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

    let id_inserted = sqlx::query(
        r#"
        INSERT INTO user_identities
            (id, user_id, provider, provider_sub, password_hash, created_at, hash_upgraded_at)
        VALUES
            (uuid_generate_v7(), $1, 'internal', $2, $3, NOW(), NULL)
        ON CONFLICT (provider, provider_sub) DO NOTHING
        "#,
    )
    .bind(user_uuid)
    .bind(user_uuid.to_string())
    .bind(phc)
    .execute(&mut *tx)
    .await
    .context("insert user_identities")?
    .rows_affected()
        > 0;

    let audit_inserted = sqlx::query(
        r#"
        INSERT INTO audit_events
            (id, group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata, created_at)
        SELECT uuid_generate_v7(), NULL, NULL, 'system.migrate_workspace',
               'users.imported_from_sqlite', 'user', $1::text,
               jsonb_build_object(
                   'source', 'mobile_users',
                   'legacy_id', $2::text,
                   'hash_algorithm', 'pbkdf2-sha256',
                   'iterations', 600000,
                   'lazy_upgrade_pending', true),
               NOW()
        WHERE NOT EXISTS (
            SELECT 1 FROM audit_events
            WHERE action = 'users.imported_from_sqlite'
              AND resource_id = $1::text
        )
        "#,
    )
    .bind(user_uuid)
    .bind(&row.id)
    .execute(&mut *tx)
    .await
    .context("insert audit_events")?
    .rows_affected()
        > 0;

    Ok((id_inserted, audit_inserted))
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

/// Format the resolved SQLite path for error messages; keep separate to
/// avoid accidental String allocations in hot paths.
#[allow(dead_code)]
pub(crate) fn sqlite_display(path: &Path) -> PathBuf {
    path.to_path_buf()
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
    fn stage1_report_default_is_zero_and_dry_run_false() {
        let r = Stage1Report::default();
        assert_eq!(r.users_inserted, 0);
        assert_eq!(r.identities_inserted, 0);
        assert_eq!(r.audit_events_inserted, 0);
        assert!(!r.dry_run);
    }
}
