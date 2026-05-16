use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::middleware::{AuthenticatedAdmin, build_clear_cookie, build_session_cookie, extract_ip};
use super::rbac::{Action, Resource, Role, check_permission};
use super::secrets::redact_config_secrets;

// Slice 9.a (GAR-439): `AdminState` and `derive_encryption_key` extracted to
// `admin::shared`. Re-exported here so external paths
// `super::handlers::AdminState` and `super::handlers::derive_encryption_key`
// (consumed by `admin/routes.rs`) keep resolving without changes.
pub use super::shared::{AdminState, derive_encryption_key};

// Slice 9.b (GAR-470): provider console handlers extracted to `admin::providers`.
// Re-exported so `routes.rs` paths (`handlers::admin_list_providers`, etc.) keep resolving.
pub use super::providers::{
    SetProviderOverrideRequest, UpdateProviderSettingsRequest, admin_list_providers,
    disable_provider, enable_provider, list_provider_overrides, provider_failover, provider_health,
    set_provider_override, update_provider_settings,
};

// Slice 9.c (GAR-471): MCP server CRUD handlers extracted to `admin::mcp`.
// Re-exported so `routes.rs` paths (`handlers::admin_list_mcp`, etc.) keep resolving.
pub use super::mcp::{
    CreateMcpRequest, admin_create_mcp, admin_delete_mcp, admin_list_mcp, admin_restart_mcp,
};

// Slice 9.d (GAR-472): MCP template handlers extracted to `admin::mcp_templates`.
// Re-exported so `routes.rs` paths (`handlers::list_mcp_templates`, etc.) keep resolving.
pub use super::mcp_templates::{
    McpTemplate, delete_mcp_template, list_mcp_templates, save_mcp_template,
};

// Slice 9.e (GAR-473): Observability/UI handlers extracted to `admin::observability`.
// Re-exported so `routes.rs` paths (`handlers::admin_logs`, etc.) keep resolving.
pub use super::observability::{
    about, admin_alerts, admin_logs, admin_metrics, admin_prometheus, get_layout_preferences,
    list_templates, list_themes,
};

// ── Auth endpoints ──────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// POST /admin/api/login
pub async fn login(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let guard = state.store.lock().await;

    let user = match guard.verify_password(&body.username, &body.password) {
        Some(u) => u,
        None => {
            let _ = guard.append_audit(
                None,
                Some(&body.username),
                "login",
                "auth",
                None,
                Some("invalid credentials"),
                ip.as_deref(),
                "failure",
            );
            drop(guard);
            return (
                StatusCode::UNAUTHORIZED,
                HeaderMap::new(),
                Json(serde_json::json!({"error": "invalid credentials"})),
            );
        }
    };

    let session = match guard.create_session(&user.id, ip.as_deref(), None) {
        Ok(s) => s,
        Err(e) => {
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                Json(serde_json::json!({"error": format!("session creation failed: {e}")})),
            );
        }
    };

    let _ = guard.append_audit(
        Some(&user.id),
        Some(&user.username),
        "login",
        "auth",
        None,
        None,
        ip.as_deref(),
        "success",
    );
    drop(guard);

    let cookie = build_session_cookie(&session.token, 86400);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({
            "user": {
                "id": user.id,
                "username": user.username,
                "role": user.role,
            },
            "csrf_token": session.csrf_token,
            "expires_at": session.expires_at,
        })),
    )
}

/// POST /admin/api/logout
pub async fn logout(
    State(state): State<AdminState>,
    headers: HeaderMap,
    admin: Option<axum::Extension<AuthenticatedAdmin>>,
) -> impl IntoResponse {
    if let Some(axum::Extension(admin)) = admin {
        let guard = state.store.lock().await;
        let _ = guard.delete_session(&admin.session_token);
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "logout",
            "auth",
            None,
            None,
            extract_ip(&headers, None).as_deref(),
            "success",
        );
    }

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        axum::http::header::SET_COOKIE,
        build_clear_cookie().parse().unwrap(),
    );

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({"ok": true})),
    )
}

/// GET /admin/api/me — return current authenticated user info
pub async fn me(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "user": {
            "id": admin.user_id,
            "username": admin.username,
            "role": admin.role,
        },
        "csrf_token": admin.csrf_token,
    }))
}

