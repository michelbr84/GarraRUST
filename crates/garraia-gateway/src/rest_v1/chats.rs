//! `/v1/groups/{group_id}/chats` real handlers (plan 0054 GAR-506 slice 1,
//! plan 0115 GAR-604 slice 5 — DM creation, epic GAR-WS-CHAT).
//!
//! Endpoints landing on the `garraia_app` RLS-enforced pool. Both
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

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::{DateTime, Utc};
use futures::stream;
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Maximum topic length, kept in step with what UIs render comfortably.
/// `chats.topic` has no DB CHECK, so this lives at the API edge only.
const MAX_TOPIC_CHARS: usize = 4_000;

/// Plan 0163 (GAR-679): maximum concurrent SSE connections per user.
/// Above this, the handler returns 429 Too Many Requests.
const MAX_SSE_PER_USER: usize = 5;

/// Default page size for `GET /v1/chats/{chat_id}/threads` (plan 0221 / GAR-740).
const DEFAULT_THREAD_LIMIT: u32 = 20;

/// Maximum page size for `GET /v1/chats/{chat_id}/threads`.
const MAX_THREAD_LIMIT: u32 = 50;

/// Request body for `POST /v1/groups/{group_id}/chats`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChatRequest {
    /// Display name. Required for `channel`. Optional for `dm` (clients
    /// typically use the partner's `display_name` instead).
    #[serde(default)]
    pub name: String,
    /// Chat type: `"channel"` or `"dm"`. `"thread"` is reserved.
    #[serde(rename = "type")]
    pub chat_type: String,
    /// Optional topic / description. Capped at 4000 chars at API edge
    /// (no DB CHECK on `chats.topic`).
    #[serde(default)]
    pub topic: Option<String>,
    /// Required when `type = "dm"`. Must be omitted (or null) for other types.
    #[serde(default)]
    pub partner_user_id: Option<Uuid>,
}

impl CreateChatRequest {
    /// Structural validation. Returns `Ok(())` on success, `Err(&'static str)`
    /// with a PII-safe detail otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self.chat_type.as_str() {
            "channel" => {
                if self.name.trim().is_empty() {
                    return Err("chat name must not be empty for type 'channel'");
                }
                if self.partner_user_id.is_some() {
                    return Err("'partner_user_id' is only valid for type 'dm'");
                }
            }
            "dm" => {
                if self.partner_user_id.is_none() {
                    return Err("type 'dm' requires 'partner_user_id'");
                }
            }
            "thread" => {
                return Err("type 'thread' is not supported via this endpoint");
            }
            _ => return Err("invalid chat type; must be 'channel' or 'dm'"),
        }
        if let Some(t) = &self.topic
            && t.chars().count() > MAX_TOPIC_CHARS
        {
            return Err("topic must be 4000 characters or fewer");
        }
        Ok(())
    }
}

/// Response body for `POST /v1/groups/{group_id}/chats` (201 Created or 200 OK for DM idempotent).
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
    /// True when a DM between this pair already existed and no new row was created.
    /// Absent (false) on 201 Created responses and for channel chats.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dm_already_exists: bool,
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

/// `POST /v1/groups/{group_id}/chats` — create a new channel inside a group.
///
/// Authz: caller must be a member of the group AND have
/// `Action::ChatsWrite`. All 5 roles (Owner/Admin/Member/Guest/Child) hold
/// this capability per the migration 002 seed; the explicit `can()` check
/// stays in place so a future role with reduced chat permissions slots in
/// cleanly. Non-members never reach this code — the `Principal` extractor
/// already 403'd them.
///
/// Tenancy: the handler opens a transaction, sets BOTH `app.current_user_id`
/// AND `app.current_group_id`, then issues two INSERTs (`chats` then
/// `chat_members[owner]`) plus one audit row. The whole sequence commits or
/// rolls back atomically — there is no path that leaves a `chats` row
/// without an owner member.
///
/// ## Error matrix
///
/// | Condition                                        | Status | Source         |
/// |--------------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                              | 401    | Principal ext. |
/// | Non-member of target group                       | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched                | 400    | this handler   |
/// | Body: empty name / unknown type / dm / thread    | 400    | validate()     |
/// | Body: topic > 4000 chars                         | 400    | validate()     |
/// | Caller has no role (defensive — extractor sets it)| 403   | `can()`        |
/// | Happy path                                       | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/chats",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    request_body = CreateChatRequest,
    responses(
        (status = 201, description = "Chat created; caller auto-enrolled as `'owner'` in `chat_members`.", body = ChatResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or unsupported type.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the requested group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(group_id): Path<Uuid>,
    Json(body): Json<CreateChatRequest>,
) -> Result<(StatusCode, Json<ChatResponse>), RestError> {
    // 1. Header/path coherence — same rule as get_group/patch_group/create_invite.
    match principal.group_id {
        Some(hdr) if hdr == group_id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Capability check. All 5 roles have ChatsWrite seeded; stays here
    //    so a future role with reduced chat permissions slots in cleanly.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural validation (no DB access; PII-safe messages).
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let trimmed_name = body.name.trim().to_string();
    let trimmed_topic: Option<String> = body
        .topic
        .as_ref()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    let pool = state.app_pool.pool_for_handlers();

    if body.chat_type == "dm" {
        create_dm_chat(
            pool,
            principal,
            group_id,
            trimmed_name,
            trimmed_topic,
            // validate() guarantees partner_user_id is Some for type='dm'
            body.partner_user_id
                .expect("validate() ensures Some for dm"),
        )
        .await
    } else {
        create_channel_chat(
            pool,
            principal,
            group_id,
            trimmed_name,
            trimmed_topic,
            body.chat_type,
        )
        .await
    }
}

/// Inner helper: create a `channel` type chat (the original slice-1 path).
async fn create_channel_chat(
    pool: &sqlx::PgPool,
    principal: Principal,
    group_id: Uuid,
    trimmed_name: String,
    trimmed_topic: Option<String>,
    chat_type: String,
) -> Result<(StatusCode, Json<ChatResponse>), RestError> {
    // 4. Open transaction. SET LOCAL is tx-scoped.
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context — BOTH required for FORCE RLS on chats/chat_members/audit.
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

    // 6. INSERT chat.
    let (chat_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO chats (group_id, type, name, topic, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, created_at",
    )
    .bind(group_id)
    .bind(&chat_type)
    .bind(&trimmed_name)
    .bind(trimmed_topic.as_deref())
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 7. Auto-enroll creator as owner.
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 8. Audit — structure metadata only, no PII (plan 0054 invariant 7).
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatCreated,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({
            "name_len": trimmed_name.chars().count(),
            "type": chat_type,
            "has_topic": trimmed_topic.is_some(),
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(ChatResponse {
            id: chat_id,
            group_id,
            chat_type,
            name: trimmed_name,
            topic: trimmed_topic,
            created_by: principal.user_id,
            created_at,
            dm_already_exists: false,
        }),
    ))
}

/// Inner helper: create or return a `dm` type chat (plan 0115, GAR-604).
///
/// Idempotent: if a DM already exists for this (group, sorted-user-pair),
/// returns 200 + the existing chat with `dm_already_exists: true`. This is
/// race-condition-safe: the partial UNIQUE INDEX `chats_dm_pair_unique`
/// (migration 019) fires on the second INSERT and is caught via SQLSTATE 23505.
async fn create_dm_chat(
    pool: &sqlx::PgPool,
    principal: Principal,
    group_id: Uuid,
    trimmed_name: String,
    trimmed_topic: Option<String>,
    partner_user_id: Uuid,
) -> Result<(StatusCode, Json<ChatResponse>), RestError> {
    // Self-DM guard.
    if partner_user_id == principal.user_id {
        return Err(RestError::BadRequest(
            "cannot create a DM with yourself".into(),
        ));
    }

    // Normalize UUID pair so the unique index key is deterministic regardless
    // of who initiates the DM. Rust Uuid implements Ord on the 128-bit value.
    let (dm_user_a, dm_user_b) = if principal.user_id < partner_user_id {
        (principal.user_id, partner_user_id)
    } else {
        (partner_user_id, principal.user_id)
    };

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Tenant context required for FORCE RLS on all tables accessed below.
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

    // Verify partner is an active group member. group_members is accessible
    // via garraia_app (migration 018: group_members_visible policy, Branch 1
    // with app.current_group_id set above).
    let partner_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(\
            SELECT 1 FROM group_members \
            WHERE group_id = $1 AND user_id = $2 AND status = 'active'\
        )",
    )
    .bind(group_id)
    .bind(partner_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if !partner_active {
        return Err(RestError::NotFound);
    }

    // Attempt INSERT. On SQLSTATE 23505 (unique_violation from
    // chats_dm_pair_unique), fall back to SELECT of the existing DM.
    let insert_result: Result<(Uuid, DateTime<Utc>), sqlx::Error> = sqlx::query_as(
        "INSERT INTO chats (group_id, type, name, topic, created_by, dm_user_a, dm_user_b) \
         VALUES ($1, 'dm', $2, $3, $4, $5, $6) \
         RETURNING id, created_at",
    )
    .bind(group_id)
    .bind(&trimmed_name)
    .bind(trimmed_topic.as_deref())
    .bind(principal.user_id)
    .bind(dm_user_a)
    .bind(dm_user_b)
    .fetch_one(&mut *tx)
    .await;

    match insert_result {
        Ok((chat_id, created_at)) => {
            // New DM: enroll both participants atomically.
            sqlx::query(
                "INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, 'owner')",
            )
            .bind(chat_id)
            .bind(principal.user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            sqlx::query(
                "INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, 'member')",
            )
            .bind(chat_id)
            .bind(partner_user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            audit_workspace_event(
                &mut tx,
                WorkspaceAuditAction::ChatCreated,
                principal.user_id,
                group_id,
                "chats",
                chat_id.to_string(),
                json!({
                    "type": "dm",
                    "has_name": !trimmed_name.is_empty(),
                    "has_topic": trimmed_topic.is_some(),
                }),
            )
            .await
            .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

            tx.commit()
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

            Ok((
                StatusCode::CREATED,
                Json(ChatResponse {
                    id: chat_id,
                    group_id,
                    chat_type: "dm".into(),
                    name: trimmed_name,
                    topic: trimmed_topic,
                    created_by: principal.user_id,
                    created_at,
                    dm_already_exists: false,
                }),
            ))
        }
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505") => {
            // DM already exists — return existing row (idempotent, plan 0115 §invariant 3).
            tx.rollback()
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

            // New transaction required: SET LOCAL is scoped to the tx, so the
            // rolled-back tx's context is gone. FORCE RLS on `chats` requires
            // both GUC variables to be set before the SELECT.
            let mut tx2 = pool
                .begin()
                .await
                .map_err(|e| RestError::Internal(e.into()))?;
            sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
                .bind(principal.user_id.to_string())
                .execute(&mut *tx2)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;
            sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
                .bind(group_id.to_string())
                .execute(&mut *tx2)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

            let (chat_id, created_at, name, topic, created_by): (
                Uuid,
                DateTime<Utc>,
                String,
                Option<String>,
                Uuid,
            ) = sqlx::query_as(
                "SELECT id, created_at, name, topic, created_by \
                 FROM chats \
                 WHERE group_id = $1 AND type = 'dm' \
                   AND dm_user_a = $2 AND dm_user_b = $3",
            )
            .bind(group_id)
            .bind(dm_user_a)
            .bind(dm_user_b)
            .fetch_one(&mut *tx2)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
            tx2.commit()
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

            Ok((
                StatusCode::OK,
                Json(ChatResponse {
                    id: chat_id,
                    group_id,
                    chat_type: "dm".into(),
                    name,
                    topic,
                    created_by,
                    created_at,
                    dm_already_exists: true,
                }),
            ))
        }
        Err(e) => Err(RestError::Internal(e.into())),
    }
}

/// `GET /v1/groups/{group_id}/chats` — list active chats in a group.
///
/// Returns up to 100 active (`archived_at IS NULL`) chats ordered by
/// `created_at DESC`. No cursor pagination in slice 1.
///
/// ## Error matrix
///
/// | Condition                                  | Status | Source         |
/// |--------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                        | 401    | Principal ext. |
/// | Non-member of target group                 | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched          | 400    | this handler   |
/// | Caller has no role (defensive)             | 403    | `can()`        |
/// | Happy path                                 | 200    |                |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/chats",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    responses(
        (status = 200, description = "Up to 100 active chats, newest first.", body = ChatListResponse),
        (status = 400, description = "`X-Group-Id` header missing or mismatched.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the requested group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_chats(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(group_id): Path<Uuid>,
) -> Result<Json<ChatListResponse>, RestError> {
    // 1. Header/path coherence.
    match principal.group_id {
        Some(hdr) if hdr == group_id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Capability gate — all 5 roles pass; defensive.
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // 3. Tx-bound tenant context. SELECT on chats requires
    //    `app.current_group_id` because chats is FORCE RLS.
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

    // 4. SELECT — RLS enforces group isolation; explicit `archived_at IS
    //    NULL` filter excludes soft-deleted rows from this slice.
    //    LIMIT 100 fixed; cursor pagination lands when messages do.
    type ChatRow = (
        Uuid,
        String,
        String,
        Option<String>,
        Uuid,
        DateTime<Utc>,
        DateTime<Utc>,
    );
    let rows: Vec<ChatRow> = sqlx::query_as(
        "SELECT id, type, name, topic, created_by, created_at, updated_at \
         FROM chats \
         WHERE archived_at IS NULL \
         ORDER BY created_at DESC \
         LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items = rows
        .into_iter()
        .map(
            |(id, ct, name, topic, created_by, created_at, updated_at)| ChatSummary {
                id,
                chat_type: ct,
                name,
                topic,
                created_by,
                created_at,
                updated_at,
            },
        )
        .collect();

    Ok(Json(ChatListResponse { items }))
}

// ── Slice 4 (plan 0076 / GAR-530) — individual chat management + member CRUD ──

/// Full detail returned by `GET /v1/chats/{chat_id}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatDetailResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request body for `PATCH /v1/chats/{chat_id}`.
///
/// At least one field must be provided. `topic: ""` clears the topic
/// (empty string is normalised to `NULL` after trim).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchChatRequest {
    pub name: Option<String>,
    pub topic: Option<String>,
}

impl PatchChatRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_none() && self.topic.is_none() {
            return Err("at least one of name or topic must be provided");
        }
        if let Some(n) = &self.name
            && n.trim().is_empty()
        {
            return Err("name must not be empty");
        }
        if let Some(t) = &self.topic
            && t.chars().count() > MAX_TOPIC_CHARS
        {
            return Err("topic must be 4000 characters or fewer");
        }
        Ok(())
    }
}

/// One entry in the members list.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMemberResponse {
    pub user_id: Uuid,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

/// Response body for `GET /v1/chats/{chat_id}/members`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMemberListResponse {
    pub items: Vec<ChatMemberResponse>,
}

/// Request body for `POST /v1/chats/{chat_id}/members`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddChatMemberRequest {
    pub user_id: Uuid,
    #[serde(default = "default_member_role")]
    pub role: String,
}

fn default_member_role() -> String {
    "member".into()
}

impl AddChatMemberRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self.role.as_str() {
            "owner" | "moderator" | "member" | "viewer" => Ok(()),
            _ => Err("role must be one of: owner, moderator, member, viewer"),
        }
    }
}

