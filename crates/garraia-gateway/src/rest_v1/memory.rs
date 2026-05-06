//! `/v1/memory` handlers (plan 0062, GAR-514, epic GAR-WS-MEMORY slice 1).
//!
//! Three endpoints on the `garraia_app` RLS-enforced pool:
//! - `GET /v1/memory` — cursor-paginated list with scope filtering
//! - `POST /v1/memory` — create memory item
//! - `DELETE /v1/memory/{id}` — soft-delete
//!
//! ## Tenant-context protocol
//!
//! `memory_items` FORCE RLS dual policy (`memory_items_group_or_self`): group/chat scope uses
//! `group_id = app.current_group_id`; user scope uses `created_by = app.current_user_id AND
//! group_id IS NULL`. Both RLS vars set via parameterized `set_config` (plan 0056).
//!
//! ## Security filters applied to every SELECT
//!
//! `AND deleted_at IS NULL` · `AND sensitivity <> 'secret'` ·
//! `AND (ttl_expires_at IS NULL OR ttl_expires_at > now())`.
//! `sensitivity='secret'` items are never auto-returned.
//!
//! ## Scope validation (app-layer, on top of RLS)
//!
//! `user` → `scope_id` = `principal.user_id`. `group` → `scope_id` = `principal.group_id`.
//! `chat` → chat's `group_id` = `principal.group_id` (verified in-tx).

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

// ─── Constants ───────────────────────────────────────────────────────────────

/// Maximum memory item content length. Mirrors DB CHECK (migration 005).
const MAX_CONTENT_CHARS: usize = 10_000;

/// Default and maximum page sizes for list pagination.
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

/// Allowed `scope_type` values (DB CHECK in migration 005).
const ALLOWED_SCOPE_TYPES: &[&str] = &["user", "group", "chat"];

/// Allowed `kind` values (DB CHECK in migration 005).
const ALLOWED_KINDS: &[&str] = &["fact", "preference", "note", "reminder", "rule", "profile"];

/// Allowed `sensitivity` values (DB CHECK in migration 005).
const ALLOWED_SENSITIVITIES: &[&str] = &["public", "group", "private"];
// 'secret' is a valid DB value but must NOT be settable via this API in slice 1.

// ─── Private type aliases ────────────────────────────────────────────────────

/// (id, scope_type, scope_id, kind, content_preview, ttl_expires_at, pinned_at, created_at)
type MemoryListRow = (
    Uuid,
    String,
    Uuid,
    String,
    String,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

/// (id, kind, scope_type, pinned_at, ttl_expires_at, updated_at)
type MemoryPinRow = (
    Uuid,
    String,
    String,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

/// (id, created_by, group_id, ttl_expires_at, created_at, updated_at)
type MemoryInsertRow = (
    Uuid,
    Option<Uuid>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
);

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Request body for `POST /v1/memory`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryRequest {
    /// Scope type. One of `"user"`, `"group"`, `"chat"`.
    pub scope_type: String,
    /// UUID identifying the scope owner: `users.id`, `groups.id`, or `chats.id`.
    pub scope_id: Uuid,
    /// Semantic category. One of `"fact"`, `"preference"`, `"note"`,
    /// `"reminder"`, `"rule"`, `"profile"`.
    pub kind: String,
    /// Memory content. 1–10,000 characters (mirrors DB CHECK).
    pub content: String,
    /// Visibility tier. One of `"public"`, `"group"`, `"private"`.
    /// Defaults to `"private"`. `"secret"` may not be set via this endpoint.
    #[serde(default = "default_sensitivity")]
    pub sensitivity: String,
    /// Optional: the chat this memory was extracted from.
    pub source_chat_id: Option<Uuid>,
    /// Optional: the specific message this memory was extracted from.
    pub source_message_id: Option<Uuid>,
    /// Optional TTL. Must be in the future.
    pub ttl_expires_at: Option<DateTime<Utc>>,
}

fn default_sensitivity() -> String {
    "private".to_string()
}

impl CreateMemoryRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !ALLOWED_SCOPE_TYPES.contains(&self.scope_type.as_str()) {
            return Err("scope_type must be one of: user, group, chat");
        }
        if !ALLOWED_KINDS.contains(&self.kind.as_str()) {
            return Err("kind must be one of: fact, preference, note, reminder, rule, profile");
        }
        if !ALLOWED_SENSITIVITIES.contains(&self.sensitivity.as_str()) {
            return Err("sensitivity must be one of: public, group, private");
        }
        let content_chars = self.content.chars().count();
        if content_chars == 0 {
            return Err("content must not be empty");
        }
        if content_chars > MAX_CONTENT_CHARS {
            return Err("content exceeds 10,000 character limit");
        }
        if self.ttl_expires_at.is_some_and(|ttl| ttl <= Utc::now()) {
            return Err("ttl_expires_at must be in the future");
        }
        Ok(())
    }
}

