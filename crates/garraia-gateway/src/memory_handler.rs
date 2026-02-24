use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use garraia_db::RecallQuery;
use serde::Deserialize;

use crate::state::SharedState;

#[derive(Deserialize)]
pub struct SearchMemoryQuery {
    pub q: String,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct RecentMemoryQuery {
    pub limit: Option<usize>,
}

/// GET /api/memory/recent
pub async fn get_recent_memory(
    State(state): State<SharedState>,
    Query(params): Query<RecentMemoryQuery>,
) -> impl IntoResponse {
    let memory_provider = match state.agents.memory_provider() {
        Some(provider) => provider,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Memory is disabled" })),
            )
                .into_response();
        }
    };

    let limit = params.limit.unwrap_or(50);
    
    let query = RecallQuery {
        query_text: None,
        query_embedding: None,
        session_id: None,
        continuity_key: None,
        limit,
    };

    match memory_provider.recall(query).await {
        Ok(entries) => (
            StatusCode::OK,
            Json(serde_json::json!({ "memories": entries })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/memory/search
pub async fn search_memory(
    State(state): State<SharedState>,
    Query(params): Query<SearchMemoryQuery>,
) -> impl IntoResponse {
    let memory_provider = match state.agents.memory_provider() {
        Some(provider) => provider,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Memory is disabled" })),
            )
                .into_response();
        }
    };

    let limit = params.limit.unwrap_or(20);
    let query = RecallQuery {
        query_text: Some(params.q),
        query_embedding: None, 
        session_id: None,
        continuity_key: None,
        limit,
    };

    match memory_provider.recall(query).await {
        Ok(entries) => (
            StatusCode::OK,
            Json(serde_json::json!({ "memories": entries })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ClearMemoryQuery {
    pub session_id: String,
}

/// DELETE /api/memory
pub async fn clear_memory(
    State(state): State<SharedState>,
    Query(params): Query<ClearMemoryQuery>,
) -> impl IntoResponse {
    let memory_provider = match state.agents.memory_provider() {
        Some(provider) => provider,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Memory is disabled" })),
            )
                .into_response();
        }
    };

    match memory_provider.delete_session_memory(&params.session_id).await {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "success": true, "deleted_count": count })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

