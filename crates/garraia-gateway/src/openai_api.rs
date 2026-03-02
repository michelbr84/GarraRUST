//! OpenAI-Compatible API endpoints for VS Code integration
//!
//! This module provides `/v1/chat/completions` endpoint that is compatible
//! with the OpenAI API format, enabling VS Code extensions to connect.
//! Supports both streaming (SSE) and non-streaming modes.

use std::convert::Infallible;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response},
    response::{sse::{Event, Sse}, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures::stream::{self, StreamExt};
use garraia_agents::{ChatMessage, ChatRole, MessagePart};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

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
    #[serde(default)]
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
    /// GAR-225: Optional - tools available for the model
    #[serde(default)]
    pub tools: Option<Vec<ToolDefinition>>,
    /// GAR-225: Optional - controls which tool to use (none/auto/required/named)
    /// When not specified, defaults to auto behavior
    /// Accepts: "none", "auto", "required", or {"type": "function", "function": {"name": "..."}}
    #[serde(default, rename = "tool_choice")]
    pub tool_choice: serde_json::Value,
}

/// Tool definition for OpenAI-compatible API
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Tool type (currently only "function" is supported)
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Function definition
    pub function: Option<FunctionDefinition>,
}

/// Function definition within a tool
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDefinition {
    /// Function name
    pub name: String,
    /// Function description
    pub description: Option<String>,
    /// JSON schema for function parameters
    pub parameters: Option<serde_json::Value>,
}

/// Tool choice string value (none/auto/required)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoiceString {
    /// Unknown value (must be last in untagged enum)
    Unknown(String),
    /// Don't use any tools
    None,
    /// Let the model decide
    Auto,
    /// Force at least one tool call
    Required,
}

/// Tool choice with auto default - simplifies parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoiceAuto {
    /// Force a specific tool by name (must be checked first due to untagged)
    Function(FunctionChoice),
    /// Not specified - use default behavior
    StringChoice(ToolChoiceString),
}

impl Default for ToolChoiceAuto {
    fn default() -> Self {
        ToolChoiceAuto::StringChoice(ToolChoiceString::Unknown(String::new()))
    }
}

/// Force a specific function to be called
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionChoice {
    /// The type (always "function")
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The name of the function to force
    pub function: FunctionNameOnly,
}

/// Function name only
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionNameOnly {
    /// The function name
    pub name: String,
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
// Streaming Response Types (SSE)
// ============================================================================

/// Chunk sent during streaming (OpenAI format)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkChoice {
    pub index: i32,
    pub delta: DeltaContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ============================================================================
// Request Handlers
// ============================================================================

/// POST /v1/chat/completions
/// OpenAI-compatible chat completions endpoint with streaming support
pub async fn chat_completions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<ChatCompletionRequest>,
) -> Response<Body> {
    // Extract request ID for tracing (GAR-234)
    let request_id = resolve_request_id(&headers);
    
    // Extract session ID from headers or create new one
    let session_id = resolve_session_id(&headers).await.unwrap_or_else(|_| Uuid::new_v4().to_string());
    let is_streaming = body.stream.unwrap_or(false);

    // Resolve user identity from Authorization header
    let user_id = resolve_user_id(&headers, &state);
    
    // GAR-234/238: Extract mode from header for logging and apply to session
    let agent_mode = headers
        .get("x-agent-mode")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    // GAR-234: Also check for mode prefix in user message (fallback: "mode: debug")
    let message_mode = body
        .messages
        .last()
        .and_then(|m| {
            let content = m.content.to_lowercase();
            // Check for "mode: <mode>" or "/mode <mode>" patterns
            if content.starts_with("mode: ") {
                Some(content.strip_prefix("mode: ").unwrap_or("").trim().to_string())
            } else if content.starts_with("/mode ") {
                Some(content.strip_prefix("/mode ").unwrap_or("").trim().to_string())
            } else {
                None
            }
        });
    
    // Use header mode first, then fall back to message mode
    let final_mode = agent_mode.or(message_mode);
    
    // Apply X-Agent-Mode header to session if provided (GAR-234)
    if let Some(ref mode) = final_mode {
        if let Some(ref session_store) = state.session_store {
            let store = session_store.lock().await;
            let _ = store.set_agent_mode(&session_id, mode);
        }
    }
    
    // GAR-225: Extract tool_choice for standardized logging
    let tool_choice_str = if body.tool_choice.is_null() {
        None
    } else if let Some(s) = body.tool_choice.as_str() {
        Some(s.to_string())
    } else if let Some(obj) = body.tool_choice.as_object() {
        if let Some(name) = obj.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()) {
            Some(format!("function:{}", name))
        } else {
            Some(body.tool_choice.to_string())
        }
    } else {
        Some(body.tool_choice.to_string())
    };
    
    info!(
        "OpenAI API request: request_id={}, session_id={}, user_id={:?}, model={:?}, stream={}, mode={:?}, tool_choice={:?}",
        request_id,
        session_id,
        user_id,
        body.model,
        is_streaming,
        final_mode,
        tool_choice_str
    );

    // Get model name
    let model = body.model.clone().unwrap_or_else(|| "gpt-4".to_string());

    // Convert messages to garraia format
    let messages: Vec<ChatMessage> = body
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: parse_role(&m.role),
            content: MessagePart::Text(m.content.clone()),
        })
        .collect();

    // Get continuity key based on user_id
    let continuity_key = state.continuity_key(user_id.as_deref()).unwrap_or_else(|| "default".to_string());

    if is_streaming {
        // Handle streaming mode
        handle_streaming(state, session_id, model, messages, continuity_key, user_id).await
    } else {
        // Handle non-streaming mode (original behavior)
        handle_non_streaming(state, session_id, model, messages, continuity_key, body, user_id).await
    }
}

