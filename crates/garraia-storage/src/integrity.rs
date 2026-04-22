//! HMAC-SHA256 anti-tampering helpers.
//!
//! Plan 0038 §3 + ADR 0004 §Security 4. The server-side integrity HMAC
//! binds three opaque pieces of metadata together so a malicious operator
//! that swaps a blob in the bucket cannot produce a matching MAC without
//! the server key:
//!
//! `HMAC-SHA256(secret, "{object_key}:{version_id}:{sha256_hex}")`
//!
//! The HMAC is stored alongside the `file_versions` row in Postgres
//! (migration 003 column `integrity_hmac TEXT`). On `get` the caller
//! recomputes and constant-time compares — divergence yields
//! [`crate::StorageError::IntegrityMismatch`].

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Build the canonical byte string the HMAC signs.
///
/// Exposed (not just used internally) so tests and the gateway can assert
/// the exact contract without re-implementing the formatter.
pub fn canonical_input(object_key: &str, version_id: &str, sha256_hex: &str) -> String {
    format!("{object_key}:{version_id}:{sha256_hex}")
}

/// Compute HMAC-SHA256 of the canonical input, hex-encoded (lowercase).
///
/// `secret` should come from a dedicated env var
/// (`GARRAIA_STORAGE_HMAC_SECRET` in prod) and MUST NOT be reused across
/// purposes. Callers SHOULD wrap it in [`secrecy::SecretString`] at the
/// boundary — this function takes raw bytes to stay dep-light in the
/// storage crate.
pub fn compute_hmac(secret: &[u8], object_key: &str, version_id: &str, sha256_hex: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC-SHA256 accepts any key length, so `new_from_slice` cannot fail");
    mac.update(canonical_input(object_key, version_id, sha256_hex).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time verification of `expected` against a freshly computed HMAC.
///
/// Returns `Ok(())` on match. On mismatch returns a stable error message
/// that does NOT leak the expected/actual values.
pub fn verify_hmac(
    secret: &[u8],
    object_key: &str,
    version_id: &str,
    sha256_hex: &str,
    expected_hex: &str,
) -> Result<(), &'static str> {
    let got = compute_hmac(secret, object_key, version_id, sha256_hex);
    // Decode both to bytes so we compare fixed-length arrays. Different
    // lengths already signal tampering — return fail-closed.
    let lhs = match hex::decode(&got) {
        Ok(b) => b,
        Err(_) => return Err("internal HMAC hex decode failed"),
    };
    let rhs = match hex::decode(expected_hex) {
        Ok(b) => b,
        Err(_) => return Err("expected HMAC is not valid hex"),
    };
    if lhs.len() != rhs.len() {
        return Err("HMAC length mismatch");
    }
    if lhs.ct_eq(&rhs).into() {
        Ok(())
    } else {
        Err("HMAC mismatch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"hmac-test-secret-never-in-prod";

    #[test]
    fn compute_is_deterministic() {
        let a = compute_hmac(SECRET, "group/file/v1", "v1", "abcd");
        let b = compute_hmac(SECRET, "group/file/v1", "v1", "abcd");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "hex-encoded SHA-256 output");
    }

    #[test]
    fn compute_varies_on_key() {
        let a = compute_hmac(SECRET, "group/file/v1", "v1", "abcd");
        let b = compute_hmac(SECRET, "group/file/v2", "v1", "abcd");
        assert_ne!(a, b, "different object_key must yield different HMAC");
    }

    #[test]
    fn compute_varies_on_version() {
        let a = compute_hmac(SECRET, "group/file/x", "v1", "abcd");
        let b = compute_hmac(SECRET, "group/file/x", "v2", "abcd");
        assert_ne!(a, b);
    }

    #[test]
    fn compute_varies_on_sha256() {
        let a = compute_hmac(SECRET, "group/file/x", "v1", "abcd");
        let b = compute_hmac(SECRET, "group/file/x", "v1", "dcba");
        assert_ne!(a, b);
    }

    #[test]
    fn compute_varies_on_secret() {
        let a = compute_hmac(SECRET, "group/file/x", "v1", "abcd");
        let b = compute_hmac(b"other-secret", "group/file/x", "v1", "abcd");
        assert_ne!(a, b);
    }

    #[test]
    fn verify_matches_compute() {
        let expected = compute_hmac(SECRET, "k", "v1", "dead");
        assert!(verify_hmac(SECRET, "k", "v1", "dead", &expected).is_ok());
    }

    #[test]
    fn verify_detects_wrong_secret() {
        let expected = compute_hmac(SECRET, "k", "v1", "dead");
        assert_eq!(
            verify_hmac(b"wrong", "k", "v1", "dead", &expected).unwrap_err(),
            "HMAC mismatch"
        );
    }

    #[test]
    fn verify_detects_tampered_sha() {
        let expected = compute_hmac(SECRET, "k", "v1", "dead");
        assert_eq!(
            verify_hmac(SECRET, "k", "v1", "beef", &expected).unwrap_err(),
            "HMAC mismatch"
        );
    }

    #[test]
    fn verify_rejects_non_hex_expected() {
        assert_eq!(
            verify_hmac(SECRET, "k", "v1", "dead", "notvalidhex!!!").unwrap_err(),
            "expected HMAC is not valid hex"
        );
    }

    #[test]
    fn verify_rejects_length_mismatch() {
        let expected = "abcd"; // 2 bytes, not 32
        assert_eq!(
            verify_hmac(SECRET, "k", "v1", "abcd", expected).unwrap_err(),
            "HMAC length mismatch"
        );
    }

    #[test]
    fn canonical_input_is_stable_format() {
        assert_eq!(
            canonical_input("group-a/file-b/v1", "v1", "0123"),
            "group-a/file-b/v1:v1:0123"
        );
    }
}
