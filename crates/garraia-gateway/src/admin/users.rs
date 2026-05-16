//! Admin user-management handlers (Q9.g / GAR-474).
//! Extracted from `admin/handlers.rs` — zero behaviour change.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::middleware::{AuthenticatedAdmin, build_session_cookie, extract_ip};
use super::rbac::{Action, Resource, Role, check_permission};
use super::shared::AdminState;

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
