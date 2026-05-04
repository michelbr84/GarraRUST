//! JWT issuance + verification (HS256) and refresh-token primitives.
//!
//! Two distinct token types:
//!   1. **Access token** — JWT HS256, 15-min TTL, carries `sub=user_id`,
//!      `iat`, `exp`, `iss="garraia-gateway"`. The signing secret comes from
//!      the env var `GARRAIA_JWT_SECRET` (≥32 bytes). `garraia-config`
//!      integration is deferred to GAR-391c.
//!   2. **Refresh token** — 32 random bytes (URL-safe base64, no padding),
//!      hashed via HMAC-SHA256 with a SEPARATE secret
//!      (`GARRAIA_REFRESH_HMAC_SECRET`) and stored in
//!      `sessions.refresh_token_hash`. Plaintext leaves the gateway exactly
//!      once (in the login response).
//!
//! Algorithm-confusion hardening: `Validation::new(Algorithm::HS256)` rejects
//! `none` and any asymmetric algorithm at decode time.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::AuthError;

type HmacSha256 = Hmac<Sha256>;

const ACCESS_TTL_SECS: i64 = 15 * 60;
const ISSUER: &str = "garraia-gateway";

/// Access token claims. `group_id` is intentionally NOT in the access token —
/// the extractor (391c) resolves the active group per-request from
/// `X-Group-Id` + `group_members`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: Uuid,
    pub iat: i64,
    pub exp: i64,
    pub iss: String,
}

/// Configuration for [`JwtIssuer`]. Both secrets are wrapped in
/// [`SecretString`] so they never reach `Debug`/`Display`.
#[derive(Clone)]
pub struct JwtConfig {
    /// HS256 signing secret. Must be ≥ 32 bytes after UTF-8 decoding.
    /// Source: env `GARRAIA_JWT_SECRET` in 391b. 391c migrates to
    /// `garraia-config` + vault.
    pub jwt_secret: SecretString,
    /// HMAC-SHA256 key for refresh-token hashing. **Distinct** from
    /// `jwt_secret` so a compromise of one does not compromise the other.
    /// Source: env `GARRAIA_REFRESH_HMAC_SECRET`. Must be ≥ 32 bytes.
    pub refresh_hmac_secret: SecretString,
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("refresh_hmac_secret", &"[REDACTED]")
            .finish()
    }
}

/// Plain refresh token + its HMAC hash. Returned by [`JwtIssuer::issue_refresh`].
/// The plaintext leaves the gateway in the login response; the hash is what
/// gets stored in `sessions.refresh_token_hash`.
#[derive(Clone)]
pub struct RefreshTokenPair {
    pub plaintext: SecretString,
    pub hmac_hash: String,
}

impl std::fmt::Debug for RefreshTokenPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshTokenPair")
            .field("plaintext", &"[REDACTED]")
            .field("hmac_hash", &"[HEX_REDACTED]")
            .finish()
    }
}

/// Issuer + verifier for access tokens; producer of refresh-token pairs.
pub struct JwtIssuer {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtIssuer {
    /// Build a `JwtIssuer` from validated config. Returns
    /// [`AuthError::Config`] if either secret is shorter than 32 bytes.
    pub fn new(config: JwtConfig) -> Result<Self, AuthError> {
        let jwt_bytes = config.jwt_secret.expose_secret().as_bytes();
        if jwt_bytes.len() < 32 {
            return Err(AuthError::Config(
                "GARRAIA_JWT_SECRET must be at least 32 bytes".into(),
            ));
        }
        if config.refresh_hmac_secret.expose_secret().len() < 32 {
            return Err(AuthError::Config(
                "GARRAIA_REFRESH_HMAC_SECRET must be at least 32 bytes".into(),
            ));
        }

        let encoding_key = EncodingKey::from_secret(jwt_bytes);
        let decoding_key = DecodingKey::from_secret(jwt_bytes);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        // Force `sub`, `exp`, and `iss` to be present at decode time.
        // Without this, a token missing any of these would still pass the
        // signature check and rely on serde to default the field — which
        // is unsafe for `sub` (Uuid). Security review 391b M-1.
        validation.set_required_spec_claims(&["sub", "exp", "iss"]);
        validation.validate_exp = true;
        validation.leeway = 30; // seconds; tolerates clock skew

        Ok(Self {
            config,
            encoding_key,
            decoding_key,
            validation,
        })
    }

