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
//! O advisory lock e **session-scoped**: ele mora na conexao que o pegou.
//! Por isso o tick inteiro roda sobre UMA conexao fixada do `PurgePool`
//! (`PoolConnection` adquirida no inicio e devolvida so no fim): pegar o lock
//! numa conexao e solta-lo noutra (o padrao antigo, via guard com
//! `tokio::spawn`) falha em silencio — o lock vaza e toda varredura futura
//! daquele banco e condenada a pular. Se a liberacao falhar, a conexao e
//! **fechada** em vez de devolvida ao pool: fechar mata o backend e o Postgres
//! solta o lock junto com ele. (O `uploads_worker` ainda tem o padrao antigo
//! — ver a issue de acompanhamento; ele nao foi tocado aqui para nao misturar
//! escopos.)
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
//! Quem tem `EXECUTE` nessas funcoes e **so** o role `garraia_purge`
//! (migration 034 revoga o PUBLIC e nao granta ao `garraia_app`): pegar o
//! `DELETE` de um tenant inteiro nao pode estar ao alcance de credencial de
//! request — qualquer SQLi ou bug com app viraria destruicao cross-tenant.
//! O worker conecta pelo `PurgePool` (newtype que recusa URL cujo role
//! conectado nao seja `garraia_purge`); os handlers, nunca.
//!
//! O `garraia_app` **nao** tem `UPDATE` em `account_deletion_requests` (grant
//! da migration 034), logo a maquina de estados so anda por essas funcoes —
//! e ele tambem nao tem `EXECUTE` nelas, entao nem pelo app pool da para
//! contorna-las. So `fail_account_purge` pode condenar um pedido, e o texto
//! persistido em `last_error` passa por [`sanitize_sqlx_error`] para que uma
//! mensagem de erro de banco (que pode citar valores) nao vaze para a tabela.
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

use garraia_auth::{AppPool, PurgePool, WorkspaceAuditAction, audit_workspace_event};
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

/// Roda uma varredura. Devolve o relatorio do lote.
///
/// Duas pools, com papéis distintos: `purge_pool` (`garraia_purge`,
/// EXECUTE-only nas quatro funcoes da migration 034) e `app_pool`
/// (`garraia_app`, para o INSERT de audit). O app role nao tem EXECUTE nas
/// funcoes de apagamento — credencial de request nunca alcanca a destruicao
/// de um tenant.
///
/// O advisory lock e session-scoped, entao o tick inteiro roda sobre UMA
/// conexao fixada do purge pool: o lock e verificado, as quatro funcoes sao
/// chamadas e o lock e liberado na MESMA sessao. Se a liberacao falhar, a
/// conexao e fechada (nao devolvida ao pool) — fechar mata o backend e o
/// Postgres solta o lock junto, em vez de vazar uma sessao que condena toda
/// varredura futura a pular.
///
/// O chamador nao deve invocar isto concorrentemente do MESMO processo — o
/// advisory lock protege entre processos, mas dois tickers no mesmo processo
/// se bloqueariam.
pub async fn run_purge_tick(
    purge_pool: Arc<PurgePool>,
    app_pool: Arc<AppPool>,
    object_store: Option<Arc<dyn ObjectStore>>,
    config: &AccountPurgeWorkerConfig,
) -> Result<PurgeTickReport, sqlx::Error> {
    let mut conn = purge_pool.pool().acquire().await?;

    let got_lock: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext($1))")
        .bind(PURGE_LOCK_KEY)
        .fetch_one(&mut *conn)
        .await?;

    if !got_lock {
        debug!("account_purge_worker: lock contended; skipping tick");
        // Sem lock na sessao, devolver a conexao ao pool e inofensivo.
        return Ok(PurgeTickReport::default());
    }

    // A partir daqui a sessao de `conn` detem o lock. Todo caminho de
    // saida passa pela liberacao abaixo — inclusive o `?` interno, que
    // escapa com o erro ANTES do unlock, e e cuidado aqui em cima.
    //
    // `locked_tick` nao tem path panicavel hoje (sem `unwrap`/`expect`,
    // tudo via `?`). Se um aparecer, o unwind devolve `conn` ao pool com
    // o lock de sessao ainda ativo e a varredura do banco para ate o
    // backend morrer — `catch_unwind` aqui nao vale o custo enquanto nao
    // houver panic real; este comentario e o alarme para revisitar se
    // algum aparecer.
    let result = locked_tick(&mut conn, app_pool.as_ref(), object_store.as_ref(), config).await;

    // Liberar na MESMA sessao que pegou. `pg_advisory_unlock` devolve
    // `false` quando o lock nao esta na sessao (nunca deveria acontecer);
    // tratar como falha e fechar a conexao, que mata o backend e solta
    // qualquer lock esquecido junto com ele.
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(hashtext($1))")
        .bind(PURGE_LOCK_KEY)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(false);

    if !released {
        warn!(
            "account_purge_worker: advisory unlock failed; closing the pinned connection \
             so Postgres releases the lock with the backend"
        );
        // Um erro aqui nao muda o desfecho: o que importa e matar a sessao,
        // e o drop de `conn` fecha o backend tambem se este close falhar.
        let _ = conn.close().await;
    }

    result
}

