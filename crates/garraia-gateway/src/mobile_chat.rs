//! GAR-339: Mobile Chat — authenticated chat endpoints for Garra Cloud Alpha.
//!
//! Endpoints:
//!   POST /chat           — send a message, get a response
//!   GET  /chat/history   — return recent message history for the authenticated user

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use garraia_db::StoredMessage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use crate::mobile_auth::MobileAuth;
use crate::state::AppState;
use garraia_db::SessionStore;

/// Personalidade padrão do Garra para o app mobile.
/// Pode ser sobrescrita pela variável de ambiente `GARRA_MOBILE_PERSONA`.
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

fn garra_persona() -> &'static str {
    // Leak once — safe for a long-running server process.
    // If the env var is set, it overrides the default persona.
    use std::sync::OnceLock;
    static PERSONA: OnceLock<String> = OnceLock::new();
    PERSONA.get_or_init(|| {
        std::env::var("GARRA_MOBILE_PERSONA")
            .unwrap_or_else(|_| DEFAULT_PERSONA.to_string())
    })
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
    let persona = garra_persona();
    let result: Result<String, _> = state
        .agents
        .process_message_with_agent_config(
            &session_id,
            &req.message,
            &[],           // history already hydrated into runtime session
            Some(&session_id),
            Some(&user_id),
            None,          // provider: use default
            None,          // model: use default
            Some(persona), // Garra personality system prompt
            None,          // max_tokens: use default
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
                Json(serde_json::json!(ChatResponse {
                    reply,
                    session_id,
                })),
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
