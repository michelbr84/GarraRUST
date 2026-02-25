use std::sync::Arc;

use axum::Router;
use axum::middleware as axum_mw;
use axum::routing::{delete, get, post, put};
use tokio::sync::Mutex;

use super::handlers::{self, AdminState};
use super::middleware::{require_admin_auth, require_csrf, security_headers};
use super::store::AdminStore;
use crate::state::SharedState;

/// Build the admin sub-router mounted at `/admin`.
pub fn build_admin_router(app_state: SharedState, admin_store: Arc<Mutex<AdminStore>>) -> Router {
    let encryption_key = Arc::new(handlers::derive_encryption_key());

    let admin_state = AdminState {
        store: Arc::clone(&admin_store),
        app_state,
        encryption_key,
    };

    let public_routes = Router::new()
        .route("/api/login", post(handlers::login))
        .route("/api/setup", post(handlers::setup))
        .route("/api/setup/status", get(handlers::setup_status))
        .with_state(admin_state.clone());

    let auth_routes = Router::new()
        .route("/api/logout", post(handlers::logout))
        .route("/api/me", get(handlers::me))
        // User management
        .route(
            "/api/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route("/api/users/{id}/role", put(handlers::update_user_role))
        .route("/api/users/{id}", delete(handlers::delete_user))
        // Danger zone
        .route("/api/danger-zone", post(handlers::danger_zone))
        // Audit log
        .route("/api/audit-log", get(handlers::get_audit_log))
        // Permissions
        .route("/api/permissions", get(handlers::get_permissions_matrix))
        // Secrets
        .route(
            "/api/secrets",
            get(handlers::list_secrets).post(handlers::set_secret),
        )
        .route(
            "/api/secrets/{provider}/{key_name}",
            delete(handlers::delete_secret),
        )
        .route(
            "/api/secrets/{provider}/{key_name}/test",
            get(handlers::test_secret),
        )
        .route(
            "/api/secrets/{id}/versions",
            get(handlers::list_secret_versions),
        )
        // Config
        .route(
            "/api/config",
            get(handlers::get_config).post(handlers::save_config),
        )
        .route("/api/config/versions", get(handlers::list_config_versions))
        .route(
            "/api/config/versions/{version}",
            get(handlers::get_config_version),
        )
        .layer(axum_mw::from_fn(require_csrf))
        .layer(axum_mw::from_fn(require_admin_auth))
        .layer(axum::Extension(admin_store))
        .with_state(admin_state);

    Router::new()
        .merge(public_routes)
        .merge(auth_routes)
        .layer(axum_mw::from_fn(security_headers))
}
