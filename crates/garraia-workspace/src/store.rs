//! `Workspace` — the Postgres pool wrapper for the workspace crate.

use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::config::WorkspaceConfig;
use crate::error::{Result, WorkspaceError};

/// Embedded migration set. Paths are relative to the crate root (`Cargo.toml`).
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Handle to the workspace Postgres instance.
///
/// `Clone` is cheap — it clones only the `Arc` inside `PgPool`, not the
/// underlying connections. `Debug` prints only the pool's connection stats,
/// not the `database_url` (because `WorkspaceConfig` is no longer held after
/// `connect` returns).
#[derive(Debug, Clone)]
pub struct Workspace {
    pool: PgPool,
}

impl Workspace {
    /// Connect to Postgres, optionally running pending migrations.
    ///
    /// The `config` parameter is `skip`-ed from the tracing span so the
    /// `database_url` (which contains credentials) never reaches log output.
    #[instrument(
        skip(config),
        fields(
            max_connections = config.max_connections,
            migrate_on_start = config.migrate_on_start,
        )
    )]
    pub async fn connect(config: WorkspaceConfig) -> Result<Self> {
        // Validate here too: callers may construct WorkspaceConfig directly
        // (e.g., tests, downstream crates), bypassing from_env's validation.
        config
            .validate()
            .map_err(|e| WorkspaceError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(WorkspaceError::Connect)?;

        if config.migrate_on_start {
            MIGRATOR
                .run(&pool)
                .await
                .inspect_err(|e| tracing::error!(err = %e, "workspace migration failed"))
                .map_err(WorkspaceError::Migrate)?;
            tracing::info!("workspace migrations applied");
        }

        Ok(Self { pool })
    }

    /// Access the underlying pool. Downstream crates call this for queries.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