    /// Issue an access token for `user_id`. Returns the JWT string and the
    /// absolute expiry timestamp.
    pub fn issue_access(&self, user_id: Uuid) -> Result<(String, DateTime<Utc>), AuthError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(ACCESS_TTL_SECS);
        let claims = AccessClaims {
            sub: user_id,
            iat: now.timestamp(),
            exp: exp.timestamp(),
            iss: ISSUER.to_string(),
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)
            .map_err(AuthError::JwtIssue)?;
        Ok((token, exp))
    }

    /// Verify an access token and return the claims. Rejects `none` and
    /// any non-HS256 algorithm via the validation built in `new`.
    pub fn verify_access(&self, token: &str) -> Result<AccessClaims, AuthError> {
        let data = decode::<AccessClaims>(token, &self.decoding_key, &self.validation)
            .map_err(AuthError::JwtIssue)?;
        Ok(data.claims)
    }

    /// **Test-only constructor** — plan 0016 M2-T1.
    ///
    /// Builds a `JwtIssuer` from a single string secret used for BOTH
    /// `jwt_secret` and `refresh_hmac_secret`. The secret is
    /// deterministically padded to 32 bytes if shorter. This is the
    /// only way the gateway integration harness can produce a working
    /// `JwtIssuer` without threading the production `JwtConfig` /
    /// `AuthConfig` plumbing through test infra.
    ///
    /// Gated behind `#[cfg(any(test, feature = "test-support"))]` so
    /// it is invisible to production builds. Mirrors the pattern
    /// already used by `LoginPool::raw` and `SignupPool::raw`.
    ///
    /// **Never** call this from non-test code. An audit rule:
    /// `rg 'new_for_test' crates/` must return only hits preceded by
    /// the `#[cfg(...)]` gate or inside `tests/` directories.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(secret: &str) -> Self {
        // Pad the secret to 32 bytes if shorter, preserving it as
        // a prefix. Longer secrets pass through. This keeps the
        // test ergonomic — callers pass a short literal — while
        // still satisfying the ≥32-byte validation that `new` enforces.
        let mut padded = secret.to_string();
        while padded.len() < 32 {
            padded.push('=');
        }
        let cfg = JwtConfig {
            jwt_secret: SecretString::from(padded.clone()),
            refresh_hmac_secret: SecretString::from(padded),
        };
        Self::new(cfg).expect("JwtIssuer::new_for_test should always succeed after padding")
    }

    /// **Test-only access-token minter** — plan 0016 M2-T1.
    ///
    /// Thin wrapper over [`Self::issue_access`] that discards the
    /// expiry timestamp and panics on error. Convenient for tests
    /// that just want a valid bearer token for a given user.
    ///
    /// Gated behind `#[cfg(any(test, feature = "test-support"))]`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn issue_access_for_test(&self, user_id: Uuid) -> String {
        self.issue_access(user_id)
            .map(|(token, _exp)| token)
            .expect("issue_access_for_test should always succeed on a test issuer")
    }

    /// Generate a fresh opaque refresh token and its HMAC-SHA256 hash.
    /// 32 random bytes via `getrandom::fill` (direct OS RNG syscall),
    /// URL-safe base64 (no padding). Plan
    /// personal-api-key-revogada-vectorized-matsumoto §Decisões §3 moved
    /// from `rand::rngs::OsRng + TryRngCore::try_fill_bytes` to
    /// `getrandom::fill` to decouple this crate from the rand 0.9/0.10
    /// transitive-resolution churn that broke Dependabot PR #103.
    pub fn issue_refresh(&self) -> Result<RefreshTokenPair, AuthError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| AuthError::Config(format!("getrandom failure: {e}")))?;
        let plaintext = URL_SAFE_NO_PAD.encode(bytes);
        let hmac_hash = self.hmac_refresh(&plaintext)?;
        Ok(RefreshTokenPair {
            plaintext: SecretString::from(plaintext),
            hmac_hash,
        })
    }

    /// HMAC-SHA256 of a refresh-token plaintext, hex-encoded for storage in
    /// `sessions.refresh_token_hash`. Used by both `issue_refresh` and
    /// `SessionStore::verify_refresh`.
    ///
    /// Returns `Err(AuthError::Config)` only if `HmacSha256::new_from_slice`
    /// rejects the key (which only happens for an empty key — and we
    /// validate `>= 32 bytes` in `JwtIssuer::new`, so in practice this
    /// branch is unreachable). Code review 391c #1: `expect()` was
    /// replaced with `?` propagation per CLAUDE.md rule 4 (no expect/unwrap
    /// in production paths).
    pub fn hmac_refresh(&self, plaintext: &str) -> Result<String, AuthError> {
        let mut mac =
            HmacSha256::new_from_slice(self.config.refresh_hmac_secret.expose_secret().as_bytes())
                .map_err(|e| AuthError::Config(format!("hmac key invalid: {e}")))?;
        mac.update(plaintext.as_bytes());
        let bytes = mac.finalize().into_bytes();
        Ok(hex_encode(&bytes))
    }
}

