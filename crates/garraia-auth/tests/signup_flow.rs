//! Integration tests for `signup_user` (GAR-391c Wave 1 — impl-auth-impl).
//!
//! Boots a single `pgvector/pg16` testcontainer per test, applies workspace
//! migrations 001..009, and then applies **migration 010 manually via raw
//! SQL** — because in this isolated worktree the migration file does not
//! yet exist (it is being created in parallel by the `impl-workspace-migration`
//! agent). The orchestrator will reconcile this at merge time: once
//! migration 010 is present in the workspace crate, these tests can drop
//! the manual apply step.
//!
//! Three tests:
//!
//! 1. `signup_happy_path` — fresh email lands a `users` row + a
//!    `user_identities` row with an Argon2id PHC hash.
//! 2. `signup_duplicate_email` — collision yields `AuthError::DuplicateEmail`
//!    and no new row.
//! 3. `signup_pool_rejects_non_signup_role` — building `SignupPool` against
//!    the superuser credentials surfaces `AuthError::WrongRole("postgres")`.

use std::sync::Arc;

use garraia_auth::{
    signup_user, AuthError, LoginConfig, LoginPool, SignupConfig, SignupPool,
};
use garraia_workspace::{Workspace, WorkspaceConfig};
use secrecy::SecretString;
use sqlx::Row;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

/// Migration 010 SQL, inlined here as a worktree-isolation workaround.
/// See plan 0012 §6. Once the `impl-workspace-migration` agent's migration
/// file lands in `crates/garraia-workspace/migrations/010_*.sql` and the
/// orchestrator merges it, this block can be deleted and the test will
/// pick up the role via the normal `Workspace::connect` migration path.
const MIGRATION_010_SQL: &str = r#"
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_signup') THEN
        CREATE ROLE garraia_signup NOLOGIN BYPASSRLS;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO garraia_signup;
GRANT SELECT, INSERT ON users TO garraia_signup;
GRANT SELECT, INSERT ON user_identities TO garraia_signup;
GRANT INSERT ON audit_events TO garraia_signup;
GRANT SELECT ON sessions TO garraia_login;
"#;

struct Fixture {
    _container: ContainerAsync<PgImage>,
    admin_pool: sqlx::PgPool,
    postgres_url: String,
}

async fn boot() -> anyhow::Result<Fixture> {
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let postgres_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // Apply workspace migrations 001..009.
    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    let admin_pool = sqlx::PgPool::connect(&postgres_url).await?;

    // Manually apply migration 010 (worktree-isolation workaround).
    sqlx::raw_sql(MIGRATION_010_SQL)
        .execute(&admin_pool)
        .await?;

    Ok(Fixture {
        _container: container,
        admin_pool,
        postgres_url,
    })
}

/// Promote `garraia_signup` to LOGIN with a known password and return the
/// signup-role connection URL.
async fn promote_signup_role(fx: &Fixture) -> anyhow::Result<String> {
    sqlx::query("ALTER ROLE garraia_signup WITH LOGIN PASSWORD 'signup-pw'")
        .execute(&fx.admin_pool)
        .await?;
    Ok(fx
        .postgres_url
        .replace("postgres:postgres@", "garraia_signup:signup-pw@"))
}

/// Promote `garraia_login` to LOGIN with a known password and return the
/// login-role connection URL.
async fn promote_login_role(fx: &Fixture) -> anyhow::Result<String> {
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'login-pw'")
        .execute(&fx.admin_pool)
        .await?;
    Ok(fx
        .postgres_url
        .replace("postgres:postgres@", "garraia_login:login-pw@"))
}

async fn build_login_pool(url: String) -> anyhow::Result<Arc<LoginPool>> {
    let pool = LoginPool::from_dedicated_config(&LoginConfig {
        database_url: url,
        max_connections: 2,
    })
    .await?;
    Ok(Arc::new(pool))
}

