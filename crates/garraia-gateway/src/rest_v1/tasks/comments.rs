//! Task comment CRUD handlers.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-635 (plan 0136, Q11 slice 2).
//!
//! Three endpoints (plan 0069 / GAR-520):
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/comments` — create a comment
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/comments` — cursor-paginated list
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` — soft-delete

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
use super::{
    DEFAULT_LIMIT, MAX_LIMIT, check_group_match, insert_task_activity, require_group_id,
    set_rls_context,
};

/// DB row struct for `task_comments`.
#[derive(sqlx::FromRow)]
struct CommentRow {
    id: Uuid,
    task_id: Uuid,
    author_user_id: Option<Uuid>,
    author_label: String,
    body_md: String,
    created_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
}

/// Full comment representation returned by `POST` and included in `GET` list.
#[derive(Debug, Serialize, ToSchema)]
pub struct CommentResponse {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author_user_id: Option<Uuid>,
    pub author_label: String,
    pub body_md: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

impl From<CommentRow> for CommentResponse {
    fn from(r: CommentRow) -> Self {
        Self {
            id: r.id,
            task_id: r.task_id,
            author_user_id: r.author_user_id,
            author_label: r.author_label,
            body_md: r.body_md,
            created_at: r.created_at,
            edited_at: r.edited_at,
        }
    }
}

/// Request body for `POST /v1/groups/{group_id}/tasks/{task_id}/comments`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCommentRequest {
    /// Markdown comment body. 1–50,000 characters.
    pub body_md: String,
}

impl CreateCommentRequest {
    fn validate(&self) -> Result<(), &'static str> {
        let len = self.body_md.chars().count();
        if len == 0 {
            return Err("body_md must not be empty");
        }
        if len > 50_000 {
            return Err("body_md exceeds 50,000 character limit");
        }
        Ok(())
    }
}

/// Query parameters for `GET /v1/groups/{group_id}/tasks/{task_id}/comments`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListCommentsQuery {
    /// Cursor: the `id` of the last comment on the previous page.
    pub cursor: Option<Uuid>,
    /// Page size. 1–100. Defaults to 50.
    pub limit: Option<u32>,
}

/// Response body for `GET /v1/groups/{group_id}/tasks/{task_id}/comments`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListCommentsResponse {
    pub items: Vec<CommentResponse>,
    pub next_cursor: Option<Uuid>,
}

// ─── Handlers — slice 3 (plan 0069 / GAR-520) ────────────────────────────────

/// `POST /v1/groups/{group_id}/tasks/{task_id}/comments` — create a comment.
///
/// Author label is resolved from the caller's `display_name` in the `users` table.
/// Returns 404 if the task does not exist or belongs to a different group.
/// Authz: `Action::TasksWrite`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Validation failure                 | 400    |
/// | Task not found / cross-tenant      | 404    |
/// | Happy path                         | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/comments",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = CreateCommentRequest,
    responses(
        (status = 201, description = "Comment created.", body = CommentResponse),
        (status = 400, description = "Validation error.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_task_comment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateCommentRequest>,
) -> Result<(StatusCode, Json<CommentResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify the task exists in this group (and is not soft-deleted).
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

    let (author_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row: CommentRow = sqlx::query_as(
        "INSERT INTO task_comments \
             (task_id, author_user_id, author_label, body_md) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, task_id, author_user_id, author_label, body_md, \
                   created_at, edited_at",
    )
    .bind(task_id)
    .bind(principal.user_id)
    .bind(&author_label)
    .bind(&body.body_md)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let comment_id = row.id;
    let body_len = body.body_md.chars().count();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskCommentCreated,
        principal.user_id,
        group_id,
        "task_comments",
        comment_id.to_string(),
        json!({ "body_len": body_len }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &author_label,
        "commented",
        json!({ "body_len": body_len }),
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(CommentResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/comments` — list comments.
///
/// Returns non-deleted comments for the task, newest first, cursor-paginated.
/// Returns 404 if the task does not exist or belongs to a different group.
/// Authz: `Action::TasksRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Task not found / cross-tenant      | 404    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/comments",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ListCommentsQuery,
    ),
    responses(
        (status = 200, description = "Comment list.", body = ListCommentsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_comments(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<ListCommentsQuery>,
) -> Result<Json<ListCommentsResponse>, RestError> {
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

    // Verify task exists in this group.
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

    let rows: Vec<CommentRow> = if let Some(cursor_id) = params.cursor {
        sqlx::query_as(
            "SELECT id, task_id, author_user_id, author_label, body_md, \
                    created_at, edited_at \
             FROM task_comments \
             WHERE task_id = $1 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM task_comments WHERE id = $2 \
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
            "SELECT id, task_id, author_user_id, author_label, body_md, \
                    created_at, edited_at \
             FROM task_comments \
             WHERE task_id = $1 AND deleted_at IS NULL \
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
    let items: Vec<CommentResponse> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(CommentResponse::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListCommentsResponse { items, next_cursor }))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` — soft-delete a comment.
///
/// Sets `deleted_at = now()`. Returns 404 if the comment does not exist,
/// is already deleted, or belongs to a task in a different group (RLS).
/// Authz: `Action::TasksWrite`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Comment not found / deleted / cross-tenant | 404 |
/// | Happy path                         | 204    |
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
        ("comment_id" = Uuid, Path, description = "Comment UUID."),
    ),
    responses(
        (status = 204, description = "Comment deleted."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Comment not found, already deleted, or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_task_comment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id, comment_id)): Path<(Uuid, Uuid, Uuid)>,
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

    // Fetch comment to get body_len for audit; also verifies it exists and is not deleted.
    // RLS JOIN policy scopes to current group via tasks.
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT body_md FROM task_comments \
         WHERE id = $1 AND task_id = $2 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (body_md,) = match existing {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    let body_len = body_md.chars().count();

    sqlx::query(
        "UPDATE task_comments SET deleted_at = now() \
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskCommentDeleted,
        principal.user_id,
        group_id,
        "task_comments",
        comment_id.to_string(),
        json!({ "body_len": body_len }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}
