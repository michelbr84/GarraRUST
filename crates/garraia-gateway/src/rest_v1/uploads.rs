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

use axum::extract::{Path, State};
use axum::http::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::engine::general_purpose::STANDARD_NO_PAD as BASE64_NOPAD;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use garraia_auth::Principal;
use serde::Serialize;
use std::collections::HashMap;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Cap mirrors `files.size_bytes` CHECK from migration 003 — 5 GiB.
const MAX_UPLOAD_LENGTH: i64 = 5 * 1024 * 1024 * 1024;

/// Plan 0041 §5.4 — only 1.0.0 is accepted.
const TUS_RESUMABLE_VERSION: &str = "1.0.0";

/// 24h default per GAR-395 acceptance criteria; slice 3 adds the
/// purge worker.
const UPLOAD_EXPIRY_HOURS: i64 = 24;

/// Max raw bytes the `Upload-Metadata` header may carry. Defensive
/// cap against DoS; tus spec is silent on a concrete limit.
const MAX_UPLOAD_METADATA_LEN: usize = 1024;

// ─── Tus header name constants (lowercase — Axum normalises) ────────────
static H_TUS_RESUMABLE: HeaderName = HeaderName::from_static("tus-resumable");
static H_TUS_VERSION: HeaderName = HeaderName::from_static("tus-version");
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
        }
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

    // SET LOCAL pair — group isolation + user attribution. Interpolated
    // via `format!` because SET LOCAL does not accept bind parameters.
    // Uuid::Display is always 36 hex-with-dash chars; injection-safe.
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_user_id")))?;
    sqlx::query(&format!("SET LOCAL app.current_group_id = '{}'", group_id))
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_group_id")))?;

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
        HeaderValue::from_str(&format!("/v1/uploads/{upload_id}")).expect("uuid is ascii"),
    );
    response_headers.insert(
        H_TUS_RESUMABLE.clone(),
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

    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_user_id")))?;
    sqlx::query(&format!("SET LOCAL app.current_group_id = '{}'", group_id))
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e).context("set current_group_id")))?;

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
        HeaderValue::from_str(&upload_offset.to_string()).expect("i64 is ascii"),
    );
    response_headers.insert(
        H_UPLOAD_LENGTH.clone(),
        HeaderValue::from_str(&upload_length.to_string()).expect("i64 is ascii"),
    );
    if let Some(raw) = metadata_raw {
        if let Ok(v) = HeaderValue::from_str(&raw) {
            response_headers.insert(H_UPLOAD_METADATA.clone(), v);
        }
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
    let parsed = parse_upload_metadata(raw)?;
    Ok(Some(UploadMetadata {
        raw: raw.to_string(),
        parsed,
    }))
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
    fn object_key_matches_adr_0004_shape() {
        let g = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let u = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        assert_eq!(
            build_object_key(g, u),
            "11111111-1111-1111-1111-111111111111/uploads/22222222-2222-2222-2222-222222222222/v1"
        );
    }
}
