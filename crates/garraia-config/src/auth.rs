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
//! | `GARRAIA_JWT_SECRET` | yes | ≥32 bytes; **same env var as `mobile_auth.rs` legacy** so both flows share one secret in dev. When absent, `from_env` falls back to `GarraIA_VAULT_PASSPHRASE` (plan 0046 slice 3 — legacy compat). |
//! | `GarraIA_VAULT_PASSPHRASE` | fallback | Legacy alias accepted by `from_env` when `GARRAIA_JWT_SECRET` is missing. Preserved for zero-breaking-change dev workflows (plan 0046). Prefer `GARRAIA_JWT_SECRET` for new deployments. |
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
    ///
    /// Plan 0046 slice 3: `GARRAIA_JWT_SECRET` takes precedence over the
    /// legacy `GarraIA_VAULT_PASSPHRASE` fallback. The fallback exists
    /// solely to preserve dev workflows that predate GAR-379 —
    /// production deployments SHOULD set `GARRAIA_JWT_SECRET` explicitly.
    pub fn from_env() -> Result<Option<Self>, AuthConfigError> {
        let jwt = match std::env::var("GARRAIA_JWT_SECRET")
            .or_else(|_| std::env::var("GarraIA_VAULT_PASSPHRASE"))
        {
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
    ///
    /// Plan 0046 slice 3: `GARRAIA_JWT_SECRET` takes precedence over the
    /// legacy `GarraIA_VAULT_PASSPHRASE` fallback. When neither is set,
    /// the error surfaces `GARRAIA_JWT_SECRET` as the canonical name.
    pub fn require_from_env() -> Result<Self, AuthConfigError> {
        let jwt = std::env::var("GARRAIA_JWT_SECRET")
            .or_else(|_| std::env::var("GarraIA_VAULT_PASSPHRASE"))
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

    // ── Plan 0046 slice 3: env-var fallback tests ─────────────────────────
    //
    // These tests mutate process-global environment state, so they MUST
    // run serially. A `LazyLock<Mutex<()>>` guard forces serialization
    // even when cargo launches tests in parallel. Every test takes the
    // lock, snapshots + clears the env vars it cares about, runs the
    // assertion, and restores the original values on exit.

    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    /// Snapshot of the env vars this test module touches. Used to restore
    /// the outer environment when a test exits so concurrent non-auth
    /// suites never observe a half-cleared state.
    struct EnvSnapshot {
        jwt: Option<String>,
        vault: Option<String>,
        refresh: Option<String>,
        login_db: Option<String>,
        signup_db: Option<String>,
        app_db: Option<String>,
    }

    impl EnvSnapshot {
        fn capture() -> Self {
            Self {
                jwt: std::env::var("GARRAIA_JWT_SECRET").ok(),
                vault: std::env::var("GarraIA_VAULT_PASSPHRASE").ok(),
                refresh: std::env::var("GARRAIA_REFRESH_HMAC_SECRET").ok(),
                login_db: std::env::var("GARRAIA_LOGIN_DATABASE_URL").ok(),
                signup_db: std::env::var("GARRAIA_SIGNUP_DATABASE_URL").ok(),
                app_db: std::env::var("GARRAIA_APP_DATABASE_URL").ok(),
            }
        }

        fn restore(self) {
            // SAFETY: all env mutations in this module are serialized by
            // ENV_LOCK; restoring happens while the lock is still held.
            unsafe fn set_or_clear(key: &str, v: Option<String>) {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(key, val),
                        None => std::env::remove_var(key),
                    }
                }
            }
            unsafe {
                set_or_clear("GARRAIA_JWT_SECRET", self.jwt);
                set_or_clear("GarraIA_VAULT_PASSPHRASE", self.vault);
                set_or_clear("GARRAIA_REFRESH_HMAC_SECRET", self.refresh);
                set_or_clear("GARRAIA_LOGIN_DATABASE_URL", self.login_db);
                set_or_clear("GARRAIA_SIGNUP_DATABASE_URL", self.signup_db);
                set_or_clear("GARRAIA_APP_DATABASE_URL", self.app_db);
            }
        }
    }

    fn clear_all_auth_env() {
        // SAFETY: ENV_LOCK held by caller.
        unsafe {
            std::env::remove_var("GARRAIA_JWT_SECRET");
            std::env::remove_var("GarraIA_VAULT_PASSPHRASE");
            std::env::remove_var("GARRAIA_REFRESH_HMAC_SECRET");
            std::env::remove_var("GARRAIA_LOGIN_DATABASE_URL");
            std::env::remove_var("GARRAIA_SIGNUP_DATABASE_URL");
            std::env::remove_var("GARRAIA_APP_DATABASE_URL");
        }
    }

    fn set_required_except_jwt() {
        // SAFETY: ENV_LOCK held by caller.
        unsafe {
            std::env::set_var("GARRAIA_REFRESH_HMAC_SECRET", "r".repeat(32));
            std::env::set_var(
                "GARRAIA_LOGIN_DATABASE_URL",
                "postgres://garraia_login:pw@localhost/garraia",
            );
            std::env::set_var(
                "GARRAIA_SIGNUP_DATABASE_URL",
                "postgres://garraia_signup:pw@localhost/garraia",
            );
        }
    }

    #[test]
    fn from_env_prefers_jwt_secret_over_vault_passphrase() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = EnvSnapshot::capture();
        clear_all_auth_env();
        // SAFETY: ENV_LOCK held.
        unsafe {
            std::env::set_var("GARRAIA_JWT_SECRET", "J".repeat(32));
            std::env::set_var("GarraIA_VAULT_PASSPHRASE", "V".repeat(32));
        }
        set_required_except_jwt();

        let cfg = AuthConfig::from_env()
            .expect("should parse")
            .expect("should be Some");
        assert_eq!(
            cfg.jwt_secret.expose_secret(),
            "J".repeat(32),
            "GARRAIA_JWT_SECRET must win over GarraIA_VAULT_PASSPHRASE"
        );

        snapshot.restore();
    }

    #[test]
    fn from_env_accepts_only_vault_passphrase_when_jwt_secret_missing() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = EnvSnapshot::capture();
        clear_all_auth_env();
        // SAFETY: ENV_LOCK held.
        unsafe {
            std::env::set_var("GarraIA_VAULT_PASSPHRASE", "V".repeat(32));
        }
        set_required_except_jwt();

        let cfg = AuthConfig::from_env()
            .expect("should parse")
            .expect("legacy fallback should be accepted");
        assert_eq!(cfg.jwt_secret.expose_secret(), "V".repeat(32));

        snapshot.restore();
    }

    #[test]
    fn from_env_returns_none_when_neither_jwt_env_is_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = EnvSnapshot::capture();
        clear_all_auth_env();
        set_required_except_jwt();

        let cfg = AuthConfig::from_env().expect("should parse");
        assert!(
            cfg.is_none(),
            "absence of both env vars must return Ok(None), got Some"
        );

        snapshot.restore();
    }
}
