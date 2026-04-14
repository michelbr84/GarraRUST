//! `garraia-auth` — authentication and authorization for GarraIA Group Workspace.
//!
//! ## Status: skeleton (GAR-391a)
//!
//! This crate is a SKELETON. The trait shape, types, and the [`LoginPool`]
//! newtype are real and load-bearing. The implementation bodies are stubs
//! that return [`AuthError::NotImplemented`]. Real bodies arrive in:
//!   - GAR-391b: `verify_credential`, `find_by_provider_sub`, `create_identity`,
//!     `audit_login`, dual-verify path, JWT issuance.
//!   - GAR-391c: Axum `Principal` extractor, `RequirePermission`, gateway wiring.
//!   - GAR-391d / GAR-392: cross-group authz test suite (100+ scenarios).
//!
//! ## Decision record
//!
//! See [`docs/adr/0005-identity-provider.md`](../../docs/adr/0005-identity-provider.md).

pub mod error;
pub mod internal;
pub mod login_pool;
pub mod provider;
pub mod types;

pub use error::AuthError;
pub use internal::InternalProvider;
pub use login_pool::{LoginConfig, LoginPool};
pub use provider::IdentityProvider;
pub use types::{Credential, Identity, Principal, RequestCtx};

/// Convenience `Result` alias for crate APIs.
pub type Result<T> = std::result::Result<T, AuthError>;
