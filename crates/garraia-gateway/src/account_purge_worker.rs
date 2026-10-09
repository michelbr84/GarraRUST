//! Worker de apagamento definitivo de conta — o "worker futuro" que o
//! `DELETE /v1/me` prometia (LGPD art. 18, VI / GDPR art. 17).
//!
//! `DELETE /v1/me` sempre fez o soft-delete (`users.status = 'deleted'`) e
//! revogou as sessoes; o apagamento de verdade ficava "deferido". Agora ele
//! enfileira um pedido em `account_deletion_requests` com uma carencia
//! (`purge_after`), e este worker executa quando a carencia vence.
//!
//! ## Por que carencia e nao apagamento imediato
//!
//! Apagamento irreversivel imediato transforma um token de acesso roubado em
//! arma: o invasor destroi a conta e nada pode ser recuperado. A carencia
//! (default [`DEFAULT_GRACE_PERIOD_DAYS`] dias) da ao titular legitimo tempo
//! de reverter pelo canal de suporte e ao operador tempo de aplicar retencao
//! legal antes do ponto sem volta.
//!
//! ## Concorrencia
//!
//! Igual ao `uploads_worker` do plan 0047: varias replicas podem dividir um
//! Postgres, e a varredura e guardada por
//! `pg_try_advisory_lock(hashtext('account_purge'))` — quem pegar o lock roda
//! o tick, as outras pulam. Nao pegar o lock e o caso esperado, nao erro.
//!
//! ## Bypass-RLS / manutencao cross-tenant
//!
//! O titular pode ter dado em vários grupos, e 32 tabelas estao sob FORCE
//! RLS com politica fail-closed — consulta direta no `garraia_app` sem GUC de
//! tenant devolve zero linhas. Por isso o trabalho vive em quatro funcoes
//! `SECURITY DEFINER` da migration 034 (mesmo desenho do
//! `expire_tus_uploads_sweep`, migration 032):
//!
//! | Funcao | Papel |
//! |---|---|
//! | `claim_account_purge` | reivindica o lote devido, marca `in_progress`, devolve o progresso ja feito |
//! | `purge_account_data`  | o apagamento em si, uma transacao, devolve contagem + `object_keys` |
//! | `finish_account_purge`| fecha como `completed` e mescla o relatorio final |
//! | `fail_account_purge`  | devolve para a fila (falha transitoria) ou marca `failed` |
//!
//! O `garraia_app` **nao** tem `UPDATE` em `account_deletion_requests` (grant
//! da migration 034), logo a maquina de estados so anda por essas funcoes.
//!
//! ## Retomada (o blob mora fora do banco)
//!
//! `purge_account_data` apaga as linhas de `files`/`file_versions`, e com elas
//! as `object_key`. Se o worker morresse entre o commit do SQL e a limpeza do
//! ObjectStore, o blob ficaria orfao para sempre — dado pessoal sobrevivendo a
//! um apagamento que o audit jura ter acontecido. Para fechar isso, a funcao
//! persiste as chaves em `purge_report.object_keys` **antes** de qualquer
//! DELETE, e `claim_account_purge` as devolve junto com a marca `db_purged`.
//! Um pedido travado em `in_progress` por mais de
//! [`AccountPurgeWorkerConfig::stale_after`] e reivindicado de novo e retoma
//! exatamente no passo que faltava.
//!
//! ## Sem ObjectStore configurado
//!
//! Se ha chaves para remover e nenhum `ObjectStore` esta ligado, o tick
//! **nao** fecha o pedido: chama `fail_account_purge` e emite
//! `account.purge_failed`. Fechar como `completed` com blobs vivos seria
//! declarar no audit um apagamento que nao ocorreu. Esgotadas as tentativas o
//! pedido vira `failed` e aparece para o operador — ver
//! `docs/legal/data-subject-requests.md`.

use std::sync::Arc;
use std::time::Duration;

use garraia_auth::{AppPool, WorkspaceAuditAction, audit_workspace_event};
use garraia_storage::ObjectStore;
use serde_json::json;
use sqlx::Row;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Carencia default entre o pedido (`DELETE /v1/me`) e o apagamento ficar
/// irreversivel. 30 dias e a pratica corrente do setor e o que a prosa de
/// `docs/legal/data-subject-requests.md` promete ao titular; mudar aqui exige
/// mudar o documento junto.
pub const DEFAULT_GRACE_PERIOD_DAYS: i64 = 30;

/// Chave do advisory lock. `hashtext` mapeia para um i32 estavel.
const PURGE_LOCK_KEY: &str = "account_purge";

