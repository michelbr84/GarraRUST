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
use tracing::{debug, error, info, warn};
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
    let pedido_de_modo = agent_mode.or(message_mode);

    // Nome desconhecido nao vira escolha (#988).
    //
    // O `/mode` e o `PUT /api/mode` ja recusam nome invalido com mensagem; este
    // caminho — header `X-Agent-Mode` e prefixo `mode:` — nao recusava, e
    // gravava a string crua na sessao. Depois que a `ToolPolicy` passou a valer
    // no executor, isso virou uma escolha registrada que nao resolve para
    // perfil nenhum: o portao abre, o `/mode` exibe um modo que nao existe e
    // ninguem fica sabendo. Validar aqui e o unico ponto em que ainda da para
    // avisar.
    //
    // Nao derruba o request: pedir modo errado num header nao deve custar a
    // resposta. Vira "nao pediu modo" — inclusive para o auto-router logo
    // abaixo, que volta a poder opinar.
    let final_mode = match pedido_de_modo {
        Some(nome) => match garraia_agents::modes::AgentMode::from_str(&nome) {
            Some(modo) => Some(modo.as_str().to_string()),
            None => {
                warn!(
                    modo_pedido = %rotulo_de_modo_para_log(&nome),
                    session_id = %session_id,
                    "modo desconhecido ignorado; use GET /api/modes para os validos"
                );
                None
            }
        },
        None => None,
    };

    // As duas gravacoes do modo ficam depois de `hydrate_session_history` —
    // ver o bloco marcado logo abaixo dela.

    // GAR-227: Auto-classify mode when no explicit mode was given.
    let modo_deduzido = if final_mode.is_none() {
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
            .then_some(&*state.agents);
        garraia_agents::auto_router::auto_classify(
            &user_text_for_router,
            cfg.agent.auto_router_llm_enabled,
            cfg.agent.auto_router_model.as_deref(),
            runtime_ref,
        )
        .await
    } else {
        None
    };

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

    // O modo so pode ser gravado agora.
    //
    // `set_agent_mode` e um `UPDATE ... WHERE id = ?`, e a linha da sessao e
    // criada ali em cima, por `hydrate_session_history`. Gravar antes casava
    // zero linhas — e como o setter devolvia `Ok(())` e o chamador escrevia
    // `let _ =`, o `X-Agent-Mode` sumia sem deixar rastro. Achado rodando o
    // binario: o header dizia `search`, o banco ficava vazio.
    if let Some(ref session_store) = state.session_store {
        let store = session_store.lock().await;
        // GAR-234: modo pedido pelo usuario (header `X-Agent-Mode` ou prefixo
        // `mode:`), ja validado acima.
        if let Some(ref mode) = final_mode
            && let Err(e) = store.set_agent_mode(&session_id, mode)
        {
            warn!(session_id = %session_id, erro = %e, "falhou ao gravar o modo escolhido");
        }
        // GAR-227: modo deduzido. `_auto`, nao `set_agent_mode`: aparece no
        // `/mode` e no `GET /api/mode/current`, mas nao liga a politica de
        // ferramenta do #988 — deduzir nao e consentir. Se gravasse como
        // escolha, quem nunca digitou `/mode` perderia `file_write` porque a
        // heuristica achou que a pergunta parecia busca.
        if let Some(modo) = modo_deduzido {
            match store.set_agent_mode_auto(&session_id, modo.as_str()) {
                Ok(()) => {
                    tracing::debug!(mode = %modo, session = %session_id, "auto_router: mode assigned")
                }
                Err(e) => {
                    warn!(session_id = %session_id, erro = %e, "falhou ao gravar o modo deduzido")
                }
            }
        }
    }

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
    //
    // Only `/help`, `/mode` and MCP prompts are handled on this path. The
    // registry commands (`/clear`, `/model`, `/goal`, …) are dispatched by
    // `POST /api/sessions/{id}/messages` (`api::dispatch_slash_command`),
    // not by the OpenAI-compatible endpoint: a client that speaks this
    // protocol expects the model to answer, and a `/clear` here reaches the
    // model as text. Said here so nobody debugs it as a bug (#1040 review).
    if new_user_text.starts_with('/')
        && let Some(resolved) = crate::slash_commands::resolve(
            &new_user_text,
            crate::api::registry_commands_for_http(&state),
            state.mcp_manager_arc.as_ref(),
        )
        .await
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
        .continuity_key()
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
                &state
                    .exec_context_for(&session_id, user_id.as_deref())
                    .await,
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
            &state
                .exec_context_for(&session_id, user_id.as_deref())
                .await,
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

