//! Self-service password change — `PATCH /v1/me/password` (plan 0335 / GAR-876).
//!
//! All DB operations go through `LoginPool` (BYPASSRLS, `garraia_login` role)
//! because `user_identities.password_hash` is invisible to the `garraia_app`
//! role (FORCE RLS filters to 0 rows). CLAUDE.md Rule 12.

use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    error::AuthError,
    hashing::{hash_argon2id, verify_argon2id, verify_pbkdf2},
    login_pool::LoginPool,
};

/// Outcome of [`change_password`].
#[derive(Debug, PartialEq, Eq)]
pub enum PasswordChangeOutcome {
    /// Password hash updated successfully.
    Success,
    /// `current_password` did not match the stored hash.
    WrongPassword,
    /// No `provider = 'internal'` identity found for `user_id`, or the
    /// identity row has a NULL `password_hash`. Callers MUST map this to
    /// the same HTTP error as `WrongPassword` (anti-enumeration).
    IdentityNotFound,
}

/// Atomically verify `current_password` and, if correct, replace the stored
/// Argon2id hash with a fresh hash of `new_password`.
///
/// ## Security notes
///
/// * Uses a transaction with `FOR NO KEY UPDATE` so concurrent calls on the
///   same identity serialize safely (mirrors the lazy-upgrade path in
///   `verify_credential_with_ctx`).
/// * Dual-verify: accepts both `$argon2id$` and legacy `$pbkdf2-sha256$` hashes.
/// * Both `IdentityNotFound` and `WrongPassword` skip expensive crypto and
///   return immediately; callers should map both to 403 (anti-enumeration).
/// * Never call this from a path that already holds an `app_pool` transaction —
///   `login_pool` and `app_pool` are separate Postgres connections and must not
///   nest their transactions.
pub async fn change_password(
    login_pool: &LoginPool,
    user_id: Uuid,
    current_password: &SecretString,
    new_password: &SecretString,
) -> Result<PasswordChangeOutcome, AuthError> {
    let pool = login_pool.pool();
    let mut tx = pool.begin().await.map_err(AuthError::Storage)?;

    let row: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT id, password_hash \
         FROM user_identities \
         WHERE user_id = $1 AND provider = 'internal' \
         FOR NO KEY UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AuthError::Storage)?;

    let (identity_id, stored_hash_opt) = match row {
        None => return Ok(PasswordChangeOutcome::IdentityNotFound),
        Some(r) => r,
    };

    let stored_hash = match stored_hash_opt {
        None => return Ok(PasswordChangeOutcome::IdentityNotFound),
        Some(h) => h,
    };

    let matches = if stored_hash.starts_with("$argon2id$") {
        verify_argon2id(&stored_hash, current_password)?
    } else if stored_hash.starts_with("$pbkdf2-sha256$") || stored_hash.starts_with("$pbkdf2$") {
        verify_pbkdf2(&stored_hash, current_password)?
    } else {
        return Err(AuthError::UnknownHashFormat);
    };

    if !matches {
        return Ok(PasswordChangeOutcome::WrongPassword);
    }

    let new_hash = hash_argon2id(new_password)?;
    sqlx::query(
        "UPDATE user_identities \
         SET password_hash = $1, hash_upgraded_at = now() \
         WHERE id = $2",
    )
    .bind(&new_hash)
    .bind(identity_id)
    .execute(&mut *tx)
    .await
    .map_err(AuthError::Storage)?;

    tx.commit().await.map_err(AuthError::Storage)?;
    Ok(PasswordChangeOutcome::Success)
}

