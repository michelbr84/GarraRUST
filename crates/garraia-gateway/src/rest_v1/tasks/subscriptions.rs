//! Task subscription handlers.
//!
//! Extracted from `rest_v1/tasks/mod.rs` by GAR-635 (plan 0140, Q11 slice 5).
//!
//! Three endpoints (plan 0079 / GAR-539):
//! - `POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user subscribes
//! - `GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — list subscribers
//! - `DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user unsubscribes (idempotent)

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::RestV1FullState;
use super::super::problem::RestError;
use super::{check_group_match, require_group_id, set_rls_context};

/// DB row for a single task subscription, as fetched from `task_subscriptions`.
#[derive(sqlx::FromRow)]
struct SubscriptionRow {
    user_id: Uuid,
    subscribed_at: DateTime<Utc>,
    muted: bool,
}

/// Public response shape for a task subscription.
#[derive(Debug, Serialize, ToSchema)]
pub struct SubscriptionResponse {
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub subscribed_at: DateTime<Utc>,
    pub muted: bool,
}

/// `POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — subscribe the
/// current user to a task.
///
/// Returns 201 on success, 409 if already subscribed, 404 if task is not
/// present in this group. Body is empty. Authz: `Action::TasksWrite`.
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/tasks/{task_id}/subscriptions",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 201, description = "Subscribed.", body = SubscriptionResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
        (status = 409, description = "Already subscribed to this task.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn subscribe_to_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<SubscriptionResponse>), RestError> {
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

    let row: SubscriptionRow = sqlx::query_as(
        "INSERT INTO task_subscriptions (task_id, user_id) \
         VALUES ($1, $2) \
         RETURNING user_id, subscribed_at, muted",
    )
    .bind(task_id)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.code().as_deref() == Some("23505")
        {
            return RestError::Conflict("already subscribed to this task".into());
        }
        RestError::Internal(e.into())
    })?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskSubscribed,
        principal.user_id,
        group_id,
        "task_subscriptions",
        task_id.to_string(),
        json!({ "subscriber_user_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(SubscriptionResponse {
            task_id,
            user_id: row.user_id,
            subscribed_at: row.subscribed_at,
            muted: row.muted,
        }),
    ))
}

/// `GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — list subscribers
/// for a task.
///
/// Returns subscribers ordered by `subscribed_at ASC`, then `user_id ASC` as
/// a stable tiebreaker for subsecond inserts. Authz: `Action::TasksRead`.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/tasks/{task_id}/subscriptions",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 200, description = "List of subscribers.", body = Vec<SubscriptionResponse>),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_task_subscriptions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<SubscriptionResponse>>, RestError> {
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

    // Explicit 404 if task not found (RLS would also filter, but the explicit
    // check yields clear UX vs. silent empty list when the task is wrong).
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

    let rows: Vec<SubscriptionRow> = sqlx::query_as(
        "SELECT user_id, subscribed_at, muted \
         FROM task_subscriptions \
         WHERE task_id = $1 \
         ORDER BY subscribed_at ASC, user_id ASC",
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
        .map(|r| SubscriptionResponse {
            task_id,
            user_id: r.user_id,
            subscribed_at: r.subscribed_at,
            muted: r.muted,
        })
        .collect();

    Ok(Json(items))
}

/// `DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — unsubscribe
/// the current user from a task.
///
/// Idempotent on the subscription row: returns 204 whether or not the user
/// was previously subscribed. Returns 404 if the task itself is not present
/// in this group. Authz: `Action::TasksWrite`.
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/tasks/{task_id}/subscriptions",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("task_id" = Uuid, Path, description = "Task UUID."),
    ),
    responses(
        (status = 204, description = "Unsubscribed (or was not subscribed)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::super::problem::ProblemDetails),
        (status = 404, description = "Task not found in this group.", body = super::super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn unsubscribe_from_task(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, task_id)): Path<(Uuid, Uuid)>,
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

    sqlx::query("DELETE FROM task_subscriptions WHERE task_id = $1 AND user_id = $2")
        .bind(task_id)
        .bind(principal.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::TaskUnsubscribed,
        principal.user_id,
        group_id,
        "task_subscriptions",
        task_id.to_string(),
        json!({ "subscriber_user_id_len": 36 }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}
