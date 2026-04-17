//! REST `/v1` surface (Fase 3.4, plan 0015 + plan 0016 M1).
//!
//! Versioned HTTP API. All errors follow RFC 9457 Problem Details.
//! OpenAPI 3.1 spec is generated via `utoipa`; Swagger UI is served at
//! `/docs`.
//!
//! ## State layering (plan 0016 M1-T4)
//!
//! Two sub-states are derived from `AppState` at router build time:
//!
//! - [`RestV1AuthState`] holds only `jwt_issuer` + `login_pool`. It
//!   is the state type for handlers that need the `Principal`
//!   extractor but do not touch the RLS `garraia_app` pool — e.g.
//!   `GET /v1/me`.
//! - [`RestV1FullState`] wraps `RestV1AuthState` and adds `app_pool`
//!   (the `garraia_app` RLS pool). It is the state type for handlers
//!   that read/write the scoped tenant data — e.g. `/v1/groups/*`.
//!
//! The `FromRef` chain on `RestV1FullState` also exposes `jwt_issuer`
//! and `login_pool`, so the `Principal` extractor works against full
//! state handlers as well.
//!
//! The router builder is a three-way match:
//!
//! 1. Auth wired AND app wired → /v1/me and /v1/groups on real handlers
//! 2. Auth wired, app NOT wired → /v1/me real; /v1/groups as 503 stub
//! 3. Neither wired → every /v1 route is a 503 stub (fail-soft dev mode)
//!
//! In mode 3 the routes are registered explicitly (no `.fallback()`)
//! so the merged main router keeps its own 404 behavior.

pub mod groups;
pub mod invites;
pub mod me;
pub mod openapi;
pub mod problem;

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRef;
use axum::routing::{get, post};
use garraia_auth::{AppPool, JwtIssuer, LoginPool};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

use self::openapi::ApiDoc;
use self::problem::RestError;

/// Sub-state for `/v1` handlers that only need auth components
/// (`Principal` extractor + JWT). No `AppPool`. Used by `GET /v1/me`.
#[derive(Clone)]
pub struct RestV1AuthState {
    pub jwt_issuer: Arc<JwtIssuer>,
    pub login_pool: Arc<LoginPool>,
}

impl RestV1AuthState {
    /// Try to build from the gateway's `AppState`. Returns `None` when
    /// auth is not configured (fail-soft dev mode).
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            jwt_issuer: app.jwt_issuer.clone()?,
            login_pool: app.login_pool.clone()?,
        })
    }
}

impl FromRef<RestV1AuthState> for Arc<JwtIssuer> {
    fn from_ref(s: &RestV1AuthState) -> Self {
        s.jwt_issuer.clone()
    }
}

impl FromRef<RestV1AuthState> for Arc<LoginPool> {
    fn from_ref(s: &RestV1AuthState) -> Self {
        s.login_pool.clone()
    }
}

/// Sub-state for `/v1` handlers that need both auth + the RLS
/// `garraia_app` pool. Used by `/v1/groups/*`.
///
/// The `FromRef` chain on this state also exposes `Arc<JwtIssuer>`
/// and `Arc<LoginPool>`, so the `Principal` extractor compiles
/// against handlers that use `State<RestV1FullState>`.
#[derive(Clone)]
pub struct RestV1FullState {
    pub auth: RestV1AuthState,
    pub app_pool: Arc<AppPool>,
}

impl RestV1FullState {
    /// Try to build from the gateway's `AppState`. Returns `None`
    /// unless BOTH auth is configured AND `AppPool` was wired (i.e.
    /// `GARRAIA_APP_DATABASE_URL` is set and the connect succeeded).
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            auth: RestV1AuthState::from_app_state(app)?,
            app_pool: app.app_pool.clone()?,
        })
    }
}

impl FromRef<RestV1FullState> for Arc<JwtIssuer> {
    fn from_ref(s: &RestV1FullState) -> Self {
        s.auth.jwt_issuer.clone()
    }
}

impl FromRef<RestV1FullState> for Arc<LoginPool> {
    fn from_ref(s: &RestV1FullState) -> Self {
        s.auth.login_pool.clone()
    }
}