/// Configuracao de [`spawn_account_purge_worker`].
#[derive(Debug, Clone)]
pub struct AccountPurgeWorkerConfig {
    /// Intervalo entre varreduras.
    pub interval: Duration,
    /// Maximo de pedidos processados por tick. Limita o tempo de posse do
    /// advisory lock.
    pub batch_size: i64,
    /// Tempo apos o qual um pedido `in_progress` e considerado abandonado
    /// (worker morto) e pode ser reivindicado de novo.
    pub stale_after: Duration,
    /// Tentativas antes de o pedido virar `failed` para o operador olhar.
    pub max_attempts: i32,
}

impl Default for AccountPurgeWorkerConfig {
    fn default() -> Self {
        Self {
            // Uma hora: a carencia e medida em dias, varrer de minuto em
            // minuto so gastaria conexao.
            interval: Duration::from_secs(60 * 60),
            batch_size: 32,
            stale_after: Duration::from_secs(60 * 60),
            max_attempts: 5,
        }
    }
}

/// Resultado de um tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeTickReport {
    /// Pedidos reivindicados neste tick.
    pub claimed: u64,
    /// Pedidos fechados como `completed`.
    pub purged: u64,
    /// Blobs removidos do ObjectStore.
    pub blobs_deleted: u64,
    /// Blobs que o ObjectStore recusou remover.
    pub blobs_failed: u64,
    /// Pedidos devolvidos para a fila ou marcados `failed`.
    pub failed: u64,
    /// Emissoes de audit que falharam (o apagamento ja aconteceu).
    pub audit_failed: u64,
}

/// Roda uma varredura contra `pool`. Devolve o relatorio do lote.
///
/// O chamador nao deve invocar isto concorrentemente do MESMO processo — o
/// advisory lock protege entre processos, mas dois tickers no mesmo processo
/// se bloqueariam.
pub async fn run_purge_tick(
    pool: Arc<AppPool>,
    object_store: Option<Arc<dyn ObjectStore>>,
    config: &AccountPurgeWorkerConfig,
) -> Result<PurgeTickReport, sqlx::Error> {
    let pg = pool.pool_for_handlers();

    let got_lock: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext($1))")
        .bind(PURGE_LOCK_KEY)
        .fetch_one(pg)
        .await?;

    if !got_lock {
        debug!("account_purge_worker: lock contended; skipping tick");
        return Ok(PurgeTickReport::default());
    }

    let release_guard = AdvisoryLockGuard::new(pool.clone());
    let mut report = PurgeTickReport::default();

    // Reivindica o lote devido. `stale_after` entra como interval para a
    // funcao retomar pedidos abandonados por um worker morto.
    let claimed = sqlx::query(
        "SELECT request_id, user_id, attempts, db_purged, object_keys \
         FROM claim_account_purge(now(), $1, make_interval(secs => $2))",
    )
    .bind(config.batch_size as i32)
    .bind(config.stale_after.as_secs() as f64)
    .fetch_all(pg)
    .await?;

    for row in &claimed {
        let request_id: Uuid = row.try_get("request_id")?;
        let user_id: Uuid = row.try_get("user_id")?;
        let attempts: i32 = row.try_get("attempts")?;
        let db_purged: bool = row.try_get("db_purged")?;
        let resume_keys: Vec<String> = row.try_get("object_keys")?;

        report.claimed += 1;

        match purge_one(
            pool.as_ref(),
            object_store.as_ref(),
            user_id,
            db_purged,
            resume_keys,
        )
        .await
        {
            Ok(outcome) => {
                report.blobs_deleted += outcome.blobs_deleted;
                report.blobs_failed += outcome.blobs_failed;

                let final_report = json!({
                    "db": outcome.db_report,
                    "blobs_deleted": outcome.blobs_deleted,
                    "blobs_failed": outcome.blobs_failed,
                    // Chaves zeradas: o que sobrou foi removido, nada
                    // pendente para uma retomada futura reprocessar.
                    "object_keys": Vec::<String>::new(),
                });

                let closed: bool = sqlx::query_scalar("SELECT finish_account_purge($1, $2)")
                    .bind(request_id)
                    .bind(&final_report)
                    .fetch_one(pg)
                    .await?;

                if !closed {
                    // Outra replica fechou o pedido entre o claim e aqui.
                    // Nada a fazer: o apagamento e idempotente.
                    debug!(
                        request_id = %request_id,
                        "account_purge_worker: request no longer in_progress; skipping close"
                    );
                    continue;
                }

                report.purged += 1;

                if let Err(e) = emit_purge_audit(
                    pool.as_ref(),
                    WorkspaceAuditAction::AccountPurged,
                    user_id,
                    "users",
                    user_id.to_string(),
                    final_report,
                )
                .await
                {
                    report.audit_failed += 1;
                    warn!(
                        request_id = %request_id,
                        error = %e,
                        "account_purge_worker: audit insert failed AFTER a successful purge"
                    );
                }
            }
            Err(e) => {
                report.failed += 1;
                // `fail_account_purge` decide entre devolver para a fila e
                // condenar o pedido; `last_error` fica na tabela, fora do
                // audit, porque texto de erro de banco pode citar valores.
                let new_status: Option<String> =
                    sqlx::query_scalar("SELECT fail_account_purge($1, $2, $3)")
                        .bind(request_id)
                        .bind(e.to_string())
                        .bind(config.max_attempts)
                        .fetch_one(pg)
                        .await?;

                let terminal = new_status.as_deref() == Some("failed");
                warn!(
                    request_id = %request_id,
                    attempts = attempts,
                    terminal = terminal,
                    error = %e,
                    "account_purge_worker: purge attempt failed"
                );

                if let Err(audit_err) = emit_purge_audit(
                    pool.as_ref(),
                    WorkspaceAuditAction::AccountPurgeFailed,
                    user_id,
                    "account_deletion_requests",
                    request_id.to_string(),
                    json!({ "attempts": attempts, "terminal": terminal }),
                )
                .await
                {
                    report.audit_failed += 1;
                    warn!(
                        request_id = %request_id,
                        error = %audit_err,
                        "account_purge_worker: failure-audit insert failed"
                    );
                }
            }
        }
    }

    if report.claimed > 0 {
        info!(
            claimed = report.claimed,
            purged = report.purged,
            blobs_deleted = report.blobs_deleted,
            blobs_failed = report.blobs_failed,
            failed = report.failed,
            audit_failed = report.audit_failed,
            "account_purge_worker: tick complete"
        );
    }

    drop(release_guard); // explicito

    Ok(report)
}

