//! `/v1/groups/{group_id}/task-lists` and task/comment handlers (plan 0066/0067/0069, GAR-516/GAR-518/GAR-520).
//!
//! Twelve endpoints on the `garraia_app` RLS-enforced pool:
//!
//! **Slice 1 (plan 0066 / GAR-516):**
//! - `POST /v1/groups/{group_id}/task-lists` — create task list
//! - `GET /v1/groups/{group_id}/task-lists` — cursor-paginated list
//! - `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` — create task
//! - `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks` — cursor-paginated task list
//! - `PATCH /v1/groups/{group_id}/tasks/{task_id}` — update task fields
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}` — soft-delete
//!
//! **Slice 2 (plan 0067 / GAR-518):**
//! - `GET /v1/groups/{group_id}/tasks/{task_id}` — fetch single task
//! - `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update task list name/type/description
//! - `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive task list (idempotent)
//!
//! **Slice 3 (plan 0069 / GAR-520):**
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/comments` — create comment
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/comments` — cursor-paginated comment list
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` — soft-delete comment
//!
//! ## Tenant-context protocol
//!
//! `task_lists` and `tasks` use FORCE RLS with direct `group_id` isolation
//! (migration 006). Both RLS vars set via parameterized `set_config` (plan 0056).
//!
//! ## App-layer group validation
//!
//! Path `{group_id}` must equal `principal.group_id` — mismatch returns 403.
//! The compound FK `(list_id, group_id) → task_lists(id, group_id)` also
//! prevents cross-group task creation at the DB level.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

const ALLOWED_LIST_TYPES: &[&str] = &["list", "board", "calendar"];
const ALLOWED_STATUSES: &[&str] = &[
    "backlog",
    "todo",
    "in_progress",
    "review",
    "done",
    "canceled",
];
const ALLOWED_PRIORITIES: &[&str] = &["none", "low", "medium", "high", "urgent"];

// ─── Serde helper: Option<Option<T>> three-way deserializer ──────────────────
//
// Allows PATCH fields to distinguish:
//   key absent  → None            (leave unchanged)
//   key: null   → Some(None)      (clear to null)
//   key: "val"  → Some(Some(val)) (update to value)
//
// Usage: #[serde(default, deserialize_with = "option_nullable::deserialize")]
mod option_nullable {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Some(Option::<T>::deserialize(d)?))
    }
}

// ─── Private DB row structs ───────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct TaskListRow {
    id: Uuid,
    group_id: Uuid,
    name: String,
    list_type: String,
    description: Option<String>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: Uuid,
    list_id: Uuid,
    group_id: Uuid,
    parent_task_id: Option<Uuid>,
    title: String,
    description_md: Option<String>,
    status: String,
    priority: String,
    due_at: Option<DateTime<Utc>>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    estimated_minutes: Option<i32>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
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
    #[serde(default, deserialize_with = "option_nullable::deserialize")]
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

/// Request body for `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    /// Task title. 1–500 characters.
    pub title: String,
    /// Optional markdown description. 1–50,000 characters when provided.
    pub description_md: Option<String>,
    /// Initial status. One of `"backlog"`, `"todo"`, `"in_progress"`, `"review"`, `"done"`, `"canceled"`. Defaults to `"todo"`.
    #[serde(default = "default_status")]
    pub status: String,
    /// Priority tier. One of `"none"`, `"low"`, `"medium"`, `"high"`, `"urgent"`. Defaults to `"none"`.
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Optional due date (UTC).
    pub due_at: Option<DateTime<Utc>>,
    /// Optional time estimate in minutes. 0–100,000.
    pub estimated_minutes: Option<i32>,
}

fn default_status() -> String {
    "todo".to_string()
}
fn default_priority() -> String {
    "none".to_string()
}

impl CreateTaskRequest {
    fn validate(&self) -> Result<(), &'static str> {
        let title_chars = self.title.chars().count();
        if title_chars == 0 {
            return Err("title must not be empty");
        }
        if title_chars > 500 {
            return Err("title exceeds 500 character limit");
        }
        if !ALLOWED_STATUSES.contains(&self.status.as_str()) {
            return Err(
                "status must be one of: backlog, todo, in_progress, review, done, canceled",
            );
        }
        if !ALLOWED_PRIORITIES.contains(&self.priority.as_str()) {
            return Err("priority must be one of: none, low, medium, high, urgent");
        }
        if let Some(desc) = &self.description_md {
            let len = desc.chars().count();
            if len == 0 {
                return Err("description_md must not be empty when provided");
            }
            if len > 50_000 {
                return Err("description_md exceeds 50,000 character limit");
            }
        }
        if let Some(mins) = self.estimated_minutes
            && !(0..=100_000).contains(&mins)
        {
            return Err("estimated_minutes must be between 0 and 100000");
        }
        Ok(())
    }
}

