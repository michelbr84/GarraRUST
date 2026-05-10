//! `/v1/groups/{group_id}/files`, `/v1/groups/{group_id}/folders`,
//! `DELETE /v1/files/{file_id}`, `GET /v1/files/{file_id}/download`, and
//! `POST /v1/groups/{group_id}/files/{file_id}/versions` handlers
//! (plans 0088-0094, GAR-555/557/559/561/562/564/567, Fase 3.4 files slices 1-7).
//!
//! Endpoints on the `garraia_app` RLS-enforced pool:
//! - `GET /v1/groups/{group_id}/files?folder_id=&cursor=&limit=` — cursor-paginated list
//! - `GET /v1/groups/{group_id}/folders?parent_id=&cursor=&limit=` — cursor-paginated list
//! - `DELETE /v1/files/{file_id}` — idempotent soft-delete
//! - `GET /v1/files/{file_id}/download` — stream current version bytes (plan 0093)
//!
//! ## Tenant-context protocol
//!
//! `files` and `folders` use FORCE RLS via `app.current_group_id` (migration 003).
//! Both RLS vars set via parameterized `set_config` (plan 0056 pattern).
//!
//! ## Cross-group protection
//!
//! For group-path endpoints: `path_group_id` must equal `principal.group_id` → 403.
//! For `DELETE /v1/files/{file_id}` and `GET /v1/files/{file_id}/download`:
//! no group_id in path; RLS filters silently → 404.

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Response;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use garraia_storage::PutOptions;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

// ─── Private row types ────────────────────────────────────────────────────────