/// Extract the bearer token from an HTTP `Authorization` header.
///
/// Returns `Some(token)` if the header is present and starts with
/// `Bearer ` (case-insensitive on the scheme). `None` otherwise.
pub fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    // Case-insensitive "Bearer " prefix, per RFC 7235 §2.1.
    if value.len() < 7 {
        return None;
    }
    let (scheme, rest) = value.split_at(7);
    if scheme.eq_ignore_ascii_case("Bearer ") {
        Some(rest.trim_start())
    } else {
        None
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> JwtConfig {
        JwtConfig {
            jwt_secret: SecretString::from("a".repeat(32)),
            refresh_hmac_secret: SecretString::from("b".repeat(32)),
        }
    }

    #[test]
    fn rejects_short_jwt_secret() {
        let bad = JwtConfig {
            jwt_secret: SecretString::from("too-short".to_owned()),
            refresh_hmac_secret: SecretString::from("b".repeat(32)),
        };
        assert!(matches!(JwtIssuer::new(bad), Err(AuthError::Config(_))));
    }

    #[test]
    fn rejects_short_refresh_secret() {
        let bad = JwtConfig {
            jwt_secret: SecretString::from("a".repeat(32)),
            refresh_hmac_secret: SecretString::from("nope".to_owned()),
        };
        assert!(matches!(JwtIssuer::new(bad), Err(AuthError::Config(_))));
    }

    #[test]
    fn issue_then_verify_roundtrip() {
        let issuer = JwtIssuer::new(cfg()).expect("ctor");
        let user = Uuid::now_v7();
        let (token, exp) = issuer.issue_access(user).expect("issue");
        let claims = issuer.verify_access(&token).expect("verify");
        assert_eq!(claims.sub, user);
        assert_eq!(claims.iss, ISSUER);
        assert_eq!(claims.exp, exp.timestamp());
    }

    #[test]
    fn verify_rejects_wrong_secret() {
        let issuer1 = JwtIssuer::new(cfg()).unwrap();
        let issuer2 = JwtIssuer::new(JwtConfig {
            jwt_secret: SecretString::from("c".repeat(32)),
            refresh_hmac_secret: SecretString::from("b".repeat(32)),
        })
        .unwrap();
        let (token, _) = issuer1.issue_access(Uuid::now_v7()).unwrap();
        assert!(matches!(
            issuer2.verify_access(&token),
            Err(AuthError::JwtIssue(_))
        ));
    }

    #[test]
    fn verify_rejects_expired_token() {
        let issuer = JwtIssuer::new(cfg()).unwrap();
        // Manually craft an expired claim.
        let now = Utc::now();
        let claims = AccessClaims {
            sub: Uuid::now_v7(),
            iat: now.timestamp() - 3600,
            exp: now.timestamp() - 1800,
            iss: ISSUER.to_string(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &issuer.encoding_key,
        )
        .unwrap();
        assert!(matches!(
            issuer.verify_access(&token),
            Err(AuthError::JwtIssue(_))
        ));
    }

    #[test]
    fn verify_rejects_none_algorithm() {
        let issuer = JwtIssuer::new(cfg()).unwrap();
        // Hand-craft a "none" alg token: header={"alg":"none","typ":"JWT"}.body.
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let claims = AccessClaims {
            sub: Uuid::now_v7(),
            iat: 0,
            exp: i64::MAX,
            iss: ISSUER.to_string(),
        };
        let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let token = format!("{header}.{body}.");
        assert!(matches!(
            issuer.verify_access(&token),
            Err(AuthError::JwtIssue(_))
        ));
    }

    #[test]
    fn extract_bearer_token_parses_header() {
        use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};

        let mut h = HeaderMap::new();
        assert_eq!(extract_bearer_token(&h), None);

        h.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer abc.def.ghi"),
        );
        assert_eq!(extract_bearer_token(&h), Some("abc.def.ghi"));

        // Case-insensitive scheme.
        h.insert(AUTHORIZATION, HeaderValue::from_static("bearer xyz"));
        assert_eq!(extract_bearer_token(&h), Some("xyz"));

        h.insert(
            AUTHORIZATION,
            HeaderValue::from_static("BEARER  with-space"),
        );
        assert_eq!(extract_bearer_token(&h), Some("with-space"));

        // Wrong scheme.
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwdw=="),
        );
        assert_eq!(extract_bearer_token(&h), None);

        // Too short.
        h.insert(AUTHORIZATION, HeaderValue::from_static("short"));
        assert_eq!(extract_bearer_token(&h), None);
    }

    #[test]
    fn refresh_token_pair_shape_and_hmac_stable() {
        let issuer = JwtIssuer::new(cfg()).unwrap();
        let pair = issuer.issue_refresh().expect("issue refresh");
        // Plaintext: 32 bytes -> base64 url-safe no pad = 43 chars.
        assert_eq!(pair.plaintext.expose_secret().len(), 43);
        // HMAC SHA-256 hex = 64 chars.
        assert_eq!(pair.hmac_hash.len(), 64);
        // hmac_refresh is deterministic for the same plaintext + same key.
        let recomputed = issuer
            .hmac_refresh(pair.plaintext.expose_secret())
            .expect("hmac_refresh must not fail with a 32-byte secret");
        assert_eq!(recomputed, pair.hmac_hash);
    }

    /// GAR-468 Q6.6 — kills mutant `jwt.rs:63` (`Debug for JwtConfig`
    /// → `Ok(Default::default())`). The `Debug` impl is hand-rolled to
    /// redact both secrets; this test asserts the redaction is observable
    /// (the mutant produces empty output that lacks `[REDACTED]` markers).
    #[test]
    fn debug_for_jwt_config_redacts_both_secrets() {
        let cfg = JwtConfig {
            jwt_secret: SecretString::from("jwt-super-secret-32-bytes-AAAA!!".to_owned()),
            refresh_hmac_secret: SecretString::from("refresh-hmac-32-bytes-BBBBBBBBB!!".to_owned()),
        };
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("jwt-super-secret"),
            "Debug must not leak jwt_secret: {dbg}"
        );
        assert!(
            !dbg.contains("refresh-hmac"),
            "Debug must not leak refresh_hmac_secret: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "redaction marker missing: {dbg}"
        );
    }

    /// GAR-468 Q6.6 — kills mutant `jwt.rs:81` (`Debug for RefreshTokenPair`
    /// → `Ok(Default::default())`). Asserts the plaintext is masked and
    /// the hmac_hash is hex-redacted, regardless of input.
    #[test]
    fn debug_for_refresh_token_pair_redacts_plaintext_and_hash() {
        let pair = RefreshTokenPair {
            plaintext: SecretString::from("super-secret-token-plaintext-XYZ".to_owned()),
            hmac_hash: "deadbeefcafebabe1234567890abcdef".to_owned(),
        };
        let dbg = format!("{pair:?}");
        assert!(
            !dbg.contains("super-secret-token"),
            "Debug must not leak plaintext: {dbg}"
        );
        assert!(
            !dbg.contains("deadbeefcafebabe"),
            "Debug must not leak hmac_hash: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "plaintext marker missing: {dbg}"
        );
        assert!(
            dbg.contains("[HEX_REDACTED]"),
            "hmac_hash marker missing: {dbg}"
        );
    }

    #[test]
    fn new_for_test_and_issue_access_for_test_roundtrip() {
        // Short secret: padded to 32 bytes internally.
        let issuer = JwtIssuer::new_for_test("unit-secret");
        // Use a deterministic non-nil UUID so the `sub` claim is
        // unmistakable in the assertion. Parsing is infallible on a
        // literal — no v4 feature required.
        let uid = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let token = issuer.issue_access_for_test(uid);
        let claims = issuer.verify_access(&token).expect("verify");
        assert_eq!(claims.sub, uid);
        assert_eq!(claims.iss, ISSUER);
    }

    /// GAR-505 #1+#2 — kills `jwt.rs:31` `*` → `+` (yields TTL = 75) and
    /// `*` → `/` (yields TTL = 0). Asserts the access-token expiry window
    /// is exactly 900 seconds, the contract of `15 * 60`.
    #[test]
    fn access_token_ttl_window_is_900_seconds() {
        let issuer = JwtIssuer::new(cfg()).expect("ctor");
        let (token, _) = issuer.issue_access(Uuid::now_v7()).expect("issue");
        let claims = issuer.verify_access(&token).expect("verify");
        assert_eq!(
            claims.exp - claims.iat,
            900,
            "ACCESS_TTL_SECS must be 15 * 60 = 900 seconds"
        );
    }

    /// GAR-505 #3 — kills `jwt.rs:177` `<` → `<=` in `JwtIssuer::new_for_test`.
    /// The padding loop must stop at `padded.len() == 32` (original `<`),
    /// not at `padded.len() == 33` (mutant `<=`). With a 32-byte input the
    /// mutant pushes one extra `=` to 33 bytes; the resulting HMAC differs
    /// from the oracle computed against the literal 32-byte secret.
    #[test]
    fn new_for_test_does_not_pad_already_32_byte_secret() {
        // `Mac` trait brings `new_from_slice`, `update`, `finalize` into scope
        // for the local oracle. Keep the import local so the rest of the test
        // module is unaffected.
        use hmac::Mac;

        let secret = "x".repeat(32);
        let issuer = JwtIssuer::new_for_test(&secret);
        let plaintext = "GAR-505-oracle";
        let actual = issuer
            .hmac_refresh(plaintext)
            .expect("hmac_refresh on 32-byte secret");

        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("32 bytes is a valid HMAC key");
        mac.update(plaintext.as_bytes());
        let expected = hex_encode(&mac.finalize().into_bytes());

        assert_eq!(
            actual, expected,
            "secret must NOT be padded when input length is already 32"
        );
    }

    /// GAR-505 #4 — kills `jwt.rs:250` `<` → `<=` in `extract_bearer_token`.
    /// Header `"Bearer "` (exactly 7 bytes) must pass the length guard
    /// under the original `<`, returning `Some("")`. The mutant `<=` would
    /// reject and return `None`.
    #[test]
    fn extract_bearer_token_accepts_seven_char_boundary() {
        use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};

        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(
            extract_bearer_token(&h),
            Some(""),
            "header `Bearer ` (7 bytes) must satisfy `len < 7` rejection guard"
        );
    }
}
