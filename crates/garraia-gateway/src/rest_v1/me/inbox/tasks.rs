//! `GET /v1/me/tasks` — inbox of tasks assigned to the caller in a group.

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

// ─── GET /v1/me/tasks ────────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/tasks`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListTasksQuery {
    /// Group to scope the task inbox. Required — the caller must be a member.
    pub group_id: Uuid,
    /// Keyset cursor — `task_id` of the last task in the previous page.
    /// Returns tasks older than this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
    /// Optional status filter. Accepted values: `backlog`, `todo`, `in_progress`,
    /// `review`, `done`, `canceled`. Unknown values are rejected with 400.
    pub status: Option<String>,
}

impl ListTasksQuery {
    fn validate_status(status: &str) -> bool {
        matches!(
            status,
            "backlog" | "todo" | "in_progress" | "review" | "done" | "canceled"
        )
    }
}

/// A single task assigned to the caller.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskAssignmentSummary {
    /// UUID of the task.
    pub task_id: Uuid,
    /// UUID of the task list containing this task.
    pub list_id: Uuid,
    /// UUID of the group (denormalized for convenience).
    pub group_id: Uuid,
    /// Task title.
    pub title: String,
    /// Task status (`backlog`, `todo`, `in_progress`, `review`, `done`, `canceled`).
    pub status: String,
    /// Task priority (`none`, `low`, `medium`, `high`, `urgent`).
    pub priority: String,
    /// Optional due date (UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<DateTime<Utc>>,
    /// UTC timestamp when the task was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/tasks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TasksListResponse {
    pub items: Vec<TaskAssignmentSummary>,
    /// `task_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/tasks` — inbox of tasks assigned to the authenticated caller.
