//! GAR-202: Session token middleware for LLM conversation plane.
//!
//! Extracts a bearer token from:
//!   1. Cookie `garraia_session`
//!   2. `Authorization: Bearer <token>` header
//!   3. `X-Session-Key: <token>` header
//!
//! Validates the token against the `session_tokens` table and injects a
//! [`ValidatedSession`] extension into the request.  If validation fails and
//! `session_tokens_required` is `true`, responds with 401.

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

/// Injected by `require_session_auth` on successful token validation.
#[derive(Debug, Clone)]
pub struct ValidatedSession {
    pub session_id: String,
    pub token: String,
}

/// Extract the raw session token from cookie, Authorization header, or X-Session-Key.
pub fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    // 1. Cookie: garraia_session=<token>
    if let Some(cookie_hdr) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        for part in cookie_hdr.split(';') {
            let part = part.trim();
            if let Some(val) = part.strip_prefix("garraia_session=")
                && !val.is_empty()
            {
                return Some(val.to_string());
            }
        }
    }
    // 2. Authorization: Bearer <token>
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(t) = auth.strip_prefix("Bearer ")
    {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    // 3. X-Session-Key: <token>
    if let Some(key) = headers.get("x-session-key").and_then(|v| v.to_str().ok()) {
        let key = key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }
    None
}

/// Build a `Set-Cookie` header value for the session token.
pub fn session_cookie(token: &str, ttl_secs: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "garraia_session={token}; HttpOnly{secure_flag}; SameSite=Strict; Path=/api; Max-Age={ttl_secs}"
    )
}

/// Build a `Set-Cookie` header value that clears the session cookie.
pub fn clear_session_cookie() -> String {
    "garraia_session=; HttpOnly; SameSite=Strict; Path=/api; Max-Age=0".to_string()
}

/// Axum middleware — validates the session token and injects [`ValidatedSession`].
///
/// Requires `Arc<AppState>` to be accessible via `axum::Extension`.  When the
/// token is invalid and `session_tokens_required = true`, returns 401.
/// Otherwise (or when `session_tokens_required = false`) the request proceeds
/// with no `ValidatedSession` extension injected.
pub async fn require_session_auth(
    axum::Extension(state): axum::Extension<crate::state::SharedState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let cfg = state.current_config();
    let idle = cfg.gateway.session_idle_secs;
    let required = cfg.gateway.session_tokens_required;

    if let Some(token) = extract_session_token(request.headers()) {
        if let Some(manager) = &state.chat_session_manager {
            match manager.validate_token(&token, idle).await {
                Ok(Some(session_id)) => {
                    request
                        .extensions_mut()
                        .insert(ValidatedSession { session_id, token });
                }
                Ok(None) => {
                    if required {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "session_auth: token validation error");
                    if required {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                }
            }
        }
    } else if required {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}
