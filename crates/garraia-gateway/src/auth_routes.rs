//! `POST /v1/auth/login` — feature-gated subrouter for GAR-391b.
//!
//! This module exists ONLY when `auth-v1` is on. It does NOT touch the
//! default `AppState` of `garraia-gateway`. The router is built standalone
//! (its own `AuthState` carrying the `InternalProvider` + `JwtIssuer`) so
//! it can be tested in isolation. GAR-391c removes the feature flag, wires
//! the components into the global `AppState`, and adds the extractor +
//! refresh/logout endpoints.
//!
//! ## Scope (391b reduced)
//!
//! 391b ships **access tokens only**. The login response is
//! `{user_id, access_token, expires_at}` — no `refresh_token`. Refresh
//! tokens, the refresh endpoint, the logout endpoint and `SessionStore`
//! wiring all move to **391c** (see plan 0011 amendment "Segunda correção
//! de escopo aplicada durante Wave 1"). The reason: the `garraia_login`
//! BYPASSRLS role from migration 008 lacks `SELECT ON sessions`, which is
//! required by both `INSERT ... RETURNING id` and `verify_refresh`.
//! Adding that grant means broadening migration 008 + ADR 0005 in a
//! separate corrective delivery; doing so alongside the refresh endpoint
//! (which is intrinsically coupled) is the cleaner unit of work for 391c.
//!
//! ## Anti-enumeration
//!
//! Every failure mode (`user not found`, `wrong password`,
//! `account suspended`, `account deleted`, `unknown hash format`) returns
//! a **byte-identical** 401 response body `{"error":"invalid_credentials"}`
//! and identical headers. The integration test
//! `tests/auth_v1_login.rs::failure_modes_are_byte_identical` asserts this.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use garraia_auth::{AuthError, Credential, InternalProvider, JwtIssuer, RequestCtx};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// State carried by the `/v1/auth/*` subrouter. Held by `Arc` so the router
/// can be shared across worker threads without cloning the issuer.
///
/// Refresh-token persistence (`SessionStore`) is intentionally NOT in this
/// state for 391b — it joins in 391c when the refresh endpoint lands.
#[derive(Clone)]
pub struct AuthState {
    pub provider: Arc<InternalProvider>,
    pub jwt: Arc<JwtIssuer>,
}

/// Build the auth subrouter. Mount with `.merge(auth_routes::router(state))`
/// in `bootstrap.rs` when you want the v1 endpoint live.
pub fn router(state: AuthState) -> Router {
    Router::new()
        .route("/v1/auth/login", post(login_handler))
        .with_state(state)
}

/// `POST /v1/auth/login` request body.
///
/// `Debug` is **manually implemented** so the password never reaches
/// `tracing::debug!("{:?}", body)` accidentally. (Security review 391b H-2.)
#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("email", &self.email)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// 391b login response shape — access token only. `refresh_token` joins
/// in 391c.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user_id: Uuid,
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}


async fn login_handler(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    // Build the request context from the available headers. Production
    // gateways behind a reverse proxy will populate `X-Forwarded-For` and
    // the future extractor (391c) will parse it; for 391b we fall back to
    // the direct connection address.
    let forwarded_ip = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok());
    let ip = forwarded_ip.unwrap_or_else(|| addr.ip());
    let user_agent = headers
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());
    let request_id = headers
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let ctx = RequestCtx {
        ip: Some(ip),
        user_agent,
        request_id,
    };

    let credential = Credential::Internal {
        email: body.email.clone(),
        password: SecretString::from(body.password),
    };

    match state
        .provider
        .verify_credential_with_ctx(&credential, &ctx)
        .await
    {
        Ok(Some(user_id)) => {
            // 391b: access token only. Refresh tokens defer to 391c with
            // the refresh endpoint and the migration-010 SELECT-on-sessions
            // grant.
            let (access_token, expires_at) = match state.jwt.issue_access(user_id) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(error = %e, "jwt issuance failed");
                    return internal_error();
                }
            };
            let response = LoginResponse {
                user_id,
                access_token,
                expires_at,
            };
            // serde_json::to_value cannot fail for a #[derive(Serialize)]
            // struct of fixed shape, but we use `?`-style match instead of
            // unwrap to keep `unwrap()` strictly out of production paths
            // (project rule + code review 391b blocker #1).
            match serde_json::to_value(&response) {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err(e) => {
                    tracing::error!(error = %e, "login response serialization failed");
                    internal_error()
                }
            }
        }
        Ok(None) => unauthorized(),
        Err(AuthError::UnknownHashFormat) => {
            // Anti-enumeration: same body as wrong-password / not-found.
            unauthorized()
        }
        Err(e) => {
            tracing::error!(error = %e, "verify_credential storage error");
            internal_error()
        }
    }
}

fn unauthorized() -> axum::response::Response {
    // Static body — serializing a `#[derive(Serialize)] struct ErrorBody`
    // with a single `&'static str` field cannot fail, but we still avoid
    // `unwrap()` in production by using the `json!` macro which is
    // infallible at this call site.
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "invalid_credentials"})),
    )
        .into_response()
}

fn internal_error() -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal_error"})),
    )
        .into_response()
}
