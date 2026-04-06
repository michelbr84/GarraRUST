use std::sync::Arc;

use axum::Router;
use axum::response::Html;
use axum::routing::{get, post};
use tokio::sync::Mutex;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::a2a;
use crate::admin;
use crate::api;
use crate::mobile_auth;
use crate::mobile_chat;
use crate::oauth;
use crate::openai_api;
use crate::parrot_ws;
use crate::state::SharedState;
use crate::totp;
use crate::ws;

/// Build the main application router with all routes.
pub fn build_router(
    state: SharedState,
    whatsapp_state: garraia_channels::whatsapp::webhook::WhatsAppState,
    admin_store: Arc<Mutex<admin::store::AdminStore>>,
) -> Router {
    // Per-IP rate limit from config (default: 1 req/sec, burst 60).
    let rl = &state.config.gateway.rate_limit;
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(rl.per_second)
        .burst_size(rl.burst_size)
        .finish()
        .expect("governor config should be valid");
    let governor_limiter = governor_conf.limiter().clone();
    let governor_layer = GovernorLayer::new(governor_conf);

    // Spawn a background task to clean up rate-limiter state for inactive IPs.
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(60);
        loop {
            tokio::time::sleep(interval).await;
            governor_limiter.retain_recent();
        }
    });

    let whatsapp_routes = Router::new()
        .route(
            "/webhooks/whatsapp",
            get(garraia_channels::whatsapp::webhook::whatsapp_verify)
                .post(garraia_channels::whatsapp::webhook::whatsapp_webhook),
        )
        .with_state(whatsapp_state);

    Router::new()
        .route("/", get(web_chat))
        .route("/health", get(health))
        .route("/api/health", get(crate::health::health_handler))
        .route("/ws", get(ws::ws_handler))
        .route("/ws/parrot", get(parrot_ws::parrot_ws_handler))
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(openai_api::chat_completions))
        .route("/v1/models", get(openai_api::list_models))
        .route("/api/status", get(status))
        .route("/api/auth-check", get(auth_check))
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}/messages", post(api::send_message))
        .route("/api/sessions/{id}/history", get(api::session_history))
        .route("/api/sessions/{id}", axum::routing::delete(api::delete_session))
        .route(
            "/api/memory",
            axum::routing::delete(crate::memory_handler::clear_memory),
        )
        .route(
            "/api/memory/recent",
            get(crate::memory_handler::get_recent_memory),
        )
        .route(
            "/api/memory/search",
            get(crate::memory_handler::search_memory),
        )
        .route("/api/logs", get(crate::logs_handler::get_logs))
        .route("/api/tts", post(crate::voice_handler::synthesize))
        .route("/api/stt", post(crate::voice_handler::transcribe))
        .route("/api/providers", get(list_providers).post(add_provider))
        .route("/api/mcp", get(list_mcp_servers))
        .route("/api/mcp/tools", get(list_mcp_runtime_tools))
        .route("/api/mcp/health", get(mcp_health))
        // GAR-184: Dynamic slash commands
        .route("/api/slash-commands", get(list_slash_commands))
        // GAR-230: Mode API endpoints
        .route("/api/modes", get(api::list_modes))
        .route("/api/mode/select", post(api::select_mode))
        .route("/api/mode/current", get(api::current_mode))
        // GAR-232: Custom Mode API endpoints
        .route("/api/modes/custom", get(api::list_custom_modes).post(api::create_custom_mode))
        .route("/api/modes/custom/{id}", get(api::get_custom_mode).patch(api::update_custom_mode).delete(api::delete_custom_mode))
        // Runtime endpoints - temporarily disabled
        // .route(
        //     "/api/runtime/run",
        //     post(runtime_handler::run_turn_handler),
        // )
        // .route(
        //     "/api/runtime/tools",
        //     get(runtime_handler::list_tools_handler),
        // )
        // GAR-335/339: Mobile Cloud Alpha — auth + chat endpoints
        .route("/auth/register", post(mobile_auth::register))
        .route("/auth/login", post(mobile_auth::login))
        .route("/me", get(mobile_auth::me))
        .route("/chat", post(mobile_chat::chat))
        .route("/chat/history", get(mobile_chat::history))
        // Phase 7.1 — OAuth2/OIDC login
        .route("/auth/oauth/providers", get(oauth::list_oauth_providers))
        .route("/auth/oauth/{provider}", get(oauth::oauth_redirect))
        .route("/auth/oauth/{provider}/callback", get(oauth::oauth_callback))
        // Phase 7.1 — TOTP 2FA
        .route("/auth/2fa/setup", post(totp::setup_2fa))
        .route("/auth/2fa/verify", post(totp::verify_2fa))
        .route("/auth/2fa/disable", post(totp::disable_2fa))
        // OpenClaw bridge endpoints
        .route("/api/openclaw/status", get(crate::openclaw_handler::openclaw_status))
        .route("/api/openclaw/connect", post(crate::openclaw_handler::openclaw_connect))
        .route("/api/openclaw/disconnect", post(crate::openclaw_handler::openclaw_disconnect))
        .route("/api/openclaw/channels", get(crate::openclaw_handler::openclaw_channels))
        // Phase 3.1: Plugin Registry
        .route("/api/plugins/install", post(crate::plugins_handler::install_plugin))
        .route("/api/plugins", get(crate::plugins_handler::list_plugins))
        .route("/api/plugins/{id}", get(crate::plugins_handler::get_plugin).delete(crate::plugins_handler::uninstall_plugin))
        .route("/api/plugins/{id}/toggle", post(crate::plugins_handler::toggle_plugin))
        // Phase 3.2: MCP Marketplace
        .route("/api/mcp/marketplace", get(crate::mcp_marketplace::marketplace_catalog))
        .route("/api/mcp/marketplace/install", post(crate::mcp_marketplace::marketplace_install))
        .route("/api/mcp/{id}/health", get(crate::mcp_marketplace::mcp_server_health))
        .route("/api/mcp/{id}/config-schema", get(crate::mcp_marketplace::mcp_config_schema))
        // Phase 3.3: Skills Editor
        .route("/api/skills", get(crate::skills_handler::list_skills).post(crate::skills_handler::create_skill))
        .route("/api/skills/import", post(crate::skills_handler::import_skill))
        .route("/api/skills/{name}", get(crate::skills_handler::get_skill).put(crate::skills_handler::update_skill).delete(crate::skills_handler::delete_skill))
        .route("/api/skills/{name}/export", get(crate::skills_handler::export_skill))
        .route("/api/skills/{name}/triggers", post(crate::skills_handler::set_skill_triggers))
        // A2A protocol endpoints
        .route("/.well-known/agent.json", get(a2a::agent_card))
        .route("/a2a/tasks", post(a2a::create_task))
        .route("/a2a/tasks/{id}", get(a2a::get_task))
        .route("/a2a/tasks/{id}/cancel", post(a2a::cancel_task))
        .nest_service("/assets", ServeDir::new("crates/garraia-gateway/assets"))
        .nest_service("/static", ServeDir::new("assets"))
        .route(
            "/metrics",
            get(crate::observability::prometheus_metrics_handler),
        )
        .route("/admin", get(admin_page))
        .with_state(state.clone())
        .merge(whatsapp_routes)
        .nest(
            "/admin",
            admin::routes::build_admin_router(state, admin_store),
        )
        .layer(governor_layer)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}