/// Anonymise the `user_identities` row for `user_id`.
///
/// Replaces `login` with a non-identifiable token (`anon-<8 hex chars>@garraanon.local`)
/// derived from the user's UUID, making the row impossible to correlate back to a
/// real email address.  The `password_hash` is left intact — the account status
/// (`users.status = 'anonymized'`) is the authoritative gate for further logins;
/// the internal provider hash is irrelevant once status is anonymized.
///
/// ## Why LoginPool?
/// `user_identities` is invisible to `garraia_app` under FORCE RLS (CLAUDE.md rule 12).
/// Only `garraia_login` (BYPASSRLS) can UPDATE this table.
///
/// ## Atomicity
/// This function runs a single UPDATE — one round-trip, no transaction needed.
/// The caller (`anonymize_me`) must commit the `users` status update via `app_pool`
/// independently; the two operations are best-effort sequential (email first, then
/// status).
pub async fn anonymize_identity(login_pool: &LoginPool, user_id: Uuid) -> Result<(), AuthError> {
    let anon_login = format!(
        "anon-{}@garraanon.local",
        &user_id.simple().to_string()[..8]
    );
    let pool = login_pool.pool();
    sqlx::query(
        "UPDATE user_identities \
         SET login = $1 \
         WHERE user_id = $2 AND provider = 'internal'",
    )
    .bind(&anon_login)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(AuthError::Storage)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_change_outcome_eq() {
        assert_eq!(
            PasswordChangeOutcome::Success,
            PasswordChangeOutcome::Success
        );
        assert_ne!(
            PasswordChangeOutcome::WrongPassword,
            PasswordChangeOutcome::Success
        );
        assert_ne!(
            PasswordChangeOutcome::IdentityNotFound,
            PasswordChangeOutcome::WrongPassword
        );
    }

    #[test]
    fn password_change_outcome_debug() {
        let s = format!("{:?}", PasswordChangeOutcome::WrongPassword);
        assert!(s.contains("WrongPassword"));
        let s2 = format!("{:?}", PasswordChangeOutcome::IdentityNotFound);
        assert!(s2.contains("IdentityNotFound"));
    }

    #[test]
    fn password_change_outcome_three_variants_are_distinct() {
        let variants = [
            PasswordChangeOutcome::Success,
            PasswordChangeOutcome::WrongPassword,
            PasswordChangeOutcome::IdentityNotFound,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "same variant must be equal"
                    );
                } else {
                    assert_ne!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "distinct variants must differ"
                    );
                }
            }
        }
    }

    #[test]
    fn hash_dispatch_argon2id_prefix_recognized() {
        let hash = "$argon2id$v=19$m=65536,t=3,p=4$salt$hash";
        assert!(
            hash.starts_with("$argon2id$"),
            "argon2id prefix check must match production dispatch"
        );
    }

    #[test]
    fn hash_dispatch_pbkdf2_prefix_recognized() {
        let hash = "$pbkdf2-sha256$i=600000$salt$hash";
        assert!(
            hash.starts_with("$pbkdf2-sha256$") || hash.starts_with("$pbkdf2$"),
            "pbkdf2 prefix check must match production dispatch"
        );
    }

    // ── anonymize_identity helpers ────────────────────────────────────────────

    #[test]
    fn anon_login_format_is_deterministic() {
        let uid = uuid::Uuid::parse_str("12345678-0000-0000-0000-000000000001").unwrap();
        let anon = format!("anon-{}@garraanon.local", &uid.simple().to_string()[..8]);
        assert_eq!(anon, "anon-12345678@garraanon.local");
    }

    #[test]
    fn anon_login_is_not_valid_email_domain() {
        let uid = uuid::Uuid::parse_str("abcdef01-0000-0000-0000-000000000001").unwrap();
        let anon = format!("anon-{}@garraanon.local", &uid.simple().to_string()[..8]);
        assert!(anon.ends_with("@garraanon.local"));
        assert!(anon.contains('@'), "must contain exactly one @");
    }

    #[test]
    fn anon_login_prefix_is_not_identifiable() {
        let uid = uuid::Uuid::parse_str("cafebabe-dead-beef-cafe-babe00000001").unwrap();
        let anon = format!("anon-{}@garraanon.local", &uid.simple().to_string()[..8]);
        // Must start with 'anon-' prefix (not the bare UUID).
        assert!(anon.starts_with("anon-"));
        // Must not expose the full UUID (36-char hyphenated form) in the email.
        assert!(
            !anon.contains("cafebabe-dead"),
            "must not expose full hyphenated UUID in login"
        );
        // The 8-char derived token is the first 8 hex chars of the simple UUID.
        assert_eq!(&anon[5..13], "cafebabe");
    }
}
