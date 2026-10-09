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
//! O lock e **por sessao**, entao a aquisicao e a liberacao precisam cair na
//! MESMA conexao: o tick fixa uma `PoolConnection` do acquire ao unlock
//! (mesmo padrao de [`crate::account_purge_worker`]). Fazer o unlock numa
//! conexao diferente do pool e um no-op que vaza o lock ate o backend morrer,
//! e como o pool mantem sessoes vivas, ate o processo morrer — ver #1611.
//!
//! ## Bypass-RLS / Cross-Tenant Maintenance
//!
//! The worker needs to see rows across all `group_id` values — it runs
//! on the `AppPool` (`garraia_app` role). Because `tus_uploads` is under
//! FORCE RLS (`tus_uploads_group_isolation`), direct queries with no
//! tenant GUC set return 0 rows (fail-closed).
//! The sweep is executed via the `expire_tus_uploads_sweep(p_now, p_limit)`
//! `SECURITY DEFINER` function (migration 032), which runs with creator
//! privileges, bypasses RLS safely, and returns the expired records.
//! Audit emission then sets `app.current_group_id` per-row for inserting
//! into `audit_events`.
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

/// `hashtext('tus_uploads_expiration')` mapeia para um i32 estavel que o
/// Postgres usa como chave do lock. Literal estatico de proposito: o sqlx 0.9
/// so aceita `&'static str` em `query_scalar` (trait `SqlSafeStr`), e manter
/// lock e unlock no MESMO literal evita que os dois divergam.
const LOCK_SQL: &str = "SELECT pg_try_advisory_lock(hashtext('tus_uploads_expiration'))";
const UNLOCK_SQL: &str = "SELECT pg_advisory_unlock(hashtext('tus_uploads_expiration'))";

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
    // Adquire UMA conexao e fixa ela para o tick inteiro. O advisory lock no
    // Postgres e **por sessao**: so a sessao que chamou `pg_try_advisory_lock`
    // pode chamar `pg_advisory_unlock` com efeito. Devolver a conexao ao pool
    // nao encerra a sessao — o sqlx nao reseta estado de sessao na devolucao
    // (sem `DISCARD ALL` no `sqlx-postgres` 0.9, e o `ping()` do
    // `test_before_acquire` e so `write_sync`) — entao qualquer unlock feito
    // em OUTRA conexao do pool e um no-op silencioso que vaza o lock ate o
    // backend morrer. Ver #1611.
    let mut conn = pool.pool_for_handlers().acquire().await?;

    // Acquire the advisory lock — skip the whole tick when contended.
    // `hashtext('tus_uploads_expiration')` maps to a stable i32 that
    // Postgres uses as the lock key. A missing lock is `false`, not
    // an error.
    let got_lock: bool = sqlx::query_scalar(LOCK_SQL).fetch_one(&mut *conn).await?;

    if !got_lock {
        debug!("uploads_expiration_worker: lock contended; skipping tick");
        // Sem lock nesta sessao, devolver a conexao ao pool e inofensivo.
        return Ok(TickReport::default());
    }

    // A partir daqui esta sessao detem o lock. Todo caminho de saida passa
    // pela liberacao abaixo — inclusive o `?` interno, que escapa com o erro
    // ANTES do unlock.
    let result = locked_tick(&mut conn, pool.as_ref(), staging_dir, batch_size).await;

    // Liberar na MESMA sessao que pegou. `pg_advisory_unlock` devolve
    // `false` quando o lock nao esta nesta sessao (nunca deveria acontecer);
    // tratar como falha e fechar a conexao, que mata o backend e solta
    // qualquer lock esquecido junto com ele.
    let released: bool = sqlx::query_scalar(UNLOCK_SQL)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(false);

    if !released {
        warn!(
            "uploads_expiration_worker: advisory unlock failed; closing the pinned \
             connection so Postgres releases the lock with the backend"
        );
        // Um erro aqui nao muda o desfecho: o que importa e matar a sessao,
        // e o drop de `conn` fecha o backend tambem se este close falhar.
        let _ = conn.close().await;
    }

    result
}

/// O trabalho do lote, com o lock ja assumido e sobre a conexao fixada.
///
/// `conn` executa a varredura na MESMA sessao do lock, para que o lock e o
/// trabalho andem juntos: se esta sessao morrer no meio, o lock cai junto e a
/// retomada e feita pelo proximo tick que conseguir o lock.
async fn locked_tick(
    conn: &mut sqlx::PgConnection,
    pool: &AppPool,
    staging_dir: Option<&std::path::Path>,
    batch_size: i64,
) -> Result<TickReport, sqlx::Error> {
    let mut report = TickReport::default();

    // Sweep in one call to the `SECURITY DEFINER` function `expire_tus_uploads_sweep`,
    // limited to `batch_size`. This safely bypasses FORCE RLS on `tus_uploads`
    // to perform the cross-tenant expiration sweep.
    let rows = sqlx::query(
        "SELECT id, group_id, created_by, object_key, \
                upload_offset, upload_length, age_secs \
         FROM expire_tus_uploads_sweep(now(), $1)",
    )
    .bind(batch_size as i32)
    .fetch_all(&mut *conn)
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
            pool,
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
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(actor_user_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
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
