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

    // ───────────────────────────────────────────────────────────────────────
    // GAR-463 Q6.1 — kill 3 mutation bypasses in this module
    //
    // Coverage targets (file:line in src/hashing.rs at PR #91):
    //   - line 81:  `verify_pbkdf2 → Ok(true)`        (any password verifies)
    //   - line 107: `consume_dummy_hash → Ok(())`     (no real argon2 work)
    //
    // The deterministic PBKDF2 fixture below is generated with a FIXED salt
    // so the PHC is fully reproducible without bringing OsRng into the test.
    // No real secret — both salt and password are public test constants.
    // ───────────────────────────────────────────────────────────────────────

    /// Synthetic PBKDF2-SHA256 PHC fixture for the public test password
    /// `"correct horse battery staple"` (xkcd 936) with FIXED 15-byte salt
    /// `"crab-salt-fixed"` (base64 `Y3JhYi1zYWx0LWZpeGVk`) and i=600_000.
    ///
    /// Reproduce via `examples/gen_pbkdf2_fixture.rs` (deleted before commit):
    /// ```ignore
    /// let salt = SaltString::from_b64("Y3JhYi1zYWx0LWZpeGVk").unwrap();
    /// Pbkdf2.hash_password_customized(
    ///     b"correct horse battery staple", None, None,
    ///     pbkdf2::Params { rounds: 600_000, output_length: 32 }, &salt,
    /// ).unwrap().to_string()
    /// ```
    const PBKDF2_FIXTURE_PHC: &str = "$pbkdf2-sha256$i=600000,l=32$Y3JhYi1zYWx0LWZpeGVk$fJC/CVFjhIg4Ba4mggBBBt9+u5ygVtyQEzEFm7qN+xE";

    #[test]
    fn pbkdf2_accepts_correct_password() {
        let pw = secret("correct horse battery staple");
        assert!(
            verify_pbkdf2(PBKDF2_FIXTURE_PHC, &pw).expect("verify"),
            "fixture PHC must verify against the public xkcd-936 password"
        );
    }

    #[test]
    fn pbkdf2_rejects_wrong_password() {
        // Mutant `verify_pbkdf2 → Ok(true)` makes this assertion FAIL.
        let wrong = secret("definitely-not-the-password");
        let result = verify_pbkdf2(PBKDF2_FIXTURE_PHC, &wrong).expect("verify");
        assert!(
            !result,
            "verify_pbkdf2 must return Ok(false) for a wrong password \
             — mutant `Ok(true)` triggers this assertion failure"
        );
    }

    #[test]
    fn pbkdf2_rejects_argon2id_phc() {
        let pw = secret("anything");
        let phc = hash_argon2id(&pw).expect("hash");
        match verify_pbkdf2(&phc, &pw) {
            Err(AuthError::Hashing(_)) => {}
            other => panic!("expected Hashing error for argon2id PHC, got {other:?}"),
        }
    }

    /// Kills mutant `consume_dummy_hash → Ok(())` (src/hashing.rs:107).
    ///
    /// `consume_dummy_hash` has no observable side-effects — its contract IS
    /// its execution time (anti-enumeration timing match against a real
    /// argon2id verify). The only way to detect the `Ok(())` mutant is to
    /// assert that real argon2id work runs.
    ///
    /// Argon2id with RFC 9106 params (m=64MiB, t=3, p=4) takes ≥ 30 ms on
    /// commodity hardware (~80–250 ms on `windows-latest` GHA runners under
    /// load per test-engineer review). 8 ms lower bound gives ~1000× over
    /// the mutated `Ok(())` path (microseconds) while staying ≥ 4× under
    /// the slowest observed real lower bound — robust against scheduler
    /// jitter on shared CI without false-positive risk.
    ///
    /// The upper bound of 10 s is a sanity check: if the call genuinely
    /// takes longer than that, the runner is hosed (memory-pressured /
    /// CPU-starved) and the test result is meaningless — surface it as a
    /// failure rather than a misleading pass.
    #[test]
    fn consume_dummy_hash_performs_real_argon2_work() {
        use std::time::{Duration, Instant};
        let pw = secret("any input");

        // Warmup: first call may pay page-fault cost on DUMMY_HASH bytes.
        consume_dummy_hash(&pw).expect("warmup");

        let start = Instant::now();
        consume_dummy_hash(&pw).expect("real call");
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(8),
            "consume_dummy_hash returned in {elapsed:?}; expected >= 8ms of \
             real argon2id work — mutant `Ok(())` returns instantly"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "consume_dummy_hash took {elapsed:?}; runner is severely \
             resource-starved — the timing assertion above cannot be \
             trusted under such conditions"
        );
    }
}
