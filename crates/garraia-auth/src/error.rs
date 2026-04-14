//! `AuthError` — every failure surface of the `garraia-auth` crate.
//!
//! Variants intentionally do NOT carry the database URL, the password,
//! or any PII. `WrongRole` carries the actual role name returned by
//! `SELECT current_user`, which is non-sensitive (it's just a Postgres
//! role identifier — never a credential).

use thiserror::Error;

/// Errors surfaced by the `garraia-auth` crate.
///
/// `Storage` wraps the underlying `sqlx::Error` so callers can match on
/// connection failures vs constraint violations vs query errors. Real
/// `verify_credential` arrives in GAR-391b — until then most variants
/// are unused outside of the `LoginPool` constructor.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Stub return for skeleton methods. Real bodies in GAR-391b/c.
    #[error("not implemented in 391a skeleton — see GAR-391b for real impl")]
    NotImplemented,

    /// Configuration rejected by validation (bad URL, bad pool size, etc.).
    /// The string is the validator error report; the database URL is NOT
    /// included.
    #[error("auth config invalid: {0}")]
    Config(String),

    /// `LoginPool::from_dedicated_config` was given credentials for a role
    /// other than `garraia_login`. The actual role name is included so
    /// operators can diagnose the misconfiguration.
    #[error("login pool connected as `{0}`, expected `garraia_login`")]
    WrongRole(String),

    /// Underlying sqlx error (connect, query, transaction, decode).
    ///
    /// **PII warning (GAR-391a security review H-3).** Some `sqlx::Error`
    /// variants — notably `Io` and `Configuration` originating from
    /// `PgConnectOptions` — embed the connection string in their `Debug`
    /// output. Callers MUST NOT log this variant at `{:?}` or `{:#}` depth
    /// without first redacting the connection URL. The recommended pattern
    /// is `tracing::error!(error = %err, "storage failure")` (Display only,
    /// not Debug). 391b will introduce a redacting wrapper before any
    /// production logging path touches `AuthError::Storage`.
    #[error("storage error: {0}")]
    Storage(#[source] sqlx::Error),

    /// Generic credential rejection. Used by the constant-time path in
    /// 391b — same return for "user not found", "wrong password", and
    /// "account suspended" so callers cannot enumerate.
    #[error("invalid credentials")]
    InvalidCredentials,

    /// The provider does not handle this `Credential` variant
    /// (e.g., handing an OIDC token to `InternalProvider`).
    #[error("unsupported credential variant for provider `{0}`")]
    UnsupportedCredential(String),

    /// Stored hash format unrecognized — neither PBKDF2 nor Argon2id PHC.
    #[error("hash format unrecognized")]
    UnknownHashFormat,

    /// External identity provider unavailable (OIDC issuer down, etc.).
    /// First field is provider id, second is the underlying reason.
    #[error("provider `{0}` unavailable: {1}")]
    ProviderUnavailable(String, String),

    /// Account exists but is not in the `active` status (suspended/deleted).
    /// The Display intentionally does NOT distinguish between suspended and
    /// deleted to avoid leaking account state to the caller. `verify_credential`
    /// converts this into the same `Ok(None)` return as wrong-password to
    /// keep the response shape uniform.
    #[error("account is not active")]
    AccountNotActive,

    /// JWT issuance or verification error.
    ///
    /// **PII warning.** `jsonwebtoken::errors::Error` Display does NOT embed
    /// the secret or the token, so it is safe to log at `{}`. Avoid `{:?}`
    /// regardless out of caution.
    #[error("jwt error: {0}")]
    JwtIssue(#[source] jsonwebtoken::errors::Error),

    /// Password hashing or PHC parsing error. The string is the underlying
    /// `argon2`/`pbkdf2`/`password-hash` error and never embeds the password
    /// itself.
    #[error("hashing error: {0}")]
    Hashing(String),

    // 391c-impl-B
    /// A signup attempt collided with an existing `users.email` (or the
    /// corresponding `user_identities.provider_sub`). Surfaced by
    /// [`crate::internal::signup_user`] so the gateway can respond with
    /// HTTP 409 Conflict.
    #[error("duplicate email")]
    DuplicateEmail,
}