async fn health() -> &'static str {
    "ok"
}

async fn admin_page() -> Html<String> {
    if let Ok(content) = std::fs::read_to_string("crates/garraia-gateway/src/admin.html") {
        return Html(content);
    }
    Html(include_str!("admin.html").to_string())
}

async fn web_chat() -> Html<String> {
    if let Ok(content) = std::fs::read_to_string("crates/garraia-gateway/src/webchat.html") {
        return Html(content);
    }
    Html(include_str!("webchat.html").to_string())
}

async fn status(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let llm: serde_json::Value = state
        .config
        .llm
        .iter()
        .map(|(name, cfg)| {
            let mut info = serde_json::json!({ "provider": cfg.provider });
            if let Some(m) = &cfg.model {
                info["model"] = serde_json::Value::String(m.clone());
            }
            (name.clone(), info)
        })
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    // Check for available update (from cached check file)
    let latest_version = read_cached_latest_version();

    let mut resp = serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "channels": channels,
        "sessions": state.sessions.len(),
        "llm": llm,
    });
    if let Some(latest) = latest_version {
        let current = env!("CARGO_PKG_VERSION");
        if latest.trim_start_matches('v') != current {
            resp["latest_version"] = serde_json::Value::String(latest);
        }
    }

    axum::Json(resp)
}

async fn auth_check(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "auth_required": state.config.gateway.api_key.is_some(),
    }))
}

