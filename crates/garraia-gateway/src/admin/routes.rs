use std::sync::Arc;

use axum::Router;
use axum::middleware as axum_mw;
use axum::response::Html;
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
        .route("/api/about", get(handlers::about))
        .route("/api/themes", get(handlers::list_themes))
        .route("/api/layout", get(handlers::get_layout_preferences))
        .with_state(admin_state.clone());

    let auth_routes = Router::new()
        .route("/api/logout", post(handlers::logout))
        .route("/api/me", get(handlers::me))
        // ── Phase 1: User management ──
        .route(
            "/api/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route("/api/users/{id}/role", put(handlers::update_user_role))
        .route("/api/users/{id}", delete(handlers::delete_user))
        .route("/api/danger-zone", post(handlers::danger_zone))
        .route("/api/audit-log", get(handlers::get_audit_log))
        .route("/api/permissions", get(handlers::get_permissions_matrix))
        // ── Phase 2: Secrets ──
        .route(
            "/api/secrets",
            get(handlers::list_secrets).post(handlers::set_secret),
        )
        .route("/api/secrets/rotate", post(handlers::rotate_secret))
        .route("/api/secrets/migrate", post(handlers::migrate_secrets))
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
        // ── Phase 3: Providers ──
        .route("/api/providers", get(handlers::admin_list_providers))
        .route(
            "/api/providers/overrides",
            get(handlers::list_provider_overrides).post(handlers::set_provider_override),
        )
        .route(
            "/api/providers/{id}/settings",
            put(handlers::update_provider_settings),
        )
        .route("/api/providers/{id}/health", get(handlers::provider_health))
        .route(
            "/api/providers/{id}/enable",
            post(handlers::enable_provider),
        )
        .route(
            "/api/providers/{id}/disable",
            post(handlers::disable_provider),
        )
        .route(
            "/api/providers/{id}/failover",
            get(handlers::provider_failover),
        )
        // ── Phase 4: Config ──
        .route(
            "/api/config",
            get(handlers::get_config).post(handlers::save_config),
        )
        .route("/api/config/apply", post(handlers::apply_config))
        .route("/api/config/versions", get(handlers::list_config_versions))
        .route(
            "/api/config/versions/{version}",
            get(handlers::get_config_version),
        )
        .route(
            "/api/config/rollback/{version}",
            post(handlers::rollback_config),
        )
        .route(
            "/api/config/flags",
            get(handlers::get_flags).put(handlers::update_flags),
        )
        .route("/api/config/ports", get(handlers::get_ports))
        .route("/api/config/export", get(handlers::export_config))
        .route("/api/config/import", post(handlers::import_config))
        // ── GAR-264: Glob settings ──
        .route("/api/config/glob", get(handlers::admin_glob_config))
        .route("/api/config/glob/test", post(handlers::admin_glob_test))
        // ── Phase 5: Memory ──
        .route("/api/memory", get(handlers::admin_memory_browse))
        .route("/api/memory/clear", post(handlers::admin_memory_clear))
        .route("/api/memory/export", post(handlers::admin_memory_export))
        .route("/api/memory/health", get(handlers::admin_memory_health))
        .route("/api/memory/{id}", delete(handlers::admin_memory_delete))
        // ── Phase 5: Tools ──
        .route("/api/tools", get(handlers::admin_list_tools))
        // ── Phase 5: Channels & Sessions ──
        .route("/api/channels", get(handlers::admin_list_channels))
        .route("/api/sessions", get(handlers::admin_list_sessions))
        .route(
            "/api/sessions/{id}",
            delete(handlers::admin_disconnect_session),
        )
        // ── Phase 6: Observability ──
        .route("/api/logs", get(handlers::admin_logs))
        .route("/api/metrics", get(handlers::admin_metrics))
        .route("/api/metrics/prometheus", get(handlers::admin_prometheus))
        .route("/api/alerts", get(handlers::admin_alerts))
        // ── MCP server management ──
        .route(
            "/api/mcp",
            get(handlers::admin_list_mcp).post(handlers::admin_create_mcp),
        )
        .route("/api/mcp/{id}", delete(handlers::admin_delete_mcp))
        .route("/api/mcp/{id}/restart", post(handlers::admin_restart_mcp))
        // ── Agent templates (system prompts / named agents from config) ──
        .route("/api/templates", get(handlers::list_templates))
        // ── Phase 6: MCP Templates (GAR-296/297) ──
        .route(
            "/api/mcp/templates",
            get(handlers::list_mcp_templates).post(handlers::save_mcp_template),
        )
        .route("/api/mcp/templates/{id}", delete(handlers::delete_mcp_template))
        .layer(axum_mw::from_fn(require_csrf))
        .layer(axum_mw::from_fn(require_admin_auth))
        .layer(axum::Extension(admin_store))
        .with_state(admin_state);

    Router::new()
        .merge(public_routes)
        .merge(auth_routes)
        .layer(axum_mw::from_fn(security_headers))
}

/// Serve the admin SPA. Called both from nested router (/) and top-level (/admin).
pub async fn admin_page_handler() -> Html<String> {
    admin_page().await
}

async fn admin_page() -> Html<String> {
    if let Ok(content) = std::fs::read_to_string("crates/garraia-gateway/src/admin.html") {
        return Html(content);
    }
    Html(include_str!("../admin.html").to_string())
}
