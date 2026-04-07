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
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    /// Overall status: "healthy", "degraded", or "unhealthy"
    pub status: String,
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
                    && old.ok != r.ok {
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

/// GET /api/health — returns JSON with all provider health statuses.
///
/// ```json
/// {
///   "status": "degraded",
///   "checks": [
///     {"name": "openrouter", "ok": true, "latency_ms": 231},
///     {"name": "ollama", "ok": false, "error": "connection refused"},
///     {"name": "chatterbox", "ok": true, "latency_ms": 42}
///   ]
/// }
/// ```
pub async fn health_handler(State(state): State<SharedState>) -> Json<HealthResponse> {
    // Try to read from cache first
    if let Some(cache) = &state.health_cache {
        let cached = cache.read().await;
        if !cached.is_empty() {
            let healthy = cached.iter().filter(|r| r.ok).count();
            let total = cached.len();
            let status = if healthy == total {
                "healthy"
            } else if healthy > 0 {
                "degraded"
            } else {
                "unhealthy"
            };
            return Json(HealthResponse {
                status: status.to_string(),
                checks: cached.clone(),
            });
        }
    }

    // No cache available — run checks now
    let results = run_all_checks(&state).await;
    let healthy = results.iter().filter(|r| r.ok).count();
    let total = results.len();
    let status = if healthy == total {
        "healthy"
    } else if healthy > 0 {
        "degraded"
    } else {
        "unhealthy"
    };

    Json(HealthResponse {
        status: status.to_string(),
        checks: results,
    })
}
