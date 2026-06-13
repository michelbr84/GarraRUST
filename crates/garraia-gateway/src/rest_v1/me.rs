//! `/v1/me` — authenticated caller identity + self-service profile update.
//!
//! `GET /v1/me` returns identity info from the `Principal` extractor only —
//! no SQL. `PATCH /v1/me` updates `display_name` on the `users` table via
//! the `garraia_app` AppPool. `users` is NOT FORCE-RLS group-scoped, so no
//! `SET LOCAL` context is needed; isolation is via `WHERE id = $1` with the
//! JWT-authenticated `principal.user_id`.
//!
//! `GET /v1/me/mentions` — cursor-paginated inbox of @mentions received by
//! the caller in a given group (plan 0237 / GAR-755).
//!
//! `GET /v1/me/tasks` — cursor-paginated inbox of tasks assigned to the caller
//! in a given group (plan 0242 / GAR-763).
//!
//! `GET /v1/me/chats` — cursor-paginated inbox of chats where the caller holds
//! a `chat_members` row in a given group (plan 0245 / GAR-765).
//!
//! `GET /v1/me/files` — cursor-paginated inbox of files uploaded by the caller
//! in a given group (plan 0246 / GAR-767).
//!
//! `GET /v1/me/invites` — cursor-paginated inbox of pending group invites
//! addressed to the caller's email address (plan 0255 / GAR-777).
//!
//! `POST /v1/me/invites/{invite_id}/decline` — invitee-side explicit decline of a
//! pending group invite (plan 0258 / GAR-783).
//!
//! `GET /v1/me/reactions` — cursor-paginated inbox of messages on which the caller
//! placed emoji reactions, grouped by message (plan 0260 / GAR-788).
//!
//! `GET /v1/me/threads` — cursor-paginated inbox of threads the caller created or
//! has replied to in a given group (plan 0261 / GAR-790).
//!
//! `GET /v1/me/doc-page-mentions` — cursor-paginated inbox of doc page @mentions
//! received by the caller in a given group (plan 0318 / GAR-858).
//!
//! `GET /v1/me/doc-pages` — cursor-paginated inbox of doc pages authored by
//! the caller in a given group (plan 0322 / GAR-860).
//!
//! `GET /v1/me/sessions` — cursor-paginated list of the caller's active sessions
//! (plan 0326 / GAR-866). No `X-Group-Id` required — sessions are user-scoped.
//!
//! `DELETE /v1/me/sessions/{session_id}` — revoke a specific session
//! (plan 0326 / GAR-866). Idempotent: already-revoked returns 204..

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── GET /v1/me ──────────────────────────────────────────────────────────────

/// Response body for `GET /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    /// UUID of the authenticated user (from the JWT `sub` claim).
    pub user_id: Uuid,
    /// Active group UUID if the caller supplied `X-Group-Id` and is an
    /// active member; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    /// Group role string (e.g. `"owner"`, `"admin"`, `"member"`).
    /// Absent when `group_id` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/me",
    responses(
        (status = 200, description = "Authenticated identity", body = MeResponse),
        (status = 401, description = "Missing or invalid JWT", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of X-Group-Id", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_me(principal: Principal) -> Result<Json<MeResponse>, RestError> {
    Ok(Json(MeResponse {
        user_id: principal.user_id,
        group_id: principal.group_id,
        role: principal.role.map(|r| r.as_str().to_string()),
    }))
}

// ─── PATCH /v1/me ────────────────────────────────────────────────────────────

/// Request body for `PATCH /v1/me`.
///
/// All fields are optional. An empty body `{}` is a valid no-op that returns
/// the current profile. Unknown fields are rejected with 422 (deny_unknown_fields).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchMeRequest {
    /// Updated display name. 1–128 characters when provided.
    pub display_name: Option<String>,
}

impl PatchMeRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if let Some(dn) = &self.display_name {
            let len = dn.chars().count();
            if len == 0 {
                return Err("display_name must not be empty");
            }
            if len > 128 {
                return Err("display_name exceeds 128 character limit");
            }
        }
        Ok(())
    }
}

