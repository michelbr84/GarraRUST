//! GAR-339: Mobile Chat — authenticated chat endpoints for Garra Cloud Alpha.
//!
//! Endpoints:
//!   POST /chat           — send a message, get a response
//!   GET  /chat/history   — return recent message history for the authenticated user

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use garraia_db::StoredMessage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use crate::mobile_auth::MobileAuth;
use crate::state::AppState;
use garraia_db::SessionStore;

/// Personalidade padrão do Garra para o app mobile. Plan 0042 (GAR-379
/// slice 2) moved the persona source-of-truth to `config.mobile.persona`
/// with env + default as fallbacks; this constant is the last-resort
/// default shipped with the binary.
const DEFAULT_PERSONA: &str = r#"Você é o Garra, um assistente pessoal de IA com personalidade marcante.

Seu estilo:
- Você é animado, engraçado e levemente irreverente — como um amigo inteligente, não um robô corporativo.
- Use linguagem natural e descontraída em português brasileiro. Nada de formalidade excessiva.
- Seja conciso: prefira respostas curtas e diretas. Se o assunto precisar de mais, organize em tópicos rápidos.
- Use emojis com moderação para expressar emoção — não force.
- Quando não souber algo, admita com humor em vez de inventar.
- Comemore conquistas do usuário com entusiasmo genuíno (como o Duolingo faz).
- Nunca seja condescendente. Você torce pelo usuário.

Sua missão: tornar cada interação útil E divertida. O usuário deve sair da conversa tendo aprendido algo ou resolvido algo — e com vontade de voltar."#;

/// Resolve the persona string with precedence **config > env > default**
/// (plan 0042 §5.1).
///
/// The old `OnceLock` cache is gone. `AppState` itself is handed to
/// handlers as `Arc<AppState>`, so the call site already pays exactly
/// one Arc-deref to reach `state.config`; the `.mobile.persona`
/// lookup is a plain field access + `Option::as_deref` — cheap
/// enough (< 100 ns) that we do not need a OnceLock to amortise it.
/// Dropping the cache is what lets future config hot-reload be seen
/// immediately (code review MEDIUM clarification: the `Arc` is on
/// `AppState`, not on `config` itself).
///
/// `std::env::var` is queried per call. That is intentional: the env
/// fallback is only exercised when `config.mobile.persona` is unset,
/// which is the dev/legacy path. Caching via `OnceLock` here would
/// cement whatever value the env had at first read and defeat
/// per-test overrides.
fn garra_persona(state: &AppState) -> String {
    pick_persona(
        normalise_persona_source(state.config.mobile.persona.as_deref()),
        normalise_persona_source(std::env::var("GARRA_MOBILE_PERSONA").ok().as_deref()),
    )
}

/// Pure precedence helper — kept separate from the env/state reads so
/// the 3-tier fallback can be unit-tested without spinning up an
/// `AppState` or touching process env.
fn pick_persona(from_config: Option<&str>, from_env: Option<&str>) -> String {
    if let Some(p) = from_config {
        return p.to_string();
    }
    if let Some(p) = from_env {
        return p.to_string();
    }
    DEFAULT_PERSONA.to_string()
}

/// Security audit SEC-L normalisation: treat an empty or
/// whitespace-only value as *absent* so we do not send a blank
/// system prompt to the LLM. Applied to both config and env sources
/// before they reach [`pick_persona`].
fn normalise_persona_source(raw: Option<&str>) -> Option<&str> {
    raw.filter(|s| !s.trim().is_empty())
}

