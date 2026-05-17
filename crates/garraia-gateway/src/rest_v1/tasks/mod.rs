//! `/v1/groups/{group_id}/task-lists` and task/comment/assignee/label/subscription/attachment handlers
//! (plan 0066/0067/0069/0077/0078/0079/0083/0096, GAR-516/GAR-518/GAR-520/GAR-533/GAR-536/GAR-539/GAR-546/GAR-572).
//!
//! Twenty-seven endpoints on the `garraia_app` RLS-enforced pool:
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
//! **Slice 4 (plan 0077 / GAR-533):**
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/assignees` — assign group member
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/assignees` — list assignees
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}` — remove assignee (idempotent)
//!
//! **Slice 5 (plan 0078 / GAR-536):**
//! - `POST /v1/groups/{group_id}/task-labels` — create task label
//! - `GET /v1/groups/{group_id}/task-labels` — list task labels
//! - `DELETE /v1/groups/{group_id}/task-labels/{label_id}` — delete label (CASCADE assignments)
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/labels` — assign label to task
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}` — remove label assignment (idempotent)
//!
//! **Slice 6 (plan 0079 / GAR-539):**
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user subscribes
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — list subscribers
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user unsubscribes (idempotent)
//!
//! **Slice 9 (plan 0083 / GAR-546):**
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks` — cursor-paginated direct children
//!   (`parent_task_id = task_id AND deleted_at IS NULL`); also enables `parent_task_id`
//!   in `CreateTaskRequest` so tasks can be created as children of an existing task.
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

pub mod task_lists;
pub use task_lists::{
    // utoipa-generated path types (needed by openapi.rs paths! macro)
    __path_create_task_list,
    __path_delete_task_list,
    __path_get_task_list,
    __path_list_task_lists,
    __path_patch_task_list,
    CreateTaskListRequest,
    ListTaskListsQuery,
    ListTaskListsResponse,
    PatchTaskListRequest,
    TaskListResponse,
    TaskListSummary,
    create_task_list,
    delete_task_list,
    get_task_list,
    list_task_lists,
    patch_task_list,
};

pub mod comments;
pub use comments::{
    __path_create_task_comment, __path_delete_task_comment, __path_list_task_comments,
    CommentResponse, CreateCommentRequest, ListCommentsQuery, ListCommentsResponse,
    create_task_comment, delete_task_comment, list_task_comments,
};

pub mod assignees;
pub use assignees::{
    __path_add_task_assignee, __path_list_task_assignees, __path_remove_task_assignee,
    AddAssigneeRequest, AssigneeResponse, add_task_assignee, list_task_assignees,
    remove_task_assignee,
};

pub mod labels;
pub use labels::{
    __path_assign_task_label, __path_create_task_label, __path_delete_task_label,
    __path_list_task_labels, __path_remove_task_label_from_task, AssignTaskLabelRequest,
    CreateTaskLabelRequest, LabelAssignmentResponse, TaskLabelResponse, assign_task_label,
    create_task_label, delete_task_label, list_task_labels, remove_task_label_from_task,
};

pub mod subscriptions;
pub use subscriptions::{
    __path_list_task_subscriptions, __path_subscribe_to_task, __path_unsubscribe_from_task,
    SubscriptionResponse, list_task_subscriptions, subscribe_to_task, unsubscribe_from_task,
};

pub mod activity;
pub use activity::{
    __path_list_task_activity, ActivityResponse, ListActivityQuery, ListActivityResponse,
    list_task_activity,
};

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

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
    /// Optional parent task UUID. When provided the new task becomes a direct child
    /// of the parent (depth limit: max 1 level of nesting; the parent must itself
    /// have no parent). The parent must be in the same group and not soft-deleted.
    pub parent_task_id: Option<Uuid>,
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

