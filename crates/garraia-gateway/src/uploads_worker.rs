//! `tus_uploads` expiration worker — plan 0047 (GAR-395 slice 3).
//!
//! Periodically transitions `tus_uploads.status` from `in_progress` to
//! `expired` when `expires_at < now()`, emits one audit row per expired
//! upload via [`garraia_auth::audit_workspace_event`], and removes the
//! per-upload staging file best-effort.
//!
//! ## Concurrency
//!
//! Multiple gateway replicas may share one Postgres. The worker guards
//! its batched UPDATE with
//! `pg_try_advisory_lock(hashtext('tus_uploads_expiration'))` — whichever
//! replica wins the lock runs the sweep, the others skip the tick.
//! Failure to acquire the lock is expected and silently skipped.
//!
//! ## Bypass-RLS
//!
//! The worker needs to see rows across all `group_id` values — it runs
//! on the `AppPool` (`garraia_app` role) BUT explicitly sets the group
//! context to each row's own `group_id` before emitting the audit.
//! Alternative: use a future `BYPASSRLS` worker role. For v1 we read +
//! update via a maintenance SQL path that does not rely on
//! `app.current_group_id` being set — the RLS policy on `tus_uploads`
//! filters by that GUC, so we set the GUC **per-row** for the UPDATE
//! to succeed. This is cleaner than adding another bypass role for a
//! single maintenance path.
//!
//! ## Tick cadence
//!
//! Default 5-minute interval. Configurable via
//! [`UploadsExpirationWorkerConfig::interval`]. A single tick locks for
//! at most `batch_size` rows (default 256) to keep the advisory lock
//! hold time bounded.

use std::sync::Arc;
use std::time::Duration;

use garraia_auth::{AppPool, WorkspaceAuditAction, audit_workspace_event};
use serde_json::json;
use sqlx::Row;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Configuration envelope for [`spawn_uploads_expiration_worker`].
#[derive(Debug, Clone)]
pub struct UploadsExpirationWorkerConfig {
    /// How long to wait between sweeps.
    pub interval: Duration,
    /// Max rows transitioned per sweep. Keeps the advisory lock hold
    /// time bounded.
    pub batch_size: i64,
}

impl Default for UploadsExpirationWorkerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5 * 60),
            batch_size: 256,
        }
    }
}

/// Outcome of one sweep tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    pub expired_count: u64,
    pub staging_removed: u64,
    pub staging_missing: u64,
    pub audit_failed: u64,
}

/// Run one sweep against `pool`. Returns a report of the batch.
///
/// Caller is responsible for not invoking this concurrently from the
/// same process — the advisory lock protects across processes but a
/// single process that spawns two tickers would block itself.
pub async fn run_expiration_tick(
    pool: Arc<AppPool>,
    staging_dir: Option<&std::path::Path>,
    batch_size: i64,
) -> Result<TickReport, sqlx::Error> {
    // Acquire the advisory lock — skip the whole tick when contended.
    // `hashtext('tus_uploads_expiration')` maps to a stable i32 that
    // Postgres uses as the lock key. A missing lock is `false`, not
    // an error.
    let pg = pool.pool_for_handlers();
    let got_lock: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('tus_uploads_expiration'))")
            .fetch_one(pg)
            .await?;

    if !got_lock {
        debug!("uploads_expiration_worker: lock contended; skipping tick");
        return Ok(TickReport::default());
    }

    // Guard ensures release even if downstream code panics / errors.
    let release_guard = AdvisoryLockGuard::new(pool.clone());

    let mut report = TickReport::default();

    // Sweep in one `UPDATE ... RETURNING`, limited to `batch_size`.
    // We can't combine RETURNING with LIMIT directly — use a CTE with
    // SELECT ... FOR UPDATE SKIP LOCKED + LIMIT to pick the batch, then
    // UPDATE the fetched ids.
    let rows = sqlx::query(
        "WITH victim AS (
            SELECT id
            FROM tus_uploads
            WHERE status = 'in_progress' AND expires_at < now()
            ORDER BY expires_at ASC
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE tus_uploads AS t
        SET status = 'expired', updated_at = now()
        FROM victim
        WHERE t.id = victim.id
        RETURNING t.id, t.group_id, t.created_by, t.object_key,
                  t.upload_offset, t.upload_length,
                  EXTRACT(EPOCH FROM (now() - t.created_at))::bigint AS age_secs",
    )
    .bind(batch_size)
    .fetch_all(pg)
    .await?;

    for row in &rows {
        let upload_id: Uuid = row.try_get("id")?;
        let group_id: Uuid = row.try_get("group_id")?;
        let created_by: Uuid = row.try_get("created_by")?;
        let object_key: String = row.try_get("object_key")?;
        let upload_offset: i64 = row.try_get("upload_offset")?;
        let upload_length: i64 = row.try_get("upload_length")?;
        let age_secs: i64 = row.try_get("age_secs")?;

        report.expired_count += 1;

        // Best-effort staging cleanup. Missing file = already gone.
        // Filename pattern mirrors `UploadStaging::staging_path` in
        // `rest_v1::uploads` (plan 0044 §5.2) — `{upload_id}.staging`.
        if let Some(dir) = staging_dir {
            let staging_path = dir.join(format!("{upload_id}.staging"));
            match tokio::fs::remove_file(&staging_path).await {
                Ok(()) => report.staging_removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    report.staging_missing += 1;
                }
                Err(e) => {
                    warn!(
                        upload_id = %upload_id,
                        error = %e,
                        "uploads_expiration_worker: failed to remove staging file"
                    );
                }
            }
        }

        // Emit audit row in its own transaction. Per-row SET LOCAL of
        // `app.current_group_id` so the RLS policy on audit_events
        // allows the INSERT.
        if let Err(e) = emit_expiration_audit(
            pool.as_ref(),
            group_id,
            created_by,
            upload_id,
            &object_key,
            upload_offset,
            upload_length,
            age_secs,
        )
        .await
        {
            report.audit_failed += 1;
            warn!(
                upload_id = %upload_id,
                error = %e,
                "uploads_expiration_worker: audit insert failed"
            );
        }
    }

    if report.expired_count > 0 {
        info!(
            expired = report.expired_count,
            staging_removed = report.staging_removed,
            staging_missing = report.staging_missing,
            audit_failed = report.audit_failed,
            "uploads_expiration_worker: tick complete"
        );
    }

    drop(release_guard); // explicit

    Ok(report)
}

