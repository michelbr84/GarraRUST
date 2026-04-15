//! Sliding-window rate limiter for GarraIA gateway endpoints.
//!
//! Provides per-user, per-IP, and per-API-key rate limiting with configurable
//! windows. Headers returned to the client follow the IETF draft standard:
//!   X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset
//!
//! The implementation uses an in-memory sliding-window counter backed by a
//! `DashMap<String, WindowState>`. A background task periodically evicts
//! expired keys to prevent unbounded memory growth.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::warn;

// ── Config ────────────────────────────────────────────────────────────────────

/// Rate-limiter configuration per route group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests allowed in the per-minute window.
    pub requests_per_minute: u32,
    /// Maximum requests allowed in the per-hour window.
    pub requests_per_hour: u32,
    /// Burst allowance (requests that can be made before the per-second
    /// governor kicks in — passed to `tower_governor`).
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            requests_per_hour: 1000,
            burst_size: 10,
        }
    }
}

impl RateLimitConfig {
    /// Strict config for authentication endpoints.
    pub fn auth() -> Self {
        Self {
            requests_per_minute: 10,
            requests_per_hour: 50,
            burst_size: 3,
        }
    }

    /// Relaxed config for read-only API endpoints.
    pub fn read_only() -> Self {
        Self {
            requests_per_minute: 120,
            requests_per_hour: 2000,
            burst_size: 20,
        }
    }
}

// ── Internal window state ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct WindowState {
    /// Ring-buffer of request timestamps (unix seconds) within the hour window.
    timestamps: Vec<u64>,
    /// Last time this key was touched (unix seconds) — used for eviction.
    last_seen: u64,
}

impl WindowState {
    fn new() -> Self {
        Self {
            timestamps: Vec::with_capacity(16),
            last_seen: now_secs(),
        }
    }

    /// Remove timestamps older than `horizon` seconds from now.
    fn prune(&mut self, horizon_secs: u64) {
        let cutoff = now_secs().saturating_sub(horizon_secs);
        self.timestamps.retain(|&t| t >= cutoff);
    }

    /// Count requests in the last `window_secs` seconds.
    fn count_in_window(&self, window_secs: u64) -> u32 {
        let cutoff = now_secs().saturating_sub(window_secs);
        self.timestamps.iter().filter(|&&t| t >= cutoff).count() as u32
    }

    /// Record a new request timestamp.
    fn record(&mut self) {
        let ts = now_secs();
        self.timestamps.push(ts);
        self.last_seen = ts;
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

// ── Rate limiter ──────────────────────────────────────────────────────────────

/// A sliding-window rate limiter keyed by an arbitrary string identifier.
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    windows: Arc<DashMap<String, WindowState>>,
}

/// Result of a rate-limit check.
#[derive(Debug, Clone)]
pub struct RateLimitDecision {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// The effective per-minute limit.
    pub limit: u32,
    /// Remaining requests in the per-minute window.
    pub remaining: u32,
    /// Unix timestamp when the current per-minute window resets.
    pub reset_at: u64,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            windows: Arc::new(DashMap::new()),
        }
    }

    /// Default limiter with standard config.
    pub fn default_limiter() -> Arc<Self> {
        Arc::new(Self::new(RateLimitConfig::default()))
    }

    /// Auth-specific strict limiter.
    pub fn auth_limiter() -> Arc<Self> {
        Arc::new(Self::new(RateLimitConfig::auth()))
    }

    /// Check and record a request for `key` (IP, user_id, or API key).
    ///
    /// Returns a `RateLimitDecision` — the caller must check `allowed` and
    /// return 429 if it is `false`.
    pub fn check(&self, key: &str) -> RateLimitDecision {
        let mut entry = self
            .windows
            .entry(key.to_string())
            .or_insert_with(WindowState::new);
        entry.prune(3600); // keep at most 1 hour of history

        let per_minute = entry.count_in_window(60);
        let per_hour = entry.count_in_window(3600);

        let allowed = per_minute < self.config.requests_per_minute
            && per_hour < self.config.requests_per_hour;

        if allowed {
            entry.record();
        }

        let remaining = self
            .config
            .requests_per_minute
            .saturating_sub(per_minute + 1);
        let reset_at = now_secs() + 60 - (now_secs() % 60);

        RateLimitDecision {
            allowed,
            limit: self.config.requests_per_minute,
            remaining,
            reset_at,
        }
    }

    /// Background eviction: remove keys inactive for more than 2 hours.
    pub fn evict_stale(&self) {
        let cutoff = now_secs().saturating_sub(7200);
        let stale_keys: Vec<String> = self
            .windows
            .iter()
            .filter(|e| e.value().last_seen < cutoff)
            .map(|e| e.key().clone())
            .collect();
        let count = stale_keys.len();
        for key in stale_keys {
            self.windows.remove(&key);
        }
        if count > 0 {
            tracing::debug!("rate_limiter: evicted {count} stale keys");
        }
    }

    /// Spawn a background Tokio task that evicts stale keys every `interval`.
    pub fn spawn_cleanup(self: Arc<Self>, interval: Duration) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                self.evict_stale();
            }
        });
    }

    /// Active key count (for metrics).
    pub fn active_keys(&self) -> usize {
        self.windows.len()
    }
}

// ── Rate-limit headers ────────────────────────────────────────────────────────

/// Append standard X-RateLimit-* headers to a response.
pub fn apply_rate_limit_headers(headers: &mut HeaderMap, decision: &RateLimitDecision) {
    if let Ok(v) = HeaderValue::from_str(&decision.limit.to_string()) {
        headers.insert("x-ratelimit-limit", v);
    }
    if let Ok(v) = HeaderValue::from_str(&decision.remaining.to_string()) {
        headers.insert("x-ratelimit-remaining", v);
    }
    if let Ok(v) = HeaderValue::from_str(&decision.reset_at.to_string()) {
        headers.insert("x-ratelimit-reset", v);
    }
}

