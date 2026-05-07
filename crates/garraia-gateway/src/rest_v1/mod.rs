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

pub mod audit;
pub mod chats;
pub mod groups;
pub mod invites;
pub mod me;
pub mod memory;
pub mod messages;
pub mod openapi;
pub mod problem;
pub mod tasks;
pub mod uploads;

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRef;
use axum::routing::{delete, get, head, patch, post};
use garraia_auth::{AppPool, JwtIssuer, LoginPool};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::rate_limiter::{
    RateLimitLayerState, RateLimiter, parse_trusted_proxies, rate_limit_layer_authenticated,
};
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

/// Storage wiring sub-state — object store backend + staging context.
/// Plan 0044 (GAR-395 slice 2). Both fields are `Option` so the
/// router can degrade uploads to 503 when the backend is not
/// configured, without tearing down the rest of the `/v1` surface.
#[derive(Clone, Default)]
pub struct RestV1StorageState {
    pub object_store: Option<Arc<dyn garraia_storage::ObjectStore>>,
    pub upload_staging: Option<Arc<uploads::UploadStaging>>,
}

impl RestV1StorageState {
    fn from_app_state(app: &AppState) -> Self {
        Self {
            object_store: app.object_store.clone(),
            upload_staging: app.upload_staging.clone(),
        }
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
    /// Plan 0044: storage wiring for tus upload commit. Always
    /// present (carries `None` fields in fail-soft mode).
    pub storage: RestV1StorageState,
}

impl RestV1FullState {
    /// Try to build from the gateway's `AppState`. Returns `None`
    /// unless BOTH auth is configured AND `AppPool` was wired (i.e.
    /// `GARRAIA_APP_DATABASE_URL` is set and the connect succeeded).
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            auth: RestV1AuthState::from_app_state(app)?,
            app_pool: app.app_pool.clone()?,
            storage: RestV1StorageState::from_app_state(app),
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
            // Plan 0022 T3 (GAR-426): per-user authenticated rate-limit
            // (20/min, burst 5) on the 3 privileged endpoints. Replaces
            // the pre-0022 `rate_limit_layer` that keyed by JWT token-
            // prefix (all HS256 tokens collided on `jwt:eyJhbGci`). The
            // new `rate_limit_layer_authenticated` verifies the JWT and
            // keys by the `sub` claim (`jwt-sub:{uuid}`), giving each
            // user an isolated bucket. Unauthenticated fallback uses
            // `real_client_ip` with trusted-proxy stripping (plan 0022
            // T2, env `GARRAIA_TRUSTED_PROXIES`).
            let members_manage_rl = RateLimiter::members_manage_limiter();
            let trusted_proxies = std::env::var("GARRAIA_TRUSTED_PROXIES")
                .ok()
                .map(|v| parse_trusted_proxies(&v))
                .unwrap_or_default();
            let rate_limit_state = RateLimitLayerState::new(
                members_manage_rl,
                full.auth.jwt_issuer.clone(),
                trusted_proxies,
            );
            let rate_limited_routes = Router::new()
                .route(
                    "/v1/groups/{id}/members/{user_id}/setRole",
                    post(groups::set_member_role),
                )
                .route(
                    "/v1/groups/{id}/members/{user_id}",
                    delete(groups::delete_member),
                )
                .route("/v1/invites/{token}/accept", post(invites::accept_invite))
                .layer(axum::middleware::from_fn_with_state(
                    rate_limit_state.clone(),
                    rate_limit_layer_authenticated,
                ));

            // Plan 0041 (GAR-395 slice 1): tus 1.0 Creation + HEAD.
            // Wrapped in a 412 → Tus-Version header layer so precondition
            // failures advertise the supported version without each
            // handler having to remember the tus spec. `POST /v1/uploads`
            // is ALSO rate-limited (security audit SEC-M1) — the
            // expiration worker lands in slice 3, so without a bucket
            // per-user the `tus_uploads` table is a DoS surface. `HEAD`
            // is a cheap probe and inherits the same bucket to keep
            // the layer simple — the tighter `members_manage` preset
            // (20/min, burst 5) is acceptable for both ops in slice 1.
            // `OPTIONS /v1/uploads` is intentionally UNAUTHENTICATED
            // (tus §5.2) and NOT rate-limited via the per-user
            // authenticated limiter. Co-locate it on the same route
            // as POST using the method-routing chain so Axum's
            // method dispatcher hands OPTIONS to the tus handler
            // (instead of letting it fall through to tower-http or
            // the router 405 branch). The authenticated rate-limit
            // layer below still keys on `jwt-sub:{uuid}` and
            // fails open (unauth path → IP bucket) for the OPTIONS
            // request — acceptable because the handler returns fixed
            // headers only.
            let tus_routes = Router::new()
                .route(
                    "/v1/uploads",
                    post(uploads::create_upload).options(uploads::options_uploads),
                )
                .route(
                    "/v1/uploads/{id}",
                    head(uploads::head_upload)
                        .patch(uploads::patch_upload)
                        .delete(uploads::delete_upload),
                )
                .layer(axum::middleware::from_fn_with_state(
                    rate_limit_state,
                    rate_limit_layer_authenticated,
                ))
                .layer(axum::middleware::from_fn(uploads::tus_version_header_layer));

