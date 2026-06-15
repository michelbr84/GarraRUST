//! Integration tests for `change_password` and `anonymize_identity`
//! (plan 0347 / GAR-891 — Q6.15).
//!
//! # Mutants killed
//!
//! * `password.rs:81:8` — `delete !` in `change_password` (shard 2): killed by
//!   [`change_password_correct_argon2id_returns_success`].
//! * `password.rs:75:58` — `replace \|\| with &&` in `change_password` (shard 1): killed by
//!   [`change_password_correct_pbkdf2sha256_returns_success`].
//! * `password.rs:119:5` — `replace anonymize_identity with Ok(())` (shard 0): killed by
//!   [`anonymize_identity_updates_login`].
//!
//! All tests use the shared `Harness` (one pgvector/pg16 container per binary,
//! isolation via unique UUIDs per test).

mod common;

use common::harness::Harness;
use garraia_auth::{PasswordChangeOutcome, anonymize_identity, change_password, hash_argon2id};
use password_hash::{PasswordHasher, SaltString};
use pbkdf2::Pbkdf2;
use secrecy::SecretString;
use sqlx::Row;
use uuid::Uuid;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// Insert one user + one internal identity via the admin pool.
/// Returns the new `user_id`. The `login` column is set to `email` so that
/// `anonymize_identity` tests can detect the change by querying it.
async fn seed(admin: &sqlx::PgPool, email: &str, password_hash: &str) -> anyhow::Result<Uuid> {
    let row = sqlx::query("INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id")
        .bind(email)
        .bind(email)
        .fetch_one(admin)
        .await?;
    let user_id: Uuid = row.try_get("id")?;

    sqlx::query(
        "INSERT INTO user_identities \
         (user_id, provider, provider_sub, login, password_hash) \
         VALUES ($1, 'internal', $2, $3, $4)",
    )
    .bind(user_id)
    .bind(user_id.to_string())
    .bind(email)
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
async fn change_password_correct_argon2id_returns_success() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let email = format!("cp-ok-argon-{}@garraia.test", Uuid::now_v7());
    let correct = pw("correct-horse-battery-staple-42");
    let hash = hash_argon2id(&correct)?;
    let user_id = seed(&admin, &email, &hash).await?;

    let outcome = change_password(
        &h.login_pool,
        user_id,
        &correct,
        &pw("brand-new-password-99"),
    )
    .await?;
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
async fn change_password_correct_pbkdf2sha256_returns_success() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let email = format!("cp-ok-pbkdf2-{}@garraia.test", Uuid::now_v7());

    // Build a real PBKDF2-SHA256 PHC string — same pattern as verify_internal.rs.
    let plaintext_str = "legacy-pbkdf2-password-for-change";
    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
    let phc = Pbkdf2
        .hash_password(plaintext_str.as_bytes(), &salt)
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
        &h.login_pool,
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
async fn change_password_wrong_argon2id_returns_wrong_password() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let email = format!("cp-wrong-{}@garraia.test", Uuid::now_v7());
    let real_hash = hash_argon2id(&pw("actual-secret-password"))?;
    let user_id = seed(&admin, &email, &real_hash).await?;

    let outcome = change_password(
        &h.login_pool,
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

/// Kills `password.rs:119:5` — `replace anonymize_identity → Ok(())` mutant.
///
/// If the function body were replaced with `Ok(())`, `user_identities.login`
/// would remain the original email. This test queries the column directly and
/// panics if the UPDATE was silently skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anonymize_identity_updates_login() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;
    let email = format!("anon-target-{}@garraia.test", Uuid::now_v7());
    let hash = hash_argon2id(&pw("any-password-for-anon"))?;
    let user_id = seed(&admin, &email, &hash).await?;

    anonymize_identity(&h.login_pool, user_id).await?;

    let row = sqlx::query(
        "SELECT login FROM user_identities \
         WHERE user_id = $1 AND provider = 'internal'",
    )
    .bind(user_id)
    .fetch_one(&admin)
    .await?;
    let new_login: Option<String> = row.try_get("login")?;
    let new_login = new_login.expect(
        "login must be non-NULL after anonymize_identity — \
         fails if `replace with Ok(())` mutant (password.rs:119) is active",
    );
    assert!(
        new_login.starts_with("anon-"),
        "login must start with 'anon-' after anonymization, got: {new_login}"
    );
    assert!(
        new_login.ends_with("@garraanon.local"),
        "login must end with '@garraanon.local' after anonymization, got: {new_login}"
    );
    assert_ne!(
        new_login, email,
        "login must have changed from the original email"
    );
    Ok(())
}
