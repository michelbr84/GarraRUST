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

// ═══════════════════════════════════════════════════════════════════════
// Phase 2 extras: secret rotation, test connection, migration
// ═══════════════════════════════════════════════════════════════════════

#[derive(serde::Deserialize)]
pub struct RotateSecretRequest {
    pub provider: String,
    pub key_name: String,
    pub new_value: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
}

/// POST /admin/api/secrets/rotate — rotate a secret (archives current version)
pub async fn rotate_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<RotateSecretRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let (encrypted, nonce) = match encrypt_value(body.new_value.as_bytes(), &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
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
                "rotate",
                "secret",
                Some(&format!("{}/{}", body.provider, body.key_name)),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"id": id, "rotated": true})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// POST /admin/api/secrets/migrate — migrate secrets from config.yml/env to the secrets store
pub async fn migrate_secrets(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let mut migrated = Vec::new();

    let env_keys = [
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("mistral", "MISTRAL_API_KEY"),
        ("gemini", "GEMINI_API_KEY"),
        ("cohere", "COHERE_API_KEY"),
        ("falcon", "FALCON_API_KEY"),
        ("jais", "JAIS_API_KEY"),
        ("qwen", "QWEN_API_KEY"),
        ("yi", "YI_API_KEY"),
        ("minimax", "MINIMAX_API_KEY"),
        ("moonshot", "MOONSHOT_API_KEY"),
        ("sansa", "SANSA_API_KEY"),
    ];

    let guard = state.store.lock().await;

    for (provider, env_var) in &env_keys {
        let api_key = config
            .llm
            .get(*provider)
            .and_then(|c| c.api_key.clone())
            .or_else(|| std::env::var(env_var).ok());

        if let Some(key) = api_key {
            if key.is_empty() || key == "***REDACTED***" {
                continue;
            }
            match encrypt_value(key.as_bytes(), &state.encryption_key) {
                Ok((encrypted, nonce)) => {
                    if guard
                        .set_secret(
                            "default",
                            provider,
                            "api_key",
                            &encrypted,
                            &nonce,
                            Some(&admin.username),
                        )
                        .is_ok()
                    {
                        migrated.push(format!("{provider}/api_key"));
                    }
                }
                Err(_) => continue,
            }
        }
    }

    for (name, ch) in &config.channels {
        for (key, val) in &ch.settings {
            let lower = key.to_lowercase();
            if lower.contains("token") || lower.contains("key") || lower.contains("secret") {
                if let Some(s) = val.as_str() {
                    if s.is_empty() || s == "***REDACTED***" {
                        continue;
                    }
                    if let Ok((encrypted, nonce)) =
                        encrypt_value(s.as_bytes(), &state.encryption_key)
                    {
                        if guard
                            .set_secret(
                                "default",
                                &format!("channel:{name}"),
                                key,
                                &encrypted,
                                &nonce,
                                Some(&admin.username),
                            )
                            .is_ok()
                        {
                            migrated.push(format!("channel:{name}/{key}"));
                        }
                    }
                }
            }
        }
    }

    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "migrate",
        "secret",
        None,
        Some(&format!("migrated {} secrets", migrated.len())),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "migrated": migrated,
            "count": migrated.len(),
        })),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 3: Providers Console
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/providers — list all known providers with status
pub async fn admin_list_providers(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let active_ids = state.app_state.agents.provider_ids();
    let default_id = state.app_state.agents.default_provider_id();
    let config = state.app_state.current_config();

    let known_providers = [
        ("anthropic", "Anthropic", true),
        ("openai", "OpenAI", true),
        ("openrouter", "OpenRouter", true),
        ("deepseek", "DeepSeek", true),
        ("mistral", "Mistral", true),
        ("sansa", "Sansa", true),
        ("gemini", "Google Gemini", true),
        ("falcon", "Falcon", true),
        ("jais", "Jais", true),
        ("qwen", "Qwen", true),
        ("yi", "Yi", true),
        ("cohere", "Cohere", true),
        ("minimax", "MiniMax", true),
        ("moonshot", "Moonshot K2", true),
        ("ollama", "Ollama", false),
    ];

    let mut providers = Vec::new();
    for (id, display, needs_key) in &known_providers {
        let active = active_ids.contains(&id.to_string());
        let mut model = None;
        let mut models = Vec::new();
        let has_secret = {
            let guard = state.store.lock().await;
            guard.get_secret_meta("default", id, "api_key").is_some()
        };

        if active {
            if let Some(provider) = state.app_state.agents.get_provider(id) {
                model = provider.configured_model().map(|m| m.to_string());
                if let Ok(mut available) = provider.available_models().await {
                    available.retain(|m| !m.trim().is_empty());
                    available.sort();
                    available.dedup();
                    models = available;
                }
            }
        }

        let config_entry = config.llm.get(*id);

        providers.push(serde_json::json!({
            "id": id,
            "display_name": display,
            "active": active,
            "is_default": default_id.as_deref() == Some(*id),
            "needs_api_key": *needs_key,
            "has_secret": has_secret,
            "model": model,
            "models": models,
            "base_url": config_entry.and_then(|c| c.base_url.clone()),
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"providers": providers})),
    )
        .into_response()
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UpdateProviderSettingsRequest {
    pub enabled: Option<bool>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub set_default: Option<bool>,
}

/// PUT /admin/api/providers/{id}/settings — update provider settings
pub async fn update_provider_settings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
    Json(body): Json<UpdateProviderSettingsRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    if body.set_default == Some(true) {
        state.app_state.agents.set_default_provider_id(&provider_id);
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "update_settings",
        "provider",
        Some(&provider_id),
        Some(&serde_json::to_string(&body).unwrap_or_default()),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    let requires_restart = body.model.is_some() || body.base_url.is_some();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": requires_restart,
        })),
    )
}

