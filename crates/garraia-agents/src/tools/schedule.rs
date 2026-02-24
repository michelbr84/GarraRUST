use async_trait::async_trait;
use garraia_common::{Error, Result};
use garraia_db::SessionStore;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tools::{Tool, ToolContext, ToolOutput};

/// Atraso máximo permitido: 30 dias em segundos.
const MAX_DELAY_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Máximo de heartbeats pendentes por sessão.
const MAX_PENDING_PER_SESSION: i64 = 5;

/// Ferramenta para agendar um "heartbeat" futuro (acordar o agente no futuro).
pub struct ScheduleHeartbeat {
    store: Arc<Mutex<SessionStore>>,
}

impl ScheduleHeartbeat {
    pub fn new(store: Arc<Mutex<SessionStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for ScheduleHeartbeat {
    fn name(&self) -> &'static str {
        "schedule_heartbeat"
    }

    fn description(&self) -> &'static str {
        "Agenda um despertar futuro para si mesmo. Use para lembretes ou para verificar tarefas posteriormente."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "delay_seconds": {
                    "type": "integer",
                    "description": "Número de segundos para aguardar antes de acordar (mín 1, máx 2592000 = 30 dias)"
                },
                "reason": {
                    "type": "string",
                    "description": "Motivo/contexto do despertar (ex: 'Verificar se o deploy terminou')"
                }
            },
            "required": ["delay_seconds", "reason"]
        })
    }

    async fn execute(&self, context: &ToolContext, args: serde_json::Value) -> Result<ToolOutput> {
        // Impede agendamento recursivo dentro de um heartbeat
        if context.is_heartbeat {
            return Err(Error::Agent(
                "não é possível agendar um heartbeat durante a execução de outro heartbeat"
                    .to_string(),
            ));
        }

        let delay = args["delay_seconds"].as_i64().ok_or_else(|| {
            Error::Agent("argumento 'delay_seconds' ausente ou inválido".to_string())
        })?;

        let reason = args["reason"]
            .as_str()
            .ok_or_else(|| Error::Agent("argumento 'reason' ausente ou inválido".to_string()))?;

        if delay <= 0 {
            return Err(Error::Agent(
                "delay_seconds deve ser positivo".to_string(),
            ));
        }

        if delay > MAX_DELAY_SECONDS {
            return Err(Error::Agent(format!(
                "delay_seconds não pode exceder {} (30 dias)",
                MAX_DELAY_SECONDS
            )));
        }

        let user_id = context
            .user_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let store = self.store.lock().await;

        // Limite de tarefas pendentes por sessão
        let pending = store.count_pending_tasks_for_session(&context.session_id)?;
        if pending >= MAX_PENDING_PER_SESSION {
            return Err(Error::Agent(format!(
                "a sessão já possui {} heartbeats pendentes (máximo {})",
                pending, MAX_PENDING_PER_SESSION
            )));
        }

        let execute_at = chrono::Utc::now() + chrono::Duration::seconds(delay);

        let task_id =
            store.schedule_task(&context.session_id, &user_id, execute_at, reason)?;

        Ok(ToolOutput::success(format!(
            "Heartbeat agendado para {} (em {} segundos). ID da tarefa: {}",
            execute_at.to_rfc3339(),
            delay,
            task_id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contexto_teste(session_id: &str) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            user_id: Some("u-1".to_string()),
            is_heartbeat: false,
        }
    }

    async fn setup_store(session_id: &str) -> Arc<Mutex<SessionStore>> {
        let store = SessionStore::in_memory().expect("store em memória deve abrir");
        let store = Arc::new(Mutex::new(store));
        {
            let guard = store.lock().await;
            guard
                .upsert_session(session_id, "web", "u-1", &serde_json::json!({}))
                .expect("upsert da sessão deve funcionar");
        }
        store
    }

    #[tokio::test]
    async fn agenda_tarefa_no_store() {
        let store = setup_store("sess-1").await;
        let tool = ScheduleHeartbeat::new(Arc::clone(&store));

        let out = tool
            .execute(
                &contexto_teste("sess-1"),
                serde_json::json!({
                    "delay_seconds": 1,
                    "reason": "ping depois"
                }),
            )
            .await
            .expect("execução deve funcionar");

        assert!(!out.is_error);
        assert!(out.content.contains("ID da tarefa:"));
    }

    #[tokio::test]
    async fn rejeita_delay_negativo() {
        let store = setup_store("sess-1").await;
        let tool = ScheduleHeartbeat::new(store);

        let err = tool
            .execute(
                &contexto_teste("sess-1"),
                serde_json::json!({ "delay_seconds": -5, "reason": "erro" }),
            )
            .await;

        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("positivo"));
    }

    #[tokio::test]
    async fn rejeita_delay_excessivo() {
        let store = setup_store("sess-1").await;
        let tool = ScheduleHeartbeat::new(store);

        let err = tool
            .execute(
                &contexto_teste("sess-1"),
                serde_json::json!({
                    "delay_seconds": MAX_DELAY_SECONDS + 1,
                    "reason": "muito longo"
                }),
            )
            .await;

        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("30 dias"));
    }

    #[tokio::test]
    async fn rejeita_agendamento_dentro_de_heartbeat() {
        let store = setup_store("sess-1").await;
        let tool = ScheduleHeartbeat::new(store);

        let context = ToolContext {
            session_id: "sess-1".to_string(),
            user_id: Some("u-1".to_string()),
            is_heartbeat: true,
        };

        let err = tool
            .execute(
                &context,
                serde_json::json!({ "delay_seconds": 60, "reason": "recursivo" }),
            )
            .await;

        assert!(err.is_err());
        assert!(
            err.unwrap_err()
                .to_string()
                .contains("não é possível agendar")
        );
    }

    #[tokio::test]
    async fn rejeita_quando_excede_limite_pendentes() {
        let store = setup_store("sess-1").await;
        let tool = ScheduleHeartbeat::new(Arc::clone(&store));

        for i in 0..MAX_PENDING_PER_SESSION {
            tool.execute(
                &contexto_teste("sess-1"),
                serde_json::json!({
                    "delay_seconds": 3600,
                    "reason": format!("task {}", i)
                }),
            )
            .await
            .expect("deve funcionar dentro do limite");
        }

        let err = tool
            .execute(
                &contexto_teste("sess-1"),
                serde_json::json!({
                    "delay_seconds": 3600,
                    "reason": "excedente"
                }),
            )
            .await;

        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("pendentes"));
    }

    #[tokio::test]
    async fn limite_eh_por_sessao() {
        let store = SessionStore::in_memory().expect("store em memória deve abrir");
        let store = Arc::new(Mutex::new(store));

        {
            let guard = store.lock().await;
            guard
                .upsert_session("s1", "web", "u1", &serde_json::json!({}))
                .unwrap();
            guard
                .upsert_session("s2", "web", "u2", &serde_json::json!({}))
                .unwrap();
        }

        let tool = ScheduleHeartbeat::new(Arc::clone(&store));

        for i in 0..MAX_PENDING_PER_SESSION {
            tool.execute(
                &contexto_teste("s1"),
                serde_json::json!({
                    "delay_seconds": 3600,
                    "reason": format!("s1-{}", i)
                }),
            )
            .await
            .unwrap();
        }

        let out = tool
            .execute(
                &ToolContext {
                    session_id: "s2".to_string(),
                    user_id: Some("u2".to_string()),
                    is_heartbeat: false,
                },
                serde_json::json!({
                    "delay_seconds": 60,
                    "reason": "s2 ok"
                }),
            )
            .await
            .unwrap();

        assert!(!out.is_error);
    }
}