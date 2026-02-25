use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use tokio::sync::Mutex;

use super::middleware::{AuthenticatedAdmin, build_clear_cookie, build_session_cookie, extract_ip};
use super::rbac::{Action, Resource, Role, check_permission};
use super::store::AdminStore;
use crate::state::SharedState;

/// Shared state for admin API handlers.
#[derive(Clone)]
pub struct AdminState {
    pub store: Arc<Mutex<AdminStore>>,
    pub app_state: SharedState,
    /// Master encryption key (derived or loaded at startup) for secrets encryption.
    pub encryption_key: Arc<Vec<u8>>,
}

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

// ── Setup endpoint (first-run bootstrap) ─────────────────────────────

#[derive(serde::Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

/// POST /admin/api/setup — create the first admin user (only works when no users exist)
pub async fn setup(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> impl IntoResponse {
    let guard = state.store.lock().await;

    if guard.user_count() > 0 {
        return (
            StatusCode::CONFLICT,
            HeaderMap::new(),
            Json(serde_json::json!({"error": "setup already completed"})),
        );
    }

    if body.username.len() < 3 || body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Json(serde_json::json!({"error": "username must be >=3 chars, password >=8 chars"})),
        );
    }

    let user = match guard.create_user(&body.username, &body.password, Role::Admin) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                Json(serde_json::json!({"error": e})),
            );
        }
    };

    let ip = extract_ip(&headers, None);
    let session = match guard.create_session(&user.id, ip.as_deref(), None) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                Json(serde_json::json!({"error": e})),
            );
        }
    };

    let _ = guard.append_audit(
        Some(&user.id),
        Some(&user.username),
        "setup",
        "auth",
        None,
        Some("initial admin user created"),
        ip.as_deref(),
        "success",
    );
    drop(guard);

    let cookie = build_session_cookie(&session.token, 86400);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());

    (
        StatusCode::CREATED,
        resp_headers,
        Json(serde_json::json!({
            "user": {
                "id": user.id,
                "username": user.username,
                "role": user.role,
            },
            "csrf_token": session.csrf_token,
        })),
    )
}

/// GET /admin/api/setup/status — check if setup is needed
pub async fn setup_status(State(state): State<AdminState>) -> Json<serde_json::Value> {
    let guard = state.store.lock().await;
    let needs_setup = guard.user_count() == 0;
    Json(serde_json::json!({ "needs_setup": needs_setup }))
}

// ── User management endpoints ────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

/// POST /admin/api/users
pub async fn create_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<CreateUserRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Users, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let role = match Role::from_str(&body.role) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid role"})),
            );
        }
    };

    if body.username.len() < 3 || body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "username >=3 chars, password >=8 chars"})),
        );
    }

    let guard = state.store.lock().await;
    match guard.create_user(&body.username, &body.password, role) {
        Ok(user) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "create",
                "user",
                Some(&user.id),
                Some(&format!(
                    "created user '{}' with role '{}'",
                    user.username,
                    role.as_str()
                )),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (StatusCode::CREATED, Json(serde_json::json!({"user": user})))
        }
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": e}))),
    }
}

/// GET /admin/api/users
pub async fn list_users(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Users, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let users = guard.list_users();
    (StatusCode::OK, Json(serde_json::json!({"users": users})))
}

#[derive(serde::Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

