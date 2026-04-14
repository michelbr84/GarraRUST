//! Password hashing — Argon2id (current) + PBKDF2-SHA256 (legacy).
//!
//! The crate verifies stored PHC strings dispatched by prefix:
//!   - `$argon2id$...` → [`verify_argon2id`]
//!   - `$pbkdf2-sha256$...` → [`verify_pbkdf2`]
//!   - anything else → [`AuthError::UnknownHashFormat`] (NOT `Ok(None)`;
//!     unknown formats are operational misconfiguration, not credential
//!     failure).
//!
//! New writes always go through [`hash_argon2id`] using RFC 9106 first
//! recommendation parameters (`m=64MiB, t=3, p=4`). The legacy verify path
//! exists only to enable lazy upgrade of PBKDF2 hashes that pre-date GAR-391b
//! (mobile_users + GAR-413 migration tool).
//!
//! [`AuthError::UnknownHashFormat`]: crate::error::AuthError::UnknownHashFormat

use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use secrecy::{ExposeSecret, SecretString};

use crate::error::AuthError;

// `DUMMY_HASH: &str` is generated at compile time by `build.rs` and embedded
// in the binary so the constant-time anti-enumeration path does not pay the
// hashing cost on every cold start. Salt is fresh per build but the
// resulting PHC string is reused across all "user not found" / "account not
// active" calls.
include!(concat!(env!("OUT_DIR"), "/dummy_hash.rs"));

/// Argon2id parameters per RFC 9106 first recommendation.
///
/// `m_cost = 65536` KiB = 64 MiB, `t_cost = 3`, `p_cost = 4`, output 32 bytes.
/// These are the parameters validated by the GAR-391b smoke test against any
/// PHC string produced by [`hash_argon2id`].
fn argon2id() -> Argon2<'static> {
    let params = Params::new(64 * 1024, 3, 4, Some(32))
        .expect("RFC 9106 first recommendation params must be valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Produce a fresh Argon2id PHC string for a plaintext password.
///
/// Used by:
///   - `InternalProvider::create_identity` (signup)
///   - `InternalProvider::verify_credential` (lazy upgrade from PBKDF2)
pub fn hash_argon2id(password: &SecretString) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
    let argon = argon2id();
    let phc = argon
        .hash_password(password.expose_secret().as_bytes(), &salt)
        .map_err(|e| AuthError::Hashing(e.to_string()))?;
    Ok(phc.to_string())
}

/// Verify a plaintext password against an Argon2id PHC string.
///
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch, `Err(Hashing)` if
/// the PHC string is malformed or the algorithm identifier is wrong.
pub fn verify_argon2id(phc: &str, password: &SecretString) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(phc).map_err(|e| AuthError::Hashing(e.to_string()))?;
    if parsed.algorithm.as_str() != "argon2id" {
        return Err(AuthError::Hashing(format!(
            "expected argon2id PHC, got `{}`",
            parsed.algorithm
        )));
    }
    let argon = argon2id();
    match argon.verify_password(password.expose_secret().as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(AuthError::Hashing(e.to_string())),
    }
}

/// Verify a plaintext password against a PBKDF2-SHA256 PHC string.
///
/// Used **only** by the lazy-upgrade path in `verify_credential`. New writes
/// MUST go through [`hash_argon2id`]. Returns the same shape as
/// [`verify_argon2id`].
pub fn verify_pbkdf2(phc: &str, password: &SecretString) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(phc).map_err(|e| AuthError::Hashing(e.to_string()))?;
    if !parsed.algorithm.as_str().starts_with("pbkdf2") {
        return Err(AuthError::Hashing(format!(
            "expected pbkdf2-* PHC, got `{}`",
            parsed.algorithm
        )));
    }
    match pbkdf2::Pbkdf2.verify_password(password.expose_secret().as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(AuthError::Hashing(e.to_string())),
    }
}

/// Verify the supplied password against [`DUMMY_HASH`] and discard the result.
///
/// Called from the constant-time anti-enumeration path in `verify_credential`
/// when the user is not found or the account is not active. The wall-clock
/// latency of this call matches a real Argon2id verify, so an attacker cannot
/// distinguish "user does not exist" from "user exists, wrong password" by
/// timing.
///
/// The result is intentionally discarded — any error short-circuits with
/// `AuthError::Hashing` to surface configuration issues, but the boolean is
/// never inspected.
pub fn consume_dummy_hash(password: &SecretString) -> Result<(), AuthError> {
    let _ = verify_argon2id(DUMMY_HASH, password)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn dummy_hash_is_argon2id_phc() {
        assert!(
            DUMMY_HASH.starts_with("$argon2id$"),
            "DUMMY_HASH must be an argon2id PHC string, got: {DUMMY_HASH}"
        );
    }

    #[test]
    fn dummy_hash_uses_rfc_9106_params() {
        // PHC format: $argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>
        assert!(DUMMY_HASH.contains("m=65536,t=3,p=4"));
        assert!(DUMMY_HASH.contains("v=19"));
    }

    #[test]
    fn argon2id_roundtrip_positive() {
        let pw = secret("correct horse battery staple");
        let phc = hash_argon2id(&pw).expect("hash");
        assert!(phc.starts_with("$argon2id$"));
        assert!(phc.contains("m=65536,t=3,p=4"));
        assert!(verify_argon2id(&phc, &pw).expect("verify"));
    }

    #[test]
    fn argon2id_roundtrip_negative() {
        let pw = secret("correct horse battery staple");
        let phc = hash_argon2id(&pw).expect("hash");
        let wrong = secret("wrong password");
        assert!(!verify_argon2id(&phc, &wrong).expect("verify"));
    }

    #[test]
    fn argon2id_rejects_pbkdf2_phc() {
        // Synthetic pbkdf2 PHC — algorithm prefix mismatch.
        let phc = "$pbkdf2-sha256$i=600000,l=32$c2FsdHNhbHQ$aGFzaGhhc2g";
        let pw = secret("anything");
        match verify_argon2id(phc, &pw) {
            Err(AuthError::Hashing(_)) => {}
            other => panic!("expected Hashing error for pbkdf2 PHC, got {other:?}"),
        }
    }

    #[test]
    fn consume_dummy_hash_does_not_panic() {
        let pw = secret("any input");
        consume_dummy_hash(&pw).expect("dummy hash verify must succeed (boolean discarded)");
    }
}
