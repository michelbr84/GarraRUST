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
            .query_map(rusqlite::params![limit], |r| {
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
            })
            .map_err(|e| Error::Database(e.to_string()))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows)
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
