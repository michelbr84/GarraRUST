//! `/v1/uploads` — tus 1.0 resumable upload server (slice 1).
//!
//! Plan 0041 (GAR-395 slice 1). Ships only the two endpoints that
//! *reserve* and *probe* the upload resource; byte append (PATCH) and
//! `ObjectStore` commit land in slice 2.
//!
//! - `POST /v1/uploads` — **Creation extension**. Creates a
//!   `tus_uploads` row, returns `201 Created` + `Location`
//!   + `Tus-Resumable: 1.0.0`.
//! - `HEAD /v1/uploads/{id}` — **Resume probe**. Returns
//!   `Upload-Offset` (always `0` in slice 1 — no bytes accepted yet),
//!   `Upload-Length`, optional `Upload-Metadata`, `Cache-Control:
//!   no-store`, `Tus-Resumable: 1.0.0`.
//!
//! ## Multi-tenant posture
//!
//! Every transaction opens with `SET LOCAL app.current_user_id` and
//! `SET LOCAL app.current_group_id` as its first statements (plan 0016
//! M4 pattern). The `tus_uploads_group_isolation` RLS policy
//! (migration 014) then filters reads and blocks cross-group writes.
//!
//! ## Header policy
//!
//! The tus 1.0 spec (RFC-ish, <https://tus.io/protocols/resumable-upload>)
//! requires `Tus-Resumable: 1.0.0` on every request. A mismatch yields
//! `412 Precondition Failed` with a `Tus-Version: 1.0.0` hint header.

use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::engine::general_purpose::STANDARD_NO_PAD as BASE64_NOPAD;
use bytes::Bytes;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use garraia_storage::{ObjectStore, PutOptions};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Cap mirrors `files.size_bytes` CHECK from migration 003 — 5 GiB.
const MAX_UPLOAD_LENGTH: i64 = 5 * 1024 * 1024 * 1024;

/// Plan 0041 §5.4 — only 1.0.0 is accepted.
const TUS_RESUMABLE_VERSION: &str = "1.0.0";

/// tus 1.0 `Tus-Version` supported set, emitted on OPTIONS and 412.
const TUS_VERSION_HEADER: &str = "1.0.0";

/// tus 1.0 extensions implemented. Updated in plan 0047 (GAR-395 slice 3)
/// to advertise `termination` now that `DELETE /v1/uploads/{id}` is live.
const TUS_EXTENSION_HEADER: &str = "creation,termination";

/// Canonical tus 1.0 max size, matches `MAX_UPLOAD_LENGTH` above as
/// a string. Kept separate so `HeaderValue::from_static` can inline it.
const TUS_MAX_SIZE_HEADER: &str = "5368709120";

/// tus PATCH Content-Type (spec §3.2).
const TUS_PATCH_CONTENT_TYPE: &str = "application/offset+octet-stream";

/// 24h default per GAR-395 acceptance criteria; slice 3 adds the
/// purge worker.
const UPLOAD_EXPIRY_HOURS: i64 = 24;

/// Max raw bytes the `Upload-Metadata` header may carry. Defensive
/// cap against DoS; tus spec is silent on a concrete limit.
const MAX_UPLOAD_METADATA_LEN: usize = 1024;

// ─── Tus header name constants (lowercase — Axum normalises) ────────────
static H_TUS_RESUMABLE: HeaderName = HeaderName::from_static("tus-resumable");
static H_TUS_VERSION: HeaderName = HeaderName::from_static("tus-version");
static H_TUS_EXTENSION: HeaderName = HeaderName::from_static("tus-extension");
static H_TUS_MAX_SIZE: HeaderName = HeaderName::from_static("tus-max-size");
static H_UPLOAD_LENGTH: HeaderName = HeaderName::from_static("upload-length");
static H_UPLOAD_OFFSET: HeaderName = HeaderName::from_static("upload-offset");
static H_UPLOAD_METADATA: HeaderName = HeaderName::from_static("upload-metadata");
static H_UPLOAD_DEFER_LENGTH: HeaderName = HeaderName::from_static("upload-defer-length");

/// Error variants returned to clients. Declared here (and converted to
/// [`RestError`] at the boundary) so the enum models tus-specific
/// preconditions cleanly.
#[derive(Debug)]
enum UploadRejection {
    /// `Tus-Resumable` missing.
    MissingTusResumable,
    /// `Tus-Resumable` present but ≠ `1.0.0`.
    UnsupportedTusVersion,
    /// Both `Upload-Length` and `Upload-Defer-Length` missing.
    MissingUploadLength,
    /// Slice 1 does not implement deferred length.
    DeferredLengthUnsupported,
    /// `Upload-Length` parse failure.
    InvalidUploadLength,
    /// `Upload-Length` outside `[1, 5 GiB]`.
    OversizedUploadLength,
    /// `Upload-Metadata` parse failure or > 1 KiB.
    InvalidUploadMetadata,
    /// Plan 0044: PATCH `Upload-Offset` header missing.
    MissingUploadOffset,
    /// Plan 0044: PATCH `Upload-Offset` header parse failure.
    InvalidUploadOffset,
    /// Plan 0044: PATCH `Content-Type` is not
    /// `application/offset+octet-stream`.
    UnsupportedContentType,
}

impl UploadRejection {
    fn into_rest_error(self) -> RestError {
        match self {
            Self::MissingTusResumable => RestError::PreconditionFailed(
                "missing Tus-Resumable header (tus 1.0.0 required)".into(),
            ),
            Self::UnsupportedTusVersion => {
                RestError::PreconditionFailed("unsupported Tus-Resumable version".into())
            }
            Self::MissingUploadLength => {
                RestError::BadRequest("missing Upload-Length header".into())
            }
            Self::DeferredLengthUnsupported => {
                RestError::BadRequest("Upload-Defer-Length is not supported in this slice".into())
            }
            Self::InvalidUploadLength => {
                RestError::BadRequest("Upload-Length must be a non-negative integer".into())
            }
            Self::OversizedUploadLength => RestError::PayloadTooLarge(format!(
                "Upload-Length exceeds maximum of {MAX_UPLOAD_LENGTH} bytes (5 GiB)"
            )),
            Self::InvalidUploadMetadata => {
                RestError::BadRequest("Upload-Metadata header is malformed".into())
            }
            Self::MissingUploadOffset => {
                RestError::BadRequest("missing Upload-Offset header".into())
            }
            Self::InvalidUploadOffset => {
                RestError::BadRequest("Upload-Offset must be a non-negative integer".into())
            }
            Self::UnsupportedContentType => RestError::UnsupportedMediaType(format!(
                "Content-Type must be `{TUS_PATCH_CONTENT_TYPE}` for tus PATCH"
            )),
        }
    }
}

