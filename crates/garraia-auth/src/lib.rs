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

pub mod audit;
pub mod audit_workspace;
pub mod error;
pub mod hashing;
pub mod internal;
pub mod jwt;
pub mod login_pool;
pub mod provider;
pub mod sessions;
pub mod types;

pub use audit::{AuditAction, audit_login};
pub use audit_workspace::{WorkspaceAuditAction, audit_workspace_event};
pub use error::AuthError;
pub use hashing::{hash_argon2id, verify_argon2id, verify_pbkdf2};
pub use internal::InternalProvider;
pub use jwt::{AccessClaims, JwtConfig, JwtIssuer, RefreshTokenPair};
pub use login_pool::{LoginConfig, LoginPool};
pub use provider::IdentityProvider;
pub use sessions::{SessionId, SessionStore};
pub use types::{Credential, Identity, LoginOutcome, Principal, RequestCtx};

// 391c-impl-A — Role/Action/can/extractor
pub mod action;
pub mod can;
pub mod extractor;
pub mod role;
pub use action::Action;
pub use can::can;
pub use extractor::{RequirePermission, require_permission};
pub use role::Role;

// 391c-impl-B — SignupPool/signup_user/RedactedStorageError
pub mod signup_pool;
pub mod storage_redacted;
pub use internal::signup_user;
pub use signup_pool::{SignupConfig, SignupPool};
pub use storage_redacted::RedactedStorageError;

// 0016-M1 — AppPool (garraia_app RLS-enforced pool for /v1 handlers)
pub mod app_pool;
pub use app_pool::{AppPool, AppPoolConfig};

/// Convenience `Result` alias for crate APIs.
pub type Result<T> = std::result::Result<T, AuthError>;
