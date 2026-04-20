//! Sliding-window rate limiter for GarraIA gateway endpoints.
//!
//! Provides per-user, per-IP, and per-API-key rate limiting with configurable
//! windows. Headers returned to the client follow the IETF draft standard:
//!   X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset
//!
//! The implementation uses an in-memory sliding-window counter backed by a
//! `DashMap<String, WindowState>`. A background task periodically evicts
//! expired keys to prevent unbounded memory growth.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use dashmap::DashMap;
use garraia_auth::JwtIssuer;
use ipnet::IpNet;
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

    /// Stricter config for privileged members-management endpoints
    /// (plan 0021 / GAR-425). Covers:
    ///
    /// - `POST /v1/invites/{token}/accept` — defends against
    ///   brute-force / enumeration of invite tokens (SEC-01 from
    ///   plan 0019 security review).
    /// - `POST /v1/groups/{id}/members/{user_id}/setRole` —
    ///   defends against excessive role-change churn (plan 0020
    ///   security review).
    /// - `DELETE /v1/groups/{id}/members/{user_id}` — same.
    ///
    /// Positioned between `auth()` (10/min, very strict) and
    /// `default()` (60/min). The 20/min ceiling is conservative
    /// for legitimate UX (a user managing a group typically
    /// does 1-5 operations in a burst; 20 leaves headroom) while
    /// keeping brute-force attacks expensive (token enumeration
    /// at 20 probes/min on a 256-bit search space is infeasible).
    pub fn members_manage() -> Self {
        Self {
            requests_per_minute: 20,
            requests_per_hour: 200,
            burst_size: 5,
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
    ///
    /// Plan 0022 T5: returns `u32` via `try_from` saturated at `u32::MAX`.
    /// The cap is defensive only — `prune(3600)` bounds `timestamps.len()`
    /// to the number of requests observed in the last hour, which even
    /// at 100k req/s would saturate `Vec<u64>` memory long before
    /// overflowing `u32`. Previously cast `as u32`, which would silently
    /// truncate in the unreachable overflow case.
    fn count_in_window(&self, window_secs: u64) -> u32 {
        let cutoff = now_secs().saturating_sub(window_secs);
        let count = self.timestamps.iter().filter(|&&t| t >= cutoff).count();
        u32::try_from(count).unwrap_or(u32::MAX)
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

    /// Members-management limiter for the 3 privileged
    /// `/v1/invites/.../accept`, `/v1/groups/.../setRole`,
    /// `/v1/groups/.../members/{user_id}` routes (plan 0021).
    pub fn members_manage_limiter() -> Arc<Self> {
        Arc::new(Self::new(RateLimitConfig::members_manage()))
    }

    /// Check and record a request for `key` (IP, user_id, or API key).
    ///
    /// Returns a `RateLimitDecision` — the caller must check `allowed` and
    /// return 429 if it is `false`.
    ///
    /// ## Lock discipline (plan 0022 T4, addressing code-review HIGH of PR #30)
    ///
    /// The method is split into two phases to avoid holding a write lock on
    /// the DashMap shard for the entire window-count computation:
    ///
    /// 1. **Read phase:** probe via `get()`, clone the `WindowState`, drop
    ///    the read guard immediately. Prune + count happen on the clone —
    ///    the shard lock is not held across those loops.
    /// 2. **Write phase (only when `allowed`):** re-acquire via `entry()`
    ///    or `get_mut()` to record the new timestamp. Brief write lock.
    ///
    /// The split accepts a benign race: two callers can both observe
    /// `allowed = true` in phase 1 and both record in phase 2, producing
    /// one extra request above the nominal ceiling per race window. For
    /// the soft-limit semantics of a sliding-window rate limiter this is
    /// acceptable — the alternative (locking the shard around the full
    /// compute+write path, pre-0022 behavior) serialized all throughput
    /// to the rate of a single `count_in_window` loop even for unrelated
    /// keys sharing a DashMap shard.
    pub fn check(&self, key: &str) -> RateLimitDecision {
        let now = now_secs();

        // Phase 1: read-only snapshot. Cloning the WindowState limits the
        // read guard to a single shard-touch and lets us prune/count on
        // our local copy.
        let (per_minute, per_hour) = match self.windows.get(key) {
            Some(entry) => {
                let mut snapshot = entry.clone();
                drop(entry);
                snapshot.prune(3600);
                (snapshot.count_in_window(60), snapshot.count_in_window(3600))
            }
            None => (0, 0),
        };

        let allowed = per_minute < self.config.requests_per_minute
            && per_hour < self.config.requests_per_hour;

        // Phase 2: brief write if allowed.
        if allowed {
            let mut entry = self
                .windows
                .entry(key.to_string())
                .or_insert_with(WindowState::new);
            entry.prune(3600);
            entry.record();
        }

        let remaining = self
            .config
            .requests_per_minute
            .saturating_sub(u32::try_from(per_minute.saturating_add(1)).unwrap_or(u32::MAX));
        let reset_at = now + 60 - (now % 60);

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
///
/// Plan 0022 T5 (SEC-LOW F-06): `X-RateLimit-Reset` was removed because
/// its absolute-Unix-timestamp form leaked the exact window-reset instant,
/// which an attacker could use to synchronize brute-force bursts right
/// after each reset. `Retry-After` (set on 429 responses) is the IETF-
/// canonical signal for clients and carries the same information in a
/// relative-duration form that does not leak absolute time. The
/// `RateLimitDecision::reset_at` field is kept for internal use
/// (computing `Retry-After`) but is no longer emitted to the wire.
pub fn apply_rate_limit_headers(headers: &mut HeaderMap, decision: &RateLimitDecision) {
    if let Ok(v) = HeaderValue::from_str(&decision.limit.to_string()) {
        headers.insert("x-ratelimit-limit", v);
    }
    if let Ok(v) = HeaderValue::from_str(&decision.remaining.to_string()) {
        headers.insert("x-ratelimit-remaining", v);
    }
    // `x-ratelimit-reset` intentionally NOT emitted (plan 0022 T5).
}

/// Build a 429 Too Many Requests response with rate-limit headers.
///
/// Plan 0022 T5 (code-review NIT): headers are now applied **once** via
/// `apply_rate_limit_headers` after the body is built, instead of the
/// pre-0022 pattern that inserted `x-ratelimit-*` inline in the builder
/// AND then re-applied via the helper (resulting in inocuous but confusing
/// duplicate insertions). `Retry-After` remains in the builder because it
/// is 429-specific and not part of the generic allowed-path header set.
pub fn rate_limit_response(decision: &RateLimitDecision) -> Response {
    let retry_after = decision.reset_at.saturating_sub(now_secs());

    let mut resp = Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("content-type", "application/json")
        .header("retry-after", retry_after.to_string())
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
/// Priority: X-API-Key > Authorization bearer token-prefix >
/// X-Forwarded-For > `"ip:unknown"` sentinel.
///
/// # Known limitations (plan 0021 security review follow-ups)
///
/// - **Token-prefix, not decoded `sub` claim.** For JWTs, this uses
///   the first 8 chars of the serialized bearer token — NOT the
///   `sub` claim decoded from the payload. All HS256 JWTs emitted
///   by the same `JwtIssuer` share an identical header segment
///   (`eyJhbGci...` base64 of `{"alg":"HS256"`), so every
///   authenticated caller collides into one bucket `jwt:eyJhbGci`.
///   This is documented as a **plan 0022+ follow-up**: the
///   extractor should decode the token and key by `sub` (user_id)
///   for true per-user isolation. Until then, the jwt-keyed
///   bucket behaves as a coarse global limit for authenticated
///   traffic, which is a net-positive over no rate-limit at all
///   but insufficient for multi-tenant production load.
///
/// - **`X-Forwarded-For` is client-controlled without a trusted-proxy
///   allowlist.** Any caller can forge the header and shift their
///   bucket to an arbitrary IP, evading IP-keyed rate limits. The
///   value is used here as-is — plan 0022+ will introduce a
///   `TRUSTED_PROXIES` env var and strip the header when the
///   immediate peer is not in the list. For the current wiring
///   (rate-limit applied to authenticated endpoints only), the
///   exposure is small because the JWT bucket takes precedence
///   whenever `Authorization` is present, but the vector exists
///   for unauthenticated paths and MUST be closed before prod
///   deploy at scale.
pub fn extract_rate_limit_key(headers: &HeaderMap) -> String {
    // Use API key prefix if present
    if let Some(api_key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        let prefix = &api_key[..api_key.len().min(16)];
        return format!("apikey:{prefix}");
    }

    // Token-prefix keying — see the function docstring's "Known
    // limitations" for why this collides across HS256 JWTs.
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
    {
        let prefix = &token[..token.len().min(8)];
        return format!("jwt:{prefix}");
    }

    // WARNING: X-Forwarded-For is client-controlled here (no
    // trusted-proxy validation). Spoofable — see docstring for
    // the plan 0022 follow-up. Left in place as a best-effort
    // fallback until TRUSTED_PROXIES arrives.
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first_ip) = forwarded.split(',').next()
    {
        return format!("ip:{}", first_ip.trim());
    }

    "ip:unknown".to_string()
}

// ── Trusted-proxy handling (plan 0022 T2 / GAR-426) ───────────────────────────

/// Environment variable that configures which upstream proxies are allowed to
/// set `X-Forwarded-For` on our behalf. Comma-separated list of IPs or CIDRs.
///
/// Examples:
///   `GARRAIA_TRUSTED_PROXIES=127.0.0.1,10.0.0.0/8`
///   `GARRAIA_TRUSTED_PROXIES=::1,fc00::/7`
///
/// **Fail-closed default:** when the variable is unset OR empty, no proxy is
/// trusted and `X-Forwarded-For` is ignored entirely. The `peer_addr` (from the
/// TCP socket) becomes the sole client identifier. This is safer than the
/// pre-0022 behavior (header accepted unconditionally), at the cost of
/// requiring deployments behind real proxies to set the var explicitly.
pub const TRUSTED_PROXIES_ENV: &str = "GARRAIA_TRUSTED_PROXIES";

/// Parse `GARRAIA_TRUSTED_PROXIES` into a `Vec<IpNet>`. Bare IPs are promoted
/// to `/32` (IPv4) or `/128` (IPv6) CIDRs automatically.
///
/// Malformed entries are skipped with a tracing warning — the goal is best-
/// effort (a typo in one entry should not disable trust for the others) while
/// preserving the fail-closed semantics (empty / all-invalid ⇒ empty Vec ⇒
/// XFF ignored).
pub fn parse_trusted_proxies(value: &str) -> Vec<IpNet> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| match s.parse::<IpNet>() {
            Ok(net) => Some(net),
            Err(_) => match s.parse::<IpAddr>() {
                Ok(ip) => Some(IpNet::from(ip)),
                Err(e) => {
                    warn!(
                        entry = s,
                        error = %e,
                        "ignoring malformed entry in {TRUSTED_PROXIES_ENV}"
                    );
                    None
                }
            },
        })
        .collect()
}

/// Derive the real client IP, honoring `X-Forwarded-For` **only** when the
/// immediate peer is in the `trusted_proxies` allowlist.
///
/// Rules (plan 0022 T2 / GAR-426):
/// 1. If `trusted_proxies` is empty (env unset or all-invalid) ⇒ return
///    `peer_addr` unconditionally. XFF is ignored (fail-closed).
/// 2. If `peer_addr` is NOT in any entry of `trusted_proxies` ⇒ return
///    `peer_addr`. The peer is not an authorized proxy, so its XFF header is
///    untrusted.
/// 3. If `peer_addr` IS in `trusted_proxies` ⇒ try to parse the first IP from
///    `X-Forwarded-For`. If present and valid ⇒ return that. If absent or
///    malformed ⇒ fall back to `peer_addr`.
///
/// **Note:** this helper is currently used only by the rate-limiter's fallback
/// key-extractor branch (unauthenticated / no-JWT requests). The other two
/// gateway sites that read `X-Forwarded-For` (`api.rs:71` session token IP
/// stamp, `admin/middleware.rs:168` admin extract_ip) are intentionally out
/// of scope for plan 0022 — see GAR-426 description. A later plan (0023+)
/// should consolidate all three through this helper.
pub fn real_client_ip(
    headers: &HeaderMap,
    peer_addr: IpAddr,
    trusted_proxies: &[IpNet],
) -> IpAddr {
    if trusted_proxies.is_empty() {
        return peer_addr;
    }
    if !trusted_proxies.iter().any(|net| net.contains(&peer_addr)) {
        return peer_addr;
    }
    // Peer is a trusted proxy — honor XFF.
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok())
        .unwrap_or(peer_addr)
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

// ── Plan 0022 T3 (GAR-426): per-user authenticated rate-limit ────────────────

/// State for [`rate_limit_layer_authenticated`] — combines a shared
/// `RateLimiter` with a `JwtIssuer` for per-user keying and a
/// pre-parsed `trusted_proxies` list for the unauthenticated fallback.
///
/// The trio is cloned into middleware scope via axum's
/// `from_fn_with_state`. All fields are `Arc<T>` / `Vec<IpNet>` (cheap
/// clone) so per-request copy cost is ~pointer-sized.
#[derive(Clone)]
pub struct RateLimitLayerState {
    pub limiter: Arc<RateLimiter>,
    pub jwt_issuer: Arc<JwtIssuer>,
    pub trusted_proxies: Vec<IpNet>,
}

impl RateLimitLayerState {
    pub fn new(
        limiter: Arc<RateLimiter>,
        jwt_issuer: Arc<JwtIssuer>,
        trusted_proxies: Vec<IpNet>,
    ) -> Self {
        Self {
            limiter,
            jwt_issuer,
            trusted_proxies,
        }
    }
}

/// Derive a per-user rate-limit key from a verified JWT's `sub` claim.
///
/// Plan 0022 T3: fixes the shared-bucket problem documented as SEC-HIGH
/// F-03 in the PR #30 security audit. The pre-0022 `extract_rate_limit_key`
/// used the first 8 chars of the Bearer token as the bucket key; all
/// HS256 JWTs issued by the same `JwtIssuer` share the same header
/// segment (`eyJhbGci...`), so every authenticated caller collided on
/// one bucket. This function decodes + verifies the HMAC and returns
/// `jwt-sub:{uuid}` — a real per-user key.
///
/// Returns `None` when the `Authorization` header is absent, malformed,
/// or the signature / expiration check fails. Callers decide the fallback
/// (typically IP-keyed via `real_client_ip`).
///
/// **Double-verify trade-off (v1 accepted):** this function calls
/// `JwtIssuer::verify_access` in the rate-limit middleware, and the
/// `Principal` extractor downstream calls it again to materialize the
/// handler's `Principal`. HMAC-SHA256 on a compact JWT is ~µs, negligible
/// vs the handler itself (~ms for a DB tx + audit INSERT), so the
/// duplicate cost is acceptable in v1. Optimization path — stash the
/// decoded claims in `Request::extensions()` so the extractor can reuse
/// them — is deferred to plan 0023+ if p95 monitoring shows >5% regression.
pub fn extract_authenticated_key(headers: &HeaderMap, jwt: &JwtIssuer) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = jwt.verify_access(token).ok()?;
    Some(format!("jwt-sub:{}", claims.sub))
}

/// Extract the best client identifier for an **authenticated** rate-limit
/// bucket, with a trusted-proxy-aware IP fallback.
///
/// Priority:
/// 1. Verified JWT `sub` claim ⇒ `jwt-sub:{uuid}` (real per-user bucket).
/// 2. API key prefix ⇒ `apikey:{prefix}` (same as the unauthenticated
///    extractor; preserved so machine-to-machine callers are not demoted
///    to an IP bucket when they happen to miss a bearer).
/// 3. Real client IP via [`real_client_ip`] ⇒ `ip:{addr}`.
/// 4. Sentinel `ip:unknown`.
fn extract_authenticated_rate_limit_key(
    headers: &HeaderMap,
    jwt: &JwtIssuer,
    peer_addr: Option<std::net::IpAddr>,
    trusted_proxies: &[IpNet],
) -> String {
    if let Some(key) = extract_authenticated_key(headers, jwt) {
        return key;
    }
    if let Some(api_key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        let prefix = &api_key[..api_key.len().min(16)];
        return format!("apikey:{prefix}");
    }
    if let Some(peer) = peer_addr {
        let real = real_client_ip(headers, peer, trusted_proxies);
        return format!("ip:{real}");
    }
    "ip:unknown".to_string()
}

/// Axum middleware layer for **authenticated** rate limiting.
///
/// Uses [`extract_authenticated_rate_limit_key`] so each JWT-authenticated
/// caller gets an isolated bucket keyed by the verified `sub` claim.
/// Unauthenticated callers fall through to API-key prefix or IP keying
/// (see function docstring for the priority order).
///
/// Wire with `axum::middleware::from_fn_with_state`:
///
/// ```ignore
/// let state = RateLimitLayerState::new(limiter, jwt, trusted);
/// Router::new()
///     .route("/v1/...", post(handler))
///     .layer(axum::middleware::from_fn_with_state(
///         state,
///         rate_limit_layer_authenticated,
///     ))
/// ```
pub async fn rate_limit_layer_authenticated(
    axum::extract::State(state): axum::extract::State<RateLimitLayerState>,
    req: Request,
    next: Next,
) -> Response {
    // Pull ConnectInfo + headers from the Request instead of listing them
    // as separate middleware extractors. axum's `from_fn_with_state`
    // caps the total param count including State/Request/Next, and
    // splitting headers/peer_info into dedicated params hit that cap.
    let peer_addr = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ConnectInfo(sa)| sa.ip());
    let key = extract_authenticated_rate_limit_key(
        req.headers(),
        &state.jwt_issuer,
        peer_addr,
        &state.trusted_proxies,
    );
    let decision = state.limiter.check(&key);

    if !decision.allowed {
        warn!("rate limit exceeded for key={}", &key[..key.len().min(32)]);
        return rate_limit_response(&decision);
    }

    let mut response = next.run(req).await;
    apply_rate_limit_headers(response.headers_mut(), &decision);
    response
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

    // ── Plan 0022 T2: TRUSTED_PROXIES + real_client_ip tests ────────────────

    fn peer_v4() -> IpAddr {
        "10.0.0.5".parse().unwrap()
    }

    fn peer_v6() -> IpAddr {
        "fc00::1".parse().unwrap()
    }

    fn xff_header(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn trusted_proxies_empty_env_ignores_xff() {
        // Fail-closed: when GARRAIA_TRUSTED_PROXIES is unset or empty, no
        // proxy is trusted and X-Forwarded-For is ignored. `peer_addr`
        // becomes the sole source of truth.
        let trusted = parse_trusted_proxies("");
        assert!(trusted.is_empty(), "empty env must produce empty Vec");

        let headers = xff_header("203.0.113.99"); // forged upstream IP
        let resolved = real_client_ip(&headers, peer_v4(), &trusted);
        assert_eq!(
            resolved,
            peer_v4(),
            "empty trusted list ⇒ XFF is ignored, peer_addr wins"
        );
    }

    #[test]
    fn trusted_proxies_peer_in_allowlist_accepts_xff() {
        // Peer is a listed proxy ⇒ XFF is honored. Exact-IP form `/32`
        // and CIDR form `10.0.0.0/8` both match `peer_v4() = 10.0.0.5`.
        let trusted = parse_trusted_proxies("10.0.0.0/8, ::1");
        assert_eq!(trusted.len(), 2);

        let headers = xff_header("203.0.113.99");
        let resolved = real_client_ip(&headers, peer_v4(), &trusted);
        assert_eq!(
            resolved.to_string(),
            "203.0.113.99",
            "trusted proxy peer ⇒ XFF.first_hop wins"
        );
    }

    #[test]
    fn trusted_proxies_peer_outside_allowlist_ignores_xff() {
        // Only a different CIDR listed; peer_v4 = 10.0.0.5 is NOT inside it.
        // ⇒ peer is not a trusted proxy ⇒ XFF ignored, peer_addr returned.
        let trusted = parse_trusted_proxies("172.16.0.0/12");
        assert_eq!(trusted.len(), 1);

        let headers = xff_header("203.0.113.99");
        let resolved = real_client_ip(&headers, peer_v4(), &trusted);
        assert_eq!(
            resolved,
            peer_v4(),
            "untrusted peer ⇒ XFF ignored, peer_addr wins (spoofing defense)"
        );
    }

    #[test]
    fn trusted_proxies_cidr_ipv4_and_ipv6_parse() {
        // Exercises (a) bare IPv4 → /32 promotion, (b) IPv4 CIDR,
        // (c) bare IPv6 → /128 promotion, (d) IPv6 CIDR,
        // (e) malformed entry skipped silently.
        let trusted = parse_trusted_proxies(
            "127.0.0.1, 10.0.0.0/8, ::1, fc00::/7, not-an-ip, , 999.999.999.999",
        );
        assert_eq!(
            trusted.len(),
            4,
            "exactly 4 valid CIDRs (bare IPv4+IPv6 + 2 CIDRs); malformed/empty dropped. got: {trusted:?}"
        );

        // IPv6 peer inside fc00::/7.
        let headers = xff_header("2001:db8::1");
        let resolved = real_client_ip(&headers, peer_v6(), &trusted);
        assert_eq!(
            resolved.to_string(),
            "2001:db8::1",
            "IPv6 peer inside fc00::/7 ⇒ XFF honored"
        );
    }

    #[test]
    fn trusted_proxies_malformed_xff_falls_back_to_peer() {
        // Peer IS trusted, but XFF body is unparseable ⇒ fallback to peer.
        // Guards against a trusted proxy sending garbage XFF.
        let trusted = parse_trusted_proxies("10.0.0.0/8");
        let headers = xff_header("not-an-ip");
        let resolved = real_client_ip(&headers, peer_v4(), &trusted);
        assert_eq!(
            resolved,
            peer_v4(),
            "malformed XFF ⇒ fallback to peer_addr"
        );
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
        // Plan 0022 T5: X-RateLimit-Reset intentionally NOT emitted
        // (timing leak → brute-force planning). Retry-After on 429
        // responses is the canonical signal.
        assert!(
            !headers.contains_key("x-ratelimit-reset"),
            "X-RateLimit-Reset must not be emitted (plan 0022 T5)"
        );
    }

    #[test]
    fn rate_limit_response_has_retry_after_but_not_reset() {
        // Plan 0022 T5 regression guard: 429 responses carry
        // Retry-After (IETF canonical), Content-Type, and the
        // X-RateLimit-Limit/Remaining pair — but NOT X-RateLimit-Reset.
        let limiter = strict_limiter(1);
        limiter.check("x"); // consume
        let decision = limiter.check("x");
        assert!(!decision.allowed, "second call must hit the limit");

        let resp = rate_limit_response(&decision);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = resp.headers();
        assert!(headers.contains_key("retry-after"), "Retry-After missing");
        assert!(headers.contains_key("x-ratelimit-limit"));
        assert!(headers.contains_key("x-ratelimit-remaining"));
        assert!(
            !headers.contains_key("x-ratelimit-reset"),
            "X-RateLimit-Reset leaked into 429 response"
        );
        // Content-Type JSON so clients parse the body error envelope.
        assert_eq!(
            headers.get("content-type").and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
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