/// Bootstrap-time staging context. Holds the staging directory,
/// operational cap, and HMAC secret. Everything inside is immutable
/// after boot — shared across handlers via `Arc`.
///
/// Plan 0044 §5.2: staging is always filesystem-local regardless of
/// final backend, so a single struct fits all configurations.
/// Plan 0044 §5.6: `hmac_secret` is fail-closed — commit aborts with
/// 500 when `None`, preventing integrity-HMAC row insertion with a
/// null secret.
pub struct UploadStaging {
    /// Canonicalised staging directory. All partial files live here.
    /// Canonicalisation happens once at bootstrap; `upload_id` is a
    /// UUID so traversal is structurally impossible (plan 0044 §6 SEC-L).
    pub staging_dir: PathBuf,
    /// Max bytes accepted across all PATCHes for a single upload.
    /// Plan 0044 §5.1 — slice 3 lifts this via streaming `put`.
    pub max_patch_bytes: u64,
    /// HMAC-SHA256 secret for `file_versions.integrity_hmac`. Sourced
    /// from `GARRAIA_UPLOAD_HMAC_SECRET`. Plan 0044 §5.6 — fail-closed.
    /// Stored as `Vec<u8>` because `garraia_storage::PutOptions`
    /// accepts raw bytes; callers SHOULD zeroize after put.
    pub hmac_secret: Vec<u8>,
}

impl std::fmt::Debug for UploadStaging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SEC-M: never leak the HMAC secret through Debug.
        f.debug_struct("UploadStaging")
            .field("staging_dir", &self.staging_dir)
            .field("max_patch_bytes", &self.max_patch_bytes)
            .field("hmac_secret", &"<redacted>")
            .finish()
    }
}

impl UploadStaging {
    /// Build the staging file path for `upload_id`. UUID v7 canonical
    /// form is 36 chars of `[0-9a-f-]`, so no traversal — joined after
    /// a canonicalised `staging_dir`.
    fn staging_path(&self, upload_id: Uuid) -> PathBuf {
        self.staging_dir.join(format!("{upload_id}.staging"))
    }
}

// ─── Handlers ───────────────────────────────────────────────────────────

/// `POST /v1/uploads` — tus 1.0 Creation extension.
///
/// Returns `201 Created` with `Location: /v1/uploads/{uuid}`,
/// `Tus-Resumable: 1.0.0`. Allocates a `tus_uploads` row and a
/// deterministic `object_key` that slice 2 will use on PATCH.
#[utoipa::path(
    post,
    path = "/v1/uploads",
    request_body(content = CreateUploadRequest, description = "Headers only — tus semantics"),
    responses(
        (status = 201, description = "Upload resource created", body = CreateUploadResponse),
        (status = 400, description = "Bad request (missing/invalid headers)", body = super::problem::ProblemDetails),
        (status = 401, description = "Unauthenticated", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of X-Group-Id", body = super::problem::ProblemDetails),
        (status = 412, description = "Tus-Resumable missing or unsupported", body = super::problem::ProblemDetails),
        (status = 413, description = "Upload-Length exceeds 5 GiB", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.post_uploads",
    skip(state, headers, principal),
    fields(
        user_id = %principal.user_id,
        group_id = ?principal.group_id
    )
)]
pub async fn create_upload(
    State(state): State<RestV1FullState>,
    headers: HeaderMap,
    principal: Principal,
) -> Result<Response, RestError> {
    validate_tus_resumable(&headers).map_err(UploadRejection::into_rest_error)?;
    let upload_length =
        parse_upload_length_required(&headers).map_err(UploadRejection::into_rest_error)?;
    let upload_metadata =
        parse_upload_metadata_header(&headers).map_err(UploadRejection::into_rest_error)?;

    let group_id = principal.group_id.ok_or(RestError::Forbidden)?;
    let upload_id = Uuid::now_v7();
    let object_key = build_object_key(group_id, upload_id);
    let expires_at: DateTime<Utc> = Utc::now() + ChronoDuration::hours(UPLOAD_EXPIRY_HOURS);
    let (filename, mime_type) = upload_metadata
        .as_ref()
        .map(|m| {
            (
                m.parsed.get("filename").cloned(),
                m.parsed.get("filetype").cloned(),
            )
        })
        .unwrap_or((None, None));
    let metadata_raw = upload_metadata.map(|m| m.raw);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("begin create_upload tx")))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    sqlx::query(
        r#"
        INSERT INTO tus_uploads
            (id, group_id, created_by, object_key, upload_length, upload_offset,
             upload_metadata, filename, mime_type, status, expires_at)
        VALUES
            ($1, $2, $3, $4, $5, 0, $6, $7, $8, 'in_progress', $9)
        "#,
    )
    .bind(upload_id)
    .bind(group_id)
    .bind(principal.user_id)
    .bind(&object_key)
    .bind(upload_length)
    .bind(metadata_raw.as_deref())
    .bind(filename.as_deref())
    .bind(mime_type.as_deref())
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("insert tus_uploads")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("commit create_upload tx")))?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        LOCATION,
        header_value_from_ascii(&format!("/v1/uploads/{upload_id}"))?,
    );
    response_headers.insert(
        H_TUS_RESUMABLE.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );
    // Code review HIGH-1 — plan 0041 §1 lists `Tus-Version` on the
    // 201 response. The 412 middleware covers the precondition path
    // only; 201 must carry it explicitly so the client sees the
    // supported set on *every* successful creation.
    response_headers.insert(
        H_TUS_VERSION.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );

    Ok((StatusCode::CREATED, response_headers).into_response())
}

/// `HEAD /v1/uploads/{id}` — tus 1.0 Resume probe.
///
/// Returns `Upload-Offset`, `Upload-Length`, optional `Upload-Metadata`,
/// `Cache-Control: no-store`, `Tus-Resumable: 1.0.0`. Slice 1 always
/// emits `Upload-Offset: 0` because PATCH is not yet accepted.
///
/// Cross-group lookups return `404` (not 403) to avoid leaking the
/// existence of the resource (ADR 0004 §7).
#[utoipa::path(
    head,
    path = "/v1/uploads/{id}",
    params(
        ("id" = Uuid, Path, description = "tus upload resource id (UUID v7)")
    ),
    responses(
        (status = 200, description = "Upload status", headers(
            ("Upload-Offset" = i64, description = "Bytes persisted; always 0 in slice 1"),
            ("Upload-Length" = i64, description = "Total upload size"),
            ("Upload-Metadata" = String, description = "Original metadata (verbatim)"),
            ("Cache-Control" = String, description = "Always `no-store`"),
            ("Tus-Resumable" = String, description = "Always `1.0.0`")
        )),
        (status = 401, description = "Unauthenticated", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of X-Group-Id", body = super::problem::ProblemDetails),
        (status = 404, description = "Upload not found (or different group)", body = super::problem::ProblemDetails),
        (status = 412, description = "Tus-Resumable missing or unsupported", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.head_uploads",
    skip(state, headers, principal),
    fields(
        user_id = %principal.user_id,
        group_id = ?principal.group_id,
        upload_id = %upload_id
    )
)]
pub async fn head_upload(
    State(state): State<RestV1FullState>,
    Path(upload_id): Path<Uuid>,
    headers: HeaderMap,
    principal: Principal,
) -> Result<Response, RestError> {
    validate_tus_resumable(&headers).map_err(UploadRejection::into_rest_error)?;

    let group_id = principal.group_id.ok_or(RestError::Forbidden)?;
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("begin head_upload tx")))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // NB (code review MEDIUM): `pool.begin()` + `SET LOCAL` + `SELECT`
    // + `tx.commit()` is heavier than a bare `pool.acquire()` for a
    // read-only HEAD. We keep the transactional shape here for parity
    // with `groups.rs` and future slices that will add writes in the
    // same handler (PATCH). Optimisation is deliberately deferred.
    let row: Option<(i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT upload_length, upload_offset, upload_metadata
         FROM tus_uploads
         WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("select tus_uploads")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("commit head_upload tx")))?;

    let (upload_length, upload_offset, metadata_raw) = row.ok_or(RestError::NotFound)?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        H_UPLOAD_OFFSET.clone(),
        header_value_from_ascii(&upload_offset.to_string())?,
    );
    response_headers.insert(
        H_UPLOAD_LENGTH.clone(),
        header_value_from_ascii(&upload_length.to_string())?,
    );
    if let Some(raw) = metadata_raw
        && let Ok(v) = HeaderValue::from_str(&raw)
    {
        response_headers.insert(H_UPLOAD_METADATA.clone(), v);
    }
    response_headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response_headers.insert(
        H_TUS_RESUMABLE.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );

    Ok((StatusCode::OK, response_headers).into_response())
}

/// Middleware: attach `Tus-Version: 1.0.0` to every 412 response so
/// clients discovery-probe the supported version. Applied on the
/// `/v1/uploads*` subtree only.
///
/// Attaching the header via middleware (vs per-handler) keeps the
/// tus-spec requirement enforced uniformly even if future handlers
/// forget.
pub async fn tus_version_header_layer(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    if resp.status() == StatusCode::PRECONDITION_FAILED {
        resp.headers_mut().insert(
            H_TUS_VERSION.clone(),
            HeaderValue::from_static(TUS_RESUMABLE_VERSION),
        );
    }
    resp
}

// ─── Tenant-context helper (SET LOCAL pair) ─────────────────────────────
//
// Extracted (code review MEDIUM) so `create_upload` + `head_upload` +
// any future handler in this module can share the exact pattern. `SET
// LOCAL` does not accept bind parameters; `Uuid::Display` is fixed at
// 36 chars of `[0-9a-f-]` by RFC 4122, so the `format!` interpolation
// is injection-safe by construction.
async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<(), RestError> {
    sqlx::query(&format!("SET LOCAL app.current_user_id = '{user_id}'"))
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_user_id")))?;
    sqlx::query(&format!("SET LOCAL app.current_group_id = '{group_id}'"))
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_group_id")))?;
    Ok(())
}

/// Build a `HeaderValue` from a string the caller knows is ASCII (UUID,
/// i64). Falls back to a 500 response if the invariant is ever
/// violated — keeps CLAUDE.md rule #4 (no `expect()` in production)
/// intact without pushing the check to a panic.
fn header_value_from_ascii(s: &str) -> Result<HeaderValue, RestError> {
    HeaderValue::from_str(s)
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("header value is not ascii")))
}

// ─── Header parsing helpers (unit-tested) ───────────────────────────────

fn validate_tus_resumable(headers: &HeaderMap) -> Result<(), UploadRejection> {
    let v = headers
        .get(&H_TUS_RESUMABLE)
        .ok_or(UploadRejection::MissingTusResumable)?
        .to_str()
        .map_err(|_| UploadRejection::UnsupportedTusVersion)?
        .trim();
    if v == TUS_RESUMABLE_VERSION {
        Ok(())
    } else {
        Err(UploadRejection::UnsupportedTusVersion)
    }
}

fn parse_upload_length_required(headers: &HeaderMap) -> Result<i64, UploadRejection> {
    if headers.contains_key(&H_UPLOAD_DEFER_LENGTH) {
        return Err(UploadRejection::DeferredLengthUnsupported);
    }
    let raw = headers
        .get(&H_UPLOAD_LENGTH)
        .ok_or(UploadRejection::MissingUploadLength)?
        .to_str()
        .map_err(|_| UploadRejection::InvalidUploadLength)?
        .trim();
    parse_upload_length(raw)
}

fn parse_upload_length(raw: &str) -> Result<i64, UploadRejection> {
    let n: i64 = raw
        .parse()
        .map_err(|_| UploadRejection::InvalidUploadLength)?;
    if n <= 0 {
        return Err(UploadRejection::InvalidUploadLength);
    }
    if n > MAX_UPLOAD_LENGTH {
        return Err(UploadRejection::OversizedUploadLength);
    }
    Ok(n)
}

#[derive(Debug, Clone)]
struct UploadMetadata {
    raw: String,
    parsed: HashMap<String, String>,
}

fn parse_upload_metadata_header(
    headers: &HeaderMap,
) -> Result<Option<UploadMetadata>, UploadRejection> {
    let Some(v) = headers.get(&H_UPLOAD_METADATA) else {
        return Ok(None);
    };
    let raw = v
        .to_str()
        .map_err(|_| UploadRejection::InvalidUploadMetadata)?;
    if raw.len() > MAX_UPLOAD_METADATA_LEN {
        return Err(UploadRejection::InvalidUploadMetadata);
    }
    // Security audit SEC-M2 — reject CRLF in the raw header value
    // before it's persisted. `HeaderValue::from_str` in `head_upload`
    // would already drop a CRLF-poisoned value silently, but the
    // defense is *intentional* here: the caller must never be able
    // to seed a `tus_uploads.upload_metadata` row with bytes that
    // could later pollute an HTTP response header on replay.
    reject_crlf(raw)?;
    let parsed = parse_upload_metadata(raw)?;
    Ok(Some(UploadMetadata {
        raw: raw.to_string(),
        parsed,
    }))
}

/// Reject CR/LF in a raw metadata string. Extracted from
/// `parse_upload_metadata_header` (security audit SEC-M2) so unit
/// tests can exercise it directly — `http::HeaderValue::from_str`
/// refuses raw CR/LF by construction, so feeding a poisoned value via
/// a real `HeaderMap` isn't possible in-process.
fn reject_crlf(raw: &str) -> Result<(), UploadRejection> {
    if raw.contains('\r') || raw.contains('\n') {
        return Err(UploadRejection::InvalidUploadMetadata);
    }
    Ok(())
}

/// Parse the tus 1.0 `Upload-Metadata` header: comma-separated list of
/// `key base64-value` pairs (value optional — `"key"` alone ⇒ `""`).
/// Keys must be non-empty and contain no whitespace/control chars.
/// Values, when present, are base64 (either padded or not accepted).
fn parse_upload_metadata(raw: &str) -> Result<HashMap<String, String>, UploadRejection> {
    let mut out = HashMap::new();
    for part in raw.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let mut bits = part.splitn(2, ' ');
        let key = bits.next().unwrap_or("").trim();
        if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(UploadRejection::InvalidUploadMetadata);
        }
        let value = match bits.next() {
            None => String::new(),
            Some(b64) => {
                let b64 = b64.trim();
                let bytes = BASE64_STD
                    .decode(b64)
                    .or_else(|_| BASE64_NOPAD.decode(b64))
                    .map_err(|_| UploadRejection::InvalidUploadMetadata)?;
                String::from_utf8(bytes).map_err(|_| UploadRejection::InvalidUploadMetadata)?
            }
        };
        out.insert(key.to_string(), value);
    }
    Ok(out)
}

/// Build the object_key allocated at Creation. Format matches ADR 0004
/// §Key schema with the `uploads/` segment in place of `{folder_path}`.
fn build_object_key(group_id: Uuid, upload_id: Uuid) -> String {
    format!("{group_id}/uploads/{upload_id}/v1")
}

// ─── Slice 2 handlers: PATCH + OPTIONS ──────────────────────────────────

/// Row shape returned by the SELECT ... FOR UPDATE inside `patch_upload`.
///
/// Layout: `(upload_length, upload_offset, status, object_key, filename,
/// mime_type, created_by)`.
type TusUploadRow = (
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Uuid,
);

/// Row shape for the final commit SELECT + display_name lookup.
/// Layout: `(display_name,)` — single column, boxed to keep the helper
/// ergonomics consistent with the rest of the module.
type DisplayNameRow = (String,);

/// `PATCH /v1/uploads/{id}` — tus 1.0 Core.
///
/// Accepts bytes via `Content-Type: application/offset+octet-stream`,
/// validates `Upload-Offset` against the current `tus_uploads` row,
/// appends to the staging file, and commits to the ObjectStore when
/// `upload_offset + bytes == upload_length`. On commit:
///
/// 1. `ObjectStore::put` runs BEFORE the Postgres COMMIT (plan 0044
///    §5.3.1 two-phase ordering).
/// 2. `files` + `file_versions` + `audit_events` + `tus_uploads.status`
///    update happen inside a single transaction that rolls back if
///    any step fails — including a `put` failure (blob already went,
///    but no metadata row references it; retry remains safe).
///
/// Cross-tenant lookup returns 404 (never 403) per ADR 0004 §7.
#[utoipa::path(
    patch,
    path = "/v1/uploads/{id}",
    params(
        ("id" = Uuid, Path, description = "tus upload resource id (UUID v7)")
    ),
    request_body(
        content_type = "application/offset+octet-stream",
        content = String,
        description = "Raw bytes at the expected Upload-Offset"
    ),
    responses(
        (status = 204, description = "Bytes accepted; Upload-Offset updated"),
        (status = 401, description = "Unauthenticated", body = super::problem::ProblemDetails),
        (status = 404, description = "Upload not found or different group", body = super::problem::ProblemDetails),
        (status = 409, description = "Upload-Offset mismatch", body = super::problem::ProblemDetails),
        (status = 410, description = "Upload was completed/aborted/expired", body = super::problem::ProblemDetails),
        (status = 412, description = "Tus-Resumable missing or unsupported", body = super::problem::ProblemDetails),
        (status = 413, description = "Body exceeds Upload-Length or operator cap", body = super::problem::ProblemDetails),
        (status = 415, description = "Content-Type is not application/offset+octet-stream", body = super::problem::ProblemDetails),
        (status = 502, description = "ObjectStore commit failed (retry safe)", body = super::problem::ProblemDetails),
        (status = 503, description = "Storage backend not configured", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
#[tracing::instrument(
    name = "rest_v1.patch_uploads",
    skip(state, headers, principal, body),
    fields(
        user_id = %principal.user_id,
        group_id = ?principal.group_id,
        upload_id = %upload_id
    )
)]
pub async fn patch_upload(
    State(state): State<RestV1FullState>,
    Path(upload_id): Path<Uuid>,
    headers: HeaderMap,
    principal: Principal,
    body: Body,
) -> Result<Response, RestError> {
    validate_tus_resumable(&headers).map_err(UploadRejection::into_rest_error)?;
    validate_patch_content_type(&headers).map_err(UploadRejection::into_rest_error)?;
    let expected_offset =
        parse_upload_offset_required(&headers).map_err(UploadRejection::into_rest_error)?;

    let group_id = principal.group_id.ok_or(RestError::Forbidden)?;

    let staging = state
        .storage
        .upload_staging
        .as_ref()
        .ok_or(RestError::AuthUnconfigured)?
        .clone();
    let object_store = state
        .storage
        .object_store
        .as_ref()
        .ok_or(RestError::AuthUnconfigured)?
        .clone();

    let cap_usize: usize = staging.max_patch_bytes.try_into().unwrap_or(usize::MAX);

    // Drain body with the operator cap. `to_bytes` returns 413-shaped
    // error when the body exceeds the cap — we translate to the tus
    // 413 response. Plan 0044 §5.1 — streaming put lifts this in
    // slice 3.
    let bytes = to_bytes(body, cap_usize).await.map_err(|e| {
        tracing::debug!(error = %e, "patch body exceeds staging cap or read failed");
        RestError::PayloadTooLarge(format!(
            "PATCH body exceeds operator cap of {} bytes",
            staging.max_patch_bytes
        ))
    })?;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("begin patch_upload tx")))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Lock the row so concurrent PATCHes on the same upload_id
    // serialise — one takes the FOR UPDATE, the other waits, then
    // sees the advanced offset. Prevents two PATCHes from racing
    // past the offset precondition.
    let row: Option<TusUploadRow> = sqlx::query_as(
        "SELECT upload_length, upload_offset, status, object_key, filename, mime_type, created_by
         FROM tus_uploads
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(upload_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("select tus_uploads for patch")))?;

    let (upload_length, upload_offset_before, status, object_key, filename, mime_type, created_by) =
        row.ok_or(RestError::NotFound)?;

    // Status state machine (plan 0044 §5.10).
    match status.as_str() {
        "in_progress" => {}
        "completed" | "aborted" | "expired" => {
            return Err(RestError::Gone(format!(
                "upload is no longer available (status={status})"
            )));
        }
        other => {
            return Err(RestError::Internal(anyhow::anyhow!(
                "unexpected tus_uploads.status `{other}`"
            )));
        }
    }

    // Upload-Offset precondition (tus §3.2).
    if expected_offset != upload_offset_before {
        return Err(RestError::Conflict(format!(
            "Upload-Offset mismatch: expected {upload_offset_before}, got {expected_offset}"
        )));
    }

    let chunk_len_i64 = bytes.len() as i64;
    let new_offset = upload_offset_before
        .checked_add(chunk_len_i64)
        .ok_or_else(|| RestError::PayloadTooLarge("offset overflow".into()))?;
    if new_offset > upload_length {
        return Err(RestError::PayloadTooLarge(format!(
            "PATCH body would push upload past Upload-Length ({new_offset} > {upload_length})"
        )));
    }

    // Append to staging (plan 0044 §5.2). `upload_id` is a UUID, so
    // the composed path is traversal-safe by construction.
    let staging_path = staging.staging_path(upload_id);
    append_to_staging(&staging_path, &bytes).await?;

    // Update the offset row inside the same tx. The WHERE guard
    // against status='in_progress' + the old offset prevents a race
    // where a second PATCH picked up the FOR UPDATE after the first
    // commit — the second would see the advanced offset and fail the
    // precondition above, but this guard is a defense-in-depth against
    // any future non-transactional path.
    let updated = sqlx::query(
        "UPDATE tus_uploads
         SET upload_offset = $1, updated_at = now()
         WHERE id = $2 AND status = 'in_progress' AND upload_offset = $3",
    )
    .bind(new_offset)
    .bind(upload_id)
    .bind(upload_offset_before)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("update tus_uploads.offset")))?;

    if updated.rows_affected() == 0 {
        // Race guard: another PATCH advanced the offset between FOR
        // UPDATE and our UPDATE. Bail with 409 so the client retries
        // with the current offset.
        return Err(RestError::Conflict(
            "Upload-Offset advanced concurrently; retry".into(),
        ));
    }

    let completed = new_offset == upload_length;
    if completed {
        finalize_upload(
            &mut tx,
            &staging,
            object_store.as_ref(),
            upload_id,
            group_id,
            created_by,
            upload_length,
            &object_key,
            filename.as_deref(),
            mime_type.as_deref(),
            principal.user_id,
        )
        .await?;
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("commit patch_upload tx")))?;

    if completed {
        // Best-effort staging cleanup post-commit. A failure here is
        // benign (stale staging files sweep in slice 3); WARN-log and
        // proceed. Plan 0044 §5.2.
        if let Err(e) = tokio::fs::remove_file(&staging_path).await {
            tracing::warn!(
                upload_id = %upload_id,
                error = %e,
                "failed to remove staging file after commit"
            );
        }
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        H_UPLOAD_OFFSET.clone(),
        header_value_from_ascii(&new_offset.to_string())?,
    );
    response_headers.insert(
        H_TUS_RESUMABLE.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );
    Ok((StatusCode::NO_CONTENT, response_headers).into_response())
}

