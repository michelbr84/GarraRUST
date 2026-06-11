//! Doc-blocks handlers for the Docs Tier 2 surface.
//! Plan 0302 / GAR-840.
//!
//! Four endpoints on the `garraia_app` RLS-enforced pool:
//! - `POST /v1/doc-pages/{page_id}/blocks`  — create a block (201)
//! - `GET  /v1/doc-pages/{page_id}/blocks`  — list blocks (200)
//! - `PATCH /v1/doc-blocks/{block_id}`      — update block (200)
//! - `DELETE /v1/doc-blocks/{block_id}`     — delete block (204)
//!
//! ## Tenant-context protocol
//!
//! `doc_blocks` uses FORCE RLS with `group_id` isolation (migration 027).
//! Both RLS vars (`app.current_user_id` + `app.current_group_id`) are set
//! via parameterised `set_config` before any SQL in every transaction.
//!
//! ## Cross-group isolation
//!
//! The `page_id` path param is looked up against `doc_pages` inside the
//! caller's RLS context. A `page_id` belonging to a different group returns
//! 0 rows → 404, preventing cross-group information disclosure.

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

// ─── Valid block types ────────────────────────────────────────────────────────

const VALID_BLOCK_TYPES: &[&str] = &[
    "heading",
    "paragraph",
    "todo",
    "bullet",
    "numbered",
    "code",
    "quote",
    "callout",
    "divider",
    "file_embed",
    "task_embed",
    "chat_embed",
    "image",
];

// ─── Private DB row struct ────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DocBlockRow {
    id: Uuid,
    page_id: Uuid,
    group_id: Uuid,
    parent_block_id: Option<Uuid>,
    position: f64,
    block_type: String,
    content_jsonb: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Request body for `POST /v1/doc-pages/{page_id}/blocks`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDocBlockRequest {
    /// Block type. One of: heading, paragraph, todo, bullet, numbered, code,
    /// quote, callout, divider, file_embed, task_embed, chat_embed, image.
    #[serde(rename = "type")]
    pub block_type: String,
    /// Block-specific content as JSON. Defaults to `{}`.
    pub content: Option<serde_json::Value>,
    /// Position for ordering. Defaults to last position + 1.0.
    pub position: Option<f64>,
    /// Optional parent block UUID for nesting.
    pub parent_block_id: Option<Uuid>,
}

impl CreateDocBlockRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if !VALID_BLOCK_TYPES.contains(&self.block_type.as_str()) {
            return Err("invalid block type");
        }
        Ok(())
    }
}

/// Patch body for `PATCH /v1/doc-blocks/{block_id}`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchDocBlockRequest {
    /// New block type. Optional.
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    /// Replacement content JSON. Optional.
    pub content: Option<serde_json::Value>,
    /// New position. Optional.
    pub position: Option<f64>,
    /// New parent block UUID. `null` to promote to root. Optional.
    pub parent_block_id: Option<Option<Uuid>>,
}

impl PatchDocBlockRequest {
    fn is_empty(&self) -> bool {
        self.block_type.is_none()
            && self.content.is_none()
            && self.position.is_none()
            && self.parent_block_id.is_none()
    }

    fn validate(&self) -> Result<(), &'static str> {
        if let Some(ref bt) = self.block_type
            && !VALID_BLOCK_TYPES.contains(&bt.as_str())
        {
            return Err("invalid block type");
        }
        Ok(())
    }
}