/// Row for file list queries.
#[derive(sqlx::FromRow)]
struct FileRow {
    id: Uuid,
    name: String,
    mime_type: String,
    size_bytes: i64,
    current_version: i32,
    total_versions: i32,
    folder_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Row for folder list queries.
#[derive(sqlx::FromRow)]
struct FolderRow {
    id: Uuid,
    name: String,
    parent_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// File metadata returned in list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileSummary {
    pub id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub current_version: i32,
    pub total_versions: i32,
    /// `null` for root-level files (no folder).
    pub folder_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<FileRow> for FileSummary {
    fn from(r: FileRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
            current_version: r.current_version,
            total_versions: r.total_versions,
            folder_id: r.folder_id,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Folder metadata returned in list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderSummary {
    pub id: Uuid,
    pub name: String,
    /// `null` for root folders.
    pub parent_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<FolderRow> for FolderSummary {
    fn from(r: FolderRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            parent_id: r.parent_id,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Response body for `GET /v1/groups/{group_id}/files`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileListResponse {
    pub items: Vec<FileSummary>,
    /// Opaque cursor for the next page. `null` when the list is exhausted.
    pub next_cursor: Option<Uuid>,
}

/// Response body for `GET /v1/groups/{group_id}/folders`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderListResponse {
    pub items: Vec<FolderSummary>,
    /// Opaque cursor for the next page. `null` when the list is exhausted.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/files`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListFilesQuery {
    /// Filter to a specific folder UUID. Omit to list all files in the group.
    pub folder_id: Option<Uuid>,
    /// Cursor UUID (last seen file id). Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Query parameters for `GET /v1/groups/{group_id}/folders`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListFoldersQuery {
    /// Parent folder UUID. Omit (or `null`) to list root-level folders.
    pub parent_id: Option<Uuid>,
    /// Cursor UUID (last seen folder id). Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Query parameters for `GET /v1/groups/{group_id}/files/{file_id}/versions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListFileVersionsQuery {
    /// Cursor: last seen `version` integer (exclusive, descending). Omit for the first page.
    pub cursor: Option<i32>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Private row from `file_versions` (excludes `object_key` and `integrity_hmac`).
#[derive(sqlx::FromRow)]
struct FileVersionRow {
    version: i32,
    size_bytes: i64,
    mime_type: String,
    checksum_sha256: String,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
}

/// One version entry returned by `GET .../versions` (plan 0095, GAR-569).
///
/// `object_key` and `integrity_hmac` are intentionally absent — they are
/// internal storage identifiers (ADR 0004 invariant).
#[derive(Debug, Serialize, ToSchema)]
pub struct FileVersionSummary {
    pub version: i32,
    pub size_bytes: i64,
    pub mime_type: String,
    /// Lowercase hex SHA-256 of the version content.
    pub checksum_sha256: String,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
}

impl From<FileVersionRow> for FileVersionSummary {
    fn from(r: FileVersionRow) -> Self {
        Self {
            version: r.version,
            size_bytes: r.size_bytes,
            mime_type: r.mime_type,
            checksum_sha256: r.checksum_sha256,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
        }
    }
}

/// Response body for `GET /v1/groups/{group_id}/files/{file_id}/versions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileVersionListResponse {
    pub items: Vec<FileVersionSummary>,
    /// Opaque integer cursor for the next page. `null` when list is exhausted.
    pub next_cursor: Option<i32>,
}

/// Body for `PATCH /v1/groups/{group_id}/files/{file_id}` (plan 0089, GAR-557).
///
/// Only `name` is mutable in slice 2. Extra keys are silently ignored
/// per `serde::Deserialize` defaults — we only act on the fields we
/// declare here. Future slices that allow folder moves or settings
/// patches will add fields and be gated by their own validations.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchFileRequest {
    /// New file name. Trimmed before validation. Length must be 1..=500
    /// chars after trim, must not contain `/` or NUL byte.
    pub name: String,
}

/// Body for `POST /v1/groups/{group_id}/folders` (plan 0092, GAR-562).
///
/// `parent_id` is optional — omit (or `null`) for a root-level folder.
/// If set, the parent must exist in the same group and must not be
/// soft-deleted; a 400 is returned when the constraint is violated.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateFolderRequest {
    /// Folder name. Trimmed before validation. Length must be 1..=200
    /// chars after trim (matches DB CHECK on `folders.name`), must not
    /// contain `/` or NUL byte.
    pub name: String,
    /// Optional parent folder UUID. `null` (or omitted) means root-level.
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

/// Body for `PATCH /v1/groups/{group_id}/folders/{folder_id}`
/// (plan 0091, GAR-561).
///
/// Only `name` is mutable in slice 4. Extra keys are silently ignored
/// per `serde::Deserialize` defaults — we only act on the fields we
/// declare here. Folder moves (changing `parent_id`) are out of scope.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchFolderRequest {
    /// New folder name. Trimmed before validation. Length must be
    /// 1..=200 chars after trim (matches DB CHECK on `folders.name`),
    /// must not contain `/` or NUL byte.
    pub name: String,
}

/// Validation error message family. Plain strings — never embed
/// the offending value (it is user-controlled and may include PII).
const ERR_NAME_EMPTY: &str = "name must not be empty after trim";
const ERR_NAME_TOO_LONG: &str = "name exceeds 500 characters";
const ERR_NAME_HAS_SLASH: &str = "name must not contain '/'";
const ERR_NAME_HAS_NUL: &str = "name must not contain NUL byte";

/// Validate a candidate name. Returns the trimmed name on success or
/// a user-safe error string on failure.
fn validate_file_name(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ERR_NAME_EMPTY);
    }
    if trimmed.chars().count() > 500 {
        return Err(ERR_NAME_TOO_LONG);
    }
    if trimmed.contains('/') {
        return Err(ERR_NAME_HAS_SLASH);
    }
    if trimmed.contains('\0') {
        return Err(ERR_NAME_HAS_NUL);
    }
    Ok(trimmed.to_string())
}

// Folder validation has identical shape to file validation but a tighter
// length cap (200 chars vs 500) — matches the DB CHECK on `folders.name`
// at `migrations/003_files_and_folders.sql:59`. The only divergence
// rationale: file names may legitimately encode long descriptive paths
// inside the user's mental model (e.g. "2026-Q3 retrospective notes —
// final v3.pdf"), while folder names tend to be short tags.
const ERR_FOLDER_NAME_EMPTY: &str = "name must not be empty after trim";
const ERR_FOLDER_NAME_TOO_LONG: &str = "name exceeds 200 characters";
const ERR_FOLDER_NAME_HAS_SLASH: &str = "name must not contain '/'";
const ERR_FOLDER_NAME_HAS_NUL: &str = "name must not contain NUL byte";

/// Validate a candidate folder name. Returns the trimmed name on success
/// or a user-safe error string on failure (no PII echoed).
fn validate_folder_name(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ERR_FOLDER_NAME_EMPTY);
    }
    if trimmed.chars().count() > 200 {
        return Err(ERR_FOLDER_NAME_TOO_LONG);
    }
    if trimmed.contains('/') {
        return Err(ERR_FOLDER_NAME_HAS_SLASH);
    }
    if trimmed.contains('\0') {
        return Err(ERR_FOLDER_NAME_HAS_NUL);
    }
    Ok(trimmed.to_string())
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Set RLS GUCs for the transaction (plan 0056 pattern).
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

/// Extract and validate `principal.group_id`. Returns 400 if missing.
fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

/// Verify path group_id matches principal's group. Returns 403 on mismatch.
fn check_group_match(path_group_id: Uuid, principal_group_id: Uuid) -> Result<(), RestError> {
    if path_group_id != principal_group_id {
        return Err(RestError::Forbidden);
    }
    Ok(())
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/groups/{group_id}/files` — cursor-paginated file listing.
///
/// When `folder_id` is supplied, only files directly inside that folder are
/// returned. When omitted, all non-deleted files in the group are returned.
/// Ordering: `(created_at DESC, id DESC)`.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Not a group member                 | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/files",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListFilesQuery,
    ),
    responses(
        (status = 200, description = "File list.", body = FileListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_files(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListFilesQuery>,
) -> Result<Json<FileListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<FileRow> = match params.cursor {
        Some(cursor_id) => sqlx::query_as(
            "SELECT f.id, f.name, f.mime_type, f.size_bytes, \
                         f.current_version, f.total_versions, f.folder_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM files f \
                  WHERE f.deleted_at IS NULL \
                    AND ($1::uuid IS NULL OR f.folder_id = $1::uuid) \
                    AND (f.created_at, f.id) < ( \
                        SELECT created_at, id FROM files WHERE id = $2 \
                    ) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $3",
        )
        .bind(params.folder_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        None => sqlx::query_as(
            "SELECT f.id, f.name, f.mime_type, f.size_bytes, \
                         f.current_version, f.total_versions, f.folder_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM files f \
                  WHERE f.deleted_at IS NULL \
                    AND ($1::uuid IS NULL OR f.folder_id = $1::uuid) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $2",
        )
        .bind(params.folder_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_next = rows.len() > effective_limit as usize;
    let mut items: Vec<FileSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(FileSummary::from)
        .collect();
    let next_cursor = if has_next {
        items.last().map(|f| f.id)
    } else {
        None
    };
    if has_next {
        items.pop();
    }

    Ok(Json(FileListResponse { items, next_cursor }))
}

/// `GET /v1/groups/{group_id}/folders` — cursor-paginated folder listing.
///
/// When `parent_id` is supplied, only direct children of that folder are
/// returned. When omitted, only root-level folders (`parent_id IS NULL`)
/// are returned. Ordering: `(created_at DESC, id DESC)`.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Not a group member                 | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/folders",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListFoldersQuery,
    ),
    responses(
        (status = 200, description = "Folder list.", body = FolderListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_folders(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListFoldersQuery>,
) -> Result<Json<FolderListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<FolderRow> = match params.cursor {
        Some(cursor_id) => sqlx::query_as(
            "SELECT f.id, f.name, f.parent_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM folders f \
                  WHERE f.deleted_at IS NULL \
                    AND f.parent_id IS NOT DISTINCT FROM $1::uuid \
                    AND (f.created_at, f.id) < ( \
                        SELECT created_at, id FROM folders WHERE id = $2 \
                    ) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $3",
        )
        .bind(params.parent_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        None => sqlx::query_as(
            "SELECT f.id, f.name, f.parent_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM folders f \
                  WHERE f.deleted_at IS NULL \
                    AND f.parent_id IS NOT DISTINCT FROM $1::uuid \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $2",
        )
        .bind(params.parent_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_next = rows.len() > effective_limit as usize;
    let mut items: Vec<FolderSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(FolderSummary::from)
        .collect();
    let next_cursor = if has_next {
        items.last().map(|f| f.id)
    } else {
        None
    };
    if has_next {
        items.pop();
    }

    Ok(Json(FolderListResponse { items, next_cursor }))
}

/// `DELETE /v1/files/{file_id}` — idempotent soft-delete.
///
/// Sets `deleted_at = now()` on the file. Idempotent: if the file is already
/// soft-deleted, returns 204 without emitting an audit event. Cross-group
/// access is rejected by RLS (the file is invisible) → 404.
///
/// Authz: `Action::FilesDelete`.
///
/// ## Error matrix
///
/// | Condition                           | Status |
/// |-------------------------------------|--------|
/// | Missing/invalid JWT                 | 401    |
/// | Not a group member                  | 403    |
/// | Insufficient role (< Member)        | 403    |
/// | File not found / cross-tenant       | 404    |
/// | File already soft-deleted           | 204    |
/// | Happy path                          | 204    |
#[utoipa::path(
    delete,
    path = "/v1/files/{file_id}",
    params(
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    responses(
        (status = 204, description = "File deleted (or already deleted)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or lacks FilesDelete.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(file_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::FilesDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch the file regardless of deleted_at to distinguish:
    //   - not found / cross-group (0 rows) → 404
    //   - already deleted (deleted_at IS NOT NULL) → 204 idempotent, no audit
    //   - live file (deleted_at IS NULL) → UPDATE + audit + 204
    let existing: Option<(Option<DateTime<Utc>>, String)> =
        sqlx::query_as("SELECT deleted_at, name FROM files WHERE id = $1 AND group_id = $2")
            .bind(file_id)
            .bind(group_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let (deleted_at, name) = match existing {
        Some(row) => row,
        None => return Err(RestError::NotFound),
    };

    if deleted_at.is_some() {
        // Already deleted — idempotent 204 without audit.
        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let name_len = name.chars().count();

    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileDeleted,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({ "name_len": name_len, "group_id": group_id }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Private row returned by the download lookup query.
#[derive(sqlx::FromRow)]
struct DownloadRow {
    name: String,
    object_key: String,
    mime_type: String,
}

/// `GET /v1/files/{file_id}/download` — stream the current file version
/// (plan 0093, GAR-564, Fase 3.4 files slice 6).
///
/// Looks up the current `file_versions.object_key` via RLS-enforced JOIN,
/// then reads the bytes from the configured `ObjectStore`. Returns a 200
/// response with `Content-Type` and `Content-Disposition: attachment` so
/// browsers offer a Save dialog. The raw filename is NOT reflected in any
/// response header (PII-safe); clients use `GET .../files/{id}` for that.
///
/// Auth: `Action::FilesRead`. `X-Group-Id` header required for RLS context.
/// Cross-group attempts are silently 404 via RLS filtering.
///
/// ## Error matrix
///
/// | Condition                              | Status |
/// |----------------------------------------|--------|
/// | Missing/invalid JWT                    | 401    |
/// | Insufficient role (no `FilesRead`)     | 403    |
/// | Missing `X-Group-Id` header            | 400    |
/// | File not found / deleted / cross-group | 404    |
/// | `ObjectStore` not configured           | 503    |
/// | Object missing from storage backend    | 404    |
/// | Happy path                             | 200    |
#[utoipa::path(
    get,
    path = "/v1/files/{file_id}/download",
    params(
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    responses(
        (status = 200, description = "File bytes streamed with Content-Type and Content-Disposition."),
        (status = 400, description = "Missing X-Group-Id header.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesRead.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found, deleted, or cross-group.", body = super::problem::ProblemDetails),
        (status = 503, description = "Object store not configured.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn download_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(file_id): Path<Uuid>,
) -> Result<Response, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<DownloadRow> = sqlx::query_as(
        "SELECT f.name, fv.object_key, fv.mime_type \
         FROM   files f \
         JOIN   file_versions fv \
                ON  fv.file_id  = f.id \
                AND fv.version  = f.current_version \
                AND fv.group_id = f.group_id \
         WHERE  f.id         = $1 \
           AND  f.deleted_at IS NULL",
    )
    .bind(file_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    let filename_len = row.name.chars().count();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileDownloadIssued,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({ "file_id": file_id, "group_id": group_id, "filename_len": filename_len }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let object_store = state
        .storage
        .object_store
        .as_ref()
        .ok_or(RestError::AuthUnconfigured)?
        .clone();

    let result = object_store
        .get(&row.object_key)
        .await
        .map_err(|e| match e {
            garraia_storage::StorageError::NotFound { .. } => RestError::NotFound,
            other => RestError::Internal(anyhow::anyhow!(other)),
        })?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", row.mime_type)
        .header("content-disposition", "attachment; filename=\"download\"")
        .header("content-length", result.metadata.size_bytes.to_string())
        .body(Body::from(result.bytes))
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    Ok(response)
}

// ─── Slice 7: new file version ───────────────────────────────────────────────

/// Response body for a successful new-version upload.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileVersionResponse {
    /// UUID of the logical file (unchanged across versions).
    pub file_id: Uuid,
    /// The version number of the newly created version.
    pub version: i32,
    /// Byte count of the new version's content.
    pub size_bytes: u64,
    /// MIME type of the new version.
    pub mime_type: String,
    /// UTC timestamp of the version creation (approximate — set after commit).
    pub created_at: DateTime<Utc>,
}

/// `POST /v1/groups/{group_id}/files/{file_id}/versions` — upload a new content
/// version for an existing file (plan 0094, GAR-567, Fase 3.4 files slice 7).
///
/// Accepts raw bytes in the request body (capped to `storage.max_patch_bytes`,
/// default 100 MiB). Stores the bytes in the configured `ObjectStore`, inserts
/// a new `file_versions` row, and bumps `files.current_version` + related
/// counters atomically in one Postgres transaction.
///
/// Auth: `Action::FilesWrite`. `X-Group-Id` header required for RLS context.
/// `path_group_id` must match `principal.group_id`.
///
/// ## Two-phase commit ordering
///
/// `ObjectStore::put` runs before the Postgres COMMIT. If the commit fails
/// after the object write, the blob is orphaned — acceptable per plan 0044
/// §5.3.1 (future maintenance job reclaims orphaned blobs).
///
/// ## Error matrix
///
/// | Condition                              | Status |
/// |----------------------------------------|--------|
/// | Missing/invalid JWT                    | 401    |
/// | Insufficient role (no `FilesWrite`)    | 403    |
/// | `path_group_id` ≠ `principal.group_id` | 403    |
/// | Missing `X-Group-Id` header            | 400    |
/// | File not found / deleted               | 404    |
/// | MIME type not in allow-list            | 415    |
/// | Body exceeds cap                       | 413    |
/// | ObjectStore not configured             | 503    |
/// | Happy path                             | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/files/{file_id}/versions",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID (must match caller's group)."),
        ("file_id"  = Uuid, Path, description = "File UUID to create a new version for."),
    ),
    request_body(
        content = (),
        description = "Raw file bytes. Set Content-Type to the MIME type of the new version.",
    ),
    responses(
        (status = 201, description = "New version created.", body = FileVersionResponse),
        (status = 400, description = "Missing X-Group-Id header.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesWrite or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found or soft-deleted.", body = super::problem::ProblemDetails),
        (status = 413, description = "Body exceeds operator cap.", body = super::problem::ProblemDetails),
        (status = 415, description = "MIME type not in allow-list.", body = super::problem::ProblemDetails),
        (status = 503, description = "Object store not configured.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn post_new_version(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, file_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<FileVersionResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::FilesWrite) {
        return Err(RestError::Forbidden);
    }
    check_group_match(path_group_id, group_id)?;

    let object_store = state
        .storage
        .object_store
        .as_ref()
        .ok_or(RestError::AuthUnconfigured)?
        .clone();
    let staging = state
        .storage
        .upload_staging
        .as_ref()
        .ok_or(RestError::AuthUnconfigured)?
        .clone();

    // Validate MIME type before reading body (fail fast — 415 before 413).
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_owned())
        .ok_or(RestError::UnsupportedMediaType(
            "Content-Type header missing".into(),
        ))?;
    if !garraia_storage::mime_allowlist::is_mime_allowed(&content_type) {
        return Err(RestError::UnsupportedMediaType(format!(
            "MIME type '{content_type}' is not in the allow-list"
        )));
    }

    // Read body with operator cap.
    let cap: usize = staging.max_patch_bytes.try_into().unwrap_or(usize::MAX);
    let bytes = to_bytes(body, cap).await.map_err(|e| {
        tracing::debug!(error = %e, "new-version body too large or read failed");
        RestError::PayloadTooLarge(format!(
            "body exceeds operator cap of {} bytes",
            staging.max_patch_bytes
        ))
    })?;
    let body_len = bytes.len() as i64;
    let checksum_sha256 = sha256_hex_of_bytes(&bytes);

    // DB transaction: lock the row + verify the file exists.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<(i32, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT current_version, created_by, created_by_label \
         FROM   files \
         WHERE  id         = $1 \
           AND  deleted_at IS NULL \
         FOR UPDATE",
    )
    .bind(file_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (current_version, _file_created_by, creator_label) = match row {
        Some(r) => r,
        None => return Err(RestError::NotFound),
    };

    let new_version = current_version + 1;
    let version_uuid = Uuid::new_v4();
    let object_key = format!("groups/{group_id}/files/{file_id}/v{new_version}/{version_uuid}");

    // ObjectStore PUT — runs before Postgres COMMIT (two-phase ordering).
    let put_opts = PutOptions {
        content_type: Some(content_type.clone()),
        hmac_secret: Some(staging.hmac_secret.clone()),
        ..Default::default()
    };
    let put_meta = object_store
        .put(&object_key, bytes, put_opts)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "ObjectStore::put failed for new file version");
            RestError::BadGateway("object store write failed; please retry".into())
        })?;

    let integrity_hmac = put_meta.integrity_hmac.ok_or_else(|| {
        RestError::Internal(anyhow::anyhow!(
            "ObjectStore did not return integrity_hmac; GARRAIA_UPLOAD_HMAC_SECRET misconfigured"
        ))
    })?;

    sqlx::query(
        "INSERT INTO file_versions \
            (file_id, group_id, version, object_key, etag, checksum_sha256, \
             integrity_hmac, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(new_version)
    .bind(&object_key)
    .bind(&put_meta.etag_sha256[..put_meta.etag_sha256.len().min(200)])
    .bind(&checksum_sha256)
    .bind(&integrity_hmac)
    .bind(body_len)
    .bind(&content_type)
    .bind(principal.user_id)
    .bind(&creator_label)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("insert file_versions")))?;

    sqlx::query(
        "UPDATE files \
         SET current_version = $1, \
             total_versions  = total_versions + 1, \
             size_bytes      = $2, \
             mime_type       = $3, \
             updated_at      = now() \
         WHERE id = $4",
    )
    .bind(new_version)
    .bind(body_len)
    .bind(&content_type)
    .bind(file_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("update files")))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileVersionCreated,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({
            "file_id": file_id,
            "group_id": group_id,
            "new_version": new_version,
            "size_bytes": body_len,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(FileVersionResponse {
            file_id,
            version: new_version,
            size_bytes: body_len as u64,
            mime_type: content_type,
            created_at: Utc::now(),
        }),
    ))
}

fn sha256_hex_of_bytes(input: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(input);
    hex::encode(digest)
}

/// `PATCH /v1/groups/{group_id}/files/{file_id}` — rename a file
/// (plan 0089, GAR-557, Fase 3.4 files slice 2).
///
/// Only the `name` field may change. Returns the updated `FileSummary`
/// (200) on success.
///
/// Authz: `Action::FilesWrite`.
///
/// ## Error matrix
///
/// | Condition                                    | Status |
/// |----------------------------------------------|--------|
/// | Missing/invalid JWT                          | 401    |
/// | Path group_id ≠ principal group_id           | 403    |
/// | Caller lacks `FilesWrite`                    | 403    |
/// | Body name empty/too long/has '/' or NUL      | 400    |
/// | File not found, soft-deleted, or cross-group | 404    |
/// | Happy path                                   | 200    |
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/files/{file_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    request_body = PatchFileRequest,
    responses(
        (status = 200, description = "File renamed.", body = FileSummary),
        (status = 400, description = "Invalid name.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesWrite or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found, soft-deleted, or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, file_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchFileRequest>,
) -> Result<Json<FileSummary>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesWrite) {
        return Err(RestError::Forbidden);
    }

    let new_name =
        validate_file_name(&body.name).map_err(|msg| RestError::BadRequest(msg.into()))?;
    let name_len = new_name.chars().count();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<FileRow> = sqlx::query_as(
        "UPDATE files \
         SET name = $1, updated_at = now() \
         WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL \
         RETURNING id, name, mime_type, size_bytes, current_version, \
                   total_versions, folder_id, created_by, created_by_label, \
                   created_at, updated_at",
    )
    .bind(&new_name)
    .bind(file_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileRenamed,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({ "name_len": name_len, "group_id": group_id }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(FileSummary::from(row)))
}

/// `GET /v1/groups/{group_id}/files/{file_id}` — return a single file's metadata.
///
/// Returns 404 when the file is soft-deleted, cross-group, or not found.
/// RLS ensures cross-group files are invisible regardless.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                                    | Status |
/// |----------------------------------------------|--------|
/// | Missing/invalid JWT                          | 401    |
/// | Path group_id ≠ principal group_id           | 403    |
/// | Caller lacks `FilesRead`                     | 403    |
/// | File not found, soft-deleted, or cross-group | 404    |
/// | Happy path                                   | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/files/{file_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    responses(
        (status = 200, description = "File metadata.", body = FileSummary),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesRead or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found, soft-deleted, or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, file_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<FileSummary>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<FileRow> = sqlx::query_as(
        "SELECT id, name, mime_type, size_bytes, current_version, total_versions, \
                folder_id, created_by, created_by_label, created_at, updated_at \
         FROM files \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(file_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;
    Ok(Json(FileSummary::from(row)))
}

/// `GET /v1/groups/{group_id}/folders/{folder_id}` — return a single folder's metadata.
///
/// Returns 404 when the folder is soft-deleted, cross-group, or not found.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Path group_id ≠ principal group_id             | 403    |
/// | Caller lacks `FilesRead`                       | 403    |
/// | Folder not found, soft-deleted, or cross-group | 404    |
/// | Happy path                                     | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/folders/{folder_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("folder_id" = Uuid, Path, description = "Folder UUID."),
    ),
    responses(
        (status = 200, description = "Folder metadata.", body = FolderSummary),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesRead or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Folder not found, soft-deleted, or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_folder(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, folder_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<FolderSummary>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<FolderRow> = sqlx::query_as(
        "SELECT id, name, parent_id, created_by, created_by_label, created_at, updated_at \
         FROM folders \
         WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
    )
    .bind(folder_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;
    Ok(Json(FolderSummary::from(row)))
}

/// `PATCH /v1/groups/{group_id}/folders/{folder_id}` — rename a folder
/// (plan 0091, GAR-561, Fase 3.4 files slice 4).
///
/// Only the `name` field may change. Returns the updated `FolderSummary`
/// (200) on success.
///
/// Authz: `Action::FilesWrite`. The `can()` matrix already grants this to
/// Owner/Admin/Member; no separate `FoldersWrite` action exists in the
/// 22-action enum and folder mutations are intentionally gated by the
/// same capability as file mutations (mirrors plan 0089).
///
/// Concurrency note: `folders_unique_name_per_parent_idx` enforces unique
/// `(group_id, COALESCE(parent_id, nil_uuid), name)` for non-deleted rows.
/// A rename to a name already taken by a sibling under the same parent
/// raises Postgres `23505`, which this handler maps to `409 Conflict`
/// with a PII-safe detail (no echo of the conflicting name). Without this
/// branch the same condition would surface as 5xx — wrong UX for a
/// user-recoverable error.
///
/// ## Error matrix
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Missing/invalid JWT                            | 401    |
/// | Path group_id ≠ principal group_id             | 403    |
/// | Caller lacks `FilesWrite`                      | 403    |
/// | Body name empty/too long/has '/' or NUL        | 400    |
/// | Folder not found, soft-deleted, or cross-group | 404    |
/// | Name collides with sibling under same parent   | 409    |
/// | Happy path                                     | 200    |
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/folders/{folder_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("folder_id" = Uuid, Path, description = "Folder UUID."),
    ),
    request_body = PatchFolderRequest,
    responses(
        (status = 200, description = "Folder renamed.", body = FolderSummary),
        (status = 400, description = "Invalid name.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesWrite or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Folder not found, soft-deleted, or cross-group.", body = super::problem::ProblemDetails),
        (status = 409, description = "A sibling folder under the same parent already uses this name.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_folder(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, folder_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchFolderRequest>,
) -> Result<Json<FolderSummary>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesWrite) {
        return Err(RestError::Forbidden);
    }

    let new_name =
        validate_folder_name(&body.name).map_err(|msg| RestError::BadRequest(msg.into()))?;
    let name_len = new_name.chars().count();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let update_result: Result<Option<FolderRow>, sqlx::Error> = sqlx::query_as(
        "UPDATE folders \
         SET name = $1, updated_at = now() \
         WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL \
         RETURNING id, name, parent_id, created_by, created_by_label, \
                   created_at, updated_at",
    )
    .bind(&new_name)
    .bind(folder_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await;

    let row = match update_result {
        Ok(Some(r)) => r,
        Ok(None) => return Err(RestError::NotFound),
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            // UNIQUE collision on `folders_unique_name_per_parent_idx`.
            // PII-safe detail: never echo the conflicting name back.
            return Err(RestError::Conflict(
                "a folder with this name already exists under the same parent".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    };

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FolderRenamed,
        principal.user_id,
        group_id,
        "folders",
        folder_id.to_string(),
        json!({
            "folder_id": folder_id,
            "group_id": group_id,
            "name_len": name_len,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(FolderSummary::from(row)))
}

/// `POST /v1/groups/{group_id}/folders` — create a folder (plan 0092, GAR-562).
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Auth missing / invalid                         | 401    |
/// | Principal group_id ≠ path group_id             | 403    |
/// | Caller lacks FilesWrite                        | 403    |
/// | Invalid name (empty/long/slash/NUL)            | 400    |
/// | `parent_id` not found or soft-deleted in group | 400    |
/// | Name collision under same parent               | 409    |
/// | Happy path                                     | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/folders",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
    ),
    request_body = CreateFolderRequest,
    responses(
        (status = 201, description = "Folder created.", body = FolderSummary),
        (status = 400, description = "Invalid name or unknown parent_id.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesWrite or group mismatch.", body = super::problem::ProblemDetails),
        (status = 409, description = "A sibling folder under the same parent already uses this name.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_folder(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Json(body): Json<CreateFolderRequest>,
) -> Result<(StatusCode, Json<FolderSummary>), RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesWrite) {
        return Err(RestError::Forbidden);
    }

    let name = validate_folder_name(&body.name).map_err(|msg| RestError::BadRequest(msg.into()))?;
    let name_len = name.chars().count();
    let has_parent = body.parent_id.is_some();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Validate parent_id belongs to the same group and is not soft-deleted.
    if let Some(parent_id) = body.parent_id {
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM folders \
             WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL",
        )
        .bind(parent_id)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if exists.is_none() {
            return Err(RestError::BadRequest(
                "parent_id not found or soft-deleted in this group".into(),
            ));
        }
    }

    // Resolve created_by_label from the users table within the same transaction.
    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let folder_id = Uuid::new_v4();

    let insert_result: Result<FolderRow, sqlx::Error> = sqlx::query_as(
        "INSERT INTO folders \
             (id, group_id, parent_id, name, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, name, parent_id, created_by, created_by_label, \
                   created_at, updated_at",
    )
    .bind(folder_id)
    .bind(group_id)
    .bind(body.parent_id)
    .bind(&name)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await;

    let row = match insert_result {
        Ok(r) => r,
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "a folder with this name already exists under the same parent".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    };

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FolderCreated,
        principal.user_id,
        group_id,
        "folders",
        folder_id.to_string(),
        json!({
            "folder_id": folder_id,
            "group_id": group_id,
            "name_len": name_len,
            "has_parent": has_parent,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(FolderSummary::from(row))))
}

/// `DELETE /v1/groups/{group_id}/folders/{folder_id}` — soft-delete a folder
/// (plan 0092, GAR-562).
///
/// Idempotent: already-deleted folders return 204 without emitting an audit
/// event (mirrors `DELETE /v1/files/{file_id}` from plan 0088). Children
/// files and sub-folders are NOT cascade-deleted — they become orphans
/// visible at the group root. Cascade semantics are deferred to slice 6+.
///
/// Authz: `Action::FilesDelete` — same gate as `DELETE /v1/files/{file_id}`
/// (plan 0088). The `can()` matrix grants this only to Owner/Admin (NOT
/// Member), which is the canonical project convention for destructive
/// file/folder operations: `FilesWrite` is for create/rename mutations
/// that are reversible (PATCH rename can be undone, soft-deleted folders
/// require Admin to delete).
///
/// | Condition                                      | Status |
/// |------------------------------------------------|--------|
/// | Auth missing / invalid                         | 401    |
/// | Principal group_id ≠ path group_id             | 403    |
/// | Caller lacks `FilesDelete` (e.g. Member role)  | 403    |
/// | Folder not found or cross-group                | 404    |
/// | Already deleted (idempotent)                   | 204    |
/// | Happy path (live folder deleted)               | 204    |
#[utoipa::path(
    delete,
    path = "/v1/groups/{group_id}/folders/{folder_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("folder_id" = Uuid, Path, description = "Folder UUID."),
    ),
    responses(
        (status = 204, description = "Folder deleted (or already deleted)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesDelete or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Folder not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_folder(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, folder_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch the folder regardless of deleted_at to distinguish:
    //   - not found / cross-group (0 rows) → 404
    //   - already deleted                  → 204 idempotent, no audit
    //   - live folder                      → UPDATE + audit + 204
    let existing: Option<(Option<DateTime<Utc>>, String)> =
        sqlx::query_as("SELECT deleted_at, name FROM folders WHERE id = $1 AND group_id = $2")
            .bind(folder_id)
            .bind(group_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let (deleted_at, name) = match existing {
        Some(row) => row,
        None => return Err(RestError::NotFound),
    };

    if deleted_at.is_some() {
        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let name_len = name.chars().count();

    sqlx::query("UPDATE folders SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(folder_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FolderDeleted,
        principal.user_id,
        group_id,
        "folders",
        folder_id.to_string(),
        json!({
            "folder_id": folder_id,
            "group_id": group_id,
            "name_len": name_len,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/groups/{group_id}/files/{file_id}/versions` — paginated version list
/// (plan 0095, GAR-569). Returns versions newest-first (version DESC). Cursor is
/// the last seen `version` integer (exclusive).
///
/// Auth: `Principal` + `X-Group-Id` header + `FilesRead` action.
/// Cross-group guard: `path_group_id` must equal `principal.group_id` → 403.
/// Non-existent or soft-deleted file → 404.
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/files/{file_id}/versions",
    tag = "files",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID"),
        ("file_id"  = Uuid, Path, description = "File UUID"),
        ListFileVersionsQuery,
    ),
    responses(
        (status = 200, description = "Version list", body = FileVersionListResponse),
        (status = 400, description = "Missing X-Group-Id header"),
        (status = 403, description = "Forbidden — cross-group or insufficient role"),
        (status = 404, description = "File not found or soft-deleted"),
    ),
    security(("bearer" = [])),
)]
pub async fn list_file_versions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, file_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListFileVersionsQuery>,
) -> Result<Json<FileVersionListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;

    if path_group_id != group_id {
        return Err(RestError::Forbidden);
    }

    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify parent file exists and is not soft-deleted.
    let exists: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(file_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Paginated version list — cursor is exclusive upper bound on version (DESC).
    let rows: Vec<FileVersionRow> = sqlx::query_as(
        "SELECT version, size_bytes, mime_type, checksum_sha256, \
                created_by, created_by_label, created_at \
         FROM   file_versions \
         WHERE  file_id  = $1 \
           AND  group_id = $2 \
           AND  ($3::int IS NULL OR version < $3) \
         ORDER  BY version DESC \
         LIMIT  $4",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(q.cursor)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() > limit as usize;
    let mut rows = rows;
    if has_more {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        rows.last().map(|r| r.version)
    } else {
        None
    };
    let version_count = rows.len();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileVersionsListed,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({
            "file_id": file_id,
            "group_id": group_id,
            "version_count": version_count,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items: Vec<FileVersionSummary> = rows.into_iter().map(FileVersionSummary::from).collect();
    Ok(Json(FileVersionListResponse { items, next_cursor }))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_summary_from_row_preserves_fields() {
        let now = Utc::now();
        let file_id = Uuid::new_v4();
        let folder_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let row = FileRow {
            id: file_id,
            name: "report.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 12345,
            current_version: 2,
            total_versions: 3,
            folder_id: Some(folder_id),
            created_by: Some(user_id),
            created_by_label: "Alice".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert_eq!(summary.id, file_id);
        assert_eq!(summary.name, "report.pdf");
        assert_eq!(summary.mime_type, "application/pdf");
        assert_eq!(summary.size_bytes, 12345);
        assert_eq!(summary.current_version, 2);
        assert_eq!(summary.total_versions, 3);
        assert_eq!(summary.folder_id, Some(folder_id));
        assert_eq!(summary.created_by_label, "Alice");
    }

    #[test]
    fn folder_summary_from_row_preserves_fields() {
        let now = Utc::now();
        let folder_id = Uuid::new_v4();
        let parent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let row = FolderRow {
            id: folder_id,
            name: "Documents".to_string(),
            parent_id: Some(parent_id),
            created_by: Some(user_id),
            created_by_label: "Bob".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FolderSummary::from(row);
        assert_eq!(summary.id, folder_id);
        assert_eq!(summary.name, "Documents");
        assert_eq!(summary.parent_id, Some(parent_id));
        assert_eq!(summary.created_by_label, "Bob");
    }

    #[test]
    fn file_summary_null_folder_id() {
        let now = Utc::now();
        let row = FileRow {
            id: Uuid::new_v4(),
            name: "root_file.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "System".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert!(summary.folder_id.is_none());
        assert!(summary.created_by.is_none());
    }

    #[test]
    fn folder_summary_root_has_null_parent() {
        let now = Utc::now();
        let row = FolderRow {
            id: Uuid::new_v4(),
            name: "Root".to_string(),
            parent_id: None,
            created_by: None,
            created_by_label: "System".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FolderSummary::from(row);
        assert!(summary.parent_id.is_none());
    }

    #[test]
    fn file_list_response_empty() {
        let resp = FileListResponse {
            items: vec![],
            next_cursor: None,
        };
        assert!(resp.items.is_empty());
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn folder_list_response_empty() {
        let resp = FolderListResponse {
            items: vec![],
            next_cursor: None,
        };
        assert!(resp.items.is_empty());
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn list_files_query_defaults() {
        let q = ListFilesQuery {
            folder_id: None,
            cursor: None,
            limit: None,
        };
        assert!(q.folder_id.is_none());
        assert!(q.cursor.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn list_folders_query_defaults() {
        let q = ListFoldersQuery {
            parent_id: None,
            cursor: None,
            limit: None,
        };
        assert!(q.parent_id.is_none());
        assert!(q.cursor.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn pagination_has_next_detection() {
        let limit: u32 = 2;
        // Simulate 3 rows returned for fetch_limit = limit + 1 = 3.
        let now = Utc::now();
        let make_row = |n: u8| FileRow {
            id: Uuid::new_v4(),
            name: format!("file{n}.txt"),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let rows: Vec<FileRow> = (0..3).map(make_row).collect();
        let has_next = rows.len() > limit as usize;
        assert!(has_next, "3 rows > limit 2 → has_next");
        let truncated: Vec<FileSummary> = rows
            .into_iter()
            .take(limit as usize)
            .map(FileSummary::from)
            .collect();
        assert_eq!(truncated.len(), 2);
    }

    #[test]
    fn pagination_no_next_when_at_limit() {
        let limit: u32 = 2;
        // Exactly 2 rows → no next page.
        let now = Utc::now();
        let make_row = |n: u8| FileRow {
            id: Uuid::new_v4(),
            name: format!("file{n}.txt"),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let rows: Vec<FileRow> = (0..2).map(make_row).collect();
        let has_next = rows.len() > limit as usize;
        assert!(!has_next, "2 rows == limit 2 → no next");
    }

    #[test]
    fn require_group_id_returns_err_when_none() {
        let principal = Principal {
            user_id: Uuid::new_v4(),
            group_id: None,
            role: None,
        };
        let result = require_group_id(&principal);
        assert!(result.is_err());
    }

    #[test]
    fn require_group_id_returns_ok_when_present() {
        let gid = Uuid::new_v4();
        let principal = Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(gid),
            role: None,
        };
        let result = require_group_id(&principal);
        assert_eq!(result.unwrap(), gid);
    }

    #[test]
    fn check_group_match_ok_when_equal() {
        let id = Uuid::new_v4();
        assert!(check_group_match(id, id).is_ok());
    }

    #[test]
    fn check_group_match_err_when_different() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(check_group_match(a, b).is_err());
    }

    // ─── PATCH validation helpers (plan 0089, GAR-557) ────────────────

    #[test]
    fn validate_file_name_happy_path_trims() {
        let out = validate_file_name("  report.pdf  ").expect("trim+accept");
        assert_eq!(out, "report.pdf");
    }

    #[test]
    fn validate_file_name_rejects_empty_after_trim() {
        let err = validate_file_name("   ").expect_err("empty");
        assert_eq!(err, ERR_NAME_EMPTY);
    }

    #[test]
    fn validate_file_name_rejects_zero_length() {
        let err = validate_file_name("").expect_err("empty literal");
        assert_eq!(err, ERR_NAME_EMPTY);
    }

    #[test]
    fn validate_file_name_accepts_500_chars() {
        // Boundary: exactly 500 chars passes.
        let name: String = "a".repeat(500);
        let out = validate_file_name(&name).expect("500 ok");
        assert_eq!(out.chars().count(), 500);
    }

    #[test]
    fn validate_file_name_rejects_501_chars() {
        let name: String = "a".repeat(501);
        let err = validate_file_name(&name).expect_err("too long");
        assert_eq!(err, ERR_NAME_TOO_LONG);
    }

    #[test]
    fn validate_file_name_rejects_slash() {
        let err = validate_file_name("dir/file.txt").expect_err("slash");
        assert_eq!(err, ERR_NAME_HAS_SLASH);
    }

    #[test]
    fn validate_file_name_rejects_nul_byte() {
        let err = validate_file_name("foo\0bar").expect_err("nul");
        assert_eq!(err, ERR_NAME_HAS_NUL);
    }

    #[test]
    fn validate_file_name_accepts_unicode() {
        // Multi-byte UTF-8 chars should count as chars, not bytes.
        let name = "café-relatório.pdf";
        let out = validate_file_name(name).expect("utf8");
        assert_eq!(out, name);
    }

    // ─── PATCH folder validation helpers (plan 0091, GAR-561) ──────────

    #[test]
    fn validate_folder_name_happy_path_trims() {
        let out = validate_folder_name("  Documents  ").expect("trim+accept");
        assert_eq!(out, "Documents");
    }

    #[test]
    fn validate_folder_name_rejects_empty_after_trim() {
        let err = validate_folder_name("   ").expect_err("empty");
        assert_eq!(err, ERR_FOLDER_NAME_EMPTY);
    }

    #[test]
    fn validate_folder_name_rejects_zero_length() {
        let err = validate_folder_name("").expect_err("empty literal");
        assert_eq!(err, ERR_FOLDER_NAME_EMPTY);
    }

    #[test]
    fn validate_folder_name_accepts_200_chars() {
        // Boundary: exactly 200 chars passes (matches DB CHECK).
        let name: String = "a".repeat(200);
        let out = validate_folder_name(&name).expect("200 ok");
        assert_eq!(out.chars().count(), 200);
    }

    #[test]
    fn validate_folder_name_rejects_201_chars() {
        let name: String = "a".repeat(201);
        let err = validate_folder_name(&name).expect_err("too long");
        assert_eq!(err, ERR_FOLDER_NAME_TOO_LONG);
    }

    #[test]
    fn validate_folder_name_rejects_slash() {
        let err = validate_folder_name("a/b").expect_err("slash");
        assert_eq!(err, ERR_FOLDER_NAME_HAS_SLASH);
    }

    #[test]
    fn validate_folder_name_rejects_nul_byte() {
        let err = validate_folder_name("foo\0bar").expect_err("nul");
        assert_eq!(err, ERR_FOLDER_NAME_HAS_NUL);
    }

    #[test]
    fn validate_folder_name_accepts_unicode() {
        let name = "Relatórios-2026";
        let out = validate_folder_name(name).expect("utf8");
        assert_eq!(out, name);
    }

    #[test]
    fn file_summary_size_zero_is_valid() {
        let now = Utc::now();
        let row = FileRow {
            id: Uuid::new_v4(),
            name: "empty.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert_eq!(summary.size_bytes, 0);
    }

    #[test]
    fn file_list_response_with_cursor() {
        let id1 = Uuid::new_v4();
        let now = Utc::now();
        let resp = FileListResponse {
            items: vec![FileSummary {
                id: id1,
                name: "a.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size_bytes: 1,
                current_version: 1,
                total_versions: 1,
                folder_id: None,
                created_by: None,
                created_by_label: "x".to_string(),
                created_at: now,
                updated_at: now,
            }],
            next_cursor: Some(id1),
        };
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.next_cursor, Some(id1));
    }
}