/// Build a 429 Too Many Requests response with rate-limit headers.
pub fn rate_limit_response(decision: &RateLimitDecision) -> Response {
    let mut resp = Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("content-type", "application/json")
        .header("x-ratelimit-limit", decision.limit.to_string())
        .header("x-ratelimit-remaining", "0")
        .header("x-ratelimit-reset", decision.reset_at.to_string())
        .header(
            "retry-after",
            (decision.reset_at.saturating_sub(now_secs())).to_string(),
        )
        .body(Body::from(r#"{"error":"rate limit exceeded","code":429}"#))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .body(Body::empty())
                .expect("empty body is always valid")
        });

    apply_rate_limit_headers(resp.headers_mut(), decision);
    resp
}

// ── Axum middleware factory ───────────────────────────────────────────────────

/// Extract the best available client identifier from a request.
///
/// Priority: Authorization bearer sub-prefix > X-API-Key > X-Forwarded-For > remote IP.
pub fn extract_rate_limit_key(headers: &HeaderMap) -> String {
    // Use API key prefix if present
    if let Some(api_key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        let prefix = &api_key[..api_key.len().min(16)];
        return format!("apikey:{prefix}");
    }

    // Use first 8 chars of JWT subject as key (if present) — never the full token
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
    {
        // Use token prefix only — enough to identify the key space
        let prefix = &token[..token.len().min(8)];
        return format!("jwt:{prefix}");
    }

    // Fall back to X-Forwarded-For
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first_ip) = forwarded.split(',').next()
    {
        return format!("ip:{}", first_ip.trim());
    }

    "ip:unknown".to_string()
}

/// Axum middleware that applies a shared `RateLimiter` to every request.
///
/// On limit exceeded: returns 429 with X-RateLimit-* headers.
/// On allowed: forwards the request and appends headers to the response.
pub async fn rate_limit_middleware(
    limiter: Arc<RateLimiter>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let key = extract_rate_limit_key(&headers);
    let decision = limiter.check(&key);

    if !decision.allowed {
        warn!("rate limit exceeded for key={}", &key[..key.len().min(20)]);
        return rate_limit_response(&decision);
    }

    let mut response = next.run(req).await;
    apply_rate_limit_headers(response.headers_mut(), &decision);
    response
}

/// Convenience: build an Axum middleware layer using a cloned `Arc<RateLimiter>`.
///
/// Usage:
/// ```ignore
/// let limiter = RateLimiter::auth_limiter();
/// Router::new()
///     .route("/auth/login", post(login_handler))
///     .layer(axum::middleware::from_fn_with_state(limiter, rate_limit_layer))
/// ```
pub async fn rate_limit_layer(
    axum::extract::State(limiter): axum::extract::State<Arc<RateLimiter>>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    rate_limit_middleware(limiter, headers, req, next).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn strict_limiter(rpm: u32) -> RateLimiter {
        RateLimiter::new(RateLimitConfig {
            requests_per_minute: rpm,
            requests_per_hour: 1000,
            burst_size: 1,
        })
    }

    #[test]
    fn allows_requests_under_limit() {
        let limiter = strict_limiter(5);
        for i in 0..5 {
            let decision = limiter.check("test-key");
            assert!(decision.allowed, "request {i} should be allowed");
        }
    }

    #[test]
    fn blocks_requests_over_limit() {
        let limiter = strict_limiter(3);
        for _ in 0..3 {
            limiter.check("blocked-key");
        }
        let blocked = limiter.check("blocked-key");
        assert!(!blocked.allowed, "4th request should be blocked");
        assert_eq!(blocked.remaining, 0);
    }

    #[test]
    fn different_keys_are_independent() {
        let limiter = strict_limiter(2);
        limiter.check("key-a");
        limiter.check("key-a");
        let blocked = limiter.check("key-a");
        assert!(!blocked.allowed);

        // key-b should still be allowed
        let ok = limiter.check("key-b");
        assert!(ok.allowed);
    }

    #[test]
    fn headers_populated() {
        let limiter = strict_limiter(10);
        let decision = limiter.check("header-test");
        let mut headers = HeaderMap::new();
        apply_rate_limit_headers(&mut headers, &decision);

        assert!(headers.contains_key("x-ratelimit-limit"));
        assert!(headers.contains_key("x-ratelimit-remaining"));
        assert!(headers.contains_key("x-ratelimit-reset"));
    }

    #[test]
    fn extract_key_from_api_key_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("sk-test-1234567890"));
        let key = extract_rate_limit_key(&headers);
        assert!(key.starts_with("apikey:"));
    }

    #[test]
    fn extract_key_from_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
        );
        let key = extract_rate_limit_key(&headers);
        assert!(key.starts_with("jwt:"));
    }

    #[test]
    fn extract_key_fallback_to_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.1, 10.0.0.1"),
        );
        let key = extract_rate_limit_key(&headers);
        assert_eq!(key, "ip:203.0.113.1");
    }

    #[test]
    fn evict_stale_clears_inactive_keys() {
        let limiter = strict_limiter(100);
        limiter.check("evict-me");
        assert_eq!(limiter.active_keys(), 1);

        // Force last_seen to ancient past
        if let Some(mut entry) = limiter.windows.get_mut("evict-me") {
            entry.last_seen = 0;
        }

        limiter.evict_stale();
        assert_eq!(limiter.active_keys(), 0);
    }
}
