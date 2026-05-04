//! GAR-505 #6 — kills `app_pool.rs:203` `!=` → `==` mutant in
//! `AppPool::from_dedicated_config`.
//!
//! The role guard rejects any role that is not `garraia_app`. We exercise
//! the rejection path by promoting `garraia_login` to LOGIN and pointing
//! the AppPool constructor at it. The constructor must return
//! `AuthError::WrongRole("garraia_login")`. Mutating `!=` → `==` would
//! flip the guard (accept `garraia_login`, reject `garraia_app`) and this
//! test would fail.
//!
//! Pattern follows `verify_internal.rs` — self-contained container boot,
//! no `mod common` dependency (the shared harness is gated behind the
//! `test-support` feature and would over-couple this single test).

use garraia_auth::{AppPool, AppPoolConfig, AuthError};
use garraia_workspace::{Workspace, WorkspaceConfig};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

#[tokio::test]
async fn from_dedicated_config_rejects_non_app_role() -> anyhow::Result<()> {
    // 1. Boot pgvector/pg16. The container handle is kept alive in scope
    //    for the duration of the test; dropping it stops the container.
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // 2. Apply migrations 001..010 — defines `garraia_app`, `garraia_login`,
    //    and `garraia_signup` as NOLOGIN roles.
    Workspace::connect(WorkspaceConfig {
        database_url: admin_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // 3. Promote `garraia_login` to LOGIN with a deterministic test password
    //    so we can build a connect URL for it. We do NOT promote
    //    `garraia_app` — the AppPool guard would accept that, defeating the
    //    test.
    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'login-pw'")
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    // 4. Point AppPool::from_dedicated_config at garraia_login. The guard
    //    at app_pool.rs:203 must fire and return WrongRole("garraia_login").
    let wrong_role_url = admin_url.replace("postgres:postgres@", "garraia_login:login-pw@");
    let cfg = AppPoolConfig {
        database_url: wrong_role_url,
        max_connections: 4,
    };

    let result = AppPool::from_dedicated_config(&cfg).await;
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
             the role guard at app_pool.rs:203 did not fire"
        ),
    }
    Ok(())
}
