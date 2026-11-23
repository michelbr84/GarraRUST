//! Kills two `purge_pool.rs` mutants found by the weekly cargo-mutants pilot
//! (main run 45206007442, shards 0/3 and 2/3, 2026-11-23):
//!
//! * `purge_pool.rs:155` — role guard `!=` to `==` in
//!   `PurgePool::from_dedicated_config`: mutating the comparison would accept
//!   any wrong role and reject `garraia_purge`, collapsing the boundary that
//!   keeps the four `SECURITY DEFINER` purge functions unreachable with
//!   non-purge credentials.
//! * `purge_pool.rs:170` — `Debug for PurgePool` returning
//!   `Ok(Default::default())` (empty output) instead of the
//!   `"<PgPool[garraia_purge]>"` redaction marker.
//!
//! Both siblings (`AppPool`, `SignupPool`) already have equivalents in
//! `app_pool_role_guard.rs` and `debug_redaction_pools.rs`; `PurgePool` was
//! the one left uncovered. Each test is self-contained: it boots a fresh
//! pgvector/pg16 container, applies all workspace migrations (migration 034
//! defines the NOLOGIN `garraia_purge` role), then exercises the public
//! constructor. Pattern follows `app_pool_role_guard.rs`.

use garraia_auth::{AuthError, PurgeConfig, PurgePool};
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

/// Kills the role-guard mutant: pointing the constructor at a role that is
/// *not* `garraia_purge` must fail loudly with `AuthError::WrongRole`, never
/// build a pool.
#[tokio::test]
async fn purge_pool_rejects_non_purge_role() -> anyhow::Result<()> {
    let (_container, admin_url) = boot_pg().await?;

    // Promote `garraia_login` to LOGIN with a deterministic test password.
    // We deliberately do NOT promote `garraia_purge` — the guard must fire
    // against a real, connectable, wrong role.
    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'login-pw'")
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    let wrong_role_url = admin_url.replace("postgres:postgres@", "garraia_login:login-pw@");
    let cfg = PurgeConfig {
        database_url: wrong_role_url,
        max_connections: 4,
    };

    let result = PurgePool::from_dedicated_config(&cfg).await;
    match result {
        Err(AuthError::WrongRole(role)) => {
            assert_eq!(
                role, "garraia_login",
                "WrongRole error must carry the actual observed role"
            );
        }
        Err(other) => {
            panic!("expected Err(AuthError::WrongRole(\"garraia_login\")), got Err({other:?})")
        }
        Ok(_) => panic!(
            "expected Err(AuthError::WrongRole(\"garraia_login\")), got Ok(_) — \
             the role guard in PurgePool::from_dedicated_config did not fire"
        ),
    }
    Ok(())
}

/// Kills the `Debug for PurgePool` -> `Ok(Default::default())` mutant and the
/// redaction-string mutants: the formatted output must contain the redaction
/// marker and leak no credential fragment.
#[tokio::test]
async fn purge_pool_debug_does_not_expose_credentials() -> anyhow::Result<()> {
    let (_container, admin_url) = boot_pg().await?;

    // Promote garraia_purge to LOGIN so we can connect as it and build a
    // real pool (the only constructor validates `current_user`).
    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query("ALTER ROLE garraia_purge WITH LOGIN PASSWORD 'purge-test-pw'")
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    // Build the URL for the garraia_purge role using the same host/port.
    let purge_url = admin_url.replace("postgres:postgres@", "garraia_purge:purge-test-pw@");
    let cfg = PurgeConfig {
        database_url: purge_url,
        max_connections: 2,
    };

    let pool = PurgePool::from_dedicated_config(&cfg).await?;
    let dbg = format!("{pool:?}");

    // Must contain the redaction marker (kills the empty-output mutant).
    assert!(
        dbg.contains("<PgPool[garraia_purge]>"),
        "PurgePool Debug must contain redaction marker, got: {dbg}"
    );
    // Must NOT contain any fragment from the connection string.
    assert!(
        !dbg.contains("purge-test-pw"),
        "PurgePool Debug must not leak password, got: {dbg}"
    );
    assert!(
        !dbg.contains("garraia_purge:purge"),
        "PurgePool Debug must not leak URL credentials, got: {dbg}"
    );

    Ok(())
}