/// Resultado do apagamento de um titular.
struct PurgeOutcome {
    db_report: serde_json::Value,
    blobs_deleted: u64,
    blobs_failed: u64,
}

/// Apaga um titular: a parte de banco (quando ainda falta) e depois os blobs.
async fn purge_one(
    pool: &AppPool,
    object_store: Option<&Arc<dyn ObjectStore>>,
    user_id: Uuid,
    db_purged: bool,
    resume_keys: Vec<String>,
) -> Result<PurgeOutcome, sqlx::Error> {
    let pg = pool.pool_for_handlers();

    // Passo de banco. Quando `db_purged` ja esta marcado, uma passada
    // anterior commitou e morreu antes dos blobs: retomamos com as chaves
    // que ela persistiu, sem reexecutar o apagamento.
    let (db_report, keys) = if db_purged {
        (json!({ "resumed": true }), resume_keys)
    } else {
        let row = sqlx::query("SELECT report, object_keys FROM purge_account_data($1)")
            .bind(user_id)
            .fetch_one(pg)
            .await?;
        let report: serde_json::Value = row.try_get("report")?;
        let keys: Vec<String> = row.try_get("object_keys")?;
        (report, keys)
    };

    if keys.is_empty() {
        return Ok(PurgeOutcome {
            db_report,
            blobs_deleted: 0,
            blobs_failed: 0,
        });
    }

    // Ha blobs e nenhum backend: o pedido NAO pode ser fechado. Ver o
    // docblock do modulo — declarar `completed` com blob vivo seria mentir
    // no audit.
    let Some(store) = object_store else {
        return Err(sqlx::Error::Configuration(
            format!(
                "account purge for {user_id} has {} object(s) to remove but no ObjectStore is \
                 wired; refusing to close the request",
                keys.len()
            )
            .into(),
        ));
    };

    let mut blobs_deleted = 0u64;
    let mut blobs_failed = 0u64;
    let mut first_error: Option<String> = None;

    for key in &keys {
        // `ObjectStore::delete` e idempotente por contrato — chave ausente
        // devolve Ok, o que torna a retomada segura.
        match store.delete(key).await {
            Ok(()) => blobs_deleted += 1,
            Err(e) => {
                blobs_failed += 1;
                // A chave e derivada de `{group_id}/{file_uuid}/v{N}` e nao
                // carrega nome de arquivo; ainda assim so o erro vai para o
                // log, sem a chave, para nao correlacionar tenant e falha.
                warn!(
                    user_id = %user_id,
                    error = %e,
                    "account_purge_worker: ObjectStore delete failed"
                );
                if first_error.is_none() {
                    first_error = Some(e.to_string());
                }
            }
        }
    }

    // Qualquer blob sobrevivente mantem o pedido aberto para nova tentativa.
    // O passo de banco ja esta commitado e marcado `db_purged`, entao a
    // retomada so refaz a remocao que falhou.
    if blobs_failed > 0 {
        let detail = first_error.unwrap_or_else(|| "unknown".to_string());
        return Err(sqlx::Error::Configuration(
            format!("{blobs_failed} object(s) could not be removed: {detail}").into(),
        ));
    }

    Ok(PurgeOutcome {
        db_report,
        blobs_deleted,
        blobs_failed,
    })
}

