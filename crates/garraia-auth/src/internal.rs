//! `InternalProvider` — verifies email+password credentials against the
//! `user_identities` table for `provider = 'internal'` rows, with PBKDF2 →
//! Argon2id lazy upgrade and constant-time anti-enumeration.
//!
//! The verify path is implemented per ADR 0005 §"InternalProvider
//! implementation outline" and plan 0011 §6.1. Single transaction:
//!
//!   BEGIN
//!   SELECT ui.id, ui.user_id, ui.password_hash, u.status
//!       FROM user_identities ui JOIN users u ON u.id = ui.user_id
//!       WHERE ui.provider = 'internal' AND lower(ui.provider_sub) = lower($email)
//!       FOR NO KEY UPDATE OF ui
//!   if not found    -> consume DUMMY_HASH; audit failure_user_not_found; COMMIT; Ok(None)
//!   if status != 'active' -> consume DUMMY_HASH; audit failure_account_not_active; COMMIT; Ok(None)
//!   dispatch by hash prefix:
//!     argon2id  -> verify; if ok audit success -> Ok(Some); else audit failure_wrong_password -> Ok(None)
//!     pbkdf2    -> verify; if ok upgrade hash + audit upgrade + success -> Ok(Some); else failure
//!     other     -> audit failure_unknown_hash; ROLLBACK; Err(UnknownHashFormat)
//!   COMMIT
//!
//! The audit row is inside the same transaction as the verify (and any
//! lazy-upgrade UPDATE) for v1 atomicity. See plan 0011 §13 Q9 default.

use std::sync::Arc;

use async_trait::async_trait;
use sqlx::Row;
use uuid::Uuid;

use secrecy::SecretString;

use crate::audit::{audit_login, AuditAction};
use crate::error::AuthError;
use crate::hashing::{consume_dummy_hash, hash_argon2id, verify_argon2id, verify_pbkdf2};
use crate::login_pool::LoginPool;
use crate::provider::IdentityProvider;
use crate::signup_pool::SignupPool;
use crate::types::{Credential, Identity, RequestCtx};
use crate::Result;

/// Verifies credentials against `user_identities` using the dedicated
/// `LoginPool` (BYPASSRLS) exclusively.
///
/// Holds `Arc<LoginPool>` so the same validated pool can be shared with
/// [`crate::sessions::SessionStore`] without duplicating the connection
/// footprint. The boundary contract still holds: `LoginPool` is `!Clone`,
/// the only constructor is `LoginPool::from_dedicated_config`, and the
/// `Arc` wrapping happens at the call site after construction.
pub struct InternalProvider {
    login_pool: Arc<LoginPool>,
}

impl InternalProvider {
    /// Build an `InternalProvider` from a validated [`LoginPool`] wrapped
    /// in `Arc`. The caller MUST have constructed the inner `LoginPool`
    /// via [`LoginPool::from_dedicated_config`]; there is no other path.
    pub fn new(login_pool: Arc<LoginPool>) -> Self {
        Self { login_pool }
    }

    /// Verify a credential **with** an explicit [`RequestCtx`] used for
    /// `audit_events` insertion. This is the path the future Axum extractor
    /// (391c) will call directly. The trait method
    /// [`IdentityProvider::verify_credential`] delegates here with an empty
    /// `RequestCtx::default()` so existing callers stay valid.
    pub async fn verify_credential_with_ctx(
        &self,
        credential: &Credential,
        ctx: &RequestCtx,
    ) -> Result<Option<Uuid>> {
        let (email, password) = match credential {
            Credential::Internal { email, password } => (email.as_str(), password),
        };
        // `user_identities.provider_sub` is `text` (NOT `citext`) per migration
        // 001. The unique index is on `(provider, provider_sub)` — exact
        // match. We normalize to lowercase HERE so writes (`create_identity`
        // in 391c) and reads stay symmetric and the unique index is used
        // by the query plan. Wrapping the column in `lower()` would force
        // a sequential scan because the index has no `lower()` expression.
        let email_lower = email.to_lowercase();

        let mut tx = self
            .login_pool
            .pool()
            .begin()
            .await
            .map_err(AuthError::Storage)?;

        let row_opt = sqlx::query(
            "SELECT ui.id AS identity_id, \
                    ui.user_id AS user_id, \
                    ui.password_hash AS password_hash, \
                    u.status AS user_status \
             FROM user_identities ui \
             JOIN users u ON u.id = ui.user_id \
             WHERE ui.provider = 'internal' \
               AND ui.provider_sub = $1 \
             FOR NO KEY UPDATE OF ui",
        )
        .bind(&email_lower)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AuthError::Storage)?;

