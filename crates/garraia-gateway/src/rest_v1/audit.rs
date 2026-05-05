//! `GET /v1/groups/{group_id}/audit` — cursor-paginated audit trail (plan 0070, GAR-522).
//!
//! Authz: `Action::ExportGroup` (owner-only per migration 002 seed).
//! Cross-group: `path_group_id ≠ principal.group_id` → 404.
//! No audit event is emitted for reads (avoids circular audit noise).
//!
//! ## Tenant-context protocol
//!
//! `audit_events` has FORCE RLS. Both `app.current_user_id` and
//! `app.current_group_id` are SET LOCAL via parameterized `set_config`
//! before any SELECT (plan 0056 protocol — no `format!()` interpolation).

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, can};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Compact summary of one audit event returned in list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEventSummary {
    pub id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub actor_user_id: Option<Uuid>,
    pub actor_label: Option<String>,
    /// Textual representation of the client IP (inet → text). `None` if the
    /// original event had no IP (e.g. background jobs).
    pub ip: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/groups/{group_id}/audit`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListAuditResponse {
    pub items: Vec<AuditEventSummary>,
    /// Opaque cursor for the next page. `None` when the end of the list is
    /// reached. Pass as `?cursor=<uuid>` in the next request.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/audit`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListAuditQuery {
    /// Keyset cursor — UUID of the last audit event received. Omit for the
    /// first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
    /// Filter by action string (e.g. `member.role_changed`). Must be
    /// non-empty if provided.
    pub action: Option<String>,
    /// Filter by resource type (e.g. `group_members`). Must be non-empty if
    /// provided.
    pub resource_type: Option<String>,
}

impl ListAuditQuery {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.action.as_deref().is_some_and(str::is_empty) {
            return Err("action must not be empty if provided");
        }
        if self.resource_type.as_deref().is_some_and(str::is_empty) {
            return Err("resource_type must not be empty if provided");
        }
        Ok(())
    }
}

// ─── Private row type ─────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct AuditEventRow {
    id: Uuid,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    actor_user_id: Option<Uuid>,
    actor_label: Option<String>,
    /// ip::text cast from inet; Postgres names the result column "ip".
    ip: Option<String>,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl AuditEventRow {
    fn into_summary(self) -> AuditEventSummary {
        AuditEventSummary {
            id: self.id,
            action: self.action,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            actor_user_id: self.actor_user_id,
            actor_label: self.actor_label,
            ip: self.ip,
            metadata: self.metadata,
            created_at: self.created_at,
        }
    }
}

// ─── RLS context helper ───────────────────────────────────────────────────────

async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<(), RestError> {
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(())
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// `GET /v1/groups/{group_id}/audit` — cursor-paginated group audit trail.
///
/// Authz: `Action::ExportGroup` (owner-only).
///
/// Cross-group: path `group_id` ≠ caller's `group_id` → 404 (avoids revealing
/// foreign group existence).
///
/// No audit event is emitted for reads (invariant: no circular noise).
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | No X-Group-Id / no membership      | 404    |
/// | Cross-group (path ≠ caller group)  | 404    |
/// | Insufficient permission (member)   | 403    |
/// | Empty `action` / `resource_type`   | 400    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/audit",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListAuditQuery,
    ),
    responses(
        (status = 200, description = "Audit events.", body = ListAuditResponse),
        (status = 400, description = "Invalid query parameters.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks ExportGroup permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Group not found or cross-group attempt.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_audit(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListAuditQuery>,
) -> Result<Json<ListAuditResponse>, RestError> {
    // Caller must belong to a group.
    let caller_group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::NotFound),
    };

    // Cross-group isolation: reveal nothing about foreign groups.
    if path_group_id != caller_group_id {
        return Err(RestError::NotFound);
    }

    // Authz gate: owner-only.
    if !can(&principal, Action::ExportGroup) {
        return Err(RestError::Forbidden);
    }

    // Validate optional filter params.
    params
        .validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, caller_group_id).await?;

    // Build the SELECT dynamically — push_bind produces $N parameters (no
    // string interpolation). The only static SQL pushed via push() are
    // clause keywords and operators.
    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT id, action, resource_type, resource_id, actor_user_id, actor_label, \
         ip::text AS ip, metadata, created_at \
         FROM audit_events WHERE group_id = ",
    );
    qb.push_bind(caller_group_id);

    if let Some(action_filter) = &params.action {
        qb.push(" AND action = ");
        qb.push_bind(action_filter.clone());
    }
    if let Some(rt_filter) = &params.resource_type {
        qb.push(" AND resource_type = ");
        qb.push_bind(rt_filter.clone());
    }
    if let Some(cursor_id) = params.cursor {
        qb.push(
            " AND (created_at, id) < \
             (SELECT created_at, id FROM audit_events WHERE id = ",
        );
        qb.push_bind(cursor_id);
        qb.push(" AND group_id = ");
        qb.push_bind(caller_group_id);
        qb.push(")");
    }
    qb.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    qb.push_bind(fetch_limit);

    let rows: Vec<AuditEventRow> = qb
        .build_query_as()
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<AuditEventSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(AuditEventRow::into_summary)
        .collect();

    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListAuditResponse { items, next_cursor }))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty_action_rejected() {
        let q = ListAuditQuery {
            cursor: None,
            limit: None,
            action: Some(String::new()),
            resource_type: None,
        };
        assert!(q.validate().is_err());
    }

    #[test]
    fn validate_empty_resource_type_rejected() {
        let q = ListAuditQuery {
            cursor: None,
            limit: None,
            action: None,
            resource_type: Some(String::new()),
        };
        assert!(q.validate().is_err());
    }

    #[test]
    fn validate_both_provided_ok() {
        let q = ListAuditQuery {
            cursor: None,
            limit: None,
            action: Some("member.role_changed".to_string()),
            resource_type: Some("group_members".to_string()),
        };
        assert!(q.validate().is_ok());
    }

    #[test]
    fn validate_none_provided_ok() {
        let q = ListAuditQuery {
            cursor: None,
            limit: None,
            action: None,
            resource_type: None,
        };
        assert!(q.validate().is_ok());
    }
}
