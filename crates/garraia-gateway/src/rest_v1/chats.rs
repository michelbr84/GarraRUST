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
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
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
                return Err("type 'dm' is not yet supported in this slice; only 'channel'");
            }
            "thread" => {
                return Err("type 'thread' is not yet supported in this slice; only 'channel'");
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

    // 4. Open transaction. SET LOCAL is tx-scoped — auto-commit would
    //    drop the setting between statements (team-coordinator gate
    //    risk #6, plan 0016 M4).
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context — BOTH user and group required because `chats`
    //    is FORCE RLS on `app.current_group_id` (migration 007:89-94),
    //    `chat_members` is JOIN-RLS via chats (007:99-112) and
    //    `audit_events` requires both (007:161-168).
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

    // 6. INSERT chat. RETURNING gives us id + created_at in one roundtrip.
    let (chat_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO chats (group_id, type, name, topic, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, created_at",
    )
    .bind(group_id)
    .bind(&body.chat_type)
    .bind(&trimmed_name)
    .bind(trimmed_topic.as_deref())
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 7. Auto-enroll the creator as the chat owner. Same tx so the
    //    chat row + member row are atomic. The JOIN-RLS subquery resolves
    //    correctly inside the same tx because the freshly-inserted chats
    //    row is visible to subsequent statements before commit.
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 8. Audit. Metadata carries STRUCTURE only — chat name/topic are
    //    user-controlled and may contain PII (family nickname, customer
    //    name, internal codename). The `chats` row itself is the source
    //    of truth for read-back via GET (plan 0054 invariant 7).
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatCreated,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({
            "name_len": trimmed_name.chars().count(),
            "type": body.chat_type,
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
            chat_type: body.chat_type,
            name: trimmed_name,
            topic: trimmed_topic,
            created_by: principal.user_id,
            created_at,
        }),
    ))
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
