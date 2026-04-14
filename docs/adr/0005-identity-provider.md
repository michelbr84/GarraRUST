# 5. Identity Provider — login flow under Row-Level Security

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (review: `@security-auditor`)
- **Date:** 2026-04-13
- **Tags:** fase-3, security, ws-authz, gar-375
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-375](https://linear.app/chatgpt25/issue/GAR-375)
  - Plan: [`plans/0009-gar-375-adr-0005-identity-provider.md`](../../plans/0009-gar-375-adr-0005-identity-provider.md)
  - Implementation issue: [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) (`garraia-auth` crate, this ADR's direct consumer)
  - Migration tool: [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) (SQLite → Postgres credential import)
  - Hard blocker source: [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) (RLS migration that documented this gap)
  - Related decision: [`docs/adr/0003-database-for-workspace.md`](0003-database-for-workspace.md)
  - Schema target: [`crates/garraia-workspace/migrations/001_initial_users_groups.sql`](../../crates/garraia-workspace/migrations/001_initial_users_groups.sql) (`user_identities` table)
  - RLS policy: [`crates/garraia-workspace/migrations/007_row_level_security.sql`](../../crates/garraia-workspace/migrations/007_row_level_security.sql) (`user_identities_owner_only`)
  - Pre-merge contract: [`crates/garraia-workspace/README.md`](../../crates/garraia-workspace/README.md) §"⚠️ HARD BLOCKER for GAR-391"

---

## Context and Problem Statement

Migration 007 ([GAR-408](https://linear.app/chatgpt25/issue/GAR-408)) put `user_identities` under `FORCE ROW LEVEL SECURITY` with policy:

```sql
CREATE POLICY user_identities_owner_only ON user_identities
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);
```

The policy works correctly for the steady-state case: an authenticated user can only read their own identity records. But it creates a **chicken-and-egg problem at login time**:

> The login flow needs to read `password_hash` to verify the credential.
> But to read `password_hash`, RLS requires `app.current_user_id` to be set.
> But `app.current_user_id` is exactly what the login flow is trying to determine.

If the application pool role (`garraia_app`) tries to `SELECT password_hash FROM user_identities WHERE provider = 'internal' AND ...`, RLS filters every row and returns an empty set. **Treating that empty set as "user not found" is an anti-pattern** — it means "RLS blocked the read", which is semantically distinct from "no such user exists".

The security review of GAR-408 explicitly named this as a **hard blocker for GAR-391 production rollout**. The `crates/garraia-workspace/README.md` documents it under "⚠️ HARD BLOCKER for GAR-391". This ADR resolves the blocker.

In addition to the login flow problem, the same decision touches three adjacent concerns that should be answered together to avoid drift:

1. **JWT signature algorithm.** `garraia-gateway/src/mobile_auth.rs` (GAR-335) currently uses HS256 with `GARRAIA_JWT_SECRET`. As we move from `mobile_users` (SQLite) to `user_identities` (Postgres), do we keep HS256 or upgrade to RS256/EdDSA?
2. **Mobile users migration.** `mobile_users` table holds existing PBKDF2 credentials. They must move to `user_identities` without invalidating live accounts.
3. **PBKDF2 → Argon2id transition.** Rust ecosystem best practice (RFC 9106) is Argon2id. The current `mobile_auth.rs` uses PBKDF2_HMAC_SHA256 (600k iterations via `ring`). We need a path that doesn't break existing logins.

Closing all four together prevents the re-litigation that happens when each is decided incrementally.

---

## Decision Drivers

Ranked by weight:

1. ★★★★★ **Defense-in-depth** — the login surface is the primary attack vector; it must be the smallest possible attack surface
2. ★★★★★ **LGPD/GDPR compliance** — `password_hash` is the most sensitive piece of personal data in the schema
3. ★★★★ **Operational simplicity** — fewer moving parts means fewer human-error vectors
4. ★★★★ **Audit trail** — every login attempt (success or failure) must produce an `audit_events` row
5. ★★★ **Backward compatibility** — existing `mobile_users` PBKDF2 hashes must keep working without forcing a password reset
6. ★★★ **Future OIDC pluggability** — keep the door open for Keycloak/Auth0/Authelia/Google adapters without rewriting
7. ★★ **Performance** — login p95 should remain under 200 ms including credential verification
8. ★★ **Rollback safety** — if the chosen pattern proves insufficient, how expensive is a reversal?

---

## Considered Options

### Login flow access pattern (4 options)

#### A) BYPASSRLS dedicated role (`garraia_login`) ★ recommended

A dedicated PostgreSQL role with the `BYPASSRLS` attribute, used **exclusively** by the login endpoint via a separate connection pool.

```sql
CREATE ROLE garraia_login NOLOGIN BYPASSRLS;
GRANT SELECT, UPDATE ON user_identities TO garraia_login;
GRANT SELECT ON users TO garraia_login;
GRANT INSERT, UPDATE ON sessions TO garraia_login;
GRANT INSERT ON audit_events TO garraia_login;
```

The login pool is bound to this role at construction time. The `garraia-auth` crate exposes a `LoginPool` newtype that is **not constructable from a raw `PgPool`** — only via a specific config path that injects the dedicated role credentials. This makes "accidentally use the login pool for non-login work" a compile error, not a runtime hazard.

Argon2id verification happens in Rust application code (using the `argon2` crate), not in the database. The login pool reads `password_hash` once, the function verifies, the result is a `user_id` or an error.

**Pros:**
- Simple to reason about — one role boundary, one extra pool
- Crypto stays in Rust (single Argon2id implementation, no `pgcrypto` dependency for password verification)
- Surface audit is clear: every read of `password_hash` is via the login pool, instrumented by `garraia-telemetry` (with `password_hash` redacted from spans)
- Rollback trivial: `DROP ROLE garraia_login` + revert the GRANTs
- Argon2 parameters and version live in one place (`Cargo.toml` + `garraia-auth/src/password.rs`)
- Operationally transparent — role attributes visible in `pg_roles`

**Cons:**
- If the login pool credentials leak, `user_identities` is fully exposed across all users (mitigation: network isolation, distinct credential vault entry, rotation policy in GAR-410, `pgaudit` logging)
- Two pools to manage in production deployments

#### B) `SECURITY DEFINER` function (`verify_credential`)

A `SECURITY DEFINER` function owned by a privileged role, callable by the application pool. The function reads `user_identities` internally and returns a verification result without exposing the hash.

```sql
CREATE OR REPLACE FUNCTION verify_credential(p_email citext, p_password text)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_user_id uuid;
    v_hash text;
BEGIN
    SELECT u.id, ui.password_hash
      INTO v_user_id, v_hash
      FROM users u
      JOIN user_identities ui ON ui.user_id = u.id
     WHERE u.email = p_email AND ui.provider = 'internal';

    IF v_user_id IS NULL THEN
        RETURN NULL;
    END IF;

    -- Verification via pgcrypto.crypt() — requires Argon2 extension,
    -- which is NOT in stock pgcrypto. Would force pg_argon2 or similar.
    IF crypt(p_password, v_hash) = v_hash THEN
        RETURN v_user_id;
    END IF;

    RETURN NULL;
END;
$$;
REVOKE ALL ON FUNCTION verify_credential FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verify_credential TO garraia_app;
```

**Pros:**
- Narrowest possible surface — only this single function can read the hash
- The application pool never sees `password_hash` even indirectly
- Function ownership is auditable (single grep target in `pg_proc`)

**Cons:**
- **Forces credential verification into the database**, which means either (a) `pgcrypto.crypt()` with bcrypt (not Argon2, weaker) or (b) installing a third-party extension like `pg_argon2` (operational burden, supply-chain concern)
- Loses the unified Rust crypto stack — Argon2 parameters and the verification path live in PL/pgSQL instead of `argon2` crate
- The function can still be called by anyone with `EXECUTE` grant — surface is "narrow" but not "tight" without additional rate limiting
- Lazy upgrade PBKDF2 → Argon2id becomes harder (UPDATE inside the function vs in Rust)
- Harder to instrument with `garraia-telemetry` (DB-side execution, fewer Rust-level spans)

#### C) Hybrid: `SECURITY DEFINER` returns the hash, Rust verifies

```sql
CREATE OR REPLACE FUNCTION fetch_credential_for_verification(p_email citext)
RETURNS TABLE (user_id uuid, password_hash text, status text)
LANGUAGE plpgsql
SECURITY DEFINER
AS $$ ... $$;
```

The function reads the hash and returns it to the application. Rust verifies with Argon2.

**Pros:**
- Keeps crypto in Rust (best of A's strengths)
- Slightly narrower surface than A (only this function reads the hash, app pool can't `SELECT` directly)

**Cons:**
- The hash still leaves the database boundary — security gain over option A is **marginal**, since the application pool process now holds the hash in memory anyway
- More complex than A (function + grants + Rust client) without proportional benefit
- A compromised application process can still extract hashes by calling the function repeatedly

#### D) Disable RLS on `user_identities` entirely

`ALTER TABLE user_identities DISABLE ROW LEVEL SECURITY;`

**REJECTED.** Defeats the entire point of GAR-408. The table holds password hashes — it must be the *most* protected, not the least. The hard blocker exists precisely *because* the table is correctly under RLS. Disabling RLS would create a bigger compliance gap than the one we're solving.

### Decision: A (BYPASSRLS dedicated role)

**Rationale:**

1. **Crypto unification.** Argon2id has one canonical implementation in this codebase (`argon2` crate, version pinned in workspace `Cargo.toml`). Option B forces the choice between bcrypt (weaker than Argon2id) and a third-party extension (supply chain). Option A keeps everything in Rust where the security review process already covers it.

2. **Surface analysis is honest.** Option A admits "the login pool can read all hashes" and protects via network isolation + distinct credentials + rotation. Option C pretends to narrow the surface but the hash still reaches the application process. Option B is genuinely narrower but the cost is paid in crypto stack fragmentation.

3. **Rollback is trivial.** `DROP ROLE garraia_login` + revert the GRANT statements. Compare with B: undoing a `SECURITY DEFINER` function requires both `DROP FUNCTION` and rewriting the application code that called it.

4. **Audit is mechanical.** Every `garraia-auth` login call instruments a `tracing` span with `request_id` (already wired from GAR-384). The login pool is bound at startup; no other code can use it. `audit_events` rows are inserted in the same transaction as the verify SELECT — a partial commit is impossible.

5. **Operational simplicity dominates at the v1 stage.** When we have multiple production deployments, mature key management, and a security audit history, option C becomes more attractive as a hardening path. v1 starts simple.

**Hardening path:** option C remains a viable v2 upgrade. If a compliance audit demands narrower DB surface, the migration from A to C is mechanical (add the function, repoint the Rust caller, drop the BYPASSRLS role).

---

### JWT signature algorithm (3 options)

#### 1) HS256 (HMAC-SHA256, symmetric) ★ recommended v1

Same secret signs and verifies. Loaded from `GARRAIA_JWT_SECRET` env var (or `CredentialVault` after GAR-410).

**Pros:** matches existing `garraia-gateway/src/mobile_auth.rs` implementation, simple key management (1 secret), fast verification, well-supported in the Rust ecosystem (`jsonwebtoken` crate).

**Cons:** any verifier needs the signing secret; doesn't scale to multi-instance federation where verifiers may live in different security boundaries.

#### 2) RS256 (RSA-SHA256, asymmetric)

Private key signs, public key verifies. Public key distributable via JWKS endpoint.

**Pros:** scales to multi-instance, federation-friendly, public key can be widely distributed without compromising signing capability.

**Cons:** slower than HS256, more code to manage RSA keypairs, JWKS rotation is non-trivial, overkill for the single-instance v1 deployment.

#### 3) EdDSA (Ed25519)

Modern asymmetric algorithm, smaller keys than RSA, faster than RS256.

**Pros:** modern crypto best practice, smaller bandwidth, future-proof.

**Cons:** less ecosystem support than HS256/RS256, premature for current scale.

### Decision: HS256 v1

**Rationale:** matches current code in `mobile_auth.rs`, simple operational footprint for single-instance deployments (which is all we have), and the `jsonwebtoken` crate handles it cleanly. Migration to RS256 or EdDSA is deferred to **Fase 7** (multi-region/federation), at which point a new ADR will document the rotation procedure and JWKS endpoint.

**Key management:** the secret continues to live in `GARRAIA_JWT_SECRET` env var until GAR-410 (`CredentialVault` final) lands. After GAR-410, the vault is the canonical source.

---

### Password algorithm and migration (2 options)

#### X) Lazy upgrade dual-verify ★ recommended

New users get Argon2id hashes from day one. Existing PBKDF2 users get verified with PBKDF2 first, then re-hashed with Argon2id and `UPDATE`d in the same transaction on the next successful login.

After 6 months, run `SELECT count(*) FROM user_identities WHERE password_hash LIKE '$pbkdf2-sha256$%'`. Stragglers (users who haven't logged in) get a forced password reset email.

**Pros:** zero user disruption, gradual migration, no batch downtime, no need to know plaintexts.

**Cons:** dual code path lives until the straggler audit (~6 months). One more code branch in the login hot path.

#### Y) Forced batch re-hash

Big-bang migration: re-hash all PBKDF2 to Argon2id at deployment time.

**REJECTED.** Re-hashing requires the plaintext password, which we don't have. The only viable form is "force password reset for all users" — a UX disaster and a sign of operational immaturity.

### Decision: X (lazy upgrade dual-verify)

**Rationale:** the only viable option. Documented as the standard pattern in OWASP Authentication Cheat Sheet.

**Argon2id parameters** (RFC 9106 first recommendation):

- Memory: 64 MiB (`m=65536` in PHC string)
- Iterations: 3 (`t=3`)
- Parallelism: 4 (`p=4`)
- Salt: 16 random bytes per password
- Output length: 32 bytes

These yield ~50-100 ms verification on modern x86_64 hardware (e.g., the test container in `benches/database-poc/`), which keeps login p95 well under 200 ms.

**Hash format:** PHC string format. New: `$argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>`. Legacy: `$pbkdf2-sha256$i=600000$<salt>$<hash>` (or whatever PHC form `mobile_auth.rs` currently emits — to be verified during GAR-391 implementation).

---

## Decision Outcome

**Login flow:** `garraia_login` BYPASSRLS dedicated role (option A) + Argon2id verification in Rust via the `argon2` crate.

**JWT:** HS256 with `GARRAIA_JWT_SECRET`, 30-day refresh token, 15-minute access token (subject to GAR-391 implementation tuning). Migrate to RS256/EdDSA in Fase 7.

**Password algorithm:** Argon2id with RFC 9106 first recommendation parameters. PHC string format storage.

**Migration:** lazy upgrade dual-verify. PBKDF2 stragglers force-expired after 6 months via password reset email.

**`mobile_users` → `user_identities` import:** part of `garraia-cli migrate workspace` (GAR-413), runs as superuser, copies PBKDF2 hashes verbatim, sets `provider = 'internal'`, sets `provider_sub = users.id::text`, populates `users.legacy_sqlite_id` for audit traceability.

### `IdentityProvider` trait shape

The `garraia-auth` crate (GAR-391) will expose this trait. The shape is **frozen** by this ADR — concrete adapters (`OidcProvider`, `SamlProvider`) extend without modifying the trait surface.

```rust
use async_trait::async_trait;
use uuid::Uuid;

/// Identifies an authenticated user across providers.
#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: Uuid,
    pub provider: String,    // 'internal' | 'oidc' | 'saml'
    pub provider_sub: String, // stable subject identifier
}

/// A credential being verified. Variant determines which provider handles it.
#[derive(Debug)]
pub enum Credential {
    Internal { email: String, password: String },
    OidcIdToken { token: String, issuer: String },
    // SamlAssertion { ... } in the future
}

/// Errors a provider may return.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("unsupported credential variant for provider {0}")]
    UnsupportedCredential(String),
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("storage error: {0}")]
    Storage(#[source] sqlx::Error),
    #[error("hash format unrecognized")]
    UnknownHashFormat,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Provider id — 'internal', 'oidc', 'saml', etc.
    /// Used for the `user_identities.provider` column.
    fn id(&self) -> &str;

    /// Look up an identity by (provider, provider_sub).
    /// Used post-OIDC callback to find an existing user, and by the
    /// session refresh path.
    async fn find_by_provider_sub(&self, sub: &str) -> Result<Option<Identity>, AuthError>;

    /// Verify a credential and return the user_id if valid.
    /// For Internal: PBKDF2/Argon2id verify with lazy upgrade.
    /// For OIDC: validate ID token signature + claims.
    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>, AuthError>;

    /// Create a new identity for an existing user (post-signup or
    /// post-OIDC first login).
    async fn create_identity(&self, user_id: Uuid, credential: &Credential) -> Result<(), AuthError>;
}
```

### `InternalProvider` implementation outline

```rust
/// Login pool newtype. NOT constructable from `From<PgPool>` — only via
/// `LoginPool::from_dedicated_config` which loads credentials for the
/// `garraia_login` BYPASSRLS role from a separate config path. This makes
/// "accidentally use the login pool for normal queries" a compile error.
pub struct LoginPool(sqlx::PgPool);

impl LoginPool {
    pub async fn from_dedicated_config(config: &LoginConfig) -> Result<Self, AuthError> {
        // Validates that config.role == "garraia_login" before connecting
        // and refuses any other role. Returns a wrapped pool whose inner
        // PgPool is private and only accessible via methods on LoginPool.
    }

    pub(crate) fn pool(&self) -> &sqlx::PgPool { &self.0 }
}

pub struct InternalProvider {
    login_pool: LoginPool,
    argon2: argon2::Argon2<'static>,
}

#[async_trait]
impl IdentityProvider for InternalProvider {
    fn id(&self) -> &str { "internal" }

    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>, AuthError> {
        let (email, password) = match credential {
            Credential::Internal { email, password } => (email, password),
            _ => return Err(AuthError::UnsupportedCredential("internal".into())),
        };

        let mut tx = self.login_pool.pool().begin().await.map_err(AuthError::Storage)?;

        // SELECT runs under the BYPASSRLS role. 0 rows here = user truly
        // doesn't exist (NOT the empty-result-from-RLS pitfall flagged in
        // GAR-408). The CRITICAL distinction.
        //
        // FOR NO KEY UPDATE acquires a row-level lock that prevents
        // concurrent transactions from observing the OLD (PBKDF2) hash and
        // racing to upgrade it. Without this lock, two concurrent logins
        // for the same user could both see the PBKDF2 hash and both issue
        // UPDATE statements — Postgres serializes the writes, but only
        // after both reads have happened. The outcome is benign (both
        // converge to the same Argon2id value) but the audit trail
        // becomes confusing and the "no window between verify and write"
        // contract is violated. FOR NO KEY UPDATE closes the window
        // entirely. NO KEY (vs plain UPDATE) is sufficient because no FK
        // references user_identities.id in the login path, so we don't
        // need to block FK validations.
        let row: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT u.id, ui.password_hash
             FROM users u
             JOIN user_identities ui ON ui.user_id = u.id
             WHERE u.email = $1
               AND ui.provider = 'internal'
               AND u.status = 'active'
             FOR NO KEY UPDATE OF ui"
        )
        .bind(email)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AuthError::Storage)?;

        let (user_id, stored_hash) = match row {
            Some(r) => r,
            None => {
                // Constant-time defense: even when the user doesn't exist
                // (or is suspended), we run a dummy Argon2 verify so the
                // response timing is indistinguishable from a real failure.
                // The dummy hash MUST use identical parameters (m=65536,
                // t=3, p=4) as production hashes — see DUMMY_HASH below.
                let _ = verify_password(&self.argon2, DUMMY_HASH, password);
                audit_login(&mut tx, None, "login.failure_user_not_found", email, request_ctx).await?;
                tx.commit().await.map_err(AuthError::Storage)?;
                return Ok(None);
            }
        };

        let verified = verify_password(&self.argon2, &stored_hash, password)?;
        if !verified {
            audit_login(&mut tx, Some(user_id), "login.failure_bad_password", email, request_ctx).await?;
            tx.commit().await.map_err(AuthError::Storage)?;
            return Ok(None);
        }

        // Lazy upgrade: if the verified hash was PBKDF2, re-hash with
        // Argon2id and UPDATE in the same transaction. A failure here
        // does NOT block the login — the user gets in, the upgrade
        // retries on the next login.
        if stored_hash.starts_with("$pbkdf2-sha256$") {
            if let Ok(new_hash) = hash_argon2(&self.argon2, password) {
                // The row is already locked via FOR NO KEY UPDATE above,
                // so this UPDATE is race-free with respect to concurrent
                // logins for the same user.
                let _ = sqlx::query(
                    "UPDATE user_identities SET password_hash = $1 WHERE user_id = $2"
                )
                .bind(&new_hash)
                .bind(user_id)
                .execute(&mut *tx)
                .await;
                let _ = audit_login(&mut tx, Some(user_id), "login.password_hash_upgraded", email, request_ctx).await;
            }
        }

        audit_login(&mut tx, Some(user_id), "login.success", email, request_ctx).await?;
        tx.commit().await.map_err(AuthError::Storage)?;
        Ok(Some(user_id))
    }

    async fn find_by_provider_sub(&self, sub: &str) -> Result<Option<Identity>, AuthError> {
        // SELECT id, user_id FROM user_identities
        // WHERE provider = 'internal' AND provider_sub = $1
        // Runs under BYPASSRLS for the same reason as verify_credential.
        unimplemented!("see GAR-391 implementation")
    }

    async fn create_identity(&self, user_id: Uuid, credential: &Credential) -> Result<(), AuthError> {
        // INSERT INTO user_identities (user_id, provider, provider_sub, password_hash)
        // VALUES ($1, 'internal', $1::text, $2)
        // password_hash is the freshly-Argon2id-hashed credential.
        unimplemented!("see GAR-391 implementation")
    }
}

/// Dual-verify: detects PHC prefix and dispatches to the correct algorithm.
/// Returns Ok(true) on valid password, Ok(false) on invalid, Err on
/// unrecognized format.
fn verify_password(
    argon2: &argon2::Argon2<'_>,
    stored_hash: &str,
    password: &str,
) -> Result<bool, AuthError> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};

    if stored_hash.starts_with("$argon2id$") || stored_hash.starts_with("$argon2i$") {
        let parsed = PasswordHash::new(stored_hash).map_err(|_| AuthError::UnknownHashFormat)?;
        Ok(argon2.verify_password(password.as_bytes(), &parsed).is_ok())
    } else if stored_hash.starts_with("$pbkdf2-sha256$") {
        // Parse PHC PBKDF2 string and verify with the `pbkdf2` crate or
        // equivalent. See GAR-391 implementation for the exact crate.
        Ok(verify_pbkdf2_phc(stored_hash, password)?)
    } else {
        Err(AuthError::UnknownHashFormat)
    }
}

fn hash_argon2(argon2: &argon2::Argon2<'_>, password: &str) -> Result<String, AuthError> {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| AuthError::UnknownHashFormat)?
        .to_string())
}

/// Forensic context captured by the Axum extractor and passed into
/// every login attempt. Stored in audit_events.metadata jsonb so it
/// survives every login regardless of outcome.
#[derive(Debug, Clone)]
pub struct RequestCtx {
    pub ip: Option<std::net::IpAddr>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>, // x-request-id from tower-http
}

async fn audit_login(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Option<Uuid>,
    action: &str,
    email_for_label: &str,
    request_ctx: &RequestCtx,
) -> Result<(), AuthError> {
    let metadata = serde_json::json!({
        "ip": request_ctx.ip.map(|i| i.to_string()),
        "user_agent": request_ctx.user_agent,
        "request_id": request_ctx.request_id,
    });
    sqlx::query(
        "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata) \
         VALUES (NULL, $1, $2, $3, 'user_identity', COALESCE($1::text, 'unknown'), $4::jsonb)"
    )
    .bind(user_id)
    .bind(email_for_label) // label IS the email at login time — anti-enumeration handled via dummy-hash constant-time path
    .bind(action)
    .bind(&metadata)
    .execute(&mut **tx)
    .await
    .map_err(AuthError::Storage)?;
    Ok(())
}

/// Constant-time anti-enumeration dummy hash. MUST use identical Argon2id
/// parameters (m=65536, t=3, p=4) as production hashes so the verification
/// timing is indistinguishable from a real check. This is NOT a secret —
/// its purpose is timing parity, not confidentiality. Generated once at
/// crate build time via a build script (see GAR-391 implementation).
const DUMMY_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=4$<placeholder-salt>$<placeholder-hash>";
```

---

## Login role specification

This DDL ships in a future migration alongside GAR-391 (it is **not** part of this ADR). The exact form below is normative — GAR-391 implementation must use it verbatim.

```sql
-- Migration 008_login_role.sql (or higher), part of GAR-391, NOT this ADR.
-- Idempotent so re-runs are safe.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_login') THEN
        CREATE ROLE garraia_login NOLOGIN BYPASSRLS;
    END IF;
END
$$;

-- Grants minimal for the login flow:
--   1. SELECT user_identities to read password_hash for verification
--   2. UPDATE user_identities to lazy-upgrade PBKDF2 → Argon2id
--   3. SELECT users to look up by email and get display_name for audit_label
--   4. INSERT/UPDATE sessions to issue refresh tokens
--   5. INSERT audit_events to log every attempt
GRANT SELECT, UPDATE ON user_identities TO garraia_login;
GRANT SELECT ON users TO garraia_login;
GRANT INSERT, UPDATE ON sessions TO garraia_login;
GRANT INSERT ON audit_events TO garraia_login;

-- Login role explicitly lacks:
--   - Read access to messages, files, memory, tasks, etc. (no GRANT)
--   - Write access to anything outside auth-related tables
--   - Capability to ALTER schema or create roles

COMMENT ON ROLE garraia_login IS 'BYPASSRLS role used EXCLUSIVELY by the garraia-auth login flow. NOLOGIN — credentials must be set externally and used only by the LoginPool. Compromise = full credential store exposure. Mitigation: network isolation + distinct vault entry + rotation per GAR-410.';
```

**Production deployment requirements:**

1. The `garraia_login` role's password is set via `ALTER ROLE garraia_login WITH LOGIN PASSWORD '...'` on each deployment, sourced from the operator's secret store (NOT the `GARRAIA_JWT_SECRET` env var; a separate `GARRAIA_LOGIN_DB_PASSWORD` or vault entry).
2. The login pool's connection string must use `garraia_login` credentials and **must not** be reused by the main app pool. The pool must be configured with a small `max_connections` (recommended: 5-10) to bound the BYPASSRLS connection footprint.
3. The login pool **MUST** be network-isolated from the main app pool in production (separate Unix socket path, separate `pg_hba.conf` entry, separate firewall rule, or a combination). "When possible" is not acceptable — this is a hard production requirement. Dev/test environments may share host but production deployments must enforce isolation at the network layer.
4. Rotation policy is documented in [GAR-410](https://linear.app/chatgpt25/issue/GAR-410) (`CredentialVault` final).
5. `pgaudit` logging on `user_identities` reads is recommended for compliance audit trails (operational, not enforced by this ADR).

---

## Amendment 2026-04-13: GAR-391c additions

**Author:** Claude Opus 4.6 + @michelbr84
**Migration:** `010_signup_role_and_session_select.sql`
**Cross-references:** plans `plans/0012-gar-391c-extractor-and-wiring.md` (with its Wave 1.5 amendment banner); plan `plans/0011-gar-391b-verify-credential-impl.md` amendment (Gap A origin).

The normative DDL in §"Login role specification" above documented 4 grants for `garraia_login` and no signup role. Three structural gaps surfaced empirically during GAR-391b and GAR-391c implementation, requiring migration 010 to extend the grant set. **This amendment is normative**; the §"Login role specification" DDL block is superseded by migration 010 for the additional grants.

### Gap A — `garraia_login` gains `SELECT ON sessions`

PostgreSQL requires `SELECT` privilege on the columns named in an `INSERT ... RETURNING` clause. The login flow's `SessionStore::issue` (which does `INSERT INTO sessions ... RETURNING id`) and `SessionStore::verify_refresh` (which does `SELECT ... WHERE refresh_token_hash = $1`) both run under the `garraia_login` pool. Without this grant, both operations failed at runtime with "permission denied for table sessions". Discovered during GAR-391b, deferred to GAR-391c for resolution via migration 010.

New grant: `GRANT SELECT ON sessions TO garraia_login;`

This grant is consistent with the login role's purpose: it reads session data only as part of the login/refresh flow, not tenant content.

### Gap B — new `garraia_signup` role

The login role's defining constraint — minimal credential-verification scope — makes it unsuitable for signup. Signup requires `INSERT ON users` and `INSERT ON user_identities`, grants that would expand `garraia_login`'s blast radius to arbitrary identity creation. A dedicated `garraia_signup NOLOGIN BYPASSRLS` role was created with its own minimal grant set:

```sql
GRANT USAGE ON SCHEMA public TO garraia_signup;
GRANT SELECT, INSERT ON users TO garraia_signup;
GRANT SELECT, INSERT ON user_identities TO garraia_signup;
GRANT INSERT ON audit_events TO garraia_signup;
```

This role is accessed exclusively via the `garraia-auth::SignupPool` newtype, enforcing the same compile-time boundary as `LoginPool` (private inner `PgPool`, runtime `current_user='garraia_signup'` validation, `!Clone` enforced via `static_assertions`). Compromise of `garraia_signup` = ability to create arbitrary identities (less critical than `garraia_login` but still a tenant-onboarding attack vector). Threat model and mitigations documented in the migration 010 comment block. Rate limiting on the `/v1/auth/signup` endpoint is deferred to a 391c follow-up.

### Gap C — `garraia_login` gains `SELECT ON group_members`

The `garraia-auth` `Principal` extractor (GAR-391c) resolves the caller's group role via `SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2 AND status = 'active'`, using the `LoginPool` (BYPASSRLS). Without this grant, the query failed with "permission denied for table group_members", causing all group-scoped requests to return 401 instead of 403 (verified empirically by the `non_member_group_returns_403` test failure). `group_members` is **not** under RLS — migration 007 §scope explicitly excludes it ("recursive RLS is expensive, app-layer enforced via JOIN with the membership query above") — so this grant does not bypass any tenant isolation that wasn't already app-layer.

New grant: `GRANT SELECT ON group_members TO garraia_login;`

This grant is consistent with the login role's "resolve who you are" purpose and was approved via Option 1 (2026-04-13).

### Updated grant set for `garraia_login` (effective post-migration 010)

```sql
GRANT USAGE ON SCHEMA public TO garraia_login;
GRANT SELECT, UPDATE ON user_identities TO garraia_login;
GRANT SELECT ON users TO garraia_login;
GRANT INSERT, UPDATE ON sessions TO garraia_login;
GRANT SELECT ON sessions TO garraia_login;          -- Gap A (391c)
GRANT INSERT ON audit_events TO garraia_login;
GRANT SELECT ON group_members TO garraia_login;     -- Gap C (391c)
```

Any further grant requires (a) a new migration, (b) a new ADR amendment with rationale, and (c) a security review. The role is now at its maximum acceptable surface for the login + extractor flows; signup and tenant management use distinct roles.

---

## Migration strategy: `mobile_users` → `user_identities`

The migration is implemented by `garraia-cli migrate workspace` ([GAR-413](https://linear.app/chatgpt25/issue/GAR-413)) — **not** by this ADR. The algorithm is normative and must match exactly:

```
For each row in SQLite garraia-db.mobile_users:
    1. Generate user_id via uuid_v7() (Rust side)
    2. INSERT INTO Postgres garraia-workspace.users
       (id, email, display_name, status, legacy_sqlite_id, created_at, updated_at)
       VALUES (
           user_id,
           lower(mobile_users.email),       -- citext column lower-cases on read
           split_part(mobile_users.email, '@', 1), -- best-effort display_name
           'active',
           mobile_users.id::text,            -- bridge for audit traceability
           mobile_users.created_at,
           mobile_users.created_at
       );
    3. INSERT INTO Postgres garraia-workspace.user_identities
       (id, user_id, provider, provider_sub, password_hash, created_at)
       VALUES (
           uuid_v7(),
           user_id,
           'internal',
           user_id::text,                    -- internal provider uses user_id as sub
           mobile_users.password_hash,        -- PBKDF2 PHC string copied verbatim
           mobile_users.created_at
       );
    4. INSERT INTO Postgres garraia-workspace.audit_events
       (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata)
       VALUES (
           NULL,                              -- pre-group event
           NULL,                              -- system actor
           'system.migrate_workspace',
           'users.imported_from_sqlite',
           'user',
           user_id::text,
           jsonb_build_object(
               'source', 'mobile_users',
               'legacy_id', mobile_users.id,
               'hash_algorithm', 'pbkdf2-sha256',
               'lazy_upgrade_pending', true
           )
       );
```

**Critical invariants:**

1. **Hash format is preserved.** The `mobile_users.password_hash` PBKDF2 PHC string is copied byte-for-byte. No re-hashing during migration (we don't have plaintexts, and even if we did, batch re-hashing is disallowed per ADR decision X).
2. **Lazy upgrade triggers on next login.** The login flow detects `$pbkdf2-sha256$` prefix and upgrades transparently (see `verify_password()` pseudocode above).
3. **`mobile_users` table is NOT dropped.** Per ADR 0003, `garraia-db` (SQLite) remains as the historical record and the single-user CLI fallback. The migration is a one-way copy.
4. **Migration runs as superuser.** The migration tool needs to bypass RLS to bulk-insert into `users` and `user_identities`. It does NOT use `garraia_login` — that role is for live logins only.
5. **Audit row is mandatory per imported user.** Compliance trail for LGPD art. 18 (right to information about processing).
6. **Email is lowercased on insert.** The target column is `citext` which case-insensitively compares; lowercasing on insert keeps the stored form canonical.

**Rollback:** if the migration is incomplete, the SQLite source remains untouched and the partial Postgres state can be cleared via `DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL` (cascades through `user_identities`).

---

## PBKDF2 → Argon2id transition (dual-verify)

The transition is **lazy upgrade only**. There is no batch re-hash, no forced password reset at migration time, and no dual-storage of both hashes simultaneously.

### Detection

```rust
fn detect_format(hash: &str) -> HashFormat {
    if hash.starts_with("$argon2id$") {
        HashFormat::Argon2id
    } else if hash.starts_with("$argon2i$") {
        HashFormat::Argon2i // accept but upgrade
    } else if hash.starts_with("$pbkdf2-sha256$") {
        HashFormat::Pbkdf2Sha256
    } else {
        HashFormat::Unknown
    }
}
```

### Verify path

1. Login flow reads `password_hash` (one column, one row) via the `garraia_login` BYPASSRLS pool.
2. `verify_password()` dispatches based on PHC prefix.
3. On verification success **and** the stored hash is PBKDF2: re-hash with Argon2id, `UPDATE` the row in the same transaction. Failure to upgrade does NOT block the login — the user is authenticated; the upgrade retries on the next login.
4. `audit_events` row is inserted in the same transaction with `action = 'login.password_hash_upgraded'` (in addition to `login.success`).

### Straggler audit (operational, ~6 months post-launch)

```sql
SELECT count(*) FROM user_identities
WHERE password_hash LIKE '$pbkdf2-sha256$%';
```

If the count is non-zero after 6 months, the affected users haven't logged in. Send a password reset email forcing them to re-establish a credential. After 12 months, delete the unupgraded rows entirely (with audit trail) — at that point the affected accounts are abandoned.

### Anti-pattern (do not do this)

**Do not** store both PBKDF2 and Argon2id hashes for the same row (e.g., adding a `password_hash_v2` column and verifying both). This doubles the attack surface and creates a window where invalidating one hash without the other leaves an inconsistent record. PHC prefix detection on a single column is the correct pattern.

---

## Anti-patterns (explicit list)

The following patterns must NEVER appear in `garraia-auth` or any code that touches the login flow. They are documented here so review reviewers (`@security-auditor`, `@code-reviewer`) can pattern-match against them.

1. **Treating "0 rows from app pool" as "user not found".** Empty result from `garraia_app` reading `user_identities` means RLS blocked, not that the user doesn't exist. Use the `garraia_login` BYPASSRLS pool for credential verification, period.
2. **Logging `password_hash` in spans, error messages, or panic messages.** `garraia-telemetry::redact` covers the header path; the application code path must be reviewed manually. Tracing the verify path uses `#[instrument(skip(password, hash))]`.
3. **Returning the password hash to the API caller**, even in error responses. Login endpoints return either `{ user_id, access_token, refresh_token }` or a generic `{ error: "invalid_credentials" }`.
4. **Sharing the login pool with the main app pool.** Enforced at compile time by `LoginPool` newtype.
5. **Using `to_tsquery` on user-supplied search input** (legacy from GAR-388 — applies to the FTS path, not auth, but worth keeping in the anti-pattern list).
6. **Verifying credentials in the database** (option B of this ADR) without explicit ADR override. The crypto stack is Rust.
7. **Re-hashing PBKDF2 → Argon2id outside of the verify path.** No background job, no batch script, no admin endpoint. Lazy upgrade only.
8. **Caching `garraia_login` connections in the main app pool.** The login pool is short-lived per request; refresh tokens go through the main app pool because that path doesn't read `password_hash`.
9. **Issuing JWTs without an audit row.** Every successful login writes `audit_events.action = 'login.success'` in the same transaction as the session insert.
10. **Confusing "session valid" with "user authenticated".** Session refresh is a distinct flow that does NOT touch `password_hash` and does NOT use `garraia_login`. It uses the main app pool with `app.current_user_id` set from the refresh token claim.
11. **Logging the email or any submitted credential at INFO/DEBUG level from the login flow.** Email is PII under LGPD art. 5. The canonical place for the email is `audit_events.actor_label`. `tracing::info!(email = %email, ...)` lands in log aggregators with no controls and creates a parallel uncontrolled PII path. Use `tracing::info!(user_id = %user_id, action = "login.success")` after successful auth and skip email entirely on failure paths.
12. **Including the JWT (access or refresh) in URLs, query parameters, or `Referer` headers.** JWTs in URLs leak via access logs, CDN caches, browser history, and client-side referer leakage. Tokens go in the `Authorization: Bearer ...` header for requests and in the response body (or `Set-Cookie` with `HttpOnly; Secure; SameSite=Strict`) for the login response. Never `?token=...`.
13. **Returning differentiated error messages distinguishing "wrong password" from "user not found" to the API caller.** The login endpoint returns either a successful response or `{ "error": "invalid_credentials" }` — never `{ "error": "user not found" }` vs `{ "error": "wrong password" }`. The internal `audit_events` row distinguishes them for forensic purposes; the API does not. This combined with the dummy-hash constant-time verify (see implementation impact §7) makes user enumeration via login responses structurally impossible.
14. **Verifying credentials over plain HTTP.** The login endpoint MUST be served over TLS (operational requirement enforced by the deployment runbook + reverse proxy config; documented in `crates/garraia-workspace/README.md` "Required Postgres role privileges" section as a deployment prerequisite).

---

## Consequences

### Positive

- Single, narrow login surface — `garraia_login` role used **only** by the login endpoint, enforced at compile time via `LoginPool` newtype
- Lazy upgrade dual-verify zero-disrupts existing PBKDF2 users while moving the population to Argon2id incrementally
- Argon2id parameters match RFC 9106 first recommendation, balancing security against login latency
- JWT internal stays HS256 for v1 simplicity, with a clean migration path to RS256/EdDSA in Fase 7
- The `IdentityProvider` trait shape allows future OIDC adapters (Keycloak, Auth0, Authelia, Google) without modifying the login flow surface
- `audit_events` captures every login attempt in the same transaction as the credential read — no silent failures
- Compliance posture: the empty-result-from-RLS pitfall is explicitly addressed in code (BYPASSRLS pool returns true row counts) and in documentation (anti-pattern #1)
- Rollback is mechanical: `DROP ROLE garraia_login` + revert grants + revert this ADR
- Migration tool (GAR-413) has a normative algorithm to follow; no ambiguity

### Negative

- Two pools to manage in production (app pool + login pool) — operational complexity
- BYPASSRLS role compromise = full credential store exposure (mitigated by network isolation, audit, rotation, and the narrow grant set)
- PBKDF2 dual-verify code path lives until the straggler audit (~6 months post-launch)
- HS256 needs the shared secret distributed to every verifier — acceptable for v1 single-instance, requires migration plan when multi-region lands
- The `LoginPool` newtype adds boilerplate compared to a raw `PgPool`

### Neutral / mitigated

- `mobile_users` table stays in `garraia-db` (SQLite) post-migration as the historical record per ADR 0003 — documented, not deleted
- OIDC adapter remains a future ADR (likely 0009) — this ADR ships only the trait shape and the `Internal` implementation outline
- The straggler force-expire policy is operational, not enforced by code

---

## Risk register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| `garraia_login` pool credentials leak | Low | **Critical** | Network isolation + distinct vault entry (GAR-410) + rotation policy + `pgaudit` logging on `user_identities` reads |
| Lazy upgrade UPDATE race condition | Low | Medium | `SELECT ... FOR NO KEY UPDATE OF ui` acquires a row-level lock at SELECT time, preventing concurrent logins from racing to upgrade the same row. A transaction boundary alone is NOT sufficient — the explicit lock is mandatory. See `verify_credential` pseudocode for the exact form. |
| PBKDF2 stragglers never log in again | Medium | Low | 6-month audit + forced password reset email + 12-month deletion with audit trail |
| `GARRAIA_JWT_SECRET` rotation breaks live sessions | Medium | Medium | Document rotation procedure (kid header + key set) in GAR-410 Vault implementation; defer to Fase 6 ops runbook |
| `argon2` crate version drift breaks PHC parsing | Low | High | Pin version in workspace `Cargo.toml`; integration test on every cargo upgrade; `cargo audit` in CI (filed as part of GAR-411) |
| `LoginPool` accidentally constructed via raw `PgPool` | Medium | **High** | Newtype wrapper with private inner field; only constructable via `from_dedicated_config` which validates the role name; review pass enforces it |
| `audit_events` INSERT fails mid-login → silent compromise | Low | Medium | Audit insert in the same transaction as the verify; failure rolls back the entire login; user gets a generic 503 instead of authenticated |
| OIDC adapter (future) needs trait shape change | Medium | Low | Trait is v1, future ADR can extend or supersede; documented as "subject to GAR-391 implementation feedback" |
| User enumeration via timing differences (success vs not-found) | Medium | Medium | Constant-time response: always run Argon2 verify even for not-found users (with a static dummy hash); rate limiting via `tower-governor` (operational, GAR-391) |
| Login role grant escalation via `pg_hba.conf` misconfiguration | Low | High | Production deployments use `cert` or `scram-sha-256` auth, never `trust`; documented in `crates/garraia-workspace/README.md` |

---

## Validation

This ADR is a research/decision document. Validation is by review, not benchmark.

- **Security review by `@security-auditor`** — focused on BYPASSRLS blast radius, Argon2id parameters vs RFC 9106, JWT key management, dual-verify race conditions, audit completeness, anti-pattern clarity
- **Cross-reference verification** — links to GAR-407, GAR-408, GAR-391, GAR-413 must resolve and the cited code (RLS policy, README hard blocker section, schema columns) must exist as referenced
- **Trait shape sanity check** — pseudocode validated against `argon2 0.5`, `sqlx 0.8`, `async-trait 0.1`, `jsonwebtoken` ecosystem versions current as of 2026-04

---

## Implementation impact on GAR-391 (`garraia-auth` crate)

This ADR is the spec for `garraia-auth`. Implementation work in [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) inherits the following normative requirements:

1. **Crate structure:** `crates/garraia-auth/` with `src/lib.rs` exposing `IdentityProvider`, `Identity`, `Credential`, `AuthError`, `LoginPool`, `InternalProvider`, `Principal`, `RequirePermission` extractor.
2. **Migration:** ship a new migration (likely `008_login_role.sql`) creating `garraia_login` NOLOGIN BYPASSRLS with the exact GRANTs from the §"Login role specification" above. **No code outside `garraia-auth` may use this role.**
3. **Two pools in `AppState`:** `app_pool: PgPool` (existing pattern) and `login_pool: LoginPool` (new). The login pool's connection string is loaded from `GARRAIA_LOGIN_DATABASE_URL` (or vault entry, post-GAR-410) — distinct from `GARRAIA_WORKSPACE_DATABASE_URL`.
4. **`InternalProvider`:** implements `IdentityProvider` per the §"`InternalProvider` implementation outline" pseudocode. Uses `argon2 = "0.5"` crate with the RFC 9106 parameters.
5. **Dual-verify path:** detect PHC prefix, verify with the matching algorithm, lazy-upgrade PBKDF2 to Argon2id on success in the same transaction. Failure to upgrade is logged via `tracing::warn!` but does NOT block the login.
6. **Audit events:** every login attempt produces an `audit_events` row in the same transaction. Actions: `login.success`, `login.failure_user_not_found`, `login.failure_bad_password`, `login.failure_account_suspended`, `login.failure_account_deleted`, `login.password_hash_upgraded`. `actor_user_id` is NULL on `failure_user_not_found`. `actor_label` is the email at attempt time (acceptable PII trade-off because the audit record is the canonical source of "who tried to log in as whom"). **Forensic metadata in `audit_events.metadata` jsonb is mandatory:** `{ ip, user_agent, request_id }` populated from the Axum extractor's `RequestCtx`. Without these, account-takeover investigations are impossible.
7. **Anti-enumeration via constant-time response:** for `failure_user_not_found`, run a dummy Argon2 verify against a fixed dummy hash so that the response time is indistinguishable from `failure_bad_password`. The dummy hash lives in `garraia-auth/src/internal_provider.rs` as a `const DUMMY_HASH: &str`. **The dummy hash MUST be generated with the exact same Argon2id parameters as production hashes (m=65536, t=3, p=4)** — if the parameters differ, the timing profile differs and the mitigation fails. The dummy hash is not a secret (its purpose is timing parity, not confidentiality) and does not need rotation. Generate it once at crate build time via a build script.
8. **Axum extractor `Principal`:** loads from JWT in the `Authorization: Bearer ...` header, sets `app.current_group_id` and `app.current_user_id` via `SET LOCAL` at the start of the transaction (per GAR-408 contract). Returns 500 if the JWT is valid but the principal lacks a `group_id` for a group-scoped route.
9. **Cross-group authz tests (GAR-392):** at least 100 scenarios covering messages/chats/memory/tasks/files/audit cross-group reads. All must return 0 rows under RLS. The suite is gated on every `garraia-auth` PR.
10. **No write to `user_identities` outside of `InternalProvider::create_identity` and `verify_credential` (lazy upgrade).** Any other code path that mutates `user_identities` violates the architectural boundary.
11. **JWT issuance:** HS256 via `jsonwebtoken` crate, signing key from `GARRAIA_JWT_SECRET` env var (or vault). Access token TTL 15 min, refresh token TTL 30 days. **The refresh token MUST be a cryptographically random opaque string (NOT a JWT)**, stored in `sessions.refresh_token_hash` as HMAC-SHA256 of the token + a site key. This decoupling is critical for `GARRAIA_JWT_SECRET` rotation: rotating the JWT secret invalidates all 15-minute access tokens (intentional and survivable), but does NOT invalidate the 30-day refresh tokens (which would be a denial-of-service equivalent — every active session would die simultaneously). Argon2 is NOT used here because refresh tokens are random high-entropy strings, not dictionary-attackable; HMAC-SHA256 is fast and adequate. Refresh token rotation on use (each refresh issues a new token and revokes the old one) is recommended per OWASP — implementation detail in GAR-391.
12. **No OIDC adapter implementation.** This ADR ships only `Internal`. OIDC adapter is a future ADR (likely 0009).

---

## Rollback plan

Three levels:

1. **Before merge:** close the PR. Decision is unchanged.
2. **After merge, before GAR-391 ships:** `git revert` this ADR commit. The decision returns to "open question". `garraia-auth` work pauses until a new ADR is written.
3. **After GAR-391 ships:** the ADR is the historical record. To change the decision, write `0005-superseded.md` following standard MADR superseding pattern. The `garraia-auth` code must be refactored — non-trivial, requires its own implementation plan. The `garraia_login` role can be dropped via a new migration (`migration 009_drop_login_role.sql`) once the new pattern is in place.

Zero code is touched in this ADR — rollback at level 1 or 2 is free.

---

## Links

- [GAR-375](https://linear.app/chatgpt25/issue/GAR-375) — this ADR's source issue
- [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — implementation that this ADR enables
- [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) — migration tool that uses §"Migration strategy"
- [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) — RLS migration that created the hard blocker
- [GAR-410](https://linear.app/chatgpt25/issue/GAR-410) — `CredentialVault` final, future home for the login DB password
- [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) — `cargo audit` in CI for `argon2` version drift detection
- [`docs/adr/0003-database-for-workspace.md`](0003-database-for-workspace.md) — Postgres + RLS context
- [`docs/adr/0005-identity-provider.md`](0005-identity-provider.md) — this file
- [`crates/garraia-workspace/migrations/001_initial_users_groups.sql`](../../crates/garraia-workspace/migrations/001_initial_users_groups.sql) — schema target
- [`crates/garraia-workspace/migrations/007_row_level_security.sql`](../../crates/garraia-workspace/migrations/007_row_level_security.sql) — `user_identities_owner_only` policy
- [`crates/garraia-workspace/README.md`](../../crates/garraia-workspace/README.md) — "HARD BLOCKER for GAR-391" section
- [RFC 9106](https://datatracker.ietf.org/doc/html/rfc9106) — Argon2 specification
- [RFC 7519](https://datatracker.ietf.org/doc/html/rfc7519) — JWT specification
- [OWASP Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)
- [PostgreSQL ROLE attributes](https://www.postgresql.org/docs/16/sql-createrole.html) — BYPASSRLS reference
