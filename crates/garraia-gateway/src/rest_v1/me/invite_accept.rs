//! `POST /v1/me/invites/{invite_id}/accept` — accept a pending invite by UUID.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::{Path, State};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── POST /v1/me/invites/{invite_id}/accept ───────────────────────────────────

/// Response body for `POST /v1/me/invites/{invite_id}/accept` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptMyInviteResponse {
    /// The group the caller just joined.
    pub group_id: Uuid,
    /// The role assigned from the invite (`member`, `admin`, `guest`, or `child`).
    pub role: String,
    /// The invite UUID that was accepted.
    pub invite_id: Uuid,
}

/// `POST /v1/me/invites/{invite_id}/accept` — accept a pending group invite by UUID.
///
/// Authenticated variant of the token-based `POST /v1/invites/{token}/accept`
/// (plan 0019). The caller provides the invite UUID from their inbox
/// (`GET /v1/me/invites`); no raw plaintext token is required.
///
/// Verifies the invite belongs to the caller (by email match), is not yet
/// terminal (accepted/revoked/declined), and is not expired. On success,
/// atomically marks the invite as accepted and inserts the caller into
/// `group_members` with the `proposed_role` from the invite.
///
/// No `X-Group-Id` header required — `group_id` is resolved from the invite.
/// No capability check — any authenticated user may accept their own invite.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Invite not found / not the caller's / terminal | 404    |
/// | Invite expired                                 | 410    |
/// | Caller already a member of the group           | 409    |
/// | Happy path                                     | 200    |
#[utoipa::path(
    post,
    path = "/v1/me/invites/{invite_id}/accept",
    params(
        ("invite_id" = Uuid, Path, description = "Invite UUID to accept."),
    ),
    responses(
        (status = 200, description = "Invite accepted; caller is now a group member.", body = AcceptMyInviteResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Invite not found, not addressed to caller, or already terminal (accepted/revoked/declined).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 409, description = "Caller is already a member of this group.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 410, description = "Invite has expired.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn accept_my_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(invite_id): Path<Uuid>,
) -> Result<Json<AcceptMyInviteResponse>, RestError> {
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL user_id for FORCE-RLS on audit_events.
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Atomically mark the invite as accepted.
    // All terminal-state guards are in the WHERE clause so a concurrent
    // double-accept by another session returns 0 rows affected (safe).
    #[derive(sqlx::FromRow)]
    struct AcceptedRow {
        group_id: Uuid,
        proposed_role: String,
        invited_by: Option<Uuid>,
    }

    let accepted: Option<AcceptedRow> = sqlx::query_as(
        "UPDATE group_invites gi \
         SET accepted_at = now(), accepted_by = u.id \
         FROM users u \
         WHERE u.email = gi.invited_email \
           AND u.id = $1 \
           AND gi.id = $2 \
           AND gi.accepted_at IS NULL \
           AND gi.revoked_at IS NULL \
           AND gi.declined_at IS NULL \
           AND gi.expires_at >= now() \
         RETURNING gi.group_id, gi.proposed_role, gi.created_by AS invited_by",
    )
    .bind(principal.user_id)
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match accepted {
        Some(r) => r,
        None => {
            // Distinguish 410 (expired) from 404 (not found / terminal / wrong user).
            #[derive(sqlx::FromRow)]
            struct ExpiryRow {
                is_expired: bool,
            }
            let expiry: Option<ExpiryRow> = sqlx::query_as(
                "SELECT gi.expires_at < now() AS is_expired \
                 FROM group_invites gi \
                 JOIN users u ON u.email = gi.invited_email \
                 WHERE u.id = $1 AND gi.id = $2",
            )
            .bind(principal.user_id)
            .bind(invite_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            return match expiry {
                Some(e) if e.is_expired => Err(RestError::Gone("this invite has expired".into())),
                _ => Err(RestError::NotFound),
            };
        }
    };

    // SET LOCAL group_id now that we know it (required for FORCE-RLS audit_events INSERT).
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(row.group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Insert group_members. SQLSTATE 23505 (unique violation on PK) means the
    // caller is already a member of this group — 409 Conflict.
    let insert_result = sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, invited_by) \
         VALUES ($1, $2, $3, 'active', $4)",
    )
    .bind(row.group_id)
    .bind(principal.user_id)
    .bind(&row.proposed_role)
    .bind(row.invited_by)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Ok(_) => {}
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "you are already a member of this group".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    }

    // Emit audit event. Metadata: proposed_role only — invited_email is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::InviteAccepted,
        principal.user_id,
        row.group_id,
        "group_invites",
        invite_id.to_string(),
        json!({ "proposed_role": row.proposed_role }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(AcceptMyInviteResponse {
        group_id: row.group_id,
        role: row.proposed_role,
        invite_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rest_v1::me::invites::PendingInviteSummary;

    // ─── POST /v1/me/invites/{invite_id}/accept ──────────────────────────────

    #[test]
    fn accept_my_invite_response_serializes_all_fields() {
        let group_id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let invite_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let resp = AcceptMyInviteResponse {
            group_id,
            role: "member".into(),
            invite_id,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["group_id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["invite_id"], "bbbbbbbb-0000-0000-0000-000000000002");
        assert_eq!(v["role"], "member");
    }

    #[test]
    fn accept_my_invite_response_no_token_hash_or_email_fields() {
        // AcceptMyInviteResponse must never expose token_hash or invited_email.
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "admin".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(
            v.get("token_hash").is_none(),
            "token_hash must never appear in response"
        );
        assert!(
            v.get("invited_email").is_none(),
            "invited_email must never appear in response"
        );
        assert!(
            v.get("accepted_at").is_none(),
            "accepted_at must not be exposed in response"
        );
    }

    #[test]
    fn accept_my_invite_response_role_variants() {
        for role in &["owner", "admin", "member", "guest", "child"] {
            let resp = AcceptMyInviteResponse {
                group_id: Uuid::nil(),
                role: (*role).into(),
                invite_id: Uuid::nil(),
            };
            let v = serde_json::to_value(&resp).unwrap();
            assert_eq!(v["role"], *role, "role '{role}' must round-trip");
        }
    }

    #[test]
    fn accept_my_invite_response_nil_uuids_serialize_as_zeros() {
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "member".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v["group_id"], "00000000-0000-0000-0000-000000000000",
            "nil UUID must serialize as all-zeros string"
        );
        assert_eq!(
            v["invite_id"], "00000000-0000-0000-0000-000000000000",
            "nil UUID must serialize as all-zeros string"
        );
    }

    #[test]
    fn accept_my_invite_pending_invite_summary_excludes_accepted_at() {
        // PendingInviteSummary (used in GET /v1/me/invites) must not expose
        // accepted_at — once accepted, the invite is excluded from the inbox
        // by the WHERE clause, so leaking it would be both useless and noisy.
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "member".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            expires_at: chrono::DateTime::from_timestamp(9_999_999, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("accepted_at").is_none(),
            "accepted_at must not appear in PendingInviteSummary"
        );
        assert!(
            v.get("accepted_by").is_none(),
            "accepted_by must not appear in PendingInviteSummary"
        );
    }

    #[test]
    fn accept_my_invite_response_has_exactly_three_fields() {
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "member".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(
            obj.len(),
            3,
            "AcceptMyInviteResponse must have exactly 3 fields: group_id, role, invite_id"
        );
        assert!(obj.contains_key("group_id"));
        assert!(obj.contains_key("role"));
        assert!(obj.contains_key("invite_id"));
    }
}
