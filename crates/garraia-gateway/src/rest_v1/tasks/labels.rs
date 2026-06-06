//! Task label handlers.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-635 (plan 0138, Q11 slice 4).
//!
//! Six endpoints (plan 0078 / GAR-536 + plan 0267 / GAR-802):
//! - `POST /v1/groups/{group_id}/task-labels` — create a label
//! - `GET /v1/groups/{group_id}/task-labels` — list labels
//! - `GET /v1/groups/{group_id}/task-labels/{label_id}` — fetch single label
//! - `DELETE /v1/groups/{group_id}/task-labels/{label_id}` — delete label (CASCADE)
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/labels` — assign label to task
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}` — remove label from task

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

/// DB row returned from `task_labels`.
#[derive(sqlx::FromRow)]
struct TaskLabelRow {
    id: Uuid,
    group_id: Uuid,
    name: String,
    color: String,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
}

/// Public response shape for a single task label.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskLabelResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub color: String,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
}

impl From<TaskLabelRow> for TaskLabelResponse {
    fn from(r: TaskLabelRow) -> Self {
        Self {
            id: r.id,
            group_id: r.group_id,
            name: r.name,
            color: r.color,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
        }
    }
}

/// Request body for `POST /v1/groups/{group_id}/task-labels`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTaskLabelRequest {
    pub name: String,
    pub color: String,
}

