//! `LoginPool` — the dedicated BYPASSRLS Postgres pool for credential
//! verification.
//!
//! ## Boundary contract
//!
//! `LoginPool` wraps a [`PgPool`] connected as the `garraia_login` Postgres
//! role. The inner pool is **private**. The only constructor is
//! [`LoginPool::from_dedicated_config`], which:
//!
//! 1. Validates the [`LoginConfig`] (URL scheme, pool size).
//! 2. Connects.
//! 3. Issues `SELECT current_user::text` and refuses if the answer is
//!    anything other than `garraia_login`.
//!
//! There is **no** `From<PgPool> for LoginPool` and there is **no**
//! `pub fn new(pool: PgPool) -> Self`. Adding either is a violation of
//! ADR 0005 §"Anti-patterns" #4.
//!
//! The combination of (a) private field, (b) single validating constructor,
//! and (c) `cargo deny` / code review against forbidden impls makes
//! "accidentally use the login pool from `garraia-app` code" a compile-time
//! error: any value of type `LoginPool` was either built through the
//! validating constructor or does not exist.

use serde::Deserialize;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::error::AuthError;

/// Configuration for the dedicated login pool. Loaded from a SEPARATE
/// config path than the main app pool — production deployments MUST keep
/// these credentials in a distinct vault entry (GAR-410).
///
/// `Debug` is **manually implemented** to redact `database_url`. Mirrors
/// the `garraia-workspace::WorkspaceConfig` pattern.
#[derive(Clone, Deserialize, Validate)]
pub struct LoginConfig {
    /// Postgres URL. The connection role MUST be `garraia_login`.
    /// `LoginPool::from_dedicated_config` validates this at construction
    /// time via `SELECT current_user`.
    #[validate(custom(function = "validate_postgres_url"))]
    pub database_url: String,

    /// Pool size. Default 5. Production should keep this small to bound
    /// the BYPASSRLS connection footprint.
    #[validate(range(min = 1, max = 50))]
    pub max_connections: u32,
}

impl Default for LoginConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 5,
        }
    }
}

impl std::fmt::Debug for LoginConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginConfig")
            .field("database_url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

fn validate_postgres_url(url: &str) -> Result<(), validator::ValidationError> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_postgres_scheme"))
    }
}

/// `LoginPool` wraps a [`PgPool`] connected as the `garraia_login` BYPASSRLS
/// role. See module docs for the boundary contract.
///
/// **Forbidden:** `impl From<PgPool> for LoginPool`, `pub fn new(pool: PgPool)`,
/// `#[derive(Clone)]`, or any other path that produces a `LoginPool` without
/// the `current_user` validation. ADR 0005 §"Anti-patterns" #4.
///
/// `Clone` is intentionally NOT derived — even though the inner `PgPool` is
/// reference-counted and `Clone`, exposing a `Clone` impl on `LoginPool`
/// would let any caller fan out the BYPASSRLS pool without going through
/// the validating constructor. The denial is enforced by a compile-time
/// `static_assertions::assert_not_impl_all!` test in this module.
pub struct LoginPool {
    /// Held for use by 391b (`pool()` accessor will be added then,
    /// `pub(crate)` only). The `dead_code` allow goes away once
    /// `InternalProvider::verify_credential` reads it.
    #[allow(dead_code)]
    inner: PgPool,
}

impl LoginPool {
    /// Connect to the dedicated login database using the role validation
    /// guard. Returns [`AuthError::WrongRole`] if the connection comes back
    /// as anything other than `garraia_login`.
    ///
    /// Tracing instrumentation uses `skip(config)` so the `database_url`
    /// (containing credentials) never lands in any span.
    #[instrument(skip(config), fields(max_connections = config.max_connections))]
    pub async fn from_dedicated_config(config: &LoginConfig) -> crate::Result<Self> {
        config
            .validate()
            .map_err(|e| AuthError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(AuthError::Storage)?;

        // Runtime role guard. We query `current_user` immediately after
        // connecting and refuse if it isn't `garraia_login`. Any other
        // role (postgres, garraia_app, etc.) is a misconfiguration and
        // must fail loudly.
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .map_err(AuthError::Storage)?;

        if actual != "garraia_login" {
            // Drop the pool explicitly so the open connections are
            // returned to Postgres immediately and the misconfigured
            // pool cannot be re-used by any other code path.
            pool.close().await;
            return Err(AuthError::WrongRole(actual));
        }

        Ok(Self { inner: pool })
    }
}

impl std::fmt::Debug for LoginPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the inner pool — it carries the connection string.
        f.debug_struct("LoginPool")
            .field("inner", &"<PgPool[garraia_login]>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time denial of `Clone` on `LoginPool`. Adding `#[derive(Clone)]`
    // or a manual `impl Clone for LoginPool` in the future will fail this
    // assertion at build time. See GAR-391a security review H-1.
    static_assertions::assert_not_impl_all!(LoginPool: Clone);

    #[test]
    fn debug_does_not_leak_database_url() {
        let cfg = LoginConfig {
            database_url: "postgres://supersecret:hunter2@db:5432/garraia".into(),
            max_connections: 4,
        };
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("supersecret"), "Debug must not leak: {dbg}");
        assert!(!dbg.contains("hunter2"), "Debug must not leak: {dbg}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(dbg.contains("max_connections: 4"));
    }

    #[test]
    fn validates_postgres_scheme() {
        let mut cfg = LoginConfig {
            database_url: "postgres://garraia_login:pw@h:5432/garraia".into(),
            max_connections: 5,
        };
        assert!(cfg.validate().is_ok());

        cfg.database_url = "postgresql://garraia_login:pw@h:5432/garraia".into();
        assert!(cfg.validate().is_ok());

        cfg.database_url = "mysql://x".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_max_connections_bounds() {
        let too_big = LoginConfig {
            database_url: "postgres://garraia_login:pw@h:5432/db".into(),
            max_connections: 51,
        };
        assert!(too_big.validate().is_err());

        let zero = LoginConfig {
            database_url: "postgres://garraia_login:pw@h:5432/db".into(),
            max_connections: 0,
        };
        assert!(zero.validate().is_err());
    }
}
