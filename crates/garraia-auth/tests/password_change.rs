//! Integration tests for `change_password` and `anonymize_identity`
//! (plan 0347 / GAR-891 — Q6.15; anonymize fixado no plan 0354).
//!
//! # Mutants killed
//!
//! * `change_password` `delete !` (shard 2): killed by
//!   [`change_password_correct_argon2id_returns_success`].
//! * `change_password` `replace \|\| with &&` (shard 1): killed by
//!   [`change_password_correct_pbkdf2sha256_returns_success`].
//! * `anonymize_identity` `replace body with Ok(...)`: killed by
//!   [`anonymize_identity_replaces_provider_sub`].
//! * `anonymize_identity` `delete WHERE user_id` clause variants: killed by
//!   [`anonymize_identity_leaves_other_users_untouched`].
//!
//! All tests use the shared `Harness` (one pgvector/pg16 container per binary,
//! isolation via unique UUIDs per test).
//!
//! ## Pools por teste, não o `h.login_pool` compartilhado
//!
//! Mesmo dragão documentado em `garraia-gateway/tests/rest_v1_me_authed.rs`:
//! cada `#[tokio::test]` cria e destrói um runtime tokio próprio, e conexões
//! sqlx adquiridas num runtime nem sempre voltam ao pool compartilhado do
//! `Harness` antes de o runtime seguinte tentar adquirir — resultado flaky
//! "pool timed out while waiting for an open connection" (`#[serial]` sozinho
//! NÃO resolve; foi testado). Aqui cada teste constrói seu próprio `LoginPool`
//! via [`fresh_login_pool`], que nasce e morre dentro do runtime do teste.
//! `#[serial]` continua aplicado só para não empilhar hashing Argon2id/PBKDF2
//! (CPU-pesado em debug) em runners de CI de 2 cores.

mod common;

use common::harness::Harness;
use garraia_auth::{
    LoginConfig, LoginPool, PasswordChangeOutcome, anonymize_identity, change_password,
    hash_argon2id,
};
use password_hash::PasswordHasher;
use pbkdf2::Pbkdf2;
use secrecy::SecretString;
use sqlx::Row;
use uuid::Uuid;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// `LoginPool` dedicado ao teste corrente (ver doc do módulo). Mesma derivação
/// de URL que o `Harness` usa; 2 conexões bastam para um teste linear.
async fn fresh_login_pool(h: &Harness) -> anyhow::Result<LoginPool> {
    let login_url = h
        .admin_url
        .replace("postgres:postgres@", "garraia_login:login-pw@");
    Ok(LoginPool::from_dedicated_config(&LoginConfig {
        database_url: login_url,
        max_connections: 2,
    })
    .await?)
}

/// Insert one user + one internal identity via the admin pool.
/// Returns the new `user_id`. `provider_sub` recebe o email lowercase —
/// espelha o signup real (`create_internal_user`), que insere o email ali,
/// e o login lookup (`verify_internal`), que casa `provider_sub = $email`.
/// (Não existe coluna `login` neste schema — migration 001; o seed antigo
/// que a inseria quebrava o baseline inteiro do cargo-mutants.)
async fn seed(admin: &sqlx::PgPool, email: &str, password_hash: &str) -> anyhow::Result<Uuid> {
    let row = sqlx::query("INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id")
        .bind(email)
        .bind(email)
        .fetch_one(admin)
        .await?;
    let user_id: Uuid = row.try_get("id")?;

    sqlx::query(
        "INSERT INTO user_identities \
         (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, $3)",
    )
    .bind(user_id)
    .bind(email.to_lowercase())
    .bind(password_hash)
    .execute(admin)
    .await?;

    Ok(user_id)
}

// ── change_password tests ────────────────────────────────────────────────────

/// Kills `password.rs:81:8` — `delete !` mutant.
///
/// The production code has `if !matches { return Ok(WrongPassword); }`.
/// If the `!` were deleted it becomes `if matches { return Ok(WrongPassword); }`,
/// meaning a CORRECT password returns `WrongPassword`. This test panics in that case.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn change_password_correct_argon2id_returns_success() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let login_pool = fresh_login_pool(&h).await?;
    let email = format!("cp-ok-argon-{}@garraia.test", Uuid::now_v7());
    let correct = pw("correct-horse-battery-staple-42");
    let hash = hash_argon2id(&correct)?;
    let user_id = seed(&admin, &email, &hash).await?;

    let outcome =
        change_password(&login_pool, user_id, &correct, &pw("brand-new-password-99")).await?;
    assert_eq!(
        outcome,
        PasswordChangeOutcome::Success,
        "correct Argon2id password must return Success — \
         fails if `delete !` mutant (password.rs:81) is active"
    );
    Ok(())
}