/// `DELETE /v1/uploads/{id}` — tus 1.0 **Termination** extension (plan
/// 0047 / GAR-395 slice 3).
///
/// Semantics (spec §Termination):
/// - `204 No Content` when the upload was `in_progress` and is now
///   `aborted`. Staging file is deleted best-effort.
/// - `404 Not Found` when the upload does not exist OR belongs to a
///   different group (anti-enumeration per ADR 0004 §7 — NEVER 403).
/// - `410 Gone` when the upload is already `completed`, `aborted`, or
///   `expired` — the resource can no longer be terminated (completed
///   uploads flow through `/v1/files/*` deletion, slice 4+).
///
/// `ObjectStore` is **not** touched here — in slice 2's two-phase
/// commit, the blob is only put under `object_key` after `status =
/// 'completed'`. An `in_progress` row has nothing in the bucket to
/// remove; the staging file IS removed.
///
/// Audit: one row with action `upload.terminated` on success.
#[utoipa::path(
    delete,
    path = "/v1/uploads/{id}",
    params(("id" = Uuid, Path, description = "upload id returned by POST /v1/uploads")),
    responses(
        (status = 204, description = "upload terminated"),
        (status = 404, description = "upload not found or cross-group"),
        (status = 410, description = "upload already completed/aborted/expired"),
        (status = 412, description = "Tus-Resumable header missing or unsupported"),
    ),
)]
pub async fn delete_upload(
    State(state): State<RestV1FullState>,
    Path(upload_id): Path<Uuid>,
    headers: HeaderMap,
    principal: Principal,
) -> Result<Response, RestError> {
    validate_tus_resumable(&headers).map_err(UploadRejection::into_rest_error)?;

    let group_id = principal.group_id.ok_or(RestError::Forbidden)?;

    // Staging cleanup is best-effort AFTER the row transitions to
    // aborted — pull the staging handle now so we can remove the file
    // post-commit. A missing staging context is not a hard failure
    // (server may not have the tus wiring fully configured) — we just
    // skip the file cleanup branch.
    let staging = state.storage.upload_staging.clone();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("begin delete_upload tx")))?;

    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Lock the row so concurrent DELETE + PATCH serialise via the row
    // lock. RLS already restricts to the caller's group — if the row
    // exists in a different group, the SELECT returns 0 rows (404).
    let row: Option<(String, i64, i64, String)> = sqlx::query_as(
        "SELECT status, upload_offset, upload_length, object_key
         FROM tus_uploads
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(upload_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        RestError::Internal(anyhow::anyhow!(e).context("select tus_uploads for delete"))
    })?;

    let (status, upload_offset, upload_length, object_key) = row.ok_or(RestError::NotFound)?;

    // Status state machine — only `in_progress` can be terminated.
    match status.as_str() {
        "in_progress" => {}
        "completed" | "aborted" | "expired" => {
            return Err(RestError::Gone(format!(
                "upload is no longer available (status={status})"
            )));
        }
        other => {
            return Err(RestError::Internal(anyhow::anyhow!(
                "unexpected tus_uploads.status `{other}`"
            )));
        }
    }

    // Transition in the same tx. The WHERE guard against the current
    // status is defense-in-depth against a race with the expiration
    // worker / a concurrent DELETE.
    let updated = sqlx::query(
        "UPDATE tus_uploads
         SET status = 'aborted', updated_at = now()
         WHERE id = $1 AND status = 'in_progress'",
    )
    .bind(upload_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        RestError::Internal(anyhow::anyhow!(e).context("update tus_uploads.status=aborted"))
    })?;

    if updated.rows_affected() == 0 {
        // Another actor won the transition — treat as 410 to stay
        // idempotent from the client's POV.
        return Err(RestError::Gone(
            "upload transitioned concurrently; terminated elsewhere".into(),
        ));
    }

    // Audit atomically within the same tx.
    let object_key_hash = sha256_hex_of(object_key.as_bytes());
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::UploadTerminated,
        principal.user_id,
        group_id,
        "tus_uploads",
        upload_id.to_string(),
        json!({
            "upload_offset": upload_offset,
            "upload_length": upload_length,
            "object_key_hash": object_key_hash,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("audit upload.terminated")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("commit delete_upload tx")))?;

    // Post-commit: best-effort staging cleanup. A failure here is benign
    // (the expiration worker will eventually sweep stale staging files).
    if let Some(staging) = staging {
        let staging_path = staging.staging_path(upload_id);
        if let Err(e) = tokio::fs::remove_file(&staging_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                upload_id = %upload_id,
                error = %e,
                "failed to remove staging file after termination"
            );
        }
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        H_TUS_RESUMABLE.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );
    Ok((StatusCode::NO_CONTENT, response_headers).into_response())
}

