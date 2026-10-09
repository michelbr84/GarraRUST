//! `PurgePool` — the dedicated Postgres pool for the account-purge worker.
//!
//! ## Boundary contract
//!
//! `PurgePool` wraps a [`PgPool`] connected as the `garraia_purge` Postgres
//! role. The inner pool is **private**. The only constructor is
//! [`PurgePool::from_dedicated_config`], which:
//!
//! 1. Validates the [`PurgeConfig`] (URL scheme, pool size).
//! 2. Connects.
//! 3. Issues `SELECT current_user::text` and refuses if the answer is
//!    anything other than `garraia_purge`.
//!
//! There is **no** `From<PgPool> for PurgePool` and there is **no**
//! `pub fn new(pool: PgPool) -> Self`. Any value of type `PurgePool` was
//! either built through the validating constructor or does not exist —
//! the same compile-time guarantee `LoginPool` gives for the login role
//! (ADR 0005 §"Anti-patterns" #4, applied to a second dedicated role).
//!
//! ## Why this pool exists
//!
//! The four `SECURITY DEFINER` functions of migration 034
//! (`claim_account_purge`, `purge_account_data`, `finish_account_purge`,
//! `fail_account_purge`) can destroy an entire tenant: whoever holds
//! `EXECUTE` chooses WHICH user to purge. Granting that to `garraia_app`
//! would turn any SQL-injection or ordinary bug with app credentials into
//! cross-tenant destruction, so migration 034 revokes `PUBLIC` and grants
//! only to `garraia_purge` — a `NOLOGIN` role with **zero table
//! privileges**, whose only capability is `EXECUTE` on those four fixed
//! function bodies. The worker connects through this pool; request
//! handlers never can.
//!
//! ## Why `pool()` is public here (unlike `LoginPool::pool()`)
//!
//! `LoginPool::pool()` is `pub(crate)` because the BYPASSRLS login role
//! has broad table access and must stay confined to `garraia-auth`.
//! `garraia_purge` has the opposite shape: no `SELECT`/`INSERT`/`UPDATE`/
//! `DELETE` on anything, so the contained surface reachable through the
//! pool is exactly the four function bodies above. Exposing the pool to
//! the worker (`garraia-gateway::account_purge_worker`) adds no ambient
//! authority. Same reasoning as `AppPool::pool_for_handlers`.

use serde::Deserialize;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::error::AuthError;

/// Configuration for the dedicated purge pool. Loaded from a SEPARATE
/// config path (`GARRAIA_PURGE_DATABASE_URL`) than the app pool — the
/// role only exists so that purge capability is absent from app
/// credentials, and sharing a URL would collapse the separation.
///
/// `Debug` is **manually implemented** to redact `database_url`. Mirrors
/// [`crate::login_pool::LoginConfig`].
#[derive(Clone, Deserialize, Validate)]
pub struct PurgeConfig {
    /// Postgres URL. The connection role MUST be `garraia_purge`.
    /// [`PurgePool::from_dedicated_config`] validates this at construction
    /// time via `SELECT current_user`.
    #[validate(custom(function = "validate_postgres_url"))]
    pub database_url: String,

    /// Pool size. Default 5: the worker claims one batch per tick and
    /// processes it serially on a single pinned connection, so a small
    /// footprint is plenty.
    #[validate(range(min = 1, max = 50))]
    pub max_connections: u32,
}

impl Default for PurgeConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 5,
        }
    }
}

impl std::fmt::Debug for PurgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PurgeConfig")
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

/// `PurgePool` wraps a [`PgPool`] connected as the `garraia_purge` role.
/// See module docs for the boundary contract.
///
/// **Forbidden:** `impl From<PgPool> for PurgePool`, `pub fn new(pool: PgPool)`,
/// `#[derive(Clone)]`, or any other path that produces a `PurgePool` without
/// the `current_user` validation.
///
/// `Clone` is intentionally NOT derived — exposing `Clone` would let any
/// caller fan out the purge-capable pool without going through the
/// validating constructor. The denial is enforced by a compile-time
/// `static_assertions::assert_not_impl_all!` test in this module.
pub struct PurgePool {
    /// Wrapped private `PgPool`. Read-only access via `pool()` — public
    /// because the role's ambient authority is exactly the four purge
    /// functions of migration 034 (no table privileges at all); see
    /// module docs.
    inner: PgPool,
}

impl PurgePool {
    /// Return a reference to the inner pool. Sanctioned for the
    /// `garraia-gateway::account_purge_worker` only — the `garraia_purge`
    /// role holds no table privileges, so this accessor cannot leak data
    /// access; it only routes purge-function `EXECUTE`s and the worker's
    /// advisory-lock bookkeeping.
    pub fn pool(&self) -> &PgPool {
        &self.inner
    }

    /// Connect to the dedicated purge database using the role validation
    /// guard. Returns [`AuthError::WrongRole`] if the connection comes back
    /// as anything other than `garraia_purge`.
    ///
    /// Tracing instrumentation uses `skip(config)` so the `database_url`
    /// (containing credentials) never lands in any span.
    #[instrument(skip(config), fields(max_connections = config.max_connections))]
    pub async fn from_dedicated_config(config: &PurgeConfig) -> crate::Result<Self> {
        config
            .validate()
            .map_err(|e| AuthError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(AuthError::Storage)?;

        // Runtime role guard. We query `current_user` immediately after
        // connecting and refuse if it isn't `garraia_purge`. Any other
        // role (postgres, garraia_app, etc.) is a misconfiguration and
        // must fail loudly: the whole point of this pool is that app
        // credentials cannot reach the purge functions.
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .map_err(AuthError::Storage)?;

        if actual != "garraia_purge" {
            // Drop the pool explicitly so the open connections are
            // returned to Postgres immediately and the misconfigured
            // pool cannot be re-used by any other code path.
            pool.close().await;
            return Err(AuthError::WrongRole(actual));
        }

        Ok(Self { inner: pool })
    }
}

impl std::fmt::Debug for PurgePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the inner pool — it carries the connection string.
        f.debug_struct("PurgePool")
            .field("inner", &"<PgPool[garraia_purge]>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time denial of `Clone` on `PurgePool`. Adding `#[derive(Clone)]`
    // or a manual `impl Clone for PurgePool` in the future will fail this
    // assertion at build time.
    static_assertions::assert_not_impl_all!(PurgePool: Clone);

    #[test]
    fn debug_does_not_leak_database_url() {
        let cfg = PurgeConfig {
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
        let mut cfg = PurgeConfig {
            database_url: "postgres://garraia_purge:pw@h:5432/garraia".into(),
            max_connections: 5,
        };
        assert!(cfg.validate().is_ok());

        cfg.database_url = "postgresql://garraia_purge:pw@h:5432/garraia".into();
        assert!(cfg.validate().is_ok());

        cfg.database_url = "mysql://x".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_max_connections_bounds() {
        let too_big = PurgeConfig {
            database_url: "postgres://garraia_purge:pw@h:5432/db".into(),
            max_connections: 51,
        };
        assert!(too_big.validate().is_err());

        let zero = PurgeConfig {
            database_url: "postgres://garraia_purge:pw@h:5432/db".into(),
            max_connections: 0,
        };
        assert!(zero.validate().is_err());
    }
}
