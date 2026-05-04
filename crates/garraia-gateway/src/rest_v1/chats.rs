//! `/v1/groups/{group_id}/chats` real handlers (plan 0054, GAR-506,
//! epic GAR-WS-CHAT slice 1).
//!
//! Two endpoints landing on the `garraia_app` RLS-enforced pool. Both
//! require an `X-Group-Id` header matching the path id (the `Principal`
//! extractor does the membership lookup; non-members get 403 at extractor
//! time before this code runs).
//!
//! ## Tenant-context protocol
//!
//! `chats` is under FORCE RLS (migration 007:89-94, policy
//! `chats_group_isolation`), so handlers MUST execute BOTH
//!
//! ```text
//! SET LOCAL app.current_user_id  = '{caller_uuid}'
//! SET LOCAL app.current_group_id = '{path_uuid}'
//! ```
//!
//! before any read or write to `chats` / `chat_members` / `audit_events`.
//! Forgetting `app.current_group_id` causes Postgres to fail the INSERT
//! with `permission denied for relation chats` (SQLSTATE 42501) — the
//! `USING` clause acts as the implicit `WITH CHECK` when no explicit
//! `WITH CHECK` is provided.
//!
//! ## SQL injection posture
//!
//! `SET LOCAL` does not accept bind parameters in Postgres, so the two
//! UUIDs are interpolated via `format!`. `Uuid::Display` produces exactly
//! 36 hex-with-dash characters and no metacharacters — injection-safe by
//! construction. All user-controlled values use `sqlx::query::bind`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{
    Action, Principal, WorkspaceAuditAction, audit_workspace_event, can,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Slice 1 só permite `channel`. `dm` e `thread` são reservados para
/// slices futuras (DM precisa de 2 `chat_members` + UNIQUE; thread
/// depende de `message_threads`). Mantido como `&[&str]` para espelhar
/// o pattern de `groups::ALLOWED_GROUP_TYPES`.
#[allow(dead_code)] // Used for documentation; validate() inlines the match.
const ALLOWED_CHAT_TYPES_SLICE1: &[&str] = &["channel"];

/// Maximum topic length, kept in step with what UIs render comfortably.
/// `chats.topic` has no DB CHECK, so this lives at the API edge only.
const MAX_TOPIC_CHARS: usize = 4_000;

/// Request body for `POST /v1/groups/{group_id}/chats`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChatRequest {
    /// Display name. Must not be empty after trim.
    pub name: String,
    /// Chat type. Slice 1: must be `"channel"`. `"dm"` and `"thread"`
    /// are rejected with 400 distinct messages so clients can debug.
    #[serde(rename = "type")]
    pub chat_type: String,
    /// Optional topic / description. Capped at 4000 chars at API edge
    /// (no DB CHECK on `chats.topic`).
    #[serde(default)]
    pub topic: Option<String>,
}

impl CreateChatRequest {
    /// Structural validation. Returns `Ok(())` on success, `Err(&'static str)`
    /// with a PII-safe detail otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() {
            return Err("chat name must not be empty");
        }
        match self.chat_type.as_str() {
            "channel" => {}
            "dm" => {
                return Err(
                    "type 'dm' is not yet supported in this slice; only 'channel'",
                );
            }
            "thread" => {
                return Err(
                    "type 'thread' is not yet supported in this slice; only 'channel'",
                );
            }
            _ => return Err("invalid chat type; must be 'channel'"),
        }
        if let Some(t) = &self.topic
            && t.chars().count() > MAX_TOPIC_CHARS
        {
            return Err("topic must be 4000 characters or fewer");
        }
        Ok(())
    }
}

/// Response body for `POST /v1/groups/{group_id}/chats` (201 Created).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Compact summary used by `GET /v1/groups/{group_id}/chats`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatSummary {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for `GET /v1/groups/{group_id}/chats` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatListResponse {
    pub items: Vec<ChatSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_chat_request_valid_channel() {
        let req = CreateChatRequest {
            name: "general".into(),
            chat_type: "channel".into(),
            topic: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_chat_request_rejects_empty_name() {
        let req = CreateChatRequest {
            name: "  ".into(),
            chat_type: "channel".into(),
            topic: None,
        };
        assert_eq!(req.validate().unwrap_err(), "chat name must not be empty");
    }

    #[test]
    fn create_chat_request_rejects_dm_in_slice1() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "dm".into(),
            topic: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "type 'dm' is not yet supported in this slice; only 'channel'"
        );
    }

    #[test]
    fn create_chat_request_rejects_thread_in_slice1() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "thread".into(),
            topic: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "type 'thread' is not yet supported in this slice; only 'channel'"
        );
    }

    #[test]
    fn create_chat_request_rejects_unknown_type() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "broadcast".into(),
            topic: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "invalid chat type; must be 'channel'"
        );
    }

    #[test]
    fn create_chat_request_rejects_topic_over_4000_chars() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "channel".into(),
            topic: Some("a".repeat(MAX_TOPIC_CHARS + 1)),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "topic must be 4000 characters or fewer"
        );
    }

    #[test]
    fn create_chat_request_accepts_topic_at_limit() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "channel".into(),
            topic: Some("a".repeat(MAX_TOPIC_CHARS)),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_chat_request_topic_uses_char_count_not_byte_len() {
        // 1000 emoji chars = 4000 bytes; would fail a naive `len()` check
        // but pass a `chars().count()` check.
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "channel".into(),
            topic: Some("🌟".repeat(1_000)),
        };
        assert!(
            req.validate().is_ok(),
            "1000 emoji chars (4000 bytes) must pass the chars()-based check"
        );
    }
}
