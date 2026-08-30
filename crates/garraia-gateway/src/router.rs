use std::sync::Arc;

use axum::Router;
use axum::response::Html;
use axum::routing::{get, patch, post};
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
use crate::stats_handler;
use crate::totp;
use crate::ws;

/// Apply OpenTelemetry tracing + request-id middleware layers when the
/// `telemetry` feature is enabled. No-op otherwise.
#[cfg(feature = "telemetry")]
fn apply_telemetry_layers(router: Router) -> Router {
    // tower layers run in reverse declaration order on incoming requests,
    // so request_id_layer (declared last) runs first and populates the
    // X-Request-Id header before the trace layer records it in the span.
    router
        .layer(garraia_telemetry::propagate_request_id_layer())
        .layer(garraia_telemetry::http_trace_layer())
        .layer(garraia_telemetry::request_id_layer())
}

#[cfg(not(feature = "telemetry"))]
fn apply_telemetry_layers(router: Router) -> Router {
    router
}

/// Build the main application router with all routes.
/// True when the gateway's configured bind address reaches only this machine.
///
/// Resolved rather than string-matched: `localhost`, `127.0.0.1`, `::1` and any
/// other name that answers only with loopback all count, while `0.0.0.0` is
/// unspecified — it accepts connections on every interface — and does not.
/// A host that fails to resolve is treated as NOT loopback: the gate is
/// fail-closed, so an unparseable config errs toward requiring auth.
fn bind_is_loopback(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    match std::net::ToSocketAddrs::to_socket_addrs(&(bare, 0u16)) {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            !addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback())
        }
        Err(_) => false,
    }
}

/// Skills and Skins editors, gated on the bind address.
///
/// These 9 endpoints create, overwrite and delete `.md` / `.json` files under
/// the skills and skins directories. `validate_skill_name` keeps every write
/// inside those directories (see `path_validation.rs`), but nothing decides
/// *who* may write.
///
/// GarraIA is local-first — `gateway.host` defaults to `127.0.0.1`, and the Web
/// Console that drives these editors is itself served unauthenticated at `GET
/// /`. On loopback, adding an admin gate here would only break the console
/// without moving a trust boundary: anyone who can reach the endpoint can
/// already reach the page that calls it.
///
/// Binding anywhere else (`0.0.0.0`, a LAN address) changes that completely —
/// the endpoints become remotely reachable file write and delete. There they
/// get the same treatment as `/api/plugins/*`: admin session, CSRF on mutating
/// methods, and the security headers the `/admin` router carries.
fn build_skill_skin_routes(
    state: SharedState,
    admin_store: Arc<Mutex<admin::store::AdminStore>>,
) -> Router {
    let routes = Router::new()
        .route(
            "/api/skills",
            get(crate::skills_handler::list_skills).post(crate::skills_handler::create_skill),
        )
        .route(
            "/api/skills/import",
            post(crate::skills_handler::import_skill),
        )
        .route(
            "/api/skills/{name}",
            get(crate::skills_handler::get_skill)
                .put(crate::skills_handler::update_skill)
                .delete(crate::skills_handler::delete_skill),
        )
        .route(
            "/api/skills/{name}/export",
            get(crate::skills_handler::export_skill),
        )
        .route(
            "/api/skills/{name}/triggers",
            post(crate::skills_handler::set_skill_triggers),
        )
        .route(
            "/api/skins",
            get(crate::skins_handler::list_skins).post(crate::skins_handler::create_skin),
        )
        .route(
            "/api/skins/{name}",
            get(crate::skins_handler::get_skin).delete(crate::skins_handler::delete_skin),
        );

    if bind_is_loopback(&state.config.gateway.host) {
        return routes.with_state(state);
    }

    tracing::info!(
        host = %state.config.gateway.host,
        "gateway is not bound to loopback: requiring admin auth on /api/skills/* and /api/skins/*"
    );
    routes
        .layer(axum::middleware::from_fn(admin::middleware::require_csrf))
        .layer(axum::middleware::from_fn(
            admin::middleware::require_admin_auth,
        ))
        .layer(axum::Extension(admin_store))
        .layer(axum::middleware::from_fn(
            admin::middleware::security_headers,
        ))
        .with_state(state)
}