/// Fingerprint não-reversível de um bearer token, para correlação em log.
///
/// Neste path o token **é** o `user_id`, então logá-lo verbatim escrevia a
/// credencial em todo sink de log — incluindo o `file_appender`, que o
/// `RedactingWriter` não embrulha (ele só cobre o stderr, ver
/// `garraia-cli/src/main.rs`). O regex de redaction também não ajudaria: ele
/// conhece prefixos de vendor (`sk-`, `xoxb-`, …), não tokens próprios do
/// GarraIA.
///
/// 12 hex chars de SHA-256 bastam para correlacionar duas requisições do mesmo
/// chamador e são pouco demais para replay.
/// Reduz um nome de modo invalido ao que da para logar sem risco.
///
/// O valor vem cru do header `X-Agent-Mode` ou do prefixo `mode:` da mensagem,
/// entao e texto arbitrario de quem chama. Nome de modo real e uma palavra
/// curta em ASCII; qualquer outra coisa ali e engano — e o engano que preocupa
/// e alguem colar um segredo no lugar do nome. Manter o valor legivel importa
/// (o log existe para a pessoa ver que digitou `agente` em vez de `ask`), entao
/// nao dando para redigir tudo, limitamos o estrago: 24 caracteres, so o que
/// um nome de modo poderia conter, e uma marca quando houve corte.
///
/// Nao substitui a regra: segredo nao vai para log. Isto e o piso para quando
/// alguem colar um por engano.
fn rotulo_de_modo_para_log(nome: &str) -> String {
    const MAX: usize = 24;
    let limpo: String = nome
        .chars()
        .take(MAX)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '.'
            }
        })
        .collect();
    if nome.chars().count() > MAX {
        format!("{limpo}...(truncado)")
    } else {
        limpo
    }
}

fn token_fingerprint(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(&digest[..6])
}

/// Registra que um bearer chegou, **sem** deixá-lo virar identidade (#1012).
///
/// Separado de [`resolve_user_id`] porque não depende de `SharedState` — é o
/// que torna o path testável sem subir o gateway inteiro, e é onde vive o
/// invariante "o token não sai no log" (ver `token_fingerprint`).
///
/// Antes da #1012 esta função devolvia o próprio token como `user_id`, com o
/// comentário *"For other tokens, use the token itself as user_id — this
/// allows custom API keys to identify users"*. Ela nunca identificou ninguém:
/// a rota não verifica o token contra nada, então "o token identifica o
/// usuário" equivalia a "o chamador escolhe o próprio nome". Hoje ela só
/// deixa rastro para quem estiver se perguntando por que a chave dele não faz
/// nada.
fn note_ignored_bearer(token: &str) {
    if token.is_empty() {
        return;
    }
    // NUNCA logar `token`: ele é credencial de alguém, mesmo que este
    // caminho não a verifique. Só o fingerprint sai no log.
    info!(
        token_fp = %token_fingerprint(token),
        "bearer presented to /v1/chat/completions was ignored: this route is \
         auth-free by design and does not derive identity from the caller"
    );
}

