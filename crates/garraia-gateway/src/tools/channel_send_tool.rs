//! `telegram_send` — the agent's proactive outbound message (issue #921).
//!
//! Lives in `garraia-gateway`, not `garraia-agents`, on purpose: the agents
//! crate depends on `garraia-common` and `garraia-db` only, while reaching a
//! channel needs `AppState.channels` (and, for addressing, the session store).
//! Implementing `garraia_agents::Tool` here closes over `Arc<AppState>` and
//! adds no dependency edge — the same constructor-injection shape
//! `ScheduleHeartbeat` uses for its `SessionStore`.
//!
//! The tool is thin by design. It does not talk to the Bot API; it builds an
//! outgoing `Message` and hands it to the registered channel adapter, which is
//! the identical path `execute_scheduled_task` uses. Fixing the addressing bug
//! that made that path dead is what made this tool a wrapper instead of a new
//! subsystem.
//!
//! # Two modes, two risk levels
//!
//! * **No `chat_id`** — reply into the chat this session belongs to, resolved
//!   from `chat_session_keys`. No allowlist: the human on the other end started
//!   this conversation and is already receiving messages in it. This is the
//!   mode the issue's use cases need (a heartbeat reminder, a "backup done"
//!   notice, an answer to a long-running task).
//! * **Explicit `chat_id`** — deny-by-default against
//!   [`ProactiveTargets`](crate::channel_send::ProactiveTargets). The model
//!   choosing an arbitrary recipient is a different thing entirely, and an
//!   operator has to have named that chat in `proactive_chat_ids` first.
//!
//! Recursion: unlike `ScheduleHeartbeat`, this tool is **allowed** during a
//! heartbeat. Being callable from a scheduled turn is the entire point — it is
//! how "your 2pm reminder" reaches the user.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::{
    ChannelId, Message, MessageContent, MessageDirection, Result, SessionId, UserId,
};

use crate::channel_send::{ProactiveTargets, SendBudget, with_channel_address};
use crate::state::AppState;

/// Upper bound on a single outbound message. Telegram itself caps a text
/// message at 4096 UTF-16 code units; refusing here with a clear error beats a
/// Bot API rejection the model cannot interpret.
const MAX_TEXT_CHARS: usize = 4000;

pub struct TelegramSendTool {
    /// **Weak** on purpose. `AppState` owns `Arc<AgentRuntime>`, the runtime
    /// owns this tool, and a strong handle back would close an `Arc` cycle
    /// that leaks the whole gateway state. It is a leak the process would
    /// never notice — the state is meant to outlive everything — but a cycle
    /// that only survives because nothing ever drops is still a trap for the
    /// next person to write an integration test that builds a gateway.
    ///
    /// The alternative, holding only `channels` + `chat_session_manager`, is
    /// not available: `AppState.channels` is a plain field, not an `Arc`.
    state: Weak<AppState>,
    /// Anti-amplification ceiling, per session. See [`SendBudget`].
    budget: SendBudget,
}

impl TelegramSendTool {
    pub fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::downgrade(state),
            budget: SendBudget::default(),
        }
    }

    /// The allowlist **as of right now**.
    ///
    /// Security audit finding (LOW), issue #921: reading this once at boot
    /// meant an operator removing a chat id from `proactive_chat_ids` did not
    /// take effect until a gateway restart — and the window where a revoked
    /// target still works is precisely the wrong thing to get wrong. The
    /// gateway already hot-reloads config through `current_config()`, so the
    /// tool reads it per call. It clones the config, which is only acceptable
    /// because `SendBudget` caps this path at a handful of calls per minute.
    fn targets(state: &AppState) -> ProactiveTargets {
        ProactiveTargets::from_config(&state.current_config())
    }

    /// The live state, or `None` once the gateway has shut down.
    fn state(&self) -> Option<Arc<AppState>> {
        self.state.upgrade()
    }

    /// Resolve the chat this send should go to, and whether it was allowed.
    ///
    /// Split out from `execute` so the decision is testable without a live
    /// channel: it is the security-relevant half.
    async fn resolve_target(
        &self,
        context: &ToolContext,
        requested: Option<i64>,
        targets: &ProactiveTargets,
    ) -> std::result::Result<i64, String> {
        match requested {
            // Explicit recipient: operator-configured allowlist, deny by default.
            Some(chat_id) => {
                if targets.allows(chat_id) {
                    Ok(chat_id)
                } else if targets.is_empty() {
                    Err(
                        "envio para chat_id explícito não está habilitado. O operador precisa \
                         listar os chats permitidos em `proactive_chat_ids` na configuração do \
                         canal telegram. Omita `chat_id` para responder no chat desta conversa."
                            .to_string(),
                    )
                } else {
                    // Deliberately does not echo the requested id back: the
                    // error reaches the model, and a rejected id is still a
                    // real person's chat.
                    Err(
                        "esse chat_id não está em `proactive_chat_ids`. Omita `chat_id` para \
                         responder no chat desta conversa."
                            .to_string(),
                    )
                }
            }
            // Implicit: the chat this session already belongs to.
            None => {
                let Some(state) = self.state() else {
                    return Err("gateway encerrando".to_string());
                };
                let Some(mgr) = &state.chat_session_manager else {
                    return Err(
                        "sem session store: não há como descobrir o chat desta conversa"
                            .to_string(),
                    );
                };
                match mgr
                    .external_key_for(&context.session_id, garraia_db::ChatSource::Telegram)
                    .await
                {
                    // Security audit finding (LOW): the stored value is not
                    // echoed. It is a chat address, and in an unexpected state
                    // (a key written by another source) it could be someone
                    // else's identifier. The model gains nothing from seeing
                    // it — it cannot fix a malformed database row.
                    Ok(Some(id)) => id.trim().parse::<i64>().map_err(|_| {
                        tracing::warn!(
                            session = %context.session_id,
                            "telegram_send: chat_session_keys tem external_id não numérico"
                        );
                        "o chat mapeado para esta sessão está com formato inválido; \
                         contate o operador"
                            .to_string()
                    }),
                    Ok(None) => Err(
                        "esta conversa não veio do Telegram, então não há chat para responder. \
                         Informe `chat_id` (precisa estar em `proactive_chat_ids`)."
                            .to_string(),
                    ),
                    Err(e) => Err(format!("falha ao resolver o chat desta sessão: {e}")),
                }
            }
        }
    }
}