pub fn build_router(
    state: SharedState,
    whatsapp_state: garraia_channels::whatsapp::webhook::WhatsAppState,
    admin_store: Arc<Mutex<admin::store::AdminStore>>,
    admin_encryption_key: Arc<Vec<u8>>,
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

    // Build CORS layer — use configured origins or allow all in dev mode.
    let cors_layer = {
        let origins = &state.config.gateway.allowed_origins;
        let cors = CorsLayer::new().allow_methods(Any).allow_headers(Any);
        if origins.is_empty() {
            cors.allow_origin(Any)
        } else {
            let parsed: Vec<axum::http::HeaderValue> =
                origins.iter().filter_map(|o| o.parse().ok()).collect();
            cors.allow_origin(parsed)
        }
    };

    // EU AI Act compliance: inject X-AI-Model and X-AI-Provider headers.
    let default_provider = state.agents.default_provider_id().unwrap_or_default();
    let default_model = state
        .agents
        .get_provider(&default_provider)
        .and_then(|p| p.configured_model().map(|m| m.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let whatsapp_routes = Router::new()
        .route(
            "/webhooks/whatsapp",
            get(garraia_channels::whatsapp::webhook::whatsapp_verify)
                .post(garraia_channels::whatsapp::webhook::whatsapp_webhook),
        )
        .with_state(whatsapp_state);

    let router = Router::new()
        .route("/", get(web_chat))
        .route("/health", get(health))
        .route("/ping", get(ping))
        .route("/api/health", get(crate::health::health_handler))
        .route(
            "/api/capabilities",
            get(crate::health::capabilities_handler),
        )
        .route("/ws", get(ws::ws_handler))
        .route("/ws/parrot", get(parrot_ws::parrot_ws_handler))
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(openai_api::chat_completions))
        .route("/v1/models", get(openai_api::list_models))
        // Anthropic-compatible endpoints (plan 0361 / ADR 0014). Deliberately
        // does NOT register `/v1/models`: it is already registered just above,
        // and Axum panics at startup on a duplicate method+path.
        .route("/v1/messages", post(crate::anthropic_api::messages_handler))
        .route(
            "/v1/messages/count_tokens",
            post(crate::anthropic_api::count_tokens_handler),
        )
        .route("/api/stats", get(stats_handler::stats_handler))
        .route("/api/status", get(status))
        .route("/api/auth-check", get(auth_check))
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}/messages", post(api::send_message))
        .route("/api/sessions/{id}/history", get(api::session_history))
        .route(
            "/api/sessions/{id}",
            axum::routing::delete(api::delete_session),
        )
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
        // Plan 0156 (GAR-651): Learning Agent Web UI
        .route("/learning", get(crate::learning_handler::learning_ui))
        .route(
            "/api/learning/skills",
            get(crate::learning_handler::list_learning_skills),
        )
        .route(
            "/api/learning/skills/{name}",
            get(crate::learning_handler::get_learning_skill)
                .delete(crate::learning_handler::delete_learning_skill),
        )
        .route(
            "/api/learning/skills/{name}/approve",
            post(crate::learning_handler::approve_skill),
        )
        .route(
            "/api/learning/skills/{name}/reject",
            post(crate::learning_handler::reject_skill),
        )
        .route(
            "/api/learning/skills/{name}/lock",
            post(crate::learning_handler::lock_skill),
        )
        .route(
            "/api/learning/skills/{name}/rollback",
            post(crate::learning_handler::rollback_skill),
        )
        .route(
            "/api/learning/logs/sessions",
            get(crate::learning_handler::get_log_sessions),
        )
        .route(
            "/api/learning/logs/candidates",
            get(crate::learning_handler::get_log_candidates),
        )
        .route(
            "/api/learning/logs/scores",
            get(crate::learning_handler::get_log_scores),
        )
        .route("/api/tts", post(crate::voice_handler::synthesize))
        .route("/api/stt", post(crate::voice_handler::transcribe))
        .route("/api/providers", get(list_providers).post(add_provider))
        .route("/api/providers/test", post(test_provider))
        .route("/api/providers/default", patch(set_default_provider))
        .route("/api/channels", get(list_channels))
        .route(
            "/api/diagnostics",
            get(crate::diagnostics_handler::diagnostics_handler),
        )
        .route(
            "/api/settings/schema",
            get(crate::settings_handler::schema_handler),
        )
        .route(
            "/api/settings/effective",
            get(crate::settings_handler::effective_handler),
        )
        .route(
            "/api/settings",
            patch(crate::settings_handler::patch_handler),
        )
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
        .route(
            "/api/modes/custom",
            get(api::list_custom_modes).post(api::create_custom_mode),
        )
        .route(
            "/api/modes/custom/{id}",
            get(api::get_custom_mode)
                .patch(api::update_custom_mode)
                .delete(api::delete_custom_mode),
        )
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
        // Auth routes with strict rate limiting (10 req/min, burst 3).
        //
        // TODO(plan-0023+): migrate these routes from the deprecated
        // `rate_limit_layer` to `rate_limit_layer_authenticated` once
        // the `/auth/*` handlers reliably have a Bearer token on the
        // request (register/login today don't — they MINT the token).
        // For now the #[allow(deprecated)] is the compatibility shim.
        .merge({
            #[allow(deprecated)]
            let auth_limiter = crate::rate_limiter::RateLimiter::auth_limiter();
            #[allow(deprecated)]
            let router = Router::new()
                .route("/auth/register", post(mobile_auth::register))
                .route("/auth/login", post(mobile_auth::login))
                .route("/auth/oauth/providers", get(oauth::list_oauth_providers))
                .route("/auth/oauth/{provider}", get(oauth::oauth_redirect))
                .route(
                    "/auth/oauth/{provider}/callback",
                    get(oauth::oauth_callback),
                )
                .route("/auth/2fa/setup", post(totp::setup_2fa))
                .route("/auth/2fa/verify", post(totp::verify_2fa))
                .route("/auth/2fa/disable", post(totp::disable_2fa))
                .layer(axum::middleware::from_fn_with_state(
                    auth_limiter,
                    crate::rate_limiter::rate_limit_layer,
                ))
                .with_state(state.clone());
            router
        })
        .route("/me", get(mobile_auth::me))
        .route("/chat", post(mobile_chat::chat))
        .route("/chat/history", get(mobile_chat::history))
        // OpenClaw bridge endpoints
        .route(
            "/api/openclaw/status",
            get(crate::openclaw_handler::openclaw_status),
        )
        .route(
            "/api/openclaw/connect",
            post(crate::openclaw_handler::openclaw_connect),
        )
        .route(
            "/api/openclaw/disconnect",
            post(crate::openclaw_handler::openclaw_disconnect),
        )
        .route(
            "/api/openclaw/channels",
            get(crate::openclaw_handler::openclaw_channels),
        )
        // Phase 3.1: Plugin Registry — GAR-459 (PR-A of GAR-454):
        // mounted as a sub-router with `require_admin_auth` + `require_csrf`
        // + admin-store extension applied to the 5 routes (mirrors the
        // /admin nested router's wiring). Handlers also enforce
        // `Permission::ManagePlugins`. See plugins_handler::build_plugin_routes
        // for layer ordering rationale.
        // Phase 3.2: MCP Marketplace
        .route(
            "/api/mcp/marketplace",
            get(crate::mcp_marketplace::marketplace_catalog),
        )
        .route(
            "/api/mcp/marketplace/install",
            post(crate::mcp_marketplace::marketplace_install),
        )
        .route(
            "/api/mcp/{id}/health",
            get(crate::mcp_marketplace::mcp_server_health),
        )
        .route(
            "/api/mcp/{id}/config-schema",
            get(crate::mcp_marketplace::mcp_config_schema),
        )
        // Phase 1.3: Projects
        .route(
            "/api/projects",
            get(crate::projects_handler::list_projects)
                .post(crate::projects_handler::create_project),
        )
        .route(
            "/api/projects/{id}",
            get(crate::projects_handler::get_project)
                .put(crate::projects_handler::update_project)
                .delete(crate::projects_handler::delete_project),
        )
        .route(
            "/api/projects/{id}/files",
            get(crate::projects_handler::list_project_files),
        )
        // A2A protocol endpoints
        .route("/.well-known/agent.json", get(a2a::agent_card))
        .route("/a2a/tasks", post(a2a::create_task))
        .route("/a2a/tasks/{id}", get(a2a::get_task))
        .route("/a2a/tasks/{id}/cancel", post(a2a::cancel_task))
        .nest_service("/assets", ServeDir::new("crates/garraia-gateway/assets"))
        .nest_service("/static", ServeDir::new("assets"))
        // Plan 0024 (GAR-412): the embedded `/metrics` route is guarded
        // by `metrics_auth_layer` at runtime. With the default
        // loopback-only config (no token, no allowlist) loopback peers
        // still get `200` without friction; non-loopback peers without
        // auth receive `503` (not-configured), `401` (bad token), or
        // `403` (allowlist miss).
        .route(
            "/metrics",
            get(crate::observability::prometheus_metrics_handler).layer(
                axum::middleware::from_fn_with_state(
                    state.metrics_auth_cfg.clone(),
                    crate::metrics_auth::metrics_auth_layer,
                ),
            ),
        )
        .route("/admin", get(admin_page))
        .with_state(state.clone())
        .merge(whatsapp_routes)
        // GAR-391c: /v1/auth/{login,refresh,logout,signup} mounted
        // unconditionally. Handlers fail-soft to 503 when AuthConfig env
        // vars are missing (state.auth_provider == None).
        .merge(crate::auth_routes::router().with_state(state.clone()))
        // Fase 3.4 REST /v1 skeleton (plan 0015). Mounts /v1/me,
        // /v1/openapi.json and /docs. Fail-soft: when AuthConfig env
        // vars are missing, every /v1 route answers 503 Problem Details.
        .merge(crate::rest_v1::router(state.clone()))
        // GAR-459 (PR-A of GAR-454): /api/plugins/* protected sub-router.
        // Must merge BEFORE the /admin nest because the nest call moves
        // `admin_store`. The clone keeps the Arc<Mutex<AdminStore>> live
        // for both consumers — same admin store, two mounting points.
        .merge(build_skill_skin_routes(state.clone(), admin_store.clone()))
        .merge(crate::plugins_handler::build_plugin_routes(
            state.clone(),
            admin_store.clone(),
        ))
        .nest(
            "/admin",
            admin::routes::build_admin_router(state, admin_store, admin_encryption_key),
        )
        .layer(governor_layer)
        .layer(cors_layer)
        .layer({
            let model = default_model.clone();
            let provider = default_provider.clone();
            axum::middleware::from_fn(
                move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                    let model = model.clone();
                    let provider = provider.clone();
                    async move {
                        let mut resp = next.run(req).await;
                        resp.headers_mut().insert(
                            "X-AI-Provider",
                            axum::http::HeaderValue::from_str(&provider).unwrap_or_else(|_| {
                                axum::http::HeaderValue::from_static("unknown")
                            }),
                        );
                        resp.headers_mut().insert(
                            "X-AI-Model",
                            axum::http::HeaderValue::from_str(&model).unwrap_or_else(|_| {
                                axum::http::HeaderValue::from_static("unknown")
                            }),
                        );
                        resp
                    }
                },
            )
        });

    apply_telemetry_layers(router)
}