async fn emit_expiration_audit(
    pool: &AppPool,
    group_id: Uuid,
    actor_user_id: Uuid,
    upload_id: Uuid,
    object_key: &str,
    upload_offset: i64,
    upload_length: i64,
    age_secs: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.pool_for_handlers().begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{actor_user_id}'"
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!("SET LOCAL app.current_group_id = '{group_id}'"))
        .execute(&mut *tx)
        .await?;

    let object_key_hash = crate::uploads_worker_util::sha256_hex_of(object_key.as_bytes());
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::UploadExpired,
        actor_user_id,
        group_id,
        "tus_uploads",
        upload_id.to_string(),
        json!({
            "upload_offset": upload_offset,
            "upload_length": upload_length,
            "age_secs": age_secs,
            "object_key_hash": object_key_hash,
        }),
    )
    .await
    .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))?;

    tx.commit().await?;
    Ok(())
}

struct AdvisoryLockGuard {
    pool: Arc<AppPool>,
    released: bool,
}

impl AdvisoryLockGuard {
    fn new(pool: Arc<AppPool>) -> Self {
        Self {
            pool,
            released: false,
        }
    }
}

impl Drop for AdvisoryLockGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("SELECT pg_advisory_unlock(hashtext('tus_uploads_expiration'))")
                .execute(pool.pool_for_handlers())
                .await;
        });
    }
}

/// Spawn the periodic sweep loop. Returns the `JoinHandle` for the
/// caller to keep alive for the process lifetime (or `abort` on
/// shutdown — not wired in v1).
pub fn spawn_uploads_expiration_worker(
    pool: Arc<AppPool>,
    staging_dir: Option<std::path::PathBuf>,
    config: UploadsExpirationWorkerConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            interval_secs = config.interval.as_secs(),
            batch_size = config.batch_size,
            "uploads_expiration_worker: starting"
        );
        let mut ticker = tokio::time::interval(config.interval);
        // First tick fires immediately — skip it so the gateway has
        // room to finish bootstrap before we touch the DB.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match run_expiration_tick(pool.clone(), staging_dir.as_deref(), config.batch_size).await
            {
                Ok(report) => {
                    if report.expired_count == 0 && report.audit_failed == 0 {
                        debug!("uploads_expiration_worker: idle tick");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "uploads_expiration_worker: tick failed");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_report_default_is_zeroed() {
        let r = TickReport::default();
        assert_eq!(r.expired_count, 0);
        assert_eq!(r.staging_removed, 0);
        assert_eq!(r.staging_missing, 0);
        assert_eq!(r.audit_failed, 0);
    }

    #[test]
    fn default_config_is_five_minutes_batch_256() {
        let c = UploadsExpirationWorkerConfig::default();
        assert_eq!(c.interval, Duration::from_secs(300));
        assert_eq!(c.batch_size, 256);
    }

    #[test]
    fn config_overrides_preserved() {
        let c = UploadsExpirationWorkerConfig {
            interval: Duration::from_secs(42),
            batch_size: 7,
        };
        assert_eq!(c.interval.as_secs(), 42);
        assert_eq!(c.batch_size, 7);
    }
}
