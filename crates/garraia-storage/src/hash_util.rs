//! Shared SHA-256 helpers used by every backend to produce the
//! `etag_sha256` field on [`crate::ObjectMetadata`].
//!
//! Centralised so `LocalFs` and `S3Compatible` cannot drift on the
//! hash choice (plan 0038 code review MEDIUM).

use sha2::{Digest, Sha256};

/// Hex-encoded (lowercase) SHA-256 of `data`.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}
