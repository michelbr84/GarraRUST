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

    /// A catch-all for backend-specific failures that are none of the above.
    #[error("backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
