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
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Maximum message body length (chars, mirrors DB CHECK).
const MAX_BODY_CHARS: usize = 100_000;

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
    pub created_at: DateTime<Utc>,
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
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!("SET LOCAL app.current_group_id = '{group_id}'"))
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
    let (sender_label,): (String,) = sqlx::query_as(
        "SELECT display_name FROM users WHERE id = $1",
    )
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

    // 9. Audit. Metadata is STRUCTURAL only — body content is PII.
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
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

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
            created_at,
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

    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!("SET LOCAL app.current_group_id = '{group_id}'"))
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
    type MsgRow = (Uuid, Uuid, Uuid, String, String, Option<Uuid>, DateTime<Utc>);
    let rows: Vec<MsgRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, created_at \
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
            "SELECT id, chat_id, sender_user_id, sender_label, body, reply_to_id, created_at \
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
            |(id, chat_id, sender_user_id, sender_label, body, reply_to_id, created_at)| {
                MessageSummary {
                    id,
                    chat_id,
                    sender_user_id,
                    sender_label,
                    body,
                    reply_to_id,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MessageListResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_request_valid() {
        let req = SendMessageRequest {
            body: "Hello world".into(),
            reply_to_id: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn send_message_request_rejects_empty_body() {
        let req = SendMessageRequest {
            body: "".into(),
            reply_to_id: None,
        };
        assert_eq!(req.validate().unwrap_err(), "message body must not be empty");
    }

    #[test]
    fn send_message_request_rejects_whitespace_body() {
        let req = SendMessageRequest {
            body: "   \t\n  ".into(),
            reply_to_id: None,
        };
        assert_eq!(req.validate().unwrap_err(), "message body must not be empty");
    }

    #[test]
    fn send_message_request_rejects_body_over_100k_chars() {
        let req = SendMessageRequest {
            body: "a".repeat(MAX_BODY_CHARS + 1),
            reply_to_id: None,
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
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn send_message_request_body_uses_char_count_not_byte_len() {
        // 25000 emoji = 100,000 bytes but 25,000 chars — must pass.
        let req = SendMessageRequest {
            body: "🌟".repeat(25_000),
            reply_to_id: None,
        };
        assert!(
            req.validate().is_ok(),
            "25_000 emoji chars must pass the chars()-based limit"
        );
    }
}
