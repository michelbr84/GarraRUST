//! O store de automações — persistência SQLite das regras carregadas e do
//! histórico de execuções (a auditoria da #1128).
//!
//! Separação de papéis: o **fonte da verdade** das regras são os arquivos
//! versionáveis do usuário ([`super::spec::carregar_dir`]); a SQLite guarda
//! o retrato do que está no ar (para a futura página do console) e a
//! auditoria de execuções. Mesmo padrão do [`crate::state::DeviceStateStore`]:
//! `rusqlite` síncrono sob `tokio::sync::Mutex`.

use std::path::Path;

use tokio::sync::Mutex;

use super::spec::AutomationSpec;

/// Como uma execução terminou. Falha de gate é `bloqueada` — automação
/// tentou fazer o que a policy não deixa, e ficou registrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultadoExecucao {
    Executada,
    BloqueadaRisco,
    CondicaoFalsa,
    CondicaoErro,
    RateLimited,
    Erro,
}

impl ResultadoExecucao {
    pub fn de_texto(s: &str) -> Option<Self> {
        Some(match s {
            "executada" => Self::Executada,
            "bloqueada_risco" => Self::BloqueadaRisco,
            "condicao_falsa" => Self::CondicaoFalsa,
            "condicao_erro" => Self::CondicaoErro,
            "rate_limited" => Self::RateLimited,
            "erro" => Self::Erro,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executada => "executada",
            Self::BloqueadaRisco => "bloqueada_risco",
            Self::CondicaoFalsa => "condicao_falsa",
            Self::CondicaoErro => "condicao_erro",
            Self::RateLimited => "rate_limited",
            Self::Erro => "erro",
        }
    }
}

/// Uma linha da auditoria: o que disparou, como terminou, quanto demorou.
#[derive(Debug, Clone, PartialEq)]
pub struct Execucao {
    pub automacao: String,
    /// Milissegundos desde a época (UTC) do disparo.
    pub disparada_em: u64,
    pub resultado: ResultadoExecucao,
    /// Resumo do gatilho (JSON — `{"entity_id": "…"}` ou `{"cron": "…"}`).
    pub gatilho: serde_json::Value,
    /// Detalhe por ação (JSON — array de resultados, args usados não vão
    /// aqui: o resultado do device já os registra).
    pub detalhe: serde_json::Value,
    pub duracao_ms: u64,
}

/// O teto de auditoria — execuções velhas são podadas para a SQLite não
/// crescer infinito com um sensor barulhento.
const TETO_HISTORICO: i64 = 10_000;

pub struct AutomationStore {
    conn: Mutex<rusqlite::Connection>,
}

impl AutomationStore {
    /// Abre (ou cria) o banco em `path`.
    pub fn abrir_em(path: &Path) -> Result<Self, crate::HardwareError> {
        if let Some(pai) = path.parent() {
            std::fs::create_dir_all(pai).map_err(|e| {
                crate::HardwareError::Automations(format!("criar diretório {}: {e}", pai.display()))
            })?;
        }
        let conn = rusqlite::Connection::open(path).map_err(|e| {
            crate::HardwareError::Automations(format!("abrir {}: {e}", path.display()))
        })?;
        Self::com_conexao(conn)
    }