async fn health() -> &'static str {
    "ok"
}

/// GAR-603 — Minimal liveness probe for Runpod Load Balancer Serverless.
///
/// Must be cheap and free of any state, DB, or provider dependency: Runpod
/// routes traffic only to workers that return HTTP 200 here on `PORT_HEALTH`.
/// Independent of `/health` (which aggregates provider status and is heavier).
async fn ping() -> &'static str {
    "pong"
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

/// Fetch policy for a caller-supplied provider `base_url`.
///
/// `IpScope::AllowPrivate` is deliberate: a local Ollama on
/// `http://127.0.0.1:11434` and a LAN LM Studio are the product's offline story,
/// so loopback and RFC 1918 have to stay reachable. What the guard removes is
/// the part that is never legitimate for an LLM endpoint — link-local
/// (`169.254.169.254`, cloud instance metadata), CGNAT, multicast, the
/// unspecified address — plus every non-HTTP scheme.
///
/// Before 2026-08-29 `body.base_url` went straight into the provider
/// constructor and out to `reqwest`, so a single request could point the
/// gateway at instance metadata; `POST /api/providers/test` then fired it and
/// returned the latency, and `PATCH /api/providers/default` routed all chat
/// traffic (and the API key) through it. CodeQL: `rust/request-forgery`, 9.1.
fn provider_base_url_policy() -> garraia_common::ssrf::UrlPolicy {
    garraia_common::ssrf::UrlPolicy::http_public(
        std::time::Duration::from_secs(30),
        concat!("GarraIA/", env!("CARGO_PKG_VERSION")),
    )
    .with_ip_scope(garraia_common::ssrf::IpScope::AllowPrivate)
}

