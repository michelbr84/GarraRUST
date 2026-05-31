//! `/v1/chats/{chat_id}/messages` handlers (plan 0055, GAR-507,
//! epic GAR-WS-CHAT slice 2).
//!
//! Two endpoints on the `garraia_app` RLS-enforced pool. Both require an
//! `X-Group-Id` header matching the caller's group (the `Principal`
//! extractor does the membership lookup; non-members get 403 before this
//! code runs). Additionally the handler validates that `chat_id` belongs
//! to `principal.group_id` via a scoped SELECT within the RLS transaction
//! — returning 404 (not 403) to avoid leaking the existence of chats in
//! other tenants.
//!
//! ## Tenant-context protocol
//!
//! `messages` is under FORCE RLS (migration 007:80-87, policy
//! `messages_group_isolation`), so handlers MUST execute BOTH
//!
//! ```text
//! SET LOCAL app.current_user_id  = '{caller_uuid}'
//! SET LOCAL app.current_group_id = '{path_uuid}'
//! ```
//!
//! before any read or write to `messages` / `audit_events`.
//!
//! ## SQL injection posture
//!
//! `SET LOCAL` does not accept bind parameters in Postgres, so the two
//! UUIDs are interpolated via `format!`. `Uuid::Display` produces exactly
//! 36 hex-with-dash characters and no metacharacters — injection-safe by
//! construction. All user-controlled values (body, reply_to_id) use
//! `sqlx::query::bind`.
//!
//! ## body_tsv
//!
//! `messages.body_tsv` is `GENERATED ALWAYS AS … STORED`. It must NEVER
//! appear in INSERT column lists — Postgres maintains it automatically.

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

/// Maximum message body length (chars, mirrors DB CHECK).
const MAX_BODY_CHARS: usize = 100_000;

/// Maximum thread title length (chars).
const MAX_TITLE_CHARS: usize = 500;

/// Maximum number of @mentions per message (plan 0237 / GAR-755).
const MAX_MENTIONS: usize = 50;

/// Request body for `POST /v1/chats/{chat_id}/messages`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    /// Message text. Must be non-empty after trim. Max 100,000 characters
    /// (matches DB CHECK constraint on `messages.body`).
    pub body: String,
    /// Optional reference to the message being replied to. Accepted but
    /// not FK-verified at the handler level — the DB foreign key
    /// `reply_to_id REFERENCES messages(id) ON DELETE SET NULL` enforces
    /// integrity and returns a 400 if the referenced id does not exist.
    #[serde(default)]
    pub reply_to_id: Option<Uuid>,
    /// Optional list of user UUIDs to @mention in this message.
    /// Max 50. All UUIDs must be active members of the caller's group (422 otherwise).
    /// Deduplicated by the server — duplicate UUIDs in the list are ignored.
    #[serde(default)]
    pub mentions: Vec<Uuid>,
}

impl SendMessageRequest {
    /// Structural validation. Returns `Ok(())` or `Err(&'static str)`.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.body.trim().is_empty() {
            return Err("message body must not be empty");
        }
        if self.body.chars().count() > MAX_BODY_CHARS {
            return Err("message body must be 100,000 characters or fewer");
        }
        if self.mentions.len() > MAX_MENTIONS {
            return Err("too many mentions: maximum is 50 per message");
        }
        Ok(())
    }
}

/// Response body for `POST /v1/chats/{chat_id}/messages` (201 Created).
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub group_id: Uuid,
    pub sender_user_id: Uuid,
    pub sender_label: String,
    pub body: String,
    pub reply_to_id: Option<Uuid>,
    pub is_bot_response: bool,
    pub created_at: DateTime<Utc>,
    /// List of user UUIDs @mentioned in this message. Empty when no mentions.
    pub mentions: Vec<Uuid>,
}

/// Compact summary used by `GET /v1/chats/{chat_id}/messages`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageSummary {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub sender_user_id: Uuid,
    pub sender_label: String,
    pub body: String,
    pub reply_to_id: Option<Uuid>,
    pub is_bot_response: bool,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/chats/{chat_id}/messages` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageListResponse {
    pub items: Vec<MessageSummary>,
    /// UUID of the last message in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. `None` when the end of the history
    /// has been reached.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/chats/{chat_id}/messages`.
#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    /// Keyset cursor — UUID of the last message received. Returns messages
    /// older than this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values > 100 are clamped to 100.
    /// Values < 1 are rejected with 400.
    pub limit: Option<i64>,
}

