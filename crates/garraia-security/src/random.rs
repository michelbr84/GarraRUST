//! Bytes from the system CSPRNG, with no zeroed buffer in between.
//!
//! Before this module, every site that needed a salt, nonce, key or token
//! reimplemented the same idiom:
//!
//! ```ignore
//! let rng = SystemRandom::new();
//! let mut buf = vec![0u8; N];
//! rng.fill(&mut buf).map_err(|_| "...")?;
//! ```
//!
//! There were seven copies of it in production code, and the shape is exactly
//! what CodeQL reads as `rust/hard-coded-cryptographic-value`: a short-lived
//! buffer of zeros exists, and the analysis does not follow the `fill` on the
//! next line. That is where alert #142 (AES-256-GCM nonce in
//! `admin/secrets.rs`) and #43 (PBKDF2 salt in `credentials.rs`) came from —
//! the first fixed in code, the second dismissed in the ledger as a false
//! positive.
//!
//! [`random_bytes`] closes the whole class: `ring::rand::generate` returns the
//! array **already filled**, so no buffer of zeros ever exists in our source.
//! The length comes from the type, which also removes the chance of handing
//! `fill` a mismatched length.

use ring::rand::SystemRandom;

/// The system CSPRNG could not produce bytes.
///
/// `ring` exposes no cause detail (`ring::error::Unspecified` is opaque by
/// design), so neither does this error. Callers map it onto their own error
/// type with whatever message makes sense in their context.
#[derive(Debug, thiserror::Error)]
#[error("failed to generate random bytes")]
pub struct RandomError;

/// `N` cryptographically random bytes from the system CSPRNG.
///
/// The length is a type parameter, so callers write `random_bytes::<32>()` or
/// let inference take it from the binding.
///
/// ```
/// use garraia_security::random_bytes;
///
/// let salt: [u8; 32] = random_bytes()?;
/// let nonce = random_bytes::<12>()?;
/// assert_ne!(salt[..12], nonce[..]);
/// # Ok::<(), garraia_security::RandomError>(())
/// ```
pub fn random_bytes<const N: usize>() -> Result<[u8; N], RandomError> {
    let rng = SystemRandom::new();
    // `generate` hands back a `Random<[u8; N]>` that is already filled;
    // `expose` consumes it. No `vec![0u8; N]` exists between allocation and
    // fill, which is the whole point of this helper.
    ring::rand::generate::<[u8; N]>(&rng)
        .map(|random| random.expose())
        .map_err(|_| RandomError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_requested_length() {
        let a: [u8; 12] = random_bytes().expect("rng");
        let b: [u8; 32] = random_bytes().expect("rng");
        assert_eq!(a.len(), 12);
        assert_eq!(b.len(), 32);
    }

    #[test]
    fn successive_calls_differ() {
        // Two 32-byte draws colliding has probability 2^-256; what this test
        // actually catches is a stuck generator handing back the same buffer.
        let a: [u8; 32] = random_bytes().expect("rng");
        let b: [u8; 32] = random_bytes().expect("rng");
        assert_ne!(a, b);
    }

    #[test]
    fn output_is_not_all_zeros() {
        // Direct regression for the shape that motivated the module: if this
        // ever goes back to returning the zeroed buffer unfilled, it fails here.
        let bytes: [u8; 32] = random_bytes().expect("rng");
        assert!(bytes.iter().any(|&b| b != 0));
    }
}