/// Full doc block representation returned by `POST` and `PATCH`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocBlockResponse {
    pub id: Uuid,
    pub page_id: Uuid,
    pub group_id: Uuid,
    pub parent_block_id: Option<Uuid>,
    pub position: f64,
    #[serde(rename = "type")]
    pub block_type: String,
    pub content: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DocBlockRow> for DocBlockResponse {
    fn from(r: DocBlockRow) -> Self {
        Self {
            id: r.id,
            page_id: r.page_id,
            group_id: r.group_id,
            parent_block_id: r.parent_block_id,
            position: r.position,
            block_type: r.block_type,
            content: r.content_jsonb,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Response body for `GET /v1/doc-pages/{page_id}/blocks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListDocBlocksResponse {
    pub items: Vec<DocBlockResponse>,
}

/// Query params for `GET /v1/doc-pages/{page_id}/blocks`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListDocBlocksQuery {
    /// Filter to direct children of this parent block. Omit for root-level blocks.
    pub parent_block_id: Option<Uuid>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/doc-pages/{page_id}/blocks` — create a block in a doc page.
///
/// Authz: `Action::DocsWrite`. The `page_id` must belong to `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                    | Status |
/// |------------------------------|--------|
/// | Missing/invalid JWT          | 401    |
/// | Caller not a group member    | 403    |
/// | Missing X-Group-Id header    | 400    |
/// | Invalid block type           | 400    |
/// | Page not found / cross-group | 404    |
/// | Happy path                   | 201    |
#[utoipa::path(
    post,
    path = "/v1/doc-pages/{page_id}/blocks",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID."),
    ),
    request_body = CreateDocBlockRequest,
    responses(
        (status = 201, description = "Block created.", body = DocBlockResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Page not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_doc_block(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
    Json(body): Json<CreateDocBlockRequest>,
) -> Result<(StatusCode, Json<DocBlockResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify the page exists and belongs to the caller's group (RLS filters cross-group).
    let page_group: Option<(Uuid,)> =
        sqlx::query_as("SELECT group_id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    let _page_group_id = page_group.ok_or(RestError::NotFound)?.0;

    // Validate parent_block_id if provided.
    if let Some(parent_id) = body.parent_block_id {
        let parent_exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM doc_blocks WHERE id = $1 AND page_id = $2")
                .bind(parent_id)
                .bind(page_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;
        if parent_exists.is_none() {
            return Err(RestError::NotFound);
        }
    }

    // Compute default position: max position in this page + 1.0.
    let position = if let Some(pos) = body.position {
        pos
    } else {
        let max_pos: Option<f64> =
            sqlx::query_scalar("SELECT MAX(position) FROM doc_blocks WHERE page_id = $1")
                .bind(page_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;
        max_pos.unwrap_or(0.0) + 1.0
    };

    let content = body
        .content
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let row: DocBlockRow = sqlx::query_as(
        "INSERT INTO doc_blocks \
             (page_id, group_id, parent_block_id, position, block_type, content_jsonb) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, page_id, group_id, parent_block_id, position, \
                   block_type, content_jsonb, created_at, updated_at",
    )
    .bind(page_id)
    .bind(group_id)
    .bind(body.parent_block_id)
    .bind(position)
    .bind(&body.block_type)
    .bind(&content)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let block_id = row.id;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::DocBlockCreated,
        principal.user_id,
        group_id,
        "doc_blocks",
        block_id.to_string(),
        json!({ "block_type": body.block_type, "position": position }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(DocBlockResponse::from(row))))
}

/// `GET /v1/doc-pages/{page_id}/blocks` — list blocks in a doc page.
///
/// Returns blocks ordered by `(position ASC, id ASC)`.
/// Optional `?parent_block_id=` filter for nested traversal.
/// Authz: `Action::DocsRead`.
///
/// ## Error matrix
///
/// | Condition                    | Status |
/// |------------------------------|--------|
/// | Missing/invalid JWT          | 401    |
/// | Caller not a group member    | 403    |
/// | Page not found / cross-group | 404    |
/// | Happy path                   | 200    |
#[utoipa::path(
    get,
    path = "/v1/doc-pages/{page_id}/blocks",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID."),
        ListDocBlocksQuery,
    ),
    responses(
        (status = 200, description = "Blocks.", body = ListDocBlocksResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Page not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_doc_blocks(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
    Query(params): Query<ListDocBlocksQuery>,
) -> Result<Json<ListDocBlocksResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify page exists (RLS filters cross-group).
    let page_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    if page_exists.is_none() {
        return Err(RestError::NotFound);
    }

    let rows: Vec<DocBlockRow> = match params.parent_block_id {
        Some(parent_id) => sqlx::query_as(
            "SELECT id, page_id, group_id, parent_block_id, position, \
                    block_type, content_jsonb, created_at, updated_at \
             FROM doc_blocks \
             WHERE page_id = $1 \
               AND parent_block_id IS NOT DISTINCT FROM $2::uuid \
             ORDER BY position ASC, id ASC",
        )
        .bind(page_id)
        .bind(parent_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        None => sqlx::query_as(
            "SELECT id, page_id, group_id, parent_block_id, position, \
                    block_type, content_jsonb, created_at, updated_at \
             FROM doc_blocks \
             WHERE page_id = $1 \
             ORDER BY position ASC, id ASC",
        )
        .bind(page_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items: Vec<DocBlockResponse> = rows.into_iter().map(DocBlockResponse::from).collect();
    Ok(Json(ListDocBlocksResponse { items }))
}

/// `PATCH /v1/doc-blocks/{block_id}` — update a doc block.
///
/// Authz: `Action::DocsWrite`. The block must belong to `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                     | Status |
/// |-------------------------------|--------|
/// | Missing/invalid JWT           | 401    |
/// | Caller not a group member     | 403    |
/// | Missing X-Group-Id header     | 400    |
/// | Empty patch                   | 400    |
/// | Invalid block type            | 400    |
/// | Block not found / cross-group | 404    |
/// | Happy path                    | 200    |
#[utoipa::path(
    patch,
    path = "/v1/doc-blocks/{block_id}",
    params(
        ("block_id" = Uuid, Path, description = "Block UUID."),
    ),
    request_body = PatchDocBlockRequest,
    responses(
        (status = 200, description = "Block updated.", body = DocBlockResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Block not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn update_doc_block(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(block_id): Path<Uuid>,
    Json(body): Json<PatchDocBlockRequest>,
) -> Result<Json<DocBlockResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }
    if body.is_empty() {
        return Err(RestError::BadRequest("no fields to update".into()));
    }
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch existing row (RLS filters cross-group).
    let existing: Option<DocBlockRow> = sqlx::query_as(
        "SELECT id, page_id, group_id, parent_block_id, position, \
                block_type, content_jsonb, created_at, updated_at \
         FROM doc_blocks WHERE id = $1",
    )
    .bind(block_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    let current = existing.ok_or(RestError::NotFound)?;

    let new_block_type = body.block_type.as_deref().unwrap_or(&current.block_type);
    let new_content = body.content.as_ref().unwrap_or(&current.content_jsonb);
    let new_position = body.position.unwrap_or(current.position);
    // None → keep current; Some(None) → set NULL; Some(Some(id)) → set to id.
    let new_parent_block_id: Option<Uuid> = body.parent_block_id.unwrap_or(current.parent_block_id);

    // Track which fields changed (PII-safe: names only).
    let mut fields_updated: Vec<&str> = Vec::new();
    if body.block_type.is_some() {
        fields_updated.push("block_type");
    }
    if body.content.is_some() {
        fields_updated.push("content");
    }
    if body.position.is_some() {
        fields_updated.push("position");
    }
    if body.parent_block_id.is_some() {
        fields_updated.push("parent_block_id");
    }

    let row: DocBlockRow = sqlx::query_as(
        "UPDATE doc_blocks \
         SET block_type = $2, content_jsonb = $3, position = $4, \
             parent_block_id = $5, updated_at = now() \
         WHERE id = $1 \
         RETURNING id, page_id, group_id, parent_block_id, position, \
                   block_type, content_jsonb, created_at, updated_at",
    )
    .bind(block_id)
    .bind(new_block_type)
    .bind(new_content)
    .bind(new_position)
    .bind(new_parent_block_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::DocBlockUpdated,
        principal.user_id,
        group_id,
        "doc_blocks",
        block_id.to_string(),
        json!({ "fields_updated": fields_updated }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(DocBlockResponse::from(row)))
}

/// `GET /v1/doc-blocks/{block_id}` — fetch a single doc block by UUID.
///
/// Authz: `Action::DocsRead`. The block must belong to `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                     | Status |
/// |-------------------------------|--------|
/// | Missing/invalid JWT           | 401    |
/// | Caller not a group member     | 403    |
/// | Missing X-Group-Id header     | 400    |
/// | Block not found / cross-group | 404    |
/// | Happy path                    | 200    |
#[utoipa::path(
    get,
    path = "/v1/doc-blocks/{block_id}",
    params(
        ("block_id" = Uuid, Path, description = "Block UUID."),
    ),
    responses(
        (status = 200, description = "Block found.", body = DocBlockResponse),
        (status = 400, description = "Missing X-Group-Id.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Block not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_doc_block(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(block_id): Path<Uuid>,
) -> Result<Json<DocBlockResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<DocBlockRow> = sqlx::query_as(
        "SELECT id, page_id, group_id, parent_block_id, position, \
                block_type, content_jsonb, created_at, updated_at \
         FROM doc_blocks WHERE id = $1",
    )
    .bind(block_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let block = row.ok_or(RestError::NotFound)?;
    Ok(Json(DocBlockResponse::from(block)))
}

/// `DELETE /v1/doc-blocks/{block_id}` — delete a doc block (hard delete).
///
/// Children (`parent_block_id = this id`) have `parent_block_id` set to NULL
/// (via `ON DELETE SET NULL`) — they become root-level blocks, not deleted.
/// Authz: `Action::DocsDelete` (Owner/Admin). Returns 204 on success, 204
/// on already-absent (idempotent).
///
/// ## Error matrix
///
/// | Condition                     | Status |
/// |-------------------------------|--------|
/// | Missing/invalid JWT           | 401    |
/// | Caller is not Owner/Admin     | 403    |
/// | Missing X-Group-Id header     | 400    |
/// | Block not found / cross-group | 204    |
/// | Happy path                    | 204    |
#[utoipa::path(
    delete,
    path = "/v1/doc-blocks/{block_id}",
    params(
        ("block_id" = Uuid, Path, description = "Block UUID."),
    ),
    responses(
        (status = 204, description = "Block deleted or already absent."),
        (status = 400, description = "Missing X-Group-Id.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not Owner/Admin.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_doc_block(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(block_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Capture block_type for audit before deletion.
    let block_type_row: Option<(String,)> =
        sqlx::query_as("SELECT block_type FROM doc_blocks WHERE id = $1")
            .bind(block_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if let Some((block_type,)) = block_type_row {
        sqlx::query("DELETE FROM doc_blocks WHERE id = $1")
            .bind(block_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::DocBlockDeleted,
            principal.user_id,
            group_id,
            "doc_blocks",
            block_id.to_string(),
            json!({ "block_type": block_type }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    }
    // If block not found (cross-group or already absent): idempotent 204.

    Ok(StatusCode::NO_CONTENT)
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_row(id: Uuid, page_id: Uuid, group_id: Uuid, block_type: &str) -> DocBlockRow {
        DocBlockRow {
            id,
            page_id,
            group_id,
            parent_block_id: None,
            position: 1.0,
            block_type: block_type.to_string(),
            content_jsonb: json!({"text": "hello"}),
            created_at: Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn doc_block_response_serializes_type_as_type_key() {
        let row = make_row(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), "paragraph");
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["type"], "paragraph");
        assert!(
            v.get("block_type").is_none(),
            "block_type should not appear in JSON"
        );
    }

    #[test]
    fn doc_block_response_all_fields_present() {
        let id = Uuid::new_v4();
        let page_id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let row = make_row(id, page_id, group_id, "heading");
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], id.to_string());
        assert_eq!(v["page_id"], page_id.to_string());
        assert_eq!(v["group_id"], group_id.to_string());
        assert!(v["parent_block_id"].is_null());
        assert_eq!(v["position"], 1.0f64);
        assert_eq!(v["content"]["text"], "hello");
    }

    #[test]
    fn create_request_rejects_invalid_type() {
        let req = CreateDocBlockRequest {
            block_type: "foobar".to_string(),
            content: None,
            position: None,
            parent_block_id: None,
        };
        assert_eq!(req.validate(), Err("invalid block type"));
    }

    #[test]
    fn create_request_accepts_all_valid_types() {
        for &bt in VALID_BLOCK_TYPES {
            let req = CreateDocBlockRequest {
                block_type: bt.to_string(),
                content: None,
                position: None,
                parent_block_id: None,
            };
            assert!(req.validate().is_ok(), "should accept type: {bt}");
        }
    }

    #[test]
    fn patch_request_is_empty_when_no_fields() {
        let req = PatchDocBlockRequest {
            block_type: None,
            content: None,
            position: None,
            parent_block_id: None,
        };
        assert!(req.is_empty());
    }

    #[test]
    fn patch_request_not_empty_when_content_provided() {
        let req = PatchDocBlockRequest {
            block_type: None,
            content: Some(json!({"text": "updated"})),
            position: None,
            parent_block_id: None,
        };
        assert!(!req.is_empty());
    }

    #[test]
    fn patch_request_rejects_invalid_type() {
        let req = PatchDocBlockRequest {
            block_type: Some("invalid_type".to_string()),
            content: None,
            position: None,
            parent_block_id: None,
        };
        assert_eq!(req.validate(), Err("invalid block type"));
    }

    #[test]
    fn list_doc_blocks_response_empty() {
        let resp = ListDocBlocksResponse { items: vec![] };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn valid_block_types_count() {
        assert_eq!(VALID_BLOCK_TYPES.len(), 13, "expected 13 block types");
    }

    #[test]
    fn get_doc_block_response_from_row_has_all_fields() {
        let id = Uuid::new_v4();
        let page_id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let row = DocBlockRow {
            id,
            page_id,
            group_id,
            parent_block_id: None,
            position: 3.5,
            block_type: "code".to_string(),
            content_jsonb: json!({"language": "rust", "text": "fn main() {}"}),
            created_at: Utc.with_ymd_and_hms(2026, 6, 11, 10, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 6, 11, 11, 0, 0).unwrap(),
        };
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], id.to_string());
        assert_eq!(v["page_id"], page_id.to_string());
        assert_eq!(v["group_id"], group_id.to_string());
        assert!(v["parent_block_id"].is_null());
        assert_eq!(v["position"], 3.5f64);
        assert_eq!(v["type"], "code");
        assert_eq!(v["content"]["language"], "rust");
    }

    #[test]
    fn get_doc_block_response_with_parent_block_id() {
        let id = Uuid::new_v4();
        let parent_id = Uuid::new_v4();
        let row = DocBlockRow {
            id,
            page_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            parent_block_id: Some(parent_id),
            position: 1.0,
            block_type: "paragraph".to_string(),
            content_jsonb: json!({"text": "nested paragraph"}),
            created_at: Utc.with_ymd_and_hms(2026, 6, 11, 10, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 6, 11, 10, 0, 0).unwrap(),
        };
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["parent_block_id"], parent_id.to_string());
        assert!(!v["parent_block_id"].is_null());
    }

    #[test]
    fn get_doc_block_response_type_field_uses_type_key_not_block_type() {
        let row = make_row(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), "todo");
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["type"], "todo");
        assert!(
            v.get("block_type").is_none(),
            "block_type must not appear in serialized JSON"
        );
    }

    #[test]
    fn get_doc_block_response_timestamps_are_iso8601() {
        let row = DocBlockRow {
            id: Uuid::new_v4(),
            page_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            parent_block_id: None,
            position: 2.0,
            block_type: "heading".to_string(),
            content_jsonb: json!({"text": "Hello"}),
            created_at: Utc.with_ymd_and_hms(2026, 6, 11, 9, 30, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 6, 11, 10, 15, 0).unwrap(),
        };
        let resp = DocBlockResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        let created_str = v["created_at"].as_str().unwrap();
        let updated_str = v["updated_at"].as_str().unwrap();
        assert!(
            created_str.contains("2026-06-11"),
            "created_at must contain date"
        );
        assert!(
            updated_str.contains("2026-06-11"),
            "updated_at must contain date"
        );
        assert!(
            created_str.ends_with('Z') || created_str.contains('+'),
            "must be UTC"
        );
    }
}
