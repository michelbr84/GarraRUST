//! Integration tests for `InternalProvider::verify_credential` (GAR-391b).
//!
//! Single test binary that boots one `pgvector/pg16` testcontainer, applies
//! migrations 001..009, and exercises every terminal of the verify path:
//!
//!  1. Argon2id happy path → `Ok(Some)` + `audit_events` row `login.success`
//!  2. PBKDF2 happy path → `Ok(Some)` + lazy upgrade UPDATE + 2 audit rows
//!     (`login.password_hash_upgraded` + `login.success`)
//!  3. Wrong password → `Ok(None)` + `audit_events` row `login.failure_wrong_password`
//!  4. User not found → `Ok(None)` + `audit_events` row `login.failure_user_not_found`
//!     with `actor_user_id IS NULL`
//!  5. Account suspended → `Ok(None)` + `audit_events` row `login.failure_account_suspended`
//!  6. Account deleted → same path as suspended
//!  7. Unknown hash format → `Err(UnknownHashFormat)` + audit row
//!     `login.failure_unknown_hash` (committed even on the error path)
//!  8. `find_by_provider_sub` empty / hit
//!  9. `create_identity` writes an Argon2id PHC row
//! 10. RequestCtx propagation: `ip`, `user_agent`, `request_id` populated
//!     in the audit row's dedicated columns / metadata
//!
//! Concurrent lazy upgrade race regression lives in `concurrent_upgrade.rs`.
//! Endpoint integration test lives in `garraia-gateway` under feature `auth-v1`.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use garraia_auth::{
    AuthError, Credential, IdentityProvider, InternalProvider, LoginConfig, LoginPool, RequestCtx,
    audit::AuditAction, hash_argon2id,
};
use garraia_workspace::{Workspace, WorkspaceConfig};
use secrecy::SecretString;
use sqlx::Row;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

/// Per-test fixture: container handle (kept alive for the duration of the
/// test) + admin URL + login URL + the constructed `InternalProvider`.
struct Fixture {
    _container: ContainerAsync<PgImage>,
    admin_pool: sqlx::PgPool,
    provider: Arc<InternalProvider>,
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

    // Apply migrations 001..009.
    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // Promote garraia_login to LOGIN with a known password.
    let admin_pool = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin_pool)
        .await?;

    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let pool = Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 4,
        })
        .await?,
    );
    let provider = Arc::new(InternalProvider::new(pool));

    Ok(Fixture {
        _container: container,
        admin_pool,
        provider,
    })
}

