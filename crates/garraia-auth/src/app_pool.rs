//! `AppPool` — the dedicated RLS-enforced Postgres pool for `/v1/*` handlers
//! that live outside of `garraia-auth`.
//!
//! ## Boundary contract
//!
//! `AppPool` wraps a [`PgPool`] connected as the `garraia_app` Postgres role.
//! That role is `NOLOGIN` in production migrations (see migration 007) and
//! must be promoted to LOGIN by an operator (dev/test) or accessed via
//! `SET ROLE` in production (plan 0017 decision, deferred). The inner pool
//! is **private**. The only constructor is
//! [`AppPool::from_dedicated_config`], which:
//!
//! 1. Validates the [`AppPoolConfig`] (URL scheme, pool size).
//! 2. Connects.
//! 3. Issues `SELECT current_user::text` and refuses if the answer is
//!    anything other than `garraia_app`.
//!
//! Symmetric to `LoginPool` and `SignupPool` (ADR 0005). The three pools
//! share the same `AuthError::{Config, WrongRole, Storage}` error surface.
//!
//! ## Access patterns
//!
//! - **Crate-internal callers** (`garraia-auth` modules): use
//!   [`AppPool::pool`] which is `pub(crate)`.
//! - **`rest_v1` handlers in `garraia-gateway`**: use
//!   [`AppPool::pool_for_handlers`] which is the *only* sanctioned way to
//!   obtain the raw `PgPool` from a different crate. Every call site must
//!   be preceded by `SET LOCAL app.current_user_id = $user_id` inside a
//!   transaction so RLS policies see the correct tenant context. See
//!   `docs/adr/0005-identity-provider.md` (amendment pending plan 0017).
//! - **Test-only raw access**: the `test-support` feature exposes a
//!   `raw()` escape hatch — invisible to production builds.
//!
//! There is **no** `From<PgPool> for AppPool` and there is **no**
//! `pub fn new(pool: PgPool) -> Self`. Adding either is a violation of
//! ADR 0005 §"Anti-patterns" #4.
//!
//! The combination of (a) private field, (b) single validating constructor,
//! and (c) the static `!Clone` assertion makes "accidentally fan out the
//! app pool without the role guard" a compile-time error.

use serde::Deserialize;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::error::AuthError;

/// Configuration for the dedicated `garraia_app` pool. Loaded from a
/// SEPARATE env var (`GARRAIA_APP_DATABASE_URL`) than the login/signup
/// pools — production deployments MUST keep these credentials in a
/// distinct vault entry.
///
/// Named `AppPoolConfig` (not `AppConfig`) to avoid collision with
/// `garraia_config::AppConfig` which is the full gateway config struct.
///
/// `Debug` is **manually implemented** to redact `database_url`. Mirrors
/// the `LoginConfig` / `SignupConfig` pattern.
#[derive(Clone, Deserialize, Validate)]
pub struct AppPoolConfig {
    /// Postgres URL. The connection role MUST be `garraia_app`.
    /// `AppPool::from_dedicated_config` validates this at construction
    /// time via `SELECT current_user`.
    #[validate(custom(function = "validate_postgres_url"))]
    pub database_url: String,

    /// Pool size. Default 10 — larger than login/signup because this
    /// pool carries the actual read/write traffic of `/v1/*` handlers.
    /// Still bounded to 50 to match the other pools.
    #[validate(range(min = 1, max = 50))]
    pub max_connections: u32,
}

impl Default for AppPoolConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 10,
        }
    }
}

impl std::fmt::Debug for AppPoolConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppPoolConfig")
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

/// `AppPool` wraps a [`PgPool`] connected as the `garraia_app` RLS-enforced
/// role. See module docs for the boundary contract.
///
/// **Forbidden:** `impl From<PgPool> for AppPool`, `pub fn new(pool: PgPool)`,
/// `#[derive(Clone)]`, or any other path that produces an `AppPool` without
/// the `current_user` validation. ADR 0005 §"Anti-patterns" #4.
///
/// `Clone` is intentionally NOT derived — even though the inner `PgPool`
/// is reference-counted and `Clone`, exposing a `Clone` impl on `AppPool`
/// would let any caller fan out the pool without going through the
/// validating constructor. The denial is enforced by a compile-time
/// `static_assertions::assert_not_impl_all!` test in this module.
pub struct AppPool {
    /// Wrapped private `PgPool`. Read-only access via `pool()`
    /// (`pub(crate)`) for in-crate modules, or `pool_for_handlers()`
    /// (`pub`) for `rest_v1` handlers in `garraia-gateway`.
    inner: PgPool,
}