/// `OPTIONS /v1/uploads` — tus 1.0 discovery (spec §5.2).
///
/// Unauthenticated — tus clients probe capabilities before they hold
/// credentials. Returns a 204 with `Tus-Version`, `Tus-Resumable`,
/// `Tus-Extension`, and `Tus-Max-Size` headers.
#[utoipa::path(
    options,
    path = "/v1/uploads",
    responses(
        (status = 204, description = "tus capabilities discovery", headers(
            ("Tus-Version" = String, description = "Supported versions"),
            ("Tus-Resumable" = String, description = "Preferred version"),
            ("Tus-Extension" = String, description = "Comma-separated supported extensions"),
            ("Tus-Max-Size" = i64, description = "Server-side upload cap in bytes")
        )),
    ),
)]
pub async fn options_uploads() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        H_TUS_VERSION.clone(),
        HeaderValue::from_static(TUS_VERSION_HEADER),
    );
    headers.insert(
        H_TUS_RESUMABLE.clone(),
        HeaderValue::from_static(TUS_RESUMABLE_VERSION),
    );
    headers.insert(
        H_TUS_EXTENSION.clone(),
        HeaderValue::from_static(TUS_EXTENSION_HEADER),
    );
    headers.insert(
        H_TUS_MAX_SIZE.clone(),
        HeaderValue::from_static(TUS_MAX_SIZE_HEADER),
    );
    (StatusCode::NO_CONTENT, headers).into_response()
}

/// Commit the upload: `ObjectStore::put` + `files` / `file_versions` /
/// `audit_events` inserts + `tus_uploads.status = 'completed'`.
/// Runs inside the caller's transaction — `put` is executed BEFORE
/// `tx.commit()` per plan 0044 §5.3.1 (blob-first, row-second).
#[allow(clippy::too_many_arguments)]
async fn finalize_upload(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    staging: &UploadStaging,
    object_store: &dyn ObjectStore,
    upload_id: Uuid,
    group_id: Uuid,
    created_by: Uuid,
    size_bytes: i64,
    object_key: &str,
    filename: Option<&str>,
    mime_type_opt: Option<&str>,
    actor_user_id: Uuid,
) -> Result<(), RestError> {
    // MIME allow-list check (plan 0044 §5.5). Fail-closed → 415
    // + tx rollback. Defaults to `application/octet-stream` when the
    // client didn't supply one — which is NOT in the allow-list.
    let mime_type = mime_type_opt
        .unwrap_or("application/octet-stream")
        .to_string();
    if !garraia_storage::mime_allowlist::is_mime_allowed(&mime_type) {
        return Err(RestError::UnsupportedMediaType(format!(
            "mime type `{mime_type}` is not in the allow-list"
        )));
    }

    // Load bytes back from staging. Plan 0044 §5.1 — slice 3 will
    // stream via `ObjectStore::put_stream`. Today we read the whole
    // file; `max_patch_bytes` cap keeps memory bounded.
    let staging_path = staging.staging_path(upload_id);
    let staged_bytes = tokio::fs::read(&staging_path).await.map_err(|e| {
        RestError::Internal(anyhow::anyhow!(e).context("read staging file during finalize_upload"))
    })?;

    if staged_bytes.len() as i64 != size_bytes {
        // Defense-in-depth: the upload_offset ledger and the staging
        // file size must match. Any divergence is a gateway bug.
        return Err(RestError::Internal(anyhow::anyhow!(
            "staging file size {} does not match expected {size_bytes}",
            staged_bytes.len()
        )));
    }

    let put_bytes = Bytes::from(staged_bytes);

    // PutOptions — MIME declared, HMAC material populated from the
    // staging context so ObjectStore stores `integrity_hmac` we can
    // forward into `file_versions`. Plan 0044 §5.6 fail-closed.
    let put_opts = PutOptions {
        content_type: Some(mime_type.clone()),
        cache_control: None,
        allow_unsafe_mime: false,
        version_id: Some("v1".to_string()),
        hmac_secret: Some(staging.hmac_secret.clone()),
    };

    // Plan 0044 §5.3.1: put() executes BEFORE commit. A put() failure
    // rolls back the tx (dropped at fn exit without `commit()`), so
    // `tus_uploads.status` stays `in_progress` and no files / versions
    // / audit rows exist. Retry is safe from the client's side.
    let put_meta = object_store
        .put(object_key, put_bytes, put_opts)
        .await
        .map_err(|e| {
            // PII-safe: `StorageError::Display` is bounded to
            // operator-safe strings (no body bytes, no key traversal).
            tracing::warn!(
                upload_id = %upload_id,
                error = %e,
                "ObjectStore::put failed; upload stays in_progress"
            );
            RestError::BadGateway("ObjectStore commit failed; please retry".into())
        })?;

    // HMAC is required by migration 003 — if missing, fail-closed.
    let integrity_hmac = put_meta.integrity_hmac.ok_or_else(|| {
        RestError::Internal(anyhow::anyhow!(
            "ObjectStore did not return integrity_hmac; GARRAIA_UPLOAD_HMAC_SECRET misconfigured"
        ))
    })?;

    // Resolve files.created_by_label from users.display_name. Runs
    // inside the same tx — RLS-safe because users is readable via
    // `users_group_scope` membership for the acting user.
    let display_name_row: DisplayNameRow =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(created_by)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| {
                RestError::Internal(
                    anyhow::anyhow!(e).context("select users.display_name for label"),
                )
            })?;
    let display_name = display_name_row.0;

    // files row (folder_id NULL = root per plan 0044 §5.4).
    let file_name = filename
        .map(|f| f.to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| format!("upload-{upload_id}.bin"));

    let (file_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO files
            (group_id, folder_id, name, current_version, total_versions,
             size_bytes, mime_type, created_by, created_by_label)
         VALUES ($1, NULL, $2, 1, 1, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(group_id)
    .bind(&file_name)
    .bind(size_bytes)
    .bind(&mime_type)
    .bind(created_by)
    .bind(&display_name)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("insert files")))?;

    sqlx::query(
        "INSERT INTO file_versions
            (file_id, group_id, version, object_key, etag, checksum_sha256,
             integrity_hmac, size_bytes, mime_type, created_by, created_by_label)
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(object_key)
    .bind(&put_meta.etag_sha256[..put_meta.etag_sha256.len().min(200)])
    .bind(&put_meta.etag_sha256)
    .bind(&integrity_hmac)
    .bind(size_bytes)
    .bind(&mime_type)
    .bind(created_by)
    .bind(&display_name)
    .execute(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("insert file_versions")))?;

    // Flip the tus_uploads status. rows_affected() MUST be 1 —
    // zero indicates a race we failed to detect earlier.
    let flipped = sqlx::query(
        "UPDATE tus_uploads
         SET status = 'completed', updated_at = now()
         WHERE id = $1 AND status = 'in_progress'",
    )
    .bind(upload_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("flip tus_uploads.status")))?;

    if flipped.rows_affected() == 0 {
        return Err(RestError::Internal(anyhow::anyhow!(
            "tus_uploads row vanished during finalize — concurrency bug"
        )));
    }

    // Audit event. PII-safe metadata (plan 0044 §6 SEC-L): `upload_id`,
    // `size_bytes`, `object_key_hash`. No filename / mime_type.
    let object_key_hash = sha256_hex_of(object_key.as_bytes());
    audit_workspace_event(
        tx,
        WorkspaceAuditAction::UploadCompleted,
        actor_user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({
            "upload_id": upload_id.to_string(),
            "size_bytes": size_bytes,
            "object_key_hash": object_key_hash,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("audit upload.completed")))?;

    Ok(())
}

