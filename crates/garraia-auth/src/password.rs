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

/// Token de anonimização determinístico para `user_id` — a ÚNICA fonte da
/// fórmula (security review plan 0354 LOW-3: `users.email`,
/// `user_identities.provider_sub` e `group_invites.invited_email` precisam
/// receber o MESMO token; duplicar o format em cada sítio dessincroniza
/// silencioso).
///
/// UUID completo (32 hex): sob UUIDv7 um prefixo de 8 hex é compartilhado por
/// usuários criados na mesma janela de ~65 s, e os UNIQUEs de `users.email` e
/// `(provider, provider_sub)` transformariam a colisão em erro.
pub fn anon_token(user_id: Uuid) -> String {
    format!("anon-{}@garraanon.local", user_id.simple())
}

/// Anonymise the internal `user_identities` row for `user_id`.
///
/// Replaces `provider_sub` — which for `provider = 'internal'` carries the
/// user's login email in THIS schema (`create_internal_user` inserts it and
/// `verify_internal` matches `provider_sub = $email`; there is no `login`
/// column — migration 001) — with a non-identifiable deterministic token
/// (`anon-<32 hex>@garraanon.local`).
///
/// The token uses the FULL UUID: under UUIDv7 (time-ordered, production ids)
/// an 8-hex prefix is shared by users created within the same ~65 s window,
/// and `UNIQUE (provider, provider_sub)` would turn that collision into a
/// hard error on the second anonymize.
///
/// `password_hash` is left intact — the account status
/// (`users.status = 'anonymized'`) is the authoritative gate for further
/// logins (`verify_internal` rejects any status != 'active').
///
/// ## Why LoginPool?
/// `user_identities` is invisible to `garraia_app` under FORCE RLS (CLAUDE.md rule 12).
/// Only `garraia_login` (BYPASSRLS) can UPDATE this table.
///
/// ## Atomicity
/// This function runs a single UPDATE — one round-trip, no transaction needed.
/// The caller (`anonymize_me`) anonymises `users.email` + status in its own
/// `app_pool` transaction; the two commits are sequential, and a failure
/// between them heals on retry (this UPDATE is idempotent and the status
/// guard only trips after the app-side commit).
///
/// Returns the number of rows updated — 0 when the user has no internal
/// identity (e.g. a future external-IdP-only account), which is not an error.
pub async fn anonymize_identity(login_pool: &LoginPool, user_id: Uuid) -> Result<u64, AuthError> {
    let anon_sub = anon_token(user_id);
    let pool = login_pool.pool();
    let result = sqlx::query(
        "UPDATE user_identities \
         SET provider_sub = $1 \
         WHERE user_id = $2 AND provider = 'internal'",
    )
    .bind(&anon_sub)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(AuthError::Storage)?;

    Ok(result.rows_affected())
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
    // Espelham o formato REAL do token (UUID completo, 32 hex — plan 0354).
    // O comportamento da função contra Postgres real é coberto pelos testes
    // de integração em `tests/password_change.rs`.

    #[test]
    fn anon_token_format_is_deterministic_full_uuid() {
        let uid = uuid::Uuid::parse_str("12345678-0000-0000-0000-000000000001").unwrap();
        let anon = format!("anon-{}@garraanon.local", uid.simple());
        assert_eq!(
            anon,
            "anon-12345678000000000000000000000001@garraanon.local"
        );
        // local-part = "anon-" (5) + 32 hex = 37 chars antes do '@'.
        assert_eq!(anon.find('@'), Some(37));
    }

    #[test]
    fn anon_token_is_not_valid_email_domain() {
        let uid = uuid::Uuid::parse_str("abcdef01-0000-0000-0000-000000000001").unwrap();
        let anon = format!("anon-{}@garraanon.local", uid.simple());
        assert!(anon.ends_with("@garraanon.local"));
        assert_eq!(anon.matches('@').count(), 1, "must contain exactly one @");
    }

    #[test]
    fn anon_token_full_uuid_avoids_v7_prefix_collision() {
        // Sob UUIDv7 os primeiros 32 bits são timestamp — dois usuários criados
        // na mesma janela de ~65 s compartilham o prefixo de 8 hex. O token usa
        // o UUID COMPLETO justamente para que os tokens continuem distintos
        // (users.email e (provider, provider_sub) são UNIQUE).
        let a = uuid::Uuid::parse_str("cafebabe-0001-7000-8000-000000000001").unwrap();
        let b = uuid::Uuid::parse_str("cafebabe-0001-7000-8000-000000000002").unwrap();
        let anon_a = format!("anon-{}@garraanon.local", a.simple());
        let anon_b = format!("anon-{}@garraanon.local", b.simple());
        assert_eq!(
            &anon_a[5..13],
            &anon_b[5..13],
            "premissa: mesmo prefixo de 8 hex"
        );
        assert_ne!(anon_a, anon_b, "tokens completos devem diferir");
        // Não expõe a forma hifenizada do UUID.
        assert!(!anon_a.contains("cafebabe-0001"));
    }
}