/// Known provider types that can be added at runtime.
const KNOWN_PROVIDERS: &[(&str, &str, bool)] = &[
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

/// GET /api/providers — list known provider types with activation status.
async fn list_providers(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let active_ids = state.agents.provider_ids();
    let default_id = state.agents.default_provider_id();

    let mut providers: Vec<serde_json::Value> = Vec::with_capacity(KNOWN_PROVIDERS.len());
    for (id, display, needs_key) in KNOWN_PROVIDERS {
        let active = active_ids.contains(&id.to_string());
        let mut model = None;
        let mut models = Vec::new();

        if active && let Some(provider) = state.agents.get_provider(id) {
            model = provider.configured_model().map(|m| m.to_string());
            match provider.available_models().await {
                Ok(mut available) => {
                    available.retain(|m| !m.trim().is_empty());
                    available.sort();
                    available.dedup();
                    models = available;
                }
                Err(err) => {
                    tracing::warn!("failed to list models for provider {}: {}", id, err);
                }
            }
        }

        if let Some(selected) = model.as_ref()
            && !models.iter().any(|m| m == selected)
        {
            models.insert(0, selected.clone());
        }

        providers.push(serde_json::json!({
            "id": id,
            "display_name": display,
            "active": active,
            "is_default": default_id.as_deref() == Some(*id),
            "needs_api_key": *needs_key,
            "model": model,
            "models": models,
        }));
    }

    axum::Json(serde_json::json!({ "providers": providers }))
}

#[derive(serde::Deserialize)]
struct AddProviderRequest {
    provider_type: String,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    set_default: Option<bool>,
}

/// POST /api/providers — add a new LLM provider at runtime.
async fn add_provider(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<AddProviderRequest>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    let provider_type = body.provider_type.as_str();

    // Check if this provider type already exists
    let existing = state.agents.provider_ids();
    if existing.contains(&provider_type.to_string()) {
        // If requesting set_default, just switch
        if body.set_default == Some(true) {
            state.agents.set_default_provider_id(provider_type);
            return (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("switched default provider to {provider_type}"),
                })),
            );
        }
        return (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({
                "status": "error",
                "message": format!("provider '{provider_type}' is already active"),
            })),
        );
    }

    // Build and register the provider
    match provider_type {
        "anthropic" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for anthropic",
                    })),
                );
            };
            let provider = garraia_agents::AnthropicProvider::new(
                key.clone(),
                body.model.clone(),
                body.base_url.clone(),
            );
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("ANTHROPIC_API_KEY", key);
        }
        "openai" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for openai",
                    })),
                );
            };
            let provider = garraia_agents::OpenAiProvider::new(
                key.clone(),
                body.model.clone(),
                body.base_url.clone(),
            );
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("OPENAI_API_KEY", key);
        }
        "openrouter" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for openrouter",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://openrouter.ai/api/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("openai/gpt-4o".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("openrouter");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("OPENROUTER_API_KEY", key);
        }
        "sansa" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for sansa",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.sansaml.com".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("sansa-auto".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("sansa");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("SANSA_API_KEY", key);
        }
        "deepseek" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for deepseek",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.deepseek.com".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("deepseek-chat".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("deepseek");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("DEEPSEEK_API_KEY", key);
        }
        "mistral" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for mistral",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.mistral.ai".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("mistral-large-latest".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("mistral");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("MISTRAL_API_KEY", key);
        }
        "gemini" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for gemini",
                    })),
                );
            };
            let base_url = body.base_url.clone().or_else(|| {
                Some("https://generativelanguage.googleapis.com/v1beta/openai/".to_string())
            });
            let model = body
                .model
                .clone()
                .or_else(|| Some("gemini-2.5-flash".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("gemini");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("GEMINI_API_KEY", key);
        }
        "falcon" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for falcon",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.ai71.ai/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("tiiuae/falcon-180b-chat".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("falcon");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("FALCON_API_KEY", key);
        }
        "jais" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for jais",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.core42.ai/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("jais-adapted-70b-chat".to_string()));
            let provider =
                garraia_agents::OpenAiProvider::new(key.clone(), model, base_url).with_name("jais");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("JAIS_API_KEY", key);
        }
        "qwen" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for qwen",
                    })),
                );
            };
            let base_url = body.base_url.clone().or_else(|| {
                Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string())
            });
            let model = body.model.clone().or_else(|| Some("qwen-plus".to_string()));
            let provider =
                garraia_agents::OpenAiProvider::new(key.clone(), model, base_url).with_name("qwen");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("QWEN_API_KEY", key);
        }
        "yi" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for yi",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.lingyiwanwu.com/v1".to_string()));
            let model = body.model.clone().or_else(|| Some("yi-large".to_string()));
            let provider =
                garraia_agents::OpenAiProvider::new(key.clone(), model, base_url).with_name("yi");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("YI_API_KEY", key);
        }
        "cohere" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for cohere",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.cohere.com/compatibility/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("command-r-plus".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("cohere");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("COHERE_API_KEY", key);
        }
        "minimax" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for minimax",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.minimaxi.chat/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("MiniMax-Text-01".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("minimax");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("MINIMAX_API_KEY", key);
        }
        "moonshot" => {
            let Some(key) = &body.api_key else {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "status": "error",
                        "message": "api_key is required for moonshot",
                    })),
                );
            };
            let base_url = body
                .base_url
                .clone()
                .or_else(|| Some("https://api.moonshot.cn/v1".to_string()));
            let model = body
                .model
                .clone()
                .or_else(|| Some("kimi-k2-0711-preview".to_string()));
            let provider = garraia_agents::OpenAiProvider::new(key.clone(), model, base_url)
                .with_name("moonshot");
            state.agents.register_provider(Arc::new(provider));
            persist_api_key("MOONSHOT_API_KEY", key);
        }
        "ollama" => {
            let provider =
                garraia_agents::OllamaProvider::new(body.model.clone(), body.base_url.clone());
            state.agents.register_provider(Arc::new(provider));
        }
        other => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "status": "error",
                    "message": format!("unknown provider type: {other}"),
                })),
            );
        }
    }

    if body.set_default == Some(true) {
        state.agents.set_default_provider_id(provider_type);
    }

    (
        axum::http::StatusCode::CREATED,
        axum::Json(serde_json::json!({
            "status": "ok",
            "message": format!("provider '{provider_type}' activated"),
        })),
    )
}

