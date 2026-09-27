//! O que a retencao sabe dos dois bancos (#1436): o tamanho, a previa de
//! uma limpeza e a ultima limpeza que de fato rodou.
//!
//! Memoria (`memory.db`) e ledger de runs (`agent_runs` no `sessions.db`)
//! sao politicas distintas e bancos distintos, e continuam assim aqui: cada
//! banco guarda a propria linha de estado, na tabela `retention_state` DELE.
//!
//! # A ultima limpeza e gravada por quem apaga
//!
//! `MemoryStore::compact` e `SessionStore::prune_agent_runs` registram a
//! propria execucao. Assim a varredura automatica, a CLI (`garraia memory
//! compact`) e o Web Console aparecem no mesmo lugar, e nenhum deles
//! precisa lembrar de anotar. O registro e best-effort: a delecao ja
//! aconteceu, e falhar a chamada por causa da anotacao mentiria sobre ela.
//!
//! # A previa usa a MESMA clausula da delecao
//!
//! `COMPACT_COUNT_SQL` e `PRUNE_COUNT_SQL` repetem, palavra por palavra, o
//! `WHERE` do `DELETE` correspondente — testes cobram. Se divergirem, a
//! previa promete um numero e a limpeza apaga outro.

use chrono::{DateTime, SecondsFormat, Utc};
use garraia_common::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::agent_runs::{self, iso8601_utc};
use crate::memory_store::MemoryStore;
use crate::session_store::SessionStore;

/// Escopo da memoria na `retention_state` do `memory.db`.
pub const SCOPE_MEMORY: &str = "memory";
/// Escopo do ledger na `retention_state` do `sessions.db`.
pub const SCOPE_RUN_LEDGER: &str = "run_ledger";

/// Uma linha por escopo, no mesmo banco do dado que ela descreve.
/// Forward-only e aditivo (regra 9 do CLAUDE.md): `IF NOT EXISTS`, sem
/// tocar em tabela existente.
pub(crate) const RETENTION_STATE_SQL: &str = "CREATE TABLE IF NOT EXISTS retention_state (
    scope TEXT PRIMARY KEY,
    ran_at TEXT NOT NULL,
    deleted INTEGER NOT NULL,
    cutoff TEXT NOT NULL
);";

/// A ultima limpeza gravada. Instantes em ISO 8601 UTC com `Z`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastCleanup {
    pub at: String,
    pub deleted: u64,
    pub cutoff: String,
}

/// O que um `compact(before)` apagaria agora — nada e apagado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionPreview {
    pub would_delete: usize,
    pub before: DateTime<Utc>,
}

/// Tamanho da memoria para o console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRetentionSnapshot {
    pub entries_total: usize,
    /// Fixadas: a limpeza nunca as apaga.
    pub entries_pinned: usize,
    /// Com prazo vencido: ja fora do recall, saem na proxima limpeza.
    pub entries_expired: usize,
    /// Bytes do `memory.db` (paginas × tamanho da pagina), indice vetorial
    /// incluido. Apagar libera paginas para reuso; o arquivo nao encolhe.
    pub db_bytes: u64,
    pub last_cleanup: Option<LastCleanup>,
}

/// Tamanho do ledger para o console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLedgerSnapshot {
    pub runs: u64,
    /// Em voo: a limpeza nunca os apaga.
    pub running: u64,
    /// Bytes da tabela e dos indices dela (`dbstat`). `None` quando o
    /// SQLite nao tem a `dbstat` compilada — nunca um numero inventado.
    pub bytes: Option<u64>,
    pub last_cleanup: Option<LastCleanup>,
}

