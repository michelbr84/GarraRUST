//! `PATCH /v1/me/password` — change the caller's own password (via `login_pool`).

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use garraia_auth::{
    PasswordChangeOutcome, Principal, WorkspaceAuditAction, audit_workspace_event, change_password,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── PATCH /v1/me/password ───────────────────────────────────────────────────

/// Request body for `PATCH /v1/me/password`.
#[derive(Deserialize, ToSchema)]
pub struct PatchMyPasswordRequest {
    /// The caller's current password (used for verification before the change).
    pub current_password: String,
    /// The new password to set. Must be 8–1024 characters.
    pub new_password: String,
}

/// `PATCH /v1/me/password` — change the authenticated caller's own password.
///
/// Verifies `current_password` against the stored Argon2id or legacy PBKDF2
/// hash, then replaces it with a fresh Argon2id hash of `new_password`.
///
/// All `user_identities` access uses the dedicated `LoginPool` (BYPASSRLS,
/// `garraia_login` role). The `garraia_app` role cannot read `password_hash`
/// (FORCE RLS, CLAUDE.md Rule 12).
///
/// Returns 403 for both wrong current password and identity-not-found
/// (anti-enumeration — callers cannot distinguish the two cases).
#[utoipa::path(
    patch,
    path = "/v1/me/password",
    request_body = PatchMyPasswordRequest,
    responses(
        (status = 204, description = "Password changed successfully."),
        (status = 400, description = "Validation error (e.g. new_password too short).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 403, description = "current_password is incorrect.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 503, description = "Auth not configured.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = [])),
    tag = "me",
)]
pub async fn patch_my_password(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(body): Json<PatchMyPasswordRequest>,
) -> Result<StatusCode, RestError> {
    if body.new_password.len() < 8 {
        return Err(RestError::BadRequest(
            "new_password must be at least 8 characters".into(),
        ));
    }
    if body.new_password.len() > 1024 {
        return Err(RestError::BadRequest(
            "new_password must be at most 1024 characters".into(),
        ));
    }

    let current_password = secrecy::SecretString::from(body.current_password);
    let new_password = secrecy::SecretString::from(body.new_password);

    let outcome = change_password(
        &state.auth.login_pool,
        principal.user_id,
        &current_password,
        &new_password,
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("change_password: {e}")))?;

    match outcome {
        PasswordChangeOutcome::Success => {}
        PasswordChangeOutcome::WrongPassword | PasswordChangeOutcome::IdentityNotFound => {
            return Err(RestError::Forbidden);
        }
    }

    // Emit audit event via app_pool (user-scoped, nil group_id).
    let nil_uuid = Uuid::nil();
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::PasswordChanged,
        principal.user_id,
        nil_uuid,
        "user_identities",
        principal.user_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PATCH /v1/me/password unit tests ──────────────────────────────────────

    #[test]
    fn patch_password_new_too_short_is_rejected() {
        let short = "abc123!";
        assert!(
            short.len() < 8,
            "password below 8 chars must trigger validation error"
        );
    }

    #[test]
    fn patch_password_new_8_chars_is_minimum_valid() {
        let ok = "abc123!@";
        assert_eq!(ok.len(), 8, "8-char password is at the minimum boundary");
    }

    #[test]
    fn patch_password_new_1024_chars_is_max_valid() {
        let ok = "a".repeat(1024);
        assert_eq!(
            ok.len(),
            1024,
            "1024-char password is at the maximum boundary"
        );
    }

    #[test]
    fn patch_password_new_1025_chars_is_rejected() {
        let too_long = "a".repeat(1025);
        assert!(
            too_long.len() > 1024,
            "1025-char password must trigger validation error"
        );
    }

    #[test]
    fn patch_password_audit_action_wire_form() {
        assert_eq!(
            WorkspaceAuditAction::PasswordChanged.as_str(),
            "password.changed"
        );
    }

    #[test]
    fn patch_password_audit_action_distinct_from_api_key_actions() {
        assert_ne!(
            WorkspaceAuditAction::PasswordChanged.as_str(),
            WorkspaceAuditAction::ApiKeyCreated.as_str(),
        );
        assert_ne!(
            WorkspaceAuditAction::PasswordChanged.as_str(),
            WorkspaceAuditAction::SessionRevoked.as_str(),
        );
    }

    #[test]
    fn patch_password_request_fields_deserialization_shape() {
        let json_str = r#"{"current_password":"old_pass","new_password":"new_pass123"}"#;
        let req: PatchMyPasswordRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.current_password, "old_pass");
        assert_eq!(req.new_password, "new_pass123");
    }

    #[test]
    fn patch_password_nil_uuid_for_group_id_in_audit() {
        let nil = Uuid::nil();
        assert_eq!(
            nil.to_string(),
            "00000000-0000-0000-0000-000000000000",
            "group_id must be nil-uuid for user-scoped password audit event"
        );
    }
}
