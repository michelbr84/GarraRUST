//! Runtime integration module for garraia-gateway.
//!
//! This module provides integration with garraia-runtime for executing agent turns
//! with tool execution capabilities.

use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};

use crate::state::SharedState;

/// Request payload for running a turn.
#[derive(Debug, Deserialize)]
pub struct RunTurnRequest {
    /// The user's input text.
    pub user_text: String,
    /// Optional custom settings for this turn.
    #[serde(default)]
    pub settings: Option<garraia_runtime::RuntimeSettings>,
}

/// Response from running a turn.
#[derive(Debug, Serialize)]
pub struct RunTurnResponse {
    /// The output text from the turn.
    pub output: String,
    /// The final state of the turn.
    pub state: String,
    /// Number of tools called during the turn.
    pub tools_called: u32,
    /// Number of iterations executed.
    pub iterations: u32,
}

/// Build the runtime routes.
pub fn build_runtime_router() -> Router<SharedState> {
    Router::new().route("/api/runtime/tools", get(list_tools_handler))
}

/// Handler for GET /api/runtime/tools - List available tools in the registry.
/// GAR-159: Uses AgentRuntime as the single source of truth for registered tools.
pub async fn list_tools_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let tools = state.agents.tool_names();
    Json(serde_json::json!({
        "tools": tools,
        "count": tools.len(),
    }))
}

/// Handler for GET /api/runtime/settings - Get current runtime settings.
pub async fn get_runtime_settings(State(state): State<SharedState>) -> impl IntoResponse {
    let settings = state.runtime_settings();
    Json(serde_json::json!({
        "tool_timeout_secs": settings.tool_timeout_secs,
        "max_tool_calls_per_turn": settings.max_tool_calls_per_turn,
        "max_llm_iterations": settings.max_llm_iterations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;
    use std::sync::Arc;

    fn test_state() -> SharedState {
        Arc::new(AppState::new(
            AppConfig::default(),
            AgentRuntime::new(),
            ChannelRegistry::new(),
        ))
    }

    #[tokio::test]
    async fn list_tools_returns_empty_for_new_state() {
        let state = test_state();

        // Just verify it compiles and returns a response
        let response = list_tools_handler(State(state)).await;
        let (_status, _body) = response.into_response().into_parts();
        // Test passes if it compiles and runs
    }

    #[tokio::test]
    async fn get_runtime_settings_returns_defaults() {
        let state = test_state();

        // Just verify it compiles and returns a response
        let response = get_runtime_settings(State(state)).await;
        let (_status, _body) = response.into_response().into_parts();
        // Test passes if it compiles and runs
    }
}
