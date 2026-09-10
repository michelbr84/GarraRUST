//! Admin user-management handlers (Q9.g / GAR-474).
//! Extracted from `admin/handlers.rs` — zero behaviour change.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::middleware::{AuthenticatedAdmin, build_session_cookie, extract_ip};
use super::rbac::{Action, Resource, Role, check_permission};
use super::shared::AdminState;
use super::store::AdminStore;

/// Minimum length for a new password. Same bound the `setup` and
/// `create_user` handlers already enforce.
const MIN_PASSWORD_LEN: usize = 8;

/// Audit `action` written by [`change_password`].
pub const CHANGE_PASSWORD_ACTION: &str = "change_password";

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

    if body.username.len() < 3 || body.password.len() < MIN_PASSWORD_LEN {
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

    if body.username.len() < 3 || body.password.len() < MIN_PASSWORD_LEN {
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

// ── Self-service password change (#1120) ─────────────────────────────

#[derive(serde::Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Write one audit row for a change-password attempt.
///
/// `details` describes the terminal for the trail and never carries a
/// password — neither the current nor the new one.
fn audit_change_password(
    store: &AdminStore,
    admin: &AuthenticatedAdmin,
    details: &str,
    ip: Option<&str>,
    outcome: &str,
) {
    let _ = store.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        CHANGE_PASSWORD_ACTION,
        "user",
        Some(&admin.user_id),
        Some(details),
        ip,
        outcome,
    );
}

/// POST /admin/api/change-password — rotate the caller's own password.
///
/// #1120: before this route the only way to change an admin password was to
/// run SQL against `admin.db` by hand.
///
/// The current password is re-verified with the same `verify_password` the
/// danger zone uses, so a stolen session cookie alone cannot lock the real
/// owner out; session auth and CSRF come from the router this handler is
/// mounted in. The new hash and the revocation of the user's other sessions
/// commit in one SQLite transaction — either both land or neither does.
/// Hashing stays the local PBKDF2-HMAC-SHA256 of `store.rs` —
/// `garraia_auth` (Argon2id) belongs to the Postgres workspace identity
/// provider and pulling it here would change the scheme for existing rows.
///
/// Every terminal writes an audit event. Responses reuse the
/// `{"error": ...}` / `{"ok": true}` shapes of the rest of the admin API; no
/// password is ever echoed back or logged.
pub async fn change_password(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);

    // Cheapest rejection first: no KDF work, no DB write.
    if body.new_password.len() < MIN_PASSWORD_LEN {
        let guard = state.store.lock().await;
        audit_change_password(
            &guard,
            &admin,
            "rejected: new password shorter than the minimum",
            ip.as_deref(),
            "failure",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "new password must be >=8 chars"})),
        );
    }

    let guard = state.store.lock().await;

    let verified = guard.verify_password(&admin.username, &body.current_password);
    if verified.is_none() {
        audit_change_password(
            &guard,
            &admin,
            "rejected: current password did not verify",
            ip.as_deref(),
            "failure",
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "password verification failed"})),
        );
    }

    // The new hash and the revocation of the other sessions commit in ONE
    // SQLite transaction: a failure in either one rolls the whole rotation
    // back, so success is never announced with a stolen cookie still
    // validating. The session the caller authenticated with is kept, so the
    // console survives the rotation.
    let revoked = match guard.rotate_password_and_revoke_sessions(
        &admin.user_id,
        &body.new_password,
        &admin.session_token,
    ) {
        Ok(n) => n,
        Err(e) => {
            // Log the cause, answer with a fixed string: the SQLite message is an
            // internal detail and the admin API has been echoing it elsewhere.
            tracing::error!("change-password: rotation failed, nothing was persisted: {e}");
            audit_change_password(
                &guard,
                &admin,
                "rejected: could not persist the rotation",
                ip.as_deref(),
                "failure",
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to update password"})),
            );
        }
    };

    audit_change_password(
        &guard,
        &admin,
        &format!("self-service password change, {revoked} other session(s) revoked"),
        ip.as_deref(),
        "success",
    );

    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}