/// Session key prefix for mobile users.
/// Maps each user to a stable session in the existing conversation system.
fn mobile_session_id(user_id: &str) -> String {
    format!("mobile-{}", user_id)
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub reply: String,
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub messages: Vec<HistoryMessage>,
    pub session_id: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /chat
pub async fn chat(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    if req.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "message cannot be empty"})),
        );
    }

    let session_id = mobile_session_id(&claims.sub);
    let user_id = claims.sub.clone();

    // Hydrate session history from DB so context is preserved across app restarts.
    state
        .hydrate_session_history(&session_id, Some("mobile"), Some(&user_id))
        .await;

    // Process the message through the agent runtime.
    let persona = garra_persona(&state);
    let result: Result<String, _> = state
        .agents
        .process_message_with_agent_config(
            &session_id,
            &req.message,
            &[], // history already hydrated into runtime session
            Some(&session_id),
            Some(&user_id),
            None,           // provider: use default
            None,           // model: use default
            Some(&persona), // Garra personality system prompt
            None,           // max_tokens: use default
        )
        .await;

    match result {
        Ok(reply) => {
            // Persist turn to DB.
            state
                .persist_turn(
                    &session_id,
                    Some("mobile"),
                    Some(&user_id),
                    &req.message,
                    &reply,
                )
                .await;

            // Fire-and-forget summarization.
            {
                let summ_state = state.clone();
                let summ_session = session_id.clone();
                tokio::spawn(async move {
                    crate::context_summarizer::maybe_trigger_summarization(
                        summ_state,
                        summ_session,
                    )
                    .await;
                });
            }

            (
                StatusCode::OK,
                Json(serde_json::json!(ChatResponse { reply, session_id })),
            )
        }
        Err(e) => {
            warn!("mobile chat: agent error for user {}: {e}", claims.sub);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "agent error", "detail": e.to_string()})),
            )
        }
    }
}

/// GET /chat/history
pub async fn history(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let session_id = mobile_session_id(&claims.sub);

    let store_arc: Arc<Mutex<SessionStore>> = match &state.session_store {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        }
    };

    let messages: Vec<StoredMessage> = {
        let store = store_arc.lock().await;
        match store.load_recent_messages(&session_id, 50) {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!("mobile history: DB error for user {}: {e}", claims.sub);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "failed to load history"})),
                );
            }
        }
    };

    let history: Vec<HistoryMessage> = messages
        .into_iter()
        .map(|m| HistoryMessage {
            role: m.direction.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp.to_rfc3339(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!(HistoryResponse {
            messages: history,
            session_id,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_prefers_config_over_env_and_default() {
        let resolved = pick_persona(Some("config-persona"), Some("env-persona"));
        assert_eq!(resolved, "config-persona");
    }

    #[test]
    fn persona_falls_back_to_env_when_config_absent() {
        let resolved = pick_persona(None, Some("env-persona"));
        assert_eq!(resolved, "env-persona");
    }

    #[test]
    fn persona_falls_back_to_default_when_neither_set() {
        let resolved = pick_persona(None, None);
        assert_eq!(resolved, DEFAULT_PERSONA);
    }

    #[test]
    fn normalise_treats_empty_and_whitespace_as_absent() {
        // Security audit SEC-L: an operator that sets
        // `GARRA_MOBILE_PERSONA=""` or a whitespace-only value should
        // not silently ship a blank system prompt to the LLM. The
        // normaliser upgrades both to `None`, so the fallback chain
        // walks to the next source.
        assert_eq!(normalise_persona_source(Some("")), None);
        assert_eq!(normalise_persona_source(Some("   ")), None);
        assert_eq!(normalise_persona_source(Some("\t\n")), None);
        assert_eq!(
            normalise_persona_source(Some("real persona")),
            Some("real persona"),
        );
        assert_eq!(normalise_persona_source(None), None);
    }

    #[test]
    fn persona_empty_config_falls_through_to_env() {
        // Regression guard — empty/whitespace config source must be
        // upgraded to `None` by the caller so env (or default) wins.
        let resolved = pick_persona(
            normalise_persona_source(Some("   ")),
            normalise_persona_source(Some("env-persona")),
        );
        assert_eq!(resolved, "env-persona");
    }

    #[test]
    fn persona_empty_env_falls_through_to_default() {
        let resolved = pick_persona(
            normalise_persona_source(None),
            normalise_persona_source(Some("")),
        );
        assert_eq!(resolved, DEFAULT_PERSONA);
    }
}
