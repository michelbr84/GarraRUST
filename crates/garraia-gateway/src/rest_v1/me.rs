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

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
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
}