/// Insert a user + an internal identity row directly via the admin pool,
/// bypassing the auth crate. Returns `(user_id, identity_id)`.
///
/// `password_hash` may be NULL (caller passes an Option) so tests can
/// exercise the unknown-hash terminal.
async fn seed_user(
    admin: &sqlx::PgPool,
    email: &str,
    password_hash: Option<&str>,
    status: &str,
) -> anyhow::Result<(Uuid, Uuid)> {
    let user_row = sqlx::query(
        "INSERT INTO users (email, display_name, status) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(email)
    .bind(email)
    .bind(status)
    .fetch_one(admin)
    .await?;
    let user_id: Uuid = user_row.try_get("id")?;

    let identity_row = sqlx::query(
        "INSERT INTO user_identities (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(email)
    .bind(password_hash)
    .fetch_one(admin)
    .await?;
    let identity_id: Uuid = identity_row.try_get("id")?;
    Ok((user_id, identity_id))
}

/// Snapshot of an audit_events row used in assertions.
struct AuditRow {
    actor_user_id: Option<Uuid>,
    actor_label: Option<String>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    ip: Option<sqlx::types::ipnetwork::IpNetwork>,
    user_agent: Option<String>,
    metadata: serde_json::Value,
    #[allow(dead_code)]
    created_at: DateTime<Utc>,
}

async fn last_audit_for(admin: &sqlx::PgPool, action: &str) -> anyhow::Result<AuditRow> {
    let row = sqlx::query(
        "SELECT actor_user_id, actor_label, action, resource_type, resource_id, \
                ip, user_agent, metadata, created_at \
         FROM audit_events \
         WHERE action = $1 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(action)
    .fetch_one(admin)
    .await?;
    Ok(AuditRow {
        actor_user_id: row.try_get("actor_user_id")?,
        actor_label: row.try_get("actor_label")?,
        action: row.try_get("action")?,
        resource_type: row.try_get("resource_type")?,
        resource_id: row.try_get("resource_id")?,
        ip: row.try_get("ip")?,
        user_agent: row.try_get("user_agent")?,
        metadata: row.try_get("metadata")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn count_audit_action(admin: &sqlx::PgPool, action: &str) -> anyhow::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
        .bind(action)
        .fetch_one(admin)
        .await?;
    Ok(count)
}

fn ctx() -> RequestCtx {
    RequestCtx {
        ip: Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
        user_agent: Some("integration-test/1.0".into()),
        request_id: Some("req-abc-123".into()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn argon2id_happy_path_emits_login_success_audit() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("correct horse battery staple".to_owned());
    let phc = hash_argon2id(&pw)?;
    let (user_id, identity_id) =
        seed_user(&f.admin_pool, "alice@example.com", Some(&phc), "active").await?;

    let cred = Credential::Internal {
        email: "alice@example.com".into(),
        password: pw.clone(),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, Some(user_id));

    let row = last_audit_for(&f.admin_pool, AuditAction::LoginSuccess.as_str()).await?;
    assert_eq!(row.actor_user_id, Some(user_id));
    assert_eq!(row.actor_label.as_deref(), Some("alice@example.com"));
    assert_eq!(row.action, "login.success");
    assert_eq!(row.resource_type, "user_identities");
    assert_eq!(
        row.resource_id.as_deref(),
        Some(identity_id.to_string().as_str())
    );
    assert!(row.ip.is_some(), "ip column populated from RequestCtx");
    assert_eq!(row.user_agent.as_deref(), Some("integration-test/1.0"));
    assert_eq!(
        row.metadata.get("request_id").and_then(|v| v.as_str()),
        Some("req-abc-123")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pbkdf2_happy_path_lazy_upgrades_to_argon2id() -> anyhow::Result<()> {
    use password_hash::PasswordHasher;
    use password_hash::SaltString;
    use pbkdf2::Pbkdf2;
    let f = boot().await?;

    // Build a real PBKDF2-SHA256 PHC string with a known plaintext.
    let plaintext = "legacy-pw";
    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
    let phc = Pbkdf2
        .hash_password(plaintext.as_bytes(), &salt)
        .unwrap()
        .to_string();
    assert!(phc.starts_with("$pbkdf2-sha256$"));

    let (user_id, identity_id) =
        seed_user(&f.admin_pool, "bob@example.com", Some(&phc), "active").await?;

    let cred = Credential::Internal {
        email: "bob@example.com".into(),
        password: SecretString::from(plaintext.to_owned()),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, Some(user_id));

    // Stored hash must now be an Argon2id PHC and `hash_upgraded_at` must
    // be set.
    let row =
        sqlx::query("SELECT password_hash, hash_upgraded_at FROM user_identities WHERE id = $1")
            .bind(identity_id)
            .fetch_one(&f.admin_pool)
            .await?;
    let new_hash: String = row.try_get("password_hash")?;
    assert!(new_hash.starts_with("$argon2id$"));
    let upgraded_at: Option<DateTime<Utc>> = row.try_get("hash_upgraded_at")?;
    assert!(upgraded_at.is_some(), "hash_upgraded_at must be set");

    // Both audit rows present.
    assert_eq!(
        count_audit_action(&f.admin_pool, AuditAction::PasswordHashUpgraded.as_str()).await?,
        1
    );
    assert_eq!(
        count_audit_action(&f.admin_pool, AuditAction::LoginSuccess.as_str()).await?,
        1
    );

    // Re-verifying with the same plaintext now goes through the Argon2id
    // branch (no second upgrade row).
    let cred2 = Credential::Internal {
        email: "bob@example.com".into(),
        password: SecretString::from(plaintext.to_owned()),
    };
    let result2 = f
        .provider
        .verify_credential_with_ctx(&cred2, &ctx())
        .await?;
    assert_eq!(result2, Some(user_id));
    assert_eq!(
        count_audit_action(&f.admin_pool, AuditAction::PasswordHashUpgraded.as_str()).await?,
        1,
        "second login must NOT emit another upgrade row"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_password_returns_none_with_failure_audit() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("right".to_owned());
    let phc = hash_argon2id(&pw)?;
    let (user_id, _) = seed_user(&f.admin_pool, "carol@example.com", Some(&phc), "active").await?;

    let cred = Credential::Internal {
        email: "carol@example.com".into(),
        password: SecretString::from("wrong".to_owned()),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, None);

    let row = last_audit_for(
        &f.admin_pool,
        AuditAction::LoginFailureWrongPassword.as_str(),
    )
    .await?;
    assert_eq!(row.actor_user_id, Some(user_id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn user_not_found_returns_none_with_null_actor_audit() -> anyhow::Result<()> {
    let f = boot().await?;
    let cred = Credential::Internal {
        email: "ghost@example.com".into(),
        password: SecretString::from("anything".to_owned()),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, None);

    let row = last_audit_for(
        &f.admin_pool,
        AuditAction::LoginFailureUserNotFound.as_str(),
    )
    .await?;
    assert!(
        row.actor_user_id.is_none(),
        "actor_user_id MUST be NULL for user_not_found"
    );
    assert_eq!(row.actor_label.as_deref(), Some("ghost@example.com"));
    assert!(row.resource_id.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suspended_account_returns_none_with_account_audit() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("ok".to_owned());
    let phc = hash_argon2id(&pw)?;
    let (user_id, _) = seed_user(&f.admin_pool, "dan@example.com", Some(&phc), "suspended").await?;

    let cred = Credential::Internal {
        email: "dan@example.com".into(),
        password: pw.clone(),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, None);

    let row = last_audit_for(
        &f.admin_pool,
        AuditAction::LoginFailureAccountNotActive.as_str(),
    )
    .await?;
    assert_eq!(row.actor_user_id, Some(user_id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deleted_account_takes_same_path_as_suspended() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("ok".to_owned());
    let phc = hash_argon2id(&pw)?;
    let (user_id, _) = seed_user(&f.admin_pool, "erin@example.com", Some(&phc), "deleted").await?;

    let cred = Credential::Internal {
        email: "erin@example.com".into(),
        password: pw.clone(),
    };
    let result = f.provider.verify_credential_with_ctx(&cred, &ctx()).await?;
    assert_eq!(result, None);

    let row = last_audit_for(
        &f.admin_pool,
        AuditAction::LoginFailureAccountNotActive.as_str(),
    )
    .await?;
    assert_eq!(row.actor_user_id, Some(user_id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_hash_format_returns_err_with_audit() -> anyhow::Result<()> {
    let f = boot().await?;
    let (user_id, _) = seed_user(
        &f.admin_pool,
        "frank@example.com",
        Some("$bcrypt$2b$12$xxxx"),
        "active",
    )
    .await?;

    let cred = Credential::Internal {
        email: "frank@example.com".into(),
        password: SecretString::from("anything".to_owned()),
    };
    match f.provider.verify_credential_with_ctx(&cred, &ctx()).await {
        Err(AuthError::UnknownHashFormat) => {}
        other => panic!("expected UnknownHashFormat, got: {other:?}"),
    }

    // Audit row must still be present (insertion happened before the
    // tx commit on the err path).
    let row = last_audit_for(&f.admin_pool, AuditAction::LoginFailureUnknownHash.as_str()).await?;
    assert_eq!(row.actor_user_id, Some(user_id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn find_by_provider_sub_returns_identity() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("ok".to_owned());
    let phc = hash_argon2id(&pw)?;
    let (user_id, _) = seed_user(&f.admin_pool, "gina@example.com", Some(&phc), "active").await?;

    let id = f
        .provider
        .find_by_provider_sub("gina@example.com")
        .await?
        .expect("identity present");
    assert_eq!(id.user_id, user_id);
    assert_eq!(id.provider, "internal");

    assert!(
        f.provider
            .find_by_provider_sub("nobody@example.com")
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_identity_is_deferred_to_391c() -> anyhow::Result<()> {
    // Design oversight surfaced during 391b implementation: the
    // `garraia_login` role from migration 008 has no INSERT on
    // `user_identities`. The signup endpoint is already deferred to 391c
    // per plan 0011 §3, so `create_identity` follows suit and lands with
    // the signup pool design in 391c.
    //
    // Until then, the trait method returns NotImplemented so callers fail
    // loudly instead of accidentally trying to hit the login pool.
    let f = boot().await?;
    let cred = Credential::Internal {
        email: "hank@example.com".into(),
        password: SecretString::from("brand new".to_owned()),
    };
    match f.provider.create_identity(Uuid::nil(), &cred).await {
        Err(AuthError::NotImplemented) => Ok(()),
        other => panic!("expected NotImplemented (deferred to 391c), got: {other:?}"),
    }
}

// ───────────────────────────────────────────────────────────────────────────
// GAR-463 Q6.1 — kill `verify_credential → Ok(None)` (internal.rs:286)
//
// All other tests in this file call `verify_credential_with_ctx` directly,
// leaving the trait-level entry point (`IdentityProvider::verify_credential`)
// unexercised. cargo-mutants reports the trait method's `Ok(None)` mutant as
// missed because nothing observes its delegation. This test exercises the
// trait method via a `&dyn IdentityProvider` and asserts the happy path.
// ───────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_credential_trait_method_happy_path() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("correct horse battery staple".to_owned());
    let phc = hash_argon2id(&pw)?;

    // .invalid TLD per RFC 2606 + UUID for safe parallel test execution.
    let email = format!("trait-method-{}@example.invalid", Uuid::now_v7());
    let (user_id, _identity_id) = seed_user(&f.admin_pool, &email, Some(&phc), "active").await?;

    // Call THROUGH the IdentityProvider trait, not `_with_ctx` directly.
    // The trait method is a simple delegation
    // (`verify_credential_with_ctx(cred, &RequestCtx::default())`); the
    // mutant `Ok(None)` would skip that call entirely.
    let provider: &dyn IdentityProvider = f.provider.as_ref();
    let cred = Credential::Internal {
        email: email.clone(),
        password: pw,
    };
    let result = provider
        .verify_credential(&cred)
        .await
        .expect("trait verify_credential must not error on a valid credential");

    match result {
        Some(uid) => assert_eq!(
            uid, user_id,
            "trait method must return the same user id as the seeded user"
        ),
        None => panic!(
            "trait method returned Ok(None) for a valid credential — \
             mutant `internal.rs:286 → Ok(None)` triggers this panic"
        ),
    }
    Ok(())
}
