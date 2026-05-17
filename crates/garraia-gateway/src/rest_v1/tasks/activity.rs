//! Task activity log handler.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-635 (plan 0141, Q11 slice 6).
//!
//! One endpoint (plan 0080 / GAR-541):
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/activity` — cursor-paginated activity log
//!
//! Read-only: writes to `task_activity` are emitted inline by mutation handlers
//! (`create_task`, `patch_task`, `delete_task`, `create_task_comment`,
//! `add_task_assignee`, `remove_task_assignee`, `assign_task_label`,
//! `remove_task_label_from_task`).

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, can};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::super::RestV1FullState;
use super::super::problem::RestError;
use super::{DEFAULT_LIMIT, MAX_LIMIT, check_group_match, require_group_id, set_rls_context};

/// DB row from `task_activity`.
#[derive(sqlx::FromRow)]
struct ActivityRow {
    id: Uuid,
    kind: String,
    actor_label: String,
    payload: serde_json::Value,
    created_at: DateTime<Utc>,
}

/// Single activity event in the response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityResponse {
    pub id: Uuid,
    pub kind: String,
    pub actor_label: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl From<ActivityRow> for ActivityResponse {
    fn from(r: ActivityRow) -> Self {
        Self {
            id: r.id,
            kind: r.kind,
            actor_label: r.actor_label,
            payload: r.payload,
            created_at: r.created_at,
        }
    }
}

/// Response for `GET /v1/groups/{group_id}/tasks/{task_id}/activity`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListActivityResponse {
    pub items: Vec<ActivityResponse>,
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for the activity log endpoint.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListActivityQuery {
    /// Keyset cursor — UUID of the last activity row received. Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/activity` — cursor-paginated task activity log.
///
/// Returns activity events for the task, newest first. Cross-group tasks return
/// 404 (RLS filters them). Authz: `Action::TasksRead`.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/activity",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ListActivityQuery,
    ),
    responses(
        (status = 200, description = "Activity log.", body = ListActivityResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_activity(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<ListActivityQuery>,
) -> Result<Json<ListActivityResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = i64::from(effective_limit + 1);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify task exists in this group (and is not soft-deleted).
    let task_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM tasks WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if task_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let rows: Vec<ActivityRow> = if let Some(cursor_id) = params.cursor {
        sqlx::query_as(
            "SELECT id, kind, actor_label, payload, created_at \
             FROM task_activity \
             WHERE task_id = $1 \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM task_activity WHERE id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(task_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, kind, actor_label, payload, created_at \
             FROM task_activity \
             WHERE task_id = $1 \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(task_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<ActivityResponse> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(ActivityResponse::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListActivityResponse { items, next_cursor }))
}
