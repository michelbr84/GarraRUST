//! `AuthConfig` — load + validate the secrets the `garraia-auth` crate needs
//! to wire `LoginPool`, `SignupPool`, `JwtIssuer`, and `SessionStore` into the
//! gateway's [`AppState`] (GAR-391c).
//!
//! All fields are wrapped in [`secrecy::SecretString`] so they never reach
//! `Debug`/`Display` output. The struct's manual `Debug` impl prints
//! `[REDACTED]` placeholders. `from_env` validates that the JWT and refresh
//! HMAC secrets are at least 32 bytes long.
//!
//! ## Env var contract
//!
//! | Variable | Required | Notes |
//! |---|---|---|
//! | `GARRAIA_JWT_SECRET` | yes | ≥32 bytes; **same env var as `mobile_auth.rs` legacy** so both flows share one secret in dev. |
//! | `GARRAIA_REFRESH_HMAC_SECRET` | yes | ≥32 bytes; **distinct** from `GARRAIA_JWT_SECRET`. |
//! | `GARRAIA_LOGIN_DATABASE_URL` | yes | Postgres URL connecting as the `garraia_login` BYPASSRLS role. |
//! | `GARRAIA_SIGNUP_DATABASE_URL` | yes | Postgres URL connecting as the `garraia_signup` BYPASSRLS role. |
//! | `GARRAIA_APP_DATABASE_URL` | **optional** | Postgres URL connecting as the `garraia_app` RLS-enforced role. Used by `/v1/*` handlers outside the auth flow. When absent, `/v1/groups` and future write endpoints fail-soft to 503; `/v1/me` still works. Added in plan 0016 M1. |
//!
//! When any **required** var is missing, [`AuthConfig::from_env`] returns
//! `Ok(None)` (NOT an error) so the gateway boots in fail-soft mode with the
//! `/v1/auth/*` endpoints disabled. The bootstrap layer logs a warning.
//! `GARRAIA_APP_DATABASE_URL` is optional and does NOT trigger fail-soft
//! — its absence only degrades the `/v1/*` handler surface, not the auth
//! flow.
//! Production deployments verify the config at startup via
//! [`AuthConfig::require_from_env`] which errors instead.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use thiserror::Error;
use validator::Validate;

#[derive(Debug, Error)]
pub enum AuthConfigError {
    #[error("auth config invalid: {0}")]
    Validation(String),
    #[error("required env var `{0}` is missing")]
    MissingEnv(&'static str),
}

#[derive(Clone, Deserialize, Validate)]
pub struct AuthConfig {
    /// HS256 JWT signing secret. Loaded from `GARRAIA_JWT_SECRET`.
    /// Must be ≥32 bytes after UTF-8 decoding (validated at construction).
    pub jwt_secret: SecretString,

    /// HMAC-SHA256 key for refresh-token hashing. Loaded from
    /// `GARRAIA_REFRESH_HMAC_SECRET`. **Distinct** from `jwt_secret`.
    pub refresh_hmac_secret: SecretString,

    /// Postgres connection URL for the `garraia_login` BYPASSRLS pool.
    /// Used by `LoginPool` and `SessionStore`.
    pub login_database_url: SecretString,

    /// Postgres connection URL for the `garraia_signup` BYPASSRLS pool.
    /// Used by `SignupPool` exclusively.
    pub signup_database_url: SecretString,

