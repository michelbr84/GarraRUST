//! Authentication middleware for the `/metrics` endpoint (plan 0024 / GAR-412).
//!
//! Guards the two `/metrics` surfaces that exist today:
//!
//! 1. **Dedicated listener** spawned via
//!    [`crate::metrics_exporter::spawn_dedicated_metrics_listener`] when
//!    `GARRAIA_METRICS_ENABLED=true`. That path enforces fail-closed at
//!    startup — the listener simply does not come up when `bind` is
//!    non-loopback and no auth is configured.
//! 2. **Embedded route** on the main gateway listener (registered by
//!    `crate::router::build_router`). That path relies on this
//!    middleware at runtime: main listener always binds, but requests
//!    without credentials receive `503` (no auth configured), `401`
//!    (bad token), `403` (peer not in allowlist), or `200` (authorized).
//!
//! ## Design invariants
//!
//! - Dev ergonomics preserved — loopback peer + no token + empty
//!   allowlist ⇒ `200` without friction.
//! - Token comparison is constant-time via [`subtle::ConstantTimeEq`].
//! - CIDR parsing is delegated to
//!   [`crate::rate_limiter::parse_trusted_proxies`] so the two trust
//!   lists stay in sync.
//! - Sensitive data (`token`) is redacted from all error responses and
//!   tracing fields.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;
use ring::digest::{SHA256, digest};
use subtle::ConstantTimeEq;
use tracing::warn;

use crate::rate_limiter::parse_trusted_proxies;

/// Runtime configuration consumed by [`metrics_auth_layer`].
///
/// Built once at bootstrap from [`garraia_telemetry::TelemetryConfig`]
/// via [`MetricsAuthConfig::from_telemetry_raw`] and shared across both
/// `/metrics` surfaces. `Clone` is cheap — `Arc<str>` for the token and
/// `Arc<[IpNet]>` for the allowlist — so it is safe to clone into the
/// Axum `State` of each route's `from_fn_with_state`.
#[derive(Debug, Clone, Default)]
pub struct MetricsAuthConfig {
    token: Option<Arc<str>>,
    allowlist: Arc<[IpNet]>,
}

impl MetricsAuthConfig {
    /// Build an auth config from the raw `TelemetryConfig` fields.
    ///
    /// Malformed CIDR entries are logged and dropped by
    /// `parse_trusted_proxies`, mirroring the behavior of the
    /// `GARRAIA_TRUSTED_PROXIES` env var.
    pub fn from_telemetry_raw(token: Option<&str>, allowlist: &[String]) -> Self {
        let token = token.and_then(|t| {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Arc::<str>::from(trimmed))
            }
        });
        let joined = allowlist.join(",");
        let parsed = if joined.is_empty() {
            Vec::new()
        } else {
            parse_trusted_proxies(&joined)
        };
        Self {
            token,
            allowlist: Arc::<[IpNet]>::from(parsed),
        }
    }

    /// `true` if no token and no allowlist is configured — middleware
    /// falls back to the loopback-only dev path.
    pub fn is_unauthenticated(&self) -> bool {
        self.token.is_none() && self.allowlist.is_empty()
    }

    /// Short stable label for startup logs. Never includes the token.
    pub fn describe_mode(&self) -> &'static str {
        match (self.token.is_some(), !self.allowlist.is_empty()) {
            (false, false) => "loopback-only",
            (true, false) => "token",
            (false, true) => "allowlist",
            (true, true) => "token+allowlist",
        }
    }
}

// ── Middleware ────────────────────────────────────────────────────────────────

