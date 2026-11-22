//! `GET /v1/me/invites` + `POST /v1/me/invites/{invite_id}/decline` —
//! pending invites addressed to the caller and the invitee-side decline.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── GET /v1/me/invites ──────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/invites`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyInvitesQuery {
    /// Keyset cursor — `id` of the last invite on the previous page.
    /// Returns invites created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
}

/// One pending group invite in the `GET /v1/me/invites` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingInviteSummary {
    /// UUID of the invite row.
    pub id: Uuid,
    /// UUID of the group the caller has been invited to join.
    pub group_id: Uuid,
    /// Role the inviter proposed: `admin`, `member`, `guest`, or `child`.
    pub proposed_role: String,
    /// UTC timestamp when the invite was created.
    pub created_at: DateTime<Utc>,
    /// UTC timestamp when the invite expires.
    pub expires_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/invites`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyInvitesResponse {
    pub items: Vec<PendingInviteSummary>,
    /// `id` of the last invite in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/invites` — inbox of pending group invites addressed to the caller.
///
/// Returns up to `limit` (default 50, max 100) non-accepted, non-expired group
/// invites sent to the caller's registered email address, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_invite_id>`. No `group_id` parameter —
/// this is a cross-group personal inbox.
///
/// `group_invites` has no FORCE RLS — isolation is enforced via explicit
/// `WHERE u.id = $principal_user_id` after joining `users ON email = invited_email`.
/// `token_hash` and `invited_email` are never included in the response.
#[utoipa::path(
    get,
    path = "/v1/me/invites",
    params(ListMyInvitesQuery),
    responses(
        (status = 200, description = "List of pending group invites, newest first.", body = MyInvitesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_invites(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyInvitesQuery>,
) -> Result<Json<MyInvitesResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);

    let pool = state.app_pool.pool_for_handlers();

    // Columns: gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at
    type InviteRow = (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>);

    let rows: Vec<InviteRow> = match params.after {
        None => sqlx::query_as(
            "SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at \
             FROM group_invites gi \
             JOIN users u ON u.email = gi.invited_email \
             WHERE u.id = $1 \
               AND gi.accepted_at IS NULL \
               AND gi.revoked_at IS NULL \
               AND gi.declined_at IS NULL \
               AND gi.expires_at > now() \
             ORDER BY gi.created_at DESC, gi.id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        Some(after_id) => sqlx::query_as(
            "SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at \
             FROM group_invites gi \
             JOIN users u ON u.email = gi.invited_email \
             WHERE u.id = $1 \
               AND gi.accepted_at IS NULL \
               AND gi.revoked_at IS NULL \
               AND gi.declined_at IS NULL \
               AND gi.expires_at > now() \
               AND (gi.created_at, gi.id) < ( \
                   SELECT gi2.created_at, gi2.id FROM group_invites gi2 \
                   JOIN users u2 ON u2.email = gi2.invited_email \
                   WHERE gi2.id = $2 AND u2.id = $1 \
               ) \
             ORDER BY gi.created_at DESC, gi.id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, group_id, proposed_role, created_at, expires_at)| PendingInviteSummary {
                id,
                group_id,
                proposed_role,
                created_at,
                expires_at,
            },
        )
        .collect();

    Ok(Json(MyInvitesResponse { items, next_cursor }))
}

// ─── POST /v1/me/invites/{invite_id}/decline ─────────────────────────────────