async fn insert_task_activity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: Uuid,
    group_id: Uuid,
    actor_user_id: Uuid,
    actor_label: &str,
    kind: &str,
    payload: serde_json::Value,
) -> Result<(), RestError> {
    sqlx::query(
        "INSERT INTO task_activity \
             (task_id, group_id, actor_user_id, actor_label, kind, payload) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(task_id)
    .bind(group_id)
    .bind(actor_user_id)
    .bind(actor_label)
    .bind(kind)
    .bind(payload)
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

    // Validate parent_task_id if provided.
    if let Some(parent_id) = body.parent_task_id {
        // Parent must exist in this group and not be soft-deleted.
        let parent_row: Option<(Option<Uuid>,)> = sqlx::query_as(
            "SELECT parent_task_id FROM tasks \
             WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
        )
        .bind(parent_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        match parent_row {
            None => return Err(RestError::NotFound),
            Some((Some(_),)) => {
                return Err(RestError::BadRequest(
                    "max nesting depth exceeded: parent task already has a parent".into(),
                ));
            }
            Some((None,)) => {} // parent is a root task — allowed
        }
    }

    let row: TaskRow = sqlx::query_as(
        "INSERT INTO tasks \
             (list_id, group_id, parent_task_id, title, description_md, status, priority, \
              due_at, estimated_minutes, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING id, list_id, group_id, parent_task_id, title, description_md, \
                   status, priority, due_at, started_at, completed_at, \
                   estimated_minutes, created_by, created_by_label, \
                   created_at, updated_at, deleted_at",
    )
    .bind(list_id)
    .bind(group_id)
    .bind(body.parent_task_id)
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

    let activity_payload = match body.parent_task_id {
        Some(pid) => json!({ "parent_task_id": pid.to_string() }),
        None => json!({}),
    };
    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &created_by_label,
        "created",
        activity_payload,
    )
    .await?;

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

    // Fetch old values before updating so we can emit precise activity events.
    let needs_activity = body.status.is_some() || body.priority.is_some() || body.due_at.is_some();
    let old: Option<(String, String, Option<DateTime<Utc>>)> = if needs_activity {
        sqlx::query_as(
            "SELECT status, priority, due_at FROM tasks \
             WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
        )
        .bind(task_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        None
    };
    if needs_activity && old.is_none() {
        return Err(RestError::NotFound);
    }

    let actor_label: String = if needs_activity {
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map(|(l,): (String,)| l)
            .map_err(|e| RestError::Internal(e.into()))?
    } else {
        String::new()
    };

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

    if let Some((old_status, old_priority, old_due_at)) = old {
        if let Some(new_status) = &body.status {
            insert_task_activity(
                &mut tx,
                task_id,
                group_id,
                principal.user_id,
                &actor_label,
                "status_changed",
                json!({ "old": old_status, "new": new_status }),
            )
            .await?;
        }
        if let Some(new_priority) = &body.priority {
            insert_task_activity(
                &mut tx,
                task_id,
                group_id,
                principal.user_id,
                &actor_label,
                "priority_changed",
                json!({ "old": old_priority, "new": new_priority }),
            )
            .await?;
        }
        if body.due_at.is_some() {
            let set = body.due_at.is_some();
            insert_task_activity(
                &mut tx,
                task_id,
                group_id,
                principal.user_id,
                &actor_label,
                "due_changed",
                json!({ "set": set, "had_due": old_due_at.is_some() }),
            )
            .await?;
        }
    }

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

    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

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

    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "deleted",
        json!({}),
    )
    .await?;

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
// ─── comments module (plan 0136 / GAR-635 slice 2) ───────────────────────────
// See comments.rs for CommentResponse, CreateCommentRequest, ListCommentsQuery,
// ListCommentsResponse, create_task_comment, list_task_comments, delete_task_comment.

// ─── assignees module (plan 0137 / GAR-635 slice 3) ──────────────────────────
// See assignees.rs for AssigneeResponse, AddAssigneeRequest,
// add_task_assignee, list_task_assignees, remove_task_assignee.

// ─── subscriptions module (plan 0140 / GAR-653 slice 5) ─────────────────────
// See subscriptions.rs for SubscriptionResponse, subscribe_to_task,
// list_task_subscriptions, unsubscribe_from_task.

// ─── activity module (plan 0141 / GAR-635 slice 6) ───────────────────────────
// See activity.rs for ActivityResponse, ListActivityQuery, ListActivityResponse,
// list_task_activity.

// ─── Slice 9 — `GET tasks/{id}/subtasks` (plan 0083 / GAR-546) ───────────────

/// Query parameters for `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListSubtasksQuery {
    /// Optional status filter.
    pub status: Option<String>,
    /// Keyset cursor — UUID of the last item received.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Response body for `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListSubtasksResponse {
    pub items: Vec<TaskSummary>,
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks` — cursor-paginated direct children.
///
/// Returns tasks whose `parent_task_id = task_id` and `deleted_at IS NULL`.
/// Only direct children are listed (depth = 1). Cross-group parent task → 404.
/// Authz: `Action::TasksRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Invalid status filter              | 400    |
/// | Parent task not found              | 404    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/subtasks",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Parent task UUID."),
        ListSubtasksQuery,
    ),
    responses(
        (status = 200, description = "Direct child tasks.", body = ListSubtasksResponse),
        (status = 400, description = "Invalid status filter.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Parent task not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_subtasks(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<ListSubtasksQuery>,
) -> Result<Json<ListSubtasksResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::TasksRead) {
        return Err(RestError::Forbidden);
    }

    if params
        .status
        .as_deref()
        .is_some_and(|s| !ALLOWED_STATUSES.contains(&s))
    {
        return Err(RestError::BadRequest(
            "status must be one of: backlog, todo, in_progress, review, done, canceled".into(),
        ));
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = i64::from(effective_limit + 1);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify parent task exists in this group and is not soft-deleted.
    let parent_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM tasks WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if parent_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let rows: Vec<TaskRow> = match (params.cursor, &params.status) {
        (Some(cursor_id), Some(status)) => sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE parent_task_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND status = $3 \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM tasks WHERE id = $4 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $5",
        )
        .bind(task_id)
        .bind(group_id)
        .bind(status)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        (Some(cursor_id), None) => sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE parent_task_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM tasks WHERE id = $3 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(task_id)
        .bind(group_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        (None, Some(status)) => sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE parent_task_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND status = $3 \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(task_id)
        .bind(group_id)
        .bind(status)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        (None, None) => sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE parent_task_id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(task_id)
        .bind(group_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
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

    Ok(Json(ListSubtasksResponse { items, next_cursor }))
}

// ─── Slice 8 — `POST tasks/{id}/move` (plan 0082 / GAR-544) ──────────────────

/// Request body for `POST /v1/groups/{group_id}/tasks/{task_id}/move`.
///
/// Move semantics:
/// - `target_list_id` MUST be a list in the same group, not archived.
/// - If `target_list_id == current list_id`, the call is a no-op (200, no
///   activity row, no audit row, `updated_at` unchanged).
/// - Cross-group target → 404 (RLS filters lookup to 0 rows).
/// - Schema-level compound FK `(list_id, group_id) → task_lists(id, group_id)`
///   is the second line of defense if the §pre-validation SELECT is bypassed.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveTaskRequest {
    /// Destination task list (must live in the caller's group).
    pub target_list_id: Uuid,
}

/// `POST /v1/groups/{group_id}/tasks/{task_id}/move` — move task between lists.
///
/// Updates `tasks.list_id` to `target_list_id`. The new list MUST belong to
/// the caller's group and MUST NOT be archived. On a real transition (target
/// differs from current list) one `task_activity` row (kind=`'moved'`) and
/// one `audit_events` row (`task.moved`) are written atomically inside the
/// same transaction as the UPDATE; an idempotent self-move skips both writes.
///
/// Authz: `Action::TasksWrite`. Path `group_id` must equal `principal.group_id`.
///
/// Note on the path scheme: the GAR-544 spec used Google API verb-suffix
/// `:move`, but Axum 0.8 / matchit 0.8 cannot route a literal segment-internal
/// suffix after a named param. Plan 0082 §0 amends to `/move` sub-segment,
/// matching the existing convention for sibling task routes.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Insufficient permission            | 403    |
/// | Task not found / cross-tenant      | 404    |
/// | Task soft-deleted                  | 404    |
/// | Target list not found / cross-tenant | 404  |
/// | Target list archived               | 404    |
/// | Self-move (target == current)      | 200 (no-op) |
/// | Happy path                         | 200    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/move",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    request_body = MoveTaskRequest,
    responses(
        (status = 200, description = "Task moved (or self-move no-op).", body = TaskResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member, group mismatch, or insufficient permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task or target list not found / cross-tenant / archived / soft-deleted.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn move_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MoveTaskRequest>,
) -> Result<Json<TaskResponse>, RestError> {
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

    // Fetch current list_id; 404 if task missing or soft-deleted.
    let current: Option<(Uuid,)> = sqlx::query_as(
        "SELECT list_id FROM tasks \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let from_list_id = match current {
        Some((id,)) => id,
        None => return Err(RestError::NotFound),
    };

    // Idempotent self-move: same list — return current task body unchanged.
    if from_list_id == body.target_list_id {
        let row: TaskRow = sqlx::query_as(
            "SELECT id, list_id, group_id, parent_task_id, title, description_md, \
                    status, priority, due_at, started_at, completed_at, \
                    estimated_minutes, created_by, created_by_label, \
                    created_at, updated_at, deleted_at \
             FROM tasks \
             WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
        )
        .bind(task_id)
        .bind(group_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        return Ok(Json(TaskResponse::from(row)));
    }

    // Validate target list — must live in the same group and not be archived.
    // Cross-group targets are filtered to 0 rows by RLS, so the cross-tenant
    // case collapses into a clean 404.
    let target_ok: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM task_lists \
         WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(body.target_list_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if target_ok.is_none() {
        return Err(RestError::NotFound);
    }

    // Cache actor display name for audit + activity payloads.
    let (actor_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Apply the move. Compound FK `(list_id, group_id) → task_lists(id, group_id)`
    // re-validates same-group at the DB layer — even a buggy app layer cannot
    // point list_id at a foreign-group list (would fail with SQLSTATE 23503).
    let row: Option<TaskRow> = sqlx::query_as(
        "UPDATE tasks \
         SET list_id    = $1, \
             updated_at = now() \
         WHERE id = $2 \
           AND group_id = $3 \
           AND deleted_at IS NULL \
         RETURNING id, list_id, group_id, parent_task_id, title, description_md, \
                   status, priority, due_at, started_at, completed_at, \
                   estimated_minutes, created_by, created_by_label, \
                   created_at, updated_at, deleted_at",
    )
    .bind(body.target_list_id)
    .bind(task_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    // Activity timeline entry — kind `'moved'` introduced in migration 016.
    insert_task_activity(
        &mut tx,
        task_id,
        group_id,
        principal.user_id,
        &actor_label,
        "moved",
        json!({
            "from_list_id": from_list_id.to_string(),
            "to_list_id": body.target_list_id.to_string(),
        }),
    )
    .await?;

    // Compliance audit row. Metadata carries UUIDs only — never list names.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskMoved,
        principal.user_id,
        group_id,
        "tasks",
        task_id.to_string(),
        json!({
            "from_list_id": from_list_id.to_string(),
            "to_list_id": body.target_list_id.to_string(),
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(TaskResponse::from(row)))
}

// ─── Task attachments (plan 0096 / GAR-572, slice 9) ─────────────────────────

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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task or file not found in this group.", body = super::problem::ProblemDetails),
        (status = 409, description = "File already attached to this task.", body = super::problem::ProblemDetails),
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::problem::ProblemDetails),
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
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::problem::ProblemDetails),
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