/// Handle streaming request - connects internal streaming to SSE
async fn handle_streaming(
    state: SharedState,
    session_id: String,
    model: String,
    messages: Vec<ChatMessage>,
    continuity_key: String,
    user_id: Option<String>,
) -> Response<Body> {
    // Create channel for streaming deltas
    let (delta_tx, delta_rx) = mpsc::channel::<String>(100);

    // Get the user message (last message)
    let user_message = messages
        .last()
        .map(|m| {
            if let MessagePart::Text(t) = &m.content {
                t.clone()
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    // Clone what we need for the spawned task
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();
    let continuity_key_clone = continuity_key.clone();
    let user_msg_clone = user_message.clone();
    let messages_clone = messages.clone();
    let model_clone = model.clone();
    let user_id_clone = user_id.clone();

    // Spawn task to process streaming
    tokio::spawn(async move {
        let _ = state_clone
            .agents
            .process_message_streaming_with_context(
                &session_id_clone,
                &user_msg_clone,
                &messages_clone,
                delta_tx,
                Some(&continuity_key_clone),
                user_id_clone.as_deref(),
                Some(model_clone.as_str()),
            )
            .await;
    });

    // Prepare SSE streaming response
    let chunk_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Create the content stream from delta_rx
    let stream = stream::unfold(
        (delta_rx, chunk_id, created, model),
        |(mut rx, chunk_id, created, model)| async move {
            match rx.recv().await {
                Some(text) => {
                    let chunk = ChatCompletionChunk {
                        id: chunk_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: DeltaContent {
                                role: None,
                                content: Some(text),
                            },
                            finish_reason: None,
                        }],
                    };
                    
                    let event = Event::default()
                        .data(serde_json::to_string(&chunk).unwrap_or_default());
                    
                    Some((event, (rx, chunk_id, created, model)))
                }
                None => None,
            }
        },
    );

    // Convert to Result stream for Sse
    let result_stream = stream.map(Ok::<_, Infallible>);

    Sse::new(result_stream).into_response()
}

/// Handle non-streaming request (original behavior)
async fn handle_non_streaming(
    state: SharedState,
    session_id: String,
    model: String,
    messages: Vec<ChatMessage>,
    continuity_key: String,
    body: ChatCompletionRequest,
    user_id: Option<String>,
) -> Response<Body> {
    // Get the user message (last message)
    let user_text = messages
        .last()
        .map(|m| {
            if let MessagePart::Text(t) = &m.content {
                t.clone()
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    // Get conversation history (all messages except the last one)
    let conversation_history = &messages[..messages.len().saturating_sub(1)];

    // Call the agent using process_message_with_agent_config to pass model_override
    let result = state
        .agents
        .process_message_with_agent_config(
            &session_id,
            &user_text,
            conversation_history,
            Some(continuity_key.as_str()),
            user_id.as_deref(),
            None,
            Some(model.as_str()),
            None,
            None,
        )
        .await;

    match result {
        Ok(response) => {
            // Build response
            let response_id = format!("chatcmpl-{}", Uuid::new_v4());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let usage = Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            };

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

            Json(resp).into_response()
        }
        Err(e) => {
            error!("Agent error: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Agent error: {}", e),
            ).into_response()
        }
    }
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
    Ok(Uuid::new_v4().to_string())
}

/// GAR-234: Resolve request ID from headers for tracing
/// Returns the X-Request-Id if provided, otherwise generates a new one
fn resolve_request_id(headers: &HeaderMap) -> String {
    if let Some(request_id) = headers.get("x-request-id") {
        if let Ok(s) = request_id.to_str() {
            return s.to_string();
        }
    }
    // Generate new request ID if not provided
    Uuid::new_v4().to_string()
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

/// Resolve user identity from Authorization header.
/// Maps API key to user_id for authenticated access.
fn resolve_user_id(headers: &HeaderMap, state: &SharedState) -> Option<String> {
    // Try Authorization header first
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            // Handle "Bearer <token>" format
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let token = token.trim();
                
                // Check if it's a valid API key from the config
                // The token "garra-local" maps to the local owner
                if token == "garra-local" {
                    // Get the owner from allowlist
                    if let Ok(list) = state.allowlist.lock() {
                        if let Some(owner) = list.owner() {
                            info!("Resolved user_id={} from 'garra-local' token", owner);
                            return Some(owner.to_string());
                        }
                    }
                }
                
                // For other tokens, use the token itself as user_id
                // This allows custom API keys to identify users
                if !token.is_empty() {
                    info!("Resolved user_id={} from API token", token);
                    return Some(token.to_string());
                }
            }
        }
    }
    
    // Try X-User-Id header
    if let Some(user_id_header) = headers.get("x-user-id") {
        if let Ok(user_id) = user_id_header.to_str() {
            return Some(user_id.to_string());
        }
    }
    
    None
}

