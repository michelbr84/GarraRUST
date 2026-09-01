use axum::extract::State;
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
///     { "type": "chunk",    "text": "..." }   // streaming deltas, in order
///     { "type": "response", "text": "..." }   // final full text, authoritative
///     { "type": "error",    "message": "..." }
///
/// Streaming: each LLM delta becomes one "chunk" frame; the closing
/// "response" always carries the complete accumulated text, so a client that
/// ignores (or misses) chunks still renders the same conversation.
///
/// The desktop always uses the fixed session ID "parrot-desktop" so history
/// persists across gateway restarts and overlay reconnections.
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use tracing::{info, warn};

use crate::state::SharedState;

const SESSION_ID: &str = "parrot-desktop";
const CHANNEL: &str = "desktop";

pub async fn parrot_ws_handler(State(state): State<SharedState>, ws: WebSocketUpgrade) -> Response {
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
            serde_json::json!({ "type": "connected" })
                .to_string()
                .into(),
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

        // Build history and call the agent (streaming: deltas viram frames
        // "chunk"; o texto final segue num "response" autoritativo).
        let history = state.session_history(SESSION_ID);
        let continuity_key = state.continuity_key(None);

        let (delta_tx, delta_rx) = tokio::sync::mpsc::channel::<String>(100);
        let agents = state.agents.clone();
        let text_for_agent = user_text.clone();
        let task = tokio::spawn(async move {
            agents
                .process_message_streaming_with_agent_config(
                    SESSION_ID,
                    &text_for_agent,
                    &history,
                    delta_tx,
                    continuity_key.as_deref(),
                    None,
                    None, // use default provider
                    None, // use default model
                    None,
                    None,
                )
                .await
        });

        // Drena os deltas ENQUANTO o agente roda: o canal é limitado, então
        // esperar o task terminar antes de ler deadlockaria o produtor — a
        // mesma lição do stream_turn documentada em garraia-cli/src/chat.rs.
        forward_deltas(&mut sender, delta_rx).await;

        let reply_payload = match task.await {
            Ok(Ok(response_text)) => {
                state
                    .persist_turn(SESSION_ID, Some(CHANNEL), None, &user_text, &response_text)
                    .await;
                response_payload(&response_text)
            }
            Ok(Err(e)) => {
                warn!("agent error: session={SESSION_ID}, error={e}");
                error_payload(&e.to_string())
            }
            Err(e) => {
                warn!("agent task failed: session={SESSION_ID}, error={e}");
                error_payload("internal agent task failure")
            }
        };

        if sender
            .send(Message::Text(reply_payload.into()))
            .await
            .is_err()
        {
            break;
        }
    }

    state.disconnect_session(SESSION_ID);
    info!("Garra Desktop disconnected: session={SESSION_ID}");
}

/// Encaminha cada delta do agente como um frame `chunk` até o canal fechar
/// (produtor terminou). Genérico sobre o sink para ser testável sem socket.
/// Retorna `false` quando o socket morreu no meio do stream — nesse caso o
/// canal ainda é drenado até o fim, para o produtor (que usa `send().await`
/// num canal limitado) nunca ficar bloqueado.
async fn forward_deltas<S>(sender: &mut S, mut rx: tokio::sync::mpsc::Receiver<String>) -> bool
where
    S: futures::Sink<Message> + Unpin,
{
    while let Some(delta) = rx.recv().await {
        if sender
            .send(Message::Text(chunk_payload(&delta).into()))
            .await
            .is_err()
        {
            while rx.recv().await.is_some() {}
            return false;
        }
    }
    true
}

fn chunk_payload(text: &str) -> String {
    serde_json::json!({ "type": "chunk", "text": text }).to_string()
}

fn response_payload(text: &str) -> String {
    serde_json::json!({ "type": "response", "text": text }).to_string()
}

fn error_payload(message: &str) -> String {
    serde_json::json!({ "type": "error", "message": message }).to_string()
}

/// Extract `text` from `{"type":"message","text":"..."}`, or return None.
fn parse_message(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("type")?.as_str()? != "message" {
        return None;
    }
    v.get("text")?.as_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_extracts_text() {
        let raw = r#"{"type":"message","text":"olá garra"}"#;
        assert_eq!(parse_message(raw).as_deref(), Some("olá garra"));
    }

    #[test]
    fn parse_message_rejects_other_types_and_missing_text() {
        assert_eq!(parse_message(r#"{"type":"ping","text":"x"}"#), None);
        assert_eq!(parse_message(r#"{"type":"message"}"#), None);
        assert_eq!(parse_message("not json"), None);
    }

    #[test]
    fn payloads_have_the_documented_shapes() {
        let chunk: serde_json::Value =
            serde_json::from_str(&chunk_payload("abc")).expect("chunk json");
        assert_eq!(chunk["type"], "chunk");
        assert_eq!(chunk["text"], "abc");

        let response: serde_json::Value =
            serde_json::from_str(&response_payload("done")).expect("response json");
        assert_eq!(response["type"], "response");
        assert_eq!(response["text"], "done");

        let error: serde_json::Value =
            serde_json::from_str(&error_payload("boom")).expect("error json");
        assert_eq!(error["type"], "error");
        assert_eq!(error["message"], "boom");
    }

    #[tokio::test]
    async fn forward_deltas_emits_one_chunk_frame_per_delta_in_order() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        tx.send("olá ".to_string()).await.expect("send first delta");
        tx.send("mundo".to_string())
            .await
            .expect("send second delta");
        // Produtor termina: dropar o sender fecha o canal e encerra o loop.
        drop(tx);

        let (mut sink, mut collected) = futures::channel::mpsc::unbounded::<Message>();
        assert!(forward_deltas(&mut sink, rx).await);
        drop(sink);

        let mut frames = Vec::new();
        while let Ok(Message::Text(text)) = collected.try_recv() {
            frames.push(text.to_string());
        }
        assert_eq!(frames.len(), 2);
        let first: serde_json::Value = serde_json::from_str(&frames[0]).expect("frame json");
        assert_eq!(first["type"], "chunk");
        assert_eq!(first["text"], "olá ");
        let second: serde_json::Value = serde_json::from_str(&frames[1]).expect("frame json");
        assert_eq!(second["text"], "mundo");
    }
}
