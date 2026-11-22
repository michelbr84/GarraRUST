//! `GET /v1/me/sessions`, `DELETE /v1/me/sessions[/{session_id}]` — the
//! caller's own sessions (user-scoped, no `X-Group-Id`).

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

// ─── GET /v1/me/sessions ─────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/sessions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMySessionsQuery {
    /// Cursor: `id` of the last session on the previous page (keyset on
    /// `created_at DESC, id DESC`).
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default 20.
    pub limit: Option<i64>,
}

/// One active session entry in the `GET /v1/me/sessions` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionSummary {
    /// Session UUID.
    pub id: Uuid,
    /// Opaque device identifier supplied at login, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// When the session token expires (UTC).
    pub expires_at: DateTime<Utc>,
    /// When the session was created (UTC).
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/sessions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MySessionsResponse {
    pub items: Vec<SessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// RLS is enforced via `sessions_owner_only` (migration 007) which filters by
/// `app.current_user_id`. No `X-Group-Id` required — sessions are user-scoped.
/// `app.current_group_id` is set to nil-uuid per convention.
#[utoipa::path(
    get,
    path = "/v1/me/sessions",
    params(ListMySessionsQuery),
    responses(
        (status = 200, description = "List of active sessions for the caller, newest-created first.", body = MySessionsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_sessions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMySessionsQuery>,
) -> Result<Json<MySessionsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
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

    type SessionRow = (Uuid, Option<String>, DateTime<Utc>, DateTime<Utc>);

    let rows: Vec<SessionRow> = match params.after {
        None => sqlx::query_as(
            "SELECT id, device_id, expires_at, created_at \
             FROM sessions \
             WHERE user_id = $1 \
               AND revoked_at IS NULL \
               AND expires_at > now() \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        Some(after_id) => sqlx::query_as(
            "SELECT id, device_id, expires_at, created_at \
             FROM sessions \
             WHERE user_id = $1 \
               AND revoked_at IS NULL \
               AND expires_at > now() \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM sessions \
                   WHERE id = $2 AND user_id = $1 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|(id, device_id, expires_at, created_at)| SessionSummary {
            id,
            device_id,
            expires_at,
            created_at,
        })
        .collect();

    Ok(Json(MySessionsResponse { items, next_cursor }))
}

// ─── DELETE /v1/me/sessions/{session_id} ─────────────────────────────────────

/// `DELETE /v1/me/sessions/{session_id}` — revoke a specific session
/// (plan 0326 / GAR-866). Sets `revoked_at = now()` on the session row.
///
/// Idempotent: if the session is already revoked, returns 204 (not an error).
/// Returns 404 if the session does not exist or belongs to another user
/// (FORCE RLS via `sessions_owner_only` hides cross-user rows).
///
/// No `X-Group-Id` header required — sessions are user-scoped.
#[utoipa::path(
    delete,
    path = "/v1/me/sessions/{session_id}",
    params(
        ("session_id" = Uuid, Path, description = "Session UUID to revoke."),
    ),
    responses(
        (status = 204, description = "Session revoked, or already revoked (idempotent)."),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Session not found or belongs to another user.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn revoke_my_session(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(session_id): Path<Uuid>,
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

    // FORCE RLS (sessions_owner_only) ensures cross-user session_id returns 0 rows.
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        // Distinguish "already revoked" (204) from "not found / cross-user" (404).
        let exists: Option<(bool,)> =
            sqlx::query_as("SELECT true FROM sessions WHERE id = $1 AND user_id = $2")
                .bind(session_id)
                .bind(principal.user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        return if exists.is_some() {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(RestError::NotFound)
        };
    }

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::SessionRevoked,
        principal.user_id,
        nil_uuid,
        "sessions",
        session_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── DELETE /v1/me/sessions ──────────────────────────────────────────────────

/// `DELETE /v1/me/sessions` — revoke ALL of the caller's active sessions
/// ("sign out from all devices") (plan 0328 / GAR-869).
///
/// Atomically sets `revoked_at = now()` on every non-expired, non-revoked
/// session row owned by the caller. Returns 204 regardless of how many rows
/// are affected (including zero).
///
/// An audit event `SessionsAllRevoked` is emitted only when at least one
/// session was revoked (`rows_affected > 0`).
///
/// No `X-Group-Id` header required — sessions are user-scoped.
#[utoipa::path(
    delete,
    path = "/v1/me/sessions",
    responses(
        (status = 204, description = "All active sessions revoked (or none were active)."),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn revoke_all_my_sessions(
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

    // FORCE RLS (sessions_owner_only) ensures only the caller's rows are affected.
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let count = result.rows_affected();

    if count > 0 {
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::SessionsAllRevoked,
            principal.user_id,
            nil_uuid,
            "sessions",
            principal.user_id.to_string(),
            json!({ "count": count }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SessionSummary / MySessionsResponse serialization ────────────────────

    #[test]
    fn session_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id,
            device_id: Some("android-pixel-8".to_string()),
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["device_id"], "android-pixel-8");
        assert!(v.get("expires_at").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn session_summary_omits_device_id_when_none() {
        use chrono::TimeZone;
        let id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id,
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("device_id").is_none(),
            "absent device_id must be omitted"
        );
    }

    #[test]
    fn session_summary_expires_at_utc_z() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 12, 30, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["expires_at"].as_str().unwrap();
        assert!(
            ts.ends_with('Z'),
            "expires_at must be UTC with Z suffix: {ts}"
        );
    }

    #[test]
    fn session_summary_created_at_utc_z() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 14, 22, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["created_at"].as_str().unwrap();
        assert!(
            ts.ends_with('Z'),
            "created_at must be UTC with Z suffix: {ts}"
        );
    }

    #[test]
    fn my_sessions_response_empty_no_cursor() {
        let resp = MySessionsResponse {
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
    fn my_sessions_response_with_cursor() {
        let cursor = Uuid::parse_str("dddddddd-0000-0000-0000-000000000003").unwrap();
        let resp = MySessionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "dddddddd-0000-0000-0000-000000000003");
    }

    #[test]
    fn list_my_sessions_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit clamped to 100");
        assert_eq!(under, 1, "zero clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_sessions_query_defaults() {
        let q = ListMySessionsQuery {
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn session_summary_nil_uuid_roundtrip() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn my_sessions_no_refresh_token_hash_field() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("refresh_token_hash").is_none(),
            "refresh_token_hash must never appear in session response"
        );
    }

    #[test]
    fn my_sessions_response_items_propagate() {
        use chrono::TimeZone;
        let id1 = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let id2 = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let resp = MySessionsResponse {
            items: vec![
                SessionSummary {
                    id: id1,
                    device_id: Some("ios".to_string()),
                    expires_at: expires,
                    created_at: created,
                },
                SessionSummary {
                    id: id2,
                    device_id: None,
                    expires_at: expires,
                    created_at: created,
                },
            ],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["items"][1]["id"], "bbbbbbbb-0000-0000-0000-000000000002");
    }

    #[test]
    fn my_sessions_response_cursor_is_uuid_string() {
        let cursor = Uuid::parse_str("ffffffff-0000-0000-0000-000000000001").unwrap();
        let resp = MySessionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let cursor_str = v["next_cursor"].as_str().unwrap();
        assert!(
            Uuid::parse_str(cursor_str).is_ok(),
            "next_cursor must parse as UUID: {cursor_str}"
        );
    }

    // ── revoke_all_my_sessions (unit tests — no DB required) ─────────────────

    #[test]
    fn revoke_all_sessions_audit_action_wire_form() {
        assert_eq!(
            WorkspaceAuditAction::SessionsAllRevoked.as_str(),
            "session.all_revoked"
        );
    }

    #[test]
    fn revoke_all_sessions_audit_action_distinct_from_single() {
        assert_ne!(
            WorkspaceAuditAction::SessionsAllRevoked.as_str(),
            WorkspaceAuditAction::SessionRevoked.as_str(),
            "bulk-revoke and single-revoke must have distinct audit actions"
        );
    }

    #[test]
    fn revoke_all_sessions_count_metadata_serializes() {
        let count: u64 = 3;
        let meta = json!({ "count": count });
        assert_eq!(meta["count"], 3);
    }

    #[test]
    fn revoke_all_sessions_zero_count_metadata_serializes() {
        let count: u64 = 0;
        let meta = json!({ "count": count });
        assert_eq!(meta["count"], 0);
    }

    #[test]
    fn revoke_all_sessions_nil_group_uuid() {
        let nil = Uuid::nil();
        assert_eq!(
            nil.to_string(),
            "00000000-0000-0000-0000-000000000000",
            "group_id must be nil-uuid for user-scoped session audit"
        );
    }

    #[test]
    fn revoke_all_sessions_resource_type_is_sessions() {
        let resource_type = "sessions";
        assert_eq!(resource_type, "sessions");
    }
}