#[async_trait]
impl Tool for TelegramSendTool {
    fn name(&self) -> &str {
        "telegram_send"
    }

    fn description(&self) -> &str {
        "Envia uma mensagem no Telegram por iniciativa própria (notificação, lembrete, \
         aviso de tarefa concluída). Sem `chat_id`, responde no chat desta conversa. \
         Com `chat_id`, só envia para chats que o operador liberou."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Texto da mensagem (máximo 4000 caracteres)"
                },
                "chat_id": {
                    "type": "integer",
                    "description": "Chat de destino. Omita para responder no chat desta \
                                    conversa — é o caso normal. Um chat_id explícito precisa \
                                    estar liberado pelo operador."
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let Some(text) = input.get("text").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error("parâmetro 'text' ausente"));
        };
        if text.trim().is_empty() {
            return Ok(ToolOutput::error("'text' está vazio"));
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Ok(ToolOutput::error(format!(
                "mensagem muito longa: {} caracteres (limite: {MAX_TEXT_CHARS})",
                text.chars().count()
            )));
        }

        let Some(state) = self.state() else {
            return Ok(ToolOutput::error("gateway encerrando"));
        };

        let requested = input.get("chat_id").and_then(|v| v.as_i64());
        let targets = Self::targets(&state);
        let chat_id = match self.resolve_target(context, requested, &targets).await {
            Ok(id) => id,
            Err(reason) => return Ok(ToolOutput::error(reason)),
        };

        // Charged only after the target is authorized: a refused send must not
        // burn the budget, or a model probing chat ids could lock the user out
        // of their own notifications.
        if let Err(used) = self
            .budget
            .try_consume(&context.session_id, std::time::Instant::now())
        {
            tracing::warn!(
                session = %context.session_id,
                used,
                "telegram_send: teto de envios da sessão atingido"
            );
            return Ok(ToolOutput::error(format!(
                "limite de {used} mensagens por minuto nesta conversa atingido. \
                 Junte o que falta dizer numa única mensagem."
            )));
        }

        let metadata = with_channel_address(
            &serde_json::json!({}),
            "telegram",
            Some(&chat_id.to_string()),
        );

        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: SessionId::from_string(&context.session_id),
            channel_id: ChannelId::from_string("telegram"),
            user_id: UserId::from_string(context.user_id.as_deref().unwrap_or("genesis")),
            direction: MessageDirection::Outgoing,
            content: MessageContent::Text(text.to_string()),
            timestamp: chrono::Utc::now(),
            metadata,
        };

        let channels = state.channels.read().await;
        let Some(channel) = channels.get("telegram") else {
            return Ok(ToolOutput::error(
                "canal telegram não está registrado neste gateway",
            ));
        };

        match channel.send_message(&message).await {
            Ok(()) => {
                // chat_id is not logged: it identifies a person or a group.
                tracing::info!(
                    session = %context.session_id,
                    explicit_target = requested.is_some(),
                    chars = text.chars().count(),
                    "telegram_send: mensagem entregue"
                );
                Ok(ToolOutput::success("mensagem enviada no Telegram"))
            }
            Err(e) => Ok(ToolOutput::error(format!("falha ao enviar: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(session: &str) -> ToolContext {
        ToolContext {
            session_id: session.to_string(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        }
    }

    /// Returns the `Arc` alongside the tool: the tool holds only a `Weak`, so
    /// a fixture that drops the state would exercise the shutdown path instead
    /// of the branch under test. That the tests have to hold it is the visible
    /// consequence of not having an `Arc` cycle.
    fn tool() -> (Arc<AppState>, TelegramSendTool) {
        // No session store wired: exercises the explicit-target branch, which
        // is decided before any store lookup.
        let state = Arc::new(crate::state::AppState::new(
            garraia_config::AppConfig::default(),
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        ));
        let t = TelegramSendTool::new(&state);
        (state, t)
    }

    /// Deny-by-default. With nothing configured, an explicit recipient is
    /// refused and the error names the config key the operator needs.
    #[tokio::test]
    async fn explicit_target_is_refused_when_no_allowlist_is_configured() {
        let (_state, t) = tool();
        let err = t
            .resolve_target(&ctx("s1"), Some(12345), &ProactiveTargets::default())
            .await
            .expect_err("deve recusar");
        assert!(err.contains("proactive_chat_ids"), "err = {err}");
    }

    /// A configured allowlist still refuses everything outside it.
    #[tokio::test]
    async fn explicit_target_outside_the_allowlist_is_refused() {
        let (_state, t) = tool();
        let err = t
            .resolve_target(&ctx("s1"), Some(12345), &ProactiveTargets::from_ids([999]))
            .await
            .expect_err("deve recusar");
        assert!(err.contains("não está em"), "err = {err}");
        // The rejected id must not be echoed back — it is someone's chat.
        assert!(!err.contains("12345"), "err = {err}");
    }

    #[tokio::test]
    async fn explicit_target_inside_the_allowlist_is_allowed() {
        let (_state, t) = tool();
        let allowed = ProactiveTargets::from_ids([12345, -1009]);
        assert_eq!(
            t.resolve_target(&ctx("s1"), Some(12345), &allowed).await,
            Ok(12345)
        );
        assert_eq!(
            t.resolve_target(&ctx("s1"), Some(-1009), &allowed).await,
            Ok(-1009)
        );
    }

    /// The allowlist is never consulted for the implicit case, but without a
    /// session store there is nothing to resolve — and the error says so
    /// rather than falling back to some default chat.
    #[tokio::test]
    async fn implicit_target_without_a_session_store_fails_closed() {
        let (_state, t) = tool();
        let err = t
            .resolve_target(&ctx("s1"), None, &ProactiveTargets::from_ids([12345]))
            .await
            .expect_err("sem store não há chat");
        assert!(err.contains("session store"), "err = {err}");
    }

    #[tokio::test]
    async fn rejects_empty_and_oversized_text() {
        let (_state, t) = tool();

        let out = t
            .execute(&ctx("s1"), serde_json::json!({"text": "   "}))
            .await
            .unwrap();
        assert!(out.is_error);

        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        let out = t
            .execute(&ctx("s1"), serde_json::json!({"text": long}))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("muito longa"));

        let out = t.execute(&ctx("s1"), serde_json::json!({})).await.unwrap();
        assert!(out.is_error);
    }

    /// Finding LOW: uma remoção de `proactive_chat_ids` tem de valer sem
    /// restart. `targets()` lê da config viva, então o teste altera a config e
    /// espera que a decisão mude.
    #[test]
    fn allowlist_is_read_from_the_live_config() {
        let mut config = garraia_config::AppConfig::default();
        let mut settings = std::collections::HashMap::new();
        settings.insert("proactive_chat_ids".to_string(), serde_json::json!([555]));
        config.channels.insert(
            "tg".to_string(),
            garraia_config::model::ChannelConfig {
                channel_type: "telegram".into(),
                enabled: Some(true),
                settings,
            },
        );
        let state = crate::state::AppState::new(
            config,
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        );
        assert!(TelegramSendTool::targets(&state).allows(555));

        // Config sem a lista → nada permitido.
        let empty = crate::state::AppState::new(
            garraia_config::AppConfig::default(),
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        );
        assert!(TelegramSendTool::targets(&empty).is_empty());
    }

    /// Finding MEDIUM: o teto por sessão existe e é cobrado. Um alvo recusado
    /// **não** consome cota — senão um modelo sondando chat_ids trancaria o
    /// usuário fora das próprias notificações.
    #[tokio::test]
    async fn refused_sends_do_not_burn_the_budget() {
        let (_state, t) = tool();
        for _ in 0..(crate::channel_send::MAX_SENDS_PER_WINDOW + 3) {
            let out = t
                .execute(
                    &ctx("s1"),
                    serde_json::json!({"text": "oi", "chat_id": 999}),
                )
                .await
                .unwrap();
            assert!(out.is_error);
            // A recusa é do allowlist, nunca do teto.
            assert!(
                out.content.contains("proactive_chat_ids"),
                "{}",
                out.content
            );
        }
    }

    /// Schema sanity: `chat_id` must stay optional, or the model would be
    /// forced into the allowlist-gated path for ordinary replies.
    #[test]
    fn only_text_is_required() {
        let (_state, t) = tool();
        let schema = t.input_schema();
        assert_eq!(schema["required"], serde_json::json!(["text"]));
    }
}