/// Append `bytes` to the staging file. Creates the file on first
/// PATCH with append mode; subsequent PATCHes extend it.
async fn append_to_staging(staging_path: &std::path::Path, bytes: &Bytes) -> Result<(), RestError> {
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(staging_path)
        .await
        .map_err(|e| {
            RestError::Internal(anyhow::anyhow!(e).context("open staging file for append"))
        })?;
    f.write_all(bytes)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("append to staging file")))?;
    f.flush()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("flush staging file")))?;
    Ok(())
}

/// Compute hex SHA-256 of a byte slice. Used only for
/// `audit_events.metadata.object_key_hash` — a debug aid that lets
/// operators correlate audit rows with object keys without exposing
/// the raw key (which leaks `group_id` + `upload_id`).
fn sha256_hex_of(input: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(input);
    hex::encode(digest)
}

// ─── PATCH / OPTIONS header parsing helpers ─────────────────────────────

fn validate_patch_content_type(headers: &HeaderMap) -> Result<(), UploadRejection> {
    let v = headers
        .get(axum::http::header::CONTENT_TYPE)
        .ok_or(UploadRejection::UnsupportedContentType)?
        .to_str()
        .map_err(|_| UploadRejection::UnsupportedContentType)?
        .trim();
    // Strip any `;parameters` suffix that some clients append.
    let primary = v.split(';').next().unwrap_or("").trim();
    if primary.eq_ignore_ascii_case(TUS_PATCH_CONTENT_TYPE) {
        Ok(())
    } else {
        Err(UploadRejection::UnsupportedContentType)
    }
}

fn parse_upload_offset_required(headers: &HeaderMap) -> Result<i64, UploadRejection> {
    let raw = headers
        .get(&H_UPLOAD_OFFSET)
        .ok_or(UploadRejection::MissingUploadOffset)?
        .to_str()
        .map_err(|_| UploadRejection::InvalidUploadOffset)?
        .trim();
    parse_upload_offset(raw)
}

fn parse_upload_offset(raw: &str) -> Result<i64, UploadRejection> {
    let n: i64 = raw
        .parse()
        .map_err(|_| UploadRejection::InvalidUploadOffset)?;
    if n < 0 {
        return Err(UploadRejection::InvalidUploadOffset);
    }
    Ok(n)
}

// ─── OpenAPI schemas ────────────────────────────────────────────────────

/// Empty marker struct so `utoipa::path` has a `request_body` target
/// (tus 1.0 Creation has no JSON body — everything flows through
/// headers). Used only for schema documentation.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateUploadRequest {}

