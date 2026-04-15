//! OpenAI-Compatible API endpoints for VS Code integration
//!
//! This module provides `/v1/chat/completions` endpoint that is compatible
//! with the OpenAI API format, enabling VS Code extensions to connect.
//! Supports both streaming (SSE) and non-streaming modes.

use std::convert::Infallible;

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response},
    response::{
        IntoResponse, Json,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures::stream::{self, StreamExt};
use garraia_agents::{ChatMessage, ChatRole, MessagePart};
use serde::{Deserialize, Serialize};
use std::time::Instant;
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
    let started_at = Instant::now();
    // Extract request ID for tracing (GAR-234)
    let request_id = resolve_request_id(&headers);

    // Extract session ID from headers or create new one
    let session_id = resolve_session_id(&headers)
        .await
        .unwrap_or_else(|_| Uuid::new_v4().to_string());
    let is_streaming = body.stream.unwrap_or(false);

    // Resolve user identity from Authorization header
    let user_id = resolve_user_id(&headers, &state);

    // GAR-234/238: Extract mode from header for logging and apply to session
    let agent_mode = headers
        .get("x-agent-mode")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // GAR-234: Also check for mode prefix in user message (fallback: "mode: debug")
    let message_mode = body.messages.last().and_then(|m| {
        let content = m.content.to_lowercase();
        // Check for "mode: <mode>" or "/mode <mode>" patterns
        if content.starts_with("mode: ") {
            Some(
                content
                    .strip_prefix("mode: ")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            )
        } else if content.starts_with("/mode ") {
            Some(
                content
                    .strip_prefix("/mode ")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            )
        } else {
            None
        }
    });

    // Use header mode first, then fall back to message mode
    let final_mode = agent_mode.or(message_mode);

    // Apply X-Agent-Mode header to session if provided (GAR-234)
    if let Some(ref mode) = final_mode
        && let Some(ref session_store) = state.session_store
    {
        let store = session_store.lock().await;
        let _ = store.set_agent_mode(&session_id, mode);
    }

    // GAR-227: Auto-classify mode when no explicit mode was given.
    if final_mode.is_none() {
        let cfg = state.current_config();
        let user_text_for_router = body
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let runtime_ref = state
            .agents
            .default_provider()
            .is_some()
            .then_some(&state.agents);
        let auto_mode = crate::auto_router::auto_classify(
            &user_text_for_router,
            cfg.agent.auto_router_llm_enabled,
            cfg.agent.auto_router_model.as_deref(),
            runtime_ref,
        )
        .await;
        if let Some(mode) = auto_mode {
            if let Some(ref session_store) = state.session_store {
                let store = session_store.lock().await;
                let _ = store.set_agent_mode(&session_id, mode.as_str());
            }
            tracing::debug!(mode = %mode, session = %session_id, "auto_router: mode assigned");
        }
    }

    // GAR-225: Extract tool_choice for standardized logging
    let _tool_choice_str = if body.tool_choice.is_null() {
        None
    } else if let Some(s) = body.tool_choice.as_str() {
        Some(s.to_string())
    } else if let Some(obj) = body.tool_choice.as_object() {
        if let Some(name) = obj
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
        {
            Some(format!("function:{}", name))
        } else {
            Some(body.tool_choice.to_string())
        }
    } else {
        Some(body.tool_choice.to_string())
    };

    // GAR-214: Detect request source from User-Agent or X-Source header
    let source = headers
        .get("x-source")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(|ua| {
                    if ua.contains("vscode") || ua.contains("continue") || ua.contains("VSCode") {
                        "vscode".to_string()
                    } else if ua.contains("Telegram") {
                        "telegram".to_string()
                    } else {
                        "http".to_string()
                    }
                })
        })
        .unwrap_or_else(|| "http".to_string());

    // GAR-214: Structured log fields for end-to-end tracing
    info!(
        request_id = %request_id,
        session_id = %session_id,
        source = %source,
        model = %body.model.as_deref().unwrap_or("default"),
        streaming = is_streaming,
        mode = ?final_mode,
        "chat.request.started"
    );

    // Get model name
    let model = body.model.clone().unwrap_or_else(|| "gpt-4".to_string());

    // GAR-204: Hydrate session history from DB so the server is the source of truth.
    // This is a no-op for brand-new sessions (no-op if no session_store).
    state
        .hydrate_session_history(&session_id, Some("vscode"), user_id.as_deref())
        .await;

    // Extract the new user message from the client request (the last message is the current input)
    let new_user_text = body
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // Build conversation history: prefer DB history (source of truth) over client-sent messages.
    // If the DB has no history yet (first message), fall back to client-provided messages[].
    let db_history = state.session_history(&session_id);
    let mut messages: Vec<ChatMessage> = if db_history.is_empty() {
        // No DB history: use client's messages[] (minus the last user message) as seed context
        body.messages[..body.messages.len().saturating_sub(1)]
            .iter()
            .map(|m| ChatMessage {
                role: parse_role(&m.role),
                content: MessagePart::Text(m.content.clone()),
            })
            .collect()
    } else {
        db_history
    };
    // Append the new user message at the end so handlers can extract it as `messages.last()`
    messages.push(ChatMessage {
        role: ChatRole::User,
        content: MessagePart::Text(new_user_text.clone()),
    });

    // GAR-184: Resolve slash commands (MCP prompts + /help).
    // /mode is excluded here — it is already handled by the `final_mode` logic above.
    if new_user_text.starts_with('/')
        && let Some(resolved) =
            crate::slash_commands::resolve(&new_user_text, state.mcp_manager_arc.as_ref()).await
    {
        match resolved {
            crate::slash_commands::ResolvedCommand::McpPrompt(prompt_msgs) => {
                // Replace the slash command message with the MCP prompt context.
                messages.pop();
                messages.extend(prompt_msgs);
            }
        }
    }

    // Get continuity key based on user_id
    let continuity_key = state
        .continuity_key(user_id.as_deref())
        .unwrap_or_else(|| "default".to_string());

    let response = if is_streaming {
        // Handle streaming mode (latency = setup time; LLM latency logged separately)
        handle_streaming(
            state,
            session_id.clone(),
            model,
            messages,
            continuity_key,
            user_id,
        )
        .await
    } else {
        handle_non_streaming(
            state,
            session_id.clone(),
            model,
            messages,
            continuity_key,
            user_id,
        )
        .await
    };

    // GAR-214: Log request completion with latency
    info!(
        request_id = %request_id,
        session_id = %session_id,
        source = %source,
        streaming = is_streaming,
        latency_ms = started_at.elapsed().as_millis(),
        "chat.request.completed"
    );

    response
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

    // Conversation history = all messages except the last user message
    let conversation_history: Vec<ChatMessage> =
        messages[..messages.len().saturating_sub(1)].to_vec();

    // Clone what we need for the spawned task
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();
    let continuity_key_clone = continuity_key.clone();
    let user_msg_clone = user_message.clone();
    let model_clone = model.clone();
    let user_id_clone = user_id.clone();

    // Spawn task to process streaming and persist the turn when done (GAR-204)
    tokio::spawn(async move {
        if let Ok(response_text) = state_clone
            .agents
            .process_message_streaming_with_agent_config(
                &session_id_clone,
                &user_msg_clone,
                &conversation_history,
                delta_tx,
                Some(continuity_key_clone.as_str()),
                user_id_clone.as_deref(),
                None,
                Some(model_clone.as_str()),
                None,
                None,
            )
            .await
        {
            // GAR-204: Persist the turn to DB after streaming completes
            state_clone
                .persist_turn(
                    &session_id_clone,
                    Some("vscode"),
                    user_id_clone.as_deref(),
                    &user_msg_clone,
                    &response_text,
                )
                .await;
            // GAR-208: background summarization (fire-and-forget)
            let summ_state = state_clone.clone();
            let summ_session = session_id_clone.clone();
            tokio::spawn(async move {
                crate::context_summarizer::maybe_trigger_summarization(summ_state, summ_session)
                    .await;
            });
        }
    });

    // Prepare SSE streaming response
    let chunk_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // GAR-205: Full OpenAI-compatible SSE stream.
    // Phase 0 → initial role chunk
    // Phase 1 → content delta chunks from the mpsc receiver
    // Phase 2 → final finish_reason="stop" chunk
    // Phase 3 → data: [DONE] sentinel
    // Phase 4 → stream ends
    let stream = stream::unfold(
        (0u8, Some(delta_rx), chunk_id, created, model),
        |(phase, mut rx_opt, chunk_id, created, model)| async move {
            match phase {
                // Phase 0: emit initial chunk establishing role
                0 => {
                    let chunk = ChatCompletionChunk {
                        id: chunk_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: DeltaContent {
                                role: Some("assistant".to_string()),
                                content: Some(String::new()),
                            },
                            finish_reason: None,
                        }],
                    };
                    let event =
                        Event::default().data(serde_json::to_string(&chunk).unwrap_or_default());
                    Some((event, (1, rx_opt, chunk_id, created, model)))
                }
                // Phase 1: stream content deltas; on channel close → go to phase 2
                1 => {
                    let mut rx = rx_opt.take().unwrap();
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
                            Some((event, (1, Some(rx), chunk_id, created, model)))
                        }
                        None => {
                            // Channel closed — emit finish chunk
                            let chunk = ChatCompletionChunk {
                                id: chunk_id.clone(),
                                object: "chat.completion.chunk".to_string(),
                                created,
                                model: model.clone(),
                                choices: vec![ChunkChoice {
                                    index: 0,
                                    delta: DeltaContent {
                                        role: None,
                                        content: None,
                                    },
                                    finish_reason: Some("stop".to_string()),
                                }],
                            };
                            let event = Event::default()
                                .data(serde_json::to_string(&chunk).unwrap_or_default());
                            Some((event, (3, None, chunk_id, created, model)))
                        }
                    }
                }
                // Phase 3: emit [DONE] sentinel
                3 => {
                    let event = Event::default().data("[DONE]");
                    Some((event, (4, rx_opt, chunk_id, created, model)))
                }
                // Phase 4+: stream ended
                _ => None,
            }
        },
    );

    // Convert to Result stream for Sse
    let result_stream = stream.map(Ok::<_, Infallible>);

    Sse::new(result_stream).into_response()
}

