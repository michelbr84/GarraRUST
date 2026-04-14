//! Integration test harness for `garraia-gateway` (plan 0016 M2-T3).
//!
//! Process-wide shared testcontainer (pgvector/pg16) + migrations
//! 001..010 + three typed pools + `JwtIssuer::new_for_test` + a
//! prebuilt `axum::Router` via `GatewayServer::build_router_for_test`.
//! All of that behind a `OnceCell<Arc<Harness>>` so the container
//! boots exactly once per `cargo test` invocation.
//!
//! ## Scope (M2 reduced slice)
//!
//! This module exposes ONLY the boot + router. No fixtures, no seed
//! helpers, no authed-path smoke tests. Those land in plan 0016 M3.
//!
//! ## Side-effect isolation
//!
//! Before constructing `AppState`, the harness sets
//! `GARRAIA_CONFIG_DIR` to a process-lifetime tempdir. This diverts
//! `McpPersistenceService::with_default_path()` (which the normal
//! gateway bootstrap uses) away from the developer's real
//! `%APPDATA%\garraia\` directory — a risk flagged by the
//! team-coordinator pre-gate for plan 0016 M2.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::Router;
use garraia_auth::{AppPool, AppPoolConfig, JwtIssuer, LoginConfig, LoginPool, SignupConfig, SignupPool};
use garraia_config::AppConfig;
use garraia_gateway::server::build_router_for_test;
use garraia_workspace::{Workspace, WorkspaceConfig};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres as PgImage;
use tokio::sync::OnceCell;

static SHARED: OnceCell<Arc<Harness>> = OnceCell::const_new();

/// Process-wide shared test harness.
///
/// Obtain via `Harness::get().await`. The container boots exactly
/// once per test process — subsequent calls return the same `Arc`.
pub struct Harness {
    /// Container handle — kept alive for the process lifetime of
    /// the test binary by the `Arc<Harness>` in `SHARED`.
    _container: ContainerAsync<PgImage>,

    /// Temp directory used as `GARRAIA_CONFIG_DIR`. Held so it
    /// survives for the whole test process and is cleaned up when
    /// `Harness` is dropped.
    _config_tempdir: tempfile::TempDir,

    /// Superuser URL (`postgres:postgres@...`). Only used by tests
    /// that legitimately need RLS-bypass (fixture setup in M3).
    pub admin_url: String,

    /// Typed `garraia_app` RLS-enforced pool.
    pub app_pool: Arc<AppPool>,

    /// Typed `garraia_login` BYPASSRLS pool.
    pub login_pool: Arc<LoginPool>,

    /// Typed `garraia_signup` BYPASSRLS pool.
    pub signup_pool: Arc<SignupPool>,

    /// Deterministic `JwtIssuer` shared by the router + any test
    /// that needs to mint a bearer token via `issue_access_for_test`.
    pub jwt: Arc<JwtIssuer>,

    /// Pre-built Axum router. Ready for `tower::ServiceExt::oneshot`
    /// — no socket binding.
    pub router: Router,
}

impl Harness {
    /// Idempotent process-wide accessor. First call boots the
    /// container, runs migrations, promotes roles, builds the pools
    /// and the router. Subsequent calls return the cached `Arc`.
    pub async fn get() -> Arc<Self> {
        SHARED
            .get_or_init(|| async {
                Arc::new(
                    Self::boot()
                        .await
                        .expect("gateway integration harness boot"),
                )
            })
            .await
            .clone()
    }

