use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use garraia_agents::{AgentMode, ContentBlock, MessagePart, ModeEngine};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::agent_router;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    /// Optional named agent to use for this session.
    pub agent_id: Option<String>,
}

#[derive(Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    /// Optional named agent override for this message.
    pub agent_id: Option<String>,
    /// Optional model override for this message.
    pub model: Option<String>,
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub session_id: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub channel_id: Option<String>,
    pub connected: bool,
    pub history_length: usize,
}

/// POST /api/sessions — create a new session.
pub async fn create_session(
    State(state): State<SharedState>,
    Json(body): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let session_id = state.create_session();

    // If an agent_id was requested, tag it on the session metadata
    if let Some(ref agent_id) = body.agent_id
        && let Some(mut session) = state.sessions.get_mut(&session_id)
    {
        session.channel_id = Some(format!("api:{agent_id}"));
    }

    (
        StatusCode::CREATED,
        Json(CreateSessionResponse {
            session_id,
            agent_id: body.agent_id,
        }),
    )
}

/// POST /api/sessions/:id/messages — send a message to a session.
pub async fn send_message(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    if !state.sessions.contains_key(&session_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "session not found" })),
        )
            .into_response();
    }

    // Hydrate history
    state
        .hydrate_session_history(&session_id, Some("api"), None)
        .await;
    let history = state.session_history(&session_id);
    let continuity_key = state.continuity_key(None);

    // Resolve named agent config
    let config = state.current_config();
    let agent_config = agent_router::resolve(&config, body.agent_id.as_deref(), None);

    let result = if let Some(ac) = agent_config {
        state
            .agents
            .process_message_with_agent_config(
                &session_id,
                &body.content,
                &history,
                continuity_key.as_deref(),
                None,
                ac.provider.as_deref(),
                body.model.as_deref(),
                ac.system_prompt.as_deref(),
                ac.max_tokens,
            )
            .await
    } else if body.model.is_some() {
        state
            .agents
            .process_message_with_agent_config(
                &session_id,
                &body.content,
                &history,
                continuity_key.as_deref(),
                None,
                None,
                body.model.as_deref(),
                None,
                None,
            )
            .await
    } else {
        state
            .agents
            .process_message_with_context(
                &session_id,
                &body.content,
                &history,
                continuity_key.as_deref(),
                None,
            )
            .await
    };

    match result {
        Ok(response_text) => {
            state
                .persist_turn(
                    &session_id,
                    Some("api"),
                    None,
                    &body.content,
                    &response_text,
                )
                .await;
            (
                StatusCode::OK,
                Json(serde_json::json!(SendMessageResponse {
                    session_id,
                    content: response_text,
                })),
            )
                .into_response()
        }
        Err(e) => {
            warn!("agent error in API session {session_id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

/// GET /api/sessions/:id/history — get session history.
pub async fn session_history(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if !state.sessions.contains_key(&session_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "session not found" })),
        )
            .into_response();
    }

    state
        .hydrate_session_history(&session_id, Some("api"), None)
        .await;
    let history = state.session_history(&session_id);
    let messages: Vec<serde_json::Value> = history
        .iter()
        .map(|m| {
            let text = match &m.content {
                MessagePart::Text(s) => s.clone(),
                MessagePart::Parts(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            serde_json::json!({
                "role": format!("{:?}", m.role).to_lowercase(),
                "content": text,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "messages": messages })),
    )
        .into_response()
}

/// GET /api/sessions — list active sessions.
pub async fn list_sessions(State(state): State<SharedState>) -> impl IntoResponse {
    let sessions: Vec<SessionInfo> = state
        .sessions
        .iter()
        .map(|entry| SessionInfo {
            session_id: entry.id.clone(),
            channel_id: entry.channel_id.clone(),
            connected: entry.connected,
            history_length: entry.history.len(),
        })
        .collect();

    Json(serde_json::json!({ "sessions": sessions }))
}

// =============================================================================
// GAR-230: Mode API Endpoints
// =============================================================================

/// Response for GET /api/modes - list all available modes
#[derive(Serialize)]
struct ModeInfo {
    id: String,
    name: String,
    description: String,
    tool_policy: garraia_agents::ToolPolicy,
}

/// GET /api/modes — list all available agent modes with their profiles.
pub async fn list_modes() -> impl IntoResponse {
    let engine = ModeEngine::new();
    let profiles = engine.list_profiles();

    let modes: Vec<ModeInfo> = profiles
        .iter()
        .map(|p| ModeInfo {
            id: p.name.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            tool_policy: p.tool_policy.clone(),
        })
        .collect();

    Json(serde_json::json!({ "modes": modes }))
}

/// Request for POST /api/mode/select
#[derive(Deserialize)]
pub struct SelectModeRequest {
    /// Mode name to select (e.g., "code", "ask", "debug")
    pub mode: String,
}

/// Response for mode selection
#[derive(Serialize)]
struct SelectModeResponse {
    success: bool,
    mode: String,
    message: String,
}

/// POST /api/mode/select — select mode for a session.
/// Header: X-Session-Id (optional) - if not provided, uses a default session ID.
pub async fn select_mode(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<SelectModeRequest>,
) -> impl IntoResponse {
    // Validate mode
    let mode_str = body.mode.to_lowercase();
    if AgentMode::from_str(&mode_str).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(SelectModeResponse {
                success: false,
                mode: mode_str.clone(),
                message: format!(
                    "Invalid mode '{}'. Use GET /api/modes to see available modes.",
                    mode_str
                ),
            })),
        );
    }

    // Get session ID from X-Session-Id header or use default
    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "api:default".to_string());

    // Ensure session exists in store
    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        // Upsert session if needed (using default values)
        let _ = store.upsert_session(&session_id, "api", "anonymous", &serde_json::json!({}));
        // Set the mode
        match store.set_agent_mode(&session_id, &mode_str) {
            Ok(_) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!(SelectModeResponse {
                        success: true,
                        mode: mode_str.clone(),
                        message: format!("Mode set to '{}'", mode_str),
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(SelectModeResponse {
                        success: false,
                        mode: mode_str.clone(),
                        message: format!("Failed to set mode: {}", e),
                    })),
                );
            }
        }
    }

    // If no session store, just return success (mode will be in-memory only)
    (
        StatusCode::OK,
        Json(serde_json::json!(SelectModeResponse {
            success: true,
            mode: mode_str,
            message: "Mode set (in-memory only, no session store)".to_string(),
        })),
    )
}

