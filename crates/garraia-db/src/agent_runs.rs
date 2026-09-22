//! Ledger durável de runs de agentes em background (P1 do gap analysis
//! 2026-09-15, ref. Hermes "delegate_task não é durável" e OpenClaw
//! "background tasks ledger").
//!
//! Contrato: todo spawn de sub-agente grava `running` no banco; fim grava
//! `done`/`error`/`cancelled`. No restart, runs que ficaram `running` são
//! marcadas `interrupted` — o run NÃO sobrevive ao processo (isso é
//! documentado; o que sobrevive é a **auditoria** e a capacidade de listar
//! o que estava em voo quando a máquina caiu).
//!
//! Schemas sem juros: conteúdo de goal/result é truncado para auditoria
//! (não é armazenamento de conversa — isso continua em `messages`).

use crate::session_store::SessionStore;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};

/// Limite de chars de goal/erro preservado no ledger (auditoria, não armazenamento).
const SNIPPET_MAX: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Done,
    Error,
    Cancelled,
    /// Estava `running` quando o processo caiu (marcado no startup).
    Interrupted,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Done => "done",
            RunStatus::Error => "error",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Interrupted => "interrupted",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "done" => RunStatus::Done,
            "error" => RunStatus::Error,
            "cancelled" => RunStatus::Cancelled,
            "interrupted" => RunStatus::Interrupted,
            _ => RunStatus::Running,
        }
    }
}

/// Linha do ledger (`agent_runs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRow {
    pub id: String,
    pub session_id: Option<String>,
    pub goal: String,
    pub mode: Option<String>,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub result_snippet: Option<String>,
    pub error_snippet: Option<String>,
}

/// Le uma linha completa de `agent_runs` na ordem de colunas usada pelas
/// duas listagens. Uma funcao so para as duas nao divergirem em silencio
/// quando a projecao mudar.
fn map_run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRunRow> {
    Ok(AgentRunRow {
        id: r.get(0)?,
        session_id: r.get(1)?,
        goal: r.get(2)?,
        mode: r.get(3)?,
        status: RunStatus::from_str(&r.get::<_, String>(4)?),
        started_at: r.get(5)?,
        finished_at: r.get(6)?,
        result_snippet: r.get(7)?,
        error_snippet: r.get(8)?,
    })
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= SNIPPET_MAX {
        s.to_string()
    } else {
        s.chars().take(SNIPPET_MAX).collect()
    }
}