/// `POST /v1/groups/{group_id}/task-labels` — create a task label for a group.
///
/// Returns 201 on success, 409 if a label with the same name already exists
/// in this group (`UNIQUE (group_id, name)`). Both `app.current_user_id` and
/// `app.current_group_id` are set before accessing any FORCE-RLS table.
/// Authz: `Action::TasksWrite`.
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/task-labels",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
    ),
    request_body = CreateTaskLabelRequest,
    responses(
        (status = 201, description = "Label created.", body = TaskLabelResponse),
        (status = 400, description = "Validation error.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "Label name already exists in this group.", body = super::super::problem::ProblemDetails),
        (status = 422, description = "Invalid color format (expected #RRGGBB).", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_task_label(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Json(body): Json<CreateTaskLabelRequest>,
) -> Result<(StatusCode, Json<TaskLabelResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }

    let color = body.color.trim().to_string();
    if !is_valid_hex_color(&color) {
        return Err(RestError::BadRequest(
            "color must be in #RRGGBB format".into(),
        ));
    }
    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 80 {
        return Err(RestError::BadRequest(
            "name must be 1\u{2013}80 characters".into(),
        ));
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let row: TaskLabelRow = sqlx::query_as(
        "INSERT INTO task_labels (group_id, name, color, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, group_id, name, color, created_by, created_by_label, created_at",
    )
    .bind(group_id)
    .bind(&name)
    .bind(&color)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("label name already exists in this group".into());
        }
        RestError::Internal(e.into())
    })?;

    let label_id = row.id;
    let name_len = name.len();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskLabelCreated,
        principal.user_id,
        group_id,
        "task_labels",
        label_id.to_string(),
        json!({ "name_len": name_len, "color": color }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(TaskLabelResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/task-labels` — list all labels for a group.
///
/// Returns labels ordered by `created_at ASC`. Authz: `Action::TasksRead`.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/task-labels",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
    ),
    responses(
        (status = 200, description = "List of task labels.", body = Vec<TaskLabelResponse>),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_labels(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
) -> Result<Json<Vec<TaskLabelResponse>>, RestError> {
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

    let rows: Vec<TaskLabelRow> = sqlx::query_as(
        "SELECT id, group_id, name, color, created_by, created_by_label, created_at \
         FROM task_labels \
         ORDER BY created_at ASC",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(
        rows.into_iter().map(TaskLabelResponse::from).collect(),
    ))
}

/// `GET /v1/groups/{group_id}/task-labels/{label_id}` — fetch a single task label.
///
/// Returns 200 + `TaskLabelResponse` on success. Returns 404 if `label_id` does
/// not exist or belongs to a different group (cross-group guard, no existence leak).
/// Authz: `Action::TasksRead`.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/task-labels/{label_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("label_id" = Uuid, Path, description = "Label UUID."),
    ),
    responses(
        (status = 200, description = "Task label.", body = TaskLabelResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Label not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_task_label(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, label_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskLabelResponse>, RestError> {
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

    let row: Option<TaskLabelRow> = sqlx::query_as(
        "SELECT id, group_id, name, color, created_by, created_by_label, created_at \
         FROM task_labels \
         WHERE id = $1 AND group_id = $2",
    )
    .bind(label_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;
    Ok(Json(TaskLabelResponse::from(row)))
}

/// `DELETE /v1/groups/{group_id}/task-labels/{label_id}` — delete a task label.
///
/// Idempotent: returns 204 whether or not the label existed. The DB CASCADE
/// removes all `task_label_assignments` referencing this label automatically.
/// Authz: `Action::TasksWrite`.
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/task-labels/{label_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("label_id" = Uuid, Path, description = "Label UUID."),
    ),
    responses(
        (status = 204, description = "Label deleted (or did not exist)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_task_label(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, label_id)): Path<(Uuid, Uuid)>,
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

    sqlx::query("DELETE FROM task_labels WHERE id = $1 AND group_id = $2")
        .bind(label_id)
        .bind(group_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskLabelDeleted,
        principal.user_id,
        group_id,
        "task_labels",
        label_id.to_string(),
        json!({ "assignments_cascade": true }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Request body for `PATCH /v1/groups/{group_id}/task-labels/{label_id}`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchTaskLabelRequest {
    /// New label name (1–80 chars). If absent the name is unchanged.
    pub name: Option<String>,
    /// New color in `#RRGGBB` format. If absent the color is unchanged.
    pub color: Option<String>,
}

/// `PATCH /v1/groups/{group_id}/task-labels/{label_id}` — edit a task label's name/color.
///
/// At least one of `name` or `color` must be provided. Returns the updated label
/// on success. 409 if the new name conflicts with an existing label in this group.
/// 404 if `label_id` does not exist or belongs to a different group.
/// Authz: `Action::TasksWrite`.
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/task-labels/{label_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("label_id" = Uuid, Path, description = "Label UUID."),
    ),
    request_body = PatchTaskLabelRequest,
    responses(
        (status = 200, description = "Label updated.", body = TaskLabelResponse),
        (status = 400, description = "Validation error or no fields provided.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Label not found in this group.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "Label name already exists in this group.", body = super::super::problem::ProblemDetails),
        (status = 422, description = "Invalid color format (expected #RRGGBB).", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_task_label(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, label_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchTaskLabelRequest>,
) -> Result<Json<TaskLabelResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }

    if body.name.is_none() && body.color.is_none() {
        return Err(RestError::BadRequest(
            "at least one of name or color must be provided".into(),
        ));
    }

    let name = body.name.as_deref().map(str::trim).map(str::to_string);
    let color = body.color.as_deref().map(str::trim).map(str::to_string);

    if let Some(ref n) = name
        && (n.is_empty() || n.len() > 80)
    {
        return Err(RestError::BadRequest(
            "name must be 1\u{2013}80 characters".into(),
        ));
    }
    if let Some(ref c) = color
        && !is_valid_hex_color(c)
    {
        return Err(RestError::BadRequest(
            "color must be in #RRGGBB format".into(),
        ));
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<TaskLabelRow> = sqlx::query_as(
        "UPDATE task_labels \
         SET name = COALESCE($3, name), color = COALESCE($4, color) \
         WHERE id = $1 AND group_id = $2 \
         RETURNING id, group_id, name, color, created_by, created_by_label, created_at",
    )
    .bind(label_id)
    .bind(group_id)
    .bind(&name)
    .bind(&color)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("label name already exists in this group".into());
        }
        RestError::Internal(e.into())
    })?;

    let row = row.ok_or(RestError::NotFound)?;

    let name_len = row.name.len();
    let updated_color = row.color.clone();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskLabelEdited,
        principal.user_id,
        group_id,
        "task_labels",
        label_id.to_string(),
        json!({ "name_len": name_len, "color": updated_color }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(TaskLabelResponse::from(row)))
}

/// DB row for a label assignment.
#[derive(sqlx::FromRow)]
struct LabelAssignmentRow {
    task_id: Uuid,
    label_id: Uuid,
    assigned_at: DateTime<Utc>,
}

/// Public response for a label assignment.
#[derive(Debug, Serialize, ToSchema)]
pub struct LabelAssignmentResponse {
    pub task_id: Uuid,
    pub label_id: Uuid,
    pub assigned_at: DateTime<Utc>,
}

/// Request body for `POST /v1/groups/{group_id}/tasks/{task_id}/labels`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignTaskLabelRequest {
    pub label_id: Uuid,
}

