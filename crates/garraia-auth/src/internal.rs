//! `InternalProvider` — verifies email+password credentials against the
//! `user_identities` table for `provider = 'internal'` rows.
//!
//! **Skeleton (GAR-391a):** all methods return [`AuthError::NotImplemented`].
//! Real bodies arrive in GAR-391b per ADR 0005 §"InternalProvider implementation
//! outline" (Argon2id verify, PBKDF2 dual-verify with lazy upgrade,
//! `SELECT ... FOR NO KEY UPDATE`, account status check, audit_events insertion).

use async_trait::async_trait;
use uuid::Uuid;

use crate::error::AuthError;
use crate::login_pool::LoginPool;
use crate::provider::IdentityProvider;
use crate::types::{Credential, Identity};
use crate::Result;

/// Verifies credentials against `user_identities` using the dedicated
/// `LoginPool` (BYPASSRLS) exclusively.
///
/// The `LoginPool` is held by-value so the type system guarantees that
/// every `InternalProvider` instance owns a pool that has already been
/// validated as `garraia_login` at construction time.
pub struct InternalProvider {
    /// Held for use by 391b. The `dead_code` allow goes away once
    /// `verify_credential` reads it.
    #[allow(dead_code)]
    login_pool: LoginPool,
}

impl InternalProvider {
    /// Build an `InternalProvider` from a validated [`LoginPool`]. The
    /// caller MUST have constructed the pool via
    /// [`LoginPool::from_dedicated_config`]; there is no other path.
    pub fn new(login_pool: LoginPool) -> Self {
        Self { login_pool }
    }
}

#[async_trait]
impl IdentityProvider for InternalProvider {
    fn id(&self) -> &str {
        "internal"
    }

    async fn find_by_provider_sub(&self, _sub: &str) -> Result<Option<Identity>> {
        // GAR-391b: SELECT id, user_id FROM user_identities
        //          WHERE provider = 'internal' AND provider_sub = $1
        //          via login_pool (BYPASSRLS).
        Err(AuthError::NotImplemented)
    }

    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>> {
        // 391a guard: only Internal variant is supported by this provider.
        // The match is exhaustive today; new variants must be handled
        // explicitly (no wildcard arm) so the compiler enforces coverage.
        match credential {
            Credential::Internal { .. } => Err(AuthError::NotImplemented),
        }
    }

    async fn create_identity(&self, _user_id: Uuid, _credential: &Credential) -> Result<()> {
        Err(AuthError::NotImplemented)
    }
}
