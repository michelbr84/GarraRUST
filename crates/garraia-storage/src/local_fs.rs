//! Filesystem-backed [`crate::ObjectStore`] — dev baseline.
//!
//! The backend rejects invalid keys via [`crate::path_sanitize::sanitise_key`]
//! before any I/O; combined with storing under a single base directory
//! this closes the path-traversal surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, warn};
use url::Url;

use crate::error::{Result, StorageError};
use crate::hash_util::sha256_hex;
use crate::object_store::{
    GetResult, ObjectMetadata, ObjectStore, PutOptions, check_mime_allowlist,
    maybe_compute_integrity_hmac,
};
use crate::path_sanitize::sanitise_key;

/// Store each object under `<base_dir>/<key>`.
///
/// Concurrent writers to the **same key** are not fully atomic in this
/// skeleton; callers SHOULD serialise on the logical key if they care
/// (see plan 0037 §7 SEC-H "race"). Readers see either the old or new
/// bytes depending on OS semantics.
#[derive(Debug, Clone)]
pub struct LocalFs {
    base_dir: PathBuf,
}

impl LocalFs {
    /// Create the base directory if absent and return a ready-to-use store.
    ///
    /// **Code review #1 (plan 0037):** `base_dir` is `canonicalize`d so the
    /// prefix check in [`Self::resolve`] compares apples-to-apples even when
    /// the caller passes a non-canonical path (e.g. via `TempDir::path()` on
    /// Windows which can carry a `\\?\` prefix, or via a symlink-to-dir).
    pub fn new(base_dir: impl Into<PathBuf>) -> Result<Self> {
        let requested = base_dir.into();
        if !requested.exists() {
            std::fs::create_dir_all(&requested)?;
        }
        if !requested.is_dir() {
            return Err(StorageError::Backend(format!(
                "base_dir `{}` exists and is not a directory",
                requested.display()
            )));
        }
        let base_dir = std::fs::canonicalize(&requested).map_err(|e| {
            StorageError::Backend(format!(
                "failed to canonicalize base_dir `{}`: {e}",
                requested.display()
            ))
        })?;
        Ok(Self { base_dir })
    }

    /// Build an absolute path under [`Self::base_dir`] for `key`.
    ///
    /// **SEC-F-01 (plan 0037 audit):** symlinks *within* `base_dir` are not
    /// followed by this check — a pre-existing symlink like `base_dir/link
    /// → /etc` would have its logical path pass the prefix guard and then
    /// resolve outside the base at the OS layer during `File::open`. Callers
    /// MUST ensure `base_dir` is not world-writable and contains no
    /// adversarial symlinks. Slice 2 (S3 backend) eliminates this surface
    /// entirely because object keys there are virtual, not filesystem paths.
    fn resolve(&self, key: &str) -> Result<PathBuf> {
        let key = sanitise_key(key)?;
        let mut path = self.base_dir.clone();
        for seg in key.split('/') {
            path.push(seg);
        }
        if !path.starts_with(&self.base_dir) {
            return Err(StorageError::InvalidKey(format!(
                "resolved path `{}` escapes base_dir",
                path.display()
            )));
        }
        Ok(path)
    }
}

