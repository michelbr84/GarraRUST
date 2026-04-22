//! `ObjectStore` trait + request/response types.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::Result;

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
}

/// Options passed to [`ObjectStore::put`]. `Default` is always safe — the
/// backend decides what metadata to persist when the option is `None`.
#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    pub content_type: Option<String>,
    /// Optional cache-control hint surfaced to the CDN/browser when the
    /// backend supports it (S3 does; LocalFs ignores it).
    pub cache_control: Option<String>,
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

    /// Fetch the bytes + metadata for `key`.
    async fn get(&self, key: &str) -> Result<GetResult>;

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