/// Full task representation returned by `POST`, `GET`, and `PATCH`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: Uuid,
    pub list_id: Uuid,
    pub group_id: Uuid,
    pub parent_task_id: Option<Uuid>,
    pub title: String,
    pub description_md: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub estimated_minutes: Option<i32>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl From<TaskRow> for TaskResponse {
    fn from(r: TaskRow) -> Self {
        Self {
            id: r.id,
            list_id: r.list_id,
            group_id: r.group_id,
            parent_task_id: r.parent_task_id,
            title: r.title,
            description_md: r.description_md,
            status: r.status,
            priority: r.priority,
            due_at: r.due_at,
            started_at: r.started_at,
            completed_at: r.completed_at,
            estimated_minutes: r.estimated_minutes,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }
}

/// Compact task item used in `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskSummary {
    pub id: Uuid,
    pub list_id: Uuid,
    pub group_id: Uuid,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListTasksResponse {
    pub items: Vec<TaskSummary>,
    /// Cursor for the next page. `None` when end of list is reached.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListTasksQuery {
    /// Optional status filter. One of `backlog`, `todo`, `in_progress`, `review`, `done`, `canceled`.
    pub status: Option<String>,
    /// Keyset cursor — UUID of the last item received. Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Request body for `PATCH /v1/groups/{group_id}/tasks/{task_id}`.
///
/// All fields are optional. Only provided (non-null) fields are updated.
/// Note: nullable fields (`due_at`, `description_md`) cannot be cleared to
/// `null` via PATCH in slice 1 — omit the field to leave it unchanged.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchTaskRequest {
    /// Updated title. 1–500 characters when provided.
    pub title: Option<String>,
    /// Updated description. 1–50,000 characters when provided.
    pub description_md: Option<String>,
    /// Updated status.
    pub status: Option<String>,
    /// Updated priority.
    pub priority: Option<String>,
    /// Updated due date. Cannot be set to null via PATCH (omit to leave unchanged).
    pub due_at: Option<DateTime<Utc>>,
    /// Updated time estimate. Cannot be set to null via PATCH.
    pub estimated_minutes: Option<i32>,
}

impl PatchTaskRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if let Some(t) = &self.title {
            let len = t.chars().count();
            if len == 0 {
                return Err("title must not be empty");
            }
            if len > 500 {
                return Err("title exceeds 500 character limit");
            }
        }
        if let Some(d) = &self.description_md {
            let len = d.chars().count();
            if len == 0 {
                return Err("description_md must not be empty when provided");
            }
            if len > 50_000 {
                return Err("description_md exceeds 50,000 character limit");
            }
        }
        if let Some(s) = &self.status
            && !ALLOWED_STATUSES.contains(&s.as_str())
        {
            return Err(
                "status must be one of: backlog, todo, in_progress, review, done, canceled",
            );
        }
        if let Some(p) = &self.priority
            && !ALLOWED_PRIORITIES.contains(&p.as_str())
        {
            return Err("priority must be one of: none, low, medium, high, urgent");
        }
        if let Some(mins) = self.estimated_minutes
            && !(0..=100_000).contains(&mins)
        {
            return Err("estimated_minutes must be between 0 and 100000");
        }
        Ok(())
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

fn check_group_match(path_group_id: Uuid, principal_group_id: Uuid) -> Result<(), RestError> {
    if path_group_id != principal_group_id {
        Err(RestError::Forbidden)
    } else {
        Ok(())
    }
}

// ─── Handlers — slice 1 (plan 0066 / GAR-516) ────────────────────────────────

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
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
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

/// `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` — create a task.
///
/// The task list must exist and belong to `group_id`. Cross-list creation
/// is prevented at the DB level by the compound FK `(list_id, group_id)`.
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
/// | Task list not found / archived     | 404    |
/// | Happy path                         | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/task-lists/{list_id}/tasks",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("list_id" = Uuid, Path, description = "Task list UUID."),
    ),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Task created.", body = TaskResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task list not found or archived.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, list_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), RestError> {
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

    let list_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM task_lists WHERE id = $1 AND archived_at IS NULL")
            .bind(list_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if list_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let title_trimmed = body.title.trim().to_string();

    let row: TaskRow = sqlx::query_as(
        "INSERT INTO tasks \
             (list_id, group_id, title, description_md, status, priority, \
              due_at, estimated_minutes, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         RETURNING id, list_id, group_id, parent_task_id, title, description_md, \
                   status, priority, due_at, started_at, completed_at, \
                   estimated_minutes, created_by, created_by_label, \
                   created_at, updated_at, deleted_at",
    )
    .bind(list_id)
    .bind(group_id)
    .bind(&title_trimmed)
    .bind(&body.description_md)
    .bind(&body.status)
    .bind(&body.priority)
    .bind(body.due_at)
    .bind(body.estimated_minutes)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let task_id = row.id;
    let title_len = title_trimmed.chars().count();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskCreated,
        principal.user_id,
        group_id,
        "tasks",
        task_id.to_string(),
        json!({ "title_len": title_len, "status": body.status, "priority": body.priority }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(TaskResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks` — list tasks (cursor-paginated).
///
/// Returns non-deleted tasks for the specified list, newest first.
/// Optional `?status=` filter. Authz: `Action::TasksRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Invalid status filter              | 400    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/task-lists/{list_id}/tasks",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("list_id" = Uuid, Path, description = "Task list UUID."),
        ListTasksQuery,
    ),
    responses(
        (status = 200, description = "Tasks.", body = ListTasksResponse),
        (status = 400, description = "Invalid status filter.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_tasks(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, list_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<ListTasksResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    if let Some(s) = &params.status
        && !ALLOWED_STATUSES.contains(&s.as_str())
    {
        return Err(RestError::BadRequest(
            "status must be one of: backlog, todo, in_progress, review, done, canceled".into(),
        ));
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<TaskRow> = if let Some(cursor_id) = params.cursor {
        sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE list_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND ($3::text IS NULL OR status = $3) \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM tasks WHERE id = $4 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $5",
        )
        .bind(list_id)
        .bind(group_id)
        .bind(&params.status)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE list_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND ($3::text IS NULL OR status = $3) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(list_id)
        .bind(group_id)
        .bind(&params.status)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<TaskSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(|r| TaskSummary {
            id: r.id,
            list_id: r.list_id,
            group_id: r.group_id,
            title: r.title,
            status: r.status,
            priority: r.priority,
            due_at: r.due_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();
    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListTasksResponse { items, next_cursor }))
}

/// `PATCH /v1/groups/{group_id}/tasks/{task_id}` — update task fields.
///
/// All body fields are optional. Only provided (non-null) fields are updated.
/// `updated_at` is always refreshed. Returns 404 for cross-tenant tasks (RLS).
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
/// | Happy path                         | 200    |
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/tasks/{task_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = PatchTaskRequest,
    responses(
        (status = 200, description = "Task updated.", body = TaskResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchTaskRequest>,
) -> Result<Json<TaskResponse>, RestError> {
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

    let row: Option<TaskRow> = sqlx::query_as(
        "UPDATE tasks \
         SET title              = COALESCE($2, title), \
             description_md     = COALESCE($3, description_md), \
             status             = COALESCE($4, status), \
             priority           = COALESCE($5, priority), \
             due_at             = COALESCE($6, due_at), \
             estimated_minutes  = COALESCE($7, estimated_minutes), \
             updated_at         = now() \
         WHERE id = $1 \
           AND group_id = $8 \
           AND deleted_at IS NULL \
         RETURNING id, list_id, group_id, parent_task_id, title, description_md, \
                   status, priority, due_at, started_at, completed_at, \
                   estimated_minutes, created_by, created_by_label, \
                   created_at, updated_at, deleted_at",
    )
    .bind(task_id)
    .bind(&body.title)
    .bind(&body.description_md)
    .bind(&body.status)
    .bind(&body.priority)
    .bind(body.due_at)
    .bind(body.estimated_minutes)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(TaskResponse::from(row)))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}` — soft-delete a task.
///
/// Sets `deleted_at = now()`. The task is not physically removed. Returns
/// 404 for cross-tenant tasks (RLS filters them). Authz: `Action::TasksDelete`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Task not found / cross-tenant      | 404    |
/// | Task already deleted               | 404    |
/// | Happy path                         | 204    |
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 204, description = "Task deleted."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
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

    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT title, status FROM tasks \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (title, status) = match existing {
        Some(row) => row,
        None => return Err(RestError::NotFound),
    };

    let title_len = title.chars().count();

    sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskDeleted,
        principal.user_id,
        group_id,
        "tasks",
        task_id.to_string(),
        json!({ "title_len": title_len, "status": status }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Handlers — slice 2 (plan 0067 / GAR-518) ────────────────────────────────

/// `GET /v1/groups/{group_id}/tasks/{task_id}` — fetch a single task.
///
/// Returns 404 for missing, cross-tenant, or soft-deleted tasks (no 403 leak).
/// Authz: `Action::TasksRead`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Task not found / deleted / cross-tenant | 404 |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 200, description = "Task.", body = TaskResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found, deleted, or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskResponse>, RestError> {
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

    let row: Option<TaskRow> = sqlx::query_as(
        "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                status, priority, due_at, started_at, completed_at, \
                estimated_minutes, created_by, created_by_label, \
                created_at, updated_at, deleted_at \
         FROM tasks \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    match row {
        Some(r) => Ok(Json(TaskResponse::from(r))),
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
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task list not found, archived, or cross-tenant.", body = super::problem::ProblemDetails),
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task list not found or cross-tenant.", body = super::problem::ProblemDetails),
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

// ─── DTOs — slice 3 (plan 0069 / GAR-520) ────────────────────────────────────

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
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::problem::ProblemDetails),
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

    let (author_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found or cross-tenant.", body = super::problem::ProblemDetails),
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Comment not found, already deleted, or cross-tenant.", body = super::problem::ProblemDetails),
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
