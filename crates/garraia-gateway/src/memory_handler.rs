use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
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
        tenant_id: None,
        query_text: None,
        query_embedding: None,
        embedding_model: None,
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
        tenant_id: None,
        query_text: Some(params.q),
        query_embedding: None,
        embedding_model: None,
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

    match memory_provider
        .delete_session_memory(&params.session_id)
        .await
    {
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

/// DELETE /api/memory/{id} — one entry, by id.
///
/// `204` when it was there, `404` when it was not (already gone, or never
/// this runtime's), `503` with memory disabled. Same shape the mobile app
/// expects from every other `/api/*` delete. Pinned entries go too:
/// `MemoryStore::delete_entry` does not consult the pin, so the client is the
/// one that warns before asking.
pub async fn delete_memory_entry(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(memory_provider) = state.agents.memory_provider() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Memory is disabled" })),
        )
            .into_response();
    };

    match memory_provider.delete_entry(&id).await {
        Ok(true) => {
            // Trilha minima para uma operacao irreversivel: o id, nunca o
            // conteudo (auditoria do #1043). O admin loga em audit_events;
            // aqui o log estruturado e o que existe.
            tracing::info!(memory_id = %id, "memory entry deleted via /api/memory/{id}");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "memory entry not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;
    use garraia_db::{MemoryProvider, MemoryRole, MemoryStore, NewMemoryEntry};
    use std::sync::Arc;

    async fn state_with_memory(dir: &std::path::Path) -> (SharedState, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::open(&dir.join("memory.db")).expect("abrir memoria"));
        let mut runtime = AgentRuntime::new();
        runtime.set_memory_provider(Arc::clone(&store) as Arc<dyn MemoryProvider>);
        let state = Arc::new(crate::state::AppState::new(
            AppConfig::default(),
            Arc::new(runtime),
            ChannelRegistry::new(),
        ));
        (state, store)
    }

    async fn remember(store: &MemoryStore, content: &str) -> String {
        store
            .remember(NewMemoryEntry {
                tenant_id: "default".into(),
                session_id: "s1".into(),
                channel_id: None,
                user_id: None,
                continuity_key: None,
                role: MemoryRole::User,
                content: content.into(),
                embedding: None,
                embedding_model: None,
                metadata: serde_json::json!({}),
            })
            .await
            .expect("remember")
    }

    /// 204 apaga so a entrada pedida; a outra fica; repetir da 404.
    #[tokio::test]
    async fn deletes_one_entry_and_reports_absence_after() {
        let dir = tempfile::tempdir().unwrap();
        let (state, store) = state_with_memory(dir.path()).await;
        let a = remember(&store, "lembrar do cafe").await;
        let b = remember(&store, "lembrar do pao").await;

        let res = delete_memory_entry(State(Arc::clone(&state)), Path(a.clone()))
            .await
            .into_response();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = delete_memory_entry(State(Arc::clone(&state)), Path(a))
            .await
            .into_response();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        let res = delete_memory_entry(State(state), Path(b))
            .await
            .into_response();
        assert_eq!(
            res.status(),
            StatusCode::NO_CONTENT,
            "a outra entrada seguia la"
        );
    }

    /// Sem memoria configurada, 503 — nunca um 404 que pareceria "ja apagou".
    #[tokio::test]
    async fn memory_disabled_is_503() {
        let state = Arc::new(crate::state::AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        ));
        let res = delete_memory_entry(State(state), Path("x".into()))
            .await
            .into_response();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
