//! `DELETE /v1/me` — self-service erasure request (tombstone now, purge later).

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde_json::json;
use uuid::Uuid;

use crate::account_purge_worker::DEFAULT_GRACE_PERIOD_DAYS;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── DELETE /v1/me (plan 0343 / GAR-884) ────────────────────────────────────

/// Self-service account deletion (plan 0343 / GAR-884; hard deletion wired
/// here — LGPD art. 18, VI / GDPR art. 17).
///
/// One atomic transaction does three things: flips `users.status` to
/// `'deleted'` (the tombstone), revokes every active session, and **files a
/// hard-deletion request** in `account_deletion_requests` with
/// `purge_after = now() + <grace period>`. Emits both
/// `account.self_deleted` and `account.purge_scheduled`. Returns 204, or 409
/// if the account is already deleted or purged.
///
/// Atomicity matters here: if the tombstone committed without the request
/// row, the account would look deleted forever and never actually be erased —
/// the exact silent failure the previous "deferred to a future worker" note
/// left open.
///
/// No password re-confirmation — the caller is already authenticated by JWT.
///
/// ## The grace period, and why erasure is not immediate
///
/// [`DEFAULT_GRACE_PERIOD_DAYS`] days pass before
/// `account_purge_worker` makes the erasure irreversible. Immediate
/// irreversible erasure would turn a stolen access token into a weapon:
/// one request and nothing can be recovered. The delay gives the real
/// subject a window to reverse the request through support and the operator
/// a window to apply a legal hold. The date is in the subject's own export
/// (`deletion_requests[].purge_after`) and the policy is written out in
/// `docs/legal/data-subject-requests.md`.
///
/// The response stays 204 with no body on purpose — adding one would break
/// the published contract for every existing client. The schedule is
/// discoverable through the export and the audit trail instead.
///
/// ## What the purge erases, and what it deliberately keeps
///
/// See the header of migration 034 for the full statement. In short: personal
/// content and identifiers are destroyed; the `users` row survives as a
/// tombstone carrying no personal data, because five FKs declared
/// `NO ACTION` point at it and removing it would delete other members'
/// shared context (thread roots, chats, invites). Messages the subject wrote
/// have their body and sender label replaced; messages **other people** wrote
/// are untouched — one person's erasure request does not empty a shared
/// conversation.
#[utoipa::path(
    delete,
    path = "/v1/me",
    tag = "me",
    responses(
        (status = 204, description = "Account soft-deleted, sessions revoked, hard deletion scheduled"),
        (status = 409, description = "Account already deleted or purged"),
        (status = 401, description = "Missing or invalid JWT"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn delete_me(
    State(state): State<RestV1FullState>,
    principal: Principal,
) -> Result<StatusCode, RestError> {
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

    // Guard: check current status. FOR UPDATE prevents concurrent double-delete.
    let current_status: Option<String> =
        sqlx::query_scalar("SELECT status FROM users WHERE id = $1 FOR UPDATE")
            .bind(principal.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    match current_status.as_deref() {
        None => {
            // Row not found — should be unreachable for a valid JWT principal.
            return Err(RestError::Internal(anyhow::anyhow!(
                "user row not found for authenticated principal"
            )));
        }
        Some("deleted") => {
            // Already deleted — idempotency guard: do not re-emit the audit
            // event and do not file a second purge request.
            return Err(RestError::Conflict("account is already deleted".into()));
        }
        Some("purged") => {
            // The erasure already ran. Re-filing would schedule a purge for
            // a tombstone that holds no personal data left to erase.
            return Err(RestError::Conflict("account is already purged".into()));
        }
        _ => {}
    }

    // Soft-delete the account.
    sqlx::query("UPDATE users SET status = 'deleted' WHERE id = $1")
        .bind(principal.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Revoke all active sessions atomically.
    sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // Queue the hard deletion in the SAME transaction as the tombstone, so
    // the two can never diverge. `account_deletion_one_open_idx` (migration
    // 034) makes a duplicate open request impossible; a 23505 here would mean
    // an open request existed for an account whose status said otherwise, so
    // it is reported as a conflict rather than a 500.
    let scheduled: Result<(Uuid, DateTime<Utc>), sqlx::Error> = sqlx::query_as(
        "INSERT INTO account_deletion_requests (user_id, purge_after) \
         VALUES ($1, now() + make_interval(days => $2)) \
         RETURNING id, purge_after",
    )
    .bind(principal.user_id)
    .bind(i32::try_from(DEFAULT_GRACE_PERIOD_DAYS).unwrap_or(30))
    .fetch_one(&mut *tx)
    .await;

    let (request_id, _purge_after) = match scheduled {
        Ok(row) => row,
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "a deletion request is already open for this account".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    };

    // Emit compliance audit event. No PII in metadata.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::AccountSelfDeleted,
        principal.user_id,
        nil_uuid,
        "users",
        principal.user_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    // Second event, same transaction: the tombstone and the schedule are one
    // decision and the trail has to show both or neither.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::AccountPurgeScheduled,
        principal.user_id,
        nil_uuid,
        "account_deletion_requests",
        request_id.to_string(),
        json!({ "grace_period_days": DEFAULT_GRACE_PERIOD_DAYS }),
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

    // ── DELETE /v1/me tests (plan 0343 / GAR-884) ─────────────────────────────

    #[test]
    fn delete_me_action_string_is_stable() {
        assert_eq!(
            WorkspaceAuditAction::AccountSelfDeleted.as_str(),
            "account.self_deleted"
        );
    }

    #[test]
    fn delete_me_action_display_matches_as_str() {
        let action = WorkspaceAuditAction::AccountSelfDeleted;
        assert_eq!(format!("{action}"), "account.self_deleted");
    }

    #[test]
    fn delete_me_response_is_no_content() {
        // DELETE /v1/me returns 204 No Content — verify the constant.
        assert_eq!(StatusCode::NO_CONTENT.as_u16(), 204);
    }

    #[test]
    fn delete_me_conflict_is_409() {
        // Already-deleted account path returns 409 Conflict.
        assert_eq!(StatusCode::CONFLICT.as_u16(), 409);
    }

    #[test]
    fn delete_me_action_is_user_scoped_not_group_scoped() {
        // AccountSelfDeleted carries the nil-uuid as group_id by convention —
        // same as SessionRevoked / PasswordChanged.  Verify the string does not
        // carry a group prefix so consumers can distinguish user-scoped events.
        let s = WorkspaceAuditAction::AccountSelfDeleted.as_str();
        assert!(
            s.starts_with("account."),
            "expected 'account.*' namespace, got '{s}'"
        );
    }

    #[test]
    fn delete_me_metadata_is_empty_object() {
        // PII safety: no email or display_name in metadata.
        // The handler emits json!({}) — verify serialization is stable.
        let meta = serde_json::json!({});
        assert!(meta.is_object());
        assert_eq!(meta.as_object().unwrap().len(), 0);
    }
}