/// Vet a caller-supplied `base_url`, if one was sent. `Ok(())` when absent —
/// omitting it means "use the provider's built-in default", which is a
/// compile-time constant and needs no check.
fn validate_provider_base_url(base_url: Option<&String>) -> Result<(), String> {
    let Some(raw) = base_url else {
        return Ok(());
    };
    garraia_common::ssrf::vet_url(raw, &provider_base_url_policy())
        .map(|_| ())
        .map_err(|e| format!("base_url rejected: {e}"))
}

/// POST /api/providers — add a new LLM provider at runtime.
async fn add_provider(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<AddProviderRequest>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    // SSRF gate, ahead of every provider branch so none can be forgotten.
    if let Err(message) = validate_provider_base_url(body.base_url.as_ref()) {
        tracing::warn!(provider_type = %body.provider_type, "{message}");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "status": "error",
                "message": message,
            })),
        );
    }

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

    // Persist once, centrally, keyed off the shared provider->env-var table.
    // The per-arm `persist_api_key(...)` calls that used to live above threw
    // away the return value, so a vault write that no-opped for lack of
    // `GARRAIA_VAULT_PASSPHRASE` still produced `201 {"status":"ok"}`. The
    // provider then worked for the rest of the process lifetime and vanished on
    // restart — one of the two ways "I added the provider correctly" was true
    // and the gateway still came up with no providers.
    let persisted = match (
        garraia_config::provider_key_env(provider_type),
        body.api_key.as_deref(),
    ) {
        (Some(vault_key), Some(key)) => persist_api_key(vault_key, key),
        // Keyless provider (ollama): nothing to persist, nothing to warn about.
        _ => true,
    };

    let message = if persisted {
        format!("provider '{provider_type}' activated")
    } else {
        tracing::warn!(
            "provider '{provider_type}' is active in memory but its API key was NOT persisted: \
             the credential vault needs {vault_env} to be set. The provider will be gone after \
             the next restart. Either export {vault_env} and try again, or put the key in \
             `llm.{provider_type}.api_key` in config.yml.",
            vault_env = garraia_config::provider_keys::VAULT_PASSPHRASE_ENV
        );
        format!(
            "provider '{provider_type}' activated for this session only — the API key could not \
             be persisted (the credential vault requires {} to be set), so it will be lost on \
             restart.",
            garraia_config::provider_keys::VAULT_PASSPHRASE_ENV
        )
    };

    (
        axum::http::StatusCode::CREATED,
        axum::Json(serde_json::json!({
            "status": "ok",
            "message": message,
            // `false` means in-memory only — the caller must persist the key
            // another way if it should survive a restart.
            "persisted": persisted,
        })),
    )
}