// Slice 9.g (GAR-474): setup, user-management, and danger-zone handlers extracted to
// `admin::users`. Re-exported so `routes.rs` paths (`handlers::setup`, etc.) keep resolving.
pub use super::users::{
    CreateUserRequest, DangerZoneRequest, SetupRequest, UpdateUserRoleRequest, create_user,
    danger_zone, delete_user, list_users, setup, setup_status, update_user_role,
};

// Slice 9.f (GAR-475): secrets CRUD + rotation + migration + AES-256-GCM helpers extracted to
// `admin::secrets`. Re-exported so `routes.rs` paths (`handlers::set_secret`, etc.) keep resolving.
pub use super::secrets::{
    RotateSecretRequest, SetSecretRequest, delete_secret, list_secret_versions, list_secrets,
    migrate_secrets, rotate_secret, set_secret, test_secret,
};

// ── Audit log endpoints ──────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct AuditLogQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /// Filter by resource_type (exact match).
    pub resource_type: Option<String>,
    /// Filter by action (exact match).
    pub action: Option<String>,
    /// Filter by user_id (exact match) — Phase 7.1 extension.
    pub user_id: Option<String>,
    /// ISO-8601 lower bound on timestamp — Phase 7.1 extension.
    pub from: Option<String>,
    /// ISO-8601 upper bound on timestamp — Phase 7.1 extension.
    pub to: Option<String>,
}

/// GET /admin/api/audit-log
///
/// Query params: limit, offset, resource_type, action, user_id, from, to
pub async fn get_audit_log(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(query): axum::extract::Query<AuditLogQuery>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::AuditLog, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let limit = query.limit.unwrap_or(50).min(1000);
    let offset = query.offset.unwrap_or(0);

    let guard = state.store.lock().await;
    let entries = guard.list_audit_log_filtered(
        limit,
        offset,
        query.user_id.as_deref(),
        query.action.as_deref(),
        query.resource_type.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"entries": entries, "count": entries.len()})),
    )
}

// ── Config endpoints ─────────────────────────────────────────────────

/// GET /admin/api/config
pub async fn get_config(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let config = state.app_state.current_config();
    let mut config_safe = config.clone();
    redact_config_secrets(&mut config_safe);

    let yaml = match serde_yaml::to_string(&config_safe) {
        Ok(y) => y,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("serialization failed: {e}")})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({"config_yaml": yaml})),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct SaveConfigRequest {
    pub config_yaml: String,
    pub comment: Option<String>,
}

/// POST /admin/api/config — save new config version
pub async fn save_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SaveConfigRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    if serde_yaml::from_str::<garraia_config::AppConfig>(&body.config_yaml).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid YAML config"})),
        );
    }

    let guard = state.store.lock().await;
    match guard.save_config_version(
        &body.config_yaml,
        Some(&admin.username),
        body.comment.as_deref(),
    ) {
        Ok(version) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "save",
                "config",
                Some(&version.to_string()),
                body.comment.as_deref(),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"version": version})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// GET /admin/api/config/versions
pub async fn list_config_versions(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    let guard = state.store.lock().await;
    let versions = guard.list_config_versions(limit);
    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
    )
}

/// GET /admin/api/config/versions/{version}
pub async fn get_config_version(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(version): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    match guard.get_config_version(version) {
        Some(yaml) => (
            StatusCode::OK,
            Json(serde_json::json!({"version": version, "config_yaml": yaml})),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "version not found"})),
        ),
    }
}