///
/// Returns up to `limit` (default 50, max 100) tasks in `group_id` where
/// the caller appears in `task_assignees`, ordered by
/// `(tasks.created_at DESC, tasks.id DESC)`.
///
/// Cursor-based pagination via `?after=<last_task_id>`. Optional
/// `?status=<value>` filter narrows to a single status value.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/tasks",
    params(ListTasksQuery),
    responses(
        (status = 200, description = "List of assigned tasks, newest first.", body = TasksListResponse),
        (status = 400, description = "Validation error (invalid status value).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_tasks(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListTasksQuery>,
) -> Result<Json<TasksListResponse>, RestError> {
    if let Some(ref s) = params.status
        && !ListTasksQuery::validate_status(s)
    {
        return Err(RestError::BadRequest(
            "status must be one of: backlog, todo, in_progress, review, done, canceled".into(),
        ));
    }

    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);
    let group_id = params.group_id;

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
        .bind(group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    type TaskRow = (
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        String,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );

    let rows: Vec<TaskRow> = match (params.after, params.status.as_deref()) {
        (None, None) => sqlx::query_as(
            "SELECT t.id, t.list_id, t.group_id, t.title, t.status, t.priority, \
                        t.due_at, t.created_at \
                 FROM task_assignees ta \
                 JOIN tasks t ON ta.task_id = t.id \
                 WHERE ta.user_id = $1 \
                   AND t.group_id = $2 \
                   AND t.deleted_at IS NULL \
                 ORDER BY t.created_at DESC, t.id DESC \
                 LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        (None, Some(status)) => sqlx::query_as(
            "SELECT t.id, t.list_id, t.group_id, t.title, t.status, t.priority, \
                        t.due_at, t.created_at \
                 FROM task_assignees ta \
                 JOIN tasks t ON ta.task_id = t.id \
                 WHERE ta.user_id = $1 \
                   AND t.group_id = $2 \
                   AND t.status = $3 \
                   AND t.deleted_at IS NULL \
                 ORDER BY t.created_at DESC, t.id DESC \
                 LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(status)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        (Some(after_id), None) => {
            // Cursor subquery: if after_id is deleted or from a different group,
            // the subquery returns NULL → comparison is always false → empty safe result.
            sqlx::query_as(
                "SELECT t.id, t.list_id, t.group_id, t.title, t.status, t.priority, \
                        t.due_at, t.created_at \
                 FROM task_assignees ta \
                 JOIN tasks t ON ta.task_id = t.id \
                 WHERE ta.user_id = $1 \
                   AND t.group_id = $2 \
                   AND t.deleted_at IS NULL \
                   AND (t.created_at, t.id) < ( \
                       SELECT created_at, id FROM tasks \
                       WHERE id = $3 AND group_id = $2 AND deleted_at IS NULL \
                   ) \
                 ORDER BY t.created_at DESC, t.id DESC \
                 LIMIT $4",
            )
            .bind(principal.user_id)
            .bind(group_id)
            .bind(after_id)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?
        }
        (Some(after_id), Some(status)) => sqlx::query_as(
            "SELECT t.id, t.list_id, t.group_id, t.title, t.status, t.priority, \
                        t.due_at, t.created_at \
                 FROM task_assignees ta \
                 JOIN tasks t ON ta.task_id = t.id \
                 WHERE ta.user_id = $1 \
                   AND t.group_id = $2 \
                   AND t.status = $3 \
                   AND t.deleted_at IS NULL \
                   AND (t.created_at, t.id) < ( \
                       SELECT created_at, id FROM tasks \
                       WHERE id = $4 AND group_id = $2 AND deleted_at IS NULL \
                   ) \
                 ORDER BY t.created_at DESC, t.id DESC \
                 LIMIT $5",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(status)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(task_id, ..)| *task_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(task_id, list_id, group_id, title, status, priority, due_at, created_at)| {
                TaskAssignmentSummary {
                    task_id,
                    list_id,
                    group_id,
                    title,
                    status,
                    priority,
                    due_at,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(TasksListResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TasksListResponse / TaskAssignmentSummary serialization ───────────────

    #[test]
    fn tasks_list_response_empty_no_cursor() {
        let resp = TasksListResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(
            v.get("next_cursor").is_none(),
            "absent cursor must be omitted"
        );
    }

    #[test]
    fn tasks_list_response_with_cursor() {
        let cursor = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let resp = TasksListResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "bbbbbbbb-0000-0000-0000-000000000002");
    }

    #[test]
    fn task_assignment_summary_serializes_all_fields() {
        let task_id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let list_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("cccccccc-0000-0000-0000-000000000001").unwrap();
        let summary = TaskAssignmentSummary {
            task_id,
            list_id,
            group_id,
            title: "Fix the bug".into(),
            status: "in_progress".into(),
            priority: "high".into(),
            due_at: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["task_id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["list_id"], "bbbbbbbb-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "cccccccc-0000-0000-0000-000000000001");
        assert_eq!(v["title"], "Fix the bug");
        assert_eq!(v["status"], "in_progress");
        assert_eq!(v["priority"], "high");
        assert!(v.get("due_at").is_none(), "absent due_at must be omitted");
    }

    #[test]
    fn task_assignment_summary_includes_due_at_when_present() {
        let summary = TaskAssignmentSummary {
            task_id: Uuid::nil(),
            list_id: Uuid::nil(),
            group_id: Uuid::nil(),
            title: "Deploy".into(),
            status: "todo".into(),
            priority: "urgent".into(),
            due_at: Some(chrono::DateTime::from_timestamp(1_000_000, 0).unwrap()),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("due_at").is_some(),
            "present due_at must be serialized"
        );
    }

    #[test]
    fn list_tasks_query_status_valid_values() {
        for s in &[
            "backlog",
            "todo",
            "in_progress",
            "review",
            "done",
            "canceled",
        ] {
            assert!(
                ListTasksQuery::validate_status(s),
                "expected '{s}' to be valid"
            );
        }
    }

    #[test]
    fn list_tasks_query_status_invalid_value() {
        assert!(
            !ListTasksQuery::validate_status("unknown"),
            "expected 'unknown' to be invalid"
        );
        assert!(
            !ListTasksQuery::validate_status(""),
            "expected empty string to be invalid"
        );
    }
}