impl FromRef<RestV1FullState> for Arc<AppPool> {
    fn from_ref(s: &RestV1FullState) -> Self {
        s.app_pool.clone()
    }
}

/// Build the `/v1` router.
///
/// Three modes based on what's wired in `AppState`:
///
/// 1. **Auth + AppPool wired**: `/v1/me` (real handler), `/v1/groups*`
///    (stub `unconfigured_handler` in M1 — real handlers land in M4),
///    `/v1/openapi.json`, `/docs`.
/// 2. **Auth wired, AppPool missing**: `/v1/me` (real), `/v1/groups*`
///    answer 503 via `unconfigured_handler`. `/docs` still served.
/// 3. **Neither wired (fail-soft dev mode)**: every `/v1/*` route
///    is registered explicitly on `unconfigured_handler`. No
///    `.fallback()` — the merged main router keeps its 404 behavior
///    for paths outside `/v1`.
pub fn router(app_state: Arc<AppState>) -> Router {
    // Try the most specific state first, then degrade.
    match (
        RestV1FullState::from_app_state(&app_state),
        RestV1AuthState::from_app_state(&app_state),
    ) {
        (Some(full), Some(_auth)) => {
            // Mode 1: auth + AppPool wired.
            //
            // Uses `RestV1FullState` as the router state so the
            // `FromRef` chain exposes `Arc<JwtIssuer>`,
            // `Arc<LoginPool>` AND `Arc<AppPool>` at the extractor
            // level. `GET /v1/me` still compiles against this state
            // because `RestV1FullState: FromRef<Arc<JwtIssuer>>` and
            // `FromRef<Arc<LoginPool>>`.
            //
            // Plan 0016 M4: `/v1/groups` routes now point at the
            // real handlers (`groups::create_group` + `groups::get_group`).
            // Modes 2 and 3 still answer 503 via `unconfigured_handler`
            // because they lack `Arc<AppPool>` in state.
            Router::new()
                .route("/v1/me", get(me::get_me))
                .route("/v1/groups", post(groups::create_group))
                .route(
                    "/v1/groups/{id}",
                    get(groups::get_group).patch(groups::patch_group),
                )
                .route("/v1/groups/{id}/invites", post(groups::create_invite))
                .route(
                    "/v1/invites/{token}/accept",
                    post(invites::accept_invite),
                )
                .with_state(full)
                .merge(SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        }
        (None, Some(auth)) => {
            // Mode 2: auth wired, AppPool missing. `/v1/me` still
            // works (uses `RestV1AuthState`); `/v1/groups` answers
            // 503 via `unconfigured_handler`. Same route surface as
            // mode 1 so clients see consistent URLs regardless of
            // whether `GARRAIA_APP_DATABASE_URL` is set.
            Router::new()
                .route("/v1/me", get(me::get_me))
                .route("/v1/groups", post(unconfigured_handler))
                .route(
                    "/v1/groups/{id}",
                    get(unconfigured_handler).patch(unconfigured_handler),
                )
                .route("/v1/groups/{id}/invites", post(unconfigured_handler))
                .route("/v1/invites/{token}/accept", post(unconfigured_handler))
                .with_state(auth)
                .merge(SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        }
        (_, None) => {
            // Mode 3: no auth at all. Every route is a stub.
            Router::new()
                .route("/v1/me", get(unconfigured_handler))
                .route("/v1/groups", post(unconfigured_handler))
                .route(
                    "/v1/groups/{id}",
                    get(unconfigured_handler).patch(unconfigured_handler),
                )
                .route("/v1/groups/{id}/invites", post(unconfigured_handler))
                .route("/v1/invites/{token}/accept", post(unconfigured_handler))
                .route("/v1/openapi.json", get(unconfigured_handler))
                .route("/docs", get(unconfigured_handler))
                .route("/docs/{*rest}", get(unconfigured_handler))
        }
    }
}

/// Fail-soft handler used when `AuthConfig` / `AppPool` is missing.
/// Routes that cannot serve in the current mode fall back here and
/// answer 503 Problem Details via `RestError::AuthUnconfigured`.
async fn unconfigured_handler() -> impl axum::response::IntoResponse {
    RestError::AuthUnconfigured
}
