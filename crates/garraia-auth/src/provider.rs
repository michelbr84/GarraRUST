//! `IdentityProvider` — frozen trait shape per ADR 0005.
//!
//! Extension policy: new credential backends add a [`crate::Credential`]
//! variant + match arm in their provider implementation. Adding a new
//! trait method requires a superseding ADR.

use async_trait::async_trait;
use uuid::Uuid;

use crate::types::{Credential, Identity};
use crate::Result;

/// `IdentityProvider` is the trait every credential backend implements.
/// The shape is FROZEN by ADR 0005 — extensions come via new variants
/// of [`Credential`], not new trait methods.
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Provider id — `'internal'`, `'oidc'`, `'saml'`, etc. Used for the
    /// `user_identities.provider` column.
    fn id(&self) -> &str;

    /// Look up an identity by `(provider, provider_sub)`. Used post-OIDC
    /// callback and by the session refresh path (future 391c).
    async fn find_by_provider_sub(&self, sub: &str) -> Result<Option<Identity>>;

    /// Verify a credential. Returns `Some(user_id)` on success, `None` on
    /// invalid credentials. Errors propagate storage or config failures.
    ///
    /// For `Credential::Internal`: PBKDF2 / Argon2id verify with lazy
    /// upgrade in the same transaction. Real implementation in GAR-391b.
    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>>;

    /// Create a new identity for an existing user (post-signup flow).
    async fn create_identity(&self, user_id: Uuid, credential: &Credential) -> Result<()>;
}
