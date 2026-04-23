//! Tiny utility helpers shared between `rest_v1::uploads` and
//! `uploads_worker`. Kept in a dedicated module so the worker does not
//! pull in the entire rest_v1 handler surface.
//!
//! Plan 0047 (GAR-395 slice 3).

/// Hex-encoded SHA-256 of a byte slice. Used to hash `object_key`
/// before it is stored in `audit_events.metadata.object_key_hash` —
/// operators can correlate audit rows with bucket keys without the raw
/// key (which leaks `group_id` + `upload_id`) being persisted twice.
pub fn sha256_hex_of(input: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(input);
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vector() {
        // NIST CAVS: SHA-256 of the empty string.
        assert_eq!(
            sha256_hex_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
    }

    #[test]
    fn sha256_hex_is_stable_for_same_input() {
        let a = sha256_hex_of(b"group-123/uploads/aaa/v1");
        let b = sha256_hex_of(b"group-123/uploads/aaa/v1");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn sha256_hex_diverges_for_different_input() {
        let a = sha256_hex_of(b"group-123/uploads/aaa/v1");
        let b = sha256_hex_of(b"group-456/uploads/aaa/v1");
        assert_ne!(a, b);
    }
}
