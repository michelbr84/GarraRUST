//! Admin provider console handlers.
//!
//! Slice 9.b of GAR-470 / Q9 of EPIC GAR-430 (Quality Gates Phase 3.6).
//! Extracted from `admin/handlers.rs` (lines 1255-1601) without behavior change.
//! Covers provider listing, settings, health, enable/disable, failover, and overrides.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::middleware::{AuthenticatedAdmin, extract_ip};
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ═══════════════════════════════════════════════════════════════════════
// Phase 3: Providers Console
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/providers — list all known providers with status
pub async fn admin_list_providers(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let active_ids = state.app_state.agents.provider_ids();
    let default_id = state.app_state.agents.default_provider_id();
    let config = state.app_state.current_config();

    let known_providers = [
        ("anthropic", "Anthropic", true),
        ("openai", "OpenAI", true),
        ("openrouter", "OpenRouter", true),
        ("deepseek", "DeepSeek", true),
        ("mistral", "Mistral", true),
        ("sansa", "Sansa", true),
        ("gemini", "Google Gemini", true),
        ("falcon", "Falcon", true),
        ("jais", "Jais", true),
        ("qwen", "Qwen", true),
        ("yi", "Yi", true),
        ("cohere", "Cohere", true),
        ("minimax", "MiniMax", true),
        ("moonshot", "Moonshot K2", true),
        ("ollama", "Ollama", false),
    ];

    let mut providers = Vec::new();
    for (id, display, needs_key) in &known_providers {
        let active = active_ids.contains(&id.to_string());
        let mut model = None;
        let mut models = Vec::new();
        let has_secret = {
            let guard = state.store.lock().await;
            guard.get_secret_meta("default", id, "api_key").is_some()
        };

        if active && let Some(provider) = state.app_state.agents.get_provider(id) {
            model = provider.configured_model().map(|m| m.to_string());
            if let Ok(mut available) = provider.available_models().await {
                available.retain(|m| !m.trim().is_empty());
                available.sort();
                available.dedup();
                models = available;
            }
        }

        let config_entry = config.llm.get(*id);

        providers.push(serde_json::json!({
            "id": id,
            "display_name": display,
            "active": active,
            "is_default": default_id.as_deref() == Some(*id),
            "needs_api_key": *needs_key,
            "has_secret": has_secret,
            "model": model,
            "models": models,
            "base_url": config_entry.and_then(|c| c.base_url.clone()),
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"providers": providers})),
    )
        .into_response()
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UpdateProviderSettingsRequest {
    pub enabled: Option<bool>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub set_default: Option<bool>,
}

/// PUT /admin/api/providers/{id}/settings — update provider settings
pub async fn update_provider_settings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
    Json(body): Json<UpdateProviderSettingsRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    if body.set_default == Some(true) {
        state.app_state.agents.set_default_provider_id(&provider_id);
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "update_settings",
        "provider",
        Some(&provider_id),
        Some(&serde_json::to_string(&body).unwrap_or_default()),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    let requires_restart = body.model.is_some() || body.base_url.is_some();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": requires_restart,
        })),
    )
}

/// GET /admin/api/providers/{id}/health — provider health check
pub async fn provider_health(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let provider = state.app_state.agents.get_provider(&provider_id);
    match provider {
        Some(p) => {
            let model = p.configured_model().map(|m| m.to_string());
            let models_result = p.available_models().await;
            let healthy = models_result.is_ok();
            let model_count = models_result.map(|m| m.len()).unwrap_or(0);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "provider": provider_id,
                    "healthy": healthy,
                    "model": model,
                    "available_models": model_count,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "provider": provider_id,
                "healthy": false,
                "error": "provider not active",
            })),
        ),
    }
}

/// POST /admin/api/providers/{id}/enable
pub async fn enable_provider(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "enable",
        "provider",
        Some(&provider_id),
        None,
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": true,
            "message": format!("Provider '{}' will be enabled on next restart or config reload", provider_id),
        })),
    )
}

/// POST /admin/api/providers/{id}/disable
pub async fn disable_provider(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "disable",
        "provider",
        Some(&provider_id),
        None,
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": true,
            "message": format!("Provider '{}' will be disabled on next restart", provider_id),
        })),
    )
}

/// GET /admin/api/providers/{id}/failover — get failover/resilience status
pub async fn provider_failover(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let active_ids = state.app_state.agents.provider_ids();
    let default_id = state.app_state.agents.default_provider_id();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider_id,
            "active_providers": active_ids,
            "default_provider": default_id,
            "circuit_breaker": {
                "status": if active_ids.contains(&provider_id) { "closed" } else { "open" },
            },
        })),
    )
}

/// GET /admin/api/providers/overrides — per-tenant provider overrides
pub async fn list_provider_overrides(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let overrides: Vec<serde_json::Value> = state
        .app_state
        .channel_models
        .iter()
        .map(|entry| {
            serde_json::json!({
                "channel": entry.key().clone(),
                "model": entry.value().clone(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"overrides": overrides})),
    )
}

#[derive(serde::Deserialize)]
pub struct SetProviderOverrideRequest {
    pub channel: String,
    pub model: String,
}

/// POST /admin/api/providers/overrides
pub async fn set_provider_override(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SetProviderOverrideRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    state
        .app_state
        .channel_models
        .insert(body.channel.clone(), body.model.clone());

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "set_override",
        "provider",
        Some(&body.channel),
        Some(&format!("model={}", body.model)),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}
