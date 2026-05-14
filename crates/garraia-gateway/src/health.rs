//! Centralized health check module for all GarraIA providers and services.
//!
//! Provides:
//! - `HealthStatus` — result of a single health check
//! - `HealthCheckable` — trait for any service that can be health-checked
//! - `run_all_checks()` — execute all registered checks
//! - `format_boot_table()` — pretty terminal output at startup
//! - `spawn_periodic_checks()` — background task for continuous monitoring
//! - `health_handler()` — axum GET /api/health endpoint

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::state::SharedState;

// ─── Types ─────────────────────────────────────────────────────────────────

/// Result of a single health check.
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Aggregate health response for the `/api/health` endpoint.
///
/// Plan 0118 / PR-5 extends the original `{status, checks}` shape with the
/// fields the web-console Dashboard consumes (`version`, `gateway_url`,
/// `uptime_secs`, `active_sessions`, `provider`, `model`, `channels`,
/// `warnings`). `checks` is retained for back-compat with any operator
/// scripts that already parsed it.
///
/// Secret-free by construction: nothing in the response derives from
/// `state.config.gateway.api_key` or any provider API key.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    /// Overall status: "healthy", "degraded", or "unhealthy"
    pub status: String,
    /// Gateway binary version (`CARGO_PKG_VERSION` at build time).
    pub version: &'static str,
    /// Effective listener URL (`http(s)://host:port`).
    pub gateway_url: String,
    /// Seconds since process boot.
    pub uptime_secs: u64,
    /// Number of in-memory sessions (DashMap entry count).
    pub active_sessions: usize,
    /// Default LLM provider id, if one is configured.
    pub provider: Option<String>,
    /// Configured model on the default provider, if any.
    pub model: Option<String>,
    /// Live channel registry list.
    pub channels: Vec<String>,
    /// Human-readable warnings derived from failed `checks`.
    pub warnings: Vec<String>,
    /// Per-check results (preserved for back-compat).
    pub checks: Vec<HealthStatus>,
}

/// Cached health check results, updated periodically by the background task.
pub type HealthCache = Arc<RwLock<Vec<HealthStatus>>>;

/// Create a new empty health cache.
pub fn new_health_cache() -> HealthCache {
    Arc::new(RwLock::new(Vec::new()))
}

// ─── Health Check Implementations ──────────────────────────────────────────

/// Check an HTTP endpoint's health by hitting a URL and measuring latency.
async fn check_http(name: &str, url: &str, timeout_secs: u64) -> HealthStatus {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let start = Instant::now();
    match client.get(url).send().await {
        Ok(resp) => {
            let latency = start.elapsed().as_millis();
            if resp.status().is_success()
                || resp.status().as_u16() == 405
                || resp.status().as_u16() == 406
            {
                // 405/406 = endpoint exists but wrong method — still reachable
                HealthStatus {
                    name: name.to_string(),
                    ok: true,
                    latency_ms: Some(latency),
                    error: None,
                }
            } else {
                HealthStatus {
                    name: name.to_string(),
                    ok: false,
                    latency_ms: Some(latency),
                    error: Some(format!("HTTP {}", resp.status())),
                }
            }
        }
        Err(e) => {
            let latency = start.elapsed().as_millis();
            HealthStatus {
                name: name.to_string(),
                ok: false,
                latency_ms: Some(latency),
                error: Some(format!("{e}")),
            }
        }
    }
}

