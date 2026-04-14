//! `SignupPool` — the dedicated BYPASSRLS Postgres pool for the signup flow.
//!
//! ## Boundary contract
//!
//! `SignupPool` wraps a [`PgPool`] connected as the `garraia_signup` Postgres
//! role. The inner pool is **private**. The only constructor is
//! [`SignupPool::from_dedicated_config`], which:
//!
//! 1. Validates the [`SignupConfig`] (URL scheme, pool size).
//! 2. Connects.
//! 3. Issues `SELECT current_user::text` and refuses if the answer is
//!    anything other than `garraia_signup`.
//!
//! `garraia_signup` is a **separate** role from `garraia_login`: it has
//! INSERT on `users` and `user_identities` (which the login role does
//! not), and no access to `sessions` or any tenant data. See plan 0012
//! §3.1 and migration 010 for the full grant surface.
//!
//! There is **no** `From<PgPool> for SignupPool` and there is **no**
//! `pub fn new(pool: PgPool) -> Self`. Adding either is a violation of
//! ADR 0005 §"Anti-patterns" #4 applied to the signup role.
//!
//! The combination of (a) private field, (b) single validating constructor,
//! and (c) no `Clone` impl makes "accidentally use the signup pool from
//! `garraia-app` code" a compile-time error.

use serde::Deserialize;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::error::AuthError;

/// Configuration for the dedicated signup pool. Loaded from a SEPARATE
/// config path than the login pool and the main app pool.
///
/// `Debug` is **manually implemented** to redact `database_url`.
#[derive(Clone, Deserialize, Validate)]
pub struct SignupConfig {
    /// Postgres URL. The connection role MUST be `garraia_signup`.
    /// [`SignupPool::from_dedicated_config`] validates this at construction
    /// time via `SELECT current_user`.
    #[validate(custom(function = "validate_postgres_url"))]
    pub database_url: String,

    /// Pool size. Default 5. Production should keep this small — signup is
    /// a low-throughput endpoint and the BYPASSRLS connection footprint
    /// should be bounded tightly.
    #[validate(range(min = 1, max = 20))]
    pub max_connections: u32,
}

impl Default for SignupConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 5,
        }
    }
}

impl std::fmt::Debug for SignupConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupConfig")
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

/// `SignupPool` wraps a [`PgPool`] connected as the `garraia_signup` BYPASSRLS
/// role. See module docs for the boundary contract.
///
/// **Forbidden:** `impl From<PgPool> for SignupPool`, `pub fn new(pool: PgPool)`,
/// `#[derive(Clone)]`, or any other path that produces a `SignupPool` without
/// the `current_user` validation.
///
/// `Clone` is intentionally NOT derived — the denial is enforced by a
/// compile-time `static_assertions::assert_not_impl_all!` test in this
/// module.
pub struct SignupPool {
    /// Wrapped private `PgPool`. Read-only access via `pool()` (`pub(crate)`)
    /// to limit the boundary surface to other modules of `garraia-auth` only.
    inner: PgPool,
}

impl SignupPool {
    /// Return a reference to the inner pool. **Crate-private** so external
    /// callers cannot bypass the boundary contract by extracting the pool
    /// and reusing it for non-auth queries.
    pub(crate) fn pool(&self) -> &PgPool {
        &self.inner
    }

    /// Connect to the dedicated signup database using the role validation
    /// guard. Returns [`AuthError::WrongRole`] if the connection comes back
    /// as anything other than `garraia_signup`.
    ///
    /// Tracing instrumentation uses `skip(config)` so the `database_url`
    /// (containing credentials) never lands in any span.
    #[instrument(skip(config), fields(max_connections = config.max_connections))]
    pub async fn from_dedicated_config(config: &SignupConfig) -> crate::Result<Self> {
        config
            .validate()
            .map_err(|e| AuthError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(AuthError::Storage)?;

        // Runtime role guard: `current_user` must be `garraia_signup`.
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .map_err(AuthError::Storage)?;

        if actual != "garraia_signup" {
            // Drop the pool explicitly so the misconfigured pool cannot
            // be re-used.
            pool.close().await;
            return Err(AuthError::WrongRole(actual));
        }

        Ok(Self { inner: pool })
    }
}

impl std::fmt::Debug for SignupPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the inner pool — it carries the connection string.
        f.debug_struct("SignupPool")
            .field("inner", &"<PgPool[garraia_signup]>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time denial of `Clone` on `SignupPool`.
    static_assertions::assert_not_impl_all!(SignupPool: Clone);

    #[test]
    fn debug_does_not_leak_database_url() {
        let cfg = SignupConfig {
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
        let mut cfg = SignupConfig {
            database_url: "postgres://garraia_signup:pw@h:5432/garraia".into(),
            max_connections: 5,
        };
        assert!(cfg.validate().is_ok());

        cfg.database_url = "postgresql://garraia_signup:pw@h:5432/garraia".into();
        assert!(cfg.validate().is_ok());

        cfg.database_url = "mysql://x".into();
        assert!(cfg.validate().is_err());

        cfg.database_url = "http://example.com".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_max_connections_bounds() {
        let too_big = SignupConfig {
            database_url: "postgres://garraia_signup:pw@h:5432/db".into(),
            max_connections: 21,
        };
        assert!(too_big.validate().is_err());

        let zero = SignupConfig {
            database_url: "postgres://garraia_signup:pw@h:5432/db".into(),
            max_connections: 0,
        };
        assert!(zero.validate().is_err());

        let ok = SignupConfig {
            database_url: "postgres://garraia_signup:pw@h:5432/db".into(),
            max_connections: 20,
        };
        assert!(ok.validate().is_ok());
    }
}