/// Full memory item response for `POST /v1/memory`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MemoryItemResponse {
    pub id: Uuid,
    pub scope_type: String,
    pub scope_id: Uuid,
    pub group_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub kind: String,
    pub content: String,
    pub sensitivity: String,
    pub source_chat_id: Option<Uuid>,
    pub source_message_id: Option<Uuid>,
    pub ttl_expires_at: Option<DateTime<Utc>>,
    /// Non-null when the item is pinned. Pin clears `ttl_expires_at`.
    pub pinned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for `POST /v1/memory/{id}/pin` and `/unpin`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PinMemoryResponse {
    pub id: Uuid,
    /// Non-null when pinned. `null` when unpinned.
    pub pinned_at: Option<DateTime<Utc>>,
    /// Always `null` when pinned (pin clears TTL). After unpin, TTL
    /// is NOT restored — caller must re-set it explicitly.
    pub ttl_expires_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Compact summary used in `GET /v1/memory` list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct MemoryItemSummary {
    pub id: Uuid,
    pub scope_type: String,
    pub scope_id: Uuid,
    pub kind: String,
    /// First 200 characters of content for list view. Full content via
    /// `GET /v1/memory/{id}` (plan 0063).
    pub content_preview: String,
    pub sensitivity: String,
    pub ttl_expires_at: Option<DateTime<Utc>>,
    /// Non-null when the item is pinned.
    pub pinned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/memory`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListMemoryResponse {
    pub items: Vec<MemoryItemSummary>,
    /// Cursor for the next page. `None` when the end of the list is reached.
    /// Pass as `?cursor=<uuid>` in the next request.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/memory`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMemoryQuery {
    /// Scope type filter. Required. One of `user`, `group`, `chat`.
    pub scope_type: String,
    /// Scope ID filter. Required. UUID of the user, group, or chat.
    pub scope_id: Uuid,
    /// Keyset cursor — UUID of the last item received. Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Set RLS context and return the group_id to use for audit events.
/// For scope_type='user', we still set app.current_group_id to the
/// principal's group so that audit_events (which are always group-scoped)
/// can be inserted.
async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<(), RestError> {
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(())
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/memory` — list memory items visible to the caller.
///
/// Authz: caller must be a group member with `Action::MemoryRead`.
///
/// Security filters applied automatically:
/// - `deleted_at IS NULL`
/// - `sensitivity <> 'secret'`
/// - `ttl_expires_at IS NULL OR ttl_expires_at > now()`
///
/// ## Scope validation
///
/// - `scope_type=user` → `scope_id` must equal `principal.user_id`.
/// - `scope_type=group` → `scope_id` must equal `principal.group_id`.
/// - `scope_type=chat` → chat's group must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Missing/invalid scope_type / id   | 400    |
/// | scope_id mismatch (cross-group)    | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/memory",
    params(ListMemoryQuery),
    responses(
        (status = 200, description = "Memory items.", body = ListMemoryResponse),
        (status = 400, description = "Invalid scope parameters.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or scope mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMemoryQuery>,
) -> Result<Json<ListMemoryResponse>, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::MemoryRead) {
        return Err(RestError::Forbidden);
    }

    // Validate scope_type.
    if !ALLOWED_SCOPE_TYPES.contains(&params.scope_type.as_str()) {
        return Err(RestError::BadRequest(
            "scope_type must be one of: user, group, chat".into(),
        ));
    }

    // App-layer scope validation.
    match params.scope_type.as_str() {
        "user" => {
            if params.scope_id != principal.user_id {
                return Err(RestError::Forbidden);
            }
        }
        "group" => {
            if params.scope_id != group_id {
                return Err(RestError::Forbidden);
            }
        }
        "chat" => {
            // Validated in-tx below.
        }
        _ => unreachable!(),
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // For chat scope, verify the chat belongs to the caller's group.
    if params.scope_type == "chat" {
        let chat_row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
        )
        .bind(params.scope_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if chat_row.is_none() {
            return Err(RestError::Forbidden);
        }
    }

    // Fetch one extra row to determine if there's a next page.
    let fetch_limit = (effective_limit + 1) as i64;

    // Cursor-keyset pagination: items older than the cursor (by created_at, id
    // tiebreak). First page: no cursor.
    let rows: Vec<MemoryListRow> = if let Some(cursor_id) = params.cursor {
        sqlx::query_as(
            "SELECT m.id, m.scope_type, m.scope_id, m.kind, \
                        left(m.content, 200) AS content_preview, \
                        m.ttl_expires_at, m.pinned_at, m.created_at \
                 FROM memory_items m \
                 WHERE m.scope_type = $1 \
                   AND m.scope_id = $2 \
                   AND m.deleted_at IS NULL \
                   AND m.sensitivity <> 'secret' \
                   AND (m.ttl_expires_at IS NULL OR m.ttl_expires_at > now()) \
                   AND (m.created_at, m.id) < ( \
                       SELECT created_at, id FROM memory_items WHERE id = $3 \
                   ) \
                 ORDER BY m.created_at DESC, m.id DESC \
                 LIMIT $4",
        )
        .bind(&params.scope_type)
        .bind(params.scope_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT m.id, m.scope_type, m.scope_id, m.kind, \
                        left(m.content, 200) AS content_preview, \
                        m.ttl_expires_at, m.pinned_at, m.created_at \
                 FROM memory_items m \
                 WHERE m.scope_type = $1 \
                   AND m.scope_id = $2 \
                   AND m.deleted_at IS NULL \
                   AND m.sensitivity <> 'secret' \
                   AND (m.ttl_expires_at IS NULL OR m.ttl_expires_at > now()) \
                 ORDER BY m.created_at DESC, m.id DESC \
                 LIMIT $3",
        )
        .bind(&params.scope_type)
        .bind(params.scope_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<MemoryItemSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(
            |(
                id,
                scope_type,
                scope_id,
                kind,
                content_preview,
                ttl_expires_at,
                pinned_at,
                created_at,
            )| {
                MemoryItemSummary {
                    id,
                    scope_type,
                    scope_id,
                    kind,
                    content_preview,
                    sensitivity: String::new(), // not exposed in summary for security
                    ttl_expires_at,
                    pinned_at,
                    created_at,
                }
            },
        )
        .collect();

    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListMemoryResponse { items, next_cursor }))
}

/// `POST /v1/memory` — create a memory item.
///
/// Authz: caller must be a group member with `Action::MemoryWrite`.
///
/// Emits `memory.created` audit event with structural metadata only
/// (no content — that's PII).
///
/// ## Error matrix
///
/// | Condition                              | Status |
/// |----------------------------------------|--------|
/// | Missing/invalid JWT                    | 401    |
/// | Non-member of group                    | 403    |
/// | scope_id mismatch (cross-group)        | 403    |
/// | Validation failure                     | 400    |
/// | chat scope_id not found in group       | 404    |
/// | Happy path                             | 201    |
#[utoipa::path(
    post,
    path = "/v1/memory",
    request_body = CreateMemoryRequest,
    responses(
        (status = 201, description = "Memory item created.", body = MemoryItemResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or scope mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Chat not found or not in caller's group (chat scope only).", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(body): Json<CreateMemoryRequest>,
) -> Result<(StatusCode, Json<MemoryItemResponse>), RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::MemoryWrite) {
        return Err(RestError::Forbidden);
    }

    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // App-layer scope validation.
    match body.scope_type.as_str() {
        "user" => {
            if body.scope_id != principal.user_id {
                return Err(RestError::Forbidden);
            }
        }
        "group" => {
            if body.scope_id != group_id {
                return Err(RestError::Forbidden);
            }
        }
        "chat" => {
            // Validated in-tx below.
        }
        _ => unreachable!(), // validate() already caught this
    }

    // Determine group_id for the row:
    // - group scope → group_id = scope_id = group_id
    // - chat scope  → group_id = principal.group_id
    // - user scope  → group_id = NULL (personal memories)
    let row_group_id: Option<Uuid> = match body.scope_type.as_str() {
        "user" => None,
        "group" | "chat" => Some(group_id),
        _ => unreachable!(),
    };

    let content_trimmed = body.content.trim().to_string();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // For chat scope: verify the chat belongs to the caller's group.
    if body.scope_type == "chat" {
        let chat_row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
        )
        .bind(body.scope_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if chat_row.is_none() {
            return Err(RestError::NotFound);
        }
    }

    // Resolve created_by_label from users.display_name within the tx.
    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    // INSERT memory item.
    let row: MemoryInsertRow = sqlx::query_as(
        "INSERT INTO memory_items \
             (scope_type, scope_id, group_id, created_by, created_by_label, \
              kind, content, sensitivity, source_chat_id, source_message_id, ttl_expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING id, created_by, group_id, ttl_expires_at, created_at, updated_at",
    )
    .bind(&body.scope_type)
    .bind(body.scope_id)
    .bind(row_group_id)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .bind(&body.kind)
    .bind(&content_trimmed)
    .bind(&body.sensitivity)
    .bind(body.source_chat_id)
    .bind(body.source_message_id)
    .bind(body.ttl_expires_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (mem_id, created_by, returned_group_id, ttl_expires_at, created_at, updated_at) = row;

    // Audit: structural metadata only — no content (PII).
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemoryCreated,
        principal.user_id,
        group_id,
        "memory_items",
        mem_id.to_string(),
        json!({
            "content_len": content_trimmed.chars().count(),
            "kind": body.kind,
            "scope_type": body.scope_type,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(MemoryItemResponse {
            id: mem_id,
            scope_type: body.scope_type,
            scope_id: body.scope_id,
            group_id: returned_group_id,
            created_by,
            created_by_label,
            kind: body.kind,
            content: content_trimmed,
            sensitivity: body.sensitivity,
            source_chat_id: body.source_chat_id,
            source_message_id: body.source_message_id,
            ttl_expires_at,
            pinned_at: None,
            created_at,
            updated_at,
        }),
    ))
}

/// `DELETE /v1/memory/{id}` — soft-delete a memory item.
///
/// Authz: caller must be a group member with `Action::MemoryDelete`.
/// The caller must also be visible as the creator or group member
/// (enforced by RLS). Cross-tenant items return 404.
///
/// Sets `deleted_at = now()`. The item is not physically removed.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Item not found / cross-tenant      | 404    |
/// | Item already deleted               | 404    |
/// | Happy path                         | 204    |
#[utoipa::path(
    delete,
    path = "/v1/memory/{id}",
    params(
        ("id" = Uuid, Path, description = "Memory item UUID."),
    ),
    responses(
        (status = 204, description = "Memory item deleted."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Memory item not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(memory_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::MemoryDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch the item to confirm existence + get kind/scope for audit.
    // RLS filters cross-tenant rows to 0 rows → 404.
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT kind, scope_type FROM memory_items \
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (kind, scope_type) = match existing {
        Some(row) => row,
        None => return Err(RestError::NotFound),
    };

    // Soft-delete.
    sqlx::query("UPDATE memory_items SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(memory_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Audit.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemoryDeleted,
        principal.user_id,
        group_id,
        "memory_items",
        memory_id.to_string(),
        json!({
            "kind": kind,
            "scope_type": scope_type,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/memory/{id}/pin` — pin a memory item (never expires).
///
/// Authz: caller must be a group member with `Action::MemoryWrite`.
///
/// Sets `pinned_at = now()` and `ttl_expires_at = NULL` atomically.
/// Pinning is **idempotent**: re-pinning refreshes `pinned_at`. RLS
/// filters cross-tenant rows to 0 rows → 404 (not 403).
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Item not found / cross-tenant      | 404    |
/// | Item already deleted               | 404    |
/// | Happy path                         | 200    |
#[utoipa::path(
    post,
    path = "/v1/memory/{id}/pin",
    params(
        ("id" = Uuid, Path, description = "Memory item UUID."),
    ),
    responses(
        (status = 200, description = "Memory item pinned.", body = PinMemoryResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Memory item not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn pin_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(memory_id): Path<Uuid>,
) -> Result<Json<PinMemoryResponse>, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::MemoryWrite) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Pin: set pinned_at = now(), clear ttl_expires_at.
    // RETURNING kind + scope_type for audit; id, pinned_at, ttl_expires_at,
    // updated_at for response. RLS filters cross-tenant rows → 0 rows → 404.
    let row: Option<MemoryPinRow> = sqlx::query_as(
        "UPDATE memory_items \
         SET pinned_at = now(), ttl_expires_at = NULL, updated_at = now() \
         WHERE id = $1 AND deleted_at IS NULL \
         RETURNING id, kind, scope_type, pinned_at, ttl_expires_at, updated_at",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (id, kind, scope_type, pinned_at, ttl_expires_at, updated_at) = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemoryPinned,
        principal.user_id,
        group_id,
        "memory_items",
        id.to_string(),
        json!({
            "kind": kind,
            "scope_type": scope_type,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(PinMemoryResponse {
        id,
        pinned_at,
        ttl_expires_at,
        updated_at,
    }))
}

/// `POST /v1/memory/{id}/unpin` — remove pin from a memory item.
///
/// Authz: caller must be a group member with `Action::MemoryWrite`.
///
/// Sets `pinned_at = NULL`. `ttl_expires_at` is **NOT** restored —
/// caller must re-set it explicitly if a TTL is desired. Unpinning
/// an already-unpinned item is a no-op (returns 200).
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Item not found / cross-tenant      | 404    |
/// | Item already deleted               | 404    |
/// | Happy path                         | 200    |
#[utoipa::path(
    post,
    path = "/v1/memory/{id}/unpin",
    params(
        ("id" = Uuid, Path, description = "Memory item UUID."),
    ),
    responses(
        (status = 200, description = "Memory item unpinned.", body = PinMemoryResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Memory item not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn unpin_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(memory_id): Path<Uuid>,
) -> Result<Json<PinMemoryResponse>, RestError> {
    let group_id = match principal.group_id {
        Some(g) => g,
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    };

    if !can(&principal, Action::MemoryWrite) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Unpin: set pinned_at = NULL; ttl_expires_at intentionally NOT touched.
    // Idempotent: already-unpinned items updated with no-op (pinned_at stays NULL).
    let row: Option<MemoryPinRow> = sqlx::query_as(
        "UPDATE memory_items \
         SET pinned_at = NULL, updated_at = now() \
         WHERE id = $1 AND deleted_at IS NULL \
         RETURNING id, kind, scope_type, pinned_at, ttl_expires_at, updated_at",
    )
    .bind(memory_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (id, kind, scope_type, pinned_at, ttl_expires_at, updated_at) = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemoryUnpinned,
        principal.user_id,
        group_id,
        "memory_items",
        id.to_string(),
        json!({
            "kind": kind,
            "scope_type": scope_type,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(PinMemoryResponse {
        id,
        pinned_at,
        ttl_expires_at,
        updated_at,
    }))
}