/// Run health checks for all known providers and services.
///
/// Checks are run concurrently for speed.
pub async fn run_all_checks(state: &SharedState) -> Vec<HealthStatus> {
    let timeout = state.config.timeouts.health.default_secs;
    let mut handles = Vec::new();

    // Check each LLM provider
    for (name, llm_config) in &state.config.llm {
        let name = name.clone();
        let provider = llm_config.provider.clone();
        let base_url = llm_config.base_url.clone();

        let check_url = match provider.as_str() {
            "ollama" => {
                let base = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
                Some(format!("{}/api/tags", base))
            }
            "openrouter" => {
                // Check if API key is configured first
                let has_key =
                    llm_config.api_key.is_some() || std::env::var("OPENROUTER_API_KEY").is_ok();
                if has_key {
                    Some("https://openrouter.ai/api/v1/models".to_string())
                } else {
                    // No API key — report as disabled, don't hit the endpoint
                    handles.push(tokio::spawn(async move {
                        HealthStatus {
                            name,
                            ok: false,
                            latency_ms: None,
                            error: Some("no API key configured".to_string()),
                        }
                    }));
                    continue;
                }
            }
            "openai" => {
                let has_key =
                    llm_config.api_key.is_some() || std::env::var("OPENAI_API_KEY").is_ok();
                if has_key {
                    let base = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());
                    let base = base.trim_end_matches('/');
                    // Avoid /v1/v1/models when base_url already ends with /v1
                    let health_url = if base.ends_with("/v1") {
                        format!("{}/models", base)
                    } else {
                        format!("{}/v1/models", base)
                    };
                    Some(health_url)
                } else {
                    handles.push(tokio::spawn(async move {
                        HealthStatus {
                            name,
                            ok: false,
                            latency_ms: None,
                            error: Some("no API key configured".to_string()),
                        }
                    }));
                    continue;
                }
            }
            "anthropic" => {
                let has_key =
                    llm_config.api_key.is_some() || std::env::var("ANTHROPIC_API_KEY").is_ok();
                if !has_key {
                    handles.push(tokio::spawn(async move {
                        HealthStatus {
                            name,
                            ok: false,
                            latency_ms: None,
                            error: Some("no API key configured".to_string()),
                        }
                    }));
                    continue;
                }
                // Anthropic doesn't have a simple health endpoint, skip HTTP check
                handles.push(tokio::spawn(async move {
                    HealthStatus {
                        name,
                        ok: true,
                        latency_ms: None,
                        error: None,
                    }
                }));
                continue;
            }
            _ => None,
        };

        if let Some(url) = check_url {
            let t = timeout;
            handles.push(tokio::spawn(
                async move { check_http(&name, &url, t).await },
            ));
        }
    }

    // Check voice services only if voice is enabled
    if state.config.voice.enabled {
        let provider = state.config.voice.tts_provider.clone();

        // Check the active TTS provider endpoint
        if state.voice_client.is_some() {
            let endpoint = state.config.voice.tts_endpoint.clone();
            let t = timeout;
            let check_name = format!("tts-{}", provider);
            // LM Studio uses /v1/models, others use root
            let health_url = if provider == "lmstudio" {
                format!("{}/v1/models", endpoint)
            } else {
                format!("{}/", endpoint)
            };
            handles.push(tokio::spawn(async move {
                check_http(&check_name, &health_url, t).await
            }));
        }

        // Check STT if stt_endpoint differs from tts_endpoint (separate service)
        let stt_endpoint = state.config.voice.stt_endpoint.clone();
        let tts_endpoint = state.config.voice.tts_endpoint.clone();
        if state.stt_client.is_some() && stt_endpoint != tts_endpoint {
            let t = timeout;
            // whisper.cpp server responds on root /, standalone whisper uses /health
            let health_url = format!("{}/", stt_endpoint);
            handles.push(tokio::spawn(async move {
                check_http("whisper-stt", &health_url, t).await
            }));
        }

        // Check Hibiki only if it's the active TTS provider
        if provider == "hibiki" {
            let hibiki_endpoint = state.config.voice.hibiki_endpoint.clone();
            let t = timeout;
            handles.push(tokio::spawn(async move {
                check_http("hibiki-tts", &format!("{}/", hibiki_endpoint), t).await
            }));
        }
    }

    // Collect all results
    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(status) => results.push(status),
            Err(e) => results.push(HealthStatus {
                name: "unknown".to_string(),
                ok: false,
                latency_ms: None,
                error: Some(format!("task panicked: {e}")),
            }),
        }
    }

    results
}

// ─── Boot Table ────────────────────────────────────────────────────────────