/// Handle non-streaming request
async fn handle_non_streaming(
    state: SharedState,
    session_id: String,
    model: String,
    messages: Vec<ChatMessage>,
    continuity_key: String,
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

    // Call the agent
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

            // GAR-204: Persist the turn to DB via state (handles both in-memory and persistent storage)
            state
                .persist_turn(
                    &session_id,
                    Some("vscode"),
                    user_id.as_deref(),
                    &user_text,
                    &response,
                )
                .await;
            // GAR-208: background summarization (fire-and-forget)
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

            Json(resp).into_response()
        }
        Err(e) => {
            error!("Agent error: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Agent error: {}", e),
            )
                .into_response()
        }
    }
}

/// Resolve session ID from headers or create new one
async fn resolve_session_id(
    headers: &HeaderMap,
) -> Result<String, (axum::http::StatusCode, String)> {
    // Try X-Session-Id header first
    if let Some(session_id) = headers.get("x-session-id")
        && let Ok(s) = session_id.to_str()
    {
        return Ok(s.to_string());
    }

    // Create new session
    Ok(Uuid::new_v4().to_string())
}

/// GAR-234: Resolve request ID from headers for tracing
/// Returns the X-Request-Id if provided, otherwise generates a new one
fn resolve_request_id(headers: &HeaderMap) -> String {
    if let Some(request_id) = headers.get("x-request-id")
        && let Ok(s) = request_id.to_str()
    {
        return s.to_string();
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
    if let Some(auth_header) = headers.get("authorization")
        && let Ok(auth_str) = auth_header.to_str()
    {
        // Handle "Bearer <token>" format
        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            let token = token.trim();

            // Check if it's a valid API key from the config
            // The token "garra-local" maps to the local owner
            if token == "garra-local" {
                // Get the owner from allowlist
                if let Ok(list) = state.allowlist.lock()
                    && let Some(owner) = list.owner()
                {
                    info!("Resolved user_id={} from 'garra-local' token", owner);
                    return Some(owner.to_string());
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

    // Try X-User-Id header
    if let Some(user_id_header) = headers.get("x-user-id")
        && let Ok(user_id) = user_id_header.to_str()
    {
        return Some(user_id.to_string());
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
    }))
    .into_response()
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
        assert!(
            req.tool_choice.is_null(),
            "tool_choice should be null when not specified"
        );
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
