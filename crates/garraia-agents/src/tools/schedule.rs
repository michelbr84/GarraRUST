use async_trait::async_trait;
use garraia_common::{Error, Result};
use garraia_db::SessionStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tools::{Tool, ToolContext, ToolOutput};

/// Atraso máximo permitido: 30 dias em segundos.
const MAX_DELAY_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Máximo de heartbeats pendentes por sessão.
const MAX_PENDING_PER_SESSION: i64 = 5;

// ── Phase 5.4: Webhook & Event Trigger Support ──────────────────────────

/// Event types that can trigger agent execution
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Pull request created
    PrCreated,
    /// Code pushed to repository
    Push,
    /// Issue opened
    IssueOpened,
    /// Custom event type
    Custom(String),
}

impl EventType {
    /// Parse from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pr_created" => Self::PrCreated,
            "push" => Self::Push,
            "issue_opened" => Self::IssueOpened,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Convert to string
    pub fn as_str(&self) -> &str {
        match self {
            Self::PrCreated => "pr_created",
            Self::Push => "push",
            Self::IssueOpened => "issue_opened",
            Self::Custom(s) => s,
        }
    }
}

/// Status of a scheduled task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Pending execution
    Pending,
    /// Currently running
    Running,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Scheduled task with metadata for dashboard display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Unique task ID
    pub id: String,
    /// Session ID
    pub session_id: String,
    /// Task description/reason
    pub reason: String,
    /// Current status
    pub status: TaskStatus,
    /// When the task was last run
    #[serde(default)]
    pub last_run: Option<String>,
    /// When the task will next run
    #[serde(default)]
    pub next_run: Option<String>,
    /// Task type
    pub task_type: ScheduledTaskType,
}

/// Type of scheduled task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskType {
    /// Time-based heartbeat
    Heartbeat,
    /// Webhook trigger
    Webhook,
    /// Event trigger
    Event,
}

/// Webhook trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTrigger {
    /// URL pattern to match incoming webhooks
    pub url_pattern: String,
    /// Session to wake up
    pub session_id: String,
    /// Handler description
    pub handler: String,
    /// Whether the trigger is active
    pub active: bool,
    /// Created timestamp
    pub created_at: String,
}

/// Event trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTrigger {
    /// Event type to listen for
    pub event_type: EventType,
    /// Session to wake up
    pub session_id: String,
    /// Handler description
    pub handler: String,
    /// Whether the trigger is active
    pub active: bool,
    /// Created timestamp
    pub created_at: String,
}

/// Registry for webhook and event triggers
pub struct TriggerRegistry {
    /// Webhook triggers indexed by URL pattern
    webhooks: Arc<Mutex<HashMap<String, WebhookTrigger>>>,
    /// Event triggers indexed by event type string
    events: Arc<Mutex<HashMap<String, Vec<EventTrigger>>>>,
}

impl TriggerRegistry {
    /// Create a new TriggerRegistry
    pub fn new() -> Self {
        Self {
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            events: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a webhook trigger
    pub async fn on_webhook(&self, url_pattern: &str, session_id: &str, handler: &str) {
        let trigger = WebhookTrigger {
            url_pattern: url_pattern.to_string(),
            session_id: session_id.to_string(),
            handler: handler.to_string(),
            active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let mut webhooks = self.webhooks.lock().await;
        webhooks.insert(url_pattern.to_string(), trigger);
    }

    /// Register an event trigger
    pub async fn on_event(&self, event_type: EventType, session_id: &str, handler: &str) {
        let trigger = EventTrigger {
            event_type: event_type.clone(),
            session_id: session_id.to_string(),
            handler: handler.to_string(),
            active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let mut events = self.events.lock().await;
        events
            .entry(event_type.as_str().to_string())
            .or_insert_with(Vec::new)
            .push(trigger);
    }

    /// Find webhook triggers matching a URL
    pub async fn match_webhook(&self, url: &str) -> Vec<WebhookTrigger> {
        let webhooks = self.webhooks.lock().await;
        webhooks
            .values()
            .filter(|w| w.active && url.contains(&w.url_pattern))
            .cloned()
            .collect()
    }

    /// Find event triggers matching an event type
    pub async fn match_event(&self, event_type: &EventType) -> Vec<EventTrigger> {
        let events = self.events.lock().await;
        events
            .get(event_type.as_str())
            .map(|triggers| triggers.iter().filter(|t| t.active).cloned().collect())
            .unwrap_or_default()
    }

    /// List all scheduled tasks for dashboard
    pub async fn list_scheduled(&self) -> Vec<ScheduledTask> {
        let mut tasks = Vec::new();

        // Add webhook triggers
        let webhooks = self.webhooks.lock().await;
        for (id, webhook) in webhooks.iter() {
            tasks.push(ScheduledTask {
                id: id.clone(),
                session_id: webhook.session_id.clone(),
                reason: webhook.handler.clone(),
                status: if webhook.active {
                    TaskStatus::Pending
                } else {
                    TaskStatus::Cancelled
                },
                last_run: None,
                next_run: None,
                task_type: ScheduledTaskType::Webhook,
            });
        }
        drop(webhooks);

        // Add event triggers
        let events = self.events.lock().await;
        for (event_type, triggers) in events.iter() {
            for trigger in triggers {
                tasks.push(ScheduledTask {
                    id: format!("event_{}_{}", event_type, trigger.session_id),
                    session_id: trigger.session_id.clone(),
                    reason: trigger.handler.clone(),
                    status: if trigger.active {
                        TaskStatus::Pending
                    } else {
                        TaskStatus::Cancelled
                    },
                    last_run: None,
                    next_run: None,
                    task_type: ScheduledTaskType::Event,
                });
            }
        }

        tasks
    }

    /// Remove a webhook trigger
    pub async fn remove_webhook(&self, url_pattern: &str) -> bool {
        let mut webhooks = self.webhooks.lock().await;
        webhooks.remove(url_pattern).is_some()
    }

    /// Remove all event triggers for a session
    pub async fn remove_event_triggers(&self, session_id: &str) {
        let mut events = self.events.lock().await;
        for triggers in events.values_mut() {
            triggers.retain(|t| t.session_id != session_id);
        }
    }
}

impl Default for TriggerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

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
            return Err(Error::Agent("delay_seconds deve ser positivo".to_string()));
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

        let task_id = store.schedule_task(&context.session_id, &user_id, execute_at, reason)?;

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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
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
                    is_confirmation_approved: false,
                    working_dir: None,
                    project_id: None,
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
