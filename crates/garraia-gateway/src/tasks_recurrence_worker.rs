//! Workspace task recurrence worker.
//!
//! `tasks.recurrence_rrule` has existed since migration 006 with a charset
//! CHECK and an explicit "future recurrence engine" note — nothing ever
//! expanded it. This worker closes that gap: when a recurring task is
//! completed, it materialises the next occurrence.
//!
//! ## Concurrency
//!
//! Several gateway replicas may share one Postgres, so each tick is guarded
//! by `pg_try_advisory_lock(hashtext('task_recurrence_sweep'))`. Losing the
//! lock is the expected outcome on all but one replica and skips the tick.
//!
//! ## Bypass-RLS / cross-tenant maintenance
//!
//! `tasks` is under FORCE RLS (fail-closed without `app.current_group_id`),
//! so the sweep runs through the `due_task_recurrences` SECURITY DEFINER
//! function (migration 033), exactly like the uploads worker uses
//! `expire_tus_uploads_sweep`. Reads are cross-tenant; every **write** is
//! done with `app.current_group_id` set to the row's own group, so the
//! normal RLS `WITH CHECK` still applies to the insert.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use garraia_auth::AppPool;
use garraia_workspace::recurrence;
use sqlx::Row;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// `hashtext('task_recurrence_sweep')` mapeia para um i32 estavel que o
/// Postgres usa como chave do lock. Literal estatico de proposito: o sqlx 0.9
/// so aceita `&'static str` em `query_scalar` (trait `SqlSafeStr`), e manter
/// lock e unlock no MESMO literal evita que os dois divergam.
const LOCK_SQL: &str = "SELECT pg_try_advisory_lock(hashtext('task_recurrence_sweep'))";
const UNLOCK_SQL: &str = "SELECT pg_advisory_unlock(hashtext('task_recurrence_sweep'))";

/// Configuration envelope for [`spawn_task_recurrence_worker`].
#[derive(Debug, Clone)]
pub struct TaskRecurrenceWorkerConfig {
    /// How long to wait between sweeps.
    pub interval: Duration,
    /// Max tasks materialised per sweep, bounding the advisory-lock hold.
    pub batch_size: i64,
}

impl Default for TaskRecurrenceWorkerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            batch_size: 128,
        }
    }
}

/// Outcome of one sweep tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecurrenceTickReport {
    /// Next occurrences created.
    pub spawned: u64,
    /// Rules that ran out (UNTIL/COUNT) — task closed, nothing spawned.
    pub exhausted: u64,
    /// Rules rejected by the parser; the task is marked so it is not retried
    /// forever.
    pub invalid: u64,
}

/// One candidate returned by the SECURITY DEFINER sweep.
struct DueRecurrence {
    id: Uuid,
    group_id: Uuid,
    list_id: Uuid,
    title: String,
    description_md: Option<String>,
    priority: String,
    due_at: Option<DateTime<Utc>>,
    rrule: String,
    tz: Option<String>,
    created_by: Option<Uuid>,
    created_by_label: String,
}