/// Kills `password.rs:75:58` — `replace \|\| with &&` mutant.
///
/// A PBKDF2-SHA256 PHC string starts with `$pbkdf2-sha256$` but NOT `$pbkdf2$`.
/// The production branch is:
/// ```
/// } else if stored_hash.starts_with("$pbkdf2-sha256$") || stored_hash.starts_with("$pbkdf2$") {
/// ```
/// If `||` were replaced with `&&`, a `$pbkdf2-sha256$` hash would not match
/// (it satisfies the first condition but not the second), causing the function
/// to fall through to `Err(AuthError::UnknownHashFormat)` instead of `Success`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn change_password_correct_pbkdf2sha256_returns_success() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let login_pool = fresh_login_pool(&h).await?;
    let email = format!("cp-ok-pbkdf2-{}@garraia.test", Uuid::now_v7());

    // Build a real PBKDF2-SHA256 PHC string — same pattern as verify_internal.rs.
    // Salt generated internally by `hash_password` (password-hash 0.6).
    let plaintext_str = "legacy-pbkdf2-password-for-change";
    let phc = Pbkdf2::default()
        .hash_password(plaintext_str.as_bytes())
        .expect("PBKDF2 hash must succeed in tests")
        .to_string();
    // Hard assertion: the prefix must be exactly what production code dispatches on.
    assert!(
        phc.starts_with("$pbkdf2-sha256$"),
        "PBKDF2 PHC must start with $pbkdf2-sha256$ — prefix assumption violated: {phc}"
    );
    assert!(
        !phc.starts_with("$pbkdf2$"),
        "PBKDF2-SHA256 must NOT start with bare $pbkdf2$ — test is targeting the wrong mutant"
    );

    let user_id = seed(&admin, &email, &phc).await?;
    let outcome = change_password(
        &login_pool,
        user_id,
        &pw(plaintext_str),
        &pw("new-password-after-upgrade"),
    )
    .await?;
    assert_eq!(
        outcome,
        PasswordChangeOutcome::Success,
        "PBKDF2-SHA256 correct password must return Success — \
         fails if `|| → &&` mutant (password.rs:75) is active"
    );
    Ok(())
}

/// Baseline: wrong password returns `WrongPassword` (anti-enumeration path).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn change_password_wrong_argon2id_returns_wrong_password() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let login_pool = fresh_login_pool(&h).await?;
    let email = format!("cp-wrong-{}@garraia.test", Uuid::now_v7());
    let real_hash = hash_argon2id(&pw("actual-secret-password"))?;
    let user_id = seed(&admin, &email, &real_hash).await?;

    let outcome = change_password(
        &login_pool,
        user_id,
        &pw("definitely-wrong-password"),
        &pw("irrelevant-new-password"),
    )
    .await?;
    assert_eq!(
        outcome,
        PasswordChangeOutcome::WrongPassword,
        "wrong password must return WrongPassword"
    );
    Ok(())
}

// ── anonymize_identity tests ─────────────────────────────────────────────────

/// Kills o mutant `replace anonymize_identity → Ok(...)`.
///
/// Neste schema o email de login mora em `user_identities.provider_sub`
/// (o signup insere o email ali; `verify_internal` casa `provider_sub = $email`).
/// Após `anonymize_identity` o token deve ser determinístico com o UUID
/// COMPLETO (32 hex): um prefixo de 8 hex colidiria entre usuários criados
/// na mesma janela de ~65 s sob UUIDv7 — e `UNIQUE (provider, provider_sub)`
/// transformaria a colisão em erro do segundo anonymize.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn anonymize_identity_replaces_provider_sub() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let login_pool = fresh_login_pool(&h).await?;
    let email = format!("anon-target-{}@garraia.test", Uuid::now_v7());
    let hash = hash_argon2id(&pw("any-password-for-anon"))?;
    let user_id = seed(&admin, &email, &hash).await?;

    let rows = anonymize_identity(&login_pool, user_id).await?;
    assert_eq!(
        rows, 1,
        "exactly the seeded internal identity must be updated"
    );

    let row = sqlx::query(
        "SELECT provider_sub FROM user_identities \
         WHERE user_id = $1 AND provider = 'internal'",
    )
    .bind(user_id)
    .fetch_one(&admin)
    .await?;
    let new_sub: String = row.try_get("provider_sub")?;
    let expected = format!("anon-{}@garraanon.local", user_id.simple());
    assert_eq!(
        new_sub, expected,
        "provider_sub must be the deterministic full-UUID anon token"
    );
    assert_ne!(
        new_sub,
        email.to_lowercase(),
        "provider_sub must have changed from the original email"
    );
    Ok(())
}

/// Kills mutants que degradam a cláusula WHERE (`user_id = $2` /
/// `provider = 'internal'`): anonimizar o usuário A não pode tocar o B.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn anonymize_identity_leaves_other_users_untouched() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let login_pool = fresh_login_pool(&h).await?;
    let email_a = format!("anon-a-{}@garraia.test", Uuid::now_v7());
    let email_b = format!("anon-b-{}@garraia.test", Uuid::now_v7());
    let hash = hash_argon2id(&pw("shared-password-for-pair"))?;
    let user_a = seed(&admin, &email_a, &hash).await?;
    let user_b = seed(&admin, &email_b, &hash).await?;

    let rows = anonymize_identity(&login_pool, user_a).await?;
    assert_eq!(rows, 1, "only user A's identity may be updated");

    let row = sqlx::query(
        "SELECT provider_sub FROM user_identities \
         WHERE user_id = $1 AND provider = 'internal'",
    )
    .bind(user_b)
    .fetch_one(&admin)
    .await?;
    let sub_b: String = row.try_get("provider_sub")?;
    assert_eq!(
        sub_b,
        email_b.to_lowercase(),
        "user B's provider_sub must remain the original email"
    );
    Ok(())
}

/// Contrato: usuário sem identidade `internal` (ou inexistente) → 0 linhas,
/// sem erro. O handler decide o que fazer; a função não inventa falha.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn anonymize_identity_returns_zero_for_unknown_user() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let login_pool = fresh_login_pool(&h).await?;
    let rows = anonymize_identity(&login_pool, Uuid::now_v7()).await?;
    assert_eq!(rows, 0, "unknown user must affect zero rows");
    Ok(())
}