    async fn boot() -> anyhow::Result<Self> {
        // 1. Divert GARRAIA_CONFIG_DIR to a tempdir BEFORE anything
        //    in the gateway touches `default_config_dir()`. This
        //    prevents `McpPersistenceService::with_default_path()`
        //    from writing to the developer's real
        //    `%APPDATA%\garraia\` directory during tests.
        //
        //    Safety: `std::env::set_var` is `unsafe` on Edition 2024.
        //    We run it before spawning any task that reads these
        //    env vars, so no other thread races us.
        let config_tempdir = tempfile::tempdir()?;
        unsafe {
            std::env::set_var("GARRAIA_CONFIG_DIR", config_tempdir.path());
        }

        // 2. Boot pgvector/pg16. Cold first run can take several
        //    minutes on first image pull — that is expected.
        let container = PgImage::default()
            .with_name("pgvector/pgvector")
            .with_tag("pg16")
            .start()
            .await?;
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        // 3. Apply workspace migrations 001..010 via `Workspace`.
        Workspace::connect(WorkspaceConfig {
            database_url: admin_url.clone(),
            max_connections: 5,
            migrate_on_start: true,
        })
        .await?;

        // 4. Promote the three NOLOGIN roles to LOGIN with
        //    deterministic passwords. Test-only: container is
        //    ephemeral and the URLs never leave the process.
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

        // 5. Build the three typed pools via their production
        //    constructors — validates the `SELECT current_user`
        //    guards identically to how production builds them.
        let app_url = admin_url.replace("postgres:postgres@", "garraia_app:app-pw@");
        let app_pool = Arc::new(
            AppPool::from_dedicated_config(&AppPoolConfig {
                database_url: app_url,
                max_connections: 4,
            })
            .await?,
        );

        let login_url = admin_url.replace("postgres:postgres@", "garraia_login:login-pw@");
        let login_pool = Arc::new(
            LoginPool::from_dedicated_config(&LoginConfig {
                database_url: login_url,
                max_connections: 4,
            })
            .await?,
        );

        let signup_url = admin_url.replace("postgres:postgres@", "garraia_signup:signup-pw@");
        let signup_pool = Arc::new(
            SignupPool::from_dedicated_config(&SignupConfig {
                database_url: signup_url,
                max_connections: 4,
            })
            .await?,
        );

        // 6. Build a deterministic JWT issuer via the test helper
        //    (plan 0016 M2-T1). The 32-byte minimum is handled by
        //    `new_for_test` itself.
        let jwt = Arc::new(JwtIssuer::new_for_test("harness-jwt-secret"));

        // 7. Build the Router via the gateway test helper. The
        //    test-helpers feature gate prevents this from being
        //    reachable in any non-test build.
        let config = minimal_test_config();
        let router = build_router_for_test(
            config,
            login_pool.clone(),
            signup_pool.clone(),
            jwt.clone(),
            Some(app_pool.clone()),
        )
        .await;

        Ok(Self {
            _container: container,
            _config_tempdir: config_tempdir,
            admin_url,
            app_pool,
            login_pool,
            signup_pool,
            jwt,
            router,
        })
    }
}

/// Minimal `AppConfig` that satisfies the router builder without
/// touching any external resource. Relies on `AppConfig::default`
/// producing non-zero rate limit values (verified by team-coordinator
/// pre-gate: `default_rate_per_second = 1`, `default_rate_burst_size = 60`).
fn minimal_test_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.memory.enabled = false;
    cfg
}

/// Helper to grab the config tempdir path for diagnostic assertions.
#[allow(dead_code)]
pub fn tempdir_for_debug(h: &Harness) -> PathBuf {
    h._config_tempdir.path().to_path_buf()
}

/// Build a `GET` request suitable for `tower::ServiceExt::oneshot`
/// against the harness router.
///
/// The production router composition includes a `tower_governor`
/// rate-limit layer whose default `PeerIpKeyExtractor` reads peer
/// IP from the request's `ConnectInfo<SocketAddr>` extension. That
/// extension is normally populated by Axum's TCP listener and is
/// ABSENT on requests constructed via `Request::builder()` — causing
/// the governor to bail out with `"Unable To Extract Key!"` and the
/// handler chain to answer 500.
///
/// This helper injects a fixed `127.0.0.1:1` ConnectInfo so the
/// governor's key extractor succeeds and the real handler runs.
/// Any additional headers (e.g. `Authorization: Bearer ...`) can
/// be added via `req.headers_mut()` after calling this.
pub fn harness_get(path: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .expect("request builder should succeed");
    req.extensions_mut().insert(ConnectInfo::<SocketAddr>(
        "127.0.0.1:1".parse().expect("valid fixed test peer"),
    ));
    req
}
