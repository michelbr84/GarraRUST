//! `ObjectStore` trait + request/response types.

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncRead;
use url::Url;

use crate::error::{Result, StorageError};
use crate::integrity;

/// Boxed, pinned, `Send` async reader — the value type the streaming
/// [`ObjectStore::put_stream`] accepts. Box + Pin + Send is the minimal
/// set of bounds that lets the trait remain `dyn`-compatible under
/// `async_trait` (plan 0047 §streaming). Callers construct with e.g.
/// `Box::pin(tokio::fs::File::open(path).await?)`.
pub type AsyncByteReader = Pin<Box<dyn AsyncRead + Send>>;

/// Metadata returned by [`ObjectStore::put`] and [`ObjectStore::head`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectMetadata {
    /// Canonical object key.
    pub key: String,
    /// Size in bytes as stored.
    pub size_bytes: u64,
    /// Hex-encoded SHA-256 of the bytes stored.
    /// Feeds `file_versions.checksum_sha256` (migration 003 regex
    /// `^[0-9a-f]{64}$`).
    pub etag_sha256: String,
    /// Optional content type hint supplied by the caller.
    pub content_type: Option<String>,
    /// Hex-encoded HMAC-SHA256 of `"{object_key}:{version_id}:{etag_sha256}"`
    /// when the caller supplied `PutOptions::hmac_secret` + `version_id`.
    /// Plan 0038 §3, ADR 0004 §Security 4. Feeds
    /// `file_versions.integrity_hmac`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity_hmac: Option<String>,
}

/// Options passed to [`ObjectStore::put`]. `Default` is always safe — the
/// backend decides what metadata to persist when the option is `None`.
///
/// Plan 0038 extends this with MIME + HMAC material; existing callers that
/// build `PutOptions { content_type, cache_control }` explicitly continue
/// to compile because both new fields live on the struct-update path and
/// ship sane defaults (MIME strict, HMAC off).
#[derive(Clone, Default)]
pub struct PutOptions {
    pub content_type: Option<String>,
    /// Optional cache-control hint surfaced to the CDN/browser when the
    /// backend supports it (S3 does; LocalFs ignores it).
    pub cache_control: Option<String>,
    /// When `content_type` is `Some` and this is `false` (the default),
    /// the backend rejects uploads whose MIME type is not in the
    /// [`crate::mime_allowlist::DEFAULT_ALLOWED`] list. Set to `true` at
    /// the caller layer only after logging an audit event
    /// (`file.unsafe_mime_accepted`). ADR 0004 §Security 3.
    pub allow_unsafe_mime: bool,
    /// Optional version identifier that the caller knows will be
    /// persisted in `file_versions.version`. Used to salt the
    /// [`Self::hmac_secret`] computation.
    pub version_id: Option<String>,
    /// Server-side HMAC key material. When `Some` **and** `version_id` is
    /// `Some`, the backend computes
    /// `HMAC-SHA256(secret, "{key}:{version_id}:{sha256_hex}")` and
    /// returns the hex value in [`ObjectMetadata::integrity_hmac`].
    ///
    /// In production wire this to `GARRAIA_STORAGE_HMAC_SECRET` (a key
    /// dedicated to storage integrity — do NOT reuse
    /// `GARRAIA_REFRESH_HMAC_SECRET`). Callers SHOULD zeroize the Vec
    /// after `put` returns.
    pub hmac_secret: Option<Vec<u8>>,
}

impl fmt::Debug for PutOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Redact PII-adjacent and secret fields; disclose only presence.
        f.debug_struct("PutOptions")
            .field("content_type", &self.content_type)
            .field("cache_control", &self.cache_control)
            .field("allow_unsafe_mime", &self.allow_unsafe_mime)
            .field("version_id", &self.version_id.as_ref().map(|_| "<set>"))
            .field(
                "hmac_secret",
                &self.hmac_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Options passed to [`ObjectStore::get_with`]. `Default` behaves
/// identically to [`ObjectStore::get`] (no integrity verification).
#[derive(Clone, Default)]
pub struct GetOptions {
    /// Caller-expected integrity HMAC (hex). When set together with
    /// `hmac_secret` + `version_id` the backend recomputes and
    /// constant-time-compares before returning bytes; divergence yields
    /// [`StorageError::IntegrityMismatch`].
    pub expected_integrity_hmac: Option<String>,
    pub version_id: Option<String>,
    pub hmac_secret: Option<Vec<u8>>,
}

impl fmt::Debug for GetOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GetOptions")
            .field(
                "expected_integrity_hmac",
                &self.expected_integrity_hmac.as_ref().map(|_| "<set>"),
            )
            .field("version_id", &self.version_id.as_ref().map(|_| "<set>"))
            .field(
                "hmac_secret",
                &self.hmac_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Bytes + metadata returned by [`ObjectStore::get`].
#[derive(Debug, Clone)]
pub struct GetResult {
    pub bytes: Bytes,
    pub metadata: ObjectMetadata,
}

/// Abstraction over a blob storage backend.
///
/// See the crate-level doc for scope and non-goals. The trait uses
/// [`async_trait`] because `dyn ObjectStore` will be common in Fase 3.5
/// where multiple backends co-exist at runtime (LocalFs dev, S3 prod).
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    /// Upload `bytes` under `key`, replacing any previous content.
    async fn put(&self, key: &str, bytes: Bytes, opts: PutOptions) -> Result<ObjectMetadata>;

    /// Streaming upload under `key`, replacing any previous content.
    ///
    /// Plan 0047 (GAR-395 slice 3). Default implementation buffers the
    /// full stream into memory and delegates to [`Self::put`] — this keeps
    /// the contract backwards-compatible for existing callers and lets
    /// backends opt into true streaming (LocalFs: `tokio::io::copy` into
    /// an open `File`; S3: multipart upload) without breaking the world.
    ///
    /// `content_length` is the **expected** byte count — backends MAY use
    /// it for pre-allocation (e.g. S3 multipart part sizing). A caller
    /// that does not know the length up-front should buffer via `put`.
    /// The actual bytes consumed from `reader` drive the final etag and
    /// `ObjectMetadata::size_bytes`.
    ///
    /// **Semantics of failure mid-stream:** backends MUST leave the
    /// target key in a consistent state — either the old content
    /// (replace failed, nothing visible) or the new full content
    /// (replace succeeded). A backend that cannot guarantee this (e.g.
    /// a naive LocalFs write-in-place) MUST write to a temp path and
    /// atomically rename on success, OR override this default with a
    /// safer path.
    async fn put_stream(
        &self,
        key: &str,
        reader: AsyncByteReader,
        content_length: u64,
        opts: PutOptions,
    ) -> Result<ObjectMetadata> {
        // Default: buffer into memory up to `content_length` bytes and
        // delegate. Overridable — see `LocalFs::put_stream`.
        let _ = content_length; // advisory for default impl; backends override
        let mut reader = reader;
        let mut buf: Vec<u8> = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf)
            .await
            .map_err(StorageError::Io)?;
        self.put(key, Bytes::from(buf), opts).await
    }

    /// Fetch the bytes + metadata for `key`.
    async fn get(&self, key: &str) -> Result<GetResult>;

    /// Fetch with optional integrity verification. Default implementation
    /// calls [`Self::get`] and applies the HMAC check after the bytes
    /// arrive; backends that can verify server-side (future S3
    /// extension) may override.
    async fn get_with(&self, key: &str, opts: GetOptions) -> Result<GetResult> {
        let result = self.get(key).await?;
        maybe_verify_integrity(key, &result.metadata, &opts)?;
        Ok(result)
    }

    /// Return only the metadata for `key` — cheaper than [`Self::get`].
    async fn head(&self, key: &str) -> Result<ObjectMetadata>;

    /// Remove `key` from the backend. Idempotent: deleting a missing key
    /// returns `Ok(())`.
    async fn delete(&self, key: &str) -> Result<()>;

    /// Check whether `key` currently exists.
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Issue a time-limited URL the caller can PUT directly against.
    /// `LocalFs` returns [`crate::StorageError::Unsupported`].
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url>;

    /// Issue a time-limited URL the caller can GET directly against.
    /// `LocalFs` returns [`crate::StorageError::Unsupported`].
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url>;
}

/// Presigned URL TTL lower bound (ADR 0004 §Security 10).
pub const PRESIGN_TTL_MIN: Duration = Duration::from_secs(30);
/// Presigned URL TTL upper bound (ADR 0004 §Security 1).
pub const PRESIGN_TTL_MAX: Duration = Duration::from_secs(15 * 60);

/// Guard helper used by backends that support presigned URLs. Returns
/// `StorageError::TtlOutOfRange` when `ttl` is not in `[30s, 900s]`.
///
/// Visibility is `pub(crate)` because only backend `impl`s call this —
/// consumers (`garraia-gateway` slice 3) rely on `presign_*` which
/// enforces the range internally. When only the `LocalFs` backend is
/// compiled (default), the helper is unused — suppress the warning
/// via `#[allow(dead_code)]` on that path.
#[cfg_attr(not(feature = "storage-s3"), allow(dead_code))]
pub(crate) fn check_presign_ttl(ttl: Duration) -> Result<()> {
    if ttl < PRESIGN_TTL_MIN || ttl > PRESIGN_TTL_MAX {
        return Err(StorageError::TtlOutOfRange {
            requested_secs: ttl.as_secs(),
            min_secs: PRESIGN_TTL_MIN.as_secs(),
            max_secs: PRESIGN_TTL_MAX.as_secs(),
        });
    }
    Ok(())
}

/// Shared helper used by backends to either reject based on MIME
/// allow-list or let the upload proceed. Centralised here so LocalFs and
/// S3Compatible apply the exact same rule (plan 0038 §5.2).
///
/// Visibility is `pub(crate)` — callers interact via `put`, which
/// delegates to this helper.
pub(crate) fn check_mime_allowlist(opts: &PutOptions) -> Result<()> {
    let Some(ct) = opts.content_type.as_deref() else {
        // No type declared — the backend cannot classify. Downstream
        // slice may switch to deny-by-default; for now log at the
        // caller layer when this path is taken on public uploads.
        return Ok(());
    };
    if opts.allow_unsafe_mime {
        tracing::warn!(
            target: "garraia_storage::mime",
            content_type = ct,
            "allow_unsafe_mime=true; upload bypasses allow-list — caller must emit `file.unsafe_mime_accepted` audit event",
        );
        return Ok(());
    }
    if !crate::mime_allowlist::is_mime_allowed(ct) {
        return Err(StorageError::disallowed_mime(ct));
    }
    Ok(())
}

/// Compute the integrity HMAC when the caller supplied both pieces of
/// material. Returns `None` when either is absent.
pub fn maybe_compute_integrity_hmac(
    opts: &PutOptions,
    key: &str,
    sha256_hex: &str,
) -> Option<String> {
    match (&opts.version_id, &opts.hmac_secret) {
        (Some(v), Some(secret)) => Some(integrity::compute_hmac(secret, key, v, sha256_hex)),
        _ => None,
    }
}

fn maybe_verify_integrity(key: &str, meta: &ObjectMetadata, opts: &GetOptions) -> Result<()> {
    let Some(expected) = opts.expected_integrity_hmac.as_deref() else {
        return Ok(());
    };
    let (Some(secret), Some(version)) = (opts.hmac_secret.as_deref(), opts.version_id.as_deref())
    else {
        return Err(StorageError::IntegrityMismatch {
            key: key.to_owned(),
            reason: "expected_integrity_hmac set without hmac_secret + version_id".into(),
        });
    };
    integrity::verify_hmac(secret, key, version, &meta.etag_sha256, expected).map_err(|reason| {
        StorageError::IntegrityMismatch {
            key: key.to_owned(),
            reason: reason.to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ttl_rejects_too_short() {
        let err = check_presign_ttl(Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, StorageError::TtlOutOfRange { .. }));
    }

    #[test]
    fn check_ttl_rejects_too_long() {
        let err = check_presign_ttl(Duration::from_secs(3600)).unwrap_err();
        assert!(matches!(err, StorageError::TtlOutOfRange { .. }));
    }

    #[test]
    fn check_ttl_accepts_edges() {
        check_presign_ttl(PRESIGN_TTL_MIN).unwrap();
        check_presign_ttl(PRESIGN_TTL_MAX).unwrap();
        check_presign_ttl(Duration::from_secs(300)).unwrap();
    }

    #[test]
    fn mime_check_allows_without_content_type() {
        let opts = PutOptions::default();
        check_mime_allowlist(&opts).unwrap();
    }

    #[test]
    fn mime_check_allows_whitelisted() {
        let opts = PutOptions {
            content_type: Some("image/png".into()),
            ..Default::default()
        };
        check_mime_allowlist(&opts).unwrap();
    }

    #[test]
    fn mime_check_rejects_disallowed() {
        let opts = PutOptions {
            content_type: Some("application/x-msdownload".into()),
            ..Default::default()
        };
        let err = check_mime_allowlist(&opts).unwrap_err();
        assert!(matches!(err, StorageError::DisallowedMime { .. }));
    }

    #[test]
    fn mime_check_bypass_with_allow_unsafe_mime() {
        let opts = PutOptions {
            content_type: Some("application/x-executable".into()),
            allow_unsafe_mime: true,
            ..Default::default()
        };
        check_mime_allowlist(&opts).unwrap();
    }

    #[test]
    fn maybe_compute_integrity_hmac_requires_both() {
        let sha = "abc";
        let only_secret = PutOptions {
            hmac_secret: Some(b"k".to_vec()),
            ..Default::default()
        };
        assert!(maybe_compute_integrity_hmac(&only_secret, "k", sha).is_none());

        let only_version = PutOptions {
            version_id: Some("v1".into()),
            ..Default::default()
        };
        assert!(maybe_compute_integrity_hmac(&only_version, "k", sha).is_none());

        let both = PutOptions {
            version_id: Some("v1".into()),
            hmac_secret: Some(b"k".to_vec()),
            ..Default::default()
        };
        let hmac = maybe_compute_integrity_hmac(&both, "k", sha).unwrap();
        assert_eq!(hmac.len(), 64);
    }

    #[test]
    fn put_options_debug_is_redacted() {
        let opts = PutOptions {
            content_type: Some("image/png".into()),
            version_id: Some("v7".into()),
            hmac_secret: Some(b"supersecret".to_vec()),
            ..Default::default()
        };
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(!rendered.contains("v7"));
        assert!(rendered.contains("<redacted>"));
    }
}