/// GET /admin/api/permissions — return the full permissions matrix
pub async fn get_permissions_matrix(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> Json<serde_json::Value> {
    let resources = [
        "secrets",
        "config",
        "providers",
        "memory",
        "tools",
        "channels",
        "sessions",
        "audit_log",
        "users",
        "alerts",
        "metrics",
    ];
    let actions = ["read", "create", "update", "delete", "execute"];
    let roles = ["viewer", "operator", "admin"];

    let mut matrix = serde_json::Map::new();
    for role_name in &roles {
        let role = Role::from_str(role_name).unwrap();
        let mut role_perms = serde_json::Map::new();
        for resource_name in &resources {
            let resource = match *resource_name {
                "secrets" => Resource::Secrets,
                "config" => Resource::Config,
                "providers" => Resource::Providers,
                "memory" => Resource::Memory,
                "tools" => Resource::Tools,
                "channels" => Resource::Channels,
                "sessions" => Resource::Sessions,
                "audit_log" => Resource::AuditLog,
                "users" => Resource::Users,
                "alerts" => Resource::Alerts,
                "metrics" => Resource::Metrics,
                _ => continue,
            };
            let mut perms = serde_json::Map::new();
            for action_name in &actions {
                let action = match *action_name {
                    "read" => Action::Read,
                    "create" => Action::Create,
                    "update" => Action::Update,
                    "delete" => Action::Delete,
                    "execute" => Action::Execute,
                    _ => continue,
                };
                perms.insert(
                    action_name.to_string(),
                    serde_json::Value::Bool(check_permission(role, resource, action)),
                );
            }
            role_perms.insert(resource_name.to_string(), serde_json::Value::Object(perms));
        }
        matrix.insert(role_name.to_string(), serde_json::Value::Object(role_perms));
    }

    Json(serde_json::json!({
        "permissions": matrix,
        "current_role": admin.role,
    }))
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 4: Config Console (editor, versions, hot-reload, flags, ports, import/export)
// ═══════════════════════════════════════════════════════════════════════

/// POST /admin/api/config/apply — apply a config change (hot-reload when safe)
pub async fn apply_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SaveConfigRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Execute) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let new_config: garraia_config::AppConfig = match serde_yaml::from_str(&body.config_yaml) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("invalid YAML: {e}")})),
            );
        }
    };

    let current = state.app_state.current_config();
    let requires_restart = current.gateway.port != new_config.gateway.port
        || current.gateway.host != new_config.gateway.host;

    let config_path = garraia_config::ConfigLoader::default_config_dir().join("config.yml");
    if let Err(e) = std::fs::write(&config_path, &body.config_yaml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to write config: {e}")})),
        );
    }

    let guard = state.store.lock().await;
    let version = guard
        .save_config_version(
            &body.config_yaml,
            Some(&admin.username),
            body.comment.as_deref(),
        )
        .ok();
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "apply",
        "config",
        version.as_ref().map(|v| v.to_string()).as_deref(),
        Some(if requires_restart {
            "requires restart"
        } else {
            "hot-reloadable"
        }),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "version": version,
            "requires_restart": requires_restart,
            "message": if requires_restart { "Config saved. Some changes require a restart to take effect." } else { "Config applied. Changes will be picked up by hot-reload." },
        })),
    )
}

/// POST /admin/api/config/rollback/{version} — rollback to a previous config version
pub async fn rollback_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(version): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Execute) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let yaml = match guard.get_config_version(version) {
        Some(y) => y,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "version not found"})),
            );
        }
    };

    if serde_yaml::from_str::<garraia_config::AppConfig>(&yaml).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "stored config is invalid"})),
        );
    }

    let config_path = garraia_config::ConfigLoader::default_config_dir().join("config.yml");
    if let Err(e) = std::fs::write(&config_path, &yaml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("write failed: {e}")})),
        );
    }

    let new_version = guard
        .save_config_version(
            &yaml,
            Some(&admin.username),
            Some(&format!("rollback to v{version}")),
        )
        .ok();
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "rollback",
        "config",
        Some(&version.to_string()),
        Some(&format!("rolled back to version {version}")),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "rolled_back_to": version,
            "new_version": new_version,
            "requires_restart": true,
        })),
    )
}

/// GET /admin/api/config/flags — list feature flags (memory, tools, etc.)
pub async fn get_flags(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "flags": {
                "memory_enabled": config.memory.enabled,
                "shared_continuity": config.memory.shared_continuity,
                "has_embedding_provider": config.memory.embedding_provider.is_some(),
            }
        })),
    )
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UpdateFlagsRequest {
    pub memory_enabled: Option<bool>,
    pub shared_continuity: Option<bool>,
}

/// PUT /admin/api/config/flags
pub async fn update_flags(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<UpdateFlagsRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "update_flags",
        "config",
        None,
        Some(&serde_json::to_string(&body).unwrap_or_default()),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": true,
            "message": "Flag changes require a restart to take effect.",
        })),
    )
}

