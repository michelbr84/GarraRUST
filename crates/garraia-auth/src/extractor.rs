//! Axum extractors: [`Principal`] and [`RequirePermission`].
//!
//! Axum 0.8 uses native AFIT for `FromRequestParts` — no `#[async_trait]`.
//!
//! Wiring requirements: the application state `S` must expose
//! `Arc<JwtIssuer>` and `Arc<LoginPool>` via `FromRef`. The gateway wires
//! these via its `AppState` in 391c-impl-B.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::{StatusCode, request::Parts};
use uuid::Uuid;

use crate::action::Action;
use crate::can::can;
use crate::jwt::{JwtIssuer, extract_bearer_token};
use crate::login_pool::LoginPool;
use crate::role::Role;
use crate::types::Principal;

/// `X-Group-Id` request header — carries the active group UUID the caller
/// wants to operate under. Optional: its absence means "no group context".
const X_GROUP_ID: &str = "x-group-id";

/// Rejection helper: unauthenticated.
fn unauth() -> (StatusCode, &'static str) {
    (StatusCode::UNAUTHORIZED, "unauthenticated")
}

/// Rejection helper: forbidden.
fn forbid() -> (StatusCode, &'static str) {
    (StatusCode::FORBIDDEN, "forbidden")
}

impl<S> FromRequestParts<S> for Principal
where
    S: Send + Sync,
    Arc<JwtIssuer>: FromRef<S>,
    Arc<LoginPool>: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Bearer token.
        let token = extract_bearer_token(&parts.headers).ok_or_else(unauth)?;

        // 2. Decode + verify JWT.
        let issuer: Arc<JwtIssuer> = Arc::<JwtIssuer>::from_ref(state);
        let claims = issuer.verify_access(token).map_err(|_| unauth())?;

        // 3. Optional X-Group-Id.
        let group_header = parts.headers.get(X_GROUP_ID);
        let Some(header_val) = group_header else {
            return Ok(Principal {
                user_id: claims.sub,
                group_id: None,
                role: None,
            });
        };

        let header_str = header_val
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid X-Group-Id"))?;
        let group_id = Uuid::parse_str(header_str)
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid X-Group-Id"))?;

        // 4. Membership lookup against `group_members`.
        // Security review M-1 (391c): log storage errors via Display so
        // operators can distinguish "DB unavailable" from "invalid token"
        // in production logs. The mapped status remains 401 to preserve
        // the anti-enumeration contract — clients see one shape regardless.
        let pool: Arc<LoginPool> = Arc::<LoginPool>::from_ref(state);
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT role::text FROM group_members \
             WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(group_id)
        .bind(claims.sub)
        .fetch_optional(pool.pool())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "group_members membership lookup failed");
            unauth()
        })?;

        let Some((role_str,)) = row else {
            return Err(forbid());
        };
        let role = Role::from_str(&role_str).ok_or_else(forbid)?;

        Ok(Principal {
            user_id: claims.sub,
            group_id: Some(group_id),
            role: Some(role),
        })
    }
}

/// Extractor that requires a specific [`Action`] capability. Usage:
///
/// ```ignore
/// async fn handler(RequirePermission(_): RequirePermission<{ Action::FilesWrite as u8 }>) {}
/// ```
///
/// Because const generics over enums are unstable, we instead construct the
/// extractor via tuple-struct field at route-build time by wrapping the
/// action in a newtype and using `axum::middleware::from_fn_with_state`. The
/// simplest working shape is a plain extractor wrapping `Principal` that is
/// checked in the handler, BUT for ergonomics we also expose a
/// `require_permission` helper below.
///
/// This type exists so callers can check `RequirePermission::check(&p, a)`
/// as a guard inside handlers without repeating the `can` import.
pub struct RequirePermission(pub Action);

impl RequirePermission {
    /// Returns `Ok(())` if `principal` has `action`, else a 403 rejection.
    pub fn check(principal: &Principal, action: Action) -> Result<(), (StatusCode, &'static str)> {
        if can(principal, action) {
            Ok(())
        } else {
            Err(forbid())
        }
    }
}

/// Free function form of the check — convenient when the handler already
/// has a `Principal` in scope and just needs a one-liner guard.
pub fn require_permission(
    principal: &Principal,
    action: Action,
) -> Result<(), (StatusCode, &'static str)> {
    RequirePermission::check(principal, action)
}

// Unit tests for `unauth`, `forbid`, `RequirePermission::check`, and
// `require_permission`. These tests must NOT use testcontainers so they run
// under `cargo mutants --package garraia-auth` (which omits
// `--features test-support` and therefore skips all testcontainer-backed
// integration test binaries). GAR-774 / Q6.11.
#[cfg(test)]
mod tests {
    use super::*;

    fn owner_principal() -> Principal {
        Principal {
            user_id: Uuid::nil(),
            group_id: Some(Uuid::from_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            ])),
            role: Some(Role::Owner),
        }
    }

    fn child_principal() -> Principal {
        Principal {
            user_id: Uuid::nil(),
            group_id: Some(Uuid::from_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
            ])),
            role: Some(Role::Child),
        }
    }

    #[test]
    fn unauth_returns_unauthorized_status() {
        let (status, msg) = unauth();
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "unauth() must return 401 — mutation `(Default::default(), \"\")` produces 200"
        );
        assert!(
            !msg.is_empty(),
            "unauth() rejection message must not be empty"
        );
    }

    #[test]
    fn forbid_returns_forbidden_status() {
        let (status, msg) = forbid();
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "forbid() must return 403 — mutation `(Default::default(), \"\")` produces 200"
        );
        assert!(
            !msg.is_empty(),
            "forbid() rejection message must not be empty"
        );
    }

    #[test]
    fn require_permission_check_denies_insufficient_role() {
        // Child-role principal must NOT be able to delete the group.
        // Mutation `RequirePermission::check → Ok(())` would make this
        // return Ok and panic on `.expect_err()`.
        let err = RequirePermission::check(&child_principal(), Action::GroupDelete)
            .expect_err("Child must not have GroupDelete permission");
        assert_eq!(
            err.0,
            StatusCode::FORBIDDEN,
            "permission denial must return 403"
        );
    }

    #[test]
    fn require_permission_check_allows_sufficient_role() {
        // Owner must be able to delete the group; this positive path ensures
        // we don't accidentally reject valid principals.
        assert!(
            RequirePermission::check(&owner_principal(), Action::GroupDelete).is_ok(),
            "Owner must have GroupDelete permission"
        );
    }

    #[test]
    fn require_permission_free_fn_denies_insufficient_role() {
        // Exercises the free-function wrapper. Mutation `require_permission → Ok(())`
        // would make this return Ok and panic on `.expect_err()`.
        let err = require_permission(&child_principal(), Action::FilesWrite)
            .expect_err("Child must not be able to write files");
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }
}