impl SessionStore {
    /// Cria a tabela do ledger (idempotente; chamada no `new` do store).
    pub(crate) fn create_agent_runs_table(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS agent_runs (
                    id TEXT PRIMARY KEY,
                    session_id TEXT,
                    goal TEXT NOT NULL,
                    mode TEXT,
                    status TEXT NOT NULL DEFAULT 'running',
                    started_at TEXT NOT NULL DEFAULT (datetime('now')),
                    finished_at TEXT,
                    result_snippet TEXT,
                    error_snippet TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_agent_runs_status
                    ON agent_runs(status);
                CREATE INDEX IF NOT EXISTS idx_agent_runs_started
                    ON agent_runs(started_at);",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    /// Grava o início de um run (`running`). Idempotente por `id`.
    pub fn start_agent_run(
        &self,
        id: &str,
        session_id: Option<&str>,
        goal: &str,
        mode: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO agent_runs (id, session_id, goal, mode, status)
                 VALUES (?1, ?2, ?3, ?4, 'running')",
                rusqlite::params![id, session_id, truncate(goal), mode],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    /// Fecha um run com status terminal (`done`/`error`/`cancelled`).
    pub fn finish_agent_run(
        &self,
        id: &str,
        status: RunStatus,
        result_snippet: Option<&str>,
        error_snippet: Option<&str>,
    ) -> Result<()> {
        debug_assert!(matches!(
            status,
            RunStatus::Done | RunStatus::Error | RunStatus::Cancelled
        ));
        self.conn
            .execute(
                "UPDATE agent_runs
                 SET status = ?2, finished_at = datetime('now'),
                     result_snippet = ?3, error_snippet = ?4
                 WHERE id = ?1 AND status = 'running'",
                rusqlite::params![
                    id,
                    status.as_str(),
                    result_snippet.map(truncate),
                    error_snippet.map(truncate)
                ],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    /// Recuperação pós-restart: runs `running` viram `interrupted` e são
    /// devolvidos para auditoria/ação do operador.
    pub fn mark_interrupted_runs(&self) -> Result<Vec<AgentRunRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, goal, mode, started_at
                 FROM agent_runs WHERE status = 'running'",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let pendentes: Vec<AgentRunRow> = stmt
            .query_map([], |r| {
                Ok(AgentRunRow {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    goal: r.get(2)?,
                    mode: r.get(3)?,
                    status: RunStatus::Running,
                    started_at: r.get(4)?,
                    finished_at: None,
                    result_snippet: None,
                    error_snippet: None,
                })
            })
            .map_err(|e| Error::Database(e.to_string()))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::Database(e.to_string()))?;
        drop(stmt);
        if !pendentes.is_empty() {
            self.conn
                .execute(
                    "UPDATE agent_runs SET status = 'interrupted',
                     finished_at = datetime('now') WHERE status = 'running'",
                    [],
                )
                .map_err(|e| Error::Database(e.to_string()))?;
        }
        Ok(pendentes)
    }

    /// Runs mais recentes primeiro (auditoria no CLI/gateway).
    pub fn list_recent_agent_runs(&self, limit: u32) -> Result<Vec<AgentRunRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, goal, mode, status, started_at, finished_at,
                        result_snippet, error_snippet
                 FROM agent_runs ORDER BY started_at DESC, id DESC LIMIT ?1",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit], map_run_row)
            .map_err(|e| Error::Database(e.to_string()))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows)
    }

    /// Mesma listagem, restrita a um status (`garra runs list --status`).
    ///
    /// O filtro vive no SQL, e nao no chamador: filtrar depois de trazer
    /// `LIMIT` linhas faria `--status interrupted --limit 50` devolver menos
    /// de 50 interrompidos so porque runs `done` recentes ocuparam a janela.
    pub fn list_recent_agent_runs_by_status(
        &self,
        status: &RunStatus,
        limit: u32,
    ) -> Result<Vec<AgentRunRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, goal, mode, status, started_at, finished_at,
                        result_snippet, error_snippet
                 FROM agent_runs WHERE status = ?1
                 ORDER BY started_at DESC, id DESC LIMIT ?2",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![status.as_str(), limit], map_run_row)
            .map_err(|e| Error::Database(e.to_string()))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows)
    }
}

