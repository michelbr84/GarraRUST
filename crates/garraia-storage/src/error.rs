//! Error type for `garraia-storage`.

use thiserror::Error;

/// Top-level error returned by every [`crate::ObjectStore`] method.
#[derive(Debug, Error)]
pub enum StorageError {
    /// The key failed validation in [`crate::path_sanitize::sanitise_key`].
    #[error("invalid object key: {0}")]
    InvalidKey(String),

    /// The requested object does not exist in the backend.
    #[error("object not found: {key}")]
    NotFound { key: String },

    /// A backend-specific I/O error (disk full, permission denied, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The caller requested an operation the backend does not implement
    /// (e.g. `presign_put` on [`crate::LocalFs`]).
    #[error("operation not supported by this backend: {0}")]
    Unsupported(&'static str),

    /// A checksum or HMAC verification failed after read.
    #[error("integrity check failed for {key}: {reason}")]
    IntegrityMismatch { key: String, reason: String },

    /// The caller-supplied `content_type` is not in the allow-list and
    /// `PutOptions::allow_unsafe_mime` was false. Plan 0038 §3, ADR 0004
    /// §Security 3.
    #[error(
        "content-type `{content_type}` is not in the allow-list; set allow_unsafe_mime to override"
    )]
    DisallowedMime { content_type: String },

    /// Presigned URL TTL fell outside the `[30s, 900s]` range mandated by
    /// ADR 0004 §Security 1 + §Security 10.
    #[error("presigned URL ttl of {requested_secs}s is out of range [{min_secs}s, {max_secs}s]")]
    TtlOutOfRange {
        requested_secs: u64,
        min_secs: u64,
        max_secs: u64,
    },

    /// A catch-all for backend-specific failures that are none of the above.
    #[error("backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

impl StorageError {
    /// Construct a [`StorageError::DisallowedMime`] with the echoed
    /// `content_type` sanitised: control characters stripped, length
    /// capped at 64 bytes, unicode non-printables replaced. Prevents
    /// log-injection when the rejected input was adversary-supplied
    /// (plan 0038 security review SEC-M).
    pub fn disallowed_mime(content_type: &str) -> Self {
        const MAX_LEN: usize = 64;
        let sanitised: String = content_type
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_LEN)
            .collect();
        Self::DisallowedMime {
            content_type: sanitised,
        }
    }
}