impl AppPool {
    /// Return a reference to the inner pool. **Crate-private** so
    /// `garraia-auth` internal modules (future `exec_with_tenant`
    /// helper, etc.) can operate against the typed newtype without
    /// re-implementing the guard.
    ///
    /// Currently has no in-crate caller — `#[allow(dead_code)]` is
    /// intentional and placeholder for plan 0017 which will add an
    /// `exec_with_tenant` helper that closes over this method.
    #[allow(dead_code)]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.inner
    }

    /// **Sanctioned cross-crate accessor** for `rest_v1` handlers in
    /// `garraia-gateway`.
    ///
    /// This is the *only* `pub fn` that hands out the raw `PgPool`. It
    /// exists because the REST handlers live in a separate crate and
    /// cannot use the `pub(crate)` accessor. Every call site MUST:
    ///
    /// 1. Open a `sqlx::Transaction` on the returned pool.
    /// 2. Issue `SET LOCAL app.current_user_id = $1` binding a
    ///    trusted `Uuid` (the authenticated `Principal::user_id`).
    ///    **Never** bind a user-controlled string — SET LOCAL does not
    ///    accept placeholders, so the Uuid is interpolated via
    ///    `{uuid}` which is safe by construction (Uuids have a fixed
    ///    syntax and no quote characters).
    /// 3. Run the scoped query/queries inside the same transaction.
    /// 4. Commit or rollback — `SET LOCAL` is cleared at transaction
    ///    end.
    ///
    /// Violating this protocol silently bypasses RLS. A code review
    /// gate (plan 0016 security-auditor dispatch) verifies every call
    /// site follows the protocol. A future plan may replace this with
    /// a closure-based `exec_with_tenant` API to enforce the protocol
    /// at the type level.
    ///
    /// Audit: `rg 'pool_for_handlers' crates/` must return only call
    /// sites inside `crates/garraia-gateway/src/rest_v1/`.
    pub fn pool_for_handlers(&self) -> &PgPool {
        &self.inner
    }

    /// **Test-only escape hatch** mirroring [`crate::login_pool::LoginPool::raw`]
    /// for the RLS matrix suite (GAR-392, plan 0013) and future gateway
    /// integration tests (plan 0016 M2).
    ///
    /// Gated behind `#[cfg(any(test, feature = "test-support"))]` so it
    /// is invisible to any production build.
    #[cfg(any(test, feature = "test-support"))]
    pub fn raw(&self) -> &PgPool {
        &self.inner
    }
}

impl AppPool {
    /// Connect to the dedicated `garraia_app` database using the role
    /// validation guard. Returns [`AuthError::WrongRole`] if the
    /// connection comes back as anything other than `garraia_app`.
    ///
    /// Tracing instrumentation uses `skip(config)` so the
    /// `database_url` (containing credentials) never lands in any span.
    #[instrument(skip(config), fields(max_connections = config.max_connections))]
    pub async fn from_dedicated_config(config: &AppPoolConfig) -> crate::Result<Self> {
        config
            .validate()
            .map_err(|e| AuthError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(AuthError::Storage)?;

        // Runtime role guard. We query `current_user` immediately after
        // connecting and refuse if it isn't `garraia_app`. Any other
        // role (postgres, garraia_login, garraia_signup, etc.) is a
        // misconfiguration and must fail loudly.
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .map_err(AuthError::Storage)?;

        if actual != "garraia_app" {
            // Drop the pool explicitly so open connections return to
            // Postgres immediately and the misconfigured pool cannot
            // be re-used by any other code path.
            pool.close().await;
            return Err(AuthError::WrongRole(actual));
        }

        Ok(Self { inner: pool })
    }
}

impl std::fmt::Debug for AppPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the inner pool — it carries the connection string.
        f.debug_struct("AppPool")
            .field("inner", &"<PgPool[garraia_app]>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time denial of `Clone` on `AppPool`. Adding `#[derive(Clone)]`
    // or a manual `impl Clone for AppPool` in the future will fail this
    // assertion at build time. Mirrors LoginPool security review H-1.
    static_assertions::assert_not_impl_all!(AppPool: Clone);

    #[test]
    fn debug_does_not_leak_database_url() {
        let cfg = AppPoolConfig {
            database_url: "postgres://garraia_app:topsecret@db:5432/garraia".into(),
            max_connections: 10,
        };
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("topsecret"), "Debug must not leak: {dbg}");
        assert!(!dbg.contains("garraia_app:topsecret"), "Debug must not leak creds: {dbg}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(dbg.contains("max_connections: 10"));
    }

    #[test]
    fn validates_postgres_scheme() {
        let mut cfg = AppPoolConfig {
            database_url: "postgres://garraia_app:pw@h:5432/garraia".into(),
            max_connections: 10,
        };
        assert!(cfg.validate().is_ok());

        cfg.database_url = "postgresql://garraia_app:pw@h:5432/garraia".into();
        assert!(cfg.validate().is_ok());

        cfg.database_url = "mysql://x".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_max_connections_bounds() {
        let too_big = AppPoolConfig {
            database_url: "postgres://garraia_app:pw@h:5432/db".into(),
            max_connections: 51,
        };
        assert!(too_big.validate().is_err());

        let zero = AppPoolConfig {
            database_url: "postgres://garraia_app:pw@h:5432/db".into(),
            max_connections: 0,
        };
        assert!(zero.validate().is_err());
    }

    #[test]
    fn invalid_url_returns_config_error_from_constructor() {
        // We can't spin up Postgres in a unit test, but we CAN verify
        // the validator gate fires before any network activity.
        let cfg = AppPoolConfig {
            database_url: "not-a-postgres-url".into(),
            max_connections: 10,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(AppPool::from_dedicated_config(&cfg));
        assert!(matches!(err, Err(AuthError::Config(_))));
    }

    #[test]
    fn default_config_is_invalid() {
        // Default database_url is empty → validator rejects.
        let cfg = AppPoolConfig::default();
        assert!(cfg.validate().is_err());
    }
}
