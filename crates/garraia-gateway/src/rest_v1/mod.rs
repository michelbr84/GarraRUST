//! REST `/v1` surface (Fase 3.4, plan 0015).
//!
//! Versioned HTTP API. All errors follow RFC 9457 Problem Details.
//! OpenAPI 3.1 spec is generated via `utoipa`; Swagger UI is served at
//! `/docs`.

pub mod me;
pub mod openapi;
pub mod problem;

use std::sync::Arc;

use axum::extract::FromRef;
use axum::routing::get;
use axum::Router;
use garraia_auth::{JwtIssuer, LoginPool};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

use self::openapi::ApiDoc;
use self::problem::RestError;

/// Sub-state for the `/v1` router. Holds exactly the `Arc`s that the
/// `garraia_auth::Principal` extractor needs via `FromRef`.
///
/// Built from `AppState` when `AuthConfig` is wired (both `jwt_issuer`
/// and `login_pool` are `Some`). When unwired — i.e. the gateway is
/// running in fail-soft dev mode — `from_app_state` returns `None` and
/// the router falls back to a 503 Problem Details handler for every
/// `/v1/*` path.
#[derive(Clone)]
pub struct RestV1State {
    pub jwt_issuer: Arc<JwtIssuer>,
    pub login_pool: Arc<LoginPool>,
}

impl RestV1State {
    /// Try to build from the gateway's `AppState`. Returns `None` when
    /// auth is not configured.
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            jwt_issuer: app.jwt_issuer.clone()?,
            login_pool: app.login_pool.clone()?,
        })
    }
}

impl FromRef<RestV1State> for Arc<JwtIssuer> {
    fn from_ref(s: &RestV1State) -> Self {
        s.jwt_issuer.clone()
    }
}

impl FromRef<RestV1State> for Arc<LoginPool> {
    fn from_ref(s: &RestV1State) -> Self {
        s.login_pool.clone()
    }
}

/// Build the `/v1` router (Fase 3.4 slice 1).
///
/// When `AuthConfig` is wired, returns a router that serves:
/// - `GET /v1/me` via the `Principal` extractor,
/// - `/v1/openapi.json` — the generated OpenAPI 3.1 document,
/// - `GET /docs` — Swagger UI rendering that document.
///
/// When `AuthConfig` is missing (fail-soft dev mode), returns a router
/// with the same route set but every handler answers 503 Problem Details.
/// Routes are enumerated explicitly (no `.fallback()`) so the merged
/// main router does not lose its own 404 behavior.
pub fn router(app_state: Arc<AppState>) -> Router {
    match RestV1State::from_app_state(&app_state) {
        Some(sub) => Router::new()
            .route("/v1/me", get(me::get_me))
            .with_state(sub)
            .merge(
                SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()),
            ),
        None => Router::new()
            .route("/v1/me", get(unconfigured_handler))
            .route("/v1/openapi.json", get(unconfigured_handler))
            .route("/docs", get(unconfigured_handler)),
    }
}

/// Fail-soft handler used when `AuthConfig` is missing. Every `/v1`
/// route falls back to this while the gateway boots without auth.
async fn unconfigured_handler() -> RestError {
    RestError::AuthUnconfigured
}