/// PUT /admin/api/users/{id}/role
pub async fn update_user_role(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(body): Json<UpdateUserRoleRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Users, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let role = match Role::from_str(&body.role) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid role"})),
            );
        }
    };

    let guard = state.store.lock().await;
    match guard.update_user_role(&user_id, role) {
        Ok(()) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "update_role",
                "user",
                Some(&user_id),
                Some(&format!("changed role to '{}'", role.as_str())),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (StatusCode::OK, Json(serde_json::json!({"ok": true})))
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

/// DELETE /admin/api/users/{id}
pub async fn delete_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Users, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    if user_id == admin.user_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "cannot delete yourself"})),
        );
    }

    let guard = state.store.lock().await;
    match guard.delete_user(&user_id) {
        Ok(()) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "delete",
                "user",
                Some(&user_id),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (StatusCode::OK, Json(serde_json::json!({"ok": true})))
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

// ── Danger zone (re-auth required) ───────────────────────────────────

#[derive(serde::Deserialize)]
pub struct DangerZoneRequest {
    pub password: String,
    pub action: String,
    #[serde(default)]
    pub target_id: Option<String>,
}

/// POST /admin/api/danger-zone — execute destructive actions with password re-confirmation
pub async fn danger_zone(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<DangerZoneRequest>,
) -> impl IntoResponse {
    if admin.role != Role::Admin {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin role required"})),
        );
    }

    let guard = state.store.lock().await;
    let verified = guard.verify_password(&admin.username, &body.password);
    if verified.is_none() {
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            &body.action,
            "danger_zone",
            body.target_id.as_deref(),
            Some("re-auth failed"),
            extract_ip(&headers, None).as_deref(),
            "failure",
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "password verification failed"})),
        );
    }

    let result = match body.action.as_str() {
        "reset_all_sessions" => {
            let users = guard.list_users();
            for user in &users {
                let _ = guard.delete_user_sessions(&user.id);
            }
            Ok("all sessions cleared".to_string())
        }
        "delete_all_secrets" => {
            let secrets = guard.list_secrets("default");
            for secret in &secrets {
                let _ = guard.delete_secret(&secret.tenant_id, &secret.provider, &secret.key_name);
            }
            Ok(format!("{} secrets deleted", secrets.len()))
        }
        "delete_user" => {
            if let Some(target_id) = &body.target_id {
                if target_id == &admin.user_id {
                    Err("cannot delete yourself".to_string())
                } else {
                    guard
                        .delete_user(target_id)
                        .map(|_| "user deleted".to_string())
                }
            } else {
                Err("target_id required".to_string())
            }
        }
        _ => Err(format!("unknown danger zone action: {}", body.action)),
    };

    match result {
        Ok(msg) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                &body.action,
                "danger_zone",
                body.target_id.as_deref(),
                Some(&msg),
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"ok": true, "message": msg})),
            )
        }
        Err(e) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                &body.action,
                "danger_zone",
                body.target_id.as_deref(),
                Some(&e),
                extract_ip(&headers, None).as_deref(),
                "failure",
            );
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            )
        }
    }
}

// ── Audit log endpoints ──────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct AuditLogQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub resource_type: Option<String>,
    pub action: Option<String>,
}

/// GET /admin/api/audit-log
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

    let guard = state.store.lock().await;
    let entries = guard.list_audit_log(
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
        query.resource_type.as_deref(),
        query.action.as_deref(),
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"entries": entries})),
    )
}

// ── Secrets endpoints ────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct SetSecretRequest {
    pub provider: String,
    pub key_name: String,
    pub value: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
}

fn default_tenant() -> String {
    "default".to_string()
}

/// POST /admin/api/secrets — create or update a secret (never returns the value)
pub async fn set_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SetSecretRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let (encrypted, nonce) = match encrypt_value(body.value.as_bytes(), &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("encryption failed: {e}")})),
            );
        }
    };

    let guard = state.store.lock().await;
    match guard.set_secret(
        &body.tenant_id,
        &body.provider,
        &body.key_name,
        &encrypted,
        &nonce,
        Some(&admin.username),
    ) {
        Ok(id) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "set",
                "secret",
                Some(&format!("{}/{}", body.provider, body.key_name)),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"id": id, "is_set": true})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// GET /admin/api/secrets — list secrets (only metadata, NEVER values)
pub async fn list_secrets(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;
    let secrets = guard.list_secrets(tenant_id);

    let result: Vec<serde_json::Value> = secrets
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "tenant_id": s.tenant_id,
                "provider": s.provider,
                "key_name": s.key_name,
                "is_set": s.is_set,
                "version": s.version,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({"secrets": result})))
}