        // ─── User not found ────────────────────────────────────────────────
        let Some(row) = row_opt else {
            consume_dummy_hash(password)?;
            audit_login(
                &mut tx,
                AuditAction::LoginFailureUserNotFound,
                None,
                &email_lower,
                None,
                ctx,
            )
            .await?;
            tx.commit().await.map_err(AuthError::Storage)?;
            return Ok(None);
        };

        let identity_id: Uuid = row.try_get("identity_id").map_err(AuthError::Storage)?;
        let user_id: Uuid = row.try_get("user_id").map_err(AuthError::Storage)?;
        let stored_hash: Option<String> =
            row.try_get("password_hash").map_err(AuthError::Storage)?;
        let user_status: String = row.try_get("user_status").map_err(AuthError::Storage)?;

        // ─── Account not active ────────────────────────────────────────────
        if user_status != "active" {
            consume_dummy_hash(password)?;
            audit_login(
                &mut tx,
                AuditAction::LoginFailureAccountNotActive,
                Some(user_id),
                &email_lower,
                Some(identity_id),
                ctx,
            )
            .await?;
            tx.commit().await.map_err(AuthError::Storage)?;
            return Ok(None);
        }

        // user_identities.password_hash is nullable (provider != internal
        // rows have NULL). For `provider = 'internal'` rows it MUST be set;
        // a NULL here is an operational bug and is treated as unknown hash
        // format. Constant-time path: still consume the dummy hash before
        // returning so the wall-clock latency matches a real verify path
        // (security review 391b H-1).
        let Some(hash) = stored_hash else {
            consume_dummy_hash(password)?;
            audit_login(
                &mut tx,
                AuditAction::LoginFailureUnknownHash,
                Some(user_id),
                &email_lower,
                Some(identity_id),
                ctx,
            )
            .await?;
            // Commit BEFORE returning Err so the audit row persists. This
            // is intentional: an operational misconfiguration must leave a
            // forensic trail even though the request fails. Reviewers:
            // do NOT change to rollback.
            tx.commit().await.map_err(AuthError::Storage)?;
            return Err(AuthError::UnknownHashFormat);
        };

        // ─── Dispatch by prefix ────────────────────────────────────────────
        let outcome = if hash.starts_with("$argon2id$") {
            verify_argon2id(&hash, password)?
        } else if hash.starts_with("$pbkdf2-sha256$") || hash.starts_with("$pbkdf2$") {
            let ok = verify_pbkdf2(&hash, password)?;
            if ok {
                // Lazy upgrade in the same transaction. The row is
                // protected by FOR NO KEY UPDATE so concurrent verifies
                // serialize safely (race regression test in
                // tests/verify_internal.rs::concurrent_lazy_upgrade).
                let new_hash = hash_argon2id(password)?;
                sqlx::query(
                    "UPDATE user_identities \
                     SET password_hash = $1, \
                         hash_upgraded_at = now() \
                     WHERE id = $2",
                )
                .bind(&new_hash)
                .bind(identity_id)
                .execute(&mut *tx)
                .await
                .map_err(AuthError::Storage)?;
                audit_login(
                    &mut tx,
                    AuditAction::PasswordHashUpgraded,
                    Some(user_id),
                    &email_lower,
                    Some(identity_id),
                    ctx,
                )
                .await?;
            }
            ok
        } else {
            // Hash with an unrecognized prefix. Consume the dummy hash to
            // keep the timing profile constant (security review 391b H-1)
            // before audit + commit + Err.
            consume_dummy_hash(password)?;
            audit_login(
                &mut tx,
                AuditAction::LoginFailureUnknownHash,
                Some(user_id),
                &email_lower,
                Some(identity_id),
                ctx,
            )
            .await?;
            // Commit BEFORE returning Err so the audit row persists for
            // forensics. Same intentional commit-before-error pattern as
            // the NULL-hash branch above.
            tx.commit().await.map_err(AuthError::Storage)?;
            return Err(AuthError::UnknownHashFormat);
        };

