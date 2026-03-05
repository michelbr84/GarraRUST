/// WebSocket handler for the Garra Desktop overlay — GET /ws/parrot
///
/// Protocol (all messages are JSON):
///
///   Client → Server:
///     { "type": "message", "text": "..." }
///
///   Server → Client:
///     { "type": "connected" }
///     { "type": "thinking" }
///     { "type": "response", "text": "..." }
///     { "type": "error",    "message": "..." }
///
/// The desktop always uses the fixed session ID "parrot-desktop" so history
/// persists across gateway restarts and overlay reconnections.
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use tracing::{info, warn};

use crate::state::SharedState;

const SESSION_ID: &str = "parrot-desktop";
const CHANNEL: &str = "desktop";

pub async fn parrot_ws_handler(
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_parrot_socket(socket, state))
}

async fn handle_parrot_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();

    // Hydrate persistent history for the desktop session
    state
        .hydrate_session_history(SESSION_ID, Some(CHANNEL), None)
        .await;

    info!("Garra Desktop connected: session={SESSION_ID}");

    // Greet the overlay
    let _ = sender
        .send(Message::Text(
            serde_json::json!({ "type": "connected" }).to_string().into(),
        ))
        .await;

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let user_text = match parse_message(&text) {
            Some(t) => t,
            None => continue,
        };

        // Security: sanitize and check prompt injection
        let user_text = garraia_security::InputValidator::sanitize(&user_text);
        if garraia_security::InputValidator::check_prompt_injection(&user_text) {
            warn!("prompt injection blocked: session={SESSION_ID}");
            let _ = sender
                .send(Message::Text(
                    serde_json::json!({
                        "type": "error",
                        "message": "input rejected: potential prompt injection"
                    })
                    .to_string()
                    .into(),
                ))
                .await;
            continue;
        }

        // Notify the overlay that the agent is thinking
        if sender
            .send(Message::Text(
                serde_json::json!({ "type": "thinking" }).to_string().into(),
            ))
            .await
            .is_err()
        {
            break;
        }

        // Build history and call the agent
        let history = state.session_history(SESSION_ID);
        let continuity_key = state.continuity_key(None);

        let reply_payload = match state
            .agents
            .process_message_with_agent_config(
                SESSION_ID,
                &user_text,
                &history,
                continuity_key.as_deref(),
                None,
                None, // use default provider
                None, // use default model
                None,
                None,
            )
            .await
        {
            Ok(response_text) => {
                state
                    .persist_turn(SESSION_ID, Some(CHANNEL), None, &user_text, &response_text)
                    .await;
                serde_json::json!({ "type": "response", "text": response_text })
            }
            Err(e) => {
                warn!("agent error: session={SESSION_ID}, error={e}");
                serde_json::json!({ "type": "error", "message": e.to_string() })
            }
        };

        if sender
            .send(Message::Text(reply_payload.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }

    state.disconnect_session(SESSION_ID);
    info!("Garra Desktop disconnected: session={SESSION_ID}");
}

/// Extract `text` from `{"type":"message","text":"..."}`, or return None.
fn parse_message(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("type")?.as_str()? != "message" {
        return None;
    }
    v.get("text")?.as_str().map(|s| s.to_string())
}