async fn build_signup_pool(url: String) -> anyhow::Result<Arc<SignupPool>> {
    let pool = SignupPool::from_dedicated_config(&SignupConfig {
        database_url: url,
        max_connections: 2,
    })
    .await?;
    Ok(Arc::new(pool))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signup_happy_path() -> anyhow::Result<()> {
    let fx = boot().await?;
    let signup_url = promote_signup_role(&fx).await?;
    let login_url = promote_login_role(&fx).await?;

    let signup_pool = build_signup_pool(signup_url).await?;
    let login_pool = build_login_pool(login_url).await?;

    let password = SecretString::from("test-password".to_owned());
    let user_id = signup_user(
        login_pool.as_ref(),
        signup_pool.as_ref(),
        "alice@example.com",
        &password,
        "Alice",
    )
    .await?;

    // users row exists.
    let user_row = sqlx::query(
        "SELECT id, email::text AS email, display_name, status \
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&fx.admin_pool)
    .await?;
    let got_id: Uuid = user_row.try_get("id")?;
    let got_email: String = user_row.try_get("email")?;
    let got_display: String = user_row.try_get("display_name")?;
    let got_status: String = user_row.try_get("status")?;
    assert_eq!(got_id, user_id);
    assert_eq!(got_email, "alice@example.com");
    assert_eq!(got_display, "Alice");
    assert_eq!(got_status, "active");

    // user_identities row exists with Argon2id PHC hash.
    let identity_row = sqlx::query(
        "SELECT provider, provider_sub, password_hash \
         FROM user_identities WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&fx.admin_pool)
    .await?;
    let provider: String = identity_row.try_get("provider")?;
    let provider_sub: String = identity_row.try_get("provider_sub")?;
    let password_hash: Option<String> = identity_row.try_get("password_hash")?;
    assert_eq!(provider, "internal");
    assert_eq!(provider_sub, "alice@example.com");
    let hash = password_hash.expect("password_hash must be set on signup");
    assert!(
        hash.starts_with("$argon2id$"),
        "expected argon2id PHC, got: {hash}"
    );

    // Sanity: exactly one row each.
    let users_count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&fx.admin_pool)
        .await?;
    assert_eq!(users_count, 1);
    let ids_count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_identities")
        .fetch_one(&fx.admin_pool)
        .await?;
    assert_eq!(ids_count, 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signup_duplicate_email() -> anyhow::Result<()> {
    let fx = boot().await?;
    let signup_url = promote_signup_role(&fx).await?;
    let login_url = promote_login_role(&fx).await?;

    let signup_pool = build_signup_pool(signup_url).await?;
    let login_pool = build_login_pool(login_url).await?;

    // Seed an existing user via the admin pool (bypassing the auth crate).
    sqlx::query(
        "INSERT INTO users (email, display_name, status) \
         VALUES ($1, $2, 'active')",
    )
    .bind("bob@example.com")
    .bind("Bob")
    .execute(&fx.admin_pool)
    .await?;

    let before_users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&fx.admin_pool)
        .await?;
    let before_identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_identities")
            .fetch_one(&fx.admin_pool)
            .await?;

    let password = SecretString::from("whatever".to_owned());
    let result = signup_user(
        login_pool.as_ref(),
        signup_pool.as_ref(),
        "bob@example.com",
        &password,
        "Bob 2",
    )
    .await;

    match result {
        Err(AuthError::DuplicateEmail) => {}
        other => panic!("expected DuplicateEmail, got: {other:?}"),
    }

    // No new rows — the transaction must have rolled back.
    let after_users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&fx.admin_pool)
        .await?;
    let after_identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_identities")
            .fetch_one(&fx.admin_pool)
            .await?;
    assert_eq!(before_users, after_users, "users row count changed");
    assert_eq!(
        before_identities, after_identities,
        "user_identities row count changed"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signup_pool_rejects_non_signup_role() -> anyhow::Result<()> {
    let fx = boot().await?;

    // Build a SignupPool from the SUPERUSER credentials — MUST fail.
    let bad_config = SignupConfig {
        database_url: fx.postgres_url.clone(),
        max_connections: 2,
    };
    let result = SignupPool::from_dedicated_config(&bad_config).await;
    match result {
        Err(AuthError::WrongRole(actual)) => {
            assert_eq!(actual, "postgres", "expected current_user = postgres");
        }
        other => panic!("expected WrongRole(postgres), got: {other:?}"),
    }
    Ok(())
}