        if outcome {
            audit_login(
                &mut tx,
                AuditAction::LoginSuccess,
                Some(user_id),
                &email_lower,
                Some(identity_id),
                ctx,
            )
            .await?;
            tx.commit().await.map_err(AuthError::Storage)?;
            Ok(Some(user_id))
        } else {
            audit_login(
                &mut tx,
                AuditAction::LoginFailureWrongPassword,
                Some(user_id),
                &email_lower,
                Some(identity_id),
                ctx,
            )
            .await?;
            tx.commit().await.map_err(AuthError::Storage)?;
            Ok(None)
        }
    }
}

#[async_trait]
impl IdentityProvider for InternalProvider {
    fn id(&self) -> &str {
        "internal"
    }

    /// Look up an identity by `provider_sub` (the email for `internal`).
    /// Read-only, no audit row, no lock. Used by the future refresh path
    /// and by tests.
    async fn find_by_provider_sub(&self, sub: &str) -> Result<Option<Identity>> {
        // Exact match on `provider_sub` so the unique index on
        // (provider, provider_sub) is used. `provider_sub` for `internal`
        // identities is normalized to lowercase on write.
        let row_opt = sqlx::query(
            "SELECT user_id FROM user_identities \
             WHERE provider = 'internal' AND provider_sub = $1",
        )
        .bind(sub.to_lowercase())
        .fetch_optional(self.login_pool.pool())
        .await
        .map_err(AuthError::Storage)?;
        let Some(row) = row_opt else {
            return Ok(None);
        };
        let user_id: Uuid = row.try_get("user_id").map_err(AuthError::Storage)?;
        Ok(Some(Identity {
            user_id,
            provider: "internal".to_string(),
            provider_sub: sub.to_lowercase(),
        }))
    }

    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>> {
        // Trait-level entry point keeps the audit ctx empty. The future
        // Axum extractor (391c) calls `verify_credential_with_ctx` directly
        // to populate ip/user_agent/request_id.
        self.verify_credential_with_ctx(credential, &RequestCtx::default())
            .await
    }