/// Axum middleware that authenticates `/metrics` requests.
///
/// Outcomes (short-circuited before `next.run`):
///
/// - **200** (delegates to `next`) — request is authorized (loopback
///   fallback, allowlist match, or valid token).
/// - **401** — token is configured and the request is missing it or
///   presents a bad value.
/// - **403** — allowlist is configured and the peer IP is not in it.
/// - **503** — peer is non-loopback and neither token nor allowlist is
///   configured (safety net — the startup fail-closed check should
///   have already refused to bind this listener).
pub async fn metrics_auth_layer(
    State(cfg): State<MetricsAuthConfig>,
    req: Request,
    next: Next,
) -> Response {
    // Pull `ConnectInfo` and `HeaderMap` from the request extensions/headers
    // rather than listing them as separate middleware extractors. Axum 0.8's
    // `from_fn_with_state` `FromFn<F, S, I, T>` ladder only implements
    // `Service` for tuples up to 16 `FromRequestParts` extractors, but the
    // trait-solver in practice trips up earlier when the tuple mixes
    // `Option<ConnectInfo>` + concrete types. Copying the pattern from
    // `rate_limit_layer_authenticated` keeps us inside the happy path.
    let peer_ip: Option<IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(sa)| sa.ip());
    let is_loopback = peer_ip.is_some_and(|ip| ip.is_loopback());
    let headers = req.headers().clone();

    // (a) Dev ergonomics: loopback peer AND no auth configured ⇒ allow.
    if is_loopback && cfg.is_unauthenticated() {
        return next.run(req).await;
    }

    // (b) Allowlist (when configured): peer must match or we 403.
    if !cfg.allowlist.is_empty() {
        // Code-review MEDIUM: if `ConnectInfo` was not populated (e.g. the
        // middleware is reused over a Unix socket or a synthetic test
        // request without the extension), `peer_ip` is `None`. Returning
        // 403 would be misleading — we can't evaluate the allowlist. Log
        // + 503 so operators can distinguish "wrong peer" from "peer
        // unavailable". Both `/metrics` surfaces today are wired with
        // `into_make_service_with_connect_info`, so this path is
        // defensive.
        let Some(ip) = peer_ip else {
            warn!("metrics_auth_layer: peer address unavailable (no ConnectInfo)");
            return deny(
                StatusCode::SERVICE_UNAVAILABLE,
                "metrics: peer address unavailable",
            );
        };
        if !cfg.allowlist.iter().any(|net| net.contains(&ip)) {
            return deny(StatusCode::FORBIDDEN, "metrics: peer not allowed");
        }
    }

    // (c) Token (when configured): Authorization: Bearer <token> required.
    if let Some(expected) = cfg.token.as_deref() {
        match extract_bearer(&headers) {
            Some(got) if constant_time_token_eq(got.as_bytes(), expected.as_bytes()) => {
                // authorized, fall through
            }
            _ => return deny_unauthorized("metrics: invalid token"),
        }
    } else if cfg.allowlist.is_empty() && !is_loopback {
        // Safety net — the dedicated listener's startup check should
        // have refused to bind. The embedded route on the main listener
        // falls back here for non-loopback peers with no auth at all.
        return deny(
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics: auth not configured",
        );
    }

    next.run(req).await
}

fn deny(status: StatusCode, body: &'static str) -> Response {
    (status, body).into_response()
}

/// RFC 7235 §4.1 requires `WWW-Authenticate` on 401 so clients (and
/// Prometheus scrapers) know to retry with a Bearer token. The header
/// value is a constant — no secret leaks into the response.
fn deny_unauthorized(body: &'static str) -> Response {
    let mut resp = (StatusCode::UNAUTHORIZED, body).into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(r#"Bearer realm="metrics""#),
    );
    resp
}

/// Compare two byte slices for equality in a way that is constant-time
/// with respect to **both** the content and the input lengths.
///
/// `subtle::ConstantTimeEq::ct_eq` on raw `[u8]` returns `Choice::zero()`
/// immediately when the slices have different lengths — that early-exit
/// exposes a length oracle to callers who can measure response timing
/// (security audit M-1, plan 0024 review). Hashing both inputs to a
/// fixed-size SHA-256 digest and `ct_eq`'ing the digests removes the
/// length dependency entirely; the cost (two SHA-256 of <64 bytes) is
/// in the noise compared to the HTTP round trip.
fn constant_time_token_eq(a: &[u8], b: &[u8]) -> bool {
    let a_hash = digest(&SHA256, a);
    let b_hash = digest(&SHA256, b);
    a_hash.as_ref().ct_eq(b_hash.as_ref()).unwrap_u8() == 1
}