/// GET /admin/api/providers/{id}/health — provider health check
pub async fn provider_health(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let provider = state.app_state.agents.get_provider(&provider_id);
    match provider {
        Some(p) => {
            let model = p.configured_model().map(|m| m.to_string());
            let models_result = p.available_models().await;
            let healthy = models_result.is_ok();
            let model_count = models_result.map(|m| m.len()).unwrap_or(0);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "provider": provider_id,
                    "healthy": healthy,
                    "model": model,
                    "available_models": model_count,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "provider": provider_id,
                "healthy": false,
                "error": "provider not active",
            })),
        ),
    }
}

/// POST /admin/api/providers/{id}/enable
pub async fn enable_provider(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "enable",
        "provider",
        Some(&provider_id),
        None,
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": true,
            "message": format!("Provider '{}' will be enabled on next restart or config reload", provider_id),
        })),
    )
}

/// POST /admin/api/providers/{id}/disable
pub async fn disable_provider(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "disable",
        "provider",
        Some(&provider_id),
        None,
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "requires_restart": true,
            "message": format!("Provider '{}' will be disabled on next restart", provider_id),
        })),
    )
}

/// GET /admin/api/providers/{id}/failover — get failover/resilience status
pub async fn provider_failover(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(provider_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let active_ids = state.app_state.agents.provider_ids();
    let default_id = state.app_state.agents.default_provider_id();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider_id,
            "active_providers": active_ids,
            "default_provider": default_id,
            "circuit_breaker": {
                "status": if active_ids.contains(&provider_id) { "closed" } else { "open" },
            },
        })),
    )
}

/// GET /admin/api/providers/overrides — per-tenant provider overrides
pub async fn list_provider_overrides(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let overrides: Vec<serde_json::Value> = state
        .app_state
        .channel_models
        .iter()
        .map(|entry| {
            serde_json::json!({
                "channel": entry.key().clone(),
                "model": entry.value().clone(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"overrides": overrides})),
    )
}

#[derive(serde::Deserialize)]
pub struct SetProviderOverrideRequest {
    pub channel: String,
    pub model: String,
}

/// POST /admin/api/providers/overrides
pub async fn set_provider_override(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SetProviderOverrideRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Providers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    state
        .app_state
        .channel_models
        .insert(body.channel.clone(), body.model.clone());

    let guard = state.store.lock().await;
    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "set_override",
        "provider",
        Some(&body.channel),
        Some(&format!("model={}", body.model)),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
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

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Observability/UI
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/logs — stream recent log entries
pub async fn admin_logs(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Sessions, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);
    let log_path = dirs::home_dir()
        .map(|h| h.join(".garraia").join("garraia.log"))
        .unwrap_or_default();

    if !log_path.exists() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"lines": [], "count": 0})),
        )
            .into_response();
    }

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().rev().take(limit).collect();
            let lines: Vec<&str> = lines.into_iter().rev().collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"lines": lines, "count": lines.len()})),
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