    /// Postgres connection URL for the `garraia_app` RLS-enforced pool.
    /// Used by `AppPool` (plan 0016 M1) for `/v1/*` handlers outside
    /// the auth flow. **Optional** — when absent, `/v1/groups` and
    /// future write endpoints fail-soft to 503, but the core auth
    /// flow and `/v1/me` continue to work. Added in plan 0016 M1
    /// without breaking existing callers.
    pub app_database_url: Option<SecretString>,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("refresh_hmac_secret", &"[REDACTED]")
            .field("login_database_url", &"[REDACTED]")
            .field("signup_database_url", &"[REDACTED]")
            .field(
                "app_database_url",
                &self.app_database_url.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl AuthConfig {
    /// Validate the loaded config. Called by `from_env` / `require_from_env`
    /// after loading from the environment.
    fn validate_secrets(&self) -> Result<(), AuthConfigError> {
        if self.jwt_secret.expose_secret().len() < 32 {
            return Err(AuthConfigError::Validation(
                "GARRAIA_JWT_SECRET must be at least 32 bytes".into(),
            ));
        }
        if self.refresh_hmac_secret.expose_secret().len() < 32 {
            return Err(AuthConfigError::Validation(
                "GARRAIA_REFRESH_HMAC_SECRET must be at least 32 bytes".into(),
            ));
        }
        let login_url = self.login_database_url.expose_secret();
        if !(login_url.starts_with("postgres://") || login_url.starts_with("postgresql://")) {
            return Err(AuthConfigError::Validation(
                "GARRAIA_LOGIN_DATABASE_URL must be a postgres:// URL".into(),
            ));
        }
        let signup_url = self.signup_database_url.expose_secret();
        if !(signup_url.starts_with("postgres://") || signup_url.starts_with("postgresql://")) {
            return Err(AuthConfigError::Validation(
                "GARRAIA_SIGNUP_DATABASE_URL must be a postgres:// URL".into(),
            ));
        }
        if let Some(app_url) = self.app_database_url.as_ref() {
            let app_url = app_url.expose_secret();
            if !(app_url.starts_with("postgres://") || app_url.starts_with("postgresql://")) {
                return Err(AuthConfigError::Validation(
                    "GARRAIA_APP_DATABASE_URL must be a postgres:// URL".into(),
                ));
            }
        }
        Ok(())
    }

    /// Fail-soft env loader. Returns `Ok(None)` when any required variable
    /// is missing (gateway boots without auth endpoints + warns), `Ok(Some)`
    /// when all are present and valid, `Err` on validation failure.
    pub fn from_env() -> Result<Option<Self>, AuthConfigError> {
        let jwt = match std::env::var("GARRAIA_JWT_SECRET") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let refresh = match std::env::var("GARRAIA_REFRESH_HMAC_SECRET") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let login_db = match std::env::var("GARRAIA_LOGIN_DATABASE_URL") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let signup_db = match std::env::var("GARRAIA_SIGNUP_DATABASE_URL") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };

        // Optional: GARRAIA_APP_DATABASE_URL. Plan 0016 M1 — absence
        // does NOT trigger fail-soft of the whole AuthConfig; it only
        // means AppPool will not be constructed and /v1/groups-style
        // handlers will answer 503.
        let app_db = std::env::var("GARRAIA_APP_DATABASE_URL")
            .ok()
            .map(SecretString::from);

        let cfg = AuthConfig {
            jwt_secret: SecretString::from(jwt),
            refresh_hmac_secret: SecretString::from(refresh),
            login_database_url: SecretString::from(login_db),
            signup_database_url: SecretString::from(signup_db),
            app_database_url: app_db,
        };
        cfg.validate_secrets()?;
        Ok(Some(cfg))
    }

    /// Strict env loader for production. Errors on missing or invalid vars.
    pub fn require_from_env() -> Result<Self, AuthConfigError> {
        let jwt = std::env::var("GARRAIA_JWT_SECRET")
            .map_err(|_| AuthConfigError::MissingEnv("GARRAIA_JWT_SECRET"))?;
        let refresh = std::env::var("GARRAIA_REFRESH_HMAC_SECRET")
            .map_err(|_| AuthConfigError::MissingEnv("GARRAIA_REFRESH_HMAC_SECRET"))?;
        let login_db = std::env::var("GARRAIA_LOGIN_DATABASE_URL")
            .map_err(|_| AuthConfigError::MissingEnv("GARRAIA_LOGIN_DATABASE_URL"))?;
        let signup_db = std::env::var("GARRAIA_SIGNUP_DATABASE_URL")
            .map_err(|_| AuthConfigError::MissingEnv("GARRAIA_SIGNUP_DATABASE_URL"))?;

        // Optional: GARRAIA_APP_DATABASE_URL. Plan 0016 M1 — even in
        // `require_from_env` this is treated as optional. Operators
        // who require /v1/groups-style handlers are responsible for
        // failing their own boot if `app_database_url` is None.
        let app_db = std::env::var("GARRAIA_APP_DATABASE_URL")
            .ok()
            .map(SecretString::from);

        let cfg = AuthConfig {
            jwt_secret: SecretString::from(jwt),
            refresh_hmac_secret: SecretString::from(refresh),
            login_database_url: SecretString::from(login_db),
            signup_database_url: SecretString::from(signup_db),
            app_database_url: app_db,
        };
        cfg.validate_secrets()?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(secret_len: usize) -> AuthConfig {
        AuthConfig {
            jwt_secret: SecretString::from("a".repeat(secret_len)),
            refresh_hmac_secret: SecretString::from("b".repeat(secret_len)),
            login_database_url: SecretString::from(
                "postgres://garraia_login:pw@localhost/garraia".to_string(),
            ),
            signup_database_url: SecretString::from(
                "postgres://garraia_signup:pw@localhost/garraia".to_string(),
            ),
            // Plan 0016 M1: optional. Tests default to None to exercise
            // the "AuthConfig present but AppPool disabled" path.
            app_database_url: None,
        }
    }

    #[test]
    fn debug_redacts_all_secrets() {
        let cfg = mk(32);
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("aaaa"));
        assert!(!dbg.contains("postgres://"));
        assert!(dbg.contains("[REDACTED]"));
    }

    #[test]
    fn validates_jwt_secret_min_length() {
        let bad = mk(31);
        assert!(matches!(
            bad.validate_secrets(),
            Err(AuthConfigError::Validation(_))
        ));
    }

    #[test]
    fn accepts_valid_config() {
        assert!(mk(32).validate_secrets().is_ok());
        assert!(mk(64).validate_secrets().is_ok());
    }

    #[test]
    fn rejects_non_postgres_login_url() {
        let mut bad = mk(32);
        bad.login_database_url = SecretString::from("mysql://x/y".to_string());
        assert!(bad.validate_secrets().is_err());
    }
}