fn iso_z(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Registra uma limpeza que rodou. Best-effort: falha vira `warn!` — a
/// delecao ja aconteceu, e devolver erro por causa da anotacao faria o
/// chamador relatar que nada foi apagado.
pub(crate) fn record_cleanup(
    conn: &Connection,
    scope: &str,
    deleted: usize,
    cutoff: DateTime<Utc>,
) {
    let resultado = conn.execute(
        "INSERT INTO retention_state (scope, ran_at, deleted, cutoff)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(scope) DO UPDATE SET
             ran_at = excluded.ran_at,
             deleted = excluded.deleted,
             cutoff = excluded.cutoff",
        params![
            scope,
            iso_z(Utc::now()),
            i64::try_from(deleted).unwrap_or(i64::MAX),
            iso_z(cutoff)
        ],
    );
    if let Err(e) = resultado {
        warn!(scope, erro = %e, "retencao: a limpeza rodou, mas o registro da ultima limpeza falhou");
    }
}

/// A ultima limpeza de `scope`, ou `None` (nunca rodou, ou ilegivel — e ai
/// o console diz "sem registro", nunca inventa um instante).
pub(crate) fn read_last_cleanup(conn: &Connection, scope: &str) -> Option<LastCleanup> {
    let linha = conn
        .query_row(
            "SELECT ran_at, deleted, cutoff FROM retention_state WHERE scope = ?1",
            params![scope],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional();
    match linha {
        Ok(Some((at, deleted, cutoff))) => Some(LastCleanup {
            at: iso8601_utc(&at),
            deleted: u64::try_from(deleted).unwrap_or(0),
            cutoff: iso8601_utc(&cutoff),
        }),
        Ok(None) => None,
        Err(e) => {
            warn!(scope, erro = %e, "retencao: nao consegui ler a ultima limpeza");
            None
        }
    }
}

fn contar(conn: &Connection, sql: &'static str) -> Result<usize> {
    let n: i64 = conn
        .query_row(sql, [], |r| r.get(0))
        .map_err(|e| Error::Database(format!("retention count failed: {e}")))?;
    Ok(usize::try_from(n).unwrap_or(0))
}

impl MemoryStore {
    /// Quantas entradas um `compact(before)` apagaria agora, sem apagar.
    ///
    /// O bind e o mesmo do `compact` (`before.to_rfc3339()`), e a clausula
    /// e a mesma do `DELETE` — ver `COMPACT_COUNT_SQL`.
    pub fn compact_preview_sync(&self, before: DateTime<Utc>) -> Result<CompactionPreview> {
        let conn = self.connection()?;
        let n: i64 = conn
            .query_row(
                crate::memory_store::COMPACT_COUNT_SQL,
                params![before.to_rfc3339()],
                |r| r.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to preview compaction: {e}")))?;
        Ok(CompactionPreview {
            would_delete: usize::try_from(n).unwrap_or(0),
            before,
        })
    }

    /// Contagens, bytes e a ultima limpeza.
    ///
    /// As tres contagens sao as MESMAS consultas do `integrity_report` (o
    /// `garraia memory stats`): o console e a CLI nao podem discordar sobre
    /// quantas entradas existem. Aqui nao ha varredura de ids nem de orfaos
    /// — isso e diagnostico, e esta tela so precisa do tamanho.
    pub fn retention_snapshot_sync(&self) -> Result<MemoryRetentionSnapshot> {
        let conn = self.connection()?;
        let entries_total = contar(&conn, "SELECT count(*) FROM memory_entries")?;
        let entries_pinned = contar(
            &conn,
            "SELECT count(*) FROM memory_entries WHERE pinned_at IS NOT NULL",
        )?;
        let entries_expired = contar(
            &conn,
            "SELECT count(*) FROM memory_entries
                     WHERE ttl_expires_at IS NOT NULL
                       AND datetime(ttl_expires_at) <= datetime('now')",
        )?;
        let paginas: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .map_err(|e| Error::Database(format!("failed to read page_count: {e}")))?;
        let tamanho: i64 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .map_err(|e| Error::Database(format!("failed to read page_size: {e}")))?;
        let db_bytes = u64::try_from(paginas)
            .unwrap_or(0)
            .saturating_mul(u64::try_from(tamanho).unwrap_or(0));
        Ok(MemoryRetentionSnapshot {
            entries_total,
            entries_pinned,
            entries_expired,
            db_bytes,
            last_cleanup: read_last_cleanup(&conn, SCOPE_MEMORY),
        })
    }
}

impl SessionStore {
    /// Quantos runs um `prune_agent_runs(cutoff)` apagaria agora, sem apagar.
    /// Mesma clausula e mesmo formato de corte do `DELETE`.
    pub fn count_prunable_agent_runs(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row(
                agent_runs::PRUNE_COUNT_SQL,
                params![agent_runs::corte_do_prune(cutoff)],
                |r| r.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to preview run prune: {e}")))?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// Contagens, bytes e a ultima limpeza do ledger.
    pub fn run_ledger_snapshot(&self) -> Result<RunLedgerSnapshot> {
        let runs = contar(&self.conn, "SELECT count(*) FROM agent_runs")?;
        let running = contar(
            &self.conn,
            "SELECT count(*) FROM agent_runs WHERE status = 'running'",
        )?;
        Ok(RunLedgerSnapshot {
            runs: u64::try_from(runs).unwrap_or(0),
            running: u64::try_from(running).unwrap_or(0),
            bytes: bytes_do_ledger(&self.conn),
            last_cleanup: read_last_cleanup(&self.conn, SCOPE_RUN_LEDGER),
        })
    }
}

/// Bytes da tabela `agent_runs` e dos indices dela, pela `dbstat`.
///
/// Uma consulta por b-tree, com `name = ?1`: a `dbstat` so percorre as
/// paginas daquela arvore, e nao o `sessions.db` inteiro (que carrega as
/// mensagens). Qualquer falha — `dbstat` ausente num SQLite do sistema, por
/// exemplo — vira `None`, que o console mostra como "desconhecido".
fn bytes_do_ledger(conn: &Connection) -> Option<u64> {
    let nomes: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE tbl_name = 'agent_runs'")
            .ok()?;
        let linhas = stmt.query_map([], |r| r.get::<_, String>(0)).ok()?;
        linhas.collect::<std::result::Result<Vec<_>, _>>().ok()?
    };
    let mut total: u64 = 0;
    for nome in &nomes {
        let n: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat WHERE name = ?1",
                params![nome],
                |r| r.get(0),
            )
            .ok()?;
        total = total.saturating_add(u64::try_from(n).unwrap_or(0));
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunStatus;
    use crate::memory_store::{MemoryProvider, MemoryRole, NewMemoryEntry};
    use chrono::Duration;

    fn entrada(conteudo: &str) -> NewMemoryEntry {
        NewMemoryEntry {
            tenant_id: "default".to_string(),
            session_id: "s1".to_string(),
            channel_id: None,
            user_id: None,
            continuity_key: None,
            role: MemoryRole::User,
            content: conteudo.to_string(),
            embedding: None,
            embedding_model: None,
            metadata: serde_json::Value::Null,
        }
    }

    /// Quatro entradas: velha, vencida, fixada-e-velha e nova. Uma limpeza
    /// de 90 dias leva exatamente as duas primeiras.
    async fn memoria_semeada() -> MemoryStore {
        let store = MemoryStore::in_memory().expect("store");
        let velha = store.remember(entrada("velha")).await.expect("insere");
        let vencida = store.remember(entrada("vencida")).await.expect("insere");
        let fixada = store.remember(entrada("fixada")).await.expect("insere");
        store.remember(entrada("nova")).await.expect("insere");
        {
            let conn = store.connection().expect("conn");
            for id in [&velha, &fixada] {
                conn.execute(
                    "UPDATE memory_entries SET created_at = datetime('now', '-200 days') WHERE id = ?1",
                    rusqlite::params![id],
                )
                .expect("envelhece");
            }
        }
        assert!(
            store
                .set_ttl(&vencida, Some(Utc::now() - Duration::hours(1)))
                .expect("ttl")
        );
        assert!(store.set_pinned(&fixada, true).expect("fixa"));
        store
    }

    #[tokio::test]
    async fn previa_da_memoria_conta_o_que_o_compact_apaga_e_nao_apaga() {
        let store = memoria_semeada().await;
        let corte = Utc::now() - Duration::days(90);
        let previa = store.compact_preview(corte).await.expect("previa");
        assert_eq!(
            previa.would_delete, 2,
            "velha + vencida; fixada e nova ficam"
        );
        assert_eq!(previa.before, corte);
        assert_eq!(
            store.integrity_report().expect("stats").entries_total,
            4,
            "a previa nao apaga nada"
        );
        assert_eq!(
            store.retention_snapshot().await.expect("snap").last_cleanup,
            None,
            "a previa nao e uma limpeza"
        );

        let feito = store.compact(corte).await.expect("compact");
        assert_eq!(feito.deleted_entries, previa.would_delete);
    }

    #[tokio::test]
    async fn compact_grava_a_ultima_limpeza() {
        let store = memoria_semeada().await;
        let corte = Utc::now() - Duration::days(90);
        store.compact(corte).await.expect("compact");

        let ultima = store
            .retention_snapshot()
            .await
            .expect("snap")
            .last_cleanup
            .expect("gravada pelo compact");
        assert_eq!(ultima.deleted, 2);
        assert!(ultima.at.ends_with('Z'), "{}", ultima.at);
        assert_eq!(
            ultima.cutoff,
            corte.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );

        // Uma segunda limpeza sem nada a apagar tambem e registrada: e ela
        // que prova que a varredura esta viva.
        store.compact(corte).await.expect("compact");
        let ultima = store
            .retention_snapshot()
            .await
            .expect("snap")
            .last_cleanup
            .expect("gravada");
        assert_eq!(ultima.deleted, 0);
    }

    /// O console e o `garraia memory stats` contam a mesma coisa.
    #[tokio::test]
    async fn snapshot_da_memoria_conta_como_o_stats() {
        let store = memoria_semeada().await;
        let snap = store.retention_snapshot().await.expect("snap");
        let stats = store.integrity_report().expect("stats");
        assert_eq!(snap.entries_total, stats.entries_total);
        assert_eq!(snap.entries_pinned, stats.entries_pinned);
        assert_eq!(snap.entries_expired, stats.entries_expired);
        assert_eq!(
            (
                snap.entries_total,
                snap.entries_pinned,
                snap.entries_expired
            ),
            (4, 1, 1)
        );
        assert!(snap.db_bytes > 0, "{}", snap.db_bytes);
    }

    #[test]
    fn sql_da_previa_da_memoria_em_sincronia_com_o_delete() {
        let contagem = crate::memory_store::COMPACT_COUNT_SQL
            .split_once("WHERE ")
            .expect("COUNT tem WHERE")
            .1;
        let delete = crate::memory_store::COMPACT_DELETE_SQL
            .split_once("WHERE ")
            .expect("DELETE tem WHERE")
            .1;
        assert_eq!(contagem, delete);
    }

    fn semeia_run(st: &SessionStore, id: &str, status: &str, inicio: &str, fim: Option<&str>) {
        st.conn
            .execute(
                "INSERT INTO agent_runs (id, goal, status, started_at, finished_at)
                 VALUES (?1, 'objetivo', ?2, ?3, ?4)",
                rusqlite::params![id, status, inicio, fim],
            )
            .expect("semeia");
    }

    /// `running` velho, terminal velho, terminal sem fim (inicio velho) e
    /// terminal novo. Uma limpeza de 30 dias leva exatamente os dois do meio.
    fn ledger_semeado() -> SessionStore {
        let st = SessionStore::in_memory().expect("store");
        semeia_run(&st, "em-voo", "running", "2020-01-01 00:00:00", None);
        semeia_run(
            &st,
            "velho",
            "done",
            "2020-01-01 00:00:00",
            Some("2020-01-01 00:10:00"),
        );
        semeia_run(&st, "sem-fim", "interrupted", "2020-01-02 00:00:00", None);
        st.start_agent_run("novo", None, "objetivo novo", None)
            .expect("abre");
        st.finish_agent_run("novo", RunStatus::Done, Some("ok"), None)
            .expect("fecha");
        st
    }

    #[test]
    fn previa_do_ledger_conta_o_que_o_prune_apaga_e_nao_apaga() {
        let st = ledger_semeado();
        let corte = Utc::now() - Duration::days(30);
        let previa = st.count_prunable_agent_runs(corte).expect("previa");
        assert_eq!(previa, 2, "velho + sem-fim; running e novo ficam");
        assert_eq!(
            st.count_agent_runs().expect("conta"),
            4,
            "a previa nao apaga"
        );
        assert_eq!(st.run_ledger_snapshot().expect("snap").last_cleanup, None);

        assert_eq!(st.prune_agent_runs(corte).expect("prune"), previa);
    }

    #[test]
    fn prune_grava_a_ultima_limpeza_do_ledger() {
        let st = ledger_semeado();
        let corte = Utc::now() - Duration::days(30);
        st.prune_agent_runs(corte).expect("prune");
        let ultima = st
            .run_ledger_snapshot()
            .expect("snap")
            .last_cleanup
            .expect("gravada pelo prune");
        assert_eq!(ultima.deleted, 2);
        assert!(ultima.at.ends_with('Z'), "{}", ultima.at);
        assert_eq!(
            ultima.cutoff,
            corte.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
    }

    #[test]
    fn snapshot_do_ledger_conta_runs_e_os_em_voo() {
        let st = ledger_semeado();
        let snap = st.run_ledger_snapshot().expect("snap");
        assert_eq!(snap.runs, 4);
        assert_eq!(snap.running, 1);
        assert!(snap.bytes.is_some_and(|b| b > 0), "{:?}", snap.bytes);
    }

    #[test]
    fn sql_da_previa_do_ledger_em_sincronia_com_o_delete() {
        let contagem = agent_runs::PRUNE_COUNT_SQL
            .split_once("WHERE ")
            .expect("COUNT tem WHERE")
            .1;
        let delete = agent_runs::PRUNE_DELETE_SQL
            .split_once("WHERE ")
            .expect("DELETE tem WHERE")
            .1;
        assert_eq!(contagem, delete);
    }

    /// Cada banco guarda so o proprio escopo: a limpeza da memoria nao
    /// aparece como limpeza do ledger, nem o contrario.
    #[tokio::test]
    async fn memoria_e_ledger_nao_dividem_o_estado() {
        let memoria = memoria_semeada().await;
        memoria
            .compact(Utc::now() - Duration::days(90))
            .await
            .expect("compact");
        let ledger = ledger_semeado();
        assert_eq!(
            ledger.run_ledger_snapshot().expect("snap").last_cleanup,
            None
        );
        let conn = memoria.connection().expect("conn");
        assert!(read_last_cleanup(&conn, SCOPE_MEMORY).is_some());
        assert!(read_last_cleanup(&conn, SCOPE_RUN_LEDGER).is_none());
    }
}