/// Extract a bearer token from an `Authorization` header. Case-sensitive
/// match on the scheme — Prometheus scrapers always spell it `Bearer`.
fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value.strip_prefix("Bearer ")?;
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use std::net::{IpAddr, Ipv4Addr};
    use tower::ServiceExt;

    async fn ok_handler() -> &'static str {
        "metrics-body"
    }

    fn router(cfg: MetricsAuthConfig) -> Router {
        Router::new()
            .route("/metrics", get(ok_handler))
            .layer(from_fn_with_state(cfg, metrics_auth_layer))
    }

    /// Attach a synthetic `ConnectInfo<SocketAddr>` to a test request so
    /// the middleware can see a peer IP without real network IO.
    fn request_with_peer(peer: IpAddr, auth: Option<&str>) -> HttpRequest<Body> {
        let sa: SocketAddr = (peer, 55555).into();
        let mut builder = HttpRequest::builder().uri("/metrics").method("GET");
        if let Some(token) = auth {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo(sa));
        req
    }

    async fn oneshot_status(router: Router, req: HttpRequest<Body>) -> (StatusCode, String) {
        let resp = router.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn loopback_without_auth_is_allowed() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(None, &[]);
        assert_eq!(cfg.describe_mode(), "loopback-only");
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::LOCALHOST), None);
        let (status, body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "metrics-body");
    }

    #[tokio::test]
    async fn non_loopback_without_auth_returns_503() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(None, &[]);
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), None);
        let (status, body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("auth not configured"));
    }

    #[tokio::test]
    async fn allowlist_match_is_allowed() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(None, &["10.0.0.0/8".to_string()]);
        assert_eq!(cfg.describe_mode(), "allowlist");
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), None);
        let (status, _body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn allowlist_miss_returns_403() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(None, &["10.0.0.0/8".to_string()]);
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), None);
        let (status, body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body.contains("not allowed"));
    }

    #[tokio::test]
    async fn token_match_is_allowed() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
        assert_eq!(cfg.describe_mode(), "token");
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), Some("dev-token"));
        let (status, _body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn token_mismatch_returns_401() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), Some("WRONG"));
        let (status, body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("invalid token"));
    }

    #[tokio::test]
    async fn unauthorized_response_carries_www_authenticate_header() {
        // Security audit M-2: 401 must carry `WWW-Authenticate` (RFC 7235 §4.1).
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), Some("WRONG"));
        let resp = router(cfg).oneshot(req).await.expect("oneshot");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let wa = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .expect("WWW-Authenticate must be set on 401");
        assert_eq!(wa.to_str().unwrap(), r#"Bearer realm="metrics""#);
    }

    #[tokio::test]
    async fn different_length_token_is_still_rejected() {
        // Security audit M-1 regression guard: tokens of different lengths
        // must be rejected without panicking or early-returning. The
        // hash-based compare in `constant_time_token_eq` handles any
        // length pair safely.
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("short"), &[]);
        let req = request_with_peer(
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
            Some("a-much-longer-token-that-would-panic-on-naive-ct_eq"),
        );
        let (status, _) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn allowlist_configured_but_peer_missing_returns_503() {
        // Code-review MEDIUM: when ConnectInfo is absent, we cannot
        // evaluate the allowlist — return 503 with a clear body instead
        // of 403 ("peer not allowed"), which would mislead the operator.
        let cfg = MetricsAuthConfig::from_telemetry_raw(None, &["10.0.0.0/8".to_string()]);
        // Do NOT insert ConnectInfo → peer_ip = None inside the middleware.
        let req = HttpRequest::builder()
            .uri("/metrics")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let (status, body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            body.contains("peer address unavailable"),
            "expected clear error body, got: {body}"
        );
    }

    #[test]
    fn constant_time_token_eq_handles_any_length() {
        // Sanity: equal bytes return true; unequal return false. Any
        // length combination is safe (no panic, no early-exit).
        assert!(constant_time_token_eq(b"abc", b"abc"));
        assert!(!constant_time_token_eq(b"abc", b"abd"));
        assert!(!constant_time_token_eq(b"a", b"aaaaaaaaaaaaaaa"));
        assert!(!constant_time_token_eq(b"", b"nonempty"));
        assert!(constant_time_token_eq(b"", b""));
    }

    #[tokio::test]
    async fn missing_bearer_returns_401_when_token_configured() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), None);
        let (status, _body) = oneshot_status(router(cfg), req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn token_plus_allowlist_requires_both() {
        let cfg =
            MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &["10.0.0.0/8".to_string()]);
        assert_eq!(cfg.describe_mode(), "token+allowlist");
        // allowlist match + correct token ⇒ 200.
        let req_ok = request_with_peer(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), Some("dev-token"));
        let (status, _body) = oneshot_status(router(cfg.clone()), req_ok).await;
        assert_eq!(status, StatusCode::OK);

        // allowlist match + wrong token ⇒ 401.
        let req_bad_tok = request_with_peer(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), Some("wrong"));
        let (status, _) = oneshot_status(router(cfg.clone()), req_bad_tok).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // correct token but allowlist miss ⇒ 403 (allowlist checked before token).
        let req_off_net =
            request_with_peer(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), Some("dev-token"));
        let (status, _) = oneshot_status(router(cfg), req_off_net).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn malformed_cidr_entries_are_dropped() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(
            None,
            &[
                "not-a-cidr".to_string(),
                "10.0.0.0/8".to_string(),
                "".to_string(),
            ],
        );
        // Valid entry is kept.
        let req_ok = request_with_peer(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), None);
        let (status, _) = oneshot_status(router(cfg.clone()), req_ok).await;
        assert_eq!(status, StatusCode::OK);

        // Garbage did not poison the list — peer outside 10/8 is still 403.
        let req_miss = request_with_peer(IpAddr::V4(Ipv4Addr::new(172, 16, 1, 1)), None);
        let (status, _) = oneshot_status(router(cfg), req_miss).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn describe_mode_matrix() {
        assert_eq!(
            MetricsAuthConfig::from_telemetry_raw(None, &[]).describe_mode(),
            "loopback-only"
        );
        assert_eq!(
            MetricsAuthConfig::from_telemetry_raw(Some("t"), &[]).describe_mode(),
            "token"
        );
        assert_eq!(
            MetricsAuthConfig::from_telemetry_raw(None, &["10.0.0.0/8".into()]).describe_mode(),
            "allowlist"
        );
        assert_eq!(
            MetricsAuthConfig::from_telemetry_raw(Some("t"), &["10.0.0.0/8".into()])
                .describe_mode(),
            "token+allowlist"
        );
    }

    #[test]
    fn empty_or_whitespace_token_is_none() {
        let cfg = MetricsAuthConfig::from_telemetry_raw(Some("   "), &[]);
        assert!(cfg.is_unauthenticated());
        let cfg2 = MetricsAuthConfig::from_telemetry_raw(Some(""), &[]);
        assert!(cfg2.is_unauthenticated());
    }

    #[test]
    fn extract_bearer_parses_valid_header() {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer abc123"),
        );
        assert_eq!(extract_bearer(&h), Some("abc123"));
    }

    #[test]
    fn extract_bearer_rejects_wrong_scheme() {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Basic abc"),
        );
        assert_eq!(extract_bearer(&h), None);
    }
}