/// GET /admin/api/metrics — current metrics snapshot
pub async fn admin_metrics(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let metrics = crate::observability::global_metrics();
    let active_sessions = state.app_state.sessions.len();
    let active_providers = state.app_state.agents.provider_ids();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "requests_total": metrics.requests_total.load(std::sync::atomic::Ordering::Relaxed),
            "active_sessions": active_sessions,
            "active_providers": active_providers,
            "provider_count": active_providers.len(),
        })),
    )
        .into_response()
}

/// GET /admin/api/metrics/prometheus — raw prometheus format
pub async fn admin_prometheus(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (StatusCode::FORBIDDEN, "insufficient permissions").into_response();
    }

    let body = crate::observability::global_metrics().render_prometheus();
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}

/// GET /admin/api/alerts — basic alerts (provider down, high error rate)
pub async fn admin_alerts(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Alerts, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let mut alerts: Vec<serde_json::Value> = Vec::new();

    let active_ids = state.app_state.agents.provider_ids();
    if active_ids.is_empty() {
        alerts.push(serde_json::json!({
            "level": "warning",
            "source": "providers",
            "message": "No LLM providers are active",
        }));
    }

    let config = state.app_state.current_config();
    if !config.memory.enabled {
        alerts.push(serde_json::json!({
            "level": "info",
            "source": "memory",
            "message": "Memory system is disabled",
        }));
    }

    if config.gateway.api_key.is_none() {
        alerts.push(serde_json::json!({
            "level": "warning",
            "source": "security",
            "message": "No API key configured for the gateway",
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"alerts": alerts, "count": alerts.len()})),
    )
}

/// GET /admin/api/themes — available UI themes
pub async fn list_themes() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "themes": [
            {"id": "dark", "name": "Dark", "description": "Dark theme"},
            {"id": "light", "name": "Light", "description": "Light theme"},
            {"id": "brasil", "name": "Brasil", "description": "Green and gold accent"},
        ],
        "current": "dark",
    }))
}

/// GET /admin/api/layout — layout preferences
pub async fn get_layout_preferences() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "sidebar_compact": false,
        "density": "comfortable",
        "shortcuts": {
            "toggle_sidebar": "Ctrl+B",
            "search": "Ctrl+K",
            "settings": "Ctrl+,",
        }
    }))
}

/// GET /admin/api/templates — list prompt/persona templates
pub async fn list_templates(
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
    let mut templates: Vec<serde_json::Value> = Vec::new();

    if let Some(prompt) = &config.agent.system_prompt {
        templates.push(serde_json::json!({
            "id": "default",
            "name": "Default Agent",
            "system_prompt_preview": if prompt.len() > 100 { &prompt[..100] } else { prompt },
            "provider": config.agent.default_provider,
        }));
    }

    for (name, agent) in &config.agents {
        templates.push(serde_json::json!({
            "id": name,
            "name": name,
            "system_prompt_preview": agent.system_prompt.as_ref()
                .map(|p| if p.len() > 100 { &p[..100] } else { p }),
            "provider": agent.provider,
            "model": agent.model,
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"templates": templates})),
    )
}

/// GET /admin/api/about — build info, version, uptime
pub async fn about(State(state): State<AdminState>) -> Json<serde_json::Value> {
    let active_providers = state.app_state.agents.provider_ids();
    let session_count = state.app_state.sessions.len();

    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "GarraIA",
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "repository": "https://github.com/michelbr84/GarraRUST",
        "license": "MIT",
        "rust_version": "1.85+",
        "active_providers": active_providers.len(),
        "active_sessions": session_count,
    }))
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