/// Best-effort: persist an API key in the vault. Returns `false` when the write
/// did not happen — most commonly because `GARRAIA_VAULT_PASSPHRASE` is unset,
/// which `garraia_security::try_vault_set` reports by returning `false` without
/// logging anything itself.
fn persist_api_key(vault_key: &str, value: &str) -> bool {
    match crate::bootstrap::default_vault_path() {
        Some(vault_path) => garraia_security::try_vault_set(&vault_path, vault_key, value),
        None => false,
    }
}

// ─── Provider test / default (plan 0119 / PR-6) ───────────────────────────

/// POST /api/providers/test — exercise the registered provider by listing
/// its models. The response is a small `{ok, latency_ms, error?}` payload
/// that the Web Console renders next to each provider card. Never echoes
/// the API key or any portion of the request.
#[derive(serde::Deserialize)]
struct ProviderTestRequest {
    provider: String,
}

#[derive(serde::Serialize)]
struct ProviderTestResponse {
    ok: bool,
    provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn test_provider(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<ProviderTestRequest>,
) -> (axum::http::StatusCode, axum::Json<ProviderTestResponse>) {
    let id = body.provider.trim();
    if id.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(ProviderTestResponse {
                ok: false,
                provider: id.to_string(),
                latency_ms: None,
                model_count: None,
                error: Some("provider id is required".into()),
            }),
        );
    }
    let Some(provider) = state.agents.get_provider(id) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(ProviderTestResponse {
                ok: false,
                provider: id.to_string(),
                latency_ms: None,
                model_count: None,
                error: Some("provider not registered".into()),
            }),
        );
    };

    let start = std::time::Instant::now();
    match provider.available_models().await {
        Ok(models) => (
            axum::http::StatusCode::OK,
            axum::Json(ProviderTestResponse {
                ok: true,
                provider: id.to_string(),
                latency_ms: Some(start.elapsed().as_millis()),
                model_count: Some(models.len()),
                error: None,
            }),
        ),
        Err(err) => (
            axum::http::StatusCode::OK,
            axum::Json(ProviderTestResponse {
                ok: false,
                provider: id.to_string(),
                latency_ms: Some(start.elapsed().as_millis()),
                model_count: None,
                error: Some(format!("{err}")),
            }),
        ),
    }
}