/// GET /admin/api/config/ports — current port configuration
pub async fn get_ports(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "gateway": {
                "host": config.gateway.host,
                "port": config.gateway.port,
            },
            "rate_limit": {
                "per_second": config.gateway.rate_limit.per_second,
                "burst_size": config.gateway.rate_limit.burst_size,
            }
        })),
    )
}

/// GET /admin/api/config/export — export current config as YAML
pub async fn export_config(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let mut config = state.app_state.current_config();
    redact_config_secrets(&mut config);

    match serde_yaml::to_string(&config) {
        Ok(yaml) => (
            StatusCode::OK,
            Json(serde_json::json!({"config_yaml": yaml})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

/// POST /admin/api/config/import — import config from YAML
pub async fn import_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SaveConfigRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    if serde_yaml::from_str::<garraia_config::AppConfig>(&body.config_yaml).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid YAML config"})),
        );
    }

    let guard = state.store.lock().await;
    match guard.save_config_version(
        &body.config_yaml,
        Some(&admin.username),
        Some("imported config"),
    ) {
        Ok(version) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "import",
                "config",
                Some(&version.to_string()),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"version": version, "ok": true})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 5: Ops (Memory, Tools, Channels, Sessions)
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/memory — browse memory entries
pub async fn admin_memory_browse(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Memory, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);
    let query_text = params.get("q").cloned();

    let memory = state.app_state.agents.memory_provider();
    let Some(provider) = memory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "memory not enabled"})),
        )
            .into_response();
    };

    let query = garraia_db::RecallQuery {
        tenant_id: None,
        query_text,
        query_embedding: None,
        session_id: None,
        continuity_key: None,
        limit,
    };

    match provider.recall(query).await {
        Ok(entries) => {
            let results: Vec<serde_json::Value> =
                entries.iter().map(memory_entry_to_json).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"entries": results, "count": results.len()})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

/// DELETE /admin/api/memory/{id} — delete memory for a session
pub async fn admin_memory_delete(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Memory, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let memory = state.app_state.agents.memory_provider();
    let Some(provider) = memory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "memory not enabled"})),
        );
    };

    match provider.delete_session_memory(&session_id).await {
        Ok(count) => {
            let guard = state.store.lock().await;
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "delete",
                "memory",
                Some(&session_id),
                Some(&format!("deleted {count} entries")),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok": true, "deleted_count": count})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /admin/api/memory/clear — clear memory for a session
pub async fn admin_memory_clear(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Memory, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let session_id = params
        .get("session_id")
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let memory = state.app_state.agents.memory_provider();
    let Some(provider) = memory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "memory not enabled"})),
        );
    };

    match provider.delete_session_memory(&session_id).await {
        Ok(count) => {
            let guard = state.store.lock().await;
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "clear",
                "memory",
                Some(&session_id),
                Some(&format!("cleared {count} entries")),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok": true, "deleted_count": count})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        ),
    }
}

/// POST /admin/api/memory/export — export memory entries as JSON
pub async fn admin_memory_export(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Memory, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let memory = state.app_state.agents.memory_provider();
    let Some(provider) = memory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "memory not enabled"})),
        )
            .into_response();
    };

    let query = garraia_db::RecallQuery {
        tenant_id: None,
        query_text: None,
        query_embedding: None,
        session_id: None,
        continuity_key: None,
        limit: 10000,
    };

    match provider.recall(query).await {
        Ok(entries) => {
            let results: Vec<serde_json::Value> =
                entries.iter().map(memory_entry_to_json).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"entries": results, "count": results.len()})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

/// GET /admin/api/memory/health — memory provider health status
pub async fn admin_memory_health(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Memory, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let memory = state.app_state.agents.memory_provider();

    let status = if let Some(provider) = memory {
        let probe = garraia_db::RecallQuery {
            tenant_id: None,
            query_text: None,
            query_embedding: None,
            session_id: None,
            continuity_key: None,
            limit: 1,
        };
        let healthy = provider.recall(probe).await.is_ok();
        serde_json::json!({
            "enabled": config.memory.enabled,
            "healthy": healthy,
            "embedding_provider": config.memory.embedding_provider,
            "shared_continuity": config.memory.shared_continuity,
        })
    } else {
        serde_json::json!({
            "enabled": config.memory.enabled,
            "healthy": false,
            "embedding_provider": config.memory.embedding_provider,
            "error": "memory provider not initialized",
        })
    };

    (StatusCode::OK, Json(status))
}