/// Resolve a identidade a ser **gravada** para uma chamada de
/// `/v1/chat/completions`.
///
/// Esta rota é auth-free por desenho, como todo o `/api/*`
/// (`docs/security/threat-model.md` §5.7 e §5.9). Auth-free significa "não
/// exige credencial" — **não** "aceita a identidade que o chamador afirmar".
///
/// Antes da #1012 ela aceitava as duas coisas que o chamador escreve: um
/// bearer qualquer virava o `user_id`, e na ausência dele o header
/// `X-User-Id` era usado cru. O impacto medido era atribuição falsa, e não
/// leitura cruzada — a carga de histórico é chaveada por `session_id`, não
/// por `user_id` —, mas é a mesma forma do buraco que a #1010 fechou de
/// propósito **antes** de ligar execução nele. Um header que não decide nada
/// não pode ser forjado.
///
/// A identidade é a do dono da instalação local, ou `None`. `None` é a
/// resposta honesta para uma instalação que ainda não sabe de quem é;
/// preenchê-la com o que veio no header seria inventar um dono.
fn resolve_user_id(headers: &HeaderMap, state: &SharedState) -> Option<String> {
    // O `garra-local` continua sendo a convenção do cliente local. Ele não
    // muda a resposta — o dono é o dono com ou sem ele —, mas distinguir os
    // dois casos no log é o que diz a quem depurar se o cliente está mandando
    // o que acha que manda.
    if let Some(auth_header) = headers.get("authorization")
        && let Ok(auth_str) = auth_header.to_str()
        && let Some(token) = auth_str.strip_prefix("Bearer ")
    {
        let token = token.trim();
        if token != "garra-local" {
            note_ignored_bearer(token);
        }
    }

    // O `X-User-Id` é deliberadamente **não lido**. Ver o doc-comment acima:
    // é o header que a #1012 nomeia, e a correção é que ele deixe de decidir.

    let owner = state
        .allowlist
        .lock()
        .ok()
        .and_then(|list| list.owner().map(str::to_string));

    // O **valor** do dono nunca sai no log, e essa e a diferenca em relacao ao
    // codigo anterior. Ele so logava o dono quando um `garra-local` era
    // apresentado; esta funcao resolve o dono em **toda** requisicao, entao
    // logar o valor aqui o repetiria a cada chamada. E o que vira dono nem
    // sempre e um id opaco: no WhatsApp e o proprio numero de telefone
    // (`bootstrap/whatsapp.rs:88`, `claim_owner(&from_number)`), no iMessage o
    // numero ou o Apple ID (`bootstrap/imessage.rs:59`). Regra absoluta 6.
    //
    // O que quem depura precisa saber e se a requisicao foi atribuida a alguem
    // ou a ninguem — um booleano. O valor esta no `allowlist.json`, que a
    // mesma pessoa pode abrir.
    match &owner {
        Some(_) => debug!("user_id resolvido pelo dono da allowlist local (valor omitido)"),
        None => info!("nenhum dono reivindicado ainda; requisicao gravada sem user_id"),
    }
    owner
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

    /// Guard de regressão do vazamento corrigido em 2026-08-29: até então
    /// `resolve_user_id` fazia `info!("Resolved user_id={} from API token",
    /// token)`, escrevendo o bearer literal em **toda** requisição
    /// autenticada. O `RedactingWriter` não salvava: ele embrulha só o stderr,
    /// não o `file_appender`, e o regex dele conhece prefixos de vendor, não
    /// tokens do GarraIA.
    #[tracing_test::traced_test]
    #[test]
    fn api_token_never_reaches_the_log() {
        const SECRET: &str = "garra-tok-9f3c1d7ab24e0058";

        // Desde a #1012 o token nao vira mais `user_id` — mas ele continua
        // chegando pela rede e continua sendo credencial de alguem, entao o
        // invariante do log e o mesmo e este guard segue valendo.
        note_ignored_bearer(SECRET);

        // Ele não pode aparecer no log.
        assert!(
            !logs_contain(SECRET),
            "bearer token vazou para o tracing — `resolve_token_user_id` deve \
             logar apenas `token_fingerprint(token)`"
        );

        // E o fingerprint precisa estar lá, senão o log perde a correlação
        // que justificava logar qualquer coisa.
        assert!(
            logs_contain(&token_fingerprint(SECRET)),
            "o fingerprint deveria estar no log para correlação"
        );
    }

    /// Um segredo colado no lugar do nome do modo nao sai inteiro no log.
    ///
    /// O caso real que preocupa: alguem exporta a chave para o header errado,
    /// ou cola `mode: sk-...` na mensagem. O nome invalido precisa aparecer no
    /// log para a pessoa entender o que aconteceu, mas nao o valor inteiro.
    #[test]
    fn rotulo_de_modo_nao_deixa_segredo_inteiro_no_log() {
        let segredo = "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        let saida = rotulo_de_modo_para_log(segredo);

        assert!(
            !saida.contains(segredo),
            "o valor inteiro vazou para o log: {saida}"
        );
        assert!(saida.contains("truncado"), "corte precisa ser visivel");
        assert!(
            saida.chars().count() <= 24 + "...(truncado)".len(),
            "limite estourou: {saida}"
        );
    }

    // ── #1012: identidade em `/v1/chat/completions` ──────────────────────
    //
    // A rota e auth-free por desenho, como todo o `/api/*`
    // (docs/security/threat-model.md §5.7 e §5.9). Auth-free significa "nao
    // exige credencial", e nao "aceita qualquer identidade que o chamador
    // afirme": um header forjavel que **decide** o `user_id` gravado e pior
    // que nenhum, porque parece identidade sem ser.

    /// Um `AppState` cuja allowlist **nao toca o disco**.
    ///
    /// `AppState::new` carrega a allowlist de
    /// `ConfigLoader::default_config_dir()/allowlist.json`, e `claim_owner`
    /// chama `save()`. A primeira versao deste helper usava a allowlist que
    /// vinha de la: o teste do dono gravou `dono-real` no arquivo real da
    /// maquina, e o teste seguinte — o que exige uma instalacao *sem* dono —
    /// leu esse dono de volta e falhou. Um teste que grava no estado real do
    /// operador esta errado mesmo quando passa.
    ///
    /// `Allowlist::restricted` nasce com `path: None`, e `save()` sai cedo
    /// quando nao ha path. Nada e escrito.
    fn state_de_teste_com_dono(dono: Option<&str>) -> SharedState {
        use crate::state::AppState;
        use garraia_config::AppConfig;
        use garraia_security::Allowlist;

        let state = AppState::new(
            AppConfig::default(),
            std::sync::Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        );

        let mut lista = Allowlist::restricted(Vec::new());
        if let Some(d) = dono {
            lista.claim_owner(d);
        }
        *state
            .allowlist
            .lock()
            .expect("allowlist envenenada no teste") = lista;

        std::sync::Arc::new(state)
    }

    fn headers(pares: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pares {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).expect("header invalido"),
                v.parse().expect("valor de header invalido"),
            );
        }
        h
    }

    /// O caso que a issue nomeia: `X-User-Id` cru, sem `Authorization`.
    ///
    /// Antes deste conserto o header virava o `user_id` sem nenhuma
    /// verificacao, e ia parar em `session.user_id` e na coluna `user` do
    /// upsert da sessao. O impacto era atribuicao falsa, nao leitura cruzada
    /// — mas e o mesmo formato do buraco que o #1010 fechou de proposito
    /// antes de ligar execucao nele.
    #[test]
    fn x_user_id_forjado_nao_vira_identidade() {
        let state = state_de_teste_com_dono(Some("dono-real"));
        let h = headers(&[("x-user-id", "vitima")]);

        let resolvido = resolve_user_id(&h, &state);

        assert_ne!(
            resolvido.as_deref(),
            Some("vitima"),
            "o `X-User-Id` do chamador nao pode decidir a identidade gravada"
        );
        assert_eq!(
            resolvido.as_deref(),
            Some("dono-real"),
            "sem credencial, a identidade e a do dono da instalacao local"
        );
    }

    /// Sem dono definido, o header forjado tambem nao pode preencher o vazio.
    ///
    /// `None` e a resposta honesta: a instalacao ainda nao sabe de quem e.
    #[test]
    fn x_user_id_forjado_nao_preenche_instalacao_sem_dono() {
        let state = state_de_teste_com_dono(None);
        let h = headers(&[("x-user-id", "vitima")]);

        assert_eq!(resolve_user_id(&h, &state), None);
    }

    /// Um bearer arbitrario tambem nao e identidade.
    ///
    /// O comportamento antigo (`"For other tokens, use the token itself as
    /// user_id"`) nunca distinguiu ninguem de verdade: qualquer um podia
    /// mandar qualquer token. Era um nome escolhido pelo chamador.
    #[test]
    fn bearer_arbitrario_nao_vira_identidade() {
        let state = state_de_teste_com_dono(Some("dono-real"));
        let h = headers(&[("authorization", "Bearer sou-quem-eu-quiser")]);

        let resolvido = resolve_user_id(&h, &state);

        assert_ne!(
            resolvido.as_deref(),
            Some("sou-quem-eu-quiser"),
            "o token do chamador nao pode virar o `user_id`"
        );
        assert_eq!(resolvido.as_deref(), Some("dono-real"));
    }

    /// O **valor** do dono nao pode aparecer no log.
    ///
    /// Achado ALTO da auditoria do #1012, e uma regressao que a *primeira*
    /// versao desta correcao introduziu: o codigo antigo so logava o dono
    /// quando um `garra-local` era apresentado; a correcao passou a resolver o
    /// dono em toda requisicao e logava o valor junto — mais vezes, portanto,
    /// que o codigo vulneravel que ela substituia.
    ///
    /// E o dono nem sempre e um id opaco: no WhatsApp e o proprio numero de
    /// telefone (`bootstrap/whatsapp.rs:88` chama `claim_owner(&from_number)`),
    /// no iMessage e o numero ou o Apple ID. Regra absoluta 6.
    #[tracing_test::traced_test]
    #[test]
    fn o_valor_do_dono_nao_vai_para_o_log() {
        const DONO: &str = "+15551234567";
        let state = state_de_teste_com_dono(Some(DONO));

        let resolvido = resolve_user_id(&headers(&[]), &state);

        // O dono continua sendo resolvido — a correcao e sobre o log, nao
        // sobre a resolucao.
        assert_eq!(resolvido.as_deref(), Some(DONO));

        assert!(
            !logs_contain(DONO),
            "o valor do dono vazou para o log — no WhatsApp isso e um numero \
             de telefone, repetido a cada requisicao"
        );
    }

    /// E o caminho que precisa continuar funcionando: `garra-local`.
    ///
    /// Criterio de aceitacao explicito da #1012 — "nenhuma regressao nos
    /// caminhos `garra-local` e allowlist".
    #[test]
    fn garra_local_continua_resolvendo_o_dono() {
        let state = state_de_teste_com_dono(Some("dono-real"));
        let h = headers(&[("authorization", "Bearer garra-local")]);

        assert_eq!(resolve_user_id(&h, &state).as_deref(), Some("dono-real"));
    }

    /// E um typo comum continua legivel, que e a razao de o campo existir.
    #[test]
    fn rotulo_de_modo_preserva_typo_curto() {
        assert_eq!(rotulo_de_modo_para_log("agente"), "agente");
        assert_eq!(rotulo_de_modo_para_log("code-2"), "code-2");
        // Controle e quebra de linha nao entram no log: `\n` num campo de
        // tracing quebraria a linha e permitiria forjar uma entrada.
        assert_eq!(rotulo_de_modo_para_log("a\nb\tc"), "a.b.c");
    }

    #[test]
    fn token_fingerprint_is_stable_short_and_not_the_token() {
        let fp = token_fingerprint("garra-tok-9f3c1d7ab24e0058");

        // Determinístico: duas requisições do mesmo chamador correlacionam.
        assert_eq!(fp, token_fingerprint("garra-tok-9f3c1d7ab24e0058"));
        // 6 bytes de SHA-256 em hex.
        assert_eq!(fp.len(), 12);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        // Tokens distintos não colidem trivialmente.
        assert_ne!(fp, token_fingerprint("garra-tok-9f3c1d7ab24e0059"));
    }

    /// `Authorization: Bearer ` sem nada depois nao gera linha de log.
    ///
    /// Antes da #1012 o guard aqui era "token vazio nao vira user_id". Agora
    /// nenhum token vira user_id, e o que sobra a proteger e o ruido: um
    /// cliente mal configurado mandando bearer vazio em toda requisicao
    /// encheria o log com o fingerprint da string vazia — o mesmo em todas.
    #[tracing_test::traced_test]
    #[test]
    fn bearer_vazio_nao_gera_linha_de_log() {
        note_ignored_bearer("");
        assert!(
            !logs_contain("was ignored"),
            "bearer vazio nao deveria produzir log"
        );
    }

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