// ─── Channels (plan 0120 / PR-7) ───────────────────────────────────────────

/// Known channels — display metadata mirrors `KNOWN_PROVIDERS`. The `id`
/// column matches `ChannelRegistry` entries; `needs_secret` is purely
/// informational (the Web Console renders an amber pill when true but the
/// actual secret value never crosses the API boundary).
const KNOWN_CHANNELS: &[(&str, &str, bool)] = &[
    ("web", "Web Chat", false),
    ("api", "REST API", false),
    ("telegram", "Telegram", true),
    ("discord", "Discord", true),
    ("slack", "Slack", true),
    ("whatsapp", "WhatsApp", true),
    ("imessage", "iMessage", false),
    ("openclaw", "OpenClaw", false),
    ("mcp", "MCP", false),
    ("cli", "CLI", false),
];

#[derive(serde::Serialize)]
struct ChannelInfo {
    id: &'static str,
    display_name: &'static str,
    /// `"active"` (registered + live), `"configured"` (known but not registered),
    /// `"offline"` (not registered, secret needed), `"optional"` (no secret needed).
    status: &'static str,
    needs_secret: bool,
    /// Server-side timestamp of process boot — `last_activity` is not tracked
    /// per channel yet (plan 0122 follow-up). Present as a stable placeholder
    /// so the Web Console column always has SOMETHING to render.
    boot_time_secs: u64,
}

