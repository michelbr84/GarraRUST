//! REST `/v1` surface (Fase 3.4, plan 0015).
//!
//! Versioned HTTP API. All errors follow RFC 9457 Problem Details.
//! OpenAPI 3.1 spec is generated via `utoipa`; Swagger UI is served at
//! `/docs`.

pub mod me;
pub mod problem;

use std::sync::Arc;

use axum::extract::FromRef;
use garraia_auth::{JwtIssuer, LoginPool};

use crate::state::AppState;

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