/// Format health check results as a pretty terminal table for boot logs.
pub fn format_boot_table(results: &[HealthStatus]) {
    info!("╔══════════════════════════════════════════╗");
    info!("║       Provider Health Status             ║");
    info!("╠══════════════════════════════════════════╣");

    for r in results {
        let icon = if r.ok { "✅" } else { "❌" };
        let latency = r.latency_ms.map(|ms| format!("{ms}ms")).unwrap_or_default();
        let detail = if let Some(err) = &r.error {
            err.clone()
        } else {
            latency
        };

        info!(
            "║  {icon} {name:<16} {detail:<20} ║",
            name = r.name,
            detail = detail,
        );
    }

    let healthy = results.iter().filter(|r| r.ok).count();
    let total = results.len();
    let overall = if healthy == total {
        "healthy"
    } else if healthy > 0 {
        "degraded"
    } else {
        "unhealthy"
    };

    info!("╠══════════════════════════════════════════╣");
    info!("║  Status: {overall:<10} ({healthy}/{total} online)        ║",);
    info!("╚══════════════════════════════════════════╝");
}

// ─── Background Periodic Checks ────────────────────────────────────────────

/// Spawn a background task that periodically runs health checks and updates
/// the shared cache. Runs every 60 seconds.
pub fn spawn_periodic_checks(state: SharedState, cache: HealthCache) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(60);
        loop {
            tokio::time::sleep(interval).await;
            let results = run_all_checks(&state).await;

            // Log any status changes
            let prev = cache.read().await;
            for r in &results {
                if let Some(old) = prev.iter().find(|o| o.name == r.name)
                    && old.ok != r.ok
                {
                    if r.ok {
                        info!(provider = %r.name, "🟢 provider recovered");
                    } else {
                        warn!(
                            provider = %r.name,
                            error = r.error.as_deref().unwrap_or("unknown"),
                            "🔴 provider went down"
                        );
                    }
                }
            }
            drop(prev);

            // Update cache
            let mut w = cache.write().await;
            *w = results;
        }
    });
}

// ─── HTTP Endpoint ─────────────────────────────────────────────────────────

/// Computes the gateway URL from the live config (`scheme://host:port`).
/// `scheme` is `https` when TLS is configured, otherwise `http`. Used in
/// `/api/health` and `/api/capabilities`.
fn gateway_url_from_config(cfg: &garraia_config::AppConfig) -> String {
    let scheme = if cfg.gateway.tls_cert_path.is_some() {
        "https"
    } else {
        "http"
    };
    format!("{}://{}:{}", scheme, cfg.gateway.host, cfg.gateway.port)
}

/// Builds the Dashboard-friendly fields shared by `/api/health` and the
/// embedded status preview. Extracts a default-provider id + model and
/// the live channel list without touching any secret.
fn extras_from_state(state: &SharedState) -> (Vec<String>, Option<String>, Option<String>) {
    // Channels — `state.channels.read()` is async, but we delegate to the
    // caller; this helper only computes provider/model. Channels handled
    // separately.
    let provider = state.agents.default_provider_id().map(|s| s.to_string());
    let model = provider
        .as_deref()
        .and_then(|p| state.agents.get_provider(p))
        .and_then(|p| p.configured_model().map(|m| m.to_string()));
    (Vec::new(), provider, model)
}