/// DELETE /admin/api/secrets/{provider}/{key_name}
pub async fn delete_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path((provider, key_name)): axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;
    match guard.delete_secret(tenant_id, &provider, &key_name) {
        Ok(()) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "delete",
                "secret",
                Some(&format!("{provider}/{key_name}")),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (StatusCode::OK, Json(serde_json::json!({"ok": true})))
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

/// GET /admin/api/secrets/{provider}/{key_name}/test — test a stored secret
pub async fn test_secret(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path((provider, key_name)): axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;

    let raw = guard.get_secret_raw(tenant_id, &provider, &key_name);
    drop(guard);

    let Some((encrypted, nonce)) = raw else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "secret not found"})),
        );
    };

    let decrypted = match decrypt_value(&encrypted, &nonce, &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("decryption failed: {e}")})),
            );
        }
    };

    let is_valid = !decrypted.is_empty();
    let value_len = decrypted.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider,
            "key_name": key_name,
            "is_valid": is_valid,
            "value_length": value_len,
        })),
    )
}

/// GET /admin/api/secrets/{id}/versions — list secret versions
pub async fn list_secret_versions(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(secret_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let versions = guard.list_secret_versions(&secret_id);
    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
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

// ── Encryption helpers ───────────────────────────────────────────────

fn encrypt_value(plaintext: &[u8], key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| "failed to create encryption key".to_string())?;
    let aead_key = LessSafeKey::new(unbound);

    let rng = SystemRandom::new();
    let mut nonce_bytes = vec![0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| "failed to generate nonce".to_string())?;

    let nonce =
        Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|_| "invalid nonce".to_string())?;

    let mut in_out = plaintext.to_vec();
    aead_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "encryption failed".to_string())?;

    Ok((in_out, nonce_bytes))
}

fn decrypt_value(ciphertext: &[u8], nonce_bytes: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| "failed to create decryption key".to_string())?;
    let aead_key = LessSafeKey::new(unbound);

    let nonce =
        Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| "invalid nonce".to_string())?;

    let mut in_out = ciphertext.to_vec();
    let plaintext = aead_key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "decryption failed".to_string())?;

    Ok(plaintext.to_vec())
}

fn redact_config_secrets(config: &mut garraia_config::AppConfig) {
    config.gateway.api_key = config
        .gateway
        .api_key
        .as_ref()
        .map(|_| "***REDACTED***".to_string());

    for (_, llm) in config.llm.iter_mut() {
        llm.api_key = llm.api_key.as_ref().map(|_| "***REDACTED***".to_string());
    }

    for (_, emb) in config.embeddings.iter_mut() {
        emb.api_key = emb.api_key.as_ref().map(|_| "***REDACTED***".to_string());
    }

    for (_, ch) in config.channels.iter_mut() {
        for (key, val) in ch.settings.iter_mut() {
            let lower = key.to_lowercase();
            if lower.contains("token")
                || lower.contains("key")
                || lower.contains("secret")
                || lower.contains("password")
            {
                *val = serde_json::json!("***REDACTED***");
            }
        }
    }
}

/// Derive or generate a master encryption key for the admin secrets store.
pub fn derive_encryption_key() -> Vec<u8> {
    if let Ok(passphrase) = std::env::var("GARRAIA_ADMIN_KEY") {
        let salt = b"garraia-admin-secrets-v1";
        let iterations = std::num::NonZeroU32::new(100_000).unwrap();
        let mut key = vec![0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            passphrase.as_bytes(),
            &mut key,
        );
        return key;
    }

    if let Ok(passphrase) = std::env::var("GARRAIA_VAULT_PASSPHRASE") {
        let salt = b"garraia-admin-secrets-v1";
        let iterations = std::num::NonZeroU32::new(100_000).unwrap();
        let mut key = vec![0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            passphrase.as_bytes(),
            &mut key,
        );
        return key;
    }

    let key_path = garraia_config::ConfigLoader::default_config_dir()
        .join("admin")
        .join("master.key");

    if let Ok(data) = std::fs::read(&key_path) {
        if data.len() == 32 {
            return data;
        }
    }

    let rng = SystemRandom::new();
    let mut key = vec![0u8; 32];
    rng.fill(&mut key).expect("failed to generate master key");

    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&key_path, &key);

    key
}