/// GET /api/channels — Web Console Channels page payload. Secret-free.
async fn list_channels(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> axum::Json<serde_json::Value> {
    let live: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let mut channels: Vec<ChannelInfo> = Vec::with_capacity(KNOWN_CHANNELS.len());
    for (id, display, needs_secret) in KNOWN_CHANNELS {
        let active = live.iter().any(|name| name == *id);
        let status = if active {
            "active"
        } else if *needs_secret {
            "offline"
        } else {
            "optional"
        };
        channels.push(ChannelInfo {
            id,
            display_name: display,
            status,
            needs_secret: *needs_secret,
            boot_time_secs: state.boot_time.elapsed().as_secs(),
        });
    }

    axum::Json(serde_json::json!({ "channels": channels }))
}

/// PATCH /api/providers/default — switch the default LLM provider.
#[derive(serde::Deserialize)]
struct SetDefaultProviderRequest {
    provider: String,
}

async fn set_default_provider(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<SetDefaultProviderRequest>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    let id = body.provider.trim();
    if id.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "ok": false,
                "error": "provider is required",
            })),
        );
    }
    if !state.agents.provider_ids().iter().any(|p| p == id) {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "ok": false,
                "error": "provider not registered",
            })),
        );
    }
    state.agents.set_default_provider_id(id);
    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({
            "ok": true,
            "default": id,
        })),
    )
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
    } else if servers
        .iter()
        .all(|s| s["connected"].as_bool().unwrap_or(false))
    {
        "all_connected"
    } else if servers
        .iter()
        .any(|s| s["connected"].as_bool().unwrap_or(false))
    {
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
    let commands = crate::slash_commands::list_commands(state.mcp_manager_arc.as_ref()).await;
    axum::Json(serde_json::json!({ "commands": commands }))
}

/// Read the cached latest version from ~/.garraia/update-check.json.
fn read_cached_latest_version() -> Option<String> {
    let path = garraia_config::ConfigLoader::default_config_dir().join("update-check.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&contents).ok()?;
    v.get("latest_version")?.as_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// GAR-603 — Runpod Load Balancer Serverless requires `GET /ping` to return
    /// HTTP 200 fast (no DB / provider dependency) for a worker to be considered
    /// healthy. This test pins both the handler body and the route wiring under
    /// the same router builder pattern used in production (`router.rs:96-99`).
    #[tokio::test]
    async fn ping_route_returns_200_pong() {
        let app: Router = Router::new().route("/ping", get(ping));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .body(Body::empty())
                    .expect("ping request"),
            )
            .await
            .expect("oneshot");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        assert_eq!(&body[..], b"pong");
    }

    // ── Bind-conditional gate on /api/skills/* and /api/skins/* ─────────────

    #[test]
    fn loopback_binds_are_recognised() {
        assert!(super::bind_is_loopback("127.0.0.1"));
        assert!(super::bind_is_loopback("localhost"));
        assert!(super::bind_is_loopback("::1"));
        assert!(super::bind_is_loopback("[::1]"));
    }

    #[test]
    fn wildcard_bind_is_not_loopback() {
        // The case that matters: 0.0.0.0 accepts connections on every
        // interface, so the editors must be gated behind admin auth.
        assert!(!super::bind_is_loopback("0.0.0.0"));
        assert!(!super::bind_is_loopback("::"));
    }

    #[test]
    fn routable_address_is_not_loopback() {
        assert!(!super::bind_is_loopback("10.0.0.5"));
        assert!(!super::bind_is_loopback("93.184.216.34"));
    }

    #[test]
    fn unresolvable_host_fails_closed() {
        // An unparseable bind must err toward requiring auth, never toward
        // leaving the endpoints open.
        assert!(!super::bind_is_loopback(""));
        assert!(!super::bind_is_loopback("this is not a hostname"));
    }
}
