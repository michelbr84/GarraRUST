//! OpenAI-Compatible API endpoints for VS Code integration
//!
//! This module provides `/v1/chat/completions` endpoint that is compatible
//! with the OpenAI API format, enabling VS Code extensions to connect.

use axum::{
    extract::State,
    http::HeaderMap,
    response::Json,
    routing::{get, post},
    Router,
};
use garraia_agents::{ChatMessage, ChatRole, MessagePart};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::state::SharedState;

/// Build the OpenAI-compatible router
pub fn build_openai_router(state: SharedState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
        .with_state(state)
}

// ============================================================================
// Request/Response Types (OpenAI-compatible)
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageInput {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionRequest {
    /// Model to use (e.g., "gpt-4", "claude-3-opus")
    pub model: Option<String>,
    /// List of messages
    pub messages: Vec<ChatMessageInput>,
    /// Temperature (0.0 - 2.0)
    pub temperature: Option<f32>,
    /// Top p (0.0 - 1.0)
    pub top_p: Option<f32>,
    /// Number of messages to keep in history
    pub max_tokens: Option<i32>,
    /// Whether to stream the response
    pub stream: Option<bool>,
    /// Optional: stop sequences
    pub stop: Option<Vec<String>>,
    /// Optional: presence penalty
    pub presence_penalty: Option<f32>,
    /// Optional: frequency penalty
    pub frequency_penalty: Option<f32>,
    /// Optional: user identifier
    pub user: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub index: i32,
    pub message: ResponseMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

// ============================================================================
// Request Handlers
// ============================================================================

/// POST /v1/chat/completions
/// OpenAI-compatible chat completions endpoint
pub async fn chat_completions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (axum::http::StatusCode, String)> {
    // Extract session ID from headers or create new one
    let session_id = resolve_session_id(&headers).await?;

    info!(
        "OpenAI API request: session_id={}, model={:?}, stream={}",
        session_id,
        body.model,
        body.stream.unwrap_or(false)
    );

    // Convert messages to garraia format
    let messages: Vec<ChatMessage> = body
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: parse_role(&m.role),
            content: MessagePart::Text(m.content.clone()),
        })
        .collect();

    // Get continuity key (default if not available)
    let continuity_key = state.continuity_key(None).unwrap_or_else(|| "default".to_string());

    // Call the agent using process_message_with_context
    let response = state
        .agents
        .process_message_with_context(
            &session_id,
            &continuity_key,
            &messages,
            None,
            None,
        )
        .await
        .map_err(|e| {
            error!("Agent error: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Agent error: {}", e),
            )
        })?;

    // Build response
    let response_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let usage = Usage {
        prompt_tokens: 0, // Would need to track this from the provider
        completion_tokens: 0,
        total_tokens: 0,
    };

    let model = body.model.unwrap_or_else(|| "gpt-4".to_string());

    let resp = ChatCompletionResponse {
        id: response_id,
        object: "chat.completion".to_string(),
        created: now,
        model: model.clone(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".to_string(),
                content: response.clone(),
            },
            finish_reason: "stop".to_string(),
        }],
        usage,
    };

    // Persist the turn to database if session store is available
    if let Some(ref session_store) = state.session_store {
        let store = session_store.lock().await;
        let user_content = body
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        
        let _ = store.append_message(
            &session_id,
            "user",
            &user_content,
            chrono::Utc::now(),
            &serde_json::json!({ "source": "vscode", "model": model }),
        );
        let _ = store.append_message(
            &session_id,
            "assistant",
            &response,
            chrono::Utc::now(),
            &serde_json::json!({ "source": "vscode", "model": model }),
        );
    }

    Ok(Json(resp))
}

/// Resolve session ID from headers or create new one
async fn resolve_session_id(headers: &HeaderMap) -> Result<String, (axum::http::StatusCode, String)> {
    // Try X-Session-Id header first
    if let Some(session_id) = headers.get("x-session-id") {
        if let Ok(s) = session_id.to_str() {
            return Ok(s.to_string());
        }
    }

    // Create new session
    Ok(uuid::Uuid::new_v4().to_string())
}

/// Parse role string to ChatRole
fn parse_role(role: &str) -> ChatRole {
    match role.to_lowercase().as_str() {
        "system" => ChatRole::System,
        "user" => ChatRole::User,
        "assistant" => ChatRole::Assistant,
        "tool" => ChatRole::Tool,
        _ => ChatRole::User,
    }
}

/// GET /v1/models - List available models
pub async fn list_models() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "object": "list",
        "data": [
            {
                "id": "gpt-4",
                "object": "model",
                "created": 1687882411,
                "owned_by": "openai"
            },
            {
                "id": "gpt-4-turbo",
                "object": "model",
                "created": 1704067200,
                "owned_by": "openai"
            },
            {
                "id": "gpt-3.5-turbo",
                "object": "model",
                "created": 1677649963,
                "owned_by": "openai"
            }
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role() {
        assert!(matches!(parse_role("system"), ChatRole::System));
        assert!(matches!(parse_role("user"), ChatRole::User));
        assert!(matches!(parse_role("assistant"), ChatRole::Assistant));
        assert!(matches!(parse_role("SYSTEM"), ChatRole::System));
    }
}
