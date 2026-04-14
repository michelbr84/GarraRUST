//! Shared testcontainer + pools for the GAR-392 RLS matrix.
//!
//! The harness is process-wide: one Postgres container, one migration pass
//! (001..010), and three typed pools, all behind a `OnceCell<Arc<Harness>>`.
//! Isolation between cases comes from fresh tenants per case (see
//! `common::tenants::Tenant`), never from truncate/rollback.
//!
//! Plan 0013 path C — Task 3. No axum, no HTTP fixture (those were cut by
//! the path-C amendment after the Open Question #3 empirical verification
//! showed that 15 of 18 REST endpoints do not exist in garraia-gateway).

use std::sync::Arc;

use garraia_auth::{LoginConfig, LoginPool, SignupConfig, SignupPool};
use garraia_workspace::{Workspace, WorkspaceConfig};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres as PgImage;
use tokio::sync::OnceCell;

/// Process-wide shared harness. `OnceCell` guarantees the container boots
/// exactly once per `cargo test` invocation, even under the parallel test
/// runner, because the first call to `Harness::get()` serializes the init.
static SHARED: OnceCell<Arc<Harness>> = OnceCell::const_new();

pub struct Harness {
    /// Container handle — kept alive by the `Arc<Harness>` so it is dropped
    /// only when the test process exits.
    _container: ContainerAsync<PgImage>,

    /// Superuser URL. Used for fixture setup that legitimately needs to
    /// bypass RLS/GRANTs: creating groups, seeding `group_members`, and
    /// inserting initial recursos no group owner (Task 4, Task 8).
    pub admin_url: String,

    /// Pool connected as `garraia_app` (the RLS-enforced application role).
    /// Exercised directly by the RLS matrix.
    pub app_pool: PgPool,

    /// Typed newtype over a pool connected as `garraia_login`.
    /// Accessed via `login_pool.raw()` (feature-gated) in the RLS matrix.
    pub login_pool: LoginPool,

    /// Typed newtype over a pool connected as `garraia_signup`.
    /// Accessed via `signup_pool.raw()` (feature-gated) in the RLS matrix.
    pub signup_pool: SignupPool,
}

impl Harness {
    /// Idempotent process-wide accessor.
    ///
    /// First call: boots the container, runs migrations 001..010 via
    /// `Workspace::connect`, promotes the three NOLOGIN roles to LOGIN with
    /// deterministic passwords, and constructs the three typed pools.
    ///
    /// Subsequent calls: return the same `Arc<Harness>`.
    pub async fn get() -> Arc<Self> {
        SHARED
            .get_or_init(|| async {
                Arc::new(
                    Self::boot()
                        .await
                        .expect("harness boot"),
                )
            })
            .await
            .clone()
    }

    async fn boot() -> anyhow::Result<Self> {
        // 1. Boot pgvector/pg16 (cold ~60s on first run; warm ~3-5s).
        let container = PgImage::default()
            .with_name("pgvector/pgvector")
            .with_tag("pg16")
            .start()
            .await?;
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        // 2. Apply workspace migrations 001..010. `Workspace::connect` uses
        //    its own pool internally (dropped after this block) — the
        //    harness does not keep a Workspace handle around.
        Workspace::connect(WorkspaceConfig {
            database_url: admin_url.clone(),
            max_connections: 5,
            migrate_on_start: true,
        })
        .await?;

        // 3. Promote the three NOLOGIN roles to LOGIN with deterministic
        //    passwords. This is test-only: the container is ephemeral and
        //    the URLs never leave the process.
        let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
        sqlx::query("ALTER ROLE garraia_app    WITH LOGIN PASSWORD 'app-pw'")
            .execute(&admin_pool)
            .await?;
        sqlx::query("ALTER ROLE garraia_login  WITH LOGIN PASSWORD 'login-pw'")
            .execute(&admin_pool)
            .await?;
        sqlx::query("ALTER ROLE garraia_signup WITH LOGIN PASSWORD 'signup-pw'")
            .execute(&admin_pool)
            .await?;
        admin_pool.close().await;

        // 4. Build the three typed handles.
        let app_url = admin_url.replace("postgres:postgres@", "garraia_app:app-pw@");
        let app_pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(&app_url)
            .await?;

        let login_url = admin_url.replace("postgres:postgres@", "garraia_login:login-pw@");
        let login_pool = LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 4,
        })
        .await?;

        let signup_url = admin_url.replace("postgres:postgres@", "garraia_signup:signup-pw@");
        let signup_pool = SignupPool::from_dedicated_config(&SignupConfig {
            database_url: signup_url,
            max_connections: 4,
        })
        .await?;

        Ok(Self {
            _container: container,
            admin_url,
            app_pool,
            login_pool,
            signup_pool,
        })
    }
}
