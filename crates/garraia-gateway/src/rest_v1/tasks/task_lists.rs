//! Task-list CRUD handlers: `POST`, `GET` (list), `GET` (single), `PATCH`, `DELETE`.
//!
//! Extracted from `rest_v1/tasks.rs` by GAR-635 (plan 0135, Q11 slice 1).
//!
//! Five endpoints:
//! - `POST /v1/groups/{group_id}/task-lists` — create task list
//! - `GET /v1/groups/{group_id}/task-lists` — cursor-paginated list
//! - `GET /v1/groups/{group_id}/task-lists/{list_id}` — fetch single
//! - `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update name/type/description
//! - `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive (idempotent)

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

// ─── Constant (task-list-specific) ───────────────────────────────────────────

pub(super) const ALLOWED_LIST_TYPES: &[&str] = &["list", "board", "calendar"];

// ─── Private DB row struct ────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
pub(super) struct TaskListRow {
    pub(super) id: Uuid,
    pub(super) group_id: Uuid,
    pub(super) name: String,
    pub(super) list_type: String,
    pub(super) description: Option<String>,
    pub(super) created_by: Option<Uuid>,
    pub(super) created_by_label: String,
    pub(super) created_at: DateTime<Utc>,
    pub(super) updated_at: DateTime<Utc>,
    pub(super) archived_at: Option<DateTime<Utc>>,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Request body for `POST /v1/groups/{group_id}/task-lists`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskListRequest {
    /// Display name. 1–200 characters.
    pub name: String,
    /// View type. One of `"list"`, `"board"`, `"calendar"`.
    #[serde(rename = "type")]
    pub list_type: String,
    /// Optional description.
    pub description: Option<String>,
}

impl CreateTaskListRequest {
    fn validate(&self) -> Result<(), &'static str> {
        let name_chars = self.name.chars().count();
        if name_chars == 0 {
            return Err("name must not be empty");
        }
        if name_chars > 200 {
            return Err("name exceeds 200 character limit");
        }
        if !ALLOWED_LIST_TYPES.contains(&self.list_type.as_str()) {
            return Err("type must be one of: list, board, calendar");
        }
        Ok(())
    }
}

/// Request body for `PATCH /v1/groups/{group_id}/task-lists/{list_id}`.
///
/// All fields are optional. `description` supports three-way semantics:
/// omit key to leave unchanged, `null` to clear, string to update.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchTaskListRequest {
    /// Updated name. 1–200 characters when provided.
    pub name: Option<String>,
    /// Updated description. Pass `null` explicitly to clear. Omit to leave unchanged.
    #[serde(default, deserialize_with = "super::option_nullable::deserialize")]
    #[schema(value_type = Option<String>, nullable = true)]
    pub description: Option<Option<String>>,
    /// Updated type. One of `"list"`, `"board"`, `"calendar"`.
    #[serde(rename = "type")]
    pub list_type: Option<String>,
}

impl PatchTaskListRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if let Some(name) = &self.name {
            let len = name.chars().count();
            if len == 0 {
                return Err("name must not be empty");
            }
            if len > 200 {
                return Err("name exceeds 200 character limit");
            }
        }
        if let Some(lt) = &self.list_type
            && !ALLOWED_LIST_TYPES.contains(&lt.as_str())
        {
            return Err("type must be one of: list, board, calendar");
        }
        Ok(())
    }
}

/// Full task list representation returned by `POST` and single-item `GET`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskListResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: String,
    pub description: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl From<TaskListRow> for TaskListResponse {
    fn from(r: TaskListRow) -> Self {
        Self {
            id: r.id,
            group_id: r.group_id,
            name: r.name,
            list_type: r.list_type,
            description: r.description,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
            archived_at: r.archived_at,
        }
    }
}

