//! `GET /v1/me/chats` — inbox of chats where the caller is a member.

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
        (status = 400, description = "Validation error (invalid type value).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
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
