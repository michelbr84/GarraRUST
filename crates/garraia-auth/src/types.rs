//! Core auth types: `Identity`, `Credential`, `Principal`, `RequestCtx`.
//!
//! These are shape-only in 391a. No behavior is attached — extractor logic,
//! capability checks, and request context extraction land in 391c.

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// An authenticated identity. Returned by providers after successful
/// credential verification. Maps to a row in `user_identities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub user_id: Uuid,
    /// `'internal'`, `'oidc'`, `'saml'`, ...
    pub provider: String,
    /// Stable subject identifier from the provider. For `internal` this is
    /// the email; for OIDC it is the `sub` claim.
    pub provider_sub: String,
}

/// A credential being verified. The variant selects which provider handles
/// it. New credential types arrive via new variants — never via new trait
/// methods on `IdentityProvider` (frozen by ADR 0005).
///
/// The password field is wrapped in [`secrecy::SecretString`] so it never
/// reaches `Debug`/`Display` accidentally and is zeroed on drop. Manual
/// `Debug` impl (instead of derive) gives a stable redacted output.
#[derive(Clone)]
pub enum Credential {
    /// Email + password against `user_identities` for `provider = 'internal'`.
    /// Verification path: Argon2id (current) or PBKDF2 with lazy upgrade
    /// (legacy). Implementation in GAR-391b.
    Internal {
        email: String,
        password: SecretString,
    },
    // Future: OidcIdToken { token: SecretString, issuer: String } — ADR 0009.
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credential::Internal { email, .. } => f
                .debug_struct("Credential::Internal")
                .field("email", email)
                .field("password", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Principal — the authenticated user in the context of a specific group.
/// Carried by Axum requests after the future `Principal` extractor (391c)
/// validates the JWT and looks up group membership.
///
/// In 391a `role` is a `String` placeholder. 391c replaces it with a typed
/// `Role` enum once the capability check (`fn can(&self, action) -> bool`)
/// is implemented.
#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: Uuid,
    pub group_id: Option<Uuid>,
    /// `'owner' | 'admin' | 'member' | 'guest' | 'child'` (placeholder).
    pub role: Option<String>,
}

/// Forensic context captured by the future Axum extractor (391c) and
/// passed into every login attempt by [`crate::audit::audit_login`].
///
/// All fields are optional because every header is optional in HTTP — the
/// audit row gets `NULL` whenever the upstream proxy doesn't forward them.
#[derive(Debug, Clone, Default)]
pub struct RequestCtx {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
}

/// Result of a successful `POST /v1/auth/login` call. Used by the gateway
/// to compose the JSON response body in **391c**. Defined here in 391b
/// as load-bearing scaffolding for the refresh endpoint that is coming.
///
/// **⚠️ Not populated in GAR-391b.** The 391b endpoint returns a smaller
/// shape (`{user_id, access_token, expires_at}`) without `refresh_token`
/// because `SessionStore::issue` cannot run under the current login pool
/// grants. `LoginOutcome` becomes the canonical return shape in 391c when
/// the refresh endpoint and migration 010 land together.
///
/// `refresh_token` is the **plaintext** opaque token (32 random bytes,
/// URL-safe base64). The HMAC-SHA256 hash of this same value lives in
/// `sessions.refresh_token_hash`. Plain text leaves the gateway exactly
/// once — in the response body — and the client must store it securely.
/// `secrecy::SecretString` redacts on `Debug`, so `#[derive(Debug)]` is
/// safe.
#[derive(Debug, Clone)]
pub struct LoginOutcome {
    pub user_id: Uuid,
    pub access_token: String,
    pub refresh_token: SecretString,
    pub expires_at: DateTime<Utc>,
}