/// `POST /v1/groups/{group_id}/tasks/{task_id}/labels` — assign a label to a task.
///
/// Returns 201 on success, 409 if already assigned, 404 if task or label not
/// found in this group. Authz: `Action::TasksWrite`.
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/labels",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = AssignTaskLabelRequest,
    responses(
        (status = 201, description = "Label assigned.", body = LabelAssignmentResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task or label not found in this group.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "Label already assigned to this task.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn assign_task_label(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignTaskLabelRequest>,
) -> Result<(StatusCode, Json<LabelAssignmentResponse>), RestError> {
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

    // Verify label belongs to this group (cross-group injection guard).
    let label_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM task_labels WHERE id = $1 AND group_id = $2")
            .bind(body.label_id)
            .bind(group_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if label_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row: LabelAssignmentRow = sqlx::query_as(
        "INSERT INTO task_label_assignments (task_id, label_id) \
         VALUES ($1, $2) \
         RETURNING task_id, label_id, assigned_at",
    )
    .bind(task_id)
    .bind(body.label_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("label already assigned to this task".into());
        }
        RestError::Internal(e.into())
    })?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskLabelAssigned,
        principal.user_id,
        group_id,
        "task_label_assignments",
        task_id.to_string(),
        json!({ "label_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "labeled",
        json!({ "label_id": body.label_id.to_string() }),
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(LabelAssignmentResponse {
            task_id: row.task_id,
            label_id: row.label_id,
            assigned_at: row.assigned_at,
        }),
    ))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}` — remove a label from a task.
///
/// Idempotent: returns 204 whether or not the assignment existed.
/// Returns 404 if the task is not found in this group. Authz: `Action::TasksWrite`.
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ("label_id" = Uuid, Path, description = "Label UUID."),
    ),
    responses(
        (status = 204, description = "Label removed from task (or was not assigned)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn remove_task_label_from_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id, label_id)): Path<(Uuid, Uuid, Uuid)>,
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

    // Verify task exists in this group before emitting audit.
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

    sqlx::query("DELETE FROM task_label_assignments WHERE task_id = $1 AND label_id = $2")
        .bind(task_id)
        .bind(label_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskLabelRemoved,
        principal.user_id,
        group_id,
        "task_label_assignments",
        task_id.to_string(),
        json!({ "label_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "unlabeled",
        json!({ "label_id": label_id.to_string() }),
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Validates that a color string is in `#RRGGBB` hex format.
fn is_valid_hex_color(color: &str) -> bool {
    if color.len() != 7 {
        return false;
    }
    let bytes = color.as_bytes();
    if bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_request_name_only_roundtrip() {
        let json = r#"{"name":"urgent"}"#;
        let req: PatchTaskLabelRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name.as_deref(), Some("urgent"));
        assert!(req.color.is_none());
    }

    #[test]
    fn patch_request_color_only_roundtrip() {
        let json = r##"{"color":"#FF0000"}"##;
        let req: PatchTaskLabelRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert_eq!(req.color.as_deref(), Some("#FF0000"));
    }

    #[test]
    fn patch_request_both_absent_roundtrip() {
        let json = r#"{}"#;
        let req: PatchTaskLabelRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.color.is_none());
    }

    #[test]
    fn hex_color_valid_formats() {
        assert!(is_valid_hex_color("#AABBCC"));
        assert!(is_valid_hex_color("#000000"));
        assert!(is_valid_hex_color("#ffffff"));
        assert!(is_valid_hex_color("#1a2B3c"));
    }

    #[test]
    fn hex_color_invalid_formats() {
        assert!(!is_valid_hex_color("AABBCC"));
        assert!(!is_valid_hex_color("#AABBCCD"));
        assert!(!is_valid_hex_color("#AABBC"));
        assert!(!is_valid_hex_color("#GGHHII"));
        assert!(!is_valid_hex_color(""));
    }

    #[test]
    fn task_label_response_nil_uuid_round_trip() {
        let resp = TaskLabelResponse {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "test".to_string(),
            color: "#123456".to_string(),
            created_by: None,
            created_by_label: "Alice".to_string(),
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(val["name"], "test");
        assert_eq!(val["color"], "#123456");
        assert!(val.get("created_by").is_some());
    }

    #[test]
    fn get_label_response_all_fields_present() {
        let resp = TaskLabelResponse {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "bug".to_string(),
            color: "#FF0000".to_string(),
            created_by: Some(Uuid::nil()),
            created_by_label: "Bob".to_string(),
            created_at: DateTime::from_timestamp(1_000_000, 0).unwrap(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["name"], "bug");
        assert_eq!(val["color"], "#FF0000");
        assert_eq!(val["created_by_label"], "Bob");
        assert!(val["created_at"].is_string());
    }

    #[test]
    fn get_label_response_nil_created_by() {
        let resp = TaskLabelResponse {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "feature".to_string(),
            color: "#00FF00".to_string(),
            created_by: None,
            created_by_label: "system".to_string(),
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["created_by"].is_null());
    }

    #[test]
    fn get_label_response_utc_timestamp_z_suffix() {
        let resp = TaskLabelResponse {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "done".to_string(),
            color: "#0000FF".to_string(),
            created_by: None,
            created_by_label: "Alice".to_string(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        let ts = val["created_at"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "expected UTC Z-suffix, got: {ts}");
    }

    #[test]
    fn get_label_response_has_exactly_seven_fields() {
        let resp = TaskLabelResponse {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "x".to_string(),
            color: "#AABBCC".to_string(),
            created_by: None,
            created_by_label: "y".to_string(),
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        let obj = val.as_object().unwrap();
        assert_eq!(
            obj.len(),
            7,
            "expected 7 fields: id, group_id, name, color, created_by, created_by_label, created_at"
        );
    }
}