/// GET /api/health — Dashboard contract per plan 0116b §6.1 + back-compat
/// `checks` array. Always 200; degraded/unhealthy is conveyed in the
/// `status` field, never the HTTP code (the Web Console renders an amber
/// or red warning-box without losing the rest of the payload).
///
/// ```json
/// {
///   "status": "degraded",
///   "version": "0.2.1",
///   "gateway_url": "http://127.0.0.1:3888",
///   "uptime_secs": 1234,
///   "active_sessions": 2,
///   "provider": "openrouter",
///   "model": "openrouter/auto",
///   "channels": ["web", "telegram"],
///   "warnings": ["ollama: connection refused"],
///   "checks": [ {"name": "openrouter", "ok": true, "latency_ms": 231}, ... ]
/// }
/// ```
pub async fn health_handler(State(state): State<SharedState>) -> Json<HealthResponse> {
    // Provider check results: prefer cache, fall back to a live run.
    let checks = if let Some(cache) = &state.health_cache {
        let cached = cache.read().await;
        if !cached.is_empty() {
            cached.clone()
        } else {
            run_all_checks(&state).await
        }
    } else {
        run_all_checks(&state).await
    };

    let healthy = checks.iter().filter(|r| r.ok).count();
    let total = checks.len();
    let status = if total == 0 {
        // No registered checks — gateway itself is up; report healthy.
        "healthy"
    } else if healthy == total {
        "healthy"
    } else if healthy > 0 {
        "degraded"
    } else {
        "unhealthy"
    };

    let warnings: Vec<String> = checks
        .iter()
        .filter(|c| !c.ok)
        .map(|c| match &c.error {
            Some(e) => format!("{}: {}", c.name, e),
            None => format!("{}: unhealthy", c.name),
        })
        .collect();

    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let (_unused, provider, model) = extras_from_state(&state);

    Json(HealthResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION"),
        gateway_url: gateway_url_from_config(&state.config),
        uptime_secs: state.boot_time.elapsed().as_secs(),
        active_sessions: state.sessions.len(),
        provider,
        model,
        channels,
        warnings,
        checks,
    })
}

// ─── /api/capabilities ─────────────────────────────────────────────────────

/// What the Web Console + future remote clients can rely on. Each list is
/// computed live from the running gateway (`AgentRuntime`,
/// `ChannelRegistry`, `CommandRegistry`). Secret-free.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesResponse {
    /// Cargo-feature-flag-shaped capability flags.
    pub features: Vec<String>,
    /// Provider IDs currently registered (sorted, deduped).
    pub providers: Vec<String>,
    /// Per-provider configured model — emitted as `provider/model`.
    pub models: Vec<String>,
    /// Live channel registry list.
    pub channels: Vec<String>,
    /// Slash-command registry — names only, no descriptions or aliases that
    /// might leak admin paths.
    pub commands: Vec<String>,
    /// Skin / theme presets the front-end can offer.
    pub skins: Vec<String>,
    /// Forward-compat hook for `--experimental-*` flags. Empty for now.
    pub experimental_flags: Vec<String>,
    /// Gateway binary version, mirroring `/api/health`.
    pub version: &'static str,
}

/// GET /api/capabilities — read-only snapshot of what the gateway can
/// currently do. Renders the Dashboard "Arquitetura" card + drives the
/// Skins page enumeration without hardcoded JS lists.
pub async fn capabilities_handler(State(state): State<SharedState>) -> Json<CapabilitiesResponse> {
    // Static feature flags driven by Cargo cfg + runtime state shape.
    let mut features: Vec<String> = Vec::new();
    features.push("chat".into());
    features.push("websocket".into());
    features.push("multi-channel".into());
    if state.voice_client.is_some() {
        features.push("tts".into());
    }
    if state.stt_client.is_some() {
        features.push("stt".into());
    }
    if state.mcp_manager_arc.is_some() {
        features.push("mcp".into());
    }
    if state.openclaw_client.is_some() {
        features.push("openclaw".into());
    }
    if state.auth_provider.is_some() {
        features.push("auth-v1".into());
    }

    let mut providers: Vec<String> = state.agents.provider_ids().to_vec();
    providers.sort();
    providers.dedup();

    let models: Vec<String> = providers
        .iter()
        .filter_map(|pid| {
            state
                .agents
                .get_provider(pid)
                .and_then(|p| p.configured_model().map(|m| format!("{}/{}", pid, m)))
        })
        .collect();

    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let commands: Vec<String> = state
        .command_registry
        .read()
        .map(|r| r.list().into_iter().map(|(n, _d)| n.to_string()).collect())
        .unwrap_or_default();

    // Plan 0117 lists four canonical skins. The Settings Registry (PR-8)
    // can later persist user-defined skins server-side.
    let skins: Vec<String> = vec![
        "garra-blue".into(),
        "aurora-admin".into(),
        "editorial".into(),
        "cyber-garra".into(),
    ];

    Json(CapabilitiesResponse {
        features,
        providers,
        models,
        channels,
        commands,
        skins,
        experimental_flags: Vec::new(),
        version: env!("CARGO_PKG_VERSION"),
    })
}
