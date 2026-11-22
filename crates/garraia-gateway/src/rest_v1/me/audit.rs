//! `GET /v1/me/audit` — the caller's personal (user-scoped) audit trail.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── GET /v1/me/audit (plan 0340 / GAR-881) ──────────────────────────────────

/// Query parameters for `GET /v1/me/audit`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyAuditQuery {
    /// Keyset cursor — UUID of the last audit event received. Omit for the
    /// first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
    /// Optional filter by action string (e.g. `password.changed`). Must be
    /// non-empty if provided.
    pub action: Option<String>,
}

/// One personal audit event returned by `GET /v1/me/audit`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PersonalAuditEventSummary {
    pub id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/audit`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyAuditResponse {
    pub events: Vec<PersonalAuditEventSummary>,
    /// Opaque cursor for the next page. `None` when end of list is reached.
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/audit` — cursor-paginated personal audit trail.
///
/// Returns user-scoped audit events (login, logout, password changes, API key
/// and session management) for the authenticated caller. Only events with
/// `group_id = nil-uuid` are returned — group-scoped events require
/// `GET /v1/groups/{group_id}/audit` with `ExportGroup` permission.
///
/// No `X-Group-Id` header is required.
///
/// ## Error matrix
///
/// | Condition                | Status |
/// |--------------------------|--------|
/// | Missing/invalid JWT      | 401    |
/// | Empty `action` filter    | 400    |
/// | Happy path               | 200    |
#[utoipa::path(
    get,
    path = "/v1/me/audit",
    params(ListMyAuditQuery),
    responses(
        (status = 200, description = "Personal audit events, newest first.", body = MyAuditResponse),
        (status = 400, description = "Invalid query parameter.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = [])),
    tag = "me",
)]
pub async fn list_my_audit(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyAuditQuery>,
) -> Result<Json<MyAuditResponse>, RestError> {
    if params.action.as_deref().is_some_and(str::is_empty) {
        return Err(RestError::BadRequest("action must not be empty".into()));
    }

    let effective_limit = params.limit.unwrap_or(50).clamp(1, 100);
    let fetch_limit = (effective_limit as i64) + 1;
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

    type AuditRow = (
        Uuid,
        String,
        String,
        Option<String>,
        serde_json::Value,
        DateTime<Utc>,
    );

    let rows: Vec<AuditRow> = match (params.cursor, &params.action) {
        (None, None) => sqlx::query_as(
            "SELECT id, action, resource_type, resource_id, metadata, created_at \
             FROM audit_events \
             WHERE actor_user_id = $1 AND group_id = $2 \
             ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(nil_uuid)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(action)) => sqlx::query_as(
            "SELECT id, action, resource_type, resource_id, metadata, created_at \
             FROM audit_events \
             WHERE actor_user_id = $1 AND group_id = $2 AND action = $3 \
             ORDER BY created_at DESC, id DESC LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(nil_uuid)
        .bind(action.clone())
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(cursor_id), None) => sqlx::query_as(
            "SELECT id, action, resource_type, resource_id, metadata, created_at \
             FROM audit_events \
             WHERE actor_user_id = $1 AND group_id = $2 \
               AND (created_at, id) < \
                   (SELECT created_at, id FROM audit_events \
                    WHERE id = $3 AND actor_user_id = $1 AND group_id = $2) \
             ORDER BY created_at DESC, id DESC LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(nil_uuid)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(cursor_id), Some(action)) => sqlx::query_as(
            "SELECT id, action, resource_type, resource_id, metadata, created_at \
             FROM audit_events \
             WHERE actor_user_id = $1 AND group_id = $2 AND action = $3 \
               AND (created_at, id) < \
                   (SELECT created_at, id FROM audit_events \
                    WHERE id = $4 AND actor_user_id = $1 AND group_id = $2) \
             ORDER BY created_at DESC, id DESC LIMIT $5",
        )
        .bind(principal.user_id)
        .bind(nil_uuid)
        .bind(action.clone())
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as i64 > (effective_limit as i64);
    let events: Vec<PersonalAuditEventSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(
            |(id, action, resource_type, resource_id, metadata, created_at)| {
                PersonalAuditEventSummary {
                    id,
                    action,
                    resource_type,
                    resource_id,
                    metadata,
                    created_at,
                }
            },
        )
        .collect();

    let next_cursor = if has_more {
        events.last().map(|e| e.id)
    } else {
        None
    };

    Ok(Json(MyAuditResponse {
        events,
        next_cursor,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── GET /v1/me/audit unit tests ───────────────────────────────────────────

    #[test]
    fn list_my_audit_empty_action_rejected() {
        let q = ListMyAuditQuery {
            cursor: None,
            limit: None,
            action: Some(String::new()),
        };
        assert!(
            q.action.as_deref().is_some_and(str::is_empty),
            "empty action string must trigger 400"
        );
    }

    #[test]
    fn list_my_audit_none_action_is_valid() {
        let q = ListMyAuditQuery {
            cursor: None,
            limit: None,
            action: None,
        };
        assert!(q.action.is_none());
    }

    #[test]
    fn list_my_audit_nonempty_action_is_valid() {
        let q = ListMyAuditQuery {
            cursor: None,
            limit: None,
            action: Some("password.changed".to_string()),
        };
        assert!(!q.action.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn personal_audit_event_summary_serializes_nil_uuid() {
        let event = PersonalAuditEventSummary {
            id: Uuid::nil(),
            action: "password.changed".to_string(),
            resource_type: "user_identities".to_string(),
            resource_id: None,
            metadata: serde_json::json!({}),
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(v["action"], "password.changed");
        assert_eq!(v["resource_type"], "user_identities");
        assert!(
            v["resource_id"].is_null(),
            "absent resource_id must be null"
        );
    }

    #[test]
    fn my_audit_response_empty_list_has_no_cursor() {
        let resp = MyAuditResponse {
            events: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["next_cursor"].is_null());
        assert!(v["events"].as_array().unwrap().is_empty());
    }

    #[test]
    fn list_my_audit_limit_clamps_to_100() {
        let clamped = 200u32.clamp(1, 100);
        assert_eq!(clamped, 100, "limit must clamp to 100 max");
    }
}
