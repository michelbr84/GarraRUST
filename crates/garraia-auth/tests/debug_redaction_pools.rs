//! GAR-483 — kills two Debug-redaction mutants:
//!
//! * `signup_pool.rs:~153` — `Debug for SignupPool` returning `Ok(Default::default())`
//!   (empty output) instead of the expected `"<PgPool[garraia_signup]>"` marker.
//! * `app_pool.rs:~218`   — `Debug for AppPool`   returning `Ok(Default::default())`
//!   (empty output) instead of the expected `"<PgPool[garraia_app]>"` marker.
//!
//! Each test is self-contained: it boots a fresh pgvector/pg16 container, applies
//! all workspace migrations (which define the NOLOGIN roles), promotes the target
//! role to LOGIN, constructs the typed pool through its real constructor, and
//! asserts the `Debug` representation contains the redaction marker and leaks no
//! credential fragment.
//!
//! Pattern follows `app_pool_role_guard.rs` — no `mod common` / `test-support`
//! feature dependency needed because we use the public constructors.

use garraia_auth::{AppPool, AppPoolConfig, SignupConfig, SignupPool};
use garraia_workspace::{Workspace, WorkspaceConfig};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

/// Boots a pgvector/pg16 container, applies migrations, and returns the
/// admin connection URL.
async fn boot_pg() -> anyhow::Result<(testcontainers::ContainerAsync<PgImage>, String)> {
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    Workspace::connect(WorkspaceConfig {
        database_url: admin_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    Ok((container, admin_url))
}

#[tokio::test]
async fn app_pool_debug_does_not_expose_credentials() -> anyhow::Result<()> {
    let (_container, admin_url) = boot_pg().await?;

    // Promote garraia_app to LOGIN so we can connect as it.
    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query("ALTER ROLE garraia_app WITH LOGIN PASSWORD 'app-test-pw'")
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    // Build the URL for the garraia_app role using the same host/port.
    let app_url = admin_url.replace("postgres:postgres@", "garraia_app:app-test-pw@");
    let cfg = AppPoolConfig {
        database_url: app_url,
        max_connections: 2,
    };

    let pool = AppPool::from_dedicated_config(&cfg).await?;
    let dbg = format!("{pool:?}");

    // Must contain the redaction marker.
    assert!(
        dbg.contains("<PgPool[garraia_app]>"),
        "AppPool Debug must contain redaction marker, got: {dbg}"
    );
    // Must NOT contain any fragment from the connection string.
    assert!(
        !dbg.contains("app-test-pw"),
        "AppPool Debug must not leak password, got: {dbg}"
    );
    assert!(
        !dbg.contains("garraia_app:"),
        "AppPool Debug must not leak URL credentials, got: {dbg}"
    );

    Ok(())
}

#[tokio::test]
async fn signup_pool_debug_does_not_expose_credentials() -> anyhow::Result<()> {
    let (_container, admin_url) = boot_pg().await?;

    // Promote garraia_signup to LOGIN so we can connect as it.
    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query("ALTER ROLE garraia_signup WITH LOGIN PASSWORD 'signup-test-pw'")
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    // Build the URL for the garraia_signup role.
    let signup_url = admin_url.replace("postgres:postgres@", "garraia_signup:signup-test-pw@");
    let cfg = SignupConfig {
        database_url: signup_url,
        max_connections: 2,
    };

    let pool = SignupPool::from_dedicated_config(&cfg).await?;
    let dbg = format!("{pool:?}");

    // Must contain the redaction marker.
    assert!(
        dbg.contains("<PgPool[garraia_signup]>"),
        "SignupPool Debug must contain redaction marker, got: {dbg}"
    );
    // Must NOT contain any fragment from the connection string.
    assert!(
        !dbg.contains("signup-test-pw"),
        "SignupPool Debug must not leak password, got: {dbg}"
    );
    assert!(
        !dbg.contains("garraia_signup:"),
        "SignupPool Debug must not leak URL credentials, got: {dbg}"
    );

    Ok(())
}
