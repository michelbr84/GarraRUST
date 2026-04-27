use std::sync::Arc;

use axum::extract::Request;
use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::Mutex;

use super::rbac::Role;
use super::store::{AdminSession, AdminStore};

pub const SESSION_COOKIE_NAME: &str = "garraia_admin_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Extracted admin session available to handlers via extension.
#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    pub user_id: String,
    pub username: String,
    pub role: Role,
    pub csrf_token: String,
    pub session_token: String,
}

impl AuthenticatedAdmin {
    pub fn from_session(session: &AdminSession) -> Self {
        Self {
            user_id: session.user_id.clone(),
            username: session.username.clone(),
            role: session.role,
            csrf_token: session.csrf_token.clone(),
            session_token: session.token.clone(),
        }
    }
}

/// Middleware that validates the admin session cookie and injects `AuthenticatedAdmin`.
pub async fn require_admin_auth(
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let admin_store = request
        .extensions()
        .get::<Arc<Mutex<AdminStore>>>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let token = extract_session_cookie(&headers).ok_or(StatusCode::UNAUTHORIZED)?;

    let guard = admin_store.lock().await;
    let session = guard
        .validate_session(&token)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    drop(guard);

    let admin = AuthenticatedAdmin::from_session(&session);
    request.extensions_mut().insert(admin);

    Ok(next.run(request).await)
}

/// Middleware that validates CSRF token on mutating requests (POST/PUT/DELETE/PATCH).
pub async fn require_csrf(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = request.method().clone();

    if method == axum::http::Method::GET
        || method == axum::http::Method::HEAD
        || method == axum::http::Method::OPTIONS
    {
        return Ok(next.run(request).await);
    }

    let admin = request
        .extensions()
        .get::<AuthenticatedAdmin>()
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let csrf_from_header = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // GAR-459 (PR-A of GAR-454, security-auditor finding MEDIUM): use a
    // constant-time comparison so the CSRF check cannot leak the prefix
    // length of `admin.csrf_token` via a byte-by-byte short-circuit timing
    // oracle. Even though the CSRF token is a 32-byte secret never echoed
    // in responses, the project convention (see `garraia-auth` hashing
    // module) is to compare any secret-comparison via `subtle`.
    use subtle::ConstantTimeEq;
    let header_bytes = csrf_from_header.as_bytes();
    let stored_bytes = admin.csrf_token.as_bytes();
    if csrf_from_header.is_empty()
        || header_bytes.len() != stored_bytes.len()
        || header_bytes.ct_eq(stored_bytes).unwrap_u8() == 0
    {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

/// Middleware that adds security headers to all admin responses.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "x-xss-protection",
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self' https://fonts.googleapis.com https://fonts.gstatic.com; frame-ancestors 'none'"
        ),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        "cache-control",
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert("pragma", HeaderValue::from_static("no-cache"));

    response
}

/// Extract the session token from the cookie header.
pub fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix(SESSION_COOKIE_NAME) {
            let value = value.strip_prefix('=')?;
            return Some(value.to_string());
        }
    }
    None
}

/// Build a Set-Cookie header for admin session.
///
/// **`Path=/`** (GAR-459 PR-A of GAR-454, security-auditor finding MEDIUM):
/// the cookie used to carry `Path=/admin` which prevented browsers from
/// sending it on requests to `/api/plugins/*` under SameSite=Strict path
/// scoping. After this PR the admin session cookie also gates the
/// `/api/plugins/*` sub-router (GAR-459), so the path scope must cover both.
/// HttpOnly + Secure + SameSite=Strict remain in place.
pub fn build_session_cookie(token: &str, max_age_secs: i64) -> String {
    format!(
        "{}={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_COOKIE_NAME, token, max_age_secs
    )
}

/// Build a Set-Cookie header that clears the session. Path matches
/// `build_session_cookie` so the browser actually clears the right cookie.
pub fn build_clear_cookie() -> String {
    format!(
        "{}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0",
        SESSION_COOKIE_NAME
    )
}

/// Extract IP address from request (best effort).
pub fn extract_ip(
    headers: &HeaderMap,
    connect_info: Option<&std::net::SocketAddr>,
) -> Option<String> {
    // TODO(plan-0023+): this helper accepts `X-Forwarded-For` without
    // validating whether the immediate peer is an allowlisted proxy
    // (same pre-0022 bug fixed for rate_limiter.rs in plan 0022 T2).
    // Migrate to `crate::rate_limiter::real_client_ip` once plan 0023
    // lifts that helper into a shared module and introduces
    // `TRUSTED_PROXIES` into the admin surface. Deliberately
    // out-of-scope for plan 0022 (GAR-426 description).
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = forwarded.split(',').next()
    {
        return Some(first.trim().to_string());
    }
    connect_info.map(|addr| addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extract_session_from_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str("garraia_admin_session=abc123; other=val").unwrap(),
        );

        let token = extract_session_cookie(&headers);
        assert_eq!(token.as_deref(), Some("abc123"));
    }

    #[test]
    fn missing_cookie_returns_none() {
        let headers = HeaderMap::new();
        assert!(extract_session_cookie(&headers).is_none());
    }

    #[test]
    fn build_cookie_format() {
        let cookie = build_session_cookie("tok123", 86400);
        assert!(cookie.contains("garraia_admin_session=tok123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Strict"));
        // GAR-459 (PR-A of GAR-454): cookie path widened from `/admin` to
        // `/` so the same admin session also gates `/api/plugins/*`. The
        // previous assertion `Path=/admin` would prevent the cookie from
        // being sent to plugin routes under SameSite=Strict path scoping.
        assert!(cookie.contains("Path=/"));
        assert!(!cookie.contains("Path=/admin"));
    }

    #[test]
    fn build_clear_cookie_path_matches_session_cookie() {
        // Path on the clear-cookie MUST match the session cookie path,
        // otherwise the browser keeps the original cookie around.
        let clear = build_clear_cookie();
        assert!(clear.contains("garraia_admin_session="));
        assert!(clear.contains("Path=/"));
        assert!(clear.contains("Max-Age=0"));
        assert!(!clear.contains("Path=/admin"));
    }
}