            Router::new()
                .route("/v1/me", get(me::get_me))
                .route("/v1/groups", post(groups::create_group))
                .route(
                    "/v1/groups/{id}",
                    get(groups::get_group).patch(groups::patch_group),
                )
                .route("/v1/groups/{id}/invites", post(groups::create_invite))
                // Plan 0054 (GAR-506) — chats slice 1.
                .route(
                    "/v1/groups/{group_id}/chats",
                    post(chats::create_chat).get(chats::list_chats),
                )
                // Plan 0076 (GAR-530) — chats slice 4: individual chat ops + member CRUD.
                .route(
                    "/v1/chats/{chat_id}",
                    get(chats::get_chat)
                        .patch(chats::patch_chat)
                        .delete(chats::delete_chat),
                )
                .route(
                    "/v1/chats/{chat_id}/members",
                    get(chats::list_chat_members).post(chats::add_chat_member),
                )
                .route(
                    "/v1/chats/{chat_id}/members/{user_id}",
                    delete(chats::remove_chat_member),
                )
                // Plan 0055 (GAR-507) — messages slice 2.
                .route(
                    "/v1/chats/{chat_id}/messages",
                    post(messages::send_message).get(messages::list_messages),
                )
                // Plan 0057 (GAR-509) — threads slice 3.
                .route(
                    "/v1/messages/{message_id}/threads",
                    post(messages::create_thread),
                )
                // Plan 0062 (GAR-514) — memory API slice 1.
                .route(
                    "/v1/memory",
                    get(memory::list_memory).post(memory::create_memory),
                )
                .route(
                    "/v1/memory/{id}",
                    get(memory::get_memory)
                        .patch(memory::patch_memory)
                        .delete(memory::delete_memory),
                )
                // Plan 0072 (GAR-526) — memory API slice 2: pin/unpin.
                .route("/v1/memory/{id}/pin", post(memory::pin_memory))
                .route("/v1/memory/{id}/unpin", post(memory::unpin_memory))
                // Plan 0066/0067 (GAR-516/GAR-518) — tasks API slices 1+2.
                .route(
                    "/v1/groups/{group_id}/task-lists",
                    post(tasks::create_task_list).get(tasks::list_task_lists),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}",
                    patch(tasks::patch_task_list).delete(tasks::delete_task_list),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}/tasks",
                    post(tasks::create_task).get(tasks::list_tasks),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}",
                    get(tasks::get_task)
                        .patch(tasks::patch_task)
                        .delete(tasks::delete_task),
                )
                // Plan 0069 (GAR-520) — task comments API slice 3.
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments",
                    post(tasks::create_task_comment).get(tasks::list_task_comments),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}",
                    delete(tasks::delete_task_comment),
                )
                // Plan 0077 (GAR-533) — task assignees API slice 4.
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees",
                    post(tasks::add_task_assignee).get(tasks::list_task_assignees),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}",
                    delete(tasks::remove_task_assignee),
                )
                // Plan 0078 (GAR-536) — task labels API slice 5.
                .route(
                    "/v1/groups/{group_id}/task-labels",
                    post(tasks::create_task_label).get(tasks::list_task_labels),
                )
                .route(
                    "/v1/groups/{group_id}/task-labels/{label_id}",
                    delete(tasks::delete_task_label),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/labels",
                    post(tasks::assign_task_label),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}",
                    delete(tasks::remove_task_label_from_task),
                )
                // Plan 0079 (GAR-539) — task subscriptions API slice 6.
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/subscriptions",
                    post(tasks::subscribe_to_task)
                        .get(tasks::list_task_subscriptions)
                        .delete(tasks::unsubscribe_from_task),
                )
                // Plan 0070 (GAR-522) — audit API slice 1.
                .route("/v1/groups/{group_id}/audit", get(audit::list_audit))
                .merge(rate_limited_routes)
                .merge(tus_routes)
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
                .route(
                    "/v1/groups/{id}/members/{user_id}/setRole",
                    post(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{id}/members/{user_id}",
                    delete(unconfigured_handler),
                )
                .route("/v1/invites/{token}/accept", post(unconfigured_handler))
                // Plan 0054 (GAR-506) — chats slice 1, fail-soft 503.
                .route(
                    "/v1/groups/{group_id}/chats",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                // Plan 0076 (GAR-530) — chats slice 4, fail-soft 503.
                .route(
                    "/v1/chats/{chat_id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                .route(
                    "/v1/chats/{chat_id}/members",
                    get(unconfigured_handler).post(unconfigured_handler),
                )
                .route(
                    "/v1/chats/{chat_id}/members/{user_id}",
                    delete(unconfigured_handler),
                )
                // Plan 0055 (GAR-507) — messages slice 2, fail-soft 503.
                .route(
                    "/v1/chats/{chat_id}/messages",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                // Plan 0057 (GAR-509) — threads slice 3, fail-soft 503.
                .route(
                    "/v1/messages/{message_id}/threads",
                    post(unconfigured_handler),
                )
                // Plan 0062 (GAR-514) — memory API slice 1, fail-soft 503.
                .route(
                    "/v1/memory",
                    get(unconfigured_handler).post(unconfigured_handler),
                )
                .route(
                    "/v1/memory/{id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                // Plan 0072 (GAR-526) — memory API slice 2: pin/unpin, fail-soft 503.
                .route("/v1/memory/{id}/pin", post(unconfigured_handler))
                .route("/v1/memory/{id}/unpin", post(unconfigured_handler))
                // Plan 0066/0067/0069/0077 (GAR-516/GAR-518/GAR-520/GAR-533) — tasks API slices 1+2+3+4, fail-soft 503.
                .route(
                    "/v1/groups/{group_id}/task-lists",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}",
                    patch(unconfigured_handler).delete(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}/tasks",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                // Plan 0070 (GAR-522) — audit API slice 1, fail-soft 503.
                .route("/v1/groups/{group_id}/audit", get(unconfigured_handler))
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}",
                    delete(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}",
                    delete(unconfigured_handler),
                )
                .route(
                    "/v1/uploads",
                    post(unconfigured_handler).options(uploads::options_uploads),
                )
                .route(
                    "/v1/uploads/{id}",
                    head(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                .with_state(auth)
                .merge(SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        }
        (_, None) => {
            // Mode 3: no auth at all. Every route is a stub EXCEPT
            // `OPTIONS /v1/uploads` which has no auth requirements
            // by spec — always return the discovery headers.
            Router::new()
                .route("/v1/me", get(unconfigured_handler))
                .route("/v1/groups", post(unconfigured_handler))
                .route(
                    "/v1/groups/{id}",
                    get(unconfigured_handler).patch(unconfigured_handler),
                )
                .route("/v1/groups/{id}/invites", post(unconfigured_handler))
                .route(
                    "/v1/groups/{id}/members/{user_id}/setRole",
                    post(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{id}/members/{user_id}",
                    delete(unconfigured_handler),
                )
                .route("/v1/invites/{token}/accept", post(unconfigured_handler))
                // Plan 0054 (GAR-506) — chats slice 1, no-auth stub.
                .route(
                    "/v1/groups/{group_id}/chats",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                // Plan 0076 (GAR-530) — chats slice 4, no-auth stub.
                .route(
                    "/v1/chats/{chat_id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                .route(
                    "/v1/chats/{chat_id}/members",
                    get(unconfigured_handler).post(unconfigured_handler),
                )
                .route(
                    "/v1/chats/{chat_id}/members/{user_id}",
                    delete(unconfigured_handler),
                )
                // Plan 0055 (GAR-507) — messages slice 2, no-auth stub.
                .route(
                    "/v1/chats/{chat_id}/messages",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                // Plan 0057 (GAR-509) — threads slice 3, no-auth stub.
                .route(
                    "/v1/messages/{message_id}/threads",
                    post(unconfigured_handler),
                )
                // Plan 0062 (GAR-514) — memory API slice 1, no-auth stub.
                .route(
                    "/v1/memory",
                    get(unconfigured_handler).post(unconfigured_handler),
                )
                .route(
                    "/v1/memory/{id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                // Plan 0072 (GAR-526) — memory API slice 2: pin/unpin, no-auth stub.
                .route("/v1/memory/{id}/pin", post(unconfigured_handler))
                .route("/v1/memory/{id}/unpin", post(unconfigured_handler))
                // Plan 0066/0067/0069/0077 (GAR-516/GAR-518/GAR-520/GAR-533) — tasks API slices 1+2+3+4, no-auth stub.
                .route(
                    "/v1/groups/{group_id}/task-lists",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}",
                    patch(unconfigured_handler).delete(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/task-lists/{list_id}/tasks",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}",
                    get(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
                // Plan 0070 (GAR-522) — audit API slice 1, no-auth stub.
                .route("/v1/groups/{group_id}/audit", get(unconfigured_handler))
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}",
                    delete(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
                .route(
                    "/v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}",
                    delete(unconfigured_handler),
                )
                .route(
                    "/v1/uploads",
                    post(unconfigured_handler).options(uploads::options_uploads),
                )
                .route(
                    "/v1/uploads/{id}",
                    head(unconfigured_handler)
                        .patch(unconfigured_handler)
                        .delete(unconfigured_handler),
                )
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