/// GET /v1/models - List available models
pub async fn list_models() -> Response<Body> {
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
            },
            {
                "id": "claude-3-opus",
                "object": "model",
                "created": 1709596800,
                "owned_by": "anthropic"
            },
            {
                "id": "claude-3-sonnet",
                "object": "model",
                "created": 1709596800,
                "owned_by": "anthropic"
            }
        ]
    })).into_response()
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
    
    // GAR-225: Testes de tool_choice parsing
    #[test]
    fn test_tool_choice_none() {
        let json = r#"{"tool_choice": "none"}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        println!("Parsed tool_choice: {:?}", req.tool_choice);
        assert!(!req.tool_choice.is_null(), "tool_choice should not be null");
        assert!(req.tool_choice.is_string(), "tool_choice should be string");
    }
    
    #[test]
    fn test_tool_choice_auto() {
        let json = r#"{"tool_choice": "auto"}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        println!("Parsed tool_choice: {:?}", req.tool_choice);
        assert!(!req.tool_choice.is_null(), "tool_choice should not be null");
    }
    
    #[test]
    fn test_tool_choice_required() {
        let json = r#"{"tool_choice": "required"}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        println!("Parsed tool_choice: {:?}", req.tool_choice);
        assert!(!req.tool_choice.is_null(), "tool_choice should not be null");
    }
    
    #[test]
    fn test_tool_choice_function() {
        let json = r#"{"tool_choice": {"type": "function", "function": {"name": "my_function"}}}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        println!("Parsed tool_choice: {:?}", req.tool_choice);
        assert!(!req.tool_choice.is_null(), "tool_choice should not be null");
        assert!(req.tool_choice.is_object(), "tool_choice should be object");
    }
    
    #[test]
    fn test_tool_choice_default() {
        let json = r#"{}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        // When not specified, should be null (default for Value)
        assert!(req.tool_choice.is_null(), "tool_choice should be null when not specified");
    }
    
    #[test]
    fn test_tools_parsing() {
        let json = r#"{
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "search_repo",
                        "description": "Search in repository",
                        "parameters": {"type": "object", "properties": {}}
                    }
                }
            ]
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(req.tools.is_some());
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.as_ref().unwrap().name, "search_repo");
    }
}
