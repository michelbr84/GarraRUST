use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Observability/UI
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/logs — stream recent log entries
pub async fn admin_logs(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Sessions, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);
    let log_path = dirs::home_dir()
        .map(|h| h.join(".garraia").join("garraia.log"))
        .unwrap_or_default();

    if !log_path.exists() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"lines": [], "count": 0})),
        )
            .into_response();
    }

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().rev().take(limit).collect();
            let lines: Vec<&str> = lines.into_iter().rev().collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"lines": lines, "count": lines.len()})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

/// GET /admin/api/metrics — current metrics snapshot
pub async fn admin_metrics(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let metrics = crate::observability::global_metrics();
    let active_sessions = state.app_state.sessions.len();
    let active_providers = state.app_state.agents.provider_ids();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "requests_total": metrics.requests_total.load(std::sync::atomic::Ordering::Relaxed),
            "active_sessions": active_sessions,
            "active_providers": active_providers,
            "provider_count": active_providers.len(),
        })),
    )
        .into_response()
}

/// GET /admin/api/metrics/prometheus — raw prometheus format
pub async fn admin_prometheus(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (StatusCode::FORBIDDEN, "insufficient permissions").into_response();
    }

    let body = crate::observability::global_metrics().render_prometheus();
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}

/// GET /admin/api/alerts — basic alerts (provider down, high error rate)
pub async fn admin_alerts(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Alerts, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let mut alerts: Vec<serde_json::Value> = Vec::new();

    let active_ids = state.app_state.agents.provider_ids();
    if active_ids.is_empty() {
        alerts.push(serde_json::json!({
            "level": "warning",
            "source": "providers",
            "message": "No LLM providers are active",
        }));
    }

    let config = state.app_state.current_config();
    if !config.memory.enabled {
        alerts.push(serde_json::json!({
            "level": "info",
            "source": "memory",
            "message": "Memory system is disabled",
        }));
    }

    if config.gateway.api_key.is_none() {
        alerts.push(serde_json::json!({
            "level": "warning",
            "source": "security",
            "message": "No API key configured for the gateway",
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"alerts": alerts, "count": alerts.len()})),
    )
}

/// GET /admin/api/themes — available UI themes
pub async fn list_themes() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "themes": [
            {"id": "dark", "name": "Dark", "description": "Dark theme"},
            {"id": "light", "name": "Light", "description": "Light theme"},
            {"id": "brasil", "name": "Brasil", "description": "Green and gold accent"},
        ],
        "current": "dark",
    }))
}

/// GET /admin/api/layout — layout preferences
pub async fn get_layout_preferences() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "sidebar_compact": false,
        "density": "comfortable",
        "shortcuts": {
            "toggle_sidebar": "Ctrl+B",
            "search": "Ctrl+K",
            "settings": "Ctrl+,",
        }
    }))
}

/// GET /admin/api/templates — list prompt/persona templates
pub async fn list_templates(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let mut templates: Vec<serde_json::Value> = Vec::new();

    if let Some(prompt) = &config.agent.system_prompt {
        templates.push(serde_json::json!({
            "id": "default",
            "name": "Default Agent",
            "system_prompt_preview": if prompt.len() > 100 { &prompt[..100] } else { prompt },
            "provider": config.agent.default_provider,
        }));
    }

    for (name, agent) in &config.agents {
        templates.push(serde_json::json!({
            "id": name,
            "name": name,
            "system_prompt_preview": agent.system_prompt.as_ref()
                .map(|p| if p.len() > 100 { &p[..100] } else { p }),
            "provider": agent.provider,
            "model": agent.model,
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"templates": templates})),
    )
}

/// GET /admin/api/about — build info, version, uptime
pub async fn about(State(state): State<AdminState>) -> Json<serde_json::Value> {
    let active_providers = state.app_state.agents.provider_ids();
    let session_count = state.app_state.sessions.len();

    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "GarraIA",
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "repository": "https://github.com/michelbr84/GarraRUST",
        "license": "MIT",
        "rust_version": "1.85+",
        "active_providers": active_providers.len(),
        "active_sessions": session_count,
    }))
}