impl SessionStore {
    /// Quantas linhas o ledger tem, de qualquer status (#1227 slice 5).
    ///
    /// Existe para o aviso de boot do gateway com a retencao desligada: o
    /// operador ve o tamanho do ledger sem que nenhuma linha seja lida.
    pub fn count_agent_runs(&self) -> Result<u64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM agent_runs", [], |r| r.get(0))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Retencao do ledger (#1227 slice 5): apaga runs **terminais** cujo
    /// instante de referencia e anterior a `cutoff`. Devolve quantas linhas
    /// sairam.
    ///
    /// Tres regras, e todas sao do SQL, nao do chamador:
    ///
    /// - **`running` nunca e apagado**, qualquer que seja a idade. Um run
    ///   em voo que sumisse do ledger apagaria justamente a auditoria que o
    ///   ledger existe para guardar; o residuo de uma queda vira
    ///   `interrupted` na subida seguinte e so entao entra na politica.
    /// - O instante de referencia e `finished_at` e, na falta dele,
    ///   `started_at` — um run terminal sem fim gravado ainda envelhece.
    /// - Instante que nao parseia (`datetime()` devolve `NULL`) **fica**: a
    ///   comparacao com `NULL` e falsa, e o lado seguro de uma operacao que
    ///   apaga dado e nao apagar.
    ///
    /// O `cutoff` e recebido, e nao lido do relogio, para o teste fixar o
    /// presente. Sai no formato do `datetime('now')` do SQLite, que e o que
    /// o ledger grava, e entra por bind — nunca por concatenacao.
    pub fn prune_agent_runs(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        let corte = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();
        let apagados = self
            .conn
            .execute(
                "DELETE FROM agent_runs
                 WHERE status <> 'running'
                   AND datetime(COALESCE(finished_at, started_at)) < datetime(?1)",
                rusqlite::params![corte],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(apagados)
    }
}

/// Le um status do ledger de forma **estrita** (`garraia runs list
/// --status`, `GET /api/runs?status=`).
///
/// `RunStatus::from_str` e leniente de proposito (linha de banco com valor
/// estranho vira `Running`, o lado que nunca e apagado), mas para entrada de
/// operador isso transformaria um erro de digitacao em "nenhum run
/// encontrado" — a resposta mais enganosa possivel para quem procura um run
/// que existe. Aqui valor desconhecido e `None`.
pub fn parse_run_status(bruto: &str) -> Option<RunStatus> {
    match bruto.trim().to_ascii_lowercase().as_str() {
        "running" => Some(RunStatus::Running),
        "done" => Some(RunStatus::Done),
        "error" => Some(RunStatus::Error),
        "cancelled" => Some(RunStatus::Cancelled),
        "interrupted" => Some(RunStatus::Interrupted),
        _ => None,
    }
}

/// Normaliza o instante do banco para ISO 8601 UTC com sufixo `Z`.
///
/// O ledger grava com `datetime('now')` do SQLite, que devolve
/// `YYYY-MM-DD HH:MM:SS` **em UTC, sem dizer que e UTC**. Timestamp de
/// auditoria do projeto sai sempre com o `Z` explicito (CLAUDE.md §Convencao
/// de datas), entao a marca e colocada na leitura.
///
/// Um valor que nao parseia volta como veio: inventar um instante para uma
/// linha corrompida seria pior que mostrar o byte cru.
///
/// Mora aqui, e nao na CLI, desde o #1227 slice 6: `garraia runs list` e
/// `GET /api/runs` leem o mesmo ledger, e duas copias desta funcao seriam
/// dois formatos de instante para divergir em silencio.
pub fn iso8601_utc(bruto: &str) -> String {
    let t = bruto.trim();
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S") {
        return naive
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    // Linha gravada por outro produtor, ja com fuso: normaliza para UTC.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
        return dt
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    t.to_string()
}

/// Troca todo caractere de controle pelo caractere de substituicao Unicode.
///
/// Quem le o ledger escreve num terminal (CLI) ou numa pagina (console), e
/// `goal` pode ter nascido de uma tool call de LLM: um `\x1b[2K` embutido
/// apaga a linha impressa, um `\r` reescreve por cima da anterior. O campo e
/// dado hostil ate prova em contrario — e a prova e trocar a classe inteira,
/// nao uma lista de sequencias conhecidas.
///
/// Mapeia 1 caractere para 1 caractere, entao nao mexe em contagem nem em
/// limite de caractere de quem chama.
pub fn sanitize_control_chars(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

/// Corta o texto em `max_chars` caracteres (nao bytes: payload em portugues
/// e cheio de acento, e cortar no meio de um `ç` entrega mojibake), com `…`
/// quando houve corte — o resultado tem no maximo `max_chars + 1`.
///
/// A quebra de linha vira espaco antes da higienizacao: `goal` multilinha e
/// texto legitimo, e dobra-lo numa linha le melhor que uma fileira de `�`.
/// O resto dos controles nao tem leitura benigna e cai no sanitizador.
pub fn preview(texto: &str, max_chars: usize) -> String {
    let mut out: String = texto.chars().take(max_chars).collect();
    if out.chars().count() < texto.chars().count() {
        out.push('…');
    }
    sanitize_control_chars(&out.replace('\n', " "))
}

/// Hook de subida (gateway e CLI, #1227 slice 1): converte runs `running`
/// de uma execucao anterior em `interrupted` e loga o resultado. Um lugar
/// so para a regra de log — o log carrega ids, nunca `goal` (PII) — e uma
/// falha aqui nao pode impedir a subida: devolve 0 e avisa. Devolve quantos
/// runs foram marcados (0 na subida limpa).
pub fn log_interrupted_runs(store: &SessionStore) -> usize {
    match store.mark_interrupted_runs() {
        Ok(runs) => {
            if !runs.is_empty() {
                let ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
                tracing::warn!(
                    count = runs.len(),
                    ids = %ids.join(","),
                    "runs `running` de execucao anterior viraram `interrupted`"
                );
            }
            runs.len()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "falhou ao marcar runs interrompidos na subida");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        let st = SessionStore::in_memory().unwrap();
        st.create_agent_runs_table().unwrap();
        st
    }

    #[test]
    fn ciclo_completo_do_run() {
        let st = store();
        st.start_agent_run("run-1", Some("s-1"), "consertar o build", Some("code"))
            .unwrap();
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, RunStatus::Running);

        st.finish_agent_run("run-1", RunStatus::Done, Some("build verde"), None)
            .unwrap();
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert_eq!(rows[0].status, RunStatus::Done);
        assert_eq!(rows[0].result_snippet.as_deref(), Some("build verde"));
    }

    #[test]
    fn mark_interrupted_so_afeta_running() {
        let st = store();
        st.start_agent_run("a", None, "tarefa a", None).unwrap();
        st.start_agent_run("b", None, "tarefa b", None).unwrap();
        st.finish_agent_run("b", RunStatus::Done, None, None)
            .unwrap();

        let interrompidos = st.mark_interrupted_runs().unwrap();
        assert_eq!(interrompidos.len(), 1);
        assert_eq!(interrompidos[0].id, "a");

        // Segunda chamada: nada pendente.
        assert!(st.mark_interrupted_runs().unwrap().is_empty());
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert!(rows.iter().all(|r| r.status != RunStatus::Running));
    }

    #[test]
    fn snippets_sao_truncados() {
        let st = store();
        st.start_agent_run("x", None, &"g".repeat(2000), None)
            .unwrap();
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert_eq!(rows[0].goal.chars().count(), SNIPPET_MAX);
    }

    /// #1227 (slice 1): o hook de subida marca so o que ficou `running`,
    /// deixa status terminal em tudo e e idempotente — segunda subida nao
    /// tem nada a marcar.
    #[test]
    fn log_interrupted_runs_marca_e_conta() {
        let st = store();
        st.start_agent_run("vivo", None, "tarefa viva", None)
            .unwrap();
        st.start_agent_run("pronto", None, "tarefa pronta", None)
            .unwrap();
        st.finish_agent_run("pronto", RunStatus::Done, None, None)
            .unwrap();

        assert_eq!(super::log_interrupted_runs(&st), 1);
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert!(rows.iter().all(|r| r.status != RunStatus::Running));

        // Segunda subida: nada pendente.
        assert_eq!(super::log_interrupted_runs(&st), 0);
    }

    /// #1227 (slice 4): o filtro de status e do SQL. Se ele fosse aplicado
    /// depois do `LIMIT`, pedir 1 `done` com dois runs no banco poderia
    /// devolver lista vazia — o `LIMIT 1` traria o `interrompido` e o filtro
    /// o descartaria.
    #[test]
    fn filtro_de_status_acontece_antes_do_limite() {
        let st = store();
        st.start_agent_run("antigo", None, "tarefa antiga", None)
            .unwrap();
        st.finish_agent_run("antigo", RunStatus::Done, Some("ok"), None)
            .unwrap();
        st.start_agent_run("recente", None, "tarefa recente", None)
            .unwrap();
        super::log_interrupted_runs(&st);

        let dones = st
            .list_recent_agent_runs_by_status(&RunStatus::Done, 1)
            .unwrap();
        assert_eq!(dones.len(), 1, "o filtro perdeu a linha para o LIMIT");
        assert_eq!(dones[0].id, "antigo");

        let interrompidos = st
            .list_recent_agent_runs_by_status(&RunStatus::Interrupted, 10)
            .unwrap();
        assert_eq!(interrompidos.len(), 1);
        assert_eq!(interrompidos[0].id, "recente");

        // Status sem nenhuma linha nao e erro: e lista vazia.
        assert!(
            st.list_recent_agent_runs_by_status(&RunStatus::Cancelled, 10)
                .unwrap()
                .is_empty()
        );
    }

    /// Insere uma linha com instantes escolhidos, sem passar pelo relogio
    /// do SQLite — o prune compara idade, entao o teste precisa de idade.
    fn semeia_linha(
        st: &SessionStore,
        id: &str,
        status: &str,
        started_at: &str,
        finished_at: Option<&str>,
    ) {
        st.conn
            .execute(
                "INSERT INTO agent_runs (id, goal, status, started_at, finished_at)
                 VALUES (?1, 'objetivo', ?2, ?3, ?4)",
                rusqlite::params![id, status, started_at, finished_at],
            )
            .unwrap();
    }

    fn ids(st: &SessionStore) -> Vec<String> {
        let mut v: Vec<String> = st
            .list_recent_agent_runs(1000)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        v.sort();
        v
    }

    /// #1227 (slice 5): tabela status x idade. `running` nunca sai, nem com
    /// dez anos; terminal velho sai; terminal novo fica; terminal sem
    /// `finished_at` envelhece pelo `started_at`.
    #[test]
    fn prune_apaga_so_terminal_velho_e_nunca_running() {
        let st = store();
        let velho = "2016-01-01 00:00:00";
        let novo = "2026-09-20 00:00:00";
        let mut esperados_vivos = Vec::new();
        let mut esperados_apagados = 0;
        for status in ["running", "done", "error", "cancelled", "interrupted"] {
            // velho, com fim gravado
            let id = format!("{status}-velho");
            semeia_linha(&st, &id, status, velho, Some(velho));
            // novo
            let id_novo = format!("{status}-novo");
            semeia_linha(&st, &id_novo, status, novo, Some(novo));
            // velho, sem finished_at (usa started_at)
            let id_sem_fim = format!("{status}-sem-fim");
            semeia_linha(&st, &id_sem_fim, status, velho, None);
            // inicio velho, fim novo: o que conta e o fim
            let id_fim_novo = format!("{status}-fim-novo");
            semeia_linha(&st, &id_fim_novo, status, velho, Some(novo));

            esperados_vivos.push(id_novo);
            esperados_vivos.push(id_fim_novo);
            if status == "running" {
                esperados_vivos.push(id);
                esperados_vivos.push(id_sem_fim);
            } else {
                esperados_apagados += 2;
            }
        }
        let corte = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        let apagados = st.prune_agent_runs(corte).unwrap();
        assert_eq!(apagados, esperados_apagados);
        esperados_vivos.sort();
        assert_eq!(ids(&st), esperados_vivos);
        assert!(ids(&st).contains(&"running-velho".to_string()));

        // Idempotente: a segunda passada nao tem o que apagar.
        assert_eq!(st.prune_agent_runs(corte).unwrap(), 0);
    }

    /// Instante ilegivel nao e "velho": a linha fica. Um corte no futuro
    /// distante apaga o resto, mas nao ela.
    #[test]
    fn prune_poupa_instante_ilegivel() {
        let st = store();
        semeia_linha(&st, "lixo", "done", "nao-e-data", Some("tambem-nao"));
        semeia_linha(&st, "ok", "done", "2020-01-01 00:00:00", None);
        let futuro = chrono::Utc::now() + chrono::Duration::days(365);
        assert_eq!(st.prune_agent_runs(futuro).unwrap(), 1);
        assert_eq!(ids(&st), vec!["lixo".to_string()]);
    }

    #[test]
    fn count_agent_runs_conta_todos_os_status() {
        let st = store();
        assert_eq!(st.count_agent_runs().unwrap(), 0);
        st.start_agent_run("a", None, "g", None).unwrap();
        st.start_agent_run("b", None, "g", None).unwrap();
        st.finish_agent_run("b", RunStatus::Done, None, None)
            .unwrap();
        assert_eq!(st.count_agent_runs().unwrap(), 2);
    }

    #[test]
    fn parse_run_status_e_estrito() {
        assert_eq!(parse_run_status("done"), Some(RunStatus::Done));
        assert_eq!(
            parse_run_status(" INTERRUPTED "),
            Some(RunStatus::Interrupted)
        );
        assert_eq!(parse_run_status("dnoe"), None);
        assert_eq!(parse_run_status(""), None);
    }

    #[test]
    fn helpers_de_leitura_compartilhados() {
        assert_eq!(iso8601_utc("2026-09-21 12:34:56"), "2026-09-21T12:34:56Z");
        assert_eq!(
            iso8601_utc("2026-09-21T09:34:56-03:00"),
            "2026-09-21T12:34:56Z"
        );
        assert_eq!(iso8601_utc("nao-e-data"), "nao-e-data");
        assert_eq!(preview("coração de leão", 7), "coração…");
        assert_eq!(preview("uma\nlinha", 40), "uma linha");
        let limpo = preview("\x1b[2Kx\ry", 40);
        assert!(
            !limpo.contains('\x1b') && !limpo.contains('\r'),
            "{limpo:?}"
        );
    }

    #[test]
    fn start_idempotente_nao_sobrescreve_terminal() {
        let st = store();
        st.start_agent_run("r", None, "goal", None).unwrap();
        st.finish_agent_run("r", RunStatus::Error, None, Some("boom"))
            .unwrap();
        // Retry com mesmo id não reabre como running.
        st.start_agent_run("r", None, "goal", None).unwrap();
        let rows = st.list_recent_agent_runs(10).unwrap();
        assert_eq!(rows[0].status, RunStatus::Error);
    }
}