/// Run one sweep. Returns a report of the batch.
///
/// O advisory lock no Postgres e **por sessao**, entao a aquisicao e a
/// liberacao precisam cair na MESMA conexao: este tick fixa uma
/// `PoolConnection` do acquire ao unlock (mesmo padrao de
/// [`crate::account_purge_worker`] e [`crate::uploads_worker`]). Fazer o
/// unlock numa conexao diferente do pool e um no-op que vaza o lock ate o
/// backend morrer, e como o pool mantem sessoes vivas, ate o processo
/// morrer — ver #1611.
pub async fn run_recurrence_tick(
    pool: Arc<AppPool>,
    batch_size: i64,
) -> Result<RecurrenceTickReport, sqlx::Error> {
    let mut conn = pool.pool_for_handlers().acquire().await?;

    let got_lock: bool = sqlx::query_scalar(LOCK_SQL).fetch_one(&mut *conn).await?;
    if !got_lock {
        debug!("task_recurrence_worker: another replica holds the lock; skipping tick");
        // Sem lock nesta sessao, devolver a conexao ao pool e inofensivo.
        return Ok(RecurrenceTickReport::default());
    }

    // A partir daqui esta sessao detem o lock. Todo caminho de saida passa
    // pela liberacao abaixo — inclusive o `?` interno, que escapa com o erro
    // ANTES do unlock.
    let result = locked_tick(&mut conn, pool.as_ref(), batch_size).await;

    // Liberar na MESMA sessao que pegou. `false` significa "esta sessao nao
    // tem o lock", o que nunca deveria acontecer; tratar como falha e fechar
    // a conexao, que mata o backend e solta qualquer lock esquecido junto.
    let released: bool = sqlx::query_scalar(UNLOCK_SQL)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(false);

    if !released {
        warn!(
            "task_recurrence_worker: advisory unlock failed; closing the pinned \
             connection so Postgres releases the lock with the backend"
        );
        // Um erro aqui nao muda o desfecho: o que importa e matar a sessao,
        // e o drop de `conn` fecha o backend tambem se este close falhar.
        let _ = conn.close().await;
    }

    result
}

/// O trabalho do lote, com o lock ja assumido e sobre a conexao fixada.
async fn locked_tick(
    conn: &mut sqlx::PgConnection,
    pool: &AppPool,
    batch_size: i64,
) -> Result<RecurrenceTickReport, sqlx::Error> {
    let now = Utc::now();
    // `p_limit` da funcao (migracao 033) e `int`, e o Postgres nao faz cast
    // implicito de `bigint` na resolucao de funcao — vincular o `i64` cru faz
    // a chamada falhar com "function due_task_recurrences(timestamptz, bigint)
    // does not exist", ou seja, TODA varredura morria antes de varrer. Mesmo
    // cast do [`crate::uploads_worker`] contra `expire_tus_uploads_sweep`.
    let rows = sqlx::query("SELECT * FROM due_task_recurrences($1, $2)")
        .bind(now)
        .bind(batch_size as i32)
        .fetch_all(&mut *conn)
        .await?;

    let mut report = RecurrenceTickReport::default();
    for row in rows {
        let candidate = DueRecurrence {
            id: row.try_get("id")?,
            group_id: row.try_get("group_id")?,
            list_id: row.try_get("list_id")?,
            title: row.try_get("title")?,
            description_md: row.try_get("description_md")?,
            priority: row.try_get("priority")?,
            due_at: row.try_get("due_at")?,
            rrule: row.try_get("recurrence_rrule")?,
            tz: row.try_get("recurrence_tz")?,
            created_by: row.try_get("created_by")?,
            created_by_label: row.try_get("created_by_label")?,
        };

        match next_due(&candidate, now) {
            Ok(Some(next_due_at)) => {
                spawn_next_occurrence(pool, &candidate, next_due_at).await?;
                report.spawned += 1;
            }
            Ok(None) => {
                // UNTIL/COUNT exhausted: close the series without spawning.
                mark_spawned(pool, candidate.group_id, candidate.id, now).await?;
                report.exhausted += 1;
            }
            Err(e) => {
                // A rule the parser rejects would otherwise be retried on
                // every tick forever. Mark it and move on, loudly.
                warn!(
                    task = %candidate.id,
                    rrule = %candidate.rrule,
                    "task_recurrence_worker: unusable recurrence rule: {e}"
                );
                mark_spawned(pool, candidate.group_id, candidate.id, now).await?;
                report.invalid += 1;
            }
        }
    }

    if report != RecurrenceTickReport::default() {
        info!(
            spawned = report.spawned,
            exhausted = report.exhausted,
            invalid = report.invalid,
            "task_recurrence_worker: tick complete"
        );
    }
    Ok(report)
}

