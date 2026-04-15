//! Race-regression test for the lazy upgrade PBKDF2 → Argon2id path.
//!
//! Without `SELECT ... FOR NO KEY UPDATE OF ui` the verify path could let
//! two concurrent logins for the same PBKDF2 user both go through the
//! upgrade UPDATE, producing two `audit_events` rows for
//! `login.password_hash_upgraded`. With the row lock, exactly one of the
//! concurrent verifies upgrades; the others see the already-Argon2id hash
//! when their lock acquisition completes and skip the UPDATE branch.
//!
//! The test fans out 5 concurrent verifies for the same PBKDF2 user and
//! asserts:
//!   - all 5 return `Ok(Some(user_id))`
//!   - exactly 1 `login.password_hash_upgraded` audit row is present
//!   - 5 `login.success` audit rows are present
//!   - the stored hash is Argon2id

use std::sync::Arc;

use garraia_auth::{
    AuthError, Credential, InternalProvider, LoginConfig, LoginPool, RequestCtx, audit::AuditAction,
};
use garraia_workspace::{Workspace, WorkspaceConfig};
use password_hash::{PasswordHasher, SaltString};
use pbkdf2::Pbkdf2;
use secrecy::SecretString;
use sqlx::Row;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_lazy_upgrade_emits_exactly_one_upgrade_row() -> anyhow::Result<()> {
    // Boot.
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let postgres_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    let admin = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin)
        .await?;

    // Seed a PBKDF2 user.
    let plaintext = "concurrent-pw";
    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
    let phc = Pbkdf2
        .hash_password(plaintext.as_bytes(), &salt)
        .unwrap()
        .to_string();
    assert!(phc.starts_with("$pbkdf2-sha256$"));

    let user_row = sqlx::query(
        "INSERT INTO users (email, display_name, status) VALUES ($1, $1, 'active') RETURNING id",
    )
    .bind("race@example.com")
    .fetch_one(&admin)
    .await?;
    let user_id: Uuid = user_row.try_get("id")?;
    sqlx::query(
        "INSERT INTO user_identities (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, $3)",
    )
    .bind(user_id)
    .bind("race@example.com")
    .bind(&phc)
    .execute(&admin)
    .await?;

    // Build a SHARED InternalProvider with a multi-conn LoginPool.
    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let pool = Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 8,
        })
        .await?,
    );
    let provider = Arc::new(InternalProvider::new(pool));

    // Fan out 5 concurrent verifies. Use a barrier to maximize the chance
    // they all hit BEGIN at roughly the same time.
    let barrier = Arc::new(tokio::sync::Barrier::new(5));
    let mut handles = Vec::new();
    for _ in 0..5 {
        let p = provider.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let cred = Credential::Internal {
                email: "race@example.com".into(),
                password: SecretString::from("concurrent-pw".to_owned()),
            };
            p.verify_credential_with_ctx(&cred, &RequestCtx::default())
                .await
        }));
    }

    let mut successes = 0usize;
    for h in handles {
        let r: Result<Option<Uuid>, AuthError> = h.await?;
        let v = r?;
        assert_eq!(v, Some(user_id));
        successes += 1;
    }
    assert_eq!(successes, 5);

    // Exactly one upgrade row.
    let upgrade_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
            .bind(AuditAction::PasswordHashUpgraded.as_str())
            .fetch_one(&admin)
            .await?;
    assert_eq!(
        upgrade_count, 1,
        "concurrent lazy upgrade must produce exactly 1 upgrade audit row, got {upgrade_count}"
    );

    // 5 success rows.
    let success_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
            .bind(AuditAction::LoginSuccess.as_str())
            .fetch_one(&admin)
            .await?;
    assert_eq!(success_count, 5);

    // Stored hash is now Argon2id.
    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM user_identities WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&admin)
            .await?;
    assert!(stored.starts_with("$argon2id$"));
    Ok(())
}
