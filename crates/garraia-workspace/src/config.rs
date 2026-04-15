//! Configuration for the `garraia-workspace` crate.
//!
//! `WorkspaceConfig` carries the Postgres connection URL, pool sizing, and the
//! opt-in migration bootstrap flag. All values are validated via the `validator`
//! crate before being handed to [`crate::store::Workspace::connect`].

use std::fmt;

use serde::Deserialize;
use validator::Validate;

// Note: `crate::error::Result` is NOT imported at module scope because
// `validate_url` below returns `std::result::Result<(), validator::ValidationError>`
// which would collide with our single-generic alias. `from_env` uses the alias
// via a fully-qualified path.
use crate::error::WorkspaceError;

/// Configuration for connecting to the workspace Postgres instance.
///
/// The derived `Debug` is **manually implemented** to redact `database_url`.
/// This protects against callers writing `tracing::debug!("{:?}", config)`
/// and leaking the URL (which may contain credentials) outside of the
/// `Workspace::connect` span that explicitly `skip`s it.
#[derive(Clone, Deserialize, Validate)]
pub struct WorkspaceConfig {
    /// Full Postgres connection URL, e.g. `postgres://user:pass@host:5432/db`.
    /// NEVER logged — see `Workspace::connect` `#[instrument(skip(config))]`
    /// and the custom `Debug` impl below which replaces the URL with `[REDACTED]`.
    #[validate(custom(function = "validate_url"))]
    pub database_url: String,

    /// Pool size. Default 10; clamped to [1, 200].
    #[validate(range(min = 1, max = 200))]
    pub max_connections: u32,

    /// Whether to run `sqlx::migrate!` on connect. Default true.
    pub migrate_on_start: bool,
}

impl fmt::Debug for WorkspaceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkspaceConfig")
            .field("database_url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .field("migrate_on_start", &self.migrate_on_start)
            .finish()
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 10,
            migrate_on_start: true,
        }
    }
}

impl WorkspaceConfig {
    /// Build a config from environment variables:
    /// - `GARRAIA_WORKSPACE_DATABASE_URL` (required)
    /// - `GARRAIA_WORKSPACE_MAX_CONNECTIONS` (optional, u32)
    /// - `GARRAIA_WORKSPACE_MIGRATE_ON_START` (optional, bool)
    pub fn from_env() -> crate::error::Result<Self> {
        let database_url = std::env::var("GARRAIA_WORKSPACE_DATABASE_URL").map_err(|_| {
            WorkspaceError::Config("GARRAIA_WORKSPACE_DATABASE_URL is required".to_string())
        })?;

        let mut cfg = Self {
            database_url,
            ..Self::default()
        };

        if let Ok(v) = std::env::var("GARRAIA_WORKSPACE_MAX_CONNECTIONS") {
            cfg.max_connections = v.parse::<u32>().map_err(|e| {
                WorkspaceError::Config(format!("invalid GARRAIA_WORKSPACE_MAX_CONNECTIONS: {e}"))
            })?;
        }

        if let Ok(v) = std::env::var("GARRAIA_WORKSPACE_MIGRATE_ON_START") {
            cfg.migrate_on_start = parse_bool(&v);
        }

        cfg.validate()
            .map_err(|e| WorkspaceError::Config(e.to_string()))?;

        Ok(cfg)
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// Custom validator: the URL must use a Postgres scheme.
fn validate_url(url: &str) -> Result<(), validator::ValidationError> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_postgres_scheme"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_safe_values() {
        let d = WorkspaceConfig::default();
        assert_eq!(d.max_connections, 10);
        assert!(d.migrate_on_start);
        assert!(d.database_url.is_empty());
    }

    #[test]
    fn debug_redacts_database_url() {
        let cfg = WorkspaceConfig {
            database_url: "postgres://supersecret:password@db:5432/garraia".into(),
            max_connections: 7,
            migrate_on_start: false,
        };
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("supersecret"),
            "Debug must not leak credentials: {dbg}"
        );
        assert!(
            !dbg.contains("password"),
            "Debug must not leak credentials: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "Debug should show redaction marker"
        );
        assert!(dbg.contains("max_connections: 7"));
        assert!(dbg.contains("migrate_on_start: false"));
    }

    #[test]
    fn validates_postgres_scheme() {
        let mut cfg = WorkspaceConfig {
            database_url: "postgres://u:p@h:5432/db".into(),
            max_connections: 10,
            migrate_on_start: true,
        };
        assert!(cfg.validate().is_ok());

        cfg.database_url = "postgresql://u:p@h:5432/db".into();
        assert!(cfg.validate().is_ok());

        cfg.database_url = "mysql://u:p@h:3306/db".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_max_connections_upper_bound() {
        let cfg = WorkspaceConfig {
            database_url: "postgres://u:p@h:5432/db".into(),
            max_connections: 201,
            migrate_on_start: true,
        };
        assert!(cfg.validate().is_err());

        let cfg_zero = WorkspaceConfig {
            database_url: "postgres://u:p@h:5432/db".into(),
            max_connections: 0,
            migrate_on_start: true,
        };
        assert!(cfg_zero.validate().is_err());
    }

    // Single sequential test — avoids env-var races across parallel tests.
    // Same pattern as garraia-telemetry::config::tests::env_loading_and_validation.
    #[test]
    fn env_loading_and_validation() {
        unsafe {
            std::env::set_var(
                "GARRAIA_WORKSPACE_DATABASE_URL",
                "postgres://u:p@localhost:5432/garraia",
            );
            std::env::set_var("GARRAIA_WORKSPACE_MAX_CONNECTIONS", "25");
            std::env::set_var("GARRAIA_WORKSPACE_MIGRATE_ON_START", "false");
        }

        let cfg = WorkspaceConfig::from_env().expect("happy path");
        assert_eq!(cfg.database_url, "postgres://u:p@localhost:5432/garraia");
        assert_eq!(cfg.max_connections, 25);
        assert!(!cfg.migrate_on_start);

        // Bad scheme triggers validation failure.
        unsafe {
            std::env::set_var("GARRAIA_WORKSPACE_DATABASE_URL", "mysql://x");
        }
        assert!(WorkspaceConfig::from_env().is_err());

        // Bad max_connections parse.
        unsafe {
            std::env::set_var(
                "GARRAIA_WORKSPACE_DATABASE_URL",
                "postgres://u:p@localhost:5432/garraia",
            );
            std::env::set_var("GARRAIA_WORKSPACE_MAX_CONNECTIONS", "not-a-number");
        }
        assert!(WorkspaceConfig::from_env().is_err());

        // Cleanup.
        unsafe {
            std::env::remove_var("GARRAIA_WORKSPACE_DATABASE_URL");
            std::env::remove_var("GARRAIA_WORKSPACE_MAX_CONNECTIONS");
            std::env::remove_var("GARRAIA_WORKSPACE_MIGRATE_ON_START");
        }
    }
}
