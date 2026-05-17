//! Task attachment handlers.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-658 (plan 0143, Q11 slice 7).
//!
//! Three endpoints (plan 0096 / GAR-572, slice 9):
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/attachments` — attach a file
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/attachments` — list attachments
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}` — detach a file

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::super::RestV1FullState;
use super::super::problem::RestError;
use super::{DEFAULT_LIMIT, MAX_LIMIT, check_group_match, require_group_id, set_rls_context};

/// Request body for `POST /v1/groups/{group_id}/tasks/{task_id}/attachments`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachFileRequest {
    pub file_id: Uuid,
}

/// DB row for a single `task_attachments` record.
#[derive(sqlx::FromRow)]
struct TaskAttachmentRow {
    task_id: Uuid,
    file_id: Uuid,
    attached_by: Option<Uuid>,
    attached_at: DateTime<Utc>,
    file_name: String,
    mime_type: String,
    size_bytes: i64,
}

/// Public response shape for a single task attachment.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskAttachmentResponse {
    pub task_id: Uuid,
    pub file_id: Uuid,
    pub attached_by: Option<Uuid>,
    pub attached_at: DateTime<Utc>,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
}

impl From<TaskAttachmentRow> for TaskAttachmentResponse {
    fn from(r: TaskAttachmentRow) -> Self {
        Self {
            task_id: r.task_id,
            file_id: r.file_id,
            attached_by: r.attached_by,
            attached_at: r.attached_at,
            file_name: r.file_name,
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
        }
    }
}

/// Response envelope for `GET .../attachments`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListAttachmentsResponse {
    pub items: Vec<TaskAttachmentResponse>,
    pub next_cursor: Option<Uuid>,
}

/// Query params for `GET .../attachments`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListAttachmentsQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u32>,
}

/// `POST /v1/groups/{group_id}/tasks/{task_id}/attachments` — attach a file to a task.
///
/// The file must belong to the same group and must not be soft-deleted.
/// Returns 409 if the file is already attached.
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/attachments",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = AttachFileRequest,
    responses(
        (status = 201, description = "File attached to task.", body = TaskAttachmentResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task or file not found in this group.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "File already attached to this task.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.post_task_attachment",
    skip_all,
    fields(task_id = %task_id, file_id = %body.file_id)
)]
pub async fn post_task_attachment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AttachFileRequest>,
) -> Result<(StatusCode, Json<TaskAttachmentResponse>), RestError> {
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

    // Verify task exists and is not soft-deleted.
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

    // Verify file belongs to this group and is not soft-deleted.
    // Cross-group injection: file from another group is invisible via RLS → 0 rows → 404.
    let file_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM files \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(body.file_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if file_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // INSERT and join back to files for the response payload.
    let row: TaskAttachmentRow = sqlx::query_as(
        "INSERT INTO task_attachments \
             (task_id, file_id, group_id, attached_by, attached_by_label, attached_at) \
         VALUES ($1, $2, $3, $4, $5, now()) \
         RETURNING \
             task_id, file_id, attached_by, attached_at, \
             (SELECT name FROM files WHERE id = $2)      AS file_name, \
             (SELECT mime_type FROM files WHERE id = $2)  AS mime_type, \
             (SELECT size_bytes FROM files WHERE id = $2) AS size_bytes",
    )
    .bind(task_id)
    .bind(body.file_id)
    .bind(group_id)
    .bind(principal.user_id)
    .bind(&actor_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("file already attached to this task".into());
        }
        RestError::Internal(e.into())
    })?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskFileAttached,
        principal.user_id,
        group_id,
        "task_attachments",
        task_id.to_string(),
        json!({ "task_id": task_id.to_string(), "file_id": body.file_id.to_string() }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(TaskAttachmentResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/attachments` — list files attached to a task.
///
/// Cursor-paginated (by `attached_at ASC, file_id ASC`). Default limit 50, max 100.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/attachments",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ListAttachmentsQuery,
    ),
    responses(
        (status = 200, description = "Paginated list of attachments.", body = ListAttachmentsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.list_task_attachments",
    skip_all,
    fields(task_id = %task_id)
)]
pub async fn list_task_attachments(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<ListAttachmentsQuery>,
) -> Result<Json<ListAttachmentsResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Explicit 404 for deleted/missing tasks.
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

    let rows: Vec<TaskAttachmentRow> = if let Some(cursor) = params.cursor {
        sqlx::query_as(
            "SELECT ta.task_id, ta.file_id, ta.attached_by, ta.attached_at, \
                    f.name AS file_name, f.mime_type, f.size_bytes \
             FROM task_attachments ta \
             JOIN files f ON f.id = ta.file_id AND f.deleted_at IS NULL \
             WHERE ta.task_id = $1 \
               AND (ta.attached_at, ta.file_id) > ( \
                   SELECT attached_at, file_id FROM task_attachments \
                   WHERE task_id = $1 AND file_id = $2 \
               ) \
             ORDER BY ta.attached_at ASC, ta.file_id ASC \
             LIMIT $3",
        )
        .bind(task_id)
        .bind(cursor)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT ta.task_id, ta.file_id, ta.attached_by, ta.attached_at, \
                    f.name AS file_name, f.mime_type, f.size_bytes \
             FROM task_attachments ta \
             JOIN files f ON f.id = ta.file_id AND f.deleted_at IS NULL \
             WHERE ta.task_id = $1 \
             ORDER BY ta.attached_at ASC, ta.file_id ASC \
             LIMIT $2",
        )
        .bind(task_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as i64 > limit;
    let items: Vec<_> = rows
        .into_iter()
        .take(limit as usize)
        .map(TaskAttachmentResponse::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|r| r.file_id)
    } else {
        None
    };

    Ok(Json(ListAttachmentsResponse { items, next_cursor }))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}` — detach a file.
///
/// Idempotent: returns 204 whether or not the attachment row existed.
/// Emits audit only when a row was actually deleted.
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ("file_id" = Uuid, Path, description = "File UUID to detach."),
    ),
    responses(
        (status = 204, description = "File detached (or was never attached)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.delete_task_attachment",
    skip_all,
    fields(task_id = %task_id, file_id = %file_id)
)]
pub async fn delete_task_attachment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id, file_id)): Path<(Uuid, Uuid, Uuid)>,
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

    let deleted = sqlx::query("DELETE FROM task_attachments WHERE task_id = $1 AND file_id = $2")
        .bind(task_id)
        .bind(file_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Emit audit only when a row was actually removed (idempotent: no event on no-op).
    if deleted.rows_affected() > 0 {
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::TaskFileDetached,
            principal.user_id,
            group_id,
            "task_attachments",
            task_id.to_string(),
            json!({ "task_id": task_id.to_string(), "file_id": file_id.to_string() }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}