/// `POST /v1/chats/{chat_id}/messages` — send a message to a chat.
///
/// Authz: caller must be a group member with `Action::ChatsWrite`.
/// All 5 roles hold this capability. The handler additionally verifies
/// that `chat_id` belongs to `principal.group_id` (0 rows → 404).
///
/// `sender_label` is resolved from `users.display_name` in the same
/// transaction (erasure-survival: label cached at send time).
///
/// ## Error matrix
///
/// | Condition                              | Status | Source         |
/// |----------------------------------------|--------|----------------|
/// | Missing/invalid JWT                    | 401    | Principal ext. |
/// | Non-member of group                    | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched      | 400    | this handler   |
/// | Body empty or > 100,000 chars          | 400    | validate()     |
/// | Chat not found in caller's group       | 404    | this handler   |
/// | Happy path                             | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/chats/{chat_id}/messages",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
    ),
    request_body = SendMessageRequest,
    responses(
        (status = 201, description = "Message sent.", body = MessageResponse),
        (status = 400, description = "Validation error or header mismatch.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn send_message(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), RestError> {
    // 1. Header/path coherence — same rule as chats.rs handlers.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate — all 5 roles have ChatsWrite.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural validation.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let trimmed_body = body.body.trim().to_string();

    // 4. Open transaction with RLS context.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context — both user and group required.
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

    // 6. Verify chat belongs to principal.group_id. 0 rows → 404 (not 403)
    //    to avoid leaking the existence of chats in other tenants.
    let chat_group: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_group.is_none() {
        return Err(RestError::NotFound);
    }

    // 7. Resolve sender_label from users.display_name within the tx.
    //    display_name is NOT NULL in the users table (migration 001).
    let (sender_label,): (String,) = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 8. INSERT message. NEVER include body_tsv — it is GENERATED ALWAYS AS.
    let (msg_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO messages \
             (chat_id, group_id, sender_user_id, sender_label, body, reply_to_id) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, created_at",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(principal.user_id)
    .bind(&sender_label)
    .bind(&trimmed_body)
    .bind(body.reply_to_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 9. Process @mentions (plan 0237 / GAR-755).
    //    Deduplicate the input list (PK will reject duplicates, but we need
    //    the deduplicated count for validation and audit).
    let mut mention_ids = body.mentions.clone();
    mention_ids.sort_unstable();
    mention_ids.dedup();
    let mention_count = mention_ids.len();

    if mention_count > 0 {
        // 9a. Validate: every mentioned UUID must be an active group member.
        //     COUNT(*) must equal the deduplicated input count; any mismatch
        //     means at least one UUID is not in this group → 422.
        let (valid_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM group_members \
             WHERE group_id = $1 AND user_id = ANY($2)",
        )
        .bind(group_id)
        .bind(&mention_ids)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if valid_count as usize != mention_count {
            return Err(RestError::UnprocessableEntity(
                "one or more mentioned users are not members of this group".into(),
            ));
        }

        // 9b. Batch-INSERT into message_mentions (one query per mention — no SQL concat).
        //     ON CONFLICT DO NOTHING makes it idempotent in edge cases.
        for &mentioned_user_id in &mention_ids {
            sqlx::query(
                "INSERT INTO message_mentions \
                     (message_id, mentioned_user_id, group_id) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(msg_id)
            .bind(mentioned_user_id)
            .bind(group_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
        }

        // 9c. Audit for mentions — PII-safe: count only, no user IDs.
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::MessageMentionCreated,
            principal.user_id,
            group_id,
            "message_mentions",
            msg_id.to_string(),
            json!({ "mention_count": mention_count }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;
    }

    // 10. Audit. Metadata is STRUCTURAL only — body content is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MessageSent,
        principal.user_id,
        group_id,
        "messages",
        msg_id.to_string(),
        json!({
            "body_len": trimmed_body.chars().count(),
            "has_reply_to": body.reply_to_id.is_some(),
            "mention_count": mention_count,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Notify SSE subscribers (plan 0162, GAR-670). Fire-and-forget — no
    // active subscribers means publish_chat_event is a no-op.
    state.publish_chat_event(
        chat_id,
        serde_json::json!({
            "id": msg_id,
            "chat_id": chat_id,
            "group_id": group_id,
            "sender_user_id": principal.user_id,
            "sender_label": sender_label,
            "body": trimmed_body,
            "reply_to_id": body.reply_to_id,
            "is_bot_response": false,
            "created_at": created_at,
            "mentions": mention_ids,
        }),
    );

    // 10. Bot trigger detection (plan 0240, GAR-759). Fire-and-forget.
    if let Some(stripped) = trimmed_body.strip_prefix("/garra ") {
        let prompt = stripped.trim().to_string();
        tokio::spawn(bot_reply_task(
            state.clone(),
            chat_id,
            group_id,
            principal.user_id,
            prompt,
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            id: msg_id,
            chat_id,
            group_id,
            sender_user_id: principal.user_id,
            sender_label,
            body: trimmed_body,
            reply_to_id: body.reply_to_id,
            is_bot_response: false,
            created_at,
            mentions: mention_ids,
        }),
    ))
}

/// `GET /v1/chats/{chat_id}/messages` — list messages in a chat.
///
/// Returns up to `limit` (default 50, max 100) non-deleted messages
/// ordered by `(created_at DESC, id DESC)`. Cursor-based pagination via
/// `?after=<last_message_uuid>`.
///
/// ## Error matrix
///
/// | Condition                              | Status | Source         |
/// |----------------------------------------|--------|----------------|
/// | Missing/invalid JWT                    | 401    | Principal ext. |
/// | Non-member of group                    | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched      | 400    | this handler   |
/// | `limit < 1`                            | 400    | this handler   |
/// | Chat not found in caller's group       | 404    | this handler   |
/// | Happy path                             | 200    |                |
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}/messages",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
        ("after" = Option<Uuid>, Query, description = "Cursor — last received message UUID. Omit for first page."),
        ("limit" = Option<i64>, Query, description = "Page size. Default 50, max 100."),
    ),
    responses(
        (status = 200, description = "List of messages, newest first.", body = MessageListResponse),
        (status = 400, description = "Validation error or header mismatch.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_messages(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
    Query(params): Query<ListMessagesQuery>,
) -> Result<Json<MessageListResponse>, RestError> {
    // 1. Header coherence.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // 3. Parse + clamp limit.
    let limit: i64 = match params.limit {
        None => 50,
        Some(n) if n < 1 => {
            return Err(RestError::BadRequest("limit must be at least 1".into()));
        }
        Some(n) => n.min(100),
    };

    // 4. Open transaction with RLS context.
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

    // 5. Verify chat belongs to principal.group_id.
    let chat_group: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_group.is_none() {
        return Err(RestError::NotFound);
    }

    // 6. SELECT with keyset cursor. The cursor subquery resolves the
    //    (created_at, id) pair for the `after` message — if it does not
    //    exist (already deleted or wrong group) the subquery returns NULL,
    //    making the WHERE condition `(created_at, id) < (NULL, NULL)` which
    //    is always false in Postgres → returns empty result (safe fallback).
    type MsgRow = (
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        Option<Uuid>,
        bool,
        DateTime<Utc>,
    );
    let rows: Vec<MsgRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, \
                    is_bot_response, created_at \
             FROM messages \
             WHERE chat_id = $1 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM messages \
                   WHERE id = $2 AND chat_id = $1 AND deleted_at IS NULL \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(chat_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, \
                    is_bot_response, created_at \
             FROM messages \
             WHERE chat_id = $1 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(chat_id)
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
            |(
                id,
                chat_id,
                sender_user_id,
                sender_label,
                body,
                reply_to_id,
                is_bot_response,
                created_at,
            )| {
                MessageSummary {
                    id,
                    chat_id,
                    sender_user_id,
                    sender_label,
                    body,
                    reply_to_id,
                    is_bot_response,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MessageListResponse { items, next_cursor }))
}

/// Request body for `POST /v1/messages/{message_id}/threads`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateThreadRequest {
    /// Optional display title for the thread. Must be non-empty after trim
    /// if provided. Max 500 characters.
    #[serde(default)]
    pub title: Option<String>,
}

impl CreateThreadRequest {
    /// Structural validation. Returns `Ok(())` or `Err(&'static str)`.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(ref t) = self.title {
            if t.trim().is_empty() {
                return Err("title must not be empty when provided");
            }
            if t.chars().count() > MAX_TITLE_CHARS {
                return Err("title must be 500 characters or fewer");
            }
        }
        Ok(())
    }
}

/// Response body for `POST /v1/messages/{message_id}/threads` (201 Created).
#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadResponse {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub root_message_id: Uuid,
    pub title: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// `POST /v1/messages/{message_id}/threads` — create a thread from a message.
///
/// Promotes a top-level message to a thread root by inserting a
/// `message_threads` row. The root message's `thread_id` column is **not**
/// updated — that column tracks which thread a *reply* belongs to, not the
/// root. See schema comment in migration 004 for the rationale.
///
/// ## Error matrix
///
/// | Condition                              | Status | Source         |
/// |----------------------------------------|--------|----------------|
/// | Missing/invalid JWT                    | 401    | Principal ext. |
/// | Non-member of group                    | 403    | Principal ext. |
/// | `X-Group-Id` header missing            | 400    | this handler   |
/// | title provided but empty / too long    | 400    | validate()     |
/// | Message not found in caller's group    | 404    | this handler   |
/// | Thread already exists for message      | 409    | UNIQUE DB      |
/// | Happy path                             | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/messages/{message_id}/threads",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID to promote to thread root."),
    ),
    request_body = CreateThreadRequest,
    responses(
        (status = 201, description = "Thread created.", body = ThreadResponse),
        (status = 400, description = "Validation error or header mismatch.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found or not in caller's group.", body = super::problem::ProblemDetails),
        (status = 409, description = "A thread already exists for this message.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_thread(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Json(body): Json<CreateThreadRequest>,
) -> Result<(StatusCode, Json<ThreadResponse>), RestError> {
    // 1. X-Group-Id header required.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural validation.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let title = body.title.as_deref().map(str::trim).map(String::from);

    // 4. Open transaction with RLS context.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context — both user and group required for message_threads
    //    FORCE RLS (policy message_threads_through_chats requires
    //    app.current_group_id; audit_events requires app.current_user_id).
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

    // 6. Resolve message → chat_id. 0 rows → 404 (not 403) to avoid
    //    leaking the existence of messages in other tenants.
    let msg_row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT chat_id FROM messages \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (chat_id,) = match msg_row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    // 7. INSERT message_threads. UNIQUE (root_message_id) → 23505 → 409.
    let insert_result: Result<(Uuid, DateTime<Utc>), sqlx::Error> = sqlx::query_as(
        "INSERT INTO message_threads (chat_id, root_message_id, title, created_by) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, created_at",
    )
    .bind(chat_id)
    .bind(message_id)
    .bind(&title)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await;

    let (thread_id, created_at) = match insert_result {
        Ok(r) => r,
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "a thread already exists for this message".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    };

    // 8. Audit. Metadata is STRUCTURAL only — title content is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ThreadCreated,
        principal.user_id,
        group_id,
        "message_threads",
        thread_id.to_string(),
        json!({
            "has_title": title.is_some(),
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(ThreadResponse {
            id: thread_id,
            chat_id,
            root_message_id: message_id,
            title,
            created_by: principal.user_id,
            created_at,
        }),
    ))
}

/// Request body for `PATCH /v1/messages/{message_id}`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchMessageRequest {
    /// New message text. Must be non-empty after trim. Max 100,000 characters.
    pub body: String,
}

impl PatchMessageRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.body.trim().is_empty() {
            return Err("message body must not be empty");
        }
        if self.body.chars().count() > MAX_BODY_CHARS {
            return Err("message body must be 100,000 characters or fewer");
        }
        Ok(())
    }
}

/// Response body for `PATCH /v1/messages/{message_id}` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct EditedMessageResponse {
    pub id: Uuid,
    pub body: String,
    pub edited_at: DateTime<Utc>,
    pub group_id: Uuid,
}

