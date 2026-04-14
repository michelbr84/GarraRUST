//! `/v1/auth/*` routes — wired into the global `AppState` since GAR-391c.
//!
//! ## Endpoints
//!
//! | Method | Path | Returns | Notes |
//! |---|---|---|---|
//! | POST | `/v1/auth/login`  | 200 + tokens / 401 byte-identical | refresh_token re-enabled |
//! | POST | `/v1/auth/refresh` | 200 + new tokens / 401 byte-identical | rotation default ON |
//! | POST | `/v1/auth/logout`  | 204 always (idempotent) | anti-enumeration |
//! | POST | `/v1/auth/signup`  | 201 + tokens / 409 duplicate | uses SignupPool |
//!
//! ## Fail-soft when AuthConfig missing
//!
//! Each handler checks `state.auth_provider.is_some()` (and friends) and
//! returns `503 Service Unavailable` if any required component is `None`.
//! This lets the gateway boot in dev without `GARRAIA_*` env vars set.
//!
//! ## Anti-enumeration
//!
//! All five login failure modes return a **byte-identical** 401 response
//! body. Refresh failures use the same body. Signup uses a distinct 409
//! because login already leaks email existence and 391c's signup endpoint
//! prioritizes UX clarity over enumeration resistance (rate limiting is
//! the right tool for signup abuse — deferred to follow-up).
//!
//! Plan reference: `plans/0012-gar-391c-extractor-and-wiring.md` §3.4.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use garraia_auth::{AuthError, Credential, RequestCtx, signup_user};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth_metrics;
use crate::state::AppState;

/// Build the `/v1/auth/*` subrouter against the global `AppState`.
/// Mounted unconditionally by `crate::router::build_router` since GAR-391c.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/auth/login", post(login_handler))
        .route("/v1/auth/refresh", post(refresh_handler))
        .route("/v1/auth/logout", post(logout_handler))
        .route("/v1/auth/signup", post(signup_handler))
}

// ─── Request / response shapes ─────────────────────────────────────────────

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

#[derive(Serialize)]
pub struct LoginResponse {
    pub user_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

impl std::fmt::Debug for RefreshRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshRequest")
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

impl std::fmt::Debug for LogoutRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogoutRequest")
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

impl std::fmt::Debug for SignupRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupRequest")
            .field("email", &self.email)
            .field("password", &"[REDACTED]")
            .field("display_name", &self.display_name)
            .finish()
    }
}

#[derive(Serialize)]
pub struct SignupResponse {
    pub user_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn build_request_ctx(headers: &HeaderMap, addr: SocketAddr) -> RequestCtx {
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
    RequestCtx {
        ip: Some(ip),
        user_agent,
        request_id,
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "invalid_credentials"})),
    )
        .into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal_error"})),
    )
        .into_response()
}

fn service_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error": "auth_unavailable"})),
    )
        .into_response()
}

fn duplicate_email() -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": "duplicate_email"})),
    )
        .into_response()
}

/// Issue a fresh access + refresh pair for a verified `user_id`. Used by
/// login + refresh + signup happy paths.
async fn issue_token_pair(
    state: &AppState,
    user_id: Uuid,
) -> Result<(String, String, DateTime<Utc>), Response> {
    let jwt = state.jwt_issuer.as_ref().ok_or_else(service_unavailable)?;
    let sessions = state
        .auth_session_store
        .as_ref()
        .ok_or_else(service_unavailable)?;

    let (access_token, expires_at) = jwt.issue_access(user_id).map_err(|e| {
        tracing::error!(error = %e, "jwt issuance failed");
        internal_error()
    })?;
    let refresh = jwt.issue_refresh().map_err(|e| {
        tracing::error!(error = %e, "refresh token generation failed");
        internal_error()
    })?;
    sessions
        .issue(user_id, &refresh.hmac_hash, None)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "session insert failed");
            internal_error()
        })?;
    Ok((
        access_token,
        refresh.plaintext.expose_secret().to_string(),
        expires_at,
    ))
}

// ─── POST /v1/auth/login ───────────────────────────────────────────────────

async fn login_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let started = Instant::now();
    let Some(provider) = state.auth_provider.as_ref() else {
        auth_metrics::record_login("failure_internal", started.elapsed().as_secs_f64());
        return service_unavailable();
    };
    let ctx = build_request_ctx(&headers, addr);

    let credential = Credential::Internal {
        email: body.email.clone(),
        password: SecretString::from(body.password),
    };

    let outcome = provider
        .verify_credential_with_ctx(&credential, &ctx)
        .await;
    let elapsed = started.elapsed().as_secs_f64();

    match outcome {
        Ok(Some(user_id)) => match issue_token_pair(&state, user_id).await {
            Ok((access_token, refresh_token, expires_at)) => {
                auth_metrics::record_login("success", elapsed);
                let response = LoginResponse {
                    user_id,
                    access_token,
                    refresh_token,
                    expires_at,
                };
                match serde_json::to_value(&response) {
                    Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                    Err(e) => {
                        tracing::error!(error = %e, "login response serialization failed");
                        internal_error()
                    }
                }
            }
            Err(resp) => {
                auth_metrics::record_login("failure_internal", elapsed);
                resp
            }
        },
        Ok(None) => {
            auth_metrics::record_login("failure_invalid_credentials", elapsed);
            unauthorized()
        }
        Err(AuthError::UnknownHashFormat) => {
            auth_metrics::record_login("failure_unknown_hash", elapsed);
            unauthorized()
        }
        Err(e) => {
            tracing::error!(error = %e, "verify_credential storage error");
            auth_metrics::record_login("failure_internal", elapsed);
            internal_error()
        }
    }
}