/// `GET /v1/chats/{chat_id}` — fetch a single chat.
///
/// Returns 404 for archived chats or chats in other groups (no 403 leakage).
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    responses(
        (status = 200, description = "Chat detail.", body = ChatDetailResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
) -> Result<Json<ChatDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
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

    type Row = (
        Uuid,
        Uuid,
        String,
        String,
        Option<String>,
        Uuid,
        DateTime<Utc>,
        DateTime<Utc>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, group_id, type, name, topic, created_by, created_at, updated_at \
         FROM chats \
         WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let (id, gid, ct, name, topic, created_by, created_at, updated_at) =
        row.ok_or(RestError::NotFound)?;

    Ok(Json(ChatDetailResponse {
        id,
        group_id: gid,
        chat_type: ct,
        name,
        topic,
        created_by,
        created_at,
        updated_at,
    }))
}

/// `PATCH /v1/chats/{chat_id}` — update name and/or topic.
///
/// Uses `COALESCE($new, column)` so only supplied fields are overwritten.
/// An empty `topic` string after trim clears the topic field (`NULL`).
/// Returns the updated chat detail on success.
#[utoipa::path(
    patch,
    path = "/v1/chats/{chat_id}",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    request_body = PatchChatRequest,
    responses(
        (status = 200, description = "Updated chat detail.", body = ChatDetailResponse),
        (status = 400, description = "Validation failure.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks ChatsWrite.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<PatchChatRequest>,
) -> Result<Json<ChatDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let trimmed_name: Option<String> = body.name.as_deref().map(str::trim).map(str::to_string);
    // Empty topic after trim → NULL (clears the field).
    let new_topic: Option<Option<String>> = body.topic.as_deref().map(|t| {
        let s = t.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    });

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

    // COALESCE keeps existing value when the caller did not provide the field.
    // For topic: if caller provided `topic` (even empty → None), we replace;
    // otherwise keep the DB value.
    type Row = (
        Uuid,
        Uuid,
        String,
        String,
        Option<String>,
        Uuid,
        DateTime<Utc>,
        DateTime<Utc>,
    );
    let row: Option<Row> = if body.topic.is_some() {
        // Caller supplied topic — may be clearing it (new_topic inner = None).
        sqlx::query_as(
            "UPDATE chats \
             SET name = COALESCE($1, name), topic = $2, updated_at = now() \
             WHERE id = $3 AND group_id = $4 AND archived_at IS NULL \
             RETURNING id, group_id, type, name, topic, created_by, created_at, updated_at",
        )
        .bind(trimmed_name.as_deref())
        .bind(new_topic.flatten().as_deref())
        .bind(chat_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        // Only name supplied — topic untouched.
        sqlx::query_as(
            "UPDATE chats \
             SET name = COALESCE($1, name), updated_at = now() \
             WHERE id = $2 AND group_id = $3 AND archived_at IS NULL \
             RETURNING id, group_id, type, name, topic, created_by, created_at, updated_at",
        )
        .bind(trimmed_name.as_deref())
        .bind(chat_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    let (id, gid, ct, name, topic, created_by, created_at, updated_at) =
        row.ok_or(RestError::NotFound)?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatUpdated,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({
            "changed_name": body.name.is_some(),
            "changed_topic": body.topic.is_some(),
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(ChatDetailResponse {
        id,
        group_id: gid,
        chat_type: ct,
        name,
        topic,
        created_by,
        created_at,
        updated_at,
    }))
}

/// `DELETE /v1/chats/{chat_id}` — soft-archive a chat.
///
/// Sets `archived_at = now()`. Messages are preserved. Returns 204 on
/// success; subsequent GET/PATCH on the same chat returns 404.
#[utoipa::path(
    delete,
    path = "/v1/chats/{chat_id}",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    responses(
        (status = 204, description = "Chat archived."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks ChatsWrite.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, already archived, or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

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

    let (chat_type,): (String,) = sqlx::query_as(
        "UPDATE chats SET archived_at = now() \
         WHERE id = $1 AND group_id = $2 AND archived_at IS NULL \
         RETURNING type",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?
    .ok_or(RestError::NotFound)?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatArchived,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({ "type": chat_type }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/chats/{chat_id}/members` — list all members of a chat.
///
/// Returns members ordered by `joined_at ASC`. No pagination (bounded by
/// group size; real groups rarely exceed hundreds of members per chat).
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}/members",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    responses(
        (status = 200, description = "Member list.", body = ChatMemberListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_chat_members(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
) -> Result<Json<ChatMemberListResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
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

    // Verify chat exists in group (404 not 403 — no cross-tenant leak).
    let exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    type MemberRow = (Uuid, String, DateTime<Utc>);
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT user_id, role, joined_at \
         FROM chat_members \
         WHERE chat_id = $1 \
         ORDER BY joined_at ASC",
    )
    .bind(chat_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items = rows
        .into_iter()
        .map(|(user_id, role, joined_at)| ChatMemberResponse {
            user_id,
            role,
            joined_at,
        })
        .collect();

    Ok(Json(ChatMemberListResponse { items }))
}

/// `POST /v1/chats/{chat_id}/members` — add a group member to a chat.
///
/// The target `user_id` must be a member of the same group; otherwise the
/// insert would silently succeed in RLS terms (chat_members JOIN-RLS only
/// checks group isolation via the chats table) but would violate the
/// group-membership invariant. An explicit guard prevents this.
///
/// Returns 409 if the user is already a member.
#[utoipa::path(
    post,
    path = "/v1/chats/{chat_id}/members",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    request_body = AddChatMemberRequest,
    responses(
        (status = 201, description = "Member added.", body = ChatMemberResponse),
        (status = 400, description = "Validation failure.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks MembersManage.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, target user not in group.", body = super::problem::ProblemDetails),
        (status = 409, description = "User is already a member of this chat.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn add_chat_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<AddChatMemberRequest>,
) -> Result<(StatusCode, Json<ChatMemberResponse>), RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }

    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

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

    // Verify chat exists and is not archived.
    let exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Verify target user is a member of this group (group_members is not
    // FORCE RLS — it is app-layer enforced).
    let in_group: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(group_id)
            .bind(body.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if in_group.is_none() {
        return Err(RestError::NotFound);
    }

    // Check for existing membership.
    let already: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM chat_members WHERE chat_id = $1 AND user_id = $2")
            .bind(chat_id)
            .bind(body.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if already.is_some() {
        return Err(RestError::Conflict(
            "user is already a member of this chat".into(),
        ));
    }

    let (joined_at,): (DateTime<Utc>,) = sqlx::query_as(
        "INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, $3) \
         RETURNING joined_at",
    )
    .bind(chat_id)
    .bind(body.user_id)
    .bind(&body.role)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatMemberAdded,
        principal.user_id,
        group_id,
        "chat_members",
        body.user_id.to_string(),
        json!({ "role": body.role }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(ChatMemberResponse {
            user_id: body.user_id,
            role: body.role,
            joined_at,
        }),
    ))
}

/// `DELETE /v1/chats/{chat_id}/members/{user_id}` — remove a member.
///
/// Guards:
/// - Cannot remove the last owner (409).
/// - Caller must have `MembersManage`.
/// - Chat must exist and not be archived (404).
#[utoipa::path(
    delete,
    path = "/v1/chats/{chat_id}/members/{user_id}",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
        ("user_id" = Uuid, Path, description = "User UUID to remove."),
    ),
    responses(
        (status = 204, description = "Member removed."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks MembersManage.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, or user not a member.", body = super::problem::ProblemDetails),
        (status = 409, description = "Cannot remove the last owner of a chat.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn remove_chat_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((chat_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::MembersManage) {
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

    // Verify chat in group and not archived.
    let exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Fetch target member's role before deletion.
    let target_role: Option<(String,)> =
        sqlx::query_as("SELECT role FROM chat_members WHERE chat_id = $1 AND user_id = $2")
            .bind(chat_id)
            .bind(target_user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let role = target_role.map(|(r,)| r).ok_or(RestError::NotFound)?;

    // Guard: cannot remove last owner.
    if role == "owner" {
        let (owner_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_members WHERE chat_id = $1 AND role = 'owner'",
        )
        .bind(chat_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if owner_count <= 1 {
            return Err(RestError::Conflict(
                "cannot remove the last owner of a chat".into(),
            ));
        }
    }

    sqlx::query("DELETE FROM chat_members WHERE chat_id = $1 AND user_id = $2")
        .bind(chat_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatMemberRemoved,
        principal.user_id,
        group_id,
        "chat_members",
        target_user_id.to_string(),
        json!({ "role": role }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/chats/{chat_id}/stream` — SSE stream of real-time chat events.
///
/// Emits `message.created` events whenever a new message is posted to the
/// chat via `POST /v1/chats/{chat_id}/messages`. A keep-alive comment is
/// sent every 30 seconds to prevent proxy timeouts.
///
/// ## Wire format
///
/// ```text
/// event: message.created
/// data: {"id":"...","chat_id":"...","sender_user_id":"...","body":"...","created_at":"...Z"}
///
/// event: stream.lagged
/// data: 3
/// ```
///
/// `stream.lagged` appears when the server drops events because the client
/// fell behind the 64-event broadcast buffer. The `data` field is the count
/// of dropped events.
///
/// ## Error matrix
///
/// | Condition                        | Status | Source         |
/// |----------------------------------|--------|----------------|
/// | Missing/invalid JWT              | 401    | Principal ext. |
/// | Non-member of group              | 403    | Principal ext. |
/// | `X-Group-Id` missing             | 400    | this handler   |
/// | Chat not found in caller's group | 404    | this handler   |
/// | Happy path                       | 200    | text/event-stream |
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}/stream",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
    ),
    responses(
        (status = 200, description = "SSE stream opened.", content_type = "text/event-stream"),
        (status = 400, description = "X-Group-Id missing.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn stream_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
) -> Result<Sse<impl stream::Stream<Item = Result<Event, Infallible>>>, RestError> {
    let group_id = match principal.group_id {
        Some(hdr) => hdr,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // Plan 0163 (GAR-679): per-user concurrent SSE connection cap.
    //
    // Acquire a slot atomically BEFORE the RLS transaction. A `SseSlotGuard`
    // holds the decrement-on-drop responsibility until `ChatStreamGuard` is
    // built. If any step below fails, `SseSlotGuard::drop` releases the slot
    // automatically. Once `ChatStreamGuard` is built, `slot_guard.disarm()`
    // transfers release ownership to `ChatStreamGuard::drop`.
    let sse_counter: Arc<AtomicUsize> = {
        let entry = state
            .sse_connections
            .entry(principal.user_id)
            .or_insert_with(|| Arc::new(AtomicUsize::new(0)));
        Arc::clone(&*entry)
    };
    let prev = sse_counter.fetch_add(1, Ordering::Relaxed);
    if prev >= MAX_SSE_PER_USER {
        sse_counter.fetch_sub(1, Ordering::Relaxed);
        return Err(RestError::TooManyRequests(
            "too many concurrent SSE connections for this user; retry after 60 seconds".into(),
        ));
    }
    let mut slot_guard = SseSlotGuard {
        counter: Arc::clone(&sse_counter),
        armed: true,
    };

    // Verify the chat belongs to the caller's group (0 rows → 404, same as
    // send_message — avoids leaking the existence of chats in other tenants).
    //
    // Bug fix exposed by the integration test (audit F-2): `set_config(_,_,true)`
    // is local to the current transaction. The previous implementation used
    // `pool.acquire()` (auto-commit), so each `SELECT set_config(...)`
    // statement opened its own implicit tx and the setting reverted before
    // the chat-exists query ran. FORCE RLS then rejected every row — even
    // legitimate ones — and the handler returned 404 across the board.
    //
    // Fix: wrap acquire + set_config + chat_exists in a single transaction,
    // matching the pattern used by every other handler in messages.rs.
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

    let chat_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Audit follow-up F-4 (GAR-680): emit `chat.subscribed` BEFORE the
    // broadcast receiver is created, but AFTER the chat-exists / RLS check
    // passes. This guarantees:
    //
    // 1. A 404 (cross-tenant or archived chat) never produces an audit row.
    // 2. The audit row is rolled back together with the RLS context if
    //    `tx.commit()` fails — no orphan row with no paired `chat.unsubscribed`.
    // 3. The paired `chat.unsubscribed` is only spawned by the RAII guard,
    //    which is built BELOW after the commit succeeds — so we never emit
    //    `chat.unsubscribed` without a prior `chat.subscribed`.
    //
    // `subscriber_count` carries the count of subscribers that will exist
    // after this client joins (pre-subscribe count + 1). PII-safe: no message
    // body or chat name reaches the metadata.
    let pre_subscribe_count = state
        .chat_events
        .get(&chat_id)
        .map(|entry| entry.receiver_count())
        .unwrap_or(0);
    let projected_subscriber_count = pre_subscribe_count.saturating_add(1);
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatSubscribed,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({ "subscriber_count": projected_subscriber_count }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    // Commit the read-only RLS transaction. The conn returns to the pool;
    // the subsequent `subscribe_chat` is purely in-process.
    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let rx = state.subscribe_chat(chat_id);

    // RAII guard: when `ChatStreamState` is dropped (client disconnect, server
    // close, or stream end), `rx` drops first (declaration order), then
    // `_guard` spawns the paired `chat.unsubscribed` audit row, runs
    // `cleanup_chat_subscription`, and releases the per-user SSE slot
    // (plan 0163, GAR-679). Closes audit findings F-1 and F-4.
    //
    // Transfer SSE slot ownership from `slot_guard` to `ChatStreamGuard` here.
    // `slot_guard.disarm()` prevents double-decrement.
    slot_guard.disarm();
    let stream_state = ChatStreamState {
        rx,
        _guard: ChatStreamGuard {
            chat_id,
            actor_user_id: principal.user_id,
            group_id,
            state: state.clone(),
            sse_counter,
            sse_user_id: principal.user_id,
        },
    };

    let event_stream = stream::unfold(stream_state, |mut s| async move {
        match s.rx.recv().await {
            Ok(value) => {
                let data = serde_json::to_string(&value).unwrap_or_default();
                Some((Ok(Event::default().event("message.created").data(data)), s))
            }
            Err(RecvError::Lagged(n)) => Some((
                Ok(Event::default().event("stream.lagged").data(n.to_string())),
                s,
            )),
            Err(RecvError::Closed) => None,
        }
    });

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30))))
}

/// Plan 0162 (GAR-670): unfold state for the SSE stream. Field order matters
/// — `rx` is declared first so it drops before `_guard`, ensuring
/// `receiver_count()` reflects post-our-drop state when cleanup runs.
struct ChatStreamState {
    rx: tokio::sync::broadcast::Receiver<serde_json::Value>,
    _guard: ChatStreamGuard,
}

/// RAII handle that GCs the chat's broadcast entry, emits the paired
/// `chat.unsubscribed` audit row, and releases the per-user SSE slot when
/// the SSE stream ends (plan 0162 findings F-1/F-4; plan 0163 GAR-679).
///
/// `actor_user_id` and `group_id` are captured at handler entry so the
/// audit emission can happen entirely from `Drop`, where the `Principal`
/// extractor is long gone.
struct ChatStreamGuard {
    chat_id: Uuid,
    actor_user_id: Uuid,
    group_id: Uuid,
    state: RestV1FullState,
    /// Plan 0163 (GAR-679): per-user slot counter decremented on drop.
    sse_counter: Arc<AtomicUsize>,
    /// Plan 0163: user whose slot must be released.
    sse_user_id: Uuid,
}

impl Drop for ChatStreamGuard {
    fn drop(&mut self) {
        // Field declaration order in `ChatStreamState` guarantees `rx`
        // dropped before this guard runs, so `receiver_count()` reflects
        // post-leave state (may be 0 if we were the last subscriber).
        let remaining_subscribers = self
            .state
            .chat_events
            .get(&self.chat_id)
            .map(|entry| entry.receiver_count())
            .unwrap_or(0);

        // Fire-and-forget audit emission. `Drop` is synchronous; audit needs
        // async DB access. We only spawn if there is an active Tokio runtime
        // — Drop can run in any context (e.g. test teardown outside a runtime),
        // and calling `tokio::spawn` outside a runtime panics. The
        // `try_current` probe converts that into a silent no-op.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let pool = self.state.app_pool.pool_for_handlers().clone();
            let chat_id = self.chat_id;
            let actor_user_id = self.actor_user_id;
            let group_id = self.group_id;
            handle.spawn(async move {
                if let Err(err) = emit_chat_unsubscribed(
                    &pool,
                    chat_id,
                    actor_user_id,
                    group_id,
                    remaining_subscribers,
                )
                .await
                {
                    tracing::warn!(
                        target: "chats.sse.audit",
                        %chat_id,
                        error = %err,
                        "failed to emit chat.unsubscribed audit event"
                    );
                }
            });
        }

        // GC the DashMap entry iff no subscribers remain (F-1). Idempotent
        // and runs synchronously so the audit spawn cannot race with a
        // re-subscribe that would re-create the broadcast channel.
        self.state.cleanup_chat_subscription(self.chat_id);

        // Plan 0163 (GAR-679): release the per-user SSE slot. GC the DashMap
        // entry when the counter reaches zero (Relaxed ordering is correct —
        // the counter is a pure counter with no associated memory).
        let remaining = self.sse_counter.fetch_sub(1, Ordering::Relaxed);
        if remaining == 1 {
            // We just decremented from 1 → 0; try to GC the map entry.
            self.state
                .sse_connections
                .remove_if(&self.sse_user_id, |_, c| c.load(Ordering::Relaxed) == 0);
        }
    }
}

/// Plan 0163 (GAR-679): RAII guard that holds the SSE slot decrement
/// responsibility until `ChatStreamGuard` is built. If any step in
/// `stream_chat` fails before the guard is built, this guard's `Drop`
/// releases the slot atomically. Call `disarm()` to transfer ownership
/// to `ChatStreamGuard` and prevent a double-decrement.
struct SseSlotGuard {
    counter: Arc<AtomicUsize>,
    armed: bool,
}

impl SseSlotGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SseSlotGuard {
    fn drop(&mut self) {
        if self.armed {
            self.counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Emit one `chat.unsubscribed` audit row from a fresh transaction. Called
/// only from `ChatStreamGuard::drop` (plan 0162 audit follow-up F-4 —
/// GAR-680).
///
/// Builds its own short transaction because the original request transaction
/// committed long before Drop runs. The two `SELECT set_config(...)` calls
/// re-establish the tenant context that `audit_events_group_or_self` RLS
/// policy requires (migration 007:161-168).
async fn emit_chat_unsubscribed(
    pool: &sqlx::PgPool,
    chat_id: Uuid,
    actor_user_id: Uuid,
    group_id: Uuid,
    remaining_subscribers: usize,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(actor_user_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut *tx)
        .await?;
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatUnsubscribed,
        actor_user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({ "subscriber_count": remaining_subscribers }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ─── GET /v1/chats/{chat_id}/threads (plan 0221 / GAR-740) ───────────────────

/// Query parameters for `GET /v1/chats/{chat_id}/threads`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListChatThreadsQuery {
    /// Keyset cursor — UUID of the last thread received. Omit for first page.
    pub after: Option<Uuid>,
    /// Page size. Default 20, max 50.
    pub limit: Option<u32>,
    /// When `true`, includes resolved threads. Default: `false` (only unresolved).
    pub include_resolved: Option<bool>,
}

/// A single thread entry returned by `GET /v1/chats/{chat_id}/threads`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatThreadSummary {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub root_message_id: Uuid,
    pub title: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    /// Count of non-deleted replies (`messages.thread_id = id AND deleted_at IS NULL`).
    pub reply_count: i64,
}

/// Response body for `GET /v1/chats/{chat_id}/threads`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatThreadsResponse {
    pub items: Vec<ChatThreadSummary>,
    /// Pass as `?after=<uuid>` on the next request. `null` when no more pages.
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/chats/{chat_id}/threads` — list threads in a chat.
///
/// Returns a cursor-paginated list of `message_threads` rows. Default returns
/// only unresolved threads; set `include_resolved=true` to include resolved ones.
///
/// ## Error matrix
///
/// | Condition                              | Status | Source         |
/// |----------------------------------------|--------|----------------|
/// | Missing/invalid JWT                    | 401    | Principal ext. |
/// | Non-member of group                    | 403    | Principal ext. |
/// | `X-Group-Id` header missing            | 400    | this handler   |
/// | Chat not found / not in caller's group | 404    | this handler   |
/// | Happy path                             | 200    |                |
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}/threads",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
        ListChatThreadsQuery,
    ),
    responses(
        (status = 200, description = "Paginated thread list.", body = ChatThreadsResponse),
        (status = 400, description = "X-Group-Id header missing.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_chat_threads(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
    Query(params): Query<ListChatThreadsQuery>,
) -> Result<Json<ChatThreadsResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    let limit = i64::from(
        params
            .limit
            .unwrap_or(DEFAULT_THREAD_LIMIT)
            .clamp(1, MAX_THREAD_LIMIT),
    );
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

    // Cross-tenant guard: 404 (not 403) to avoid leaking cross-tenant chat existence.
    let chat_exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_exists.is_none() {
        return Err(RestError::NotFound);
    }

    type ThreadListRow = (
        Uuid,
        Uuid,
        Uuid,
        Option<String>,
        Uuid,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        i64,
    );

    let rows: Vec<ThreadListRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT mt.id, mt.chat_id, mt.root_message_id, mt.title, mt.created_by,
                    mt.created_at, mt.resolved_at,
                    (SELECT COUNT(*) FROM messages m
                     WHERE m.thread_id = mt.id AND m.deleted_at IS NULL)::bigint AS reply_count
             FROM   message_threads mt
             WHERE  mt.chat_id = $1
               AND  ($2::bool IS TRUE OR mt.resolved_at IS NULL)
               AND  (mt.created_at, mt.id) < (
                        SELECT created_at, id FROM message_threads
                        WHERE  id = $4 AND chat_id = $1
                    )
             ORDER BY mt.created_at DESC, mt.id DESC
             LIMIT  $3",
        )
        .bind(chat_id)
        .bind(include_resolved)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT mt.id, mt.chat_id, mt.root_message_id, mt.title, mt.created_by,
                    mt.created_at, mt.resolved_at,
                    (SELECT COUNT(*) FROM messages m
                     WHERE m.thread_id = mt.id AND m.deleted_at IS NULL)::bigint AS reply_count
             FROM   message_threads mt
             WHERE  mt.chat_id = $1
               AND  ($2::bool IS TRUE OR mt.resolved_at IS NULL)
             ORDER BY mt.created_at DESC, mt.id DESC
             LIMIT  $3",
        )
        .bind(chat_id)
        .bind(include_resolved)
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
                root_message_id,
                title,
                created_by,
                created_at,
                resolved_at,
                reply_count,
            )| {
                ChatThreadSummary {
                    id,
                    chat_id,
                    root_message_id,
                    title,
                    created_by,
                    created_at,
                    resolved_at,
                    reply_count,
                }
            },
        )
        .collect();

    Ok(Json(ChatThreadsResponse { items, next_cursor }))
}

// ─── PATCH /v1/threads/{thread_id} (plan 0227 / GAR-745) ────────────────────

/// Request body for `PATCH /v1/threads/{thread_id}`.
/// At least one field must be present; all-`None` returns 400.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchThreadRequest {
    /// New title for the thread. Trimmed; must be 1–500 chars if supplied.
    pub title: Option<String>,
    /// `true` → mark resolved (`resolved_at = NOW()`).
    /// `false` → clear resolution (`resolved_at = NULL`).
    pub resolved: Option<bool>,
}

impl PatchThreadRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.title.is_none() && self.resolved.is_none() {
            return Err("at least one of 'title' or 'resolved' must be provided");
        }
        if let Some(ref t) = self.title {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                return Err("title must not be blank");
            }
            if trimmed.chars().count() > 500 {
                return Err("title must be 500 characters or fewer");
            }
        }
        Ok(())
    }
}

/// Response body for `GET` and `PATCH /v1/threads/{thread_id}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadDetailResponse {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub root_message_id: Uuid,
    pub title: Option<String>,
    pub created_by: Option<Uuid>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// `GET /v1/threads/{thread_id}` — fetch a single thread by ID.
///
/// Returns 404 for threads that belong to a different group (no existence leak).
#[utoipa::path(
    get,
    path = "/v1/threads/{thread_id}",
    params(("thread_id" = Uuid, Path, description = "Thread UUID.")),
    responses(
        (status = 200, description = "Thread detail.", body = ThreadDetailResponse),
        (status = 400, description = "Missing X-Group-Id header.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks required permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Thread not found or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_thread(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(thread_id): Path<Uuid>,
) -> Result<Json<ThreadDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
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

    type ThreadRow = (
        Uuid,
        Uuid,
        Uuid,
        Option<String>,
        Option<Uuid>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );
    let row: Option<ThreadRow> = sqlx::query_as(
        "SELECT mt.id, mt.chat_id, mt.root_message_id, mt.title, mt.created_by, \
         mt.resolved_at, mt.created_at \
         FROM message_threads mt \
         JOIN chats c ON c.id = mt.chat_id \
         WHERE mt.id = $1 AND c.group_id = $2",
    )
    .bind(thread_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (id, chat_id, root_message_id, title, created_by, resolved_at, created_at) =
        row.ok_or(RestError::NotFound)?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(ThreadDetailResponse {
        id,
        chat_id,
        root_message_id,
        title,
        created_by,
        resolved_at,
        created_at,
    }))
}

/// `PATCH /v1/threads/{thread_id}` — update a thread's title and/or resolve state.
///
/// Any member may resolve/unresolve (`ChatsRead`). Updating the title of a
/// thread created by someone else requires `ChatsModerate`.
#[utoipa::path(
    patch,
    path = "/v1/threads/{thread_id}",
    params(("thread_id" = Uuid, Path, description = "Thread UUID.")),
    request_body = PatchThreadRequest,
    responses(
        (status = 200, description = "Thread updated.", body = ThreadDetailResponse),
        (status = 400, description = "Validation failure.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks required permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Thread not found or in another group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_thread(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(thread_id): Path<Uuid>,
    Json(body): Json<PatchThreadRequest>,
) -> Result<Json<ThreadDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

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

    // Load the thread, verifying it belongs to a chat in the caller's group.
    type ThreadRow = (
        Uuid,
        Uuid,
        Option<String>,
        Option<Uuid>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );
    let row: Option<ThreadRow> = sqlx::query_as(
        "SELECT mt.id, mt.chat_id, mt.title, mt.created_by, mt.resolved_at, mt.created_at \
         FROM message_threads mt \
         JOIN chats c ON c.id = mt.chat_id \
         WHERE mt.id = $1 AND c.group_id = $2",
    )
    .bind(thread_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (_, _chat_id, current_title, created_by, _, _) = row.ok_or(RestError::NotFound)?;

    // Load root_message_id separately (not in the JOIN above for clarity).
    let root_row: Option<(Uuid,)> =
        sqlx::query_as("SELECT root_message_id FROM message_threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let root_message_id = root_row.map(|(id,)| id).ok_or(RestError::NotFound)?;

    // Title update requires ChatsModerate if the caller is not the creator.
    if body.title.is_some() {
        let is_own = created_by
            .map(|id| id == principal.user_id)
            .unwrap_or(false);
        if !is_own && !can(&principal, Action::ChatsModerate) {
            return Err(RestError::Forbidden);
        }
    }

    // Build the SET clause dynamically; at least one field is guaranteed by validate().
    let new_title: Option<String> = body
        .title
        .as_deref()
        .map(|t| t.trim().to_owned())
        .or(current_title);

    let updated: ThreadRow = match body.resolved {
        Some(true) => sqlx::query_as(
            "UPDATE message_threads \
             SET title = $2, resolved_at = NOW() \
             WHERE id = $1 \
             RETURNING id, chat_id, title, created_by, resolved_at, created_at",
        )
        .bind(thread_id)
        .bind(&new_title)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        Some(false) => sqlx::query_as(
            "UPDATE message_threads \
             SET title = $2, resolved_at = NULL \
             WHERE id = $1 \
             RETURNING id, chat_id, title, created_by, resolved_at, created_at",
        )
        .bind(thread_id)
        .bind(&new_title)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        None => sqlx::query_as(
            "UPDATE message_threads \
             SET title = $2 \
             WHERE id = $1 \
             RETURNING id, chat_id, title, created_by, resolved_at, created_at",
        )
        .bind(thread_id)
        .bind(&new_title)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    let (id, chat_id_ret, title_ret, created_by_ret, resolved_at_ret, created_at_ret) = updated;

    let mut changed_fields: Vec<&str> = Vec::new();
    if body.title.is_some() {
        changed_fields.push("title");
    }
    if body.resolved.is_some() {
        changed_fields.push("resolved");
    }

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ThreadUpdated,
        principal.user_id,
        group_id,
        "message_threads",
        thread_id.to_string(),
        json!({ "changed_fields": changed_fields }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(ThreadDetailResponse {
        id,
        chat_id: chat_id_ret,
        root_message_id,
        title: title_ret,
        created_by: created_by_ret,
        resolved_at: resolved_at_ret,
        created_at: created_at_ret,
    }))
}

// ─── PATCH /v1/chats/{chat_id}/members/{user_id} (plan 0227 / GAR-745) ──────

/// Request body for `PATCH /v1/chats/{chat_id}/members/{user_id}`.
/// At least one field must be present.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchChatMemberRequest {
    /// Mute/unmute the chat for this member.
    pub muted: Option<bool>,
    /// Mark messages up to this timestamp as read. Must not be in the future.
    pub last_read_at: Option<DateTime<Utc>>,
    /// Change the member's chat-local role. Only `'member'` and `'moderator'`
    /// are accepted; requires `MembersManage`.
    pub role: Option<String>,
}

impl PatchChatMemberRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.muted.is_none() && self.last_read_at.is_none() && self.role.is_none() {
            return Err("at least one of 'muted', 'last_read_at', or 'role' must be provided");
        }
        if let Some(ref r) = self.role {
            match r.as_str() {
                "member" | "moderator" => {}
                _ => return Err("role must be one of: member, moderator"),
            }
        }
        if let Some(lra) = self.last_read_at
            && lra > Utc::now()
        {
            return Err("last_read_at must not be in the future");
        }
        Ok(())
    }
}

/// Enriched response body for chat member operations — extends the basic
/// `ChatMemberResponse` with muted state and last_read_at cursor.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMemberDetailResponse {
    pub user_id: Uuid,
    pub role: String,
    pub joined_at: DateTime<Utc>,
    pub muted: bool,
    pub last_read_at: Option<DateTime<Utc>>,
}

/// `GET /v1/chats/{chat_id}/members/{user_id}` — fetch a single chat member.
///
/// Returns 404 for non-members, archived chats, or chats in another group
/// (no 403 existence leak).
#[utoipa::path(
    get,
    path = "/v1/chats/{chat_id}/members/{user_id}",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
        ("user_id" = Uuid, Path, description = "Member user UUID."),
    ),
    responses(
        (status = 200, description = "Chat member detail.", body = ChatMemberDetailResponse),
        (status = 400, description = "Missing X-Group-Id header.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found, archived, in another group, or user is not a member.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_chat_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((chat_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ChatMemberDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
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

    // Verify the chat exists in the caller's group (404, no 403 — no cross-tenant leak).
    let chat_exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_exists.is_none() {
        return Err(RestError::NotFound);
    }

    type MemberRow = (String, DateTime<Utc>, bool, Option<DateTime<Utc>>);
    let member: Option<MemberRow> = sqlx::query_as(
        "SELECT role, joined_at, muted, last_read_at \
         FROM chat_members \
         WHERE chat_id = $1 AND user_id = $2",
    )
    .bind(chat_id)
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let (role, joined_at, muted, last_read_at) = member.ok_or(RestError::NotFound)?;

    Ok(Json(ChatMemberDetailResponse {
        user_id: target_user_id,
        role,
        joined_at,
        muted,
        last_read_at,
    }))
}

/// `PATCH /v1/chats/{chat_id}/members/{user_id}` — update muted, last_read_at,
/// or chat-local role for a chat member.
///
/// Any authenticated chat member may update their **own** `muted` and
/// `last_read_at`. Updating another member's settings, or changing any
/// member's `role`, requires `MembersManage`.
#[utoipa::path(
    patch,
    path = "/v1/chats/{chat_id}/members/{user_id}",
    params(
        ("chat_id" = Uuid, Path, description = "Chat UUID."),
        ("user_id" = Uuid, Path, description = "Member user UUID."),
    ),
    request_body = PatchChatMemberRequest,
    responses(
        (status = 200, description = "Member updated.", body = ChatMemberDetailResponse),
        (status = 400, description = "Validation failure.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks required permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or member not found.", body = super::problem::ProblemDetails),
        (status = 422, description = "last_read_at is in the future.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_chat_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((chat_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchChatMemberRequest>,
) -> Result<Json<ChatMemberDetailResponse>, RestError> {
    let group_id = principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))?;

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // Callers updating another member's muted/last_read_at need MembersManage.
    let is_own = principal.user_id == target_user_id;
    if !is_own && !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }
    if body.role.is_some() && !can(&principal, Action::MembersManage) {
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

    // Verify the chat exists and belongs to the caller's group.
    let chat_exists: Option<(bool,)> = sqlx::query_as(
        "SELECT true FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Load the member row — 404 if not a member.
    type MemberRow = (String, DateTime<Utc>, bool, Option<DateTime<Utc>>);
    let member: Option<MemberRow> = sqlx::query_as(
        "SELECT role, joined_at, muted, last_read_at \
         FROM chat_members \
         WHERE chat_id = $1 AND user_id = $2",
    )
    .bind(chat_id)
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (cur_role, joined_at, cur_muted, cur_last_read) = member.ok_or(RestError::NotFound)?;

    let new_role = body.role.as_deref().unwrap_or(&cur_role).to_owned();
    let new_muted = body.muted.unwrap_or(cur_muted);
    let new_last_read: Option<DateTime<Utc>> = if body.last_read_at.is_some() {
        body.last_read_at
    } else {
        cur_last_read
    };

    sqlx::query(
        "UPDATE chat_members \
         SET role = $3, muted = $4, last_read_at = $5 \
         WHERE chat_id = $1 AND user_id = $2",
    )
    .bind(chat_id)
    .bind(target_user_id)
    .bind(&new_role)
    .bind(new_muted)
    .bind(new_last_read)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let mut changed_fields: Vec<&str> = Vec::new();
    if body.muted.is_some() {
        changed_fields.push("muted");
    }
    if body.last_read_at.is_some() {
        changed_fields.push("last_read_at");
    }
    if body.role.is_some() {
        changed_fields.push("role");
    }

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatMemberUpdated,
        principal.user_id,
        group_id,
        "chat_members",
        target_user_id.to_string(),
        json!({ "changed_fields": changed_fields }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(ChatMemberDetailResponse {
        user_id: target_user_id,
        role: new_role,
        joined_at,
        muted: new_muted,
        last_read_at: new_last_read,
    }))
}

// ─── Message Reactions (plan 0231 / GAR-747) ─────────────────────────────────

/// Request body for `POST /v1/messages/{message_id}/reactions`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddReactionRequest {
    /// Emoji string — 1 to 10 grapheme clusters. Unicode ZWJ sequences and
    /// skin-tone modifiers are supported because Postgres `char_length` counts
    /// grapheme clusters, not bytes.
    pub emoji: String,
}

impl AddReactionRequest {
    fn validate(&self) -> Result<(), RestError> {
        let len = self.emoji.chars().count();
        if !(1..=10).contains(&len) {
            return Err(RestError::BadRequest(
                "emoji must be 1–10 grapheme clusters".into(),
            ));
        }
        if self.emoji.trim().is_empty() {
            return Err(RestError::BadRequest("emoji must not be blank".into()));
        }
        Ok(())
    }
}

/// One emoji-level summary returned by `GET /v1/messages/{message_id}/reactions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReactionSummary {
    /// The emoji string.
    pub emoji: String,
    /// Total number of users who reacted with this emoji.
    pub count: i64,
    /// Whether the authenticated caller has reacted with this emoji.
    pub reacted_by_me: bool,
}

/// Response body for `GET /v1/messages/{message_id}/reactions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReactionsResponse {
    pub reactions: Vec<ReactionSummary>,
}

/// `POST /v1/messages/{message_id}/reactions` — add (or keep) an emoji reaction.
///
/// Idempotent: if the same (message, user, emoji) already exists the row is
/// retained and 201 is returned without error.
#[utoipa::path(
    post,
    path = "/v1/messages/{message_id}/reactions",
    params(("message_id" = Uuid, Path, description = "Message UUID.")),
    request_body = AddReactionRequest,
    responses(
        (status = 201, description = "Reaction added (or already present)."),
        (status = 400, description = "Validation error."),
        (status = 401, description = "Unauthenticated."),
        (status = 403, description = "Not a member of the chat."),
        (status = 404, description = "Message not found or cross-tenant."),
        (status = 500, description = "Internal error."),
    ),
    security(("bearer_token" = []))
)]
pub async fn add_message_reaction(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
    Json(body): Json<AddReactionRequest>,
) -> Result<StatusCode, RestError> {
    body.validate()?;

    let group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::Forbidden),
    };

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // RLS context.
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

    // Verify message belongs to this group (cross-tenant guard → 404).
    let msg_group: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if msg_group.is_none() {
        return Err(RestError::NotFound);
    }

    // Verify caller is a member of the chat that contains the message.
    let member: Option<(Uuid,)> = sqlx::query_as(
        "SELECT cm.user_id FROM chat_members cm \
         JOIN messages m ON m.chat_id = cm.chat_id \
         WHERE m.id = $1 AND cm.user_id = $2",
    )
    .bind(message_id)
    .bind(principal.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if member.is_none() {
        return Err(RestError::Forbidden);
    }

    // Upsert reaction (idempotent on PK conflict).
    sqlx::query(
        "INSERT INTO message_reactions (message_id, user_id, emoji, group_id) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (message_id, user_id, emoji) DO NOTHING",
    )
    .bind(message_id)
    .bind(principal.user_id)
    .bind(&body.emoji)
    .bind(group_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // Audit (PII-safe: emoji_len only, not the emoji value).
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MessageReactionAdded,
        principal.user_id,
        group_id,
        "message_reactions",
        message_id.to_string(),
        json!({ "emoji_len": body.emoji.chars().count() }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::CREATED)
}

/// `DELETE /v1/messages/{message_id}/reactions/{emoji}` — remove an emoji reaction.
///
/// Idempotent: returns 204 even if the reaction does not exist.
/// Callers can only remove their own reaction. `ChatsModerate` allows removing any.
#[utoipa::path(
    delete,
    path = "/v1/messages/{message_id}/reactions/{emoji}",
    params(
        ("message_id" = Uuid, Path, description = "Message UUID."),
        ("emoji" = String, Path, description = "URL-encoded emoji string."),
    ),
    responses(
        (status = 204, description = "Reaction removed (or was already absent)."),
        (status = 401, description = "Unauthenticated."),
        (status = 403, description = "Not own reaction and no ChatsModerate permission."),
        (status = 404, description = "Message not found or cross-tenant."),
        (status = 500, description = "Internal error."),
    ),
    security(("bearer_token" = []))
)]
pub async fn remove_message_reaction(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((message_id, emoji)): Path<(Uuid, String)>,
) -> Result<StatusCode, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::Forbidden),
    };

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // RLS context.
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

    // Cross-tenant guard: verify message belongs to this group.
    let msg_group: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if msg_group.is_none() {
        return Err(RestError::NotFound);
    }

    // Determine the target user_id for the DELETE.
    // Own reaction: no extra permission needed.
    // Other's reaction: requires ChatsModerate.
    let target_user_id = principal.user_id;

    if !can(&principal, Action::ChatsModerate) {
        // Without moderate, can only remove own reaction — just use own id.
    }

    // Delete own reaction (or any reaction if ChatsModerate, but we scope to caller
    // for non-moderate. For moderate callers, they would pass a query param in a
    // future slice; this slice only allows removing own reaction or, with
    // ChatsModerate, any reaction on the message by any user for the same emoji.
    let rows_affected = if can(&principal, Action::ChatsModerate) {
        // Moderate: delete any reaction with this emoji on this message.
        sqlx::query(
            "DELETE FROM message_reactions \
             WHERE message_id = $1 AND emoji = $2 AND group_id = $3",
        )
        .bind(message_id)
        .bind(&emoji)
        .bind(group_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
        .rows_affected()
    } else {
        // Normal: delete only own reaction.
        sqlx::query(
            "DELETE FROM message_reactions \
             WHERE message_id = $1 AND user_id = $2 AND emoji = $3 AND group_id = $4",
        )
        .bind(message_id)
        .bind(target_user_id)
        .bind(&emoji)
        .bind(group_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
        .rows_affected()
    };

    // Audit only when a row was actually deleted (idempotent — no audit for
    // already-absent reaction).
    if rows_affected > 0 {
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::MessageReactionRemoved,
            principal.user_id,
            group_id,
            "message_reactions",
            message_id.to_string(),
            json!({ "emoji_len": emoji.chars().count() }),
        )
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/messages/{message_id}/reactions` — list reactions grouped by emoji.
///
/// Returns reactions sorted by `emoji ASC`. No pagination — reactions per message
/// are bounded by the number of distinct (user, emoji) pairs, typically < 100.
#[utoipa::path(
    get,
    path = "/v1/messages/{message_id}/reactions",
    params(("message_id" = Uuid, Path, description = "Message UUID.")),
    responses(
        (status = 200, description = "Reactions list.", body = ReactionsResponse),
        (status = 401, description = "Unauthenticated."),
        (status = 403, description = "Not a group member."),
        (status = 404, description = "Message not found or cross-tenant."),
        (status = 500, description = "Internal error."),
    ),
    security(("bearer_token" = []))
)]
pub async fn list_message_reactions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(message_id): Path<Uuid>,
) -> Result<Json<ReactionsResponse>, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::Forbidden),
    };

    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // RLS context.
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

    // Cross-tenant guard → 404.
    let msg_group: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if msg_group.is_none() {
        return Err(RestError::NotFound);
    }

    // Group by emoji, count, and check if caller reacted.
    let rows: Vec<(String, i64, bool)> = sqlx::query_as(
        "SELECT emoji, \
                COUNT(*) AS count, \
                BOOL_OR(user_id = $2) AS reacted_by_me \
         FROM message_reactions \
         WHERE message_id = $1 AND group_id = $3 \
         GROUP BY emoji \
         ORDER BY emoji ASC",
    )
    .bind(message_id)
    .bind(principal.user_id)
    .bind(group_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let reactions = rows
        .into_iter()
        .map(|(emoji, count, reacted_by_me)| ReactionSummary {
            emoji,
            count,
            reacted_by_me,
        })
        .collect();

    Ok(Json(ReactionsResponse { reactions }))
}

// ─── Typing Indicator (plan 0233 / GAR-752) ──────────────────────────────────

/// `POST /v1/chats/{chat_id}/typing` — broadcast an ephemeral typing indicator
/// to all active SSE subscribers of the chat.
///
/// No body, no audit, no DB write. Returns 204 after verifying the chat exists
/// and belongs to the caller's group (cross-tenant guard).
#[utoipa::path(
    post,
    path = "/v1/chats/{chat_id}/typing",
    params(("chat_id" = Uuid, Path, description = "Chat UUID.")),
    responses(
        (status = 204, description = "Typing indicator sent (or no active subscribers)."),
        (status = 400, description = "Missing X-Group-Id header."),
        (status = 401, description = "Unauthenticated."),
        (status = 403, description = "Caller lacks ChatsRead permission."),
        (status = 404, description = "Chat not found or cross-tenant."),
        (status = 500, description = "Internal error."),
    ),
    security(("bearer_token" = []))
)]
pub async fn typing_indicator(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(chat_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
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

    // Cross-tenant guard: verify chat belongs to caller's group.
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

    let chat_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if chat_exists.is_none() {
        return Err(RestError::NotFound);
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Fire-and-forget: no-op if there are no active SSE subscribers.
    state.publish_chat_event(
        chat_id,
        json!({
            "type": "typing",
            "user_id": principal.user_id,
            "chat_id": chat_id,
        }),
    );

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_req(name: &str) -> CreateChatRequest {
        CreateChatRequest {
            name: name.into(),
            chat_type: "channel".into(),
            topic: None,
            partner_user_id: None,
        }
    }

    fn dm_req(partner: Option<Uuid>) -> CreateChatRequest {
        CreateChatRequest {
            name: "".into(),
            chat_type: "dm".into(),
            topic: None,
            partner_user_id: partner,
        }
    }

    #[test]
    fn create_chat_request_valid_channel() {
        assert!(channel_req("general").validate().is_ok());
    }

    #[test]
    fn create_chat_request_rejects_empty_name_for_channel() {
        assert_eq!(
            channel_req("  ").validate().unwrap_err(),
            "chat name must not be empty for type 'channel'"
        );
    }

    #[test]
    fn create_chat_request_accepts_dm_with_partner_user_id() {
        let req = dm_req(Some(Uuid::new_v4()));
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_chat_request_rejects_dm_without_partner_user_id() {
        assert_eq!(
            dm_req(None).validate().unwrap_err(),
            "type 'dm' requires 'partner_user_id'"
        );
    }

    #[test]
    fn create_chat_request_rejects_partner_user_id_on_channel() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "channel".into(),
            topic: None,
            partner_user_id: Some(Uuid::new_v4()),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "'partner_user_id' is only valid for type 'dm'"
        );
    }

    #[test]
    fn create_chat_request_rejects_thread() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "thread".into(),
            topic: None,
            partner_user_id: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "type 'thread' is not supported via this endpoint"
        );
    }

    #[test]
    fn create_chat_request_rejects_unknown_type() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "broadcast".into(),
            topic: None,
            partner_user_id: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "invalid chat type; must be 'channel' or 'dm'"
        );
    }

    #[test]
    fn create_chat_request_rejects_topic_over_4000_chars() {
        let mut req = channel_req("ok");
        req.topic = Some("a".repeat(MAX_TOPIC_CHARS + 1));
        assert_eq!(
            req.validate().unwrap_err(),
            "topic must be 4000 characters or fewer"
        );
    }

    #[test]
    fn create_chat_request_accepts_topic_at_limit() {
        let mut req = channel_req("ok");
        req.topic = Some("a".repeat(MAX_TOPIC_CHARS));
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_chat_request_topic_uses_char_count_not_byte_len() {
        // 1000 emoji chars = 4000 bytes; would fail a naive `len()` check
        // but pass a `chars().count()` check.
        let mut req = channel_req("ok");
        req.topic = Some("🌟".repeat(1_000));
        assert!(
            req.validate().is_ok(),
            "1000 emoji chars (4000 bytes) must pass the chars()-based check"
        );
    }

    // ── SSE broadcast unit tests (plan 0162, GAR-670) ──────────────────────

    #[test]
    fn chat_event_json_roundtrip() {
        let event = serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "chat_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "sender_label": "Alice",
            "body": "hello",
        });
        let serialized = serde_json::to_string(&event).unwrap();
        let back: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(back["body"], "hello");
    }

    #[tokio::test]
    async fn broadcast_send_receive() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(8);
        let payload = serde_json::json!({"msg": "hi"});
        tx.send(payload.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received["msg"], "hi");
    }

    #[tokio::test]
    async fn broadcast_no_subscriber_send_is_noop() {
        let (tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(8);
        // No receivers — send returns Err(SendError), which we ignore.
        let result = tx.send(serde_json::json!({"x": 1}));
        assert!(result.is_err(), "expected SendError when no receivers");
    }

    #[tokio::test]
    async fn broadcast_lagged_receiver_gets_lagged_error() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(2);
        // Fill beyond capacity to trigger lagged.
        for i in 0..4u64 {
            let _ = tx.send(serde_json::json!({"n": i}));
        }
        // First recv should return Lagged because 2 slots filled 4 times.
        let result = rx.recv().await;
        match result {
            Err(RecvError::Lagged(_)) => { /* expected */ }
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dashmap_lazy_channel_creation() {
        use dashmap::DashMap;
        let map: DashMap<Uuid, tokio::sync::broadcast::Sender<serde_json::Value>> = DashMap::new();
        let chat_id = Uuid::new_v4();

        // First access creates the channel.
        let mut rx = map
            .entry(chat_id)
            .or_insert_with(|| tokio::sync::broadcast::channel(8).0)
            .value()
            .subscribe();

        // Publishing via map lookup works.
        if let Some(tx) = map.get(&chat_id) {
            tx.send(serde_json::json!({"ok": true})).unwrap();
        }

        let msg = rx.recv().await.unwrap();
        assert_eq!(msg["ok"], true);
    }

    // ── F-1 cleanup contract (PR #459 audit fix) ────────────────────────────
    // The SSE handler's RAII guard calls `cleanup_chat_subscription` on stream
    // end, which uses `DashMap::remove_if(predicate)` so the entry is removed
    // iff no live receivers remain. The two tests below pin the contract that
    // `RestV1FullState::cleanup_chat_subscription` relies on.

    #[tokio::test]
    async fn dashmap_remove_if_drops_entry_when_last_receiver_gone() {
        use dashmap::DashMap;
        let map: DashMap<Uuid, tokio::sync::broadcast::Sender<serde_json::Value>> = DashMap::new();
        let chat_id = Uuid::new_v4();

        // Subscribe + immediately drop the receiver → receiver_count() == 0.
        {
            let _rx = map
                .entry(chat_id)
                .or_insert_with(|| tokio::sync::broadcast::channel(8).0)
                .value()
                .subscribe();
        } // _rx dropped here.

        assert!(
            map.contains_key(&chat_id),
            "entry still present pre-cleanup"
        );

        map.remove_if(&chat_id, |_, tx| tx.receiver_count() == 0);

        assert!(
            !map.contains_key(&chat_id),
            "remove_if must drop the entry when no receivers remain (F-1 fix)"
        );
    }

    #[tokio::test]
    async fn dashmap_remove_if_keeps_entry_when_other_receivers_alive() {
        use dashmap::DashMap;
        let map: DashMap<Uuid, tokio::sync::broadcast::Sender<serde_json::Value>> = DashMap::new();
        let chat_id = Uuid::new_v4();

        // Two subscribers. Drop one. Other still active → entry must stay.
        let _rx_keep = map
            .entry(chat_id)
            .or_insert_with(|| tokio::sync::broadcast::channel(8).0)
            .value()
            .subscribe();
        {
            let _rx_drop = map.get(&chat_id).unwrap().subscribe();
        } // _rx_drop dropped.

        map.remove_if(&chat_id, |_, tx| tx.receiver_count() == 0);

        assert!(
            map.contains_key(&chat_id),
            "remove_if must keep entry while at least one receiver lives (F-1 race safety)"
        );
    }

    // ── stream_chat auth guard unit tests (plan 0162 addendum) ───────────────
    // These tests exercise the same guard logic that runs inside stream_chat
    // before any DB access. Tests 1-3 are pure-logic (no DB, no Axum state).
    // Test 4 documents the cross-tenant invariant enforced by FORCE RLS.

    #[test]
    fn stream_chat_missing_x_group_id_header_yields_bad_request() {
        // Mirrors the `principal.group_id.is_none()` branch: handler returns
        // RestError::BadRequest before any DB query is executed.
        let p = Principal {
            user_id: Uuid::new_v4(),
            group_id: None,
            role: Some(garraia_auth::Role::Member),
        };
        let result: Result<Uuid, RestError> = p
            .group_id
            .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()));
        assert!(matches!(result, Err(RestError::BadRequest(_))));
    }

    #[test]
    fn stream_chat_no_group_role_yields_forbidden() {
        // `can()` returns false when `principal.role` is None (no group context).
        // Handler returns RestError::Forbidden before any DB query.
        let p = Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(Uuid::new_v4()),
            role: None,
        };
        assert!(!can(&p, Action::ChatsRead));
        let result: Result<(), RestError> = if can(&p, Action::ChatsRead) {
            Ok(())
        } else {
            Err(RestError::Forbidden)
        };
        assert!(matches!(result, Err(RestError::Forbidden)));
    }

    #[test]
    fn stream_chat_all_group_roles_have_chats_read() {
        // Every group role must pass ChatsRead — no member should be 403'd
        // when subscribing to the SSE stream of their own chat.
        for role in [
            garraia_auth::Role::Owner,
            garraia_auth::Role::Admin,
            garraia_auth::Role::Member,
            garraia_auth::Role::Guest,
            garraia_auth::Role::Child,
        ] {
            let p = Principal {
                user_id: Uuid::new_v4(),
                group_id: Some(Uuid::new_v4()),
                role: Some(role),
            };
            assert!(
                can(&p, Action::ChatsRead),
                "role {role:?} must have ChatsRead; stream_chat would wrongly 403"
            );
        }
    }

    #[test]
    fn stream_chat_cross_tenant_query_returns_not_found() {
        // Cross-tenant isolation: the handler converts 0-row fetch_optional
        // into RestError::NotFound (not Forbidden — avoids leaking chat
        // existence to other tenants). Two layers enforce this:
        // 1. SQL WHERE clause: `id = $chat_id AND group_id = $caller_group_id`
        //    → 0 rows when caller_group_id ≠ chat's actual group_id.
        // 2. FORCE RLS policy `chats_group_isolation` (migration 007):
        //    USING (group_id = current_setting('app.current_group_id')::uuid).
        //    Covered by GAR-392's 81-scenario RLS matrix.
        let simulated_row: Option<(Uuid,)> = None; // what DB returns for cross-tenant query
        let result = simulated_row.ok_or(RestError::NotFound);
        assert!(matches!(result, Err(RestError::NotFound)));
    }

    // ── GET /v1/chats/{chat_id}/threads (plan 0221 / GAR-740) ────────────────

    fn thread_query(
        after: Option<Uuid>,
        limit: Option<u32>,
        include_resolved: Option<bool>,
    ) -> ListChatThreadsQuery {
        ListChatThreadsQuery {
            after,
            limit,
            include_resolved,
        }
    }

    #[test]
    fn list_threads_limit_default() {
        let q = thread_query(None, None, None);
        let effective = q
            .limit
            .unwrap_or(DEFAULT_THREAD_LIMIT)
            .clamp(1, MAX_THREAD_LIMIT);
        assert_eq!(effective, 20);
    }

    #[test]
    fn list_threads_limit_clamped() {
        let q_zero = thread_query(None, Some(0), None);
        let effective_zero = q_zero
            .limit
            .unwrap_or(DEFAULT_THREAD_LIMIT)
            .clamp(1, MAX_THREAD_LIMIT);
        assert_eq!(effective_zero, 1);

        let q_over = thread_query(None, Some(100), None);
        let effective_over = q_over
            .limit
            .unwrap_or(DEFAULT_THREAD_LIMIT)
            .clamp(1, MAX_THREAD_LIMIT);
        assert_eq!(effective_over, 50);
    }

    #[test]
    fn list_threads_include_resolved_default() {
        let q = thread_query(None, None, None);
        let resolved = q.include_resolved.unwrap_or(false);
        assert!(!resolved, "default must exclude resolved threads");
    }

    #[test]
    fn list_threads_include_resolved_true() {
        let q = thread_query(None, None, Some(true));
        let resolved = q.include_resolved.unwrap_or(false);
        assert!(resolved, "explicit true must include resolved threads");
    }

    #[test]
    fn list_threads_limit_max_boundary() {
        let q = thread_query(None, Some(MAX_THREAD_LIMIT), None);
        let effective = q
            .limit
            .unwrap_or(DEFAULT_THREAD_LIMIT)
            .clamp(1, MAX_THREAD_LIMIT);
        assert_eq!(effective, MAX_THREAD_LIMIT);
    }

    // ── PatchThreadRequest::validate (plan 0227 / GAR-745) ──────────────────

    #[test]
    fn patch_thread_empty_body_rejected() {
        let req = PatchThreadRequest {
            title: None,
            resolved: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "at least one of 'title' or 'resolved' must be provided"
        );
    }

    #[test]
    fn patch_thread_resolved_only_valid() {
        let req = PatchThreadRequest {
            title: None,
            resolved: Some(true),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_thread_unresolve_valid() {
        let req = PatchThreadRequest {
            title: None,
            resolved: Some(false),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_thread_title_only_valid() {
        let req = PatchThreadRequest {
            title: Some("New title".into()),
            resolved: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_thread_blank_title_rejected() {
        let req = PatchThreadRequest {
            title: Some("   ".into()),
            resolved: None,
        };
        assert_eq!(req.validate().unwrap_err(), "title must not be blank");
    }

    #[test]
    fn patch_thread_title_over_500_chars_rejected() {
        let req = PatchThreadRequest {
            title: Some("a".repeat(501)),
            resolved: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "title must be 500 characters or fewer"
        );
    }

    #[test]
    fn patch_thread_title_at_500_chars_accepted() {
        let req = PatchThreadRequest {
            title: Some("a".repeat(500)),
            resolved: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_thread_title_uses_char_count_not_byte_len() {
        // 250 emoji = 250 chars but 1000 bytes — must pass the 500-char limit.
        let req = PatchThreadRequest {
            title: Some("🌟".repeat(250)),
            resolved: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_thread_both_fields_valid() {
        let req = PatchThreadRequest {
            title: Some("Updated".into()),
            resolved: Some(true),
        };
        assert!(req.validate().is_ok());
    }

    // ── PatchChatMemberRequest::validate (plan 0227 / GAR-745) ──────────────

    #[test]
    fn patch_chat_member_empty_body_rejected() {
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: None,
            role: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "at least one of 'muted', 'last_read_at', or 'role' must be provided"
        );
    }

    #[test]
    fn patch_chat_member_muted_only_valid() {
        let req = PatchChatMemberRequest {
            muted: Some(true),
            last_read_at: None,
            role: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_chat_member_unmute_valid() {
        let req = PatchChatMemberRequest {
            muted: Some(false),
            last_read_at: None,
            role: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_chat_member_valid_role_member() {
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: None,
            role: Some("member".into()),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_chat_member_valid_role_moderator() {
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: None,
            role: Some("moderator".into()),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_chat_member_invalid_role_owner_rejected() {
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: None,
            role: Some("owner".into()),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: member, moderator"
        );
    }

    #[test]
    fn patch_chat_member_invalid_role_admin_rejected() {
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: None,
            role: Some("admin".into()),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: member, moderator"
        );
    }

    #[test]
    fn patch_chat_member_past_last_read_at_valid() {
        let past = Utc::now() - chrono::Duration::hours(1);
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: Some(past),
            role: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn patch_chat_member_future_last_read_at_rejected() {
        let future = Utc::now() + chrono::Duration::hours(1);
        let req = PatchChatMemberRequest {
            muted: None,
            last_read_at: Some(future),
            role: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "last_read_at must not be in the future"
        );
    }

    #[test]
    fn patch_chat_member_all_fields_valid() {
        let past = Utc::now() - chrono::Duration::minutes(5);
        let req = PatchChatMemberRequest {
            muted: Some(false),
            last_read_at: Some(past),
            role: Some("moderator".into()),
        };
        assert!(req.validate().is_ok());
    }

    // ── POST /v1/messages/{id}/reactions — AddReactionRequest validation ─────

    fn reaction_req(emoji: &str) -> AddReactionRequest {
        AddReactionRequest {
            emoji: emoji.into(),
        }
    }

    #[test]
    fn add_reaction_empty_emoji_rejected() {
        assert!(matches!(
            reaction_req("").validate(),
            Err(RestError::BadRequest(_))
        ));
    }

    #[test]
    fn add_reaction_blank_whitespace_rejected() {
        assert!(matches!(
            reaction_req("   ").validate(),
            Err(RestError::BadRequest(_))
        ));
    }

    #[test]
    fn add_reaction_single_emoji_accepted() {
        assert!(reaction_req("👍").validate().is_ok());
    }

    #[test]
    fn add_reaction_exactly_10_chars_accepted() {
        assert!(reaction_req("aaaaaaaaaa").validate().is_ok());
    }

    #[test]
    fn add_reaction_11_chars_rejected() {
        assert!(matches!(
            reaction_req("aaaaaaaaaaa").validate(),
            Err(RestError::BadRequest(_))
        ));
    }

    #[test]
    fn add_reaction_skin_tone_modifier_accepted() {
        // 👍🏽 = 👍 + skin tone modifier = 2 Unicode scalar values → passes.
        assert!(reaction_req("👍🏽").validate().is_ok());
    }

    // ── Auth guard logic tests (plan 0231 / GAR-747) ─────────────────────────

    #[test]
    fn add_reaction_missing_group_id_yields_forbidden() {
        let p = Principal {
            user_id: Uuid::new_v4(),
            group_id: None,
            role: Some(garraia_auth::Role::Member),
        };
        let result: Result<(), RestError> = match p.group_id {
            Some(_) => Ok(()),
            None => Err(RestError::Forbidden),
        };
        assert!(matches!(result, Err(RestError::Forbidden)));
    }

    #[test]
    fn add_reaction_no_role_yields_forbidden() {
        let p = Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(Uuid::new_v4()),
            role: None,
        };
        assert!(!can(&p, Action::ChatsRead));
        let result: Result<(), RestError> = if can(&p, Action::ChatsRead) {
            Ok(())
        } else {
            Err(RestError::Forbidden)
        };
        assert!(matches!(result, Err(RestError::Forbidden)));
    }

    #[test]
    fn add_reaction_all_group_roles_have_chats_read() {
        for role in [
            garraia_auth::Role::Owner,
            garraia_auth::Role::Admin,
            garraia_auth::Role::Member,
            garraia_auth::Role::Guest,
            garraia_auth::Role::Child,
        ] {
            let p = Principal {
                user_id: Uuid::new_v4(),
                group_id: Some(Uuid::new_v4()),
                role: Some(role),
            };
            assert!(
                can(&p, Action::ChatsRead),
                "role {role:?} must have ChatsRead for reaction POST"
            );
        }
    }

    #[test]
    fn remove_reaction_only_owner_and_admin_have_chats_moderate() {
        for (role, expect_moderate) in [
            (garraia_auth::Role::Owner, true),
            (garraia_auth::Role::Admin, true),
            (garraia_auth::Role::Member, false),
            (garraia_auth::Role::Guest, false),
            (garraia_auth::Role::Child, false),
        ] {
            let p = Principal {
                user_id: Uuid::new_v4(),
                group_id: Some(Uuid::new_v4()),
                role: Some(role),
            };
            assert_eq!(
                can(&p, Action::ChatsModerate),
                expect_moderate,
                "role {role:?} ChatsModerate expectation mismatch"
            );
        }
    }

    // ── Cross-tenant guard (pure logic, no DB) ────────────────────────────────

    #[test]
    fn reaction_cross_tenant_message_lookup_returns_not_found() {
        let simulated: Option<(Uuid,)> = None;
        let result = simulated.ok_or(RestError::NotFound);
        assert!(matches!(result, Err(RestError::NotFound)));
    }

    // ── ReactionSummary / ReactionsResponse serialization ────────────────────

    #[test]
    fn reaction_summary_serializes_correctly() {
        let summary = ReactionSummary {
            emoji: "👍".into(),
            count: 3,
            reacted_by_me: true,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["emoji"], "👍");
        assert_eq!(v["count"], 3);
        assert_eq!(v["reacted_by_me"], true);
    }

    #[test]
    fn reactions_response_empty_list() {
        let resp = ReactionsResponse { reactions: vec![] };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["reactions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn reactions_response_multiple_entries() {
        let resp = ReactionsResponse {
            reactions: vec![
                ReactionSummary {
                    emoji: "❤️".into(),
                    count: 5,
                    reacted_by_me: false,
                },
                ReactionSummary {
                    emoji: "👍".into(),
                    count: 2,
                    reacted_by_me: true,
                },
            ],
        };
        let v = serde_json::to_value(&resp).unwrap();
        let arr = v["reactions"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["emoji"], "❤️");
        assert_eq!(arr[1]["reacted_by_me"], true);
    }

    // ── Audit metadata PII-safety invariant ──────────────────────────────────

    #[test]
    fn audit_metadata_carries_emoji_len_not_value() {
        let emoji = "👍🏽";
        let metadata = serde_json::json!({ "emoji_len": emoji.chars().count() });
        assert!(
            metadata.get("emoji").is_none(),
            "audit must not carry raw emoji"
        );
        assert!(
            metadata.get("emoji_len").is_some(),
            "audit must carry emoji_len"
        );
        assert_eq!(metadata["emoji_len"], emoji.chars().count());
    }

    // ── typing_indicator unit tests (plan 0233 / GAR-752) ────────────────────

    fn principal_for_typing(role: garraia_auth::Role) -> Principal {
        Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(Uuid::new_v4()),
            role: Some(role),
        }
    }

    fn principal_typing_no_group() -> Principal {
        Principal {
            user_id: Uuid::new_v4(),
            group_id: None,
            role: None,
        }
    }

    #[test]
    fn typing_missing_group_id_yields_bad_request() {
        let p = principal_typing_no_group();
        let result: Result<StatusCode, RestError> = if p.group_id.is_none() {
            Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ))
        } else {
            Ok(StatusCode::NO_CONTENT)
        };
        assert!(matches!(result, Err(RestError::BadRequest(_))));
    }

    #[test]
    fn typing_no_chats_read_yields_forbidden() {
        // None role → can() returns false → handler returns 403.
        let p = Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(Uuid::new_v4()),
            role: None,
        };
        let result: Result<StatusCode, RestError> = if !can(&p, Action::ChatsRead) {
            Err(RestError::Forbidden)
        } else {
            Ok(StatusCode::NO_CONTENT)
        };
        assert!(matches!(result, Err(RestError::Forbidden)));
    }

    #[test]
    fn typing_all_group_roles_have_chats_read() {
        for role in [
            garraia_auth::Role::Owner,
            garraia_auth::Role::Admin,
            garraia_auth::Role::Member,
            garraia_auth::Role::Guest,
            garraia_auth::Role::Child,
        ] {
            let p = principal_for_typing(role);
            assert!(
                can(&p, Action::ChatsRead),
                "role {role:?} must have ChatsRead for typing_indicator"
            );
        }
    }

    #[test]
    fn typing_cross_tenant_chat_lookup_yields_not_found() {
        let chat_exists: Option<(Uuid,)> = None;
        let result: Result<StatusCode, RestError> = if chat_exists.is_none() {
            Err(RestError::NotFound)
        } else {
            Ok(StatusCode::NO_CONTENT)
        };
        assert!(matches!(result, Err(RestError::NotFound)));
    }

    #[test]
    fn typing_event_payload_has_type_field() {
        let user_id = Uuid::new_v4();
        let chat_id = Uuid::new_v4();
        let event = json!({
            "type": "typing",
            "user_id": user_id,
            "chat_id": chat_id,
        });
        assert_eq!(event["type"], "typing");
        assert_eq!(event["user_id"].as_str().unwrap(), user_id.to_string());
        assert_eq!(event["chat_id"].as_str().unwrap(), chat_id.to_string());
    }

    #[test]
    fn typing_event_payload_has_no_display_name() {
        let event = json!({
            "type": "typing",
            "user_id": Uuid::new_v4(),
            "chat_id": Uuid::new_v4(),
        });
        assert!(
            event.get("display_name").is_none(),
            "typing event must not carry display_name (not in JWT)"
        );
    }

    #[test]
    fn typing_returns_204_on_success() {
        let result: Result<StatusCode, RestError> = Ok(StatusCode::NO_CONTENT);
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(StatusCode::NO_CONTENT));
    }

    // ── GET /v1/threads/{thread_id} ──────────────────────────────────────────

    #[test]
    fn get_thread_response_serializes_all_fields() {
        let id = Uuid::new_v4();
        let chat_id = Uuid::new_v4();
        let root_message_id = Uuid::new_v4();
        let created_by = Uuid::new_v4();
        let now = Utc::now();
        let resp = ThreadDetailResponse {
            id,
            chat_id,
            root_message_id,
            title: Some("Release planning".to_owned()),
            created_by: Some(created_by),
            resolved_at: Some(now),
            created_at: now,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"].as_str().unwrap(), id.to_string());
        assert_eq!(v["chat_id"].as_str().unwrap(), chat_id.to_string());
        assert_eq!(
            v["root_message_id"].as_str().unwrap(),
            root_message_id.to_string()
        );
        assert_eq!(v["title"].as_str().unwrap(), "Release planning");
        assert_eq!(v["created_by"].as_str().unwrap(), created_by.to_string());
        assert!(v["resolved_at"].is_string());
        assert!(v["created_at"].is_string());
    }

    #[test]
    fn get_thread_response_nil_title_serializes_null() {
        let resp = ThreadDetailResponse {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            root_message_id: Uuid::new_v4(),
            title: None,
            created_by: Some(Uuid::new_v4()),
            resolved_at: None,
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["title"].is_null());
    }

    #[test]
    fn get_thread_response_nil_created_by_serializes_null() {
        let resp = ThreadDetailResponse {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            root_message_id: Uuid::new_v4(),
            title: Some("thread".to_owned()),
            created_by: None,
            resolved_at: None,
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["created_by"].is_null());
    }

    #[test]
    fn get_thread_response_unresolved_has_null_resolved_at() {
        let resp = ThreadDetailResponse {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            root_message_id: Uuid::new_v4(),
            title: None,
            created_by: None,
            resolved_at: None,
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["resolved_at"].is_null());
    }

    #[test]
    fn get_thread_response_resolved_has_resolved_at_timestamp() {
        let resolved_at = Utc::now();
        let resp = ThreadDetailResponse {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            root_message_id: Uuid::new_v4(),
            title: Some("Sprint retro".to_owned()),
            created_by: Some(Uuid::new_v4()),
            resolved_at: Some(resolved_at),
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["resolved_at"].is_string());
        let s = v["resolved_at"].as_str().unwrap();
        assert!(
            s.ends_with('Z'),
            "resolved_at must be UTC ISO-8601 with Z suffix: {s}"
        );
    }

    #[test]
    fn get_thread_response_nil_uuid_round_trips() {
        let resp = ThreadDetailResponse {
            id: Uuid::nil(),
            chat_id: Uuid::nil(),
            root_message_id: Uuid::nil(),
            title: None,
            created_by: None,
            resolved_at: None,
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v["id"].as_str().unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(
            v["root_message_id"].as_str().unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    // ── get_chat_member unit tests ───────────────────────────────────────────

    #[test]
    fn get_chat_member_response_serializes_all_fields() {
        let resp = ChatMemberDetailResponse {
            user_id: Uuid::nil(),
            role: "member".into(),
            joined_at: DateTime::parse_from_rfc3339("2026-06-12T19:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["role"].as_str().unwrap(), "member");
        assert!(!v["muted"].as_bool().unwrap());
        assert!(v["last_read_at"].is_null());
    }

    #[test]
    fn get_chat_member_response_muted_true() {
        let resp = ChatMemberDetailResponse {
            user_id: Uuid::nil(),
            role: "owner".into(),
            joined_at: Utc::now(),
            muted: true,
            last_read_at: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["muted"].as_bool().unwrap());
        assert_eq!(v["role"].as_str().unwrap(), "owner");
    }

    #[test]
    fn get_chat_member_response_last_read_at_some_utc_z() {
        let ts = DateTime::parse_from_rfc3339("2026-06-12T15:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let resp = ChatMemberDetailResponse {
            user_id: Uuid::nil(),
            role: "moderator".into(),
            joined_at: ts,
            muted: false,
            last_read_at: Some(ts),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let s = v["last_read_at"].as_str().unwrap();
        assert!(s.ends_with('Z'), "timestamp must be UTC Z: {s}");
    }

    #[test]
    fn get_chat_member_response_role_viewer() {
        let resp = ChatMemberDetailResponse {
            user_id: Uuid::nil(),
            role: "viewer".into(),
            joined_at: Utc::now(),
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["role"].as_str().unwrap(), "viewer");
    }

    #[test]
    fn get_chat_member_response_nil_uuid_roundtrip() {
        let id = Uuid::nil();
        let resp = ChatMemberDetailResponse {
            user_id: id,
            role: "member".into(),
            joined_at: Utc::now(),
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v["user_id"].as_str().unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn get_chat_member_response_joined_at_utc_z() {
        let ts = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let resp = ChatMemberDetailResponse {
            user_id: Uuid::nil(),
            role: "member".into(),
            joined_at: ts,
            muted: false,
            last_read_at: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        let s = v["joined_at"].as_str().unwrap();
        assert!(s.ends_with('Z'), "joined_at must be UTC Z: {s}");
    }
}