/// `PATCH /v1/messages/{message_id}` — edit the body of a message.
///
/// Only the original sender may edit their own message. Editing a deleted
/// message or a message sent by another user returns 404 (no existence leak).
///
/// `body_tsv` is `GENERATED ALWAYS AS … STORED` — Postgres regenerates it
/// automatically when `body` is updated; it must NOT appear in the UPDATE list.
///
/// ## Error matrix
///
/// | Condition                                      | Status | Source         |
/// |------------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                            | 401    | Principal ext. |
/// | Non-member of group                            | 403    | Principal ext. |
/// | `X-Group-Id` header missing                   | 400    | this handler   |
/// | Body empty or > 100,000 chars                  | 400    | validate()     |
/// | Message not found / deleted / wrong sender     | 404    | this handler   |
/// | Happy path                                     | 200    |                |
#[utoipa::path(
    patch,
    path = "/v1/messages/{message_id}",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
    ),
    request_body = PatchMessageRequest,
    responses(
        (status = 200, description = "Message edited.", body = EditedMessageResponse),
        (status = 400, description = "Validation error or header mismatch.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found, already deleted, or sent by a different user.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_message(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Json(body): Json<PatchMessageRequest>,
) -> Result<Json<EditedMessageResponse>, RestError> {
    // 1. X-Group-Id header required.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural validation.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let new_body = body.body.trim().to_string();

    // 4. Open transaction with RLS context.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context.
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

    // 6. UPDATE — sender-only, non-deleted.
    //    body_tsv is GENERATED ALWAYS AS — must NOT appear in the column list.
    let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "UPDATE messages \
         SET body = $1, edited_at = now() \
         WHERE id = $2 \
           AND group_id = $3 \
           AND sender_user_id = $4 \
           AND deleted_at IS NULL \
         RETURNING edited_at",
    )
    .bind(&new_body)
    .bind(message_id)
    .bind(group_id)
    .bind(principal.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (edited_at,) = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    // 7. Audit — structural metadata only; body content is PII.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MessageEdited,
        principal.user_id,
        group_id,
        "messages",
        message_id.to_string(),
        json!({ "body_len": new_body.chars().count() }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(EditedMessageResponse {
        id: message_id,
        body: new_body,
        edited_at,
        group_id,
    }))
}

/// `DELETE /v1/messages/{message_id}` — soft-delete a message.
///
/// Sets `deleted_at = now()`. Deleted messages are excluded from list
/// endpoints and full-text search. The original sender may always delete
/// their own message. Group Owners and Admins (role tier ≥ 80) may delete
/// any message in their group.
///
/// Returns 404 if the message is not found, already deleted, or the caller
/// lacks permission to delete it.
///
/// ## Error matrix
///
/// | Condition                                        | Status | Source         |
/// |--------------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                              | 401    | Principal ext. |
/// | Non-member of group                              | 403    | Principal ext. |
/// | `X-Group-Id` header missing                     | 400    | this handler   |
/// | Message not found / already deleted / no perms  | 404    | this handler   |
/// | Happy path                                       | 204    |                |
#[utoipa::path(
    delete,
    path = "/v1/messages/{message_id}",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
    ),
    responses(
        (status = 204, description = "Message deleted."),
        (status = 400, description = "Header mismatch.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found, already deleted, or insufficient permission.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_message(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    // 1. X-Group-Id header required.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Open transaction with RLS context.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 4. Tenant context.
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

    // 5. Determine if caller has admin-override permission (Owner/Admin).
    let is_admin = principal.role.map(|r| r.tier() >= 80).unwrap_or(false);

    // 6. Soft-delete. Admin/Owner may delete any message; others only their own.
    let row: Option<(Uuid,)> = if is_admin {
        sqlx::query_as(
            "UPDATE messages \
             SET deleted_at = now() \
             WHERE id = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
             RETURNING id",
        )
        .bind(message_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "UPDATE messages \
             SET deleted_at = now() \
             WHERE id = $1 \
               AND group_id = $2 \
               AND sender_user_id = $3 \
               AND deleted_at IS NULL \
             RETURNING id",
        )
        .bind(message_id)
        .bind(group_id)
        .bind(principal.user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    if row.is_none() {
        return Err(RestError::NotFound);
    }

    // 7. Audit.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MessageDeleted,
        principal.user_id,
        group_id,
        "messages",
        message_id.to_string(),
        json!({ "admin_override": is_admin }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── GET /v1/messages/{id} ────────────────────────────────────────────────────────────────────

/// Response body for `GET /v1/messages/{message_id}/threads`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadMessagesResponse {
    /// The thread anchored on this message, or `null` if no thread exists.
    pub thread: Option<ThreadResponse>,
    /// Replies in this thread, oldest-first (`created_at ASC`).
    pub messages: Vec<MessageSummary>,
    /// Cursor for the next page. `null` when all replies have been returned.
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/messages/{message_id}` — fetch a single message by ID.
///
/// Returns the full `MessageResponse` for the message if it belongs to the
/// caller's group and has not been soft-deleted. Returns 404 in all other
/// cases to avoid leaking the existence of messages in other tenants.
#[utoipa::path(
    get,
    path = "/v1/messages/{message_id}",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
    ),
    responses(
        (status = 200, description = "Message found.", body = MessageResponse),
        (status = 400, description = "Missing X-Group-Id header.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found, deleted, or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_message(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
) -> Result<Json<MessageResponse>, RestError> {
    // 1. X-Group-Id header required.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // 3. Open transaction with RLS context.
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

    // 4. Fetch message. group_id check is defense-in-depth (RLS already
    //    filters by current_group_id). 0 rows → 404 to avoid existence leaks.
    type MsgRow = (
        Uuid,
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        Option<Uuid>,
        bool,
        DateTime<Utc>,
    );
    let row: Option<MsgRow> = sqlx::query_as(
        "SELECT id, chat_id, group_id, sender_user_id, sender_label, body, reply_to_id, \
                is_bot_response, created_at \
         FROM messages \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    match row {
        None => Err(RestError::NotFound),
        Some((
            id,
            chat_id,
            grp_id,
            sender_user_id,
            sender_label,
            body,
            reply_to_id,
            is_bot_response,
            created_at,
        )) => Ok(Json(MessageResponse {
            id,
            chat_id,
            group_id: grp_id,
            sender_user_id,
            sender_label,
            body,
            reply_to_id,
            is_bot_response,
            created_at,
            mentions: vec![],
        })),
    }
}

// ─── GET /v1/messages/{id}/threads ─────────────────────────────────────────────────────────────────────────

/// Query parameters for `GET /v1/messages/{message_id}/threads`.
#[derive(Debug, Deserialize)]
pub struct ListThreadMessagesQuery {
    /// Keyset cursor — UUID of the last reply received. Returns replies
    /// newer than this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values < 1 are rejected with 400.
    pub limit: Option<i64>,
}

/// `GET /v1/messages/{message_id}/threads` — list replies in the thread.
///
/// Returns the thread metadata (or `null` if no thread exists) together with
/// the replies in oldest-first (`created_at ASC`) order. The root message
/// itself is NOT included in the `messages` array — only replies whose
/// `thread_id` matches the thread id.
///
/// If the root message exists but has no thread, returns
/// `{ thread: null, messages: [], next_cursor: null }` (not 404).
#[utoipa::path(
    get,
    path = "/v1/messages/{message_id}/threads",
    params(
        ("message_id" = Uuid, Path, description = "Root message UUID."),
        ("after" = Option<Uuid>, Query, description = "Cursor for pagination."),
        ("limit" = Option<i64>, Query, description = "Page size (default 50, max 100)."),
    ),
    responses(
        (status = 200, description = "Thread info and replies.", body = ThreadMessagesResponse),
        (status = 400, description = "Missing X-Group-Id header or bad limit.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Root message not found, deleted, or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_thread_messages(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Query(params): Query<ListThreadMessagesQuery>,
) -> Result<Json<ThreadMessagesResponse>, RestError> {
    // 1. X-Group-Id header required.
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    // 2. Capability gate.
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // 3. Parse + clamp limit.
    let limit: i64 = match params.limit {
        None => 50,
        Some(n) if n < 1 => {
            return Err(RestError::BadRequest("limit must be at least 1".into()));
        }
        Some(n) => n.min(100),
    };

    // 4. Open transaction with RLS context.
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

    // 5. Verify root message exists and belongs to caller's group.
    //    0 rows → 404 to avoid existence leaks across tenants.
    let root_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if root_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // 6. Look up thread anchored at this message (may not exist).
    type ThreadRow = (Uuid, Uuid, Uuid, Option<String>, Uuid, DateTime<Utc>);
    let thread_row: Option<ThreadRow> = sqlx::query_as(
        "SELECT id, chat_id, root_message_id, title, created_by, created_at \
         FROM message_threads \
         WHERE root_message_id = $1",
    )
    .bind(message_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let thread = thread_row.map(
        |(id, chat_id, root_message_id, title, created_by, created_at)| ThreadResponse {
            id,
            chat_id,
            root_message_id,
            title,
            created_by,
            created_at,
        },
    );

    // 7. If there is a thread, fetch its replies (cursor-paginated, ASC).
    let messages: Vec<MessageSummary> = if let Some(ref t) = thread {
        let thread_id = t.id;

        type ReplyRow = (
            Uuid,
            Uuid,
            Uuid,
            String,
            String,
            Option<Uuid>,
            bool,
            DateTime<Utc>,
        );
        let rows: Vec<ReplyRow> = if let Some(after_id) = params.after {
            sqlx::query_as(
                "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, \
                        is_bot_response, created_at \
                 FROM messages \
                 WHERE thread_id = $1 \
                   AND deleted_at IS NULL \
                   AND (created_at, id) > ( \
                       SELECT created_at, id FROM messages \
                       WHERE id = $2 AND thread_id = $1 AND deleted_at IS NULL \
                   ) \
                 ORDER BY created_at ASC, id ASC \
                 LIMIT $3",
            )
            .bind(thread_id)
            .bind(after_id)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?
        } else {
            sqlx::query_as(
                "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, \
                        is_bot_response, created_at \
                 FROM messages \
                 WHERE thread_id = $1 \
                   AND deleted_at IS NULL \
                 ORDER BY created_at ASC, id ASC \
                 LIMIT $2",
            )
            .bind(thread_id)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?
        };

        rows.into_iter()
            .map(
                |(
                    id,
                    chat_id,
                    sender_user_id,
                    sender_label,
                    body,
                    reply_to_id,
                    is_bot_response,
                    created_at,
                )| {
                    MessageSummary {
                        id,
                        chat_id,
                        sender_user_id,
                        sender_label,
                        body,
                        reply_to_id,
                        is_bot_response,
                        created_at,
                    }
                },
            )
            .collect()
    } else {
        Vec::new()
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if messages.len() as i64 == limit {
        messages.last().map(|m| m.id)
    } else {
        None
    };

    Ok(Json(ThreadMessagesResponse {
        thread,
        messages,
        next_cursor,
    }))
}

// ─── Message Attachments (plan 0182 / GAR-700) ───────────────────────────────

/// Request body for `POST /v1/messages/{message_id}/attachments`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachFileToMessageRequest {
    pub file_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct MessageAttachmentRow {
    message_id: Uuid,
    file_id: Uuid,
    attached_by: Option<Uuid>,
    attached_at: DateTime<Utc>,
    file_name: String,
    mime_type: String,
    size_bytes: i64,
}

/// Public response shape for a single message attachment.
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageAttachmentResponse {
    pub message_id: Uuid,
    pub file_id: Uuid,
    pub attached_by: Option<Uuid>,
    pub attached_at: DateTime<Utc>,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
}

impl From<MessageAttachmentRow> for MessageAttachmentResponse {
    fn from(r: MessageAttachmentRow) -> Self {
        Self {
            message_id: r.message_id,
            file_id: r.file_id,
            attached_by: r.attached_by,
            attached_at: r.attached_at,
            file_name: r.file_name,
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
        }
    }
}

/// Response envelope for `GET /v1/messages/{message_id}/attachments`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListMessageAttachmentsResponse {
    pub items: Vec<MessageAttachmentResponse>,
    pub next_cursor: Option<Uuid>,
}

/// Query params for `GET /v1/messages/{message_id}/attachments`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMessageAttachmentsQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u32>,
}

/// `POST /v1/messages/{message_id}/attachments` — attach a file to a message.
///
/// File must belong to the caller's group and must not be soft-deleted.
/// Returns 409 if already attached. Cross-group file → 404 (not 403).
#[utoipa::path(
    post,
    path = "/v1/messages/{message_id}/attachments",
    params(("message_id" = Uuid, Path, description = "Message UUID.")),
    request_body = AttachFileToMessageRequest,
    responses(
        (status = 201, description = "File attached.", body = MessageAttachmentResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message or file not found in this group.", body = super::problem::ProblemDetails),
        (status = 409, description = "File already attached to this message.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.post_message_attachment",
    skip_all,
    fields(message_id = %message_id, file_id = %body.file_id)
)]
pub async fn post_message_attachment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Json(body): Json<AttachFileToMessageRequest>,
) -> Result<(StatusCode, Json<MessageAttachmentResponse>), RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

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

    let msg_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    if msg_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let file_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM files WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
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

    let row: MessageAttachmentRow = sqlx::query_as(
        "INSERT INTO message_attachments \
             (message_id, file_id, group_id, attached_by, attached_by_label, attached_at) \
         VALUES ($1, $2, $3, $4, $5, now()) \
         RETURNING \
             message_id, file_id, attached_by, attached_at, \
             (SELECT name      FROM files WHERE id = $2) AS file_name, \
             (SELECT mime_type FROM files WHERE id = $2) AS mime_type, \
             (SELECT size_bytes FROM files WHERE id = $2) AS size_bytes",
    )
    .bind(message_id)
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
            return RestError::Conflict("file already attached to this message".into());
        }
        RestError::Internal(e.into())
    })?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MessageFileAttached,
        principal.user_id,
        group_id,
        "message_attachments",
        message_id.to_string(),
        json!({ "message_id": message_id.to_string(), "file_id": body.file_id.to_string() }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(MessageAttachmentResponse::from(row)),
    ))
}

/// `GET /v1/messages/{message_id}/attachments` — list files attached to a message.
///
/// Cursor-paginated by `(attached_at ASC, file_id ASC)`. Default 50, max 100.
#[utoipa::path(
    get,
    path = "/v1/messages/{message_id}/attachments",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
        ListMessageAttachmentsQuery,
    ),
    responses(
        (status = 200, description = "Paginated list of attachments.", body = ListMessageAttachmentsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found in this group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.list_message_attachments",
    skip_all,
    fields(message_id = %message_id)
)]
pub async fn list_message_attachments(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Query(params): Query<ListMessageAttachmentsQuery>,
) -> Result<Json<ListMessageAttachmentsResponse>, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    let limit = params.limit.unwrap_or(50).min(100) as i64;

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

    let msg_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    if msg_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let rows: Vec<MessageAttachmentRow> = if let Some(cursor) = params.cursor {
        sqlx::query_as(
            "SELECT ma.message_id, ma.file_id, ma.attached_by, ma.attached_at, \
                    f.name AS file_name, f.mime_type, f.size_bytes \
             FROM message_attachments ma \
             JOIN files f ON f.id = ma.file_id AND f.deleted_at IS NULL \
             WHERE ma.message_id = $1 \
               AND (ma.attached_at, ma.file_id) > ( \
                   SELECT attached_at, file_id FROM message_attachments \
                   WHERE message_id = $1 AND file_id = $2 \
               ) \
             ORDER BY ma.attached_at ASC, ma.file_id ASC \
             LIMIT $3",
        )
        .bind(message_id)
        .bind(cursor)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT ma.message_id, ma.file_id, ma.attached_by, ma.attached_at, \
                    f.name AS file_name, f.mime_type, f.size_bytes \
             FROM message_attachments ma \
             JOIN files f ON f.id = ma.file_id AND f.deleted_at IS NULL \
             WHERE ma.message_id = $1 \
             ORDER BY ma.attached_at ASC, ma.file_id ASC \
             LIMIT $2",
        )
        .bind(message_id)
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
        .map(MessageAttachmentResponse::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|r| r.file_id)
    } else {
        None
    };

    Ok(Json(ListMessageAttachmentsResponse { items, next_cursor }))
}

/// `DELETE /v1/messages/{message_id}/attachments/{file_id}` — detach a file.
///
/// Idempotent: returns 204 whether or not the attachment row existed.
/// Returns 404 only if the parent message does not exist in this group.
#[utoipa::path(
    delete,
    path = "/v1/messages/{message_id}/attachments/{file_id}",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
        ("file_id" = Uuid, Path, description = "File UUID to detach."),
    ),
    responses(
        (status = 204, description = "File detached (or was never attached)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Message not found in this group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.delete_message_attachment",
    skip_all,
    fields(message_id = %message_id, file_id = %file_id)
)]
pub async fn delete_message_attachment(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((message_id, file_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

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

    let msg_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    if msg_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let deleted =
        sqlx::query("DELETE FROM message_attachments WHERE message_id = $1 AND file_id = $2")
            .bind(message_id)
            .bind(file_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if deleted.rows_affected() > 0 {
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::MessageFileDetached,
            principal.user_id,
            group_id,
            "message_attachments",
            message_id.to_string(),
            json!({ "message_id": message_id.to_string(), "file_id": file_id.to_string() }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Fire-and-forget bot reply task (plan 0240, GAR-759).
///
/// Spawned by `send_message` when the message body starts with `/garra `.
/// Runs in an independent Tokio task; the 201 response returns to the
/// caller before this task completes. DB failure → error log, no panic.
///
/// V1 limitation: `sender_user_id` is the triggering user's UUID.
/// `is_bot_response = TRUE` distinguishes bot messages from human messages.
async fn bot_reply_task(
    state: RestV1FullState,
    chat_id: Uuid,
    group_id: Uuid,
    user_id: Uuid,
    prompt: String,
) {
    let response_body = if prompt.is_empty() {
        "Uso: /garra <prompt>. Exemplo: /garra me dê um resumo desta conversa.".to_string()
    } else {
        match state
            .agents
            .process_message(&chat_id.to_string(), &prompt, &[])
            .await
        {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(chat_id=%chat_id, error=%e, "bot_reply_task: AgentRuntime error");
                format!("Garra: provider não disponível ({})", e)
            }
        }
    };

    let pool = state.app_pool.pool_for_handlers();
    let result: Result<(), sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
            .bind(group_id.to_string())
            .execute(&mut *tx)
            .await?;

        let (bot_msg_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO messages \
               (chat_id, group_id, sender_user_id, sender_label, body, is_bot_response) \
             VALUES ($1, $2, $3, 'Garra', $4, TRUE) \
             RETURNING id, created_at",
        )
        .bind(chat_id)
        .bind(group_id)
        .bind(user_id)
        .bind(&response_body)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        state.publish_chat_event(
            chat_id,
            serde_json::json!({
                "id": bot_msg_id,
                "chat_id": chat_id,
                "group_id": group_id,
                "sender_user_id": user_id,
                "sender_label": "Garra",
                "body": response_body,
                "is_bot_response": true,
                "created_at": created_at,
            }),
        );

        Ok(())
    }
    .await;

    if let Err(e) = result {
        tracing::error!(chat_id=%chat_id, error=%e, "bot_reply_task: DB write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_request_valid() {
        let req = SendMessageRequest {
            body: "Hello world".into(),
            reply_to_id: None,
            mentions: vec![],
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn send_message_request_rejects_empty_body() {
        let req = SendMessageRequest {
            body: "".into(),
            reply_to_id: None,
            mentions: vec![],
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "message body must not be empty"
        );
    }

    #[test]
    fn send_message_request_rejects_whitespace_body() {
        let req = SendMessageRequest {
            body: "   \t\n  ".into(),
            reply_to_id: None,
            mentions: vec![],
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "message body must not be empty"
        );
    }

    #[test]
    fn send_message_request_rejects_body_over_100k_chars() {
        let req = SendMessageRequest {
            body: "a".repeat(MAX_BODY_CHARS + 1),
            reply_to_id: None,
            mentions: vec![],
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "message body must be 100,000 characters or fewer"
        );
    }

    #[test]
    fn send_message_request_accepts_body_at_100k_chars() {
        let req = SendMessageRequest {
            body: "a".repeat(MAX_BODY_CHARS),
            reply_to_id: None,
            mentions: vec![],
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn send_message_request_body_uses_char_count_not_byte_len() {
        // 25000 emoji = 100,000 bytes but 25,000 chars — must pass.
        let req = SendMessageRequest {
            body: "🌟".repeat(25_000),
            reply_to_id: None,
            mentions: vec![],
        };
        assert!(
            req.validate().is_ok(),
            "25_000 emoji chars must pass the chars()-based limit"
        );
    }

    // ── @mentions validation ────────────────────────────────────────────────

    #[test]
    fn send_message_request_accepts_mentions_within_limit() {
        let mentions: Vec<Uuid> = (0..MAX_MENTIONS).map(|_| Uuid::new_v4()).collect();
        let req = SendMessageRequest {
            body: "Hello team".into(),
            reply_to_id: None,
            mentions,
        };
        assert!(
            req.validate().is_ok(),
            "exactly 50 mentions must be accepted"
        );
    }

    #[test]
    fn send_message_request_rejects_mentions_over_limit() {
        let mentions: Vec<Uuid> = (0..=MAX_MENTIONS).map(|_| Uuid::new_v4()).collect();
        let req = SendMessageRequest {
            body: "Hello team".into(),
            reply_to_id: None,
            mentions,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "too many mentions: maximum is 50 per message"
        );
    }

    #[test]
    fn send_message_request_accepts_empty_mentions() {
        let req = SendMessageRequest {
            body: "No mentions here".into(),
            reply_to_id: None,
            mentions: vec![],
        };
        assert!(
            req.validate().is_ok(),
            "empty mentions list must be accepted"
        );
    }

    #[test]
    fn send_message_request_mentions_default_to_empty_when_absent() {
        let json = r#"{"body": "Hello"}"#;
        let req: SendMessageRequest = serde_json::from_str(json).unwrap();
        assert!(
            req.mentions.is_empty(),
            "absent mentions must default to empty vec"
        );
    }

    #[test]
    fn message_response_includes_mentions_field() {
        let resp = MessageResponse {
            id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            sender_user_id: Uuid::nil(),
            sender_label: "Alice".into(),
            body: "Hi".into(),
            reply_to_id: None,
            is_bot_response: false,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            mentions: vec![Uuid::nil()],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["mentions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn message_response_mentions_empty_serializes() {
        let resp = MessageResponse {
            id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            sender_user_id: Uuid::nil(),
            sender_label: "Alice".into(),
            body: "Hi".into(),
            reply_to_id: None,
            is_bot_response: false,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            mentions: vec![],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["mentions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn create_thread_request_accepts_no_title() {
        let req = CreateThreadRequest { title: None };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_thread_request_accepts_valid_title() {
        let req = CreateThreadRequest {
            title: Some("Design discussion".into()),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_thread_request_rejects_empty_title() {
        let req = CreateThreadRequest {
            title: Some("   ".into()),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "title must not be empty when provided"
        );
    }

    #[test]
    fn create_thread_request_rejects_title_over_500_chars() {
        let req = CreateThreadRequest {
            title: Some("x".repeat(MAX_TITLE_CHARS + 1)),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "title must be 500 characters or fewer"
        );
    }

    #[test]
    fn create_thread_request_accepts_title_at_500_chars() {
        let req = CreateThreadRequest {
            title: Some("x".repeat(MAX_TITLE_CHARS)),
        };
        assert!(req.validate().is_ok());
    }

    // ── Plan 0240 (GAR-759): bot trigger unit tests ───────────────────────────

    #[test]
    fn bot_trigger_detects_prefix() {
        assert!("/garra hello".starts_with("/garra "));
        assert!(!"hello".starts_with("/garra "));
        assert!(!"/garraXYZ".starts_with("/garra "));
    }

    #[test]
    fn bot_prompt_extraction() {
        let body = "/garra   hello world  ";
        let prompt = body.strip_prefix("/garra ").unwrap().trim();
        assert_eq!(prompt, "hello world");
    }

    #[test]
    fn bot_empty_prompt() {
        let body = "/garra ";
        let prompt = body.strip_prefix("/garra ").unwrap().trim();
        assert!(prompt.is_empty());
    }
}