#[async_trait]
impl ObjectStore for LocalFs {
    async fn put(&self, key: &str, bytes: Bytes, opts: PutOptions) -> Result<ObjectMetadata> {
        check_mime_allowlist(&opts)?;
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let size_bytes = bytes.len() as u64;
        let etag = sha256_hex(&bytes);
        let integrity_hmac = maybe_compute_integrity_hmac(&opts, key, &etag);
        let mut file = tokio::fs::File::create(&path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        debug!(target: "garraia_storage::local_fs", "put key={key} size={size_bytes}");
        Ok(ObjectMetadata {
            key: key.to_owned(),
            size_bytes,
            etag_sha256: etag,
            content_type: opts.content_type,
            integrity_hmac,
        })
    }

    async fn get(&self, key: &str) -> Result<GetResult> {
        let path = self.resolve(key)?;
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound {
                    key: key.to_owned(),
                });
            }
            Err(e) => return Err(StorageError::Io(e)),
        };
        let mut buf: Vec<u8> = Vec::new();
        file.read_to_end(&mut buf).await?;
        let size_bytes = buf.len() as u64;
        let etag = sha256_hex(&buf);
        Ok(GetResult {
            bytes: Bytes::from(buf),
            metadata: ObjectMetadata {
                key: key.to_owned(),
                size_bytes,
                etag_sha256: etag,
                content_type: None,
                integrity_hmac: None,
            },
        })
    }

    async fn head(&self, key: &str) -> Result<ObjectMetadata> {
        let path = self.resolve(key)?;
        let meta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound {
                    key: key.to_owned(),
                });
            }
            Err(e) => return Err(StorageError::Io(e)),
        };
        // `head` could in principle be cheaper than reading the file, but
        // `etag_sha256` requires hashing content. We fall back to full read
        // here to preserve the invariant that put/head return the same etag.
        // TODO(slice-2): persist the etag in a sidecar (e.g. `key.meta`) or
        // move etag computation out of `head` for large files — current
        // implementation loads the entire object into RAM for every call.
        // SEC-F-04 (plan 0037 audit).
        // TODO(slice-2): `content_type` is not persisted on disk today, so
        // `head` always returns `None` even when `put` was called with a
        // `content_type`. Code review MEDIUM #2 — sidecar `.meta` file or
        // filesystem xattr would close the gap.
        let bytes = tokio::fs::read(&path).await?;
        let etag = sha256_hex(&bytes);
        Ok(ObjectMetadata {
            key: key.to_owned(),
            size_bytes: meta.len(),
            etag_sha256: etag,
            content_type: None,
            integrity_hmac: None,
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.resolve(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Idempotent: deleting a missing key is not an error.
                Ok(())
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let path = self.resolve(key)?;
        match tokio::fs::metadata(&path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn presign_put(&self, _key: &str, _ttl: Duration) -> Result<Url> {
        warn!(target: "garraia_storage::local_fs", "presign_put called on LocalFs; not supported");
        Err(StorageError::Unsupported(
            "LocalFs does not serve presigned URLs; use S3/MinIO backend or a direct HTTP endpoint",
        ))
    }

    async fn presign_get(&self, _key: &str, _ttl: Duration) -> Result<Url> {
        warn!(target: "garraia_storage::local_fs", "presign_get called on LocalFs; not supported");
        Err(StorageError::Unsupported(
            "LocalFs does not serve presigned URLs; use S3/MinIO backend or a direct HTTP endpoint",
        ))
    }
}

/// Wrap `Path` helpers so callers outside the crate can inspect without
/// exposing the whole `LocalFs` internals.
impl LocalFs {
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, LocalFs) {
        let dir = TempDir::new().expect("tempdir");
        let store = LocalFs::new(dir.path()).expect("store");
        (dir, store)
    }

    #[tokio::test]
    async fn put_get_roundtrip() {
        let (_dir, store) = fresh();
        let bytes = Bytes::from_static(b"hello world");
        let meta = store
            .put("a/b/c.txt", bytes.clone(), PutOptions::default())
            .await
            .unwrap();
        assert_eq!(meta.size_bytes, 11);
        assert_eq!(meta.etag_sha256.len(), 64);
        assert_eq!(meta.key, "a/b/c.txt");

        let got = store.get("a/b/c.txt").await.unwrap();
        assert_eq!(got.bytes, bytes);
        assert_eq!(got.metadata.etag_sha256, meta.etag_sha256);
    }

    #[tokio::test]
    async fn head_matches_put_etag() {
        let (_dir, store) = fresh();
        let bytes = Bytes::from_static(b"checksum payload");
        let put_meta = store
            .put("doc/one", bytes, PutOptions::default())
            .await
            .unwrap();
        let head_meta = store.head("doc/one").await.unwrap();
        assert_eq!(put_meta.etag_sha256, head_meta.etag_sha256);
        assert_eq!(put_meta.size_bytes, head_meta.size_bytes);
    }

    #[tokio::test]
    async fn get_missing_returns_not_found() {
        let (_dir, store) = fresh();
        let err = store.get("ghost").await.unwrap_err();
        match err {
            StorageError::NotFound { key } => assert_eq!(key, "ghost"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn head_missing_returns_not_found() {
        let (_dir, store) = fresh();
        let err = store.head("nada").await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound { .. }));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let (_dir, store) = fresh();
        store
            .put("d", Bytes::from_static(b"x"), PutOptions::default())
            .await
            .unwrap();
        store.delete("d").await.unwrap();
        // Delete again — must not error.
        store.delete("d").await.unwrap();
        assert!(!store.exists("d").await.unwrap());
    }

    #[tokio::test]
    async fn exists_reflects_put_and_delete() {
        let (_dir, store) = fresh();
        assert!(!store.exists("k").await.unwrap());
        store
            .put("k", Bytes::from_static(b"v"), PutOptions::default())
            .await
            .unwrap();
        assert!(store.exists("k").await.unwrap());
        store.delete("k").await.unwrap();
        assert!(!store.exists("k").await.unwrap());
    }

    #[tokio::test]
    async fn put_rejects_invalid_key() {
        let (_dir, store) = fresh();
        let err = store
            .put("../etc", Bytes::from_static(b"x"), PutOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::InvalidKey(_)));
    }

    #[tokio::test]
    async fn presign_is_unsupported() {
        let (_dir, store) = fresh();
        let err = store
            .presign_put("k", Duration::from_secs(60))
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Unsupported(_)));

        let err = store
            .presign_get("k", Duration::from_secs(60))
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Unsupported(_)));
    }

    #[tokio::test]
    async fn overwrite_updates_etag() {
        let (_dir, store) = fresh();
        let m1 = store
            .put("file", Bytes::from_static(b"aaaa"), PutOptions::default())
            .await
            .unwrap();
        let m2 = store
            .put("file", Bytes::from_static(b"bbbb"), PutOptions::default())
            .await
            .unwrap();
        assert_ne!(m1.etag_sha256, m2.etag_sha256);
        assert_eq!(m2.size_bytes, 4);
    }

    #[tokio::test]
    async fn put_preserves_content_type_in_metadata() {
        let (_dir, store) = fresh();
        let meta = store
            .put(
                "image",
                Bytes::from_static(b"img"),
                PutOptions {
                    content_type: Some("image/png".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(meta.content_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn put_rejects_disallowed_mime() {
        let (_dir, store) = fresh();
        let err = store
            .put(
                "bad",
                Bytes::from_static(b"x"),
                PutOptions {
                    content_type: Some("application/x-msdownload".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::DisallowedMime { .. }));
    }

    #[tokio::test]
    async fn put_allows_disallowed_mime_when_opted_in() {
        let (_dir, store) = fresh();
        let meta = store
            .put(
                "archive.exe",
                Bytes::from_static(b"pe"),
                PutOptions {
                    content_type: Some("application/x-msdownload".into()),
                    allow_unsafe_mime: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            meta.content_type.as_deref(),
            Some("application/x-msdownload")
        );
    }

    #[tokio::test]
    async fn put_computes_integrity_hmac_when_material_present() {
        let (_dir, store) = fresh();
        let meta = store
            .put(
                "group/file/v1",
                Bytes::from_static(b"payload"),
                PutOptions {
                    version_id: Some("v1".into()),
                    hmac_secret: Some(b"storage-secret".to_vec()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let hmac = meta.integrity_hmac.expect("integrity_hmac populated");
        assert_eq!(hmac.len(), 64);
    }

    #[tokio::test]
    async fn put_integrity_hmac_absent_without_material() {
        let (_dir, store) = fresh();
        let meta = store
            .put("g/f/v1", Bytes::from_static(b"p"), PutOptions::default())
            .await
            .unwrap();
        assert!(meta.integrity_hmac.is_none());
    }

    #[tokio::test]
    async fn get_with_verifies_matching_hmac() {
        let (_dir, store) = fresh();
        let put_meta = store
            .put(
                "g/f/v1",
                Bytes::from_static(b"bytes"),
                PutOptions {
                    version_id: Some("v1".into()),
                    hmac_secret: Some(b"key".to_vec()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let expected = put_meta.integrity_hmac.clone().unwrap();
        let got = store
            .get_with(
                "g/f/v1",
                crate::GetOptions {
                    expected_integrity_hmac: Some(expected),
                    version_id: Some("v1".into()),
                    hmac_secret: Some(b"key".to_vec()),
                },
            )
            .await
            .unwrap();
        assert_eq!(&got.bytes[..], b"bytes");
    }

    #[tokio::test]
    async fn get_with_rejects_tampered_hmac() {
        let (_dir, store) = fresh();
        store
            .put(
                "g/f/v1",
                Bytes::from_static(b"bytes"),
                PutOptions {
                    version_id: Some("v1".into()),
                    hmac_secret: Some(b"key".to_vec()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let err = store
            .get_with(
                "g/f/v1",
                crate::GetOptions {
                    expected_integrity_hmac: Some("00".repeat(32)),
                    version_id: Some("v1".into()),
                    hmac_secret: Some(b"key".to_vec()),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::IntegrityMismatch { .. }));
    }

    #[test]
    fn new_rejects_path_that_is_not_a_directory() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("notadir");
        std::fs::write(&file_path, b"x").unwrap();
        let err = LocalFs::new(&file_path).unwrap_err();
        assert!(matches!(err, StorageError::Backend(_)));
    }
}