/// Compute the next due date for a candidate. Pure except for the parse.
fn next_due(
    candidate: &DueRecurrence,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, garraia_workspace::error::WorkspaceError> {
    let tz = recurrence::parse_timezone(candidate.tz.as_deref())?;
    // A recurring task without due_at anchors on "now": the series has no
    // meaningful calendar origin, so the next occurrence is relative.
    let anchor = candidate.due_at.unwrap_or(now);
    recurrence::next_after_run(&candidate.rrule, tz, anchor, now, anchor)
}

/// Insert the next occurrence and mark the source task as materialised,
/// atomically and under the row's own tenant context.
async fn spawn_next_occurrence(
    pool: &AppPool,
    candidate: &DueRecurrence,
    next_due_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.pool_for_handlers().begin().await?;
    set_tenant(&mut tx, candidate.group_id).await?;

    sqlx::query(
        "INSERT INTO tasks \
             (list_id, group_id, title, description_md, status, priority, due_at, \
              recurrence_rrule, recurrence_tz, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, 'todo', $5, $6, $7, $8, $9, $10)",
    )
    .bind(candidate.list_id)
    .bind(candidate.group_id)
    .bind(&candidate.title)
    .bind(&candidate.description_md)
    .bind(&candidate.priority)
    .bind(next_due_at)
    .bind(&candidate.rrule)
    .bind(&candidate.tz)
    .bind(candidate.created_by)
    .bind(&candidate.created_by_label)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE tasks SET recurrence_spawned_at = $2 WHERE id = $1")
        .bind(candidate.id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await?;

    tx.commit().await
}

/// Close a series without creating a follow-up occurrence.
async fn mark_spawned(
    pool: &AppPool,
    group_id: Uuid,
    task_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.pool_for_handlers().begin().await?;
    set_tenant(&mut tx, group_id).await?;
    sqlx::query("UPDATE tasks SET recurrence_spawned_at = $2 WHERE id = $1")
        .bind(task_id)
        .bind(at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

async fn set_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    group_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Spawn the periodic sweep loop.
pub fn spawn_task_recurrence_worker(
    pool: Arc<AppPool>,
    config: TaskRecurrenceWorkerConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            interval_secs = config.interval.as_secs(),
            batch_size = config.batch_size,
            "task_recurrence_worker: starting"
        );
        let mut ticker = tokio::time::interval(config.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick so bootstrap finishes first.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = run_recurrence_tick(pool.clone(), config.batch_size).await {
                warn!(error = %e, "task_recurrence_worker: tick failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(rrule: &str, tz: Option<&str>, due: Option<DateTime<Utc>>) -> DueRecurrence {
        DueRecurrence {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            list_id: Uuid::nil(),
            title: "t".into(),
            description_md: None,
            priority: "none".into(),
            due_at: due,
            rrule: rrule.into(),
            tz: tz.map(str::to_string),
            created_by: None,
            created_by_label: "tester".into(),
        }
    }

    #[test]
    fn worker_defaults_are_conservative() {
        let cfg = TaskRecurrenceWorkerConfig::default();
        assert_eq!(cfg.interval, Duration::from_secs(60));
        assert_eq!(cfg.batch_size, 128);
    }

    #[test]
    fn next_due_advances_from_the_task_due_date() {
        use chrono::TimeZone;
        let due = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 9, 5, 0).unwrap();
        let next = next_due(&candidate("FREQ=DAILY", Some("UTC"), Some(due)), now)
            .unwrap()
            .unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 2, 9, 0, 0).unwrap());
    }

    #[test]
    fn exhausted_rule_yields_none_so_the_series_closes() {
        use chrono::TimeZone;
        let due = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 6, 5, 0, 0, 0).unwrap();
        let got = next_due(
            &candidate("FREQ=DAILY;UNTIL=20260602T090000Z", Some("UTC"), Some(due)),
            now,
        )
        .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn invalid_rule_is_an_error_not_a_panic() {
        use chrono::TimeZone;
        let due = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
        assert!(next_due(&candidate("TOTALLY BOGUS", None, Some(due)), due).is_err());
    }
}