/// O trabalho do lote, com o lock ja assumido e sobre a conexao fixada.
///
/// `conn` (purge role) executa as quatro funcoes de migration 034;
/// `app_pool` (app role) so emite audit. As funcoes de fechamento sao
/// chamadas na MESMA sessao do lock para que a maquina de estados e o lock
/// andem juntos: se esta sessao morrer no meio, o lock cai junto e a
/// retomada e feita pelo proximo tick que conseguir o lock.
async fn locked_tick(
    conn: &mut sqlx::PgConnection,
    app_pool: &AppPool,
    object_store: Option<&Arc<dyn ObjectStore>>,
    config: &AccountPurgeWorkerConfig,
) -> Result<PurgeTickReport, sqlx::Error> {
    let mut report = PurgeTickReport::default();

    // Reivindica o lote devido. `stale_after` entra como interval para a
    // funcao retomar pedidos abandonados por um worker morto.
    let claimed = sqlx::query(
        "SELECT request_id, user_id, attempts, db_purged, object_keys \
         FROM claim_account_purge(now(), $1, make_interval(secs => $2))",
    )
    .bind(config.batch_size as i32)
    .bind(config.stale_after.as_secs() as f64)
    .fetch_all(&mut *conn)
    .await?;

    for row in &claimed {
        let request_id: Uuid = row.try_get("request_id")?;
        let user_id: Uuid = row.try_get("user_id")?;
        let attempts: i32 = row.try_get("attempts")?;
        let db_purged: bool = row.try_get("db_purged")?;
        let resume_keys: Vec<String> = row.try_get("object_keys")?;

        report.claimed += 1;

        match purge_one(conn, object_store, user_id, db_purged, resume_keys).await {
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
                    .fetch_one(&mut *conn)
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
                    app_pool,
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
                        error = %sanitize_sqlx_error(&e),
                        "account_purge_worker: audit insert failed AFTER a successful purge"
                    );
                }
            }
            Err(e) => {
                report.failed += 1;
                // `fail_account_purge` decide entre devolver para a fila e
                // condenar o pedido; `last_error` fica na tabela, fora do
                // audit. O texto vai por `sanitize_sqlx_error`: a mensagem
                // crua de um erro de Postgres pode citar valores (chave
                // duplicada mostra a chave, CHECK mostra o valor), e a
                // tabela nao e lugar para dado pessoal.
                let new_status: Option<String> =
                    sqlx::query_scalar("SELECT fail_account_purge($1, $2, $3)")
                        .bind(request_id)
                        .bind(sanitize_sqlx_error(&e))
                        .bind(config.max_attempts)
                        .fetch_one(&mut *conn)
                        .await?;

                let terminal = new_status.as_deref() == Some("failed");
                warn!(
                    request_id = %request_id,
                    attempts = attempts,
                    terminal = terminal,
                    // O MESMO texto sanitizado que foi persistido em
                    // `last_error`: log e tabela falam igual, sem a mensagem
                    // crua de Postgres (que pode citar valores).
                    error = %sanitize_sqlx_error(&e),
                    "account_purge_worker: purge attempt failed"
                );

                if let Err(audit_err) = emit_purge_audit(
                    app_pool,
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
                        error = %sanitize_sqlx_error(&audit_err),
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

    Ok(report)
}

/// Reduz um erro de `sqlx` a uma linha segura para `last_error` e log.
///
/// Erros `Database` do Postgres carregam `message()` com valores dentro
/// (violacao de unique mostra a chave, violacao de CHECK mostra o valor
/// rejeitado) — persistir isso em `account_deletion_requests.last_error`
/// espalharia dado pessoal de um titular em auditavel nao-pessoal. O que
/// fica e a classe do erro, o SQLSTATE e o nome da constraint, que juntos
/// apontam a causa sem copiar o dado.
///
/// As demais variantes passam pelo Display truncado — mas `Configuration`
/// carrega texto que ESTE modulo construiu, entao quem monta a mensagem e
/// responsavel por nao embutir dado de tenant nela: `purge_one` redige o
/// `StorageError` ([`sanitize_storage_error`]) antes de embutir, e os
/// demais `Configuration` sao so contagens e UUIDs.
fn sanitize_sqlx_error(err: &sqlx::Error) -> String {
    const MAX_LEN: usize = 200;

    let text = match err {
        sqlx::Error::Database(db_err) => match db_err.constraint() {
            Some(constraint) => format!("database error (constraint: {constraint})"),
            None => "database error".to_string(),
        },
        other => other.to_string(),
    };

    if text.len() > MAX_LEN {
        // Truncar num limite de char (byte) seguro: `truncate` so corta em
        // fronteira UTF-8, e cortar no meio de um codepoint panicaria.
        let mut cut = MAX_LEN;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    } else {
        text
    }
}

/// Reduz um [`garraia_storage::StorageError`] a uma linha segura, SEM a
/// chave do objeto.
///
/// O Display de `StorageError` cita a chave em varias variantes
/// (`object not found: {key}`, `integrity check failed for {key}: …`,
/// `invalid object key: {0}`), e a chave embute `group_id`/`file_id` —
/// correlacionar tenant e falha e exatamente o que o docblock do modulo
/// diz para nao fazer, e `last_error` e auditavel. O que fica e a classe
/// do erro, que aponta a causa sem copiar o dado. Variantes cujo payload
/// nao pode conter a chave (nome da operacao, MIME rejeitado, TTLs) sao
/// preservadas; `Io` traz so a mensagem do OS (o `LocalFs` converte o
/// `io::Error` cru, sem path), e `Backend` e texto arbitrario do SDK —
/// esse vaia por completo.
fn sanitize_storage_error(err: &garraia_storage::StorageError) -> String {
    use garraia_storage::StorageError as S;
    match err {
        S::InvalidKey(_) => "invalid object key".to_string(),
        S::NotFound { .. } => "object not found".to_string(),
        S::Io(io_err) => format!("io error: {io_err}"),
        S::Unsupported(op) => format!("operation not supported: {op}"),
        S::IntegrityMismatch { .. } => "integrity check failed".to_string(),
        S::DisallowedMime { content_type } => {
            format!("content-type not in allow-list: {content_type}")
        }
        S::TtlOutOfRange {
            requested_secs,
            min_secs,
            max_secs,
        } => format!("presigned ttl {requested_secs}s out of range [{min_secs}s, {max_secs}s]"),
        S::Backend(_) => "backend error".to_string(),
    }
}

/// Resultado do apagamento de um titular.
struct PurgeOutcome {
    db_report: serde_json::Value,
    blobs_deleted: u64,
    blobs_failed: u64,
}

/// Apaga um titular: a parte de banco (quando ainda falta) e depois os blobs.
///
/// A passada de banco roda na MESMA conexao (`conn`) do tick, que detem o
/// advisory lock: se a sessao morrer durante o `purge_account_data`, o lock
/// cai com ela e o pedido fica `in_progress` para a retomada do proximo
/// vencedor do lock — nunca dois workers apagando o mesmo titular.
async fn purge_one(
    conn: &mut sqlx::PgConnection,
    object_store: Option<&Arc<dyn ObjectStore>>,
    user_id: Uuid,
    db_purged: bool,
    resume_keys: Vec<String>,
) -> Result<PurgeOutcome, sqlx::Error> {
    // Passo de banco. Quando `db_purged` ja esta marcado, uma passada
    // anterior commitou e morreu antes dos blobs: retomamos com as chaves
    // que ela persistiu, sem reexecutar o apagamento.
    let (db_report, keys) = if db_purged {
        (json!({ "resumed": true }), resume_keys)
    } else {
        let row = sqlx::query("SELECT report, object_keys FROM purge_account_data($1)")
            .bind(user_id)
            .fetch_one(&mut *conn)
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
                // carrega nome de arquivo — mas o Display de `StorageError`
                // EMBARCA a chave, e ela embute `group_id`. Log e
                // `first_error` (que vira `last_error` via
                // `sanitize_sqlx_error`) recebem so a CLASSE do erro, nunca
                // a chave: nao correlacionar tenant e falha.
                let redacted = sanitize_storage_error(&e);
                warn!(
                    user_id = %user_id,
                    error = %redacted,
                    "account_purge_worker: ObjectStore delete failed"
                );
                if first_error.is_none() {
                    first_error = Some(redacted);
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

/// Sobe o loop periodico. Devolve o `JoinHandle` para o chamador manter vivo
/// pelo tempo de vida do processo (mesma forma do `uploads_worker`).
///
/// `purge_pool` (role `garraia_purge`, EXECUTE-only nas funcoes de
/// apagamento) e `app_pool` (role `garraia_app`, so para o INSERT de audit)
/// vem separados de proposito: ver o docblock do modulo.
pub fn spawn_account_purge_worker(
    purge_pool: Arc<PurgePool>,
    app_pool: Arc<AppPool>,
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
            match run_purge_tick(
                purge_pool.clone(),
                app_pool.clone(),
                object_store.clone(),
                &config,
            )
            .await
            {
                Ok(report) => {
                    if report.claimed == 0 {
                        debug!("account_purge_worker: idle tick");
                    }
                }
                Err(e) => {
                    warn!(error = %sanitize_sqlx_error(&e), "account_purge_worker: tick failed");
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

    /// O Display de `StorageError` embarca a chave do objeto, e a chave
    /// embute `group_id` — log e `last_error` nao podem recebe-la. Se uma
    /// variante nova (ou um bump de `garraia-storage`) passar a citar a
    /// chave de um jeito nao coberto aqui, este teste e o alarme.
    #[test]
    fn storage_error_redaction_drops_key_and_group() {
        let group = "11111111-1111-1111-1111-111111111111";
        let key = format!(
            "groups/{group}/files/22222222-2222-2222-2222-222222222222/v1/33333333-3333-3333-3333-333333333333"
        );
        let cases = vec![
            garraia_storage::StorageError::InvalidKey(key.clone()),
            garraia_storage::StorageError::NotFound { key: key.clone() },
            garraia_storage::StorageError::IntegrityMismatch {
                key: key.clone(),
                reason: "sha256 mismatch".to_string(),
            },
            garraia_storage::StorageError::Backend(format!("s3 delete failed for {key}")),
        ];

        for err in cases {
            let redacted = sanitize_storage_error(&err);
            assert!(
                !redacted.contains(&key),
                "object key leaked into log/last_error: {redacted}"
            );
            assert!(
                !redacted.contains(group),
                "group id leaked into log/last_error: {redacted}"
            );
        }

        // Payloads que nao podem conter a chave sao preservados — a linha
        // continua apontando a causa.
        let mime = garraia_storage::StorageError::DisallowedMime {
            content_type: "text/x-executable".to_string(),
        };
        assert!(sanitize_storage_error(&mime).contains("text/x-executable"));
    }
}