/// Response for GET /api/mode/current
#[derive(Serialize)]
struct CurrentModeResponse {
    mode: Option<String>,
    session_id: String,
}

/// GET /api/mode/current — get current mode for a session.
/// Header: X-Session-Id (optional) - if not provided, uses a default session ID.
pub async fn current_mode(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Get session ID from X-Session-Id header or use default
    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "api:default".to_string());

    // Try to get mode from session store
    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.get_agent_mode(&session_id) {
            Ok(mode) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!(CurrentModeResponse {
                        mode,
                        session_id,
                    })),
                );
            }
            Err(e) => {
                warn!("Failed to get agent mode: {}", e);
            }
        }
    }

    // Default mode if not set
    (
        StatusCode::OK,
        Json(serde_json::json!(CurrentModeResponse {
            mode: None,
            session_id,
        })),
    )
}

// =============================================================================
// GAR-232: Custom Mode API Endpoints
// =============================================================================

use garraia_db::session_store::CustomMode;

/// Request for POST /api/modes/custom
#[derive(Deserialize)]
pub struct CreateCustomModeRequest {
    /// Name of the custom mode
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Base mode to clone from (e.g., "code", "ask", "debug")
    pub base_mode: String,
    /// Override for tool policy
    #[serde(default)]
    pub tool_policy_overrides: serde_json::Value,
    /// Override for system prompt
    #[serde(default)]
    pub prompt_override: Option<String>,
    /// Default LLM settings
    #[serde(default)]
    pub defaults: serde_json::Value,
}

/// Request for PATCH /api/modes/custom/:id
#[derive(Deserialize)]
pub struct UpdateCustomModeRequest {
    /// Name of the custom mode
    #[serde(default)]
    pub name: Option<String>,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// Override for tool policy
    #[serde(default)]
    pub tool_policy_overrides: Option<serde_json::Value>,
    /// Override for system prompt
    #[serde(default)]
    pub prompt_override: Option<String>,
    /// Default LLM settings
    #[serde(default)]
    pub defaults: Option<serde_json::Value>,
}