    /// Store em memória — os testes e o modo efêmero.
    pub fn em_memoria() -> Result<Self, crate::HardwareError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| crate::HardwareError::Automations(format!("abrir em memória: {e}")))?;
        Self::com_conexao(conn)
    }

    fn com_conexao(conn: rusqlite::Connection) -> Result<Self, crate::HardwareError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| crate::HardwareError::Automations(format!("WAL: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS automacoes (
                nome TEXT PRIMARY KEY,
                habilitada INTEGER NOT NULL,
                spec_json TEXT NOT NULL,
                arquivo TEXT NOT NULL,
                atualizada_em INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS execucoes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                automacao TEXT NOT NULL,
                disparada_em INTEGER NOT NULL,
                resultado TEXT NOT NULL,
                gatilho_json TEXT NOT NULL,
                detalhe_json TEXT NOT NULL,
                duracao_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS execucoes_automacao
                ON execucoes(automacao, id);",
        )
        .map_err(|e| crate::HardwareError::Automations(format!("schema: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Substitui o retrato das regras carregadas — o que não está na lista
    /// sai da tabela (regra apagada do repo não continua no ar). DELETE
    /// total + reinsert atômico: são dezenas de regras, não milhões de
    /// linhas, e evita montar `IN (…)` dinâmico (regra 5).
    pub async fn salvar_specs(&self, specs: &[AutomationSpec]) -> Result<(), crate::HardwareError> {
        let agora = crate::events::milis_agora();
        let mut conn = self.conn.lock().await;
        let tx = conn
            .transaction()
            .map_err(|e| crate::HardwareError::Automations(format!("tx: {e}")))?;
        tx.execute("DELETE FROM automacoes", [])
            .map_err(|e| crate::HardwareError::Automations(format!("limpar retrato: {e}")))?;
        for spec in specs {
            let spec_json = serde_json::to_string(spec)
                .map_err(|e| crate::HardwareError::Automations(format!("serializar spec: {e}")))?;
            tx.execute(
                "INSERT INTO automacoes (nome, habilitada, spec_json, arquivo, atualizada_em)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(nome) DO UPDATE SET
                    habilitada = excluded.habilitada,
                    spec_json = excluded.spec_json,
                    arquivo = excluded.arquivo,
                    atualizada_em = excluded.atualizada_em",
                rusqlite::params![spec.name, spec.enabled as i64, spec_json, "", agora,],
            )
            .map_err(|e| crate::HardwareError::Automations(format!("salvar {}: {e}", spec.name)))?;
        }
        tx.commit()
            .map_err(|e| crate::HardwareError::Automations(format!("commit: {e}")))?;
        Ok(())
    }

    /// Registra uma execução na auditoria, podando o histórico velho de
    /// tempos em tempos (a cada 256 escritas — custo amortizado).
    pub async fn registrar(&self, execucao: &Execucao) -> Result<(), crate::HardwareError> {
        static ESCRITAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let gatilho = serde_json::to_string(&execucao.gatilho)
            .map_err(|e| crate::HardwareError::Automations(format!("serializar gatilho: {e}")))?;
        let detalhe = serde_json::to_string(&execucao.detalhe)
            .map_err(|e| crate::HardwareError::Automations(format!("serializar detalhe: {e}")))?;
        {
            let conn = self.conn.lock().await;
            conn.execute(
                "INSERT INTO execucoes
                    (automacao, disparada_em, resultado, gatilho_json, detalhe_json, duracao_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    execucao.automacao,
                    execucao.disparada_em as i64,
                    execucao.resultado.as_str(),
                    gatilho,
                    detalhe,
                    execucao.duracao_ms as i64,
                ],
            )
            .map_err(|e| crate::HardwareError::Automations(format!("registrar: {e}")))?;
        }
        let n = ESCRITAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n.is_multiple_of(256) {
            let conn = self.conn.lock().await;
            conn.execute(
                "DELETE FROM execucoes
                 WHERE id NOT IN (
                    SELECT id FROM execucoes ORDER BY id DESC LIMIT ?1
                 )",
                [TETO_HISTORICO],
            )
            .map_err(|e| crate::HardwareError::Automations(format!("podar histórico: {e}")))?;
        }
        Ok(())
    }

    /// As últimas `limite` execuções (mais novas primeiro), de uma automação
    /// ou de todas. Para a auditoria humana e a futura página do console.
    pub async fn historico(
        &self,
        automacao: Option<&str>,
        limite: u32,
    ) -> Result<Vec<Execucao>, crate::HardwareError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT automacao, disparada_em, resultado, gatilho_json, detalhe_json, duracao_ms
                 FROM execucoes
                 WHERE (?1 IS NULL OR automacao = ?1)
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(|e| crate::HardwareError::Automations(format!("preparar histórico: {e}")))?;
        let rows = stmt
            .query_map(rusqlite::params![automacao, limite], |row| {
                let resultado_txt: String = row.get(2)?;
                let gatilho_txt: String = row.get(3)?;
                let detalhe_txt: String = row.get(4)?;
                Ok(Execucao {
                    automacao: row.get(0)?,
                    disparada_em: row.get::<_, i64>(1)? as u64,
                    resultado: ResultadoExecucao::de_texto(&resultado_txt)
                        .unwrap_or(ResultadoExecucao::Erro),
                    gatilho: serde_json::from_str(&gatilho_txt).unwrap_or(serde_json::Value::Null),
                    detalhe: serde_json::from_str(&detalhe_txt).unwrap_or(serde_json::Value::Null),
                    duracao_ms: row.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(|e| crate::HardwareError::Automations(format!("histórico: {e}")))?;
        let mut linhas = Vec::new();
        for row in rows {
            linhas.push(row.map_err(|e| crate::HardwareError::Automations(format!("linha: {e}")))?);
        }
        Ok(linhas)
    }

    /// O retrato do que está no ar: `(nome, habilitada)` de todas as regras
    /// carregadas na última `salvar_specs`.
    pub async fn listar(&self) -> Result<Vec<(String, bool)>, crate::HardwareError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT nome, habilitada FROM automacoes ORDER BY nome")
            .map_err(|e| crate::HardwareError::Automations(format!("listar: {e}")))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)))
            .map_err(|e| crate::HardwareError::Automations(format!("listar: {e}")))?;
        let mut linhas = Vec::new();
        for row in rows {
            linhas.push(row.map_err(|e| crate::HardwareError::Automations(format!("linha: {e}")))?);
        }
        Ok(linhas)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn spec_exemplo(nome: &str, habilitada: bool) -> AutomationSpec {
        AutomationSpec {
            name: nome.into(),
            enabled: habilitada,
            trigger: vec![super::super::spec::TriggerSpec::StateChanged {
                entity: "sensor.x".into(),
            }],
            condition: vec![],
            action: vec![super::super::spec::ActionSpec::DeviceExecute {
                device: "d".into(),
                capability: "power".into(),
                args: serde_json::Value::Null,
            }],
            debounce_secs: None,
            rate_limit: None,
            delay_secs: None,
        }
    }

    #[tokio::test]
    async fn salvar_e_listar_o_retrato() {
        let store = AutomationStore::em_memoria().unwrap();
        store
            .salvar_specs(&[spec_exemplo("a-luz", true), spec_exemplo("b-fan", false)])
            .await
            .unwrap();
        let lista = store.listar().await.unwrap();
        assert_eq!(lista, vec![("a-luz".into(), true), ("b-fan".into(), false)]);

        // O retrato é substituído: regra apagada some.
        store
            .salvar_specs(&[spec_exemplo("a-luz", true)])
            .await
            .unwrap();
        let lista = store.listar().await.unwrap();
        assert_eq!(lista, vec![("a-luz".into(), true)]);
    }

    #[tokio::test]
    async fn registrar_e_ler_historico() {
        let store = AutomationStore::em_memoria().unwrap();
        store
            .salvar_specs(&[spec_exemplo("a", true)])
            .await
            .unwrap();
        for i in 0..3 {
            store
                .registrar(&Execucao {
                    automacao: "a".into(),
                    disparada_em: 1000 + i,
                    resultado: if i == 1 {
                        ResultadoExecucao::BloqueadaRisco
                    } else {
                        ResultadoExecucao::Executada
                    },
                    gatilho: serde_json::json!({ "entity_id": "sensor.x" }),
                    detalhe: serde_json::json!([{ "acao": "executed" }]),
                    duracao_ms: 5 + i,
                })
                .await
                .unwrap();
        }
        let hist = store.historico(Some("a"), 10).await.unwrap();
        assert_eq!(hist.len(), 3);
        // Mais novo primeiro.
        assert_eq!(hist[0].disparada_em, 1002);
        assert_eq!(hist[1].resultado, ResultadoExecucao::BloqueadaRisco);
        assert_eq!(hist[2].gatilho["entity_id"], "sensor.x");

        let de_outro = store.historico(Some("inexistente"), 10).await.unwrap();
        assert!(de_outro.is_empty());
    }

    #[tokio::test]
    async fn historico_e_podado_no_teto() {
        let store = AutomationStore::em_memoria().unwrap();
        store
            .salvar_specs(&[spec_exemplo("a", true)])
            .await
            .unwrap();
        // Menos que o intervalo de poda: nada é podado no meio.
        for i in 0..50 {
            store
                .registrar(&Execucao {
                    automacao: "a".into(),
                    disparada_em: i,
                    resultado: ResultadoExecucao::Executada,
                    gatilho: serde_json::Value::Null,
                    detalhe: serde_json::Value::Null,
                    duracao_ms: 1,
                })
                .await
                .unwrap();
        }
        let hist = store.historico(Some("a"), 1000).await.unwrap();
        assert_eq!(hist.len(), 50);
    }

    #[tokio::test]
    async fn abrir_em_cria_arquivo_e_schema() {
        let dir = std::env::temp_dir().join(format!("garraia-store-{}", std::process::id()));
        let path = dir.join("automations.db");
        std::fs::remove_dir_all(&dir).ok();
        let store = AutomationStore::abrir_em(&path).unwrap();
        store
            .salvar_specs(&[spec_exemplo("a", true)])
            .await
            .unwrap();
        drop(store);
        // Reabre e encontra o retrato.
        let store2 = AutomationStore::abrir_em(&path).unwrap();
        assert_eq!(store2.listar().await.unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