/// Marker for the 201 response — the real payload is in the headers
/// (`Location`, `Tus-Resumable`). Included here so utoipa can reference
/// a concrete type on the 201 path.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateUploadResponse {}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hmap(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            let name: HeaderName = k.parse().unwrap();
            h.insert(name, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn tus_resumable_ok() {
        validate_tus_resumable(&hmap(&[("tus-resumable", "1.0.0")])).unwrap();
    }

    #[test]
    fn tus_resumable_missing_is_precondition() {
        let err = validate_tus_resumable(&hmap(&[])).unwrap_err();
        assert!(matches!(err, UploadRejection::MissingTusResumable));
    }

    #[test]
    fn tus_resumable_wrong_version_is_precondition() {
        let err = validate_tus_resumable(&hmap(&[("tus-resumable", "0.2.2")])).unwrap_err();
        assert!(matches!(err, UploadRejection::UnsupportedTusVersion));
    }

    #[test]
    fn upload_length_happy() {
        assert_eq!(parse_upload_length("42").unwrap(), 42);
        assert_eq!(parse_upload_length("1").unwrap(), 1);
        assert_eq!(
            parse_upload_length(&MAX_UPLOAD_LENGTH.to_string()).unwrap(),
            MAX_UPLOAD_LENGTH
        );
    }

    #[test]
    fn upload_length_rejects_zero_negative_and_oversize() {
        assert!(matches!(
            parse_upload_length("0").unwrap_err(),
            UploadRejection::InvalidUploadLength
        ));
        assert!(matches!(
            parse_upload_length("-5").unwrap_err(),
            UploadRejection::InvalidUploadLength
        ));
        assert!(matches!(
            parse_upload_length(&(MAX_UPLOAD_LENGTH + 1).to_string()).unwrap_err(),
            UploadRejection::OversizedUploadLength
        ));
        assert!(matches!(
            parse_upload_length("not-a-number").unwrap_err(),
            UploadRejection::InvalidUploadLength
        ));
    }

    #[test]
    fn upload_length_missing_header_rejected() {
        let err = parse_upload_length_required(&hmap(&[])).unwrap_err();
        assert!(matches!(err, UploadRejection::MissingUploadLength));
    }

    #[test]
    fn upload_length_deferred_rejected_in_slice1() {
        let err = parse_upload_length_required(&hmap(&[("upload-defer-length", "1")])).unwrap_err();
        assert!(matches!(err, UploadRejection::DeferredLengthUnsupported));
    }

    #[test]
    fn metadata_parse_pair() {
        // base64("value1") = "dmFsdWUx"
        let m = parse_upload_metadata("filename dmFsdWUx,filetype dGV4dA").unwrap();
        assert_eq!(m.get("filename").unwrap(), "value1");
        assert_eq!(m.get("filetype").unwrap(), "text");
    }

    #[test]
    fn metadata_parse_allows_value_less_entry() {
        let m = parse_upload_metadata("marker").unwrap();
        assert_eq!(m.get("marker").unwrap(), "");
    }

    #[test]
    fn metadata_parse_rejects_invalid_base64() {
        let err = parse_upload_metadata("filename ???").unwrap_err();
        assert!(matches!(err, UploadRejection::InvalidUploadMetadata));
    }

    #[test]
    fn metadata_parse_rejects_control_chars_in_key() {
        let err = parse_upload_metadata("bad\tkey dmFsdWUx").unwrap_err();
        assert!(matches!(err, UploadRejection::InvalidUploadMetadata));
    }

    #[test]
    fn metadata_header_oversize_rejected() {
        let big = "k ".to_string() + &"A".repeat(MAX_UPLOAD_METADATA_LEN);
        let err = parse_upload_metadata_header(&hmap(&[("upload-metadata", &big)])).unwrap_err();
        assert!(matches!(err, UploadRejection::InvalidUploadMetadata));
    }

    #[test]
    fn reject_crlf_catches_all_line_separator_forms() {
        // Security audit SEC-M2 regression guard — exercises the CRLF
        // predicate directly so the guarantee is independent of hyper's
        // `HeaderValue` constructors (which *also* reject CR/LF, making
        // it impossible to feed a poisoned HeaderMap through the
        // full `parse_upload_metadata_header` path at test time).
        assert!(matches!(
            reject_crlf("filename dmFsdWU\r\nX-Evil: 1").unwrap_err(),
            UploadRejection::InvalidUploadMetadata
        ));
        assert!(matches!(
            reject_crlf("only-lf\nplain").unwrap_err(),
            UploadRejection::InvalidUploadMetadata
        ));
        assert!(matches!(
            reject_crlf("only-cr\rmarker").unwrap_err(),
            UploadRejection::InvalidUploadMetadata
        ));
        // Well-formed input passes.
        reject_crlf("filename dmFsdWU,filetype dGV4dA").unwrap();
        reject_crlf("marker").unwrap();
    }

    #[test]
    fn object_key_matches_adr_0004_shape() {
        let g = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let u = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        assert_eq!(
            build_object_key(g, u),
            "11111111-1111-1111-1111-111111111111/uploads/22222222-2222-2222-2222-222222222222/v1"
        );
    }

    // ─── Slice 2 unit tests (plan 0044 §7.1) ───────────────────────────

    #[test]
    fn patch_content_type_happy() {
        validate_patch_content_type(&hmap(&[(
            "content-type",
            "application/offset+octet-stream",
        )]))
        .unwrap();
    }

    #[test]
    fn patch_content_type_allows_case_insensitive() {
        validate_patch_content_type(&hmap(&[(
            "content-type",
            "Application/Offset+Octet-Stream",
        )]))
        .unwrap();
    }

    #[test]
    fn patch_content_type_allows_charset_suffix() {
        // Defense: parameterised MIME still matches the primary form.
        // tus clients rarely add parameters but proxies occasionally
        // do — be lenient on the happy path to avoid spurious 415s.
        validate_patch_content_type(&hmap(&[(
            "content-type",
            "application/offset+octet-stream; charset=binary",
        )]))
        .unwrap();
    }

    #[test]
    fn patch_content_type_rejects_json() {
        let err = validate_patch_content_type(&hmap(&[("content-type", "application/json")]))
            .unwrap_err();
        assert!(matches!(err, UploadRejection::UnsupportedContentType));
    }

    #[test]
    fn patch_content_type_rejects_missing() {
        let err = validate_patch_content_type(&hmap(&[])).unwrap_err();
        assert!(matches!(err, UploadRejection::UnsupportedContentType));
    }

    #[test]
    fn patch_offset_happy() {
        assert_eq!(parse_upload_offset("0").unwrap(), 0);
        assert_eq!(parse_upload_offset("42").unwrap(), 42);
        assert_eq!(
            parse_upload_offset(&MAX_UPLOAD_LENGTH.to_string()).unwrap(),
            MAX_UPLOAD_LENGTH
        );
    }

    #[test]
    fn patch_offset_rejects_negative_and_non_numeric() {
        assert!(matches!(
            parse_upload_offset("-1").unwrap_err(),
            UploadRejection::InvalidUploadOffset
        ));
        assert!(matches!(
            parse_upload_offset("hello").unwrap_err(),
            UploadRejection::InvalidUploadOffset
        ));
    }

    #[test]
    fn patch_offset_required_rejects_missing() {
        let err = parse_upload_offset_required(&hmap(&[])).unwrap_err();
        assert!(matches!(err, UploadRejection::MissingUploadOffset));
    }

    #[test]
    fn staging_path_is_traversal_safe() {
        use std::path::Path;
        // staging_dir is canonicalised at bootstrap; upload_id is a
        // UUID so `.join()` produces a deterministic child path.
        let staging = UploadStaging {
            staging_dir: Path::new("/tmp/uploads").to_path_buf(),
            max_patch_bytes: 1024,
            hmac_secret: vec![0u8; 32],
        };
        let uid = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
        let p = staging.staging_path(uid);
        assert!(p.ends_with("33333333-3333-3333-3333-333333333333.staging"));
        // No `..` segments introduced anywhere.
        assert!(!p.to_string_lossy().contains(".."));
    }

    #[test]
    fn upload_staging_debug_redacts_secret() {
        use std::path::Path;
        let staging = UploadStaging {
            staging_dir: Path::new("/tmp/uploads").to_path_buf(),
            max_patch_bytes: 1024,
            hmac_secret: b"supersecret-shouldnt-appear".to_vec(),
        };
        let rendered = format!("{staging:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn sha256_hex_of_known_vector() {
        // "abc" → sha256 = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex_of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