/// Emite um evento de audit de apagamento na sua propria transacao.
///
/// `group_id` e o nil-uuid: apagamento de conta e user-scoped, mesma
/// convencao de `PasswordChanged` / `AccountSelfDeleted`.
async fn emit_purge_audit(
    pool: &AppPool,
    action: WorkspaceAuditAction,
    user_id: Uuid,
    resource_type: &'static str,
    resource_id: String,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error> {
    let nil_uuid = Uuid::nil();
    let mut tx = pool.pool_for_handlers().begin().await?;

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await?;

    audit_workspace_event(
        &mut tx,
        action,
        user_id,
        nil_uuid,
        resource_type,
        resource_id,
        metadata,
    )
    .await
    .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))?;

    tx.commit().await?;
    Ok(())
}

/// Libera o advisory lock mesmo se o caminho acima sair por erro.
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
            let _ = sqlx::query("SELECT pg_advisory_unlock(hashtext($1))")
                .bind(PURGE_LOCK_KEY)
                .execute(pool.pool_for_handlers())
                .await;
        });
    }
}

/// Sobe o loop periodico. Devolve o `JoinHandle` para o chamador manter vivo
/// pelo tempo de vida do processo (mesma forma do `uploads_worker`).
pub fn spawn_account_purge_worker(
    pool: Arc<AppPool>,
    object_store: Option<Arc<dyn ObjectStore>>,
    config: AccountPurgeWorkerConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            interval_secs = config.interval.as_secs(),
            batch_size = config.batch_size,
            grace_period_days = DEFAULT_GRACE_PERIOD_DAYS,
            "account_purge_worker: starting"
        );
        let mut ticker = tokio::time::interval(config.interval);
        // O primeiro tick do `interval` dispara imediatamente — pulamos para
        // o gateway terminar o bootstrap antes de tocar o banco.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match run_purge_tick(pool.clone(), object_store.clone(), &config).await {
                Ok(report) => {
                    if report.claimed == 0 {
                        debug!("account_purge_worker: idle tick");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "account_purge_worker: tick failed");
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
        let r = PurgeTickReport::default();
        assert_eq!(r.claimed, 0);
        assert_eq!(r.purged, 0);
        assert_eq!(r.blobs_deleted, 0);
        assert_eq!(r.blobs_failed, 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.audit_failed, 0);
    }

    #[test]
    fn default_config_is_hourly_batch_32_five_attempts() {
        let c = AccountPurgeWorkerConfig::default();
        assert_eq!(c.interval, Duration::from_secs(3600));
        assert_eq!(c.batch_size, 32);
        assert_eq!(c.stale_after, Duration::from_secs(3600));
        assert_eq!(c.max_attempts, 5);
    }

    #[test]
    fn config_overrides_preserved() {
        let c = AccountPurgeWorkerConfig {
            interval: Duration::from_secs(42),
            batch_size: 7,
            stale_after: Duration::from_secs(11),
            max_attempts: 2,
        };
        assert_eq!(c.interval.as_secs(), 42);
        assert_eq!(c.batch_size, 7);
        assert_eq!(c.stale_after.as_secs(), 11);
        assert_eq!(c.max_attempts, 2);
    }

    /// A carencia default e citada textualmente em
    /// `docs/legal/data-subject-requests.md` e na resposta do
    /// `DELETE /v1/me`. Se alguem mudar a constante sem mudar o documento, o
    /// que o titular leu deixa de ser verdade — este teste e o alarme.
    #[test]
    fn grace_period_is_thirty_days() {
        assert_eq!(
            DEFAULT_GRACE_PERIOD_DAYS, 30,
            "mudar a carencia exige atualizar docs/legal/data-subject-requests.md \
             e a prosa do DELETE /v1/me no mesmo commit"
        );
    }

    /// O lock e por nome; um rename silencioso faria duas replicas varrerem
    /// em paralelo sem perceber.
    #[test]
    fn advisory_lock_key_is_stable() {
        assert_eq!(PURGE_LOCK_KEY, "account_purge");
    }
}