// ─── POST /v1/auth/refresh ─────────────────────────────────────────────────

async fn refresh_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshRequest>,
) -> Response {
    let Some(jwt) = state.jwt_issuer.as_ref() else {
        auth_metrics::record_refresh("failure_internal");
        return service_unavailable();
    };
    let Some(sessions) = state.auth_session_store.as_ref() else {
        auth_metrics::record_refresh("failure_internal");
        return service_unavailable();
    };

    // Verify the supplied plaintext against stored sessions.
    let result = sessions
        .verify_refresh(&body.refresh_token, jwt.as_ref())
        .await;
    match result {
        Ok(Some((session_id, user_id))) => {
            // Rotation default ON: revoke the old session and issue a new pair.
            if let Err(e) = sessions.revoke(session_id).await {
                tracing::error!(error = %e, "session revoke failed during rotation");
                auth_metrics::record_refresh("failure_internal");
                return internal_error();
            }
            match issue_token_pair(&state, user_id).await {
                Ok((access_token, refresh_token, expires_at)) => {
                    auth_metrics::record_refresh("success");
                    let response = LoginResponse {
                        user_id,
                        access_token,
                        refresh_token,
                        expires_at,
                    };
                    match serde_json::to_value(&response) {
                        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                        Err(e) => {
                            tracing::error!(error = %e, "refresh response serialization failed");
                            internal_error()
                        }
                    }
                }
                Err(resp) => {
                    auth_metrics::record_refresh("failure_internal");
                    resp
                }
            }
        }
        Ok(None) => {
            auth_metrics::record_refresh("failure_invalid_credentials");
            unauthorized()
        }
        Err(e) => {
            tracing::error!(error = %e, "verify_refresh storage error");
            auth_metrics::record_refresh("failure_internal");
            internal_error()
        }
    }
}

// ─── POST /v1/auth/logout ──────────────────────────────────────────────────

async fn logout_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LogoutRequest>,
) -> Response {
    let Some(jwt) = state.jwt_issuer.as_ref() else {
        return service_unavailable();
    };
    let Some(sessions) = state.auth_session_store.as_ref() else {
        return service_unavailable();
    };

    // Idempotent: unknown / already-revoked tokens still return 204.
    if let Ok(Some((session_id, _user_id))) =
        sessions.verify_refresh(&body.refresh_token, jwt.as_ref()).await
    {
        let _ = sessions.revoke(session_id).await;
    }
    StatusCode::NO_CONTENT.into_response()
}

// ─── POST /v1/auth/signup ──────────────────────────────────────────────────

// TODO(GAR-391c-followup): rate limiting via tower-governor on this handler.
// Plan 0012 §3.4 deferred rate limiting to a dedicated follow-up. Without it,
// `/v1/auth/signup` is the most abusable endpoint on the gateway. Open a new
// GAR issue + ADR-style note when the rate-limit story is ready. Security
// review 391c L-2 flagged the absence with no linked issue — this comment
// closes that nit by documenting the explicit deferral here at the call site.
async fn signup_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignupRequest>,
) -> Response {
    let Some(login_pool) = state.login_pool.as_ref() else {
        auth_metrics::record_signup("failure_internal");
        return service_unavailable();
    };
    let Some(signup_pool) = state.signup_pool.as_ref() else {
        auth_metrics::record_signup("failure_internal");
        return service_unavailable();
    };

    let password = SecretString::from(body.password);
    match signup_user(
        login_pool.as_ref(),
        signup_pool.as_ref(),
        &body.email,
        &password,
        &body.display_name,
    )
    .await
    {
        Ok(user_id) => match issue_token_pair(&state, user_id).await {
            Ok((access_token, refresh_token, expires_at)) => {
                auth_metrics::record_signup("success");
                let response = SignupResponse {
                    user_id,
                    access_token,
                    refresh_token,
                    expires_at,
                };
                match serde_json::to_value(&response) {
                    Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
                    Err(e) => {
                        tracing::error!(error = %e, "signup response serialization failed");
                        internal_error()
                    }
                }
            }
            Err(resp) => {
                auth_metrics::record_signup("failure_internal");
                resp
            }
        },
        Err(AuthError::DuplicateEmail) => {
            auth_metrics::record_signup("failure_duplicate_email");
            duplicate_email()
        }
        Err(e) => {
            tracing::error!(error = %e, "signup_user error");
            auth_metrics::record_signup("failure_internal");
            internal_error()
        }
    }
}

