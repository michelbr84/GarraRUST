//! Skeleton smoke test for `garraia-auth` (GAR-391a).
//!
//! Three test classes:
//! 1. `LoginPool` constructor rejects pools not connected as `garraia_login`.
//! 2. `LoginPool` constructor accepts a properly-promoted `garraia_login`.
//! 3. `InternalProvider` stub methods all return `AuthError::NotImplemented`.
//!
//! Each test spins its own `pgvector/pgvector:pg16` testcontainer because
//! `garraia-auth` is intentionally testable in isolation from
//! `garraia-workspace`. Shared fixture optimization lives in 391c.

use garraia_auth::{
    AuthError, Credential, IdentityProvider, InternalProvider, LoginConfig, LoginPool,
};
use garraia_workspace::{Workspace, WorkspaceConfig};
use secrecy::SecretString;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

/// Boot a pgvector/pg16 container, return the container handle and the
/// admin (postgres superuser) connection URL.
async fn start_pgvector_container() -> anyhow::Result<(ContainerAsync<PgImage>, String)> {
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    Ok((container, url))
}

/// Apply migrations 001..008 by going through the workspace migrator.
async fn apply_migrations(postgres_url: &str) -> anyhow::Result<()> {
    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.to_string(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;
    Ok(())
}

/// Promote `garraia_login` to LOGIN with a known password and return a
/// connection URL using those credentials.
async fn promote_login_role(postgres_url: &str) -> anyhow::Result<String> {
    let admin = sqlx::PgPool::connect(postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin)
        .await?;
    admin.close().await;
    Ok(postgres_url.replace("postgres:postgres@", "garraia_login:test-password@"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_pool_rejects_non_login_role() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;
    apply_migrations(&postgres_url).await?;

    // Try to construct a LoginPool with the SUPERUSER credentials.
    // Should fail with AuthError::WrongRole("postgres").
    let bad_config = LoginConfig {
        database_url: postgres_url.clone(),
        max_connections: 2,
    };
    let result = LoginPool::from_dedicated_config(&bad_config).await;
    match result {
        Err(AuthError::WrongRole(actual)) => {
            assert_eq!(actual, "postgres", "expected current_user = postgres");
        }
        other => panic!("expected WrongRole(postgres), got: {other:?}"),
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_pool_accepts_garraia_login_role() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;
    apply_migrations(&postgres_url).await?;
    let login_url = promote_login_role(&postgres_url).await?;

    let good_config = LoginConfig {
        database_url: login_url,
        max_connections: 2,
    };
    let pool = LoginPool::from_dedicated_config(&good_config).await?;
    // Debug must not leak the URL.
    let dbg = format!("{pool:?}");
    assert!(
        !dbg.contains("test-password"),
        "Debug leaked password: {dbg}"
    );
    assert!(dbg.contains("garraia_login"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn internal_provider_methods_return_not_implemented() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;
    apply_migrations(&postgres_url).await?;
    let login_url = promote_login_role(&postgres_url).await?;

    let pool = std::sync::Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 2,
        })
        .await?,
    );
    let provider = InternalProvider::new(pool);

    assert_eq!(provider.id(), "internal");

    let credential = Credential::Internal {
        email: "test@example.com".into(),
        password: SecretString::from("irrelevant".to_owned()),
    };

    // 391b: verify_credential against an empty user_identities table returns
    // `Ok(None)` (constant-time path) — NOT NotImplemented anymore.
    match provider.verify_credential(&credential).await {
        Ok(None) => {}
        other => panic!("expected Ok(None) for empty DB, got: {other:?}"),
    }

    // find_by_provider_sub against an empty table returns Ok(None).
    match provider.find_by_provider_sub("nobody@example.com").await {
        Ok(None) => {}
        other => panic!("expected Ok(None) for missing identity, got: {other:?}"),
    }

    Ok(())
}