    async fn create_identity(&self, _user_id: Uuid, _credential: &Credential) -> Result<()> {
        // Deferred to GAR-391c alongside the signup endpoint.
        //
        // Design oversight surfaced empirically during 391b implementation:
        // the `garraia_login` role from migration 008 has SELECT/UPDATE on
        // `user_identities` but NOT INSERT — by design (the login role is
        // for the login flow, not signup). Wiring `create_identity` through
        // the LoginPool would either:
        //   (a) require migration 010 broadening the login role's grants
        //       (security regression — the login role's whole point is
        //       minimal scope), OR
        //   (b) require a separate signup pool with INSERT grant + the
        //       signup endpoint to call it.
        //
        // Both options belong to GAR-391c (signup endpoint + signup pool
        // design), not 391b. The signup endpoint itself is already deferred
        // per plan 0011 §3 "Out of scope", so `create_identity` follows
        // suit. Tests that need to seed identities use the admin pool
        // directly, bypassing the auth crate.
        Err(AuthError::NotImplemented)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 391c-impl-B — signup_user free function
// ─────────────────────────────────────────────────────────────────────────

/// Create a new `users` row plus a corresponding `user_identities` row with
/// a fresh Argon2id password hash, all inside a single transaction on the
/// dedicated [`SignupPool`].
///
/// This is a free function (not a trait method) because the trait
/// [`crate::provider::IdentityProvider`] is intentionally bound to the
/// `LoginPool`, and signup requires INSERT grants that the login role does
/// not carry. See plan 0012 §3.1 (migration 010) for the role/grant design.
///
/// ## Flow
///
/// 1. Lowercase email (symmetric with the login lookup on `provider_sub`).
/// 2. Hash the password via [`hash_argon2id`].
/// 3. Open a transaction on `signup_pool`.
/// 4. `INSERT INTO users (email, display_name, status='active') RETURNING id`.
///    A unique-violation on `(email)` maps to [`AuthError::DuplicateEmail`]
///    and the transaction is rolled back.
/// 5. `INSERT INTO user_identities (user_id, provider='internal',
///    provider_sub=email_lower, password_hash=phc) RETURNING id`. A unique
///    violation on `(provider, provider_sub)` likewise maps to
///    `DuplicateEmail`.
/// 6. Commit and return the new `user_id`.
///
/// ## Audit
///
/// No audit row is written here — per plan 0012 §3.2 the call-site (future
/// signup endpoint in 391c gateway wiring) is responsible for inserting the
/// signup-attempt audit event. This keeps `signup_user` focused on the DB
/// mutation and allows the wiring agent to capture richer `RequestCtx`
/// fields (`ip`, `user_agent`, `request_id`) from the HTTP layer.
///
/// ## Note on the unused `login_pool` parameter
///
/// The signature accepts `&LoginPool` so the future wiring code can perform
/// a pre-flight duplicate-email SELECT on the login pool if we decide to
/// front-run signups with a cheap read (avoiding the cost of a full
/// Argon2id hash on obvious collisions). In 391c-impl-B we do NOT use it —
/// the INSERT itself is authoritative and cheaper to reason about — but
/// keeping it in the signature now avoids churn when the optimisation
/// lands. The parameter is deliberately underscore-free so callers treat
/// it as load-bearing: the orchestration layer must own both pools anyway.
#[allow(unused_variables)]
pub async fn signup_user(
    login_pool: &LoginPool,
    signup_pool: &SignupPool,
    email: &str,
    password: &SecretString,
    display_name: &str,
) -> Result<Uuid> {
    let email_lower = email.to_lowercase();
    let password_hash = hash_argon2id(password)?;

    let mut tx = signup_pool
        .pool()
        .begin()
        .await
        .map_err(AuthError::Storage)?;

    // ─── users row ─────────────────────────────────────────────────────
    let user_id: Uuid = match sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (email, display_name, status) \
         VALUES ($1, $2, 'active') \
         RETURNING id",
    )
    .bind(&email_lower)
    .bind(display_name)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(err) => {
            if is_unique_violation(&err) {
                let _ = tx.rollback().await;
                return Err(AuthError::DuplicateEmail);
            }
            let _ = tx.rollback().await;
            return Err(AuthError::Storage(err));
        }
    };

    // ─── user_identities row ───────────────────────────────────────────
    let identity_result = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO user_identities (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, $3) \
         RETURNING id",
    )
    .bind(user_id)
    .bind(&email_lower)
    .bind(&password_hash)
    .fetch_one(&mut *tx)
    .await;

    if let Err(err) = identity_result {
        if is_unique_violation(&err) {
            let _ = tx.rollback().await;
            return Err(AuthError::DuplicateEmail);
        }
        let _ = tx.rollback().await;
        return Err(AuthError::Storage(err));
    }

    tx.commit().await.map_err(AuthError::Storage)?;
    Ok(user_id)
}

/// Return true if the `sqlx::Error` is a Postgres unique-violation
/// (SQLSTATE `23505`). Used by [`signup_user`] to translate DB collisions
/// into [`AuthError::DuplicateEmail`] without probing.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        if let Some(code) = db_err.code() {
            return code == "23505";
        }
    }
    false
}