/// GET /admin/api/tools — list all registered tools
pub async fn admin_list_tools(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Tools, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tools = state.app_state.agents.list_tool_info();
    let tool_list: Vec<serde_json::Value> = tools
        .iter()
        .map(|(name, desc)| {
            serde_json::json!({
                "name": name,
                "description": desc,
                "enabled": true,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"tools": tool_list})),
    )
}

/// GET /admin/api/channels — list all channels with status
pub async fn admin_list_channels(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Channels, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let channels: Vec<String> = state
        .app_state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let config = state.app_state.current_config();

    let mut channel_info: Vec<serde_json::Value> = Vec::new();
    for (name, cfg) in &config.channels {
        let connected = channels.iter().any(|c| c.contains(name));
        channel_info.push(serde_json::json!({
            "name": name,
            "type": cfg.channel_type,
            "enabled": cfg.enabled.unwrap_or(true),
            "connected": connected,
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"channels": channel_info})),
    )
}

/// GET /admin/api/sessions — list active sessions
pub async fn admin_list_sessions(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Sessions, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let sessions: Vec<serde_json::Value> = state
        .app_state
        .sessions
        .iter()
        .map(|entry| {
            let s = entry.value();
            serde_json::json!({
                "id": s.id,
                "tenant_id": s.tenant_id,
                "user_id": s.user_id,
                "channel_id": s.channel_id,
                "connected": s.connected,
                "history_len": s.history.len(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sessions": sessions,
            "count": sessions.len(),
        })),
    )
}

/// DELETE /admin/api/sessions/{id} — disconnect a session
pub async fn admin_disconnect_session(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Sessions, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    state.app_state.disconnect_session(&session_id);

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "disconnect",
        "session",
        Some(&session_id),
        None,
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}

// ── Glob config (GAR-264) ────────────────────────────────────────────────────

/// GET /admin/api/config/glob — return current FsConfig (glob + ignore settings).
pub async fn admin_glob_config(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let fs = &config.fs;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "glob": {
                "mode": fs.glob.mode,
                "dot": fs.glob.dot,
            },
            "ignore": {
                "use_gitignore": fs.ignore.use_gitignore,
            }
        })),
    )
}

#[derive(serde::Deserialize)]
pub struct GlobTestRequest {
    /// Glob pattern to test (e.g. `**/*.rs`).
    pub pattern: String,
    /// List of relative paths to match against.
    pub paths: Vec<String>,
    /// Override dot option (defaults to config value).
    pub dot: Option<bool>,
}

/// POST /admin/api/config/glob/test — live glob pattern tester.
///
/// Tests a single glob pattern against a list of paths using the current
/// FsGlobConfig mode and returns which paths matched.
pub async fn admin_glob_test(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<GlobTestRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let fs = &config.fs;
    let dot = body.dot.unwrap_or(fs.glob.dot);

    let options = garraia_glob::MatchOptions {
        case_sensitive: true,
        dot,
        ..Default::default()
    };

    let matcher = match garraia_glob::GlobMatcher::new(vec![body.pattern.clone()], options) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": format!("invalid pattern: {e}")})),
            );
        }
    };

    let matches: Vec<&str> = body
        .paths
        .iter()
        .filter(|p| matcher.matches(p))
        .map(String::as_str)
        .collect();

    let total = body.paths.len();
    let matched = matches.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "pattern": body.pattern,
            "mode": fs.glob.mode,
            "dot": dot,
            "total": total,
            "matched": matched,
            "matches": matches,
        })),
    )
}

fn memory_entry_to_json(entry: &garraia_db::MemoryEntry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "tenant_id": entry.tenant_id,
        "session_id": entry.session_id,
        "channel_id": entry.channel_id,
        "user_id": entry.user_id,
        "role": format!("{:?}", entry.role),
        "content": entry.content,
        "created_at": entry.created_at,
    })
}