/// Response for custom mode operations
#[derive(Serialize)]
struct CustomModeResponse {
    success: bool,
    mode: Option<CustomMode>,
    message: String,
}

/// POST /api/modes/custom — create a new custom mode
pub async fn create_custom_mode(
    State(state): State<SharedState>,
    Json(body): Json<CreateCustomModeRequest>,
) -> impl IntoResponse {
    // Validate base_mode exists
    if AgentMode::from_str(&body.base_mode).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(CustomModeResponse {
                success: false,
                mode: None,
                message: format!(
                    "Invalid base_mode '{}'. Use GET /api/modes to see available modes.",
                    body.base_mode
                ),
            })),
        );
    }

    // Use a default user_id for API sessions (in a real app, this would come from auth)
    let user_id = "api:default";

    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.create_custom_mode(
            user_id,
            &body.name,
            body.description.as_deref(),
            &body.base_mode,
            &body.tool_policy_overrides,
            body.prompt_override.as_deref(),
            &body.defaults,
        ) {
            Ok(mode) => {
                return (
                    StatusCode::CREATED,
                    Json(serde_json::json!(CustomModeResponse {
                        success: true,
                        mode: Some(mode),
                        message: "Custom mode created successfully".to_string(),
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: format!("Failed to create custom mode: {}", e),
                    })),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!(CustomModeResponse {
            success: false,
            mode: None,
            message: "Session store not available".to_string(),
        })),
    )
}

/// GET /api/modes/custom — list all custom modes for the user
pub async fn list_custom_modes(State(state): State<SharedState>) -> impl IntoResponse {
    let user_id = "api:default";

    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.get_custom_modes(user_id) {
            Ok(modes) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "success": true,
                        "modes": modes
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: format!("Failed to list custom modes: {}", e),
                    })),
                );
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "modes": Vec::<CustomMode>::new()
        })),
    )
}

/// GET /api/modes/custom/:id — get a specific custom mode
pub async fn get_custom_mode(
    State(state): State<SharedState>,
    Path(mode_id): Path<String>,
) -> impl IntoResponse {
    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.get_custom_mode(&mode_id) {
            Ok(Some(mode)) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!(CustomModeResponse {
                        success: true,
                        mode: Some(mode),
                        message: "Custom mode found".to_string(),
                    })),
                );
            }
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: "Custom mode not found".to_string(),
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: format!("Failed to get custom mode: {}", e),
                    })),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!(CustomModeResponse {
            success: false,
            mode: None,
            message: "Session store not available".to_string(),
        })),
    )
}

/// PATCH /api/modes/custom/:id — update a custom mode
pub async fn update_custom_mode(
    State(state): State<SharedState>,
    Path(mode_id): Path<String>,
    Json(body): Json<UpdateCustomModeRequest>,
) -> impl IntoResponse {
    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.update_custom_mode(
            &mode_id,
            body.name.as_deref(),
            body.description.as_deref(),
            body.tool_policy_overrides.as_ref(),
            body.prompt_override.as_deref(),
            body.defaults.as_ref(),
        ) {
            Ok(Some(mode)) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!(CustomModeResponse {
                        success: true,
                        mode: Some(mode),
                        message: "Custom mode updated successfully".to_string(),
                    })),
                );
            }
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: "Custom mode not found".to_string(),
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: format!("Failed to update custom mode: {}", e),
                    })),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!(CustomModeResponse {
            success: false,
            mode: None,
            message: "Session store not available".to_string(),
        })),
    )
}

/// DELETE /api/modes/custom/:id — delete a custom mode
pub async fn delete_custom_mode(
    State(state): State<SharedState>,
    Path(mode_id): Path<String>,
) -> impl IntoResponse {
    if let Some(store) = &state.session_store {
        let store = store.lock().await;
        match store.delete_custom_mode(&mode_id) {
            Ok(true) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!(CustomModeResponse {
                        success: true,
                        mode: None,
                        message: "Custom mode deleted successfully".to_string(),
                    })),
                );
            }
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: "Custom mode not found".to_string(),
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(CustomModeResponse {
                        success: false,
                        mode: None,
                        message: format!("Failed to delete custom mode: {}", e),
                    })),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!(CustomModeResponse {
            success: false,
            mode: None,
            message: "Session store not available".to_string(),
        })),
    )
}