/// Compact task list item used in `GET /v1/groups/{group_id}/task-lists`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskListSummary {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<TaskListRow> for TaskListSummary {
    fn from(r: TaskListRow) -> Self {
        Self {
            id: r.id,
            group_id: r.group_id,
            name: r.name,
            list_type: r.list_type,
            description: r.description,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Response body for `GET /v1/groups/{group_id}/task-lists`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListTaskListsResponse {
    pub items: Vec<TaskListSummary>,
    /// Cursor for the next page. `None` when end of list is reached.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/task-lists`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListTaskListsQuery {
    /// Keyset cursor — UUID of the last item received. Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/groups/{group_id}/task-lists` — create a task list.
///
/// Authz: `Action::TasksWrite`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Validation failure                 | 400    |
/// | Happy path                         | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/task-lists",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
    ),
    request_body = CreateTaskListRequest,
    responses(
        (status = 201, description = "Task list created.", body = TaskListResponse),
        (status = 400, description = "Validation error.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_task_list(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Json(body): Json<CreateTaskListRequest>,
) -> Result<(StatusCode, Json<TaskListResponse>), RestError> {
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

    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let row: TaskListRow = sqlx::query_as(
        "INSERT INTO task_lists \
             (group_id, name, type, description, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, group_id, name, type AS list_type, description, \
                   created_by, created_by_label, created_at, updated_at, archived_at",
    )
    .bind(group_id)
    .bind(&body.name)
    .bind(&body.list_type)
    .bind(&body.description)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let list_id = row.id;
    let name_len = body.name.chars().count();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskListCreated,
        principal.user_id,
        group_id,
        "task_lists",
        list_id.to_string(),
        json!({ "name_len": name_len, "type": body.list_type }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(TaskListResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/task-lists` — list task lists (cursor-paginated).
///
/// Returns non-archived task lists for the caller's group, newest first.
/// Authz: `Action::TasksRead`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/task-lists",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListTaskListsQuery,
    ),
    responses(
        (status = 200, description = "Task lists.", body = ListTaskListsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_lists(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListTaskListsQuery>,
) -> Result<Json<ListTaskListsResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<TaskListRow> = if let Some(cursor_id) = params.cursor {
        sqlx::query_as(
            "SELECT id, group_id, name, type AS list_type, description, \
                    created_by, created_by_label, created_at, updated_at, archived_at \
             FROM task_lists \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM task_lists WHERE id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, group_id, name, type AS list_type, description, \
                    created_by, created_by_label, created_at, updated_at, archived_at \
             FROM task_lists \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(group_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<TaskListSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(TaskListSummary::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListTaskListsResponse { items, next_cursor }))
}

/// `GET /v1/groups/{group_id}/task-lists/{list_id}` — fetch a single task list.
///
/// Returns 404 for archived, cross-tenant, or non-existent lists (no 403 leak).
/// Authz: `Action::TasksRead`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                               | Status |
/// |-----------------------------------------|--------|
/// | Missing/invalid JWT                     | 401    |
/// | Non-member of group                     | 403    |
/// | Path group_id ≠ principal group_id      | 403    |
/// | List not found / archived / cross-tenant | 404   |
/// | Happy path                              | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/task-lists/{list_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("list_id" = Uuid, Path, description = "Task list UUID."),
    ),
    responses(
        (status = 200, description = "Task list.", body = TaskListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task list not found, archived, or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_task_list(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, list_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskListResponse>, RestError> {
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

    let row: Option<TaskListRow> = sqlx::query_as(
        "SELECT id, group_id, name, type AS list_type, description, \
                created_by, created_by_label, created_at, updated_at, archived_at \
         FROM task_lists \
         WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(list_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    match row {
        Some(r) => Ok(Json(TaskListResponse::from(r))),
        None => Err(RestError::NotFound),
    }
}

/// `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update a task list.
///
/// All fields are optional. `description` supports three-way semantics: omit
/// the key to leave unchanged, send `null` to clear, send a string to update.
/// Returns 404 for archived, cross-tenant, or non-existent lists (RLS).
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
/// | List not found / archived / cross-tenant | 404 |
/// | Happy path                         | 200    |
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/task-lists/{list_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("list_id" = Uuid, Path, description = "Task list UUID."),
    ),
    request_body = PatchTaskListRequest,
    responses(
        (status = 200, description = "Task list updated.", body = TaskListResponse),
        (status = 400, description = "Validation error.", body = super::super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task list not found, archived, or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_task_list(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, list_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchTaskListRequest>,
) -> Result<Json<TaskListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksWrite) {
        return Err(RestError::Forbidden);
    }
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // Three-way description semantics:
    //   body.description = None         → key absent, CASE guard = false → keep existing
    //   body.description = Some(None)   → explicit null → clear to NULL
    //   body.description = Some(Some(s)) → update to s
    let description_changed = body.description.is_some();
    let new_description: Option<String> = body.description.flatten();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<TaskListRow> = sqlx::query_as(
        "UPDATE task_lists \
         SET name        = COALESCE($2, name), \
             type        = COALESCE($3, type), \
             description = CASE WHEN $4 THEN $5 ELSE description END, \
             updated_at  = now() \
         WHERE id = $1 \
           AND group_id = $6 \
           AND archived_at IS NULL \
         RETURNING id, group_id, name, type AS list_type, description, \
                   created_by, created_by_label, created_at, updated_at, archived_at",
    )
    .bind(list_id)
    .bind(&body.name)
    .bind(&body.list_type)
    .bind(description_changed)
    .bind(&new_description)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    let name_len = row.name.chars().count();
    let list_type = row.list_type.clone();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskListUpdated,
        principal.user_id,
        group_id,
        "task_lists",
        list_id.to_string(),
        json!({ "name_len": name_len, "type": list_type }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(TaskListResponse::from(row)))
}

/// `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive a task list.
///
/// Sets `archived_at = now()`. Tasks inside are NOT deleted; they become
/// de-listed from the default UI view. **Idempotent**: a second call on an
/// already-archived list returns 204 without error. Returns 404 only when the
/// list does not exist or belongs to another group (RLS). Authz: `Action::TasksDelete`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | List not found / cross-tenant      | 404    |
/// | Happy path (including re-archive)  | 204    |
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/task-lists/{list_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("list_id" = Uuid, Path, description = "Task list UUID."),
    ),
    responses(
        (status = 204, description = "Task list archived (or already archived)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task list not found or cross-tenant.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_task_list(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, list_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch including already-archived rows so we can distinguish
    // "list doesn't exist / cross-tenant" (→ 404) from "already archived" (→ idempotent 204).
    let existing: Option<(bool, String, String)> = sqlx::query_as(
        "SELECT archived_at IS NOT NULL, name, type \
         FROM task_lists \
         WHERE id = $1 AND group_id = $2",
    )
    .bind(list_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (already_archived, name, list_type) = match existing {
        None => return Err(RestError::NotFound),
        Some(r) => r,
    };

    if !already_archived {
        let name_len = name.chars().count();

        sqlx::query(
            "UPDATE task_lists \
             SET archived_at = now(), updated_at = now() \
             WHERE id = $1 AND archived_at IS NULL",
        )
        .bind(list_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::TaskListArchived,
            principal.user_id,
            group_id,
            "task_lists",
            list_id.to_string(),
            json!({ "name_len": name_len, "type": list_type }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn task_list_response_from_row_roundtrip() {
        let now = Utc::now();
        let row = TaskListRow {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "Backlog".into(),
            list_type: "list".into(),
            description: Some("desc".into()),
            created_by: None,
            created_by_label: "Alice".into(),
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let resp = TaskListResponse::from(row);
        assert_eq!(resp.name, "Backlog");
        assert_eq!(resp.list_type, "list");
        assert_eq!(resp.description, Some("desc".into()));
        assert!(resp.archived_at.is_none());
    }
}
