//! `SessionStore` — issue / verify / revoke refresh sessions.
//!
//! Maps to the `sessions` table from migration 001:
//!
//! ```text
//! sessions:
//!   id                  uuid    PK
//!   user_id             uuid    NOT NULL FK users.id ON DELETE CASCADE
//!   refresh_token_hash  text    NOT NULL UNIQUE   -- HMAC-SHA256 hex
//!   device_id           text    NULL
//!   expires_at          timestamptz NOT NULL
//!   revoked_at          timestamptz NULL
//!   created_at          timestamptz NOT NULL DEFAULT now()
//! ```
//!
//! All access goes through the dedicated `garraia_login` BYPASSRLS pool
//! ([`crate::login_pool::LoginPool`]). The login pool was granted
//! `INSERT, UPDATE ON sessions` by migration 008 — no extra grants needed.
//!
//! `verify_refresh` does the constant-time compare against
//! `refresh_token_hash` via `subtle::ConstantTimeEq` to deny timing-based
//! enumeration of valid hashes.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sqlx::Row;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::AuthError;
use crate::jwt::JwtIssuer;
use crate::login_pool::LoginPool;

/// Strongly-typed session id wrapper. Future code that takes `SessionId`
/// instead of `Uuid` is more self-documenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub Uuid);

/// Default refresh-token TTL (30 days). Matches the existing mobile client
/// expectation in `garraia-gateway::mobile_auth`.
const REFRESH_TTL_DAYS: i64 = 30;

pub struct SessionStore {
    login_pool: Arc<LoginPool>,
}

impl SessionStore {
    /// Build a `SessionStore` from a validated [`LoginPool`] wrapped in
    /// `Arc`. The same `Arc<LoginPool>` is typically shared with
    /// [`crate::internal::InternalProvider`] so the login flow runs over
    /// a single bounded pool footprint.
    pub fn new(login_pool: Arc<LoginPool>) -> Self {
        Self { login_pool }
    }

    /// Insert a new session row for `user_id`, returning the generated
    /// session id and the absolute expiry timestamp.
    ///
    /// `refresh_hmac` is the HMAC-SHA256 hex of the refresh token plaintext,
    /// produced by [`JwtIssuer::issue_refresh`]. The plaintext itself is
    /// NEVER stored — it leaves the gateway exactly once in the login
    /// response and the client must keep it.
    ///
    /// **⚠️ NOT WIRED IN GAR-391b.** Calling this method against a
    /// `LoginPool` connected as `garraia_login` will fail at runtime with
    /// `permission denied for table sessions` because the role granted by
    /// migration 008 has `INSERT, UPDATE` but **not `SELECT`**, and
    /// `INSERT ... RETURNING id` requires SELECT on the returned column.
    /// GAR-391c will ship migration 010 adding `GRANT SELECT ON sessions`
    /// alongside the refresh endpoint that needs it. Until then this
    /// method exists only to be exercised by 391c integration tests.
    /// See plan 0011 amendment §"Segunda correção de escopo".
    pub async fn issue(
        &self,
        user_id: Uuid,
        refresh_hmac: &str,
        device_id: Option<&str>,
    ) -> Result<(SessionId, DateTime<Utc>), AuthError> {
        let now = Utc::now();
        let expires_at = now + Duration::days(REFRESH_TTL_DAYS);
        let row = sqlx::query(
            "INSERT INTO sessions (user_id, refresh_token_hash, device_id, expires_at) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(refresh_hmac)
        .bind(device_id)
        .bind(expires_at)
        .fetch_one(self.login_pool.pool())
        .await
        .map_err(AuthError::Storage)?;
        let id: Uuid = row.try_get("id").map_err(AuthError::Storage)?;
        Ok((SessionId(id), expires_at))
    }

    /// Verify a refresh-token plaintext against the stored hash.
    ///
    /// Looks up the session by re-computing the HMAC and matching against
    /// `refresh_token_hash`. The compare uses `subtle::ConstantTimeEq` as
    /// defense-in-depth against future refactors that switch to a
    /// sequential scan or non-indexed lookup. The current B-tree equality
    /// match on `refresh_token_hash` (UNIQUE) does the real timing
    /// protection — the `ct_eq` is belt-and-suspenders, not the primary
    /// guard. Security review 391b M-2.
    ///
    /// **⚠️ NOT WIRED IN GAR-391b.** Same deferral as `issue` — needs
    /// `SELECT ON sessions` granted to `garraia_login` via migration 010.
    pub async fn verify_refresh(
        &self,
        plaintext: &str,
        issuer: &JwtIssuer,
    ) -> Result<Option<(SessionId, Uuid)>, AuthError> {
        let computed = issuer.hmac_refresh(plaintext)?;
        let row = sqlx::query(
            "SELECT id, user_id, refresh_token_hash, expires_at, revoked_at \
             FROM sessions \
             WHERE refresh_token_hash = $1",
        )
        .bind(&computed)
        .fetch_optional(self.login_pool.pool())
        .await
        .map_err(AuthError::Storage)?;

        let Some(row) = row else {
            return Ok(None);
        };
        let stored_hash: String = row
            .try_get("refresh_token_hash")
            .map_err(AuthError::Storage)?;
        if computed
            .as_bytes()
            .ct_eq(stored_hash.as_bytes())
            .unwrap_u8()
            == 0
        {
            return Ok(None);
        }

        let revoked_at: Option<DateTime<Utc>> =
            row.try_get("revoked_at").map_err(AuthError::Storage)?;
        if revoked_at.is_some() {
            return Ok(None);
        }
        let expires_at: DateTime<Utc> = row.try_get("expires_at").map_err(AuthError::Storage)?;
        if expires_at <= Utc::now() {
            return Ok(None);
        }

        let id: Uuid = row.try_get("id").map_err(AuthError::Storage)?;
        let user_id: Uuid = row.try_get("user_id").map_err(AuthError::Storage)?;
        Ok(Some((SessionId(id), user_id)))
    }

    /// Mark a session as revoked. Idempotent — re-revoking is a no-op.
    pub async fn revoke(&self, id: SessionId) -> Result<(), AuthError> {
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
            .bind(id.0)
            .execute(self.login_pool.pool())
            .await
            .map_err(AuthError::Storage)?;
        Ok(())
    }
}
