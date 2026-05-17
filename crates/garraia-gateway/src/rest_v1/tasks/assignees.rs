//! Task assignee handlers.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-635 (plan 0137, Q11 slice 3).
//!
//! Three endpoints (plan 0077 / GAR-533):
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/assignees` — assign a group member
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/assignees` — list assignees
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}` — remove assignee (idempotent)

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::RestV1FullState;
use super::super::problem::RestError;
use super::{check_group_match, insert_task_activity, require_group_id, set_rls_context};

/// DB row returned from `task_assignees`.
#[derive(sqlx::FromRow)]
struct AssigneeRow {
    user_id: Uuid,
    assigned_at: DateTime<Utc>,
    assigned_by: Option<Uuid>,
}

/// Public response shape for a single assignee record.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssigneeResponse {
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub assigned_at: DateTime<Utc>,
    pub assigned_by: Option<Uuid>,
}

/// Request body for `POST /v1/groups/{group_id}/tasks/{task_id}/assignees`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddAssigneeRequest {
    pub user_id: Uuid,
}

/// `POST /v1/groups/{group_id}/tasks/{task_id}/assignees` — assign a group member to a task.
///
/// Returns 201 on success, 409 if already assigned, 404 if task or target user
/// not found in the group. Both `app.current_user_id` and `app.current_group_id`
/// are set before accessing any FORCE-RLS table. Authz: `Action::TasksWrite`.
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/assignees",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = AddAssigneeRequest,
    responses(
        (status = 201, description = "Assignee added.", body = AssigneeResponse),
        (status = 400, description = "Validation error.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task or target user not found in this group.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "User already assigned to this task.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn add_task_assignee(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddAssigneeRequest>,
) -> Result<(StatusCode, Json<AssigneeResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify task exists in this group and is not soft-deleted.
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

    // Verify target user is an active member of this group.
    // Cross-group injection: a user_id from another group returns 404 (never 403).
    let member_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM group_members \
         WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
    )
    .bind(group_id)
    .bind(body.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if member_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row: AssigneeRow = sqlx::query_as(
        "INSERT INTO task_assignees (task_id, user_id, assigned_by) \
         VALUES ($1, $2, $3) \
         RETURNING user_id, assigned_at, assigned_by",
    )
    .bind(task_id)
    .bind(body.user_id)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("user already assigned to this task".into());
        }
        RestError::Internal(e.into())
    })?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskAssigneeAdded,
        principal.user_id,
        group_id,
        "task_assignees",
        task_id.to_string(),
        json!({ "assignee_user_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "assigned",
        json!({ "assignee_id": body.user_id.to_string() }),
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(AssigneeResponse {
            task_id,
            user_id: row.user_id,
            assigned_at: row.assigned_at,
            assigned_by: row.assigned_by,
        }),
    ))
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/assignees` — list assignees for a task.
///
/// Returns all current assignees ordered by `assigned_at ASC`.
/// Authz: `Action::TasksRead`.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/assignees",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 200, description = "List of assignees.", body = Vec<AssigneeResponse>),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_assignees(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<AssigneeResponse>>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Explicit 404 if task not found (RLS also filters, but clearer UX).
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

    let rows: Vec<AssigneeRow> = sqlx::query_as(
        "SELECT user_id, assigned_at, assigned_by \
         FROM task_assignees \
         WHERE task_id = $1 \
         ORDER BY assigned_at ASC",
    )
    .bind(task_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items = rows
        .into_iter()
        .map(|r| AssigneeResponse {
            task_id,
            user_id: r.user_id,
            assigned_at: r.assigned_at,
            assigned_by: r.assigned_by,
        })
        .collect();

    Ok(Json(items))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}` — remove an assignee.
///
/// Idempotent: returns 204 whether or not the row existed.
/// Authz: `Action::TasksWrite`.
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ("user_id" = Uuid, Path, description = "User to unassign."),
    ),
    responses(
        (status = 204, description = "Assignee removed (or was never assigned)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn remove_task_assignee(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id, assignee_user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify task exists before emitting audit.
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

    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("DELETE FROM task_assignees WHERE task_id = $1 AND user_id = $2")
        .bind(task_id)
        .bind(assignee_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskAssigneeRemoved,
        principal.user_id,
        group_id,
        "task_assignees",
        task_id.to_string(),
        json!({ "assignee_user_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "unassigned",
        json!({ "assignee_id": assignee_user_id.to_string() }),
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}