/// Best-effort: persist an API key in the vault.
fn persist_api_key(vault_key: &str, value: &str) {
    if let Some(vault_path) = crate::bootstrap::default_vault_path() {
        garraia_security::try_vault_set(&vault_path, vault_key, value);
    }
}

/// GET /api/mcp — list connected MCP servers with tool counts and status.
async fn list_mcp_servers(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let servers = if let Some(mgr) = &state.mcp_manager_arc {
        let list = mgr.list_servers().await;
        list.into_iter()
            .map(|(name, tool_count, connected)| {
                serde_json::json!({
                    "name": name,
                    "tools": tool_count,
                    "connected": connected,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    axum::Json(serde_json::json!({ "servers": servers }))
}

/// GET /api/mcp/tools — list all tools currently registered in AgentRuntime (includes MCP tools).
async fn list_mcp_runtime_tools(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let all_tools = state.agents.tool_names();
    let mcp_server_tools: Vec<serde_json::Value> = if let Some(mgr) = &state.mcp_manager_arc {
        mgr.list_servers()
            .await
            .into_iter()
            .map(|(name, tool_count, connected)| {
                serde_json::json!({
                    "server": name,
                    "tool_count": tool_count,
                    "connected": connected,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    axum::Json(serde_json::json!({
        "runtime_tools": all_tools,
        "runtime_tool_count": all_tools.len(),
        "mcp_servers": mcp_server_tools,
    }))
}

/// GET /api/mcp/health — per-server MCP connection status and tool inventory.
async fn mcp_health(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let (servers, total_mcp_tools) = if let Some(mgr) = &state.mcp_manager_arc {
        let list = mgr.list_servers().await;
        let total: usize = list.iter().map(|(_, count, _)| count).sum();
        let servers = list
            .into_iter()
            .map(|(name, tool_count, connected)| {
                serde_json::json!({
                    "name": name,
                    "connected": connected,
                    "tool_count": tool_count,
                    "status": if connected { "ok" } else { "disconnected" },
                })
            })
            .collect::<Vec<_>>();
        (servers, total)
    } else {
        (Vec::new(), 0)
    };

    let all_runtime_tools = state.agents.tool_names();
    let overall_status = if servers.is_empty() {
        "no_mcp_configured"
    } else if servers.iter().all(|s| s["connected"].as_bool().unwrap_or(false)) {
        "all_connected"
    } else if servers.iter().any(|s| s["connected"].as_bool().unwrap_or(false)) {
        "partial"
    } else {
        "all_disconnected"
    };

    axum::Json(serde_json::json!({
        "status": overall_status,
        "servers": servers,
        "total_mcp_tools_available": total_mcp_tools,
        "runtime_tool_count": all_runtime_tools.len(),
        "runtime_tools": all_runtime_tools,
    }))
}

/// GET /api/slash-commands — list all available slash commands (GAR-184).
///
/// Returns built-in commands plus any prompts exposed by connected MCP servers.
async fn list_slash_commands(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let commands =
        crate::slash_commands::list_commands(state.mcp_manager_arc.as_ref()).await;
    axum::Json(serde_json::json!({ "commands": commands }))
}

/// Read the cached latest version from ~/.garraia/update-check.json.
fn read_cached_latest_version() -> Option<String> {
    let path = garraia_config::ConfigLoader::default_config_dir().join("update-check.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&contents).ok()?;
    v.get("latest_version")?.as_str().map(|s| s.to_string())
}