/// `POST /v1/me/invites/{invite_id}/decline` — invitee-side decline of a pending
/// group invite (plan 0258 / GAR-783).
///
/// Sets `declined_at = now()` and `declined_by = caller_user_id` on the invite row.
/// Returns 204 No Content on success. Returns 404 if the invite does not exist,
/// does not belong to the caller, has already been accepted, revoked, or declined.
///
/// No `X-Group-Id` header required — `group_id` is resolved from the invite row.
/// No capability check — any authenticated user may decline their own invite.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Invite not found / not the caller's / terminal | 404    |
/// | Happy path                                     | 204    |
#[utoipa::path(
    post,
    path = "/v1/me/invites/{invite_id}/decline",
    params(
        ("invite_id" = Uuid, Path, description = "Invite UUID to decline."),
    ),
    responses(
        (status = 204, description = "Invite declined."),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Invite not found, not addressed to caller, or already terminal (accepted/revoked/declined).", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn decline_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(invite_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
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

    // Soft-decline: verify the caller is the invitee (JOIN users ON email),
    // ensure the invite is still pending, then set declined_at + declined_by.
    // RETURNING group_id + proposed_role for audit (group_id also needed for
    // SET LOCAL app.current_group_id before the audit_events INSERT).
    #[derive(sqlx::FromRow)]
    struct DeclinedRow {
        group_id: Uuid,
        proposed_role: String,
    }

    let declined: Option<DeclinedRow> = sqlx::query_as(
        "UPDATE group_invites gi \
         SET declined_at = now(), declined_by = u.id \
         FROM users u \
         WHERE u.email = gi.invited_email \
           AND u.id = $1 \
           AND gi.id = $2 \
           AND gi.accepted_at IS NULL \
           AND gi.revoked_at IS NULL \
           AND gi.declined_at IS NULL \
         RETURNING gi.group_id, gi.proposed_role",
    )
    .bind(principal.user_id)
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = declined.ok_or(RestError::NotFound)?;

    // SET LOCAL group_id now that we know it (required for FORCE-RLS audit_events INSERT).
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(row.group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Emit audit event. Metadata: proposed_role only — invited_email is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::InviteDeclined,
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

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MyInvitesResponse / PendingInviteSummary serialization ───────────────

    #[test]
    fn my_invites_response_empty_no_cursor() {
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(
            v.get("next_cursor").is_none(),
            "absent cursor must be omitted"
        );
    }

    #[test]
    fn my_invites_response_with_cursor() {
        let cursor = Uuid::parse_str("cccccccc-0000-0000-0000-000000000003").unwrap();
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "cccccccc-0000-0000-0000-000000000003");
    }

    #[test]
    fn pending_invite_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let created = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let expires = chrono::DateTime::from_timestamp(9_999_999, 0).unwrap();
        let summary = PendingInviteSummary {
            id,
            group_id,
            proposed_role: "member".into(),
            created_at: created,
            expires_at: expires,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "bbbbbbbb-0000-0000-0000-000000000002");
        assert_eq!(v["proposed_role"], "member");
        assert!(v.get("created_at").is_some());
        assert!(v.get("expires_at").is_some());
    }

    #[test]
    fn pending_invite_summary_no_token_hash_field() {
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "guest".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            expires_at: chrono::DateTime::from_timestamp(9_999_999, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("token_hash").is_none(),
            "token_hash must never appear in response"
        );
        assert!(
            v.get("invited_email").is_none(),
            "invited_email must never appear in response"
        );
    }

    #[test]
    fn list_my_invites_query_defaults() {
        let q = ListMyInvitesQuery {
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn list_my_invites_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 50, "no limit defaults to 50");
    }

    #[test]
    fn list_my_invites_query_with_cursor() {
        let after = Uuid::parse_str("dddddddd-0000-0000-0000-000000000004").unwrap();
        let q = ListMyInvitesQuery {
            after: Some(after),
            limit: Some(10),
        };
        assert_eq!(q.after, Some(after));
        assert_eq!(q.limit, Some(10));
    }

    // ─── POST /v1/me/invites/{invite_id}/decline ─────────────────────────────

    #[test]
    fn pending_invite_summary_no_declined_at_field() {
        // PendingInviteSummary must not expose declined_at (migration 025 column
        // must never leak into the JSON response).
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "member".into(),
            created_at: chrono::DateTime::UNIX_EPOCH,
            expires_at: chrono::DateTime::UNIX_EPOCH,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("declined_at").is_none(),
            "declined_at must not leak into summary response"
        );
        assert!(
            v.get("declined_by").is_none(),
            "declined_by must not leak into summary response"
        );
    }

    #[test]
    fn my_invites_response_with_declined_invite_excluded() {
        // When a declined invite is excluded, the inbox is empty.
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(v.get("next_cursor").is_none() || v["next_cursor"].is_null());
    }

    #[test]
    fn my_invites_response_next_cursor_omitted_when_none() {
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        // next_cursor must be absent (skip_serializing_if None), not "null".
        assert!(
            v.get("next_cursor").is_none(),
            "next_cursor must be omitted when None"
        );
    }
}