/// Response body for `PATCH /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PatchMeResponse {
    /// UUID of the authenticated user.
    pub user_id: Uuid,
    /// Current email address (read-only, returned for client sync).
    pub email: String,
    /// Current display name after the update.
    pub display_name: String,
    /// Account creation timestamp (UTC).
    pub created_at: DateTime<Utc>,
    /// Last update timestamp (UTC).
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[utoipa::path(
    patch,
    path = "/v1/me",
    request_body = PatchMeRequest,
    responses(
        (status = 200, description = "Profile updated.", body = PatchMeResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 422, description = "Unknown field or malformed body.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_me(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(body): Json<PatchMeRequest>,
) -> Result<Json<PatchMeResponse>, RestError> {
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();

    let row: UserRow = if body.display_name.is_none() {
        // No-op path: return current user data without issuing an UPDATE.
        sqlx::query_as(
            "SELECT id, email, display_name, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(principal.user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "UPDATE users \
             SET display_name = COALESCE($2, display_name), updated_at = now() \
             WHERE id = $1 \
             RETURNING id, email, display_name, created_at, updated_at",
        )
        .bind(principal.user_id)
        .bind(body.display_name.as_deref())
        .fetch_one(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    Ok(Json(PatchMeResponse {
        user_id: row.id,
        email: row.email,
        display_name: row.display_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

// ─── GET /v1/me/mentions ─────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/mentions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMentionsQuery {
    /// Group to scope the mention inbox. Required — the caller must be a member.
    pub group_id: Uuid,
    /// Keyset cursor — `message_id` of the last mention received. Returns
    /// mentions older than this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
}

/// A single @mention received by the caller.
#[derive(Debug, Serialize, ToSchema)]
pub struct MentionSummary {
    /// UUID of the message in which the caller was mentioned.
    pub message_id: Uuid,
    /// UUID of the chat that contains the message.
    pub chat_id: Uuid,
    /// UUID of the group (denormalized for convenience).
    pub group_id: Uuid,
    /// UUID of the user who sent the message.
    pub sender_user_id: Uuid,
    /// Display name of the sender at send time.
    pub sender_label: String,
    /// First 200 characters of the message body.
    pub body_excerpt: String,
    /// UTC timestamp when the mention row was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/mentions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MentionsListResponse {
    pub items: Vec<MentionSummary>,
    /// `message_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/mentions` — inbox of @mentions received by the authenticated caller.
///
/// Returns up to `limit` (default 50, max 100) mentions in `group_id`,
/// ordered by `(mm.created_at DESC, mm.message_id DESC)`.
/// Cursor-based pagination via `?after=<last_message_id>`.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/mentions",
    params(ListMentionsQuery),
    responses(
        (status = 200, description = "List of @mentions, newest first.", body = MentionsListResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_mentions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMentionsQuery>,
) -> Result<Json<MentionsListResponse>, RestError> {
    // Validate group membership — the Principal extractor enforces this only
    // when X-Group-Id is present; here we require group_id as a query param.
    // We rely on FORCE RLS + SET LOCAL to enforce isolation — no explicit
    // membership check is needed because RLS will return 0 rows for any
    // group the caller does not belong to (correct behavior: empty inbox).

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

    type MentionRow = (Uuid, Uuid, Uuid, Uuid, String, String, DateTime<Utc>);

    let rows: Vec<MentionRow> = if let Some(after_id) = params.after {
        // Cursor subquery: if the after message_id is not found (deleted or
        // wrong group), the subquery returns NULL → comparison is always false
        // → empty safe result (same pattern as list_messages).
        sqlx::query_as(
            "SELECT mm.message_id, m.chat_id, mm.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    mm.created_at \
             FROM message_mentions mm \
             JOIN messages m ON mm.message_id = m.id \
             WHERE mm.mentioned_user_id = $1 \
               AND mm.group_id = $2 \
               AND (mm.created_at, mm.message_id) < ( \
                   SELECT created_at, message_id \
                   FROM message_mentions \
                   WHERE message_id = $3 AND mentioned_user_id = $1 \
               ) \
             ORDER BY mm.created_at DESC, mm.message_id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT mm.message_id, m.chat_id, mm.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    mm.created_at \
             FROM message_mentions mm \
             JOIN messages m ON mm.message_id = m.id \
             WHERE mm.mentioned_user_id = $1 \
               AND mm.group_id = $2 \
             ORDER BY mm.created_at DESC, mm.message_id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(message_id, ..)| *message_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(
                message_id,
                chat_id,
                group_id,
                sender_user_id,
                sender_label,
                body_excerpt,
                created_at,
            )| {
                MentionSummary {
                    message_id,
                    chat_id,
                    group_id,
                    sender_user_id,
                    sender_label,
                    body_excerpt,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MentionsListResponse { items, next_cursor }))
}

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
        (status = 400, description = "Validation error (invalid status value).", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = super::problem::ProblemDetails),
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

// ─── GET /v1/me/chats ────────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/chats`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyChatsQuery {
    /// Group to scope the chat inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Keyset cursor — `chat_id` of the last item in the previous page.
    /// Returns chats joined earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
    /// Optional chat type filter. Accepted: `channel`, `dm`, `thread`.
    /// Unknown values are rejected with 400.
    #[serde(rename = "type")]
    pub chat_type: Option<String>,
}

impl ListMyChatsQuery {
    fn validate_type(t: &str) -> bool {
        matches!(t, "channel" | "dm" | "thread")
    }
}

/// A single chat membership entry for the caller.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMembershipSummary {
    /// UUID of the chat.
    pub chat_id: Uuid,
    /// UUID of the group the chat belongs to.
    pub group_id: Uuid,
    /// Display name of the chat.
    pub name: String,
    /// Chat type: `channel`, `dm`, or `thread`.
    #[serde(rename = "type")]
    pub chat_type: String,
    /// Caller's role in this chat (`owner`, `moderator`, `member`, `viewer`).
    pub role: String,
    /// UTC timestamp when the caller joined the chat.
    pub joined_at: DateTime<Utc>,
    /// Whether the caller has muted this chat.
    pub muted: bool,
    /// UTC timestamp of the caller's last read message in this chat. `None` if never read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<DateTime<Utc>>,
}

/// Response body for `GET /v1/me/chats`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyChatsMembershipResponse {
    pub items: Vec<ChatMembershipSummary>,
    /// `chat_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older-joined) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/chats` — inbox of chats where the authenticated caller is a member.
///
/// Returns up to `limit` (default 50, max 100) non-archived chats in `group_id`
/// where the caller appears in `chat_members`, ordered by
/// `(cm.joined_at DESC, cm.chat_id DESC)`.
///
/// Cursor-based pagination via `?after=<last_chat_id>`. Optional
/// `?type=<channel|dm|thread>` filter narrows to a single chat type.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/chats",
    params(ListMyChatsQuery),
    responses(
        (status = 200, description = "List of chat memberships, newest-joined first.", body = MyChatsMembershipResponse),
        (status = 400, description = "Validation error (invalid type value).", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_chats(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyChatsQuery>,
) -> Result<Json<MyChatsMembershipResponse>, RestError> {
    if let Some(ref t) = params.chat_type
        && !ListMyChatsQuery::validate_type(t)
    {
        return Err(RestError::BadRequest(
            "type must be one of: channel, dm, thread".into(),
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

    // Columns: chat_id, group_id, name, type, role, joined_at, muted, last_read_at
    type ChatRow = (
        Uuid,
        Uuid,
        String,
        String,
        String,
        DateTime<Utc>,
        bool,
        Option<DateTime<Utc>>,
    );

    let rows: Vec<ChatRow> = match (params.after, params.chat_type.as_deref()) {
        (None, None) => sqlx::query_as(
            "SELECT c.id, c.group_id, c.name, c.type, cm.role, \
                        cm.joined_at, cm.muted, cm.last_read_at \
                 FROM chat_members cm \
                 JOIN chats c ON cm.chat_id = c.id \
                 WHERE cm.user_id = $1 \
                   AND c.group_id = $2 \
                   AND c.archived_at IS NULL \
                 ORDER BY cm.joined_at DESC, cm.chat_id DESC \
                 LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(chat_type)) => sqlx::query_as(
            "SELECT c.id, c.group_id, c.name, c.type, cm.role, \
                        cm.joined_at, cm.muted, cm.last_read_at \
                 FROM chat_members cm \
                 JOIN chats c ON cm.chat_id = c.id \
                 WHERE cm.user_id = $1 \
                   AND c.group_id = $2 \
                   AND c.type = $3 \
                   AND c.archived_at IS NULL \
                 ORDER BY cm.joined_at DESC, cm.chat_id DESC \
                 LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(chat_type)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), None) => {
            // Cursor subquery: if after_id is not found or from a different
            // group, the subquery returns NULL → comparison is always false
            // → empty safe result (no data leak).
            sqlx::query_as(
                "SELECT c.id, c.group_id, c.name, c.type, cm.role, \
                        cm.joined_at, cm.muted, cm.last_read_at \
                 FROM chat_members cm \
                 JOIN chats c ON cm.chat_id = c.id \
                 WHERE cm.user_id = $1 \
                   AND c.group_id = $2 \
                   AND c.archived_at IS NULL \
                   AND (cm.joined_at, cm.chat_id) < ( \
                       SELECT cm2.joined_at, cm2.chat_id \
                       FROM chat_members cm2 \
                       JOIN chats c2 ON cm2.chat_id = c2.id \
                       WHERE cm2.user_id = $1 \
                         AND cm2.chat_id = $3 \
                         AND c2.group_id = $2 \
                   ) \
                 ORDER BY cm.joined_at DESC, cm.chat_id DESC \
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

        (Some(after_id), Some(chat_type)) => sqlx::query_as(
            "SELECT c.id, c.group_id, c.name, c.type, cm.role, \
                        cm.joined_at, cm.muted, cm.last_read_at \
                 FROM chat_members cm \
                 JOIN chats c ON cm.chat_id = c.id \
                 WHERE cm.user_id = $1 \
                   AND c.group_id = $2 \
                   AND c.type = $3 \
                   AND c.archived_at IS NULL \
                   AND (cm.joined_at, cm.chat_id) < ( \
                       SELECT cm2.joined_at, cm2.chat_id \
                       FROM chat_members cm2 \
                       JOIN chats c2 ON cm2.chat_id = c2.id \
                       WHERE cm2.user_id = $1 \
                         AND cm2.chat_id = $4 \
                         AND c2.group_id = $2 \
                   ) \
                 ORDER BY cm.joined_at DESC, cm.chat_id DESC \
                 LIMIT $5",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(chat_type)
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
        rows.last().map(|(chat_id, ..)| *chat_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(chat_id, group_id, name, chat_type, role, joined_at, muted, last_read_at)| {
                ChatMembershipSummary {
                    chat_id,
                    group_id,
                    name,
                    chat_type,
                    role,
                    joined_at,
                    muted,
                    last_read_at,
                }
            },
        )
        .collect();

    Ok(Json(MyChatsMembershipResponse { items, next_cursor }))
}

// ─── GET /v1/me/files ────────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/files`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyFilesQuery {
    /// Group to scope the file inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Cursor: `id` of the last file on the previous page (keyset on `created_at DESC, id DESC`).
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
    /// Optional folder filter. When set, only files directly inside this folder are returned.
    pub folder_id: Option<Uuid>,
}

/// One file entry in the `GET /v1/me/files` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyFileSummary {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Response body for `GET /v1/me/files`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyFilesResponse {
    pub items: Vec<MyFileSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/files",
    params(ListMyFilesQuery),
    responses(
        (status = 200, description = "List of files uploaded by caller, newest-created first.", body = MyFilesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_files(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyFilesQuery>,
) -> Result<Json<MyFilesResponse>, RestError> {
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

    // Columns: id, group_id, name, mime_type, size_bytes, folder_id, created_at, updated_at
    type FileRow = (
        Uuid,
        Uuid,
        String,
        String,
        i64,
        Option<Uuid>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );

    let rows: Vec<FileRow> = match (params.after, params.folder_id) {
        (None, None) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(folder_id)) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND folder_id = $3 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(folder_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), None) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT f2.created_at, f2.id \
                   FROM files f2 \
                   WHERE f2.id = $3 \
                     AND f2.group_id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), Some(folder_id)) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND folder_id = $3 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT f2.created_at, f2.id \
                   FROM files f2 \
                   WHERE f2.id = $4 \
                     AND f2.group_id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $5",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(folder_id)
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
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, group_id, name, mime_type, size_bytes, folder_id, created_at, updated_at)| {
                MyFileSummary {
                    id,
                    group_id,
                    name,
                    mime_type,
                    size_bytes,
                    folder_id,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(Json(MyFilesResponse { items, next_cursor }))
}

// ─── GET /v1/me/memory ───────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/memory`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyMemoryQuery {
    /// Keyset cursor — `id` of the last item on the previous page.
    /// Returns items created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
    /// Optional kind filter. Accepted: `fact`, `preference`, `note`,
    /// `reminder`, `rule`, `profile`. Unknown values are rejected with 400.
    pub kind: Option<String>,
}

impl ListMyMemoryQuery {
    fn validate_kind(k: &str) -> bool {
        matches!(
            k,
            "fact" | "preference" | "note" | "reminder" | "rule" | "profile"
        )
    }
}

/// One memory entry in the `GET /v1/me/memory` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyMemorySummary {
    pub id: Uuid,
    /// Semantic kind: `fact`, `preference`, `note`, `reminder`, `rule`, or `profile`.
    pub kind: String,
    /// First 200 characters of the memory content (preview only — avoids bulk PII exposure).
    pub content_preview: String,
    /// UTC timestamp when this item was pinned. `None` if not pinned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<DateTime<Utc>>,
    /// UTC expiry timestamp. `None` if this item never expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/memory`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyMemoryResponse {
    pub items: Vec<MyMemorySummary>,
    /// `id` of the last item in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/memory` — inbox of the caller's personal memory items.
///
/// Returns up to `limit` (default 50, max 100) non-deleted, non-expired personal
/// memory items (scope_type='user') created by the caller, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_item_id>`. Optional
/// `?kind=<fact|preference|note|reminder|rule|profile>` filter narrows results.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` (branch 2 of the
/// `memory_items_group_or_self` dual policy in migration 007). `app.current_group_id`
/// is also set to satisfy the FORCE RLS protocol even though personal memories
/// have `group_id IS NULL` and do not match branch 1.
#[utoipa::path(
    get,
    path = "/v1/me/memory",
    params(ListMyMemoryQuery),
    responses(
        (status = 200, description = "List of personal memory items, newest first.", body = MyMemoryResponse),
        (status = 400, description = "Validation error (invalid kind value).", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyMemoryQuery>,
) -> Result<Json<MyMemoryResponse>, RestError> {
    if let Some(ref k) = params.kind
        && !ListMyMemoryQuery::validate_kind(k)
    {
        return Err(RestError::BadRequest(
            "kind must be one of: fact, preference, note, reminder, rule, profile".into(),
        ));
    }

    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL app.current_user_id — required by branch 2 of the dual RLS policy
    // (memory_items_group_or_self: group_id IS NULL AND created_by = app.current_user_id).
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL app.current_group_id — FORCE RLS protocol requires both GUCs to be set.
    // Personal memories have group_id IS NULL so branch 1 (group match) never fires;
    // using nil UUID is safe and avoids an extra param on this caller-only endpoint.
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(Uuid::nil().to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Columns: id, kind, content_preview (LEFT 200), pinned_at, ttl_expires_at, created_at
    type MemRow = (
        Uuid,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );

    let rows: Vec<MemRow> = match (params.after, params.kind.as_deref()) {
        (None, None) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(kind)) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND kind = $2 \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(kind)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), None) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
               AND (created_at, id) < ( \
                   SELECT m2.created_at, m2.id FROM memory_items m2 \
                   WHERE m2.id = $2 \
                     AND m2.created_by = $1 \
                     AND m2.scope_type = 'user' \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), Some(kind)) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND kind = $2 \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
               AND (created_at, id) < ( \
                   SELECT m2.created_at, m2.id FROM memory_items m2 \
                   WHERE m2.id = $3 \
                     AND m2.created_by = $1 \
                     AND m2.scope_type = 'user' \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(kind)
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
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, kind, content_preview, pinned_at, ttl_expires_at, created_at)| MyMemorySummary {
                id,
                kind,
                content_preview,
                pinned_at,
                ttl_expires_at,
                created_at,
            },
        )
        .collect();

    Ok(Json(MyMemoryResponse { items, next_cursor }))
}

// ─── GET /v1/me/invites ──────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/invites`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyInvitesQuery {
    /// Keyset cursor — `id` of the last invite on the previous page.
    /// Returns invites created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
}

/// One pending group invite in the `GET /v1/me/invites` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingInviteSummary {
    /// UUID of the invite row.
    pub id: Uuid,
    /// UUID of the group the caller has been invited to join.
    pub group_id: Uuid,
    /// Role the inviter proposed: `admin`, `member`, `guest`, or `child`.
    pub proposed_role: String,
    /// UTC timestamp when the invite was created.
    pub created_at: DateTime<Utc>,
    /// UTC timestamp when the invite expires.
    pub expires_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/invites`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyInvitesResponse {
    pub items: Vec<PendingInviteSummary>,
    /// `id` of the last invite in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/invites` — inbox of pending group invites addressed to the caller.
///
/// Returns up to `limit` (default 50, max 100) non-accepted, non-expired group
/// invites sent to the caller's registered email address, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_invite_id>`. No `group_id` parameter —
/// this is a cross-group personal inbox.
///
/// `group_invites` has no FORCE RLS — isolation is enforced via explicit
/// `WHERE u.id = $principal_user_id` after joining `users ON email = invited_email`.
/// `token_hash` and `invited_email` are never included in the response.
#[utoipa::path(
    get,
    path = "/v1/me/invites",
    params(ListMyInvitesQuery),
    responses(
        (status = 200, description = "List of pending group invites, newest first.", body = MyInvitesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_invites(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyInvitesQuery>,
) -> Result<Json<MyInvitesResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);

    let pool = state.app_pool.pool_for_handlers();

    // Columns: gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at
    type InviteRow = (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>);

    let rows: Vec<InviteRow> = match params.after {
        None => sqlx::query_as(
            "SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at \
             FROM group_invites gi \
             JOIN users u ON u.email = gi.invited_email \
             WHERE u.id = $1 \
               AND gi.accepted_at IS NULL \
               AND gi.revoked_at IS NULL \
               AND gi.declined_at IS NULL \
               AND gi.expires_at > now() \
             ORDER BY gi.created_at DESC, gi.id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        Some(after_id) => sqlx::query_as(
            "SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at \
             FROM group_invites gi \
             JOIN users u ON u.email = gi.invited_email \
             WHERE u.id = $1 \
               AND gi.accepted_at IS NULL \
               AND gi.revoked_at IS NULL \
               AND gi.declined_at IS NULL \
               AND gi.expires_at > now() \
               AND (gi.created_at, gi.id) < ( \
                   SELECT gi2.created_at, gi2.id FROM group_invites gi2 \
                   JOIN users u2 ON u2.email = gi2.invited_email \
                   WHERE gi2.id = $2 AND u2.id = $1 \
               ) \
             ORDER BY gi.created_at DESC, gi.id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, group_id, proposed_role, created_at, expires_at)| PendingInviteSummary {
                id,
                group_id,
                proposed_role,
                created_at,
                expires_at,
            },
        )
        .collect();

    Ok(Json(MyInvitesResponse { items, next_cursor }))
}

// ─── POST /v1/me/invites/{invite_id}/decline ─────────────────────────────────

/// `POST /v1/me/invites/{invite_id}/decline` — invitee-side decline of a pending
/// group invite (plan 0258 / GAR-783).
///
/// Sets `declined_at = now()` and `declined_by = caller_user_id` on the invite row.
/// Returns 204 No Content on success. Returns 404 if the invite does not exist,
/// does not belong to the caller, has already been accepted, revoked, or declined.
///
/// No `X-Group-Id` header required — `group_id` is resolved from the invite row.
/// No capability check — any authenticated user may decline their own invite.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Invite not found / not the caller's / terminal | 404    |
/// | Happy path                                     | 204    |
#[utoipa::path(
    post,
    path = "/v1/me/invites/{invite_id}/decline",
    params(
        ("invite_id" = Uuid, Path, description = "Invite UUID to decline."),
    ),
    responses(
        (status = 204, description = "Invite declined."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "Invite not found, not addressed to caller, or already terminal (accepted/revoked/declined).", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn decline_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(invite_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL user_id for FORCE-RLS on audit_events.
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Soft-decline: verify the caller is the invitee (JOIN users ON email),
    // ensure the invite is still pending, then set declined_at + declined_by.
    // RETURNING group_id + proposed_role for audit (group_id also needed for
    // SET LOCAL app.current_group_id before the audit_events INSERT).
    #[derive(sqlx::FromRow)]
    struct DeclinedRow {
        group_id: Uuid,
        proposed_role: String,
    }

    let declined: Option<DeclinedRow> = sqlx::query_as(
        "UPDATE group_invites gi \
         SET declined_at = now(), declined_by = u.id \
         FROM users u \
         WHERE u.email = gi.invited_email \
           AND u.id = $1 \
           AND gi.id = $2 \
           AND gi.accepted_at IS NULL \
           AND gi.revoked_at IS NULL \
           AND gi.declined_at IS NULL \
         RETURNING gi.group_id, gi.proposed_role",
    )
    .bind(principal.user_id)
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = declined.ok_or(RestError::NotFound)?;

    // SET LOCAL group_id now that we know it (required for FORCE-RLS audit_events INSERT).
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(row.group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Emit audit event. Metadata: proposed_role only — invited_email is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::InviteDeclined,
        principal.user_id,
        row.group_id,
        "group_invites",
        invite_id.to_string(),
        json!({ "proposed_role": row.proposed_role }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── POST /v1/me/invites/{invite_id}/accept ───────────────────────────────────

/// Response body for `POST /v1/me/invites/{invite_id}/accept` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptMyInviteResponse {
    /// The group the caller just joined.
    pub group_id: Uuid,
    /// The role assigned from the invite (`member`, `admin`, `guest`, or `child`).
    pub role: String,
    /// The invite UUID that was accepted.
    pub invite_id: Uuid,
}

/// `POST /v1/me/invites/{invite_id}/accept` — accept a pending group invite by UUID.
///
/// Authenticated variant of the token-based `POST /v1/invites/{token}/accept`
/// (plan 0019). The caller provides the invite UUID from their inbox
/// (`GET /v1/me/invites`); no raw plaintext token is required.
///
/// Verifies the invite belongs to the caller (by email match), is not yet
/// terminal (accepted/revoked/declined), and is not expired. On success,
/// atomically marks the invite as accepted and inserts the caller into
/// `group_members` with the `proposed_role` from the invite.
///
/// No `X-Group-Id` header required — `group_id` is resolved from the invite.
/// No capability check — any authenticated user may accept their own invite.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Invite not found / not the caller's / terminal | 404    |
/// | Invite expired                                 | 410    |
/// | Caller already a member of the group           | 409    |
/// | Happy path                                     | 200    |
#[utoipa::path(
    post,
    path = "/v1/me/invites/{invite_id}/accept",
    params(
        ("invite_id" = Uuid, Path, description = "Invite UUID to accept."),
    ),
    responses(
        (status = 200, description = "Invite accepted; caller is now a group member.", body = AcceptMyInviteResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "Invite not found, not addressed to caller, or already terminal (accepted/revoked/declined).", body = super::problem::ProblemDetails),
        (status = 409, description = "Caller is already a member of this group.", body = super::problem::ProblemDetails),
        (status = 410, description = "Invite has expired.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn accept_my_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(invite_id): Path<Uuid>,
) -> Result<Json<AcceptMyInviteResponse>, RestError> {
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL user_id for FORCE-RLS on audit_events.
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Atomically mark the invite as accepted.
    // All terminal-state guards are in the WHERE clause so a concurrent
    // double-accept by another session returns 0 rows affected (safe).
    #[derive(sqlx::FromRow)]
    struct AcceptedRow {
        group_id: Uuid,
        proposed_role: String,
        invited_by: Option<Uuid>,
    }

    let accepted: Option<AcceptedRow> = sqlx::query_as(
        "UPDATE group_invites gi \
         SET accepted_at = now(), accepted_by = u.id \
         FROM users u \
         WHERE u.email = gi.invited_email \
           AND u.id = $1 \
           AND gi.id = $2 \
           AND gi.accepted_at IS NULL \
           AND gi.revoked_at IS NULL \
           AND gi.declined_at IS NULL \
           AND gi.expires_at >= now() \
         RETURNING gi.group_id, gi.proposed_role, gi.created_by AS invited_by",
    )
    .bind(principal.user_id)
    .bind(invite_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match accepted {
        Some(r) => r,
        None => {
            // Distinguish 410 (expired) from 404 (not found / terminal / wrong user).
            #[derive(sqlx::FromRow)]
            struct ExpiryRow {
                is_expired: bool,
            }
            let expiry: Option<ExpiryRow> = sqlx::query_as(
                "SELECT gi.expires_at < now() AS is_expired \
                 FROM group_invites gi \
                 JOIN users u ON u.email = gi.invited_email \
                 WHERE u.id = $1 AND gi.id = $2",
            )
            .bind(principal.user_id)
            .bind(invite_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            return match expiry {
                Some(e) if e.is_expired => Err(RestError::Gone("this invite has expired".into())),
                _ => Err(RestError::NotFound),
            };
        }
    };

    // SET LOCAL group_id now that we know it (required for FORCE-RLS audit_events INSERT).
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(row.group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Insert group_members. SQLSTATE 23505 (unique violation on PK) means the
    // caller is already a member of this group — 409 Conflict.
    let insert_result = sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, invited_by) \
         VALUES ($1, $2, $3, 'active', $4)",
    )
    .bind(row.group_id)
    .bind(principal.user_id)
    .bind(&row.proposed_role)
    .bind(row.invited_by)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Ok(_) => {}
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "you are already a member of this group".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    }

    // Emit audit event. Metadata: proposed_role only — invited_email is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::InviteAccepted,
        principal.user_id,
        row.group_id,
        "group_invites",
        invite_id.to_string(),
        json!({ "proposed_role": row.proposed_role }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(AcceptMyInviteResponse {
        group_id: row.group_id,
        role: row.proposed_role,
        invite_id,
    }))
}

// ─── GET /v1/me/reactions ────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/reactions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListReactionsQuery {
    /// Group to scope the reactions inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Keyset cursor — `message_id` of the last row on the previous page.
    /// Returns rows with an earlier `MAX(reacted_at)` (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `20`.
    pub limit: Option<i64>,
}

/// Internal row type for decoding GROUP BY + ARRAY_AGG results.
///
/// `sqlx::FromRow` on a named struct is required here because `Vec<String>` cannot
/// be decoded from a PostgreSQL array column via an anonymous tuple type alias.
#[derive(Debug, sqlx::FromRow)]
struct ReactionGroupRow {
    message_id: Uuid,
    chat_id: Uuid,
    group_id: Uuid,
    sender_user_id: Uuid,
    sender_label: String,
    body_excerpt: String,
    emojis: Vec<String>,
    reacted_at: DateTime<Utc>,
}

/// One message-level reaction summary in the `GET /v1/me/reactions` response.
///
/// `emojis` contains all distinct emoji the caller placed on this message,
/// ordered by first reaction time then lexicographically.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyReactionSummary {
    /// UUID of the message the caller reacted to.
    pub message_id: Uuid,
    /// UUID of the chat containing the message.
    pub chat_id: Uuid,
    /// UUID of the group (denormalized from `message_reactions.group_id`).
    pub group_id: Uuid,
    /// UUID of the user who sent the message.
    pub sender_user_id: Uuid,
    /// Display label of the message sender at send time.
    pub sender_label: String,
    /// First 200 characters of the message body.
    pub body_excerpt: String,
    /// All emoji the caller placed on this message.
    pub emojis: Vec<String>,
    /// `MAX(reacted_at)` — when the caller most recently reacted to this message.
    pub reacted_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/reactions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyReactionsResponse {
    pub items: Vec<MyReactionSummary>,
    /// `message_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/reactions` — inbox of messages the authenticated caller has reacted to.
///
/// Returns up to `limit` (default 20, max 100) messages in `group_id` on which
/// the caller has at least one emoji reaction, ordered by
/// `(MAX(reacted_at) DESC, message_id DESC)`. Each item contains the full list
/// of emoji the caller placed on that message (`ARRAY_AGG` per-message group).
///
/// Cursor-based pagination via `?after=<last_message_id>`.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/reactions",
    params(ListReactionsQuery),
    responses(
        (status = 200, description = "Emoji reactions grouped by message, newest first.", body = MyReactionsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_reactions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListReactionsQuery>,
) -> Result<Json<MyReactionsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
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

    let rows: Vec<ReactionGroupRow> = if let Some(after_id) = params.after {
        // HAVING cursor: if after_id is deleted or from a different group the
        // inner SELECT returns no rows → MAX returns NULL → comparison is false
        // → empty safe result (same fail-closed pattern as list_my_mentions).
        sqlx::query_as(
            "SELECT mr.message_id, m.chat_id, mr.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    ARRAY_AGG(mr.emoji ORDER BY mr.reacted_at, mr.emoji) AS emojis, \
                    MAX(mr.reacted_at) AS reacted_at \
             FROM message_reactions mr \
             JOIN messages m ON mr.message_id = m.id \
             WHERE mr.user_id = $1 AND mr.group_id = $2 \
             GROUP BY mr.message_id, m.chat_id, mr.group_id, \
                      m.sender_user_id, m.sender_label, m.body \
             HAVING (MAX(mr.reacted_at), mr.message_id) < ( \
                 SELECT MAX(reacted_at), $3::uuid \
                 FROM message_reactions \
                 WHERE message_id = $3 AND user_id = $1 \
             ) \
             ORDER BY MAX(mr.reacted_at) DESC, mr.message_id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT mr.message_id, m.chat_id, mr.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    ARRAY_AGG(mr.emoji ORDER BY mr.reacted_at, mr.emoji) AS emojis, \
                    MAX(mr.reacted_at) AS reacted_at \
             FROM message_reactions mr \
             JOIN messages m ON mr.message_id = m.id \
             WHERE mr.user_id = $1 AND mr.group_id = $2 \
             GROUP BY mr.message_id, m.chat_id, mr.group_id, \
                      m.sender_user_id, m.sender_label, m.body \
             ORDER BY MAX(mr.reacted_at) DESC, mr.message_id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|r| r.message_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|r| MyReactionSummary {
            message_id: r.message_id,
            chat_id: r.chat_id,
            group_id: r.group_id,
            sender_user_id: r.sender_user_id,
            sender_label: r.sender_label,
            body_excerpt: r.body_excerpt,
            emojis: r.emojis,
            reacted_at: r.reacted_at,
        })
        .collect();

    Ok(Json(MyReactionsResponse { items, next_cursor }))
}

// ─── GET /v1/me/threads ──────────────────────────────────────────────────────

/// Internal DB row returned by the threads participation query.
#[derive(sqlx::FromRow)]
struct ThreadRow {
    thread_id: Uuid,
    chat_id: Uuid,
    group_id: Uuid,
    title: Option<String>,
    root_message_id: Uuid,
    resolved_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    created_by: Uuid,
}

/// Query params for `GET /v1/me/threads`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyThreadsQuery {
    /// Group UUID (required; sets RLS context).
    pub group_id: Uuid,
    /// Keyset cursor — thread UUID of the last item on the previous page.
    pub after: Option<Uuid>,
    /// Page size, clamped 1..100; defaults to 20.
    pub limit: Option<i64>,
    /// When `true`, include resolved threads. Defaults to `false`.
    pub include_resolved: Option<bool>,
}

/// One thread in the caller's thread-participation inbox.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyThreadSummary {
    pub thread_id: Uuid,
    pub chat_id: Uuid,
    pub group_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub root_message_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// `"creator"` if the caller started this thread; `"participant"` otherwise.
    pub role: String,
}

/// Response body for `GET /v1/me/threads`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyThreadsResponse {
    pub items: Vec<MyThreadSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// List threads where the authenticated caller is a creator or participant.
///
/// Returns threads in a given group where `created_by = caller` OR the caller
/// has posted at least one non-deleted reply (`messages.thread_id = thread.id
/// AND messages.sender_user_id = caller`).  Keyset cursor on
/// `(mt.created_at DESC, mt.id DESC)`.  FORCE RLS is enforced via
/// parameterised `SET LOCAL` for both `app.current_user_id` and
/// `app.current_group_id`.
#[utoipa::path(
    get,
    path = "/v1/me/threads",
    params(ListMyThreadsQuery),
    responses(
        (status = 200, description = "Thread participation inbox", body = MyThreadsResponse),
        (status = 400, description = "Missing or invalid parameters"),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_threads(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyThreadsQuery>,
) -> Result<Json<MyThreadsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let group_id = params.group_id;
    let include_resolved = params.include_resolved.unwrap_or(false);

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

    // 4-branch static SQL: cursor × include_resolved.
    // The cursor sub-select is gated by FORCE RLS, so a cross-group or deleted
    // after_id returns NULL → row-value comparison yields NULL → 0 rows (fail-closed).
    let rows: Vec<ThreadRow> = match (params.after, include_resolved) {
        (None, false) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND mt.resolved_at IS NULL \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), false) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND mt.resolved_at IS NULL \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
               AND (mt.created_at, mt.id) < ( \
                   SELECT created_at, id FROM message_threads WHERE id = $4 \
               ) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, true) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), true) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
               AND (mt.created_at, mt.id) < ( \
                   SELECT created_at, id FROM message_threads WHERE id = $4 \
               ) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|r| r.thread_id)
    } else {
        None
    };

    let user_id = principal.user_id;
    let items = rows
        .into_iter()
        .map(|r| MyThreadSummary {
            thread_id: r.thread_id,
            chat_id: r.chat_id,
            group_id: r.group_id,
            title: r.title,
            root_message_id: r.root_message_id,
            resolved_at: r.resolved_at,
            created_at: r.created_at,
            role: if r.created_by == user_id {
                "creator".into()
            } else {
                "participant".into()
            },
        })
        .collect();

    Ok(Json(MyThreadsResponse { items, next_cursor }))
}

// ─── GET /v1/me/doc-page-mentions ────────────────────────────────────────────

/// Query parameters for `GET /v1/me/doc-page-mentions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyDocPageMentionsQuery {
    /// Group to scope the mention inbox. Required — caller must be a member.
    pub group_id: Uuid,
    /// Keyset cursor — `page_id` of the last item. Returns mentions with
    /// `created_at` older than that row (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<i64>,
}

/// A single doc page mention in the caller's inbox.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageMentionInboxSummary {
    /// UUID of the doc page in which the caller was mentioned.
    pub page_id: Uuid,
    /// UUID of the group (denormalized).
    pub group_id: Uuid,
    /// Title of the doc page at the time of the query.
    pub page_title: String,
    /// UTC timestamp when the mention was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/doc-page-mentions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageMentionsInboxResponse {
    pub items: Vec<DocPageMentionInboxSummary>,
    /// `page_id` of the last item. Pass as `?after=<uuid>` for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/doc-page-mentions` — inbox of doc page @mentions received by the caller.
///
/// Returns up to `limit` (default 50, max 100) mentions in `group_id`,
/// ordered by `(dpm.created_at DESC, dpm.page_id DESC)`.
/// Cursor-based pagination via `?after=<last_page_id>`.
#[utoipa::path(
    get,
    path = "/v1/me/doc-page-mentions",
    params(ListMyDocPageMentionsQuery),
    responses(
        (status = 200, description = "List of doc page @mentions, newest first.", body = DocPageMentionsInboxResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_doc_page_mentions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyDocPageMentionsQuery>,
) -> Result<Json<DocPageMentionsInboxResponse>, RestError> {
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

    // (page_id, group_id, page_title, created_at)
    type InboxRow = (Uuid, Uuid, String, DateTime<Utc>);

    let rows: Vec<InboxRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT dpm.page_id, dpm.group_id, \
                    COALESCE(dp.title, '') AS page_title, \
                    dpm.created_at \
             FROM doc_page_mentions dpm \
             LEFT JOIN doc_pages dp ON dp.id = dpm.page_id \
             WHERE dpm.mentioned_user_id = $1 \
               AND dpm.group_id = $2 \
               AND (dpm.created_at, dpm.page_id) < ( \
                   SELECT created_at, page_id \
                   FROM doc_page_mentions \
                   WHERE page_id = $3 AND mentioned_user_id = $1 \
               ) \
             ORDER BY dpm.created_at DESC, dpm.page_id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT dpm.page_id, dpm.group_id, \
                    COALESCE(dp.title, '') AS page_title, \
                    dpm.created_at \
             FROM doc_page_mentions dpm \
             LEFT JOIN doc_pages dp ON dp.id = dpm.page_id \
             WHERE dpm.mentioned_user_id = $1 \
               AND dpm.group_id = $2 \
             ORDER BY dpm.created_at DESC, dpm.page_id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(pid, _, _, _)| *pid)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(page_id, group_id, page_title, created_at)| DocPageMentionInboxSummary {
                page_id,
                group_id,
                page_title,
                created_at,
            },
        )
        .collect();

    Ok(Json(DocPageMentionsInboxResponse { items, next_cursor }))
}

// ─── GET /v1/me/doc-pages ────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/doc-pages`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyDocPagesQuery {
    /// Group to scope the inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Keyset cursor — `id` of the last doc page on the previous page.
    /// Returns pages created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Page size. Default 20, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
    /// When `true`, archived pages (`archived_at IS NOT NULL`) are included.
    /// Default `false` — archived pages are excluded.
    pub include_archived: Option<bool>,
}

/// One doc page entry in the `GET /v1/me/doc-pages` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyDocPageSummary {
    /// UUID of the doc page.
    pub id: Uuid,
    /// UUID of the group the page belongs to.
    pub group_id: Uuid,
    /// UUID of the parent page. `null` for root-level pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_page_id: Option<Uuid>,
    /// Page title.
    pub title: String,
    /// Optional emoji or icon identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// UTC timestamp when the page was soft-deleted (archived). `null` if active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// UTC timestamp when the page was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/doc-pages`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyDocPagesResponse {
    pub items: Vec<MyDocPageSummary>,
    /// `id` of the last item in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/doc-pages` — inbox of doc pages authored by the authenticated caller.
///
/// Returns up to `limit` (default 20, max 100) doc pages in `group_id` where
/// `doc_pages.created_by = caller_user_id`, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_page_id>`. Optional
/// `?include_archived=true` includes pages with `archived_at IS NOT NULL`
/// (archived pages are excluded by default).
///
/// FORCE RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
/// A cross-group `after=` cursor returns 0 rows (fail-closed, no info leak).
#[utoipa::path(
    get,
    path = "/v1/me/doc-pages",
    params(ListMyDocPagesQuery),
    responses(
        (status = 200, description = "List of authored doc pages, newest first.", body = MyDocPagesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_doc_pages(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyDocPagesQuery>,
) -> Result<Json<MyDocPagesResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let group_id = params.group_id;
    let include_archived = params.include_archived.unwrap_or(false);

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

    // Columns: id, group_id, parent_page_id, title, icon, archived_at, created_at
    type DocPageInboxRow = (
        Uuid,
        Uuid,
        Option<Uuid>,
        String,
        Option<String>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );

    let rows: Vec<DocPageInboxRow> = if let Some(after_id) = params.after {
        // Cursor subquery: if after_id is not found or belongs to a different
        // group, the subquery returns NULL → comparison is always false → 0 rows
        // (fail-closed, no info leak for cross-group cursor attacks).
        sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, archived_at, created_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND created_by = $2 \
               AND ($5 OR archived_at IS NULL) \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM doc_pages \
                   WHERE id = $3 AND group_id = $1 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .bind(include_archived)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, archived_at, created_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND created_by = $2 \
               AND ($3 OR archived_at IS NULL) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(include_archived)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, group_id, parent_page_id, title, icon, archived_at, created_at)| {
                MyDocPageSummary {
                    id,
                    group_id,
                    parent_page_id,
                    icon,
                    title,
                    archived_at,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MyDocPagesResponse { items, next_cursor }))
}

// ─── GET /v1/me/sessions ─────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/sessions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMySessionsQuery {
    /// Cursor: `id` of the last session on the previous page (keyset on
    /// `created_at DESC, id DESC`).
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default 20.
    pub limit: Option<i64>,
}

/// One active session entry in the `GET /v1/me/sessions` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionSummary {
    /// Session UUID.
    pub id: Uuid,
    /// Opaque device identifier supplied at login, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// When the session token expires (UTC).
    pub expires_at: DateTime<Utc>,
    /// When the session was created (UTC).
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/sessions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MySessionsResponse {
    pub items: Vec<SessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// RLS is enforced via `sessions_owner_only` (migration 007) which filters by
/// `app.current_user_id`. No `X-Group-Id` required — sessions are user-scoped.
/// `app.current_group_id` is set to nil-uuid per convention.
#[utoipa::path(
    get,
    path = "/v1/me/sessions",
    params(ListMySessionsQuery),
    responses(
        (status = 200, description = "List of active sessions for the caller, newest-created first.", body = MySessionsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_sessions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMySessionsQuery>,
) -> Result<Json<MySessionsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let nil_uuid = Uuid::nil();

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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    type SessionRow = (Uuid, Option<String>, DateTime<Utc>, DateTime<Utc>);

    let rows: Vec<SessionRow> = match params.after {
        None => sqlx::query_as(
            "SELECT id, device_id, expires_at, created_at \
             FROM sessions \
             WHERE user_id = $1 \
               AND revoked_at IS NULL \
               AND expires_at > now() \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        Some(after_id) => sqlx::query_as(
            "SELECT id, device_id, expires_at, created_at \
             FROM sessions \
             WHERE user_id = $1 \
               AND revoked_at IS NULL \
               AND expires_at > now() \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM sessions \
                   WHERE id = $2 AND user_id = $1 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
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
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|(id, device_id, expires_at, created_at)| SessionSummary {
            id,
            device_id,
            expires_at,
            created_at,
        })
        .collect();

    Ok(Json(MySessionsResponse { items, next_cursor }))
}

// ─── DELETE /v1/me/sessions/{session_id} ─────────────────────────────────────

/// `DELETE /v1/me/sessions/{session_id}` — revoke a specific session
/// (plan 0326 / GAR-866). Sets `revoked_at = now()` on the session row.
///
/// Idempotent: if the session is already revoked, returns 204 (not an error).
/// Returns 404 if the session does not exist or belongs to another user
/// (FORCE RLS via `sessions_owner_only` hides cross-user rows).
///
/// No `X-Group-Id` header required — sessions are user-scoped.
#[utoipa::path(
    delete,
    path = "/v1/me/sessions/{session_id}",
    params(
        ("session_id" = Uuid, Path, description = "Session UUID to revoke."),
    ),
    responses(
        (status = 204, description = "Session revoked, or already revoked (idempotent)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "Session not found or belongs to another user.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn revoke_my_session(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(session_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let nil_uuid = Uuid::nil();

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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // FORCE RLS (sessions_owner_only) ensures cross-user session_id returns 0 rows.
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        // Distinguish "already revoked" (204) from "not found / cross-user" (404).
        let exists: Option<(bool,)> =
            sqlx::query_as("SELECT true FROM sessions WHERE id = $1 AND user_id = $2")
                .bind(session_id)
                .bind(principal.user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        return if exists.is_some() {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(RestError::NotFound)
        };
    }

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::SessionRevoked,
        principal.user_id,
        nil_uuid,
        "sessions",
        session_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn me_response_serializes_without_group_when_absent() {
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: None,
            role: None,
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["user_id"], "00000000-0000-0000-0000-000000000000");
        assert!(
            v.get("group_id").is_none(),
            "absent group_id must be skipped"
        );
        assert!(v.get("role").is_none());
    }

    #[test]
    fn me_response_serializes_with_group_when_present() {
        let gid = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: Some(gid),
            role: Some("owner".into()),
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["group_id"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(v["role"], "owner");
    }

    #[test]
    fn patch_me_request_validates_name_too_long() {
        let req = PatchMeRequest {
            display_name: Some("x".repeat(129)),
        };
        assert!(
            req.validate().is_err(),
            "129-char name must fail validation"
        );
    }

    #[test]
    fn patch_me_request_validates_empty_name() {
        let req = PatchMeRequest {
            display_name: Some(String::new()),
        };
        assert!(req.validate().is_err(), "empty name must fail validation");
    }

    #[test]
    fn patch_me_request_allows_none() {
        let req = PatchMeRequest { display_name: None };
        assert!(req.validate().is_ok(), "None display_name is valid (no-op)");
    }

    #[test]
    fn patch_me_request_rejects_unknown_fields() {
        let json = r#"{"display_name": "Alice", "status": "deleted"}"#;
        let result: Result<PatchMeRequest, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown field 'status' must be rejected");
    }

    // ── MentionsListResponse serialization ─────────────────────────────────

    #[test]
    fn mentions_list_response_empty_no_cursor() {
        let resp = MentionsListResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(
            v.get("next_cursor").is_none(),
            "absent cursor must be skipped"
        );
    }

    #[test]
    fn mentions_list_response_with_cursor() {
        let cursor = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let resp = MentionsListResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "aaaaaaaa-0000-0000-0000-000000000001");
    }

    #[test]
    fn mention_summary_serializes_all_fields() {
        let msg_id = Uuid::nil();
        let chat_id = Uuid::parse_str("11111111-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("22222222-0000-0000-0000-000000000001").unwrap();
        let sender = Uuid::parse_str("33333333-0000-0000-0000-000000000001").unwrap();
        let summary = MentionSummary {
            message_id: msg_id,
            chat_id,
            group_id,
            sender_user_id: sender,
            sender_label: "Alice".into(),
            body_excerpt: "Hello @Bob".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["message_id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(v["sender_label"], "Alice");
        assert_eq!(v["body_excerpt"], "Hello @Bob");
    }

    #[test]
    fn list_mentions_query_defaults() {
        let q = ListMentionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

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

    // ── MyChatsMembershipResponse / ChatMembershipSummary serialization ───────

    #[test]
    fn my_chats_response_empty_no_cursor() {
        let resp = MyChatsMembershipResponse {
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
    fn my_chats_response_with_cursor() {
        let cursor = Uuid::parse_str("cccccccc-0000-0000-0000-000000000003").unwrap();
        let resp = MyChatsMembershipResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "cccccccc-0000-0000-0000-000000000003");
    }

    #[test]
    fn chat_membership_summary_serializes_all_fields() {
        let chat_id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000001").unwrap();
        let summary = ChatMembershipSummary {
            chat_id,
            group_id,
            name: "general".into(),
            chat_type: "channel".into(),
            role: "member".into(),
            joined_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["chat_id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "bbbbbbbb-0000-0000-0000-000000000001");
        assert_eq!(v["name"], "general");
        assert_eq!(v["type"], "channel");
        assert_eq!(v["role"], "member");
        assert_eq!(v["muted"], false);
        assert!(
            v.get("last_read_at").is_none(),
            "absent last_read_at must be omitted"
        );
    }

    #[test]
    fn chat_membership_summary_includes_last_read_at_when_present() {
        let summary = ChatMembershipSummary {
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "random".into(),
            chat_type: "channel".into(),
            role: "owner".into(),
            joined_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            muted: true,
            last_read_at: Some(chrono::DateTime::from_timestamp(1_000_000, 0).unwrap()),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("last_read_at").is_some(),
            "present last_read_at must be serialized"
        );
        assert_eq!(v["muted"], true);
    }

    #[test]
    fn chat_membership_type_field_serialized_as_type() {
        let summary = ChatMembershipSummary {
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "dm-chat".into(),
            chat_type: "dm".into(),
            role: "member".into(),
            joined_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(
            v["type"], "dm",
            "Rust field chat_type must serialize as JSON key 'type'"
        );
        assert!(
            v.get("chat_type").is_none(),
            "'chat_type' key must not appear"
        );
    }

    #[test]
    fn list_my_chats_query_valid_type_values() {
        for t in &["channel", "dm", "thread"] {
            assert!(
                ListMyChatsQuery::validate_type(t),
                "expected '{t}' to be valid"
            );
        }
    }

    #[test]
    fn list_my_chats_query_invalid_type_value() {
        assert!(
            !ListMyChatsQuery::validate_type("direct"),
            "expected 'direct' to be invalid"
        );
        assert!(
            !ListMyChatsQuery::validate_type(""),
            "expected empty string to be invalid"
        );
        assert!(
            !ListMyChatsQuery::validate_type("Channel"),
            "expected 'Channel' (capitalized) to be invalid"
        );
    }

    #[test]
    fn list_my_chats_query_defaults() {
        let q = ListMyChatsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            chat_type: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.chat_type.is_none());
    }

    // ── MyFilesResponse / MyFileSummary serialization ─────────────────────────

    #[test]
    fn my_files_response_empty_no_cursor() {
        let resp = MyFilesResponse {
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
    fn my_files_response_with_cursor() {
        let cursor = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let resp = MyFilesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "eeeeeeee-0000-0000-0000-000000000005");
    }

    #[test]
    fn my_file_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000001").unwrap();
        let folder_id = Uuid::parse_str("cccccccc-0000-0000-0000-000000000001").unwrap();
        let summary = MyFileSummary {
            id,
            group_id,
            name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            size_bytes: 12345,
            folder_id: Some(folder_id),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "bbbbbbbb-0000-0000-0000-000000000001");
        assert_eq!(v["name"], "report.pdf");
        assert_eq!(v["mime_type"], "application/pdf");
        assert_eq!(v["size_bytes"], 12345);
        assert_eq!(v["folder_id"], "cccccccc-0000-0000-0000-000000000001");
        assert!(
            v.get("updated_at").is_none(),
            "absent updated_at must be omitted"
        );
    }

    #[test]
    fn my_file_summary_omits_folder_id_when_absent() {
        let summary = MyFileSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "photo.jpg".into(),
            mime_type: "image/jpeg".into(),
            size_bytes: 500_000,
            folder_id: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: Some(chrono::DateTime::from_timestamp(1_000_000, 0).unwrap()),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("folder_id").is_none(),
            "absent folder_id must be omitted"
        );
        assert!(
            v.get("updated_at").is_some(),
            "present updated_at must be serialized"
        );
    }

    #[test]
    fn list_my_files_query_defaults() {
        let q = ListMyFilesQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            folder_id: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.folder_id.is_none());
    }

    #[test]
    fn list_my_files_query_with_all_fields() {
        let gid = Uuid::parse_str("dddddddd-0000-0000-0000-000000000004").unwrap();
        let after = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let fid = Uuid::parse_str("ffffffff-0000-0000-0000-000000000006").unwrap();
        let q = ListMyFilesQuery {
            group_id: gid,
            after: Some(after),
            limit: Some(25),
            folder_id: Some(fid),
        };
        assert_eq!(q.group_id, gid);
        assert_eq!(q.after, Some(after));
        assert_eq!(q.limit, Some(25));
        assert_eq!(q.folder_id, Some(fid));
    }

    #[test]
    fn list_my_files_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100);
        assert_eq!(under, 1);
        assert_eq!(default, 50);
    }

    #[test]
    fn my_file_summary_large_size_bytes() {
        let summary = MyFileSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "large.bin".into(),
            mime_type: "application/octet-stream".into(),
            size_bytes: 5_368_709_120i64,
            folder_id: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["size_bytes"], 5_368_709_120i64);
    }

    // ── MyMemoryResponse / MyMemorySummary serialization ─────────────────────

    #[test]
    fn my_memory_response_empty_no_cursor() {
        let resp = MyMemoryResponse {
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
    fn my_memory_response_with_cursor() {
        let cursor = Uuid::parse_str("ffffffff-0000-0000-0000-000000000007").unwrap();
        let resp = MyMemoryResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "ffffffff-0000-0000-0000-000000000007");
    }

    #[test]
    fn my_memory_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let pinned = chrono::DateTime::from_timestamp(500_000, 0).unwrap();
        let expires = chrono::DateTime::from_timestamp(9_999_999, 0).unwrap();
        let summary = MyMemorySummary {
            id,
            kind: "fact".into(),
            content_preview: "Alice prefers dark mode".into(),
            pinned_at: Some(pinned),
            ttl_expires_at: Some(expires),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["kind"], "fact");
        assert_eq!(v["content_preview"], "Alice prefers dark mode");
        assert!(
            v.get("pinned_at").is_some(),
            "present pinned_at must be serialized"
        );
        assert!(
            v.get("ttl_expires_at").is_some(),
            "present ttl_expires_at must be serialized"
        );
    }

    #[test]
    fn my_memory_summary_omits_optional_fields_when_absent() {
        let summary = MyMemorySummary {
            id: Uuid::nil(),
            kind: "preference".into(),
            content_preview: "Prefers concise replies".into(),
            pinned_at: None,
            ttl_expires_at: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("pinned_at").is_none(),
            "absent pinned_at must be omitted"
        );
        assert!(
            v.get("ttl_expires_at").is_none(),
            "absent ttl_expires_at must be omitted"
        );
    }

    #[test]
    fn list_my_memory_query_kind_valid_values() {
        for k in &["fact", "preference", "note", "reminder", "rule", "profile"] {
            assert!(
                ListMyMemoryQuery::validate_kind(k),
                "expected '{k}' to be valid"
            );
        }
    }

    #[test]
    fn list_my_memory_query_kind_invalid_values() {
        assert!(
            !ListMyMemoryQuery::validate_kind("unknown"),
            "expected 'unknown' to be invalid"
        );
        assert!(
            !ListMyMemoryQuery::validate_kind(""),
            "expected empty string to be invalid"
        );
        assert!(
            !ListMyMemoryQuery::validate_kind("Fact"),
            "expected 'Fact' (capitalized) to be invalid"
        );
    }

    #[test]
    fn list_my_memory_query_defaults() {
        let q = ListMyMemoryQuery {
            after: None,
            limit: None,
            kind: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.kind.is_none());
    }

    #[test]
    fn list_my_memory_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 50, "no limit defaults to 50");
    }

    // ── MyInvitesResponse / PendingInviteSummary serialization ───────────────

    #[test]
    fn my_invites_response_empty_no_cursor() {
        let resp = MyInvitesResponse {
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
    fn my_invites_response_with_cursor() {
        let cursor = Uuid::parse_str("cccccccc-0000-0000-0000-000000000003").unwrap();
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "cccccccc-0000-0000-0000-000000000003");
    }

    #[test]
    fn pending_invite_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let created = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let expires = chrono::DateTime::from_timestamp(9_999_999, 0).unwrap();
        let summary = PendingInviteSummary {
            id,
            group_id,
            proposed_role: "member".into(),
            created_at: created,
            expires_at: expires,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "bbbbbbbb-0000-0000-0000-000000000002");
        assert_eq!(v["proposed_role"], "member");
        assert!(v.get("created_at").is_some());
        assert!(v.get("expires_at").is_some());
    }

    #[test]
    fn pending_invite_summary_no_token_hash_field() {
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "guest".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            expires_at: chrono::DateTime::from_timestamp(9_999_999, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("token_hash").is_none(),
            "token_hash must never appear in response"
        );
        assert!(
            v.get("invited_email").is_none(),
            "invited_email must never appear in response"
        );
    }

    #[test]
    fn list_my_invites_query_defaults() {
        let q = ListMyInvitesQuery {
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn list_my_invites_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 50, "no limit defaults to 50");
    }

    #[test]
    fn list_my_invites_query_with_cursor() {
        let after = Uuid::parse_str("dddddddd-0000-0000-0000-000000000004").unwrap();
        let q = ListMyInvitesQuery {
            after: Some(after),
            limit: Some(10),
        };
        assert_eq!(q.after, Some(after));
        assert_eq!(q.limit, Some(10));
    }

    // ─── POST /v1/me/invites/{invite_id}/decline ─────────────────────────────

    #[test]
    fn pending_invite_summary_no_declined_at_field() {
        // PendingInviteSummary must not expose declined_at (migration 025 column
        // must never leak into the JSON response).
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "member".into(),
            created_at: chrono::DateTime::UNIX_EPOCH,
            expires_at: chrono::DateTime::UNIX_EPOCH,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("declined_at").is_none(),
            "declined_at must not leak into summary response"
        );
        assert!(
            v.get("declined_by").is_none(),
            "declined_by must not leak into summary response"
        );
    }

    #[test]
    fn my_invites_response_with_declined_invite_excluded() {
        // When a declined invite is excluded, the inbox is empty.
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(v.get("next_cursor").is_none() || v["next_cursor"].is_null());
    }

    #[test]
    fn my_invites_response_next_cursor_omitted_when_none() {
        let resp = MyInvitesResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        // next_cursor must be absent (skip_serializing_if None), not "null".
        assert!(
            v.get("next_cursor").is_none(),
            "next_cursor must be omitted when None"
        );
    }

    // ─── POST /v1/me/invites/{invite_id}/accept ──────────────────────────────

    #[test]
    fn accept_my_invite_response_serializes_all_fields() {
        let group_id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let invite_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let resp = AcceptMyInviteResponse {
            group_id,
            role: "member".into(),
            invite_id,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["group_id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["invite_id"], "bbbbbbbb-0000-0000-0000-000000000002");
        assert_eq!(v["role"], "member");
    }

    #[test]
    fn accept_my_invite_response_no_token_hash_or_email_fields() {
        // AcceptMyInviteResponse must never expose token_hash or invited_email.
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "admin".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(
            v.get("token_hash").is_none(),
            "token_hash must never appear in response"
        );
        assert!(
            v.get("invited_email").is_none(),
            "invited_email must never appear in response"
        );
        assert!(
            v.get("accepted_at").is_none(),
            "accepted_at must not be exposed in response"
        );
    }

    #[test]
    fn accept_my_invite_response_role_variants() {
        for role in &["owner", "admin", "member", "guest", "child"] {
            let resp = AcceptMyInviteResponse {
                group_id: Uuid::nil(),
                role: (*role).into(),
                invite_id: Uuid::nil(),
            };
            let v = serde_json::to_value(&resp).unwrap();
            assert_eq!(v["role"], *role, "role '{role}' must round-trip");
        }
    }

    #[test]
    fn accept_my_invite_response_nil_uuids_serialize_as_zeros() {
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "member".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v["group_id"], "00000000-0000-0000-0000-000000000000",
            "nil UUID must serialize as all-zeros string"
        );
        assert_eq!(
            v["invite_id"], "00000000-0000-0000-0000-000000000000",
            "nil UUID must serialize as all-zeros string"
        );
    }

    #[test]
    fn accept_my_invite_pending_invite_summary_excludes_accepted_at() {
        // PendingInviteSummary (used in GET /v1/me/invites) must not expose
        // accepted_at — once accepted, the invite is excluded from the inbox
        // by the WHERE clause, so leaking it would be both useless and noisy.
        let summary = PendingInviteSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            proposed_role: "member".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            expires_at: chrono::DateTime::from_timestamp(9_999_999, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("accepted_at").is_none(),
            "accepted_at must not appear in PendingInviteSummary"
        );
        assert!(
            v.get("accepted_by").is_none(),
            "accepted_by must not appear in PendingInviteSummary"
        );
    }

    #[test]
    fn accept_my_invite_response_has_exactly_three_fields() {
        let resp = AcceptMyInviteResponse {
            group_id: Uuid::nil(),
            role: "member".into(),
            invite_id: Uuid::nil(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(
            obj.len(),
            3,
            "AcceptMyInviteResponse must have exactly 3 fields: group_id, role, invite_id"
        );
        assert!(obj.contains_key("group_id"));
        assert!(obj.contains_key("role"));
        assert!(obj.contains_key("invite_id"));
    }

    // ─── GET /v1/me/reactions ─────────────────────────────────────────────────

    #[test]
    fn my_reactions_response_empty_no_cursor() {
        let resp = MyReactionsResponse {
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
    fn my_reactions_response_with_cursor() {
        let cursor = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let resp = MyReactionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "eeeeeeee-0000-0000-0000-000000000005");
    }

    #[test]
    fn my_reaction_summary_serializes_all_fields() {
        let message_id = Uuid::parse_str("11111111-0000-0000-0000-000000000001").unwrap();
        let chat_id = Uuid::parse_str("22222222-0000-0000-0000-000000000002").unwrap();
        let group_id = Uuid::parse_str("33333333-0000-0000-0000-000000000003").unwrap();
        let sender_user_id = Uuid::parse_str("44444444-0000-0000-0000-000000000004").unwrap();
        let ts = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let summary = MyReactionSummary {
            message_id,
            chat_id,
            group_id,
            sender_user_id,
            sender_label: "Alice".into(),
            body_excerpt: "Hello world".into(),
            emojis: vec!["👍".into(), "❤️".into()],
            reacted_at: ts,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["message_id"], "11111111-0000-0000-0000-000000000001");
        assert_eq!(v["chat_id"], "22222222-0000-0000-0000-000000000002");
        assert_eq!(v["emojis"][0], "👍");
        assert_eq!(v["emojis"][1], "❤️");
        assert!(v.get("reacted_at").is_some());
    }

    #[test]
    fn list_my_reactions_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_reactions_query_defaults() {
        let q = ListReactionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    // ── GET /v1/me/threads tests ──────────────────────────────────────────

    fn make_thread_summary(role: &str) -> MyThreadSummary {
        MyThreadSummary {
            thread_id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            title: None,
            root_message_id: Uuid::nil(),
            resolved_at: None,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            role: role.into(),
        }
    }

    #[test]
    fn my_threads_response_empty_no_cursor() {
        let resp = MyThreadsResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["items"].as_array().unwrap().is_empty());
        assert!(
            v.get("next_cursor").is_none(),
            "next_cursor must be absent when None"
        );
    }

    #[test]
    fn my_threads_response_with_cursor() {
        let cursor = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let resp = MyThreadsResponse {
            items: vec![make_thread_summary("creator")],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["next_cursor"], "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    }

    #[test]
    fn my_thread_summary_serializes_creator_role() {
        let s = make_thread_summary("creator");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["role"], "creator");
    }

    #[test]
    fn my_thread_summary_serializes_participant_role() {
        let s = make_thread_summary("participant");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["role"], "participant");
    }

    #[test]
    fn my_thread_summary_title_and_resolved_omitted_when_none() {
        let s = make_thread_summary("creator");
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("title").is_none(), "title must be absent when None");
        assert!(
            v.get("resolved_at").is_none(),
            "resolved_at must be absent when None"
        );
    }

    #[test]
    fn my_thread_summary_title_and_resolved_present_when_some() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let s = MyThreadSummary {
            thread_id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            title: Some("My thread".into()),
            root_message_id: Uuid::nil(),
            resolved_at: Some(now),
            created_at: now,
            role: "creator".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["title"], "My thread");
        assert!(v["resolved_at"].is_string());
    }

    #[test]
    fn list_my_threads_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_threads_query_defaults() {
        let q = ListMyThreadsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            include_resolved: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.include_resolved.is_none());
        assert!(
            !q.include_resolved.unwrap_or(false),
            "default include_resolved is false"
        );
    }

    // ─── GET /v1/me/doc-page-mentions tests ──────────────────────────────────

    #[test]
    fn doc_page_mention_inbox_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let s = DocPageMentionInboxSummary {
            page_id: Uuid::nil(),
            group_id: Uuid::nil(),
            page_title: "Design notes".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("page_id").is_some());
        assert!(v.get("group_id").is_some());
        assert!(v.get("page_title").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn doc_page_mention_inbox_summary_created_at_utc_z() {
        use chrono::TimeZone;
        let s = DocPageMentionInboxSummary {
            page_id: Uuid::nil(),
            group_id: Uuid::nil(),
            page_title: "Notes".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["created_at"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "expected UTC Z suffix, got: {ts}");
    }

    #[test]
    fn doc_page_mention_inbox_response_no_next_cursor_omitted() {
        let resp = DocPageMentionsInboxResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("next_cursor").is_none());
    }

    #[test]
    fn doc_page_mention_inbox_response_cursor_present() {
        let uid = Uuid::new_v4();
        let resp = DocPageMentionsInboxResponse {
            items: vec![],
            next_cursor: Some(uid),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"].as_str().unwrap(), uid.to_string());
    }

    #[test]
    fn doc_page_mention_inbox_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100);
        assert_eq!(under, 1);
        assert_eq!(default, 50);
    }

    #[test]
    fn doc_page_mention_inbox_query_defaults() {
        let q = ListMyDocPageMentionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    // ─── GET /v1/me/doc-pages tests ──────────────────────────────────────────

    #[test]
    fn my_doc_pages_response_empty_no_cursor() {
        let resp = MyDocPagesResponse {
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
    fn my_doc_pages_response_with_cursor() {
        let cursor = Uuid::parse_str("cccccccc-0000-0000-0000-000000000001").unwrap();
        let resp = MyDocPagesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "cccccccc-0000-0000-0000-000000000001");
    }

    #[test]
    fn my_doc_page_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let ts = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let archived_ts = Utc.with_ymd_and_hms(2026, 6, 13, 8, 0, 0).unwrap();
        let s = MyDocPageSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            parent_page_id: Some(Uuid::nil()),
            title: "Meeting notes".to_string(),
            icon: Some("📝".to_string()),
            archived_at: Some(archived_ts),
            created_at: ts,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("id").is_some());
        assert!(v.get("group_id").is_some());
        assert!(v.get("parent_page_id").is_some());
        assert_eq!(v["title"], "Meeting notes");
        assert_eq!(v["icon"], "📝");
        assert!(v.get("archived_at").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn my_doc_page_summary_omits_optional_fields_when_none() {
        use chrono::TimeZone;
        let ts = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let s = MyDocPageSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            parent_page_id: None,
            title: "Root page".to_string(),
            icon: None,
            archived_at: None,
            created_at: ts,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("parent_page_id").is_none(),
            "parent_page_id must be absent when None"
        );
        assert!(v.get("icon").is_none(), "icon must be absent when None");
        assert!(
            v.get("archived_at").is_none(),
            "archived_at must be absent when None"
        );
    }

    #[test]
    fn list_my_doc_pages_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_doc_pages_query_defaults() {
        let q = ListMyDocPagesQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            include_archived: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(
            !q.include_archived.unwrap_or(false),
            "default include_archived is false"
        );
    }

    // ── SessionSummary / MySessionsResponse serialization ────────────────────

    #[test]
    fn session_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id,
            device_id: Some("android-pixel-8".to_string()),
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["device_id"], "android-pixel-8");
        assert!(v.get("expires_at").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn session_summary_omits_device_id_when_none() {
        use chrono::TimeZone;
        let id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id,
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("device_id").is_none(),
            "absent device_id must be omitted"
        );
    }

    #[test]
    fn session_summary_expires_at_utc_z() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 12, 30, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["expires_at"].as_str().unwrap();
        assert!(
            ts.ends_with('Z'),
            "expires_at must be UTC with Z suffix: {ts}"
        );
    }

    #[test]
    fn session_summary_created_at_utc_z() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 14, 22, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["created_at"].as_str().unwrap();
        assert!(
            ts.ends_with('Z'),
            "created_at must be UTC with Z suffix: {ts}"
        );
    }

    #[test]
    fn my_sessions_response_empty_no_cursor() {
        let resp = MySessionsResponse {
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
    fn my_sessions_response_with_cursor() {
        let cursor = Uuid::parse_str("dddddddd-0000-0000-0000-000000000003").unwrap();
        let resp = MySessionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "dddddddd-0000-0000-0000-000000000003");
    }

    #[test]
    fn list_my_sessions_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit clamped to 100");
        assert_eq!(under, 1, "zero clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_sessions_query_defaults() {
        let q = ListMySessionsQuery {
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn session_summary_nil_uuid_roundtrip() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn my_sessions_no_refresh_token_hash_field() {
        use chrono::TimeZone;
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let s = SessionSummary {
            id: Uuid::nil(),
            device_id: None,
            expires_at: expires,
            created_at: created,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("refresh_token_hash").is_none(),
            "refresh_token_hash must never appear in session response"
        );
    }

    #[test]
    fn my_sessions_response_items_propagate() {
        use chrono::TimeZone;
        let id1 = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let id2 = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000002").unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let created = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        let resp = MySessionsResponse {
            items: vec![
                SessionSummary {
                    id: id1,
                    device_id: Some("ios".to_string()),
                    expires_at: expires,
                    created_at: created,
                },
                SessionSummary {
                    id: id2,
                    device_id: None,
                    expires_at: expires,
                    created_at: created,
                },
            ],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["items"][1]["id"], "bbbbbbbb-0000-0000-0000-000000000002");
    }

    #[test]
    fn my_sessions_response_cursor_is_uuid_string() {
        let cursor = Uuid::parse_str("ffffffff-0000-0000-0000-000000000001").unwrap();
        let resp = MySessionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let cursor_str = v["next_cursor"].as_str().unwrap();
        assert!(
            Uuid::parse_str(cursor_str).is_ok(),
            "next_cursor must parse as UUID: {cursor_str}"
        );
    }
}
