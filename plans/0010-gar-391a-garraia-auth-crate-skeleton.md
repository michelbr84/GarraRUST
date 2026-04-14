# Plan 0010: GAR-391a — `garraia-auth` crate skeleton + login role migration

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — sub-issue 391a (skeleton + login role)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-authz`, `security`
> **Priority:** Urgent
> **Estimated session size:** 3-5 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (`user_identities` schema) + ✅ GAR-408 (RLS FORCE + `garraia_app` role + Axum extractor contract) + ✅ GAR-375 (ADR 0005 — normative spec)
> **Unblocks:** **GAR-391b** (real `verify_credential` impl + login endpoint), **GAR-391c** (Axum `Principal` extractor + `RequirePermission`), **GAR-391d** (cross-group authz test suite — GAR-392)

---

## 1. Goal (one sentence)

Criar o crate `crates/garraia-auth/` com (a) `Cargo.toml` + estrutura de módulos, (b) tipos `Identity`, `Credential`, `AuthError`, `Principal`, (c) trait `IdentityProvider` + `InternalProvider` struct skeleton com método bodies retornando `Err(AuthError::NotImplemented)`, (d) `LoginPool` newtype com runtime-validated constructor (`from_dedicated_config`) que verifica `current_user = 'garraia_login'`, (e) migration `008_login_role.sql` (em `garraia-workspace/migrations/`) criando a role `garraia_login NOLOGIN BYPASSRLS` com os GRANTs exatos do ADR 0005 §"Login role specification", e (f) smoke test em duas camadas validando que a role existe com BYPASSRLS attribute e que `LoginPool::from_dedicated_config` rejeita pools conectados como qualquer outra role — tudo seguindo a spec normativa do ADR 0005 §"Implementation impact on GAR-391", sem implementar login endpoint, sem extractor Axum, sem suite de authz.

---

## 2. Rationale — por que essa fatia primeiro

1. **Foundation antes de feature.** O crate, o trait, os tipos e o `LoginPool` newtype são pré-requisitos de TODAS as outras fatias (391b, 391c, 391d). Ship them isolated minimiza risk of refactor downstream.
2. **Migration 008 desbloqueia operações.** Sem o `garraia_login` role no DB, nenhum código de login pode rodar. Aplicar a migration agora habilita 391b a focar em lógica Rust pura, sem ficar preso em SQL/RLS debugging.
3. **`LoginPool` newtype é a defense em profundidade do compile time.** O ponto chave do ADR 0005 — "compile-time guarantee contra cross-pool misuse" — só é entregue se o newtype existe ANTES de qualquer código de login. Implementar 391b primeiro arriscaria criar atalhos via raw `PgPool` que depois precisariam refactor.
4. **Trait skeleton congela a interface.** Os 4 métodos do `IdentityProvider` (`id`, `find_by_provider_sub`, `verify_credential`, `create_identity`) entram como `unimplemented!()` ou `Err(AuthError::NotImplemented)`. 391b/c implementam corpos reais sem mexer em assinaturas. Reduces churn nos callers.
5. **Tamanho cabível e baixo risco.** 3-5h. Sem crypto real, sem endpoint, sem JWT, sem extractor. Apenas typing e migration. Risco ≈ 0 de bug funcional.
6. **Validação empírica do BYPASSRLS na sessão.** O smoke test `pg_roles WHERE rolname='garraia_login' AND rolbypassrls=true` confirma que a migration funcionou — fechando o loop empírico que o ADR 0005 só especificou em texto.

---

## 3. Scope & Non-Scope

### In scope

**Novo crate `crates/garraia-auth/`:**

- `Cargo.toml` com nome `garraia-auth`, version/edition/license/rust-version inherited from workspace, deps mínimos:
  - `sqlx` (workspace, runtime-tokio + postgres + uuid + chrono)
  - `uuid` (workspace, v7 feature)
  - `serde` + `serde_json` (workspace)
  - `thiserror` (workspace)
  - `tokio` (workspace)
  - `tracing` (workspace)
  - `async-trait = "0.1"` (NOVA dep, padrão do ecossistema async traits)
- `src/lib.rs` — re-exports públicos
- `src/error.rs` — `AuthError` enum
- `src/types.rs` — `Identity`, `Credential`, `Principal`, `RequestCtx`
- `src/provider.rs` — `IdentityProvider` trait
- `src/internal.rs` — `InternalProvider` struct + impl skeleton
- `src/login_pool.rs` — `LoginPool` newtype + `from_dedicated_config`
- `tests/skeleton.rs` — smoke test validando a construção e os retornos `NotImplemented`

**Migration nova (em `crates/garraia-workspace/migrations/`):**

- `008_login_role.sql` com:
  - `DO $$ IF NOT EXISTS ... CREATE ROLE garraia_login NOLOGIN BYPASSRLS $$` (idempotent, mesmo padrão de `garraia_app` em 007)
  - GRANTs exatos do ADR 0005 §"Login role specification":
    - `GRANT SELECT, UPDATE ON user_identities TO garraia_login;`
    - `GRANT SELECT ON users TO garraia_login;`
    - `GRANT INSERT, UPDATE ON sessions TO garraia_login;`
    - `GRANT INSERT ON audit_events TO garraia_login;`
  - `COMMENT ON ROLE garraia_login` documentando o propósito + blast radius warning
- A migration é forward-only, slot 008 aceita qualquer migration futura > 008 (009, 010...).

**Edits em arquivos existentes:**

- `Cargo.toml` (workspace root) — adicionar `"crates/garraia-auth"` ao `[workspace].members`
- `crates/garraia-workspace/tests/migration_smoke.rs` — extender:
  - Adicionar tabelas/role check para `garraia_login` em `pg_roles` com `rolbypassrls = true`
  - Validar que os 4 GRANTs estão presentes via `information_schema.role_table_grants` ou `has_table_privilege`
- `ROADMAP.md` §3.3 — marcar primeiros 3-4 itens de `garraia-auth` skeleton como `[x]` (`enum Scope`, `struct Principal`, struct skeletons)
- `CLAUDE.md` estrutura de crates — mover `garraia-auth` de "planejados" para "ativos" com descrição da skeleton

**Plano versionado:** `plans/0010-gar-391a-garraia-auth-crate-skeleton.md` (este arquivo) committed junto.

**Linear:**
- Comentar em GAR-391 (sem mover): "Sub-issue 391a (skeleton + login role) merged em commit `<hash>`. 391b (real verify_credential), 391c (extractor), 391d (authz suite) ainda pending."
- Não criar GAR-391a/b/c/d como issues separadas no Linear ainda — manter GAR-391 como umbrella enquanto cada fatia é entregue. **Mover GAR-391 para Done apenas quando 391d shippar.**

### Out of scope (deferido a 391b/c/d)

- ❌ **Login endpoint (`POST /v1/auth/login`).** É 391b/c.
- ❌ **Refresh endpoint, logout endpoint, signup endpoint.** 391b/c.
- ❌ **Real `verify_credential` body** (Argon2id verify, PBKDF2 dual-verify, lazy upgrade). 391b.
- ❌ **Real `find_by_provider_sub` body.** 391b.
- ❌ **Real `create_identity` body.** 391b.
- ❌ **`audit_login` helper implementation.** 391b (signature stays stubbed em 391a se referenciado).
- ❌ **Axum `Principal` extractor + `FromRequestParts` impl.** 391c. O `Principal` struct em 391a é só shape (campos + Debug), sem extractor logic.
- ❌ **`RequirePermission(Action)` extractor.** 391c.
- ❌ **`fn can(principal, action) -> bool` capability check.** 391c.
- ❌ **`DUMMY_HASH` generation via build script.** 391b precisa para constant-time path.
- ❌ **`RequestCtx` extraction from headers (X-Forwarded-For, User-Agent, X-Request-ID).** 391c — em 391a `RequestCtx` é só struct shape.
- ❌ **JWT issuance + verification (HS256).** 391b.
- ❌ **`sessions` table CRUD** (token issuance/refresh/revocation). 391b.
- ❌ **Cross-group authz test suite (100+ scenarios).** 391d / GAR-392.
- ❌ **Rate limiting via tower-governor.** 391c ou follow-up.
- ❌ **Account status checks** (suspended/deleted detection — ADR 0005 M2). 391b.
- ❌ **Constant-time anti-enumeration path.** 391b.
- ❌ **Wiring no gateway** (`AppState` ganhando `auth: Arc<dyn IdentityProvider>`). 391c, junto com extractor.
- ❌ **OIDC adapter** (`OidcProvider`). Futuro ADR 0009.
- ❌ **`mobile_users` migration tool integration** (GAR-413).
- ❌ **PII redaction layer specific to email** (existe via `garraia-telemetry` para headers, content layer fica em GAR-391b).
- ❌ **`#[tracing::instrument]` em todos os métodos.** Apenas no `LoginPool::from_dedicated_config` (porque ele toca config sensível).

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check -p garraia-auth` verde.
- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-auth --all-targets -- -D warnings` verde.
- [ ] `cargo clippy --workspace -- -D warnings` verde (no novo warning introduzido fora de garraia-auth).
- [ ] `cargo test -p garraia-workspace` verde — smoke test estendido valida `garraia_login` role + 4 grants.
- [ ] `cargo test -p garraia-auth` verde — novo smoke test do crate.
- [ ] Smoke test wall time da `garraia-workspace` ≤ 35s (era ~9.75s; +1 migration 008 + 5 novos asserts adiciona ~1-3s).
- [ ] Migration 008 aplica do zero (após 001-007) sem erros em ≤ 200ms.
- [ ] `pg_roles` tem entry `garraia_login` com `rolbypassrls = true` E `rolcanlogin = false`.
- [ ] `has_table_privilege('garraia_login', 'user_identities', 'SELECT, UPDATE') = true`.
- [ ] `has_table_privilege('garraia_login', 'users', 'SELECT') = true`.
- [ ] `has_table_privilege('garraia_login', 'sessions', 'INSERT, UPDATE') = true`.
- [ ] `has_table_privilege('garraia_login', 'audit_events', 'INSERT') = true`.
- [ ] `has_table_privilege('garraia_login', 'messages', 'SELECT') = false` (negative — login role MUST NOT have access to non-auth tables).
- [ ] Crate `garraia-auth` adicionado ao workspace `members` array.
- [ ] `IdentityProvider` trait existe com 4 métodos assinados.
- [ ] `InternalProvider` struct existe com `new()` constructor + 4 trait methods retornando `Err(AuthError::NotImplemented)`.
- [ ] `LoginPool` newtype existe; o campo interno é privado; o único constructor público é `from_dedicated_config(config: &LoginConfig) -> Result<Self, AuthError>` que valida `current_user = 'garraia_login'` via query.
- [ ] **Negative test:** tentativa de chamar `LoginPool::from_dedicated_config` com credenciais de outro role (ex.: `postgres` superuser) retorna `AuthError::WrongRole(actual_role)`.
- [ ] **Positive test:** `LoginPool::from_dedicated_config` com credenciais de `garraia_login` retorna `Ok(LoginPool)`.
- [ ] `Principal`, `Identity`, `Credential`, `AuthError`, `RequestCtx` derivam `Debug` + `Clone` (onde aplicável).
- [ ] `AuthError` deriva `thiserror::Error` com variants pelo menos: `NotImplemented`, `Storage(sqlx::Error)`, `Config(String)`, `WrongRole(String)`, `InvalidCredentials`, `UnsupportedCredential(String)`, `UnknownHashFormat`.
- [ ] **Nenhuma função pública em `garraia-auth` faz crypto, JWT, ou DB queries fora de `LoginPool::from_dedicated_config`.** Tudo é stub. (Verificado por inspeção do code reviewer.)
- [ ] `ROADMAP.md` §3.3 com itens iniciais de `garraia-auth` marcados `[x]`.
- [ ] `CLAUDE.md` estrutura de crates com `garraia-auth` movido para "ativos".
- [ ] Migration é forward-only (sem DROP, sem destructive ALTER).
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] **GAR-391 NÃO movida para Done** (continua Backlog) com comment em Linear linkando o commit e listando 391b/c/d como pending.

---

## 5. File-level changes

### 5.1 Novo crate

```
crates/garraia-auth/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── types.rs
│   ├── provider.rs
│   ├── internal.rs
│   └── login_pool.rs
└── tests/
    └── skeleton.rs
```

### 5.2 Nova migration (em `garraia-workspace`)

```
crates/garraia-workspace/migrations/
└── 008_login_role.sql    # ★ role + grants per ADR 0005
```

### 5.3 Edits em arquivos existentes

- `Cargo.toml` (workspace root) — adicionar `"crates/garraia-auth"` ao `[workspace].members` (alphabetical-adjacent, depois de `garraia-agents`).
- `crates/garraia-workspace/tests/migration_smoke.rs` — extender com:
  - Validação de `pg_roles` para `garraia_login` (BYPASSRLS=true, LOGIN=false)
  - Validação de 4 grants positivos via `has_table_privilege(...)`
  - Validação de 1 grant negativo (login role não pode ler `messages`)
- `ROADMAP.md` §3.3 — marcar `[x]`:
  - `enum Scope { User(Uuid), Group(Uuid), Chat(Uuid) }` skeleton
  - `struct Principal { user_id, group_id, role }` shape
- `CLAUDE.md` "Estrutura de crates" — mover `garraia-auth` da seção "planejados" para "ativos" com descrição:
  ```
  garraia-auth/        — ✅ skeleton (GAR-391a). IdentityProvider trait + InternalProvider
                         struct + LoginPool newtype + Principal/Credential/AuthError types.
                         Real verify_credential / endpoint / extractor wait for 391b/c/d.
                         Decisão: docs/adr/0005-identity-provider.md.
  ```

### 5.4 Zero edits em código existente fora dos arquivos listados

- `garraia-gateway` intocado (wiring fica em 391c)
- `garraia-config` intocado
- Demais migrations 001-007 intocadas

---

## 6. Schema details (migration 008)

```sql
-- 008_login_role.sql
-- GAR-391a — Cria garraia_login NOLOGIN BYPASSRLS dedicated role para o
-- login flow do crate garraia-auth. Resolve o hard blocker arquitetural
-- documentado em GAR-408 e formalizado em ADR 0005 (GAR-375).
--
-- Plan:     plans/0010-gar-391a-garraia-auth-crate-skeleton.md
-- ADR:      docs/adr/0005-identity-provider.md (§"Login role specification")
-- Depends:  migrations 001 (users/sessions/api_keys/user_identities)
--           e 002 (audit_events).
-- Forward-only. No DROP, no destructive ALTER.
--
-- ─── Threat model ─────────────────────────────────────────────────────────
--
-- garraia_login is a BYPASSRLS role used EXCLUSIVELY by the login endpoint
-- via the LoginPool newtype in the garraia-auth crate. It is NOT used by
-- the main app pool (garraia_app), it is NOT used by migrations (which run
-- as superuser), and it is NOT used by any background worker.
--
-- COMPROMISE OF garraia_login = FULL CREDENTIAL STORE EXPOSURE.
-- Mitigation: network isolation, distinct vault entry (GAR-410), rotation,
-- and pgaudit logging on user_identities reads.
--
-- See ADR 0005 §"Login role specification" for the production deployment
-- requirements (separate Unix socket, separate firewall rule, distinct
-- credentials never shared with the main app pool).

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_login') THEN
        CREATE ROLE garraia_login NOLOGIN BYPASSRLS;
    END IF;
END
$$;

-- Minimal grants for the login flow. See ADR 0005 §"Login role specification"
-- for the rationale of each grant. Any future addition requires a new
-- migration AND a security review.
GRANT USAGE ON SCHEMA public TO garraia_login;

-- Read user_identities to verify credentials.
-- Update user_identities for lazy upgrade PBKDF2 → Argon2id (future 391b).
GRANT SELECT, UPDATE ON user_identities TO garraia_login;

-- Read users to look up by email and get display_name for audit_label.
GRANT SELECT ON users TO garraia_login;

-- Insert/update sessions to issue refresh tokens.
GRANT INSERT, UPDATE ON sessions TO garraia_login;

-- Insert audit_events to log every login attempt (success and failure).
GRANT INSERT ON audit_events TO garraia_login;

-- Sequences used by the granted tables (DEFAULT gen_random_uuid() doesn't
-- need a sequence, but if any of the above tables ever uses a serial PK,
-- the role would need USAGE on its sequence). Pre-emptive grant for forward
-- compatibility:
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO garraia_login;

-- The login role must NOT have access to:
--   - messages, chats, chat_members, message_threads (chat data)
--   - memory_items, memory_embeddings (AI memory)
--   - tasks, task_lists, task_assignees, task_labels, task_label_assignments,
--     task_comments, task_subscriptions, task_activity (work tracking)
--   - groups, group_members, group_invites (tenant management)
--   - api_keys (separate auth surface)
--   - roles, permissions, role_permissions (RBAC config)
-- These are NOT granted, so REVOKE is unnecessary.

COMMENT ON ROLE garraia_login IS
    'BYPASSRLS dedicated role used EXCLUSIVELY by the garraia-auth login flow. '
    'NOLOGIN by default — production deployments must promote via ALTER ROLE WITH '
    'LOGIN PASSWORD. Compromise = full credential store exposure. See ADR 0005 '
    '(docs/adr/0005-identity-provider.md) and GAR-391 implementation. Code outside '
    'the LoginPool newtype in garraia-auth MUST NOT use this role under any '
    'circumstances.';
```

---

## 7. Rust skeleton outline

The actual Rust code is written in wave 1. Below is the **shape** that the agent must produce. Every method body is `Err(AuthError::NotImplemented)` or `unimplemented!()` or returns a stub value — **no real logic in 391a**.

### 7.1 `src/lib.rs`

```rust
//! `garraia-auth` — authentication and authorization for GarraIA Group Workspace.
//!
//! ## Status: skeleton (GAR-391a)
//!
//! This crate is a SKELETON. The trait shape, types, and the LoginPool newtype
//! are real and load-bearing. The implementation bodies are stubs that return
//! `AuthError::NotImplemented`. Real bodies arrive in:
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

/// Convenience `Result` alias.
pub type Result<T> = std::result::Result<T, AuthError>;
```

### 7.2 `src/error.rs`

```rust
use thiserror::Error;

/// Errors surfaced by the `garraia-auth` crate.
///
/// `Storage` wraps the underlying sqlx error so callers can match on
/// connection failures vs constraint violations vs query errors.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("not implemented in 391a skeleton — see GAR-391b for real impl")]
    NotImplemented,

    #[error("auth config invalid: {0}")]
    Config(String),

    #[error("login pool connected as `{0}`, expected `garraia_login`")]
    WrongRole(String),

    #[error("storage error: {0}")]
    Storage(#[source] sqlx::Error),

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("unsupported credential variant for provider `{0}`")]
    UnsupportedCredential(String),

    #[error("hash format unrecognized")]
    UnknownHashFormat,

    #[error("provider `{0}` unavailable: {1}")]
    ProviderUnavailable(String, String),
}
```

### 7.3 `src/types.rs`

```rust
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// An authenticated identity. Returned by providers after successful
/// credential verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub user_id: Uuid,
    pub provider: String,    // 'internal' | 'oidc' | 'saml'
    pub provider_sub: String, // stable subject identifier from provider
}

/// A credential being verified. Variant selects which provider handles it.
#[derive(Debug, Clone)]
pub enum Credential {
    /// email + password against `user_identities` table with Argon2id
    /// (or PBKDF2 legacy with lazy upgrade).
    Internal { email: String, password: String },
    // OidcIdToken { token: String, issuer: String } — added in future ADR 0009.
}

/// Principal — the authenticated user in the context of a specific group.
/// Carried by Axum requests after the future `Principal` extractor (391c)
/// validates the JWT and looks up group membership.
#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: Uuid,
    pub group_id: Option<Uuid>,
    pub role: Option<String>, // 'owner' | 'admin' | 'member' | 'guest' | 'child'
}

/// Forensic context captured by the future Axum extractor (391c) and
/// passed into every login attempt by the future audit_login helper (391b).
#[derive(Debug, Clone, Default)]
pub struct RequestCtx {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
}
```

### 7.4 `src/provider.rs`

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::types::{Credential, Identity};
use crate::Result;

/// `IdentityProvider` is the trait every credential backend implements.
/// The shape is FROZEN by ADR 0005 — extensions come via new variants
/// of `Credential`, not new trait methods.
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Provider id — `'internal'`, `'oidc'`, `'saml'`, etc.
    /// Used for the `user_identities.provider` column.
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

    /// Create a new identity for an existing user (post-signup).
    async fn create_identity(&self, user_id: Uuid, credential: &Credential) -> Result<()>;
}
```

### 7.5 `src/internal.rs`

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::error::AuthError;
use crate::login_pool::LoginPool;
use crate::provider::IdentityProvider;
use crate::types::{Credential, Identity};
use crate::Result;

/// `InternalProvider` verifies credentials against the `user_identities`
/// table for `provider = 'internal'` rows (email + password with Argon2id /
/// PBKDF2 legacy). Uses the dedicated `LoginPool` exclusively.
///
/// **Skeleton (GAR-391a):** all methods return `Err(AuthError::NotImplemented)`.
/// Real bodies arrive in GAR-391b per ADR 0005 §"InternalProvider implementation
/// outline".
pub struct InternalProvider {
    #[allow(dead_code)] // used by 391b
    login_pool: LoginPool,
}

impl InternalProvider {
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
        // The error is informative; real implementation in 391b.
        match credential {
            Credential::Internal { .. } => Err(AuthError::NotImplemented),
            // future variants land here as match arms
        }
    }

    async fn create_identity(&self, _user_id: Uuid, _credential: &Credential) -> Result<()> {
        Err(AuthError::NotImplemented)
    }
}
```

### 7.6 `src/login_pool.rs`

```rust
use serde::Deserialize;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::instrument;
use validator::Validate;

use crate::error::AuthError;

/// Configuration for the dedicated login pool. Loaded from a SEPARATE
/// config path than the main app pool — production deployments MUST keep
/// these credentials in a distinct vault entry.
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct LoginConfig {
    /// Postgres URL. The connection role MUST be `garraia_login`.
    /// `LoginPool::from_dedicated_config` validates this at construction
    /// time via `SELECT current_user`.
    #[validate(length(min = 1))]
    pub database_url: String,

    /// Pool size. Default 5. Production should keep this small to bound
    /// the BYPASSRLS connection footprint.
    #[validate(range(min = 1, max = 50))]
    pub max_connections: u32,
}

impl Default for LoginConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 5,
        }
    }
}

/// `LoginPool` wraps a `PgPool` connected as the `garraia_login` BYPASSRLS
/// role. The inner pool is **private** and only accessible via methods on
/// `LoginPool` (currently none — 391b will add `pool()` as `pub(crate)`).
///
/// `LoginPool` is **not constructable** from a raw `PgPool` — only via
/// `from_dedicated_config`, which validates that the connection role is
/// actually `garraia_login`. This makes "accidentally use the login pool
/// for normal queries" a compile-time error: any code that has a
/// `LoginPool` got it through the validating constructor.
///
/// Implementing `From<PgPool>` for `LoginPool` is FORBIDDEN by ADR 0005.
/// Adding such an impl in a future PR violates the architectural boundary.
pub struct LoginPool {
    #[allow(dead_code)] // used by 391b
    inner: PgPool,
}

impl LoginPool {
    /// Connect to the dedicated login database using the role validation
    /// guard. Returns `AuthError::WrongRole` if the connection comes back
    /// as anything other than `garraia_login`.
    ///
    /// Tracing instrumentation `skip(config)` so the `database_url`
    /// (containing credentials) never lands in any span.
    #[instrument(skip(config), fields(max_connections = config.max_connections))]
    pub async fn from_dedicated_config(config: &LoginConfig) -> Result<Self, AuthError> {
        config
            .validate()
            .map_err(|e| AuthError::Config(e.to_string()))?;

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await
            .map_err(AuthError::Storage)?;

        // Runtime role guard. We query `current_user` immediately after
        // connecting and refuse if it isn't `garraia_login`. Any other
        // role (postgres, garraia_app, etc.) is a misconfiguration and
        // must fail loudly.
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .map_err(AuthError::Storage)?;

        if actual != "garraia_login" {
            // Drop the pool explicitly so the connection is returned
            // to the OS and not used for anything else.
            drop(pool);
            return Err(AuthError::WrongRole(actual));
        }

        Ok(Self { inner: pool })
    }
}

impl std::fmt::Debug for LoginPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginPool")
            .field("inner", &"<PgPool>")
            .finish()
    }
}
```

### 7.7 `tests/skeleton.rs`

```rust
//! Skeleton smoke test for `garraia-auth` (GAR-391a).
//!
//! Two test classes:
//! 1. Stub return validation — every method returns `AuthError::NotImplemented`.
//!    Real impl arrives in 391b.
//! 2. `LoginPool` constructor validation — positive (correct role) and negative
//!    (wrong role) paths.

use garraia_auth::{
    AuthError, Credential, IdentityProvider, InternalProvider, LoginConfig, LoginPool,
};
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PgImage;

async fn start_pgvector_container() -> anyhow::Result<(testcontainers::ContainerAsync<PgImage>, String)> {
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    Ok((container, url))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_pool_rejects_non_login_role() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;

    // Apply garraia-workspace migrations to create the garraia_login role.
    let migrate_pool = sqlx::PgPool::connect(&postgres_url).await?;
    garraia_workspace::Workspace::connect(garraia_workspace::WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;
    drop(migrate_pool);

    // Try to construct a LoginPool with the SUPERUSER credentials.
    // Should fail with WrongRole.
    let bad_config = LoginConfig {
        database_url: postgres_url.clone(),
        max_connections: 2,
    };
    let result = LoginPool::from_dedicated_config(&bad_config).await;
    match result {
        Err(AuthError::WrongRole(actual)) => {
            assert_eq!(actual, "postgres", "expected current_user = postgres");
        }
        other => panic!("expected WrongRole(postgres), got: {other:?}"),
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_pool_accepts_garraia_login_role() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;

    // Apply migrations.
    garraia_workspace::Workspace::connect(garraia_workspace::WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // Promote garraia_login to LOGIN with a known password (test only).
    let admin = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin)
        .await?;
    drop(admin);

    // Build a connection string using garraia_login credentials.
    // Re-parse the original URL and substitute credentials.
    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");

    let good_config = LoginConfig {
        database_url: login_url,
        max_connections: 2,
    };
    let pool = LoginPool::from_dedicated_config(&good_config).await?;
    drop(pool);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn internal_provider_methods_return_not_implemented() -> anyhow::Result<()> {
    let (_container, postgres_url) = start_pgvector_container().await?;

    garraia_workspace::Workspace::connect(garraia_workspace::WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    let admin = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin)
        .await?;
    drop(admin);

    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let pool = LoginPool::from_dedicated_config(&LoginConfig {
        database_url: login_url,
        max_connections: 2,
    })
    .await?;
    let provider = InternalProvider::new(pool);

    assert_eq!(provider.id(), "internal");

    let credential = Credential::Internal {
        email: "test@example.com".into(),
        password: "irrelevant".into(),
    };

    match provider.verify_credential(&credential).await {
        Err(AuthError::NotImplemented) => {}
        other => panic!("expected NotImplemented, got: {other:?}"),
    }

    match provider
        .find_by_provider_sub("00000000-0000-0000-0000-000000000000")
        .await
    {
        Err(AuthError::NotImplemented) => {}
        other => panic!("expected NotImplemented, got: {other:?}"),
    }

    let dummy_user = uuid::Uuid::nil();
    match provider.create_identity(dummy_user, &credential).await {
        Err(AuthError::NotImplemented) => {}
        other => panic!("expected NotImplemented, got: {other:?}"),
    }

    Ok(())
}
```

### 7.8 `garraia-workspace/tests/migration_smoke.rs` extension

Append at the end of the existing test (after the migration 006 task block, before `Ok(())`):

```rust
// ── Migration 008 validation ────────────────────────────────────────────
//
// garraia_login NOLOGIN BYPASSRLS dedicated role for the garraia-auth
// login flow. See ADR 0005 §"Login role specification" and migration
// 008_login_role.sql.

let login_role: (bool, bool) = sqlx::query_as(
    "SELECT rolbypassrls, rolcanlogin FROM pg_roles WHERE rolname = 'garraia_login'"
)
.fetch_one(workspace.pool())
.await?;
assert!(login_role.0, "garraia_login must have BYPASSRLS attribute");
assert!(!login_role.1, "garraia_login must be NOLOGIN by default");

// Positive grants.
for (table, privs) in &[
    ("user_identities", "SELECT, UPDATE"),
    ("users", "SELECT"),
    ("sessions", "INSERT, UPDATE"),
    ("audit_events", "INSERT"),
] {
    let granted: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('garraia_login', $1, $2)"
    )
    .bind(table)
    .bind(privs)
    .fetch_one(workspace.pool())
    .await?;
    assert!(granted, "garraia_login must have {privs} on {table}");
}

// Negative grant — login role MUST NOT have access to chat data.
let leaked: bool = sqlx::query_scalar(
    "SELECT has_table_privilege('garraia_login', 'messages', 'SELECT')"
)
.fetch_one(workspace.pool())
.await?;
assert!(!leaked, "garraia_login MUST NOT have SELECT on messages");
```

---

## 8. Test plan

Three test layers, all required for 391a:

1. **`crates/garraia-workspace/tests/migration_smoke.rs`** (extended) — validates the migration 008 effect at the schema layer. Runs against the testcontainer that the workspace crate already uses. ~12 new asserts (1 role check + 4 positive grants + 1 negative grant + intermediate setup).

2. **`crates/garraia-auth/tests/skeleton.rs`** (new) — validates `LoginPool` constructor positive + negative paths and the `InternalProvider` stub returns. 3 test functions, each spinning up its own pgvector testcontainer (because the `garraia-auth` crate doesn't share fixtures with workspace tests).

3. **Compile-time assertions** — the fact that `LoginPool` cannot be built from `PgPool` without going through `from_dedicated_config` is verified by the absence of `impl From<PgPool> for LoginPool` and the field being private. No runtime test needed; code review enforces it.

### What we are NOT testing in 391a

- Argon2id verify (no real verify_credential body)
- PBKDF2 dual-verify path
- JWT issuance
- audit_events insertion from auth code
- Cross-group authz (GAR-392)
- Session refresh
- Constant-time anti-enumeration
- account status checks
- Race conditions (no real UPDATE happens yet)

All of these are GAR-391b/c/d test scope.

---

## 9. Rollback plan

Three levels:

1. **Before merge:** close the PR.
2. **After merge, before 391b ships:** `git revert` the commit. Migration 008 file removed; `sqlx::migrate!()` no longer applies it on fresh installs. The `garraia_login` role persists on any DB that already ran the migration — harmless because no code uses it yet. To clean up: `DROP ROLE garraia_login` manually (idempotent role deletion).
3. **After 391b ships:** rollback requires forward-fix. Reverting 391a alone breaks 391b. A new migration `009_drop_login_role.sql` would `DROP ROLE garraia_login` and force 391b code to be reverted in the same PR.

Zero secrets. No destructive change. Crate addition is additive.

---

## 10. Risks & mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| `LoginPool::from_dedicated_config` validation query (`SELECT current_user`) succeeds but the role validation comparison is wrong | Low | **Critical** | Negative test in `skeleton.rs` validates that constructing with `postgres` superuser fails. |
| `from_dedicated_config` is bypassable via Rust reflection / `mem::transmute` | Negligible | High | Documented in §6 as forbidden. Code review enforces. Safe Rust prevents the construction without `unsafe`. |
| Migration 008 applies but the GRANTs are wrong (typo, missing privilege) | Medium | Medium | `has_table_privilege` smoke asserts validate every required grant. Negative assert validates the absence of access to `messages`. |
| `garraia_login` role created with `LOGIN` instead of `NOLOGIN` | Low | High | `pg_roles.rolcanlogin` smoke assert catches it. |
| `async-trait` dep version pin causes build failure on `rustc` upgrade | Low | Low | Pin `0.1` (current stable major), watch for breaking changes; standard in the ecosystem. |
| `testcontainers` fixture in `garraia-auth/tests/skeleton.rs` is slower than expected (each test spins its own container) | Medium | Low | 3 tests × ~7s container start = ~21s. Acceptable. Future optimization: shared container fixture across tests in 391b/c. |
| `Workspace::connect` call from `garraia-auth/tests/skeleton.rs` creates a circular dep risk (`garraia-auth` → `garraia-workspace` → ?) | Low | Medium | `garraia-auth` depends on `garraia-workspace` at the dev-deps level only (for tests). Production dep stays one-way. |
| 391b discovers the trait shape needs a 5th method | Medium | Low | Trait extension is a non-breaking change in async-trait if the new method has a default impl. Worst case: 391b plan documents a trait shape adjustment. |

---

## 11. Sequence of work (when approved)

### Wave 1 — scaffold + migration + types + LoginPool + InternalProvider stubs (~2-3h, single agent)

1. Create `crates/garraia-auth/Cargo.toml` with deps from §3.
2. Create `src/lib.rs`, `src/error.rs`, `src/types.rs`, `src/provider.rs`, `src/internal.rs`, `src/login_pool.rs` literally per §7.
3. Create `crates/garraia-workspace/migrations/008_login_role.sql` literally per §6.
4. Add `crates/garraia-auth` to workspace `members`.
5. Extend `crates/garraia-workspace/tests/migration_smoke.rs` with the asserts per §7.8.
6. Create `crates/garraia-auth/tests/skeleton.rs` per §7.7 with 3 tests.
7. Run `cargo clean -p garraia-workspace -p garraia-auth && cargo test -p garraia-workspace -p garraia-auth`. Iterate until green.
8. Run `cargo clippy -p garraia-auth --all-targets -- -D warnings` and `cargo clippy --workspace -- -D warnings`. Iterate.

### Wave 2 — parallel review (~25min wall, 2 agents background)

9. `@security-auditor` — focused on:
   - `LoginPool::from_dedicated_config` validation correctness (does it actually catch wrong roles?)
   - Migration 008 grants exact match against ADR 0005 §"Login role specification"
   - Negative grant assert (login role can't read messages) sufficient as evidence?
   - `AuthError::WrongRole` doesn't leak the database URL in its Display impl
   - `LoginPool::Debug` impl doesn't leak the inner pool credentials
   - Anti-pattern coverage from ADR 0005 §"Anti-patterns" honored in 391a (esp. #4 "Sharing the login pool with the main app pool")
   - Forward compat: are there any easy paths for a future PR to subvert the boundary?

10. `@code-reviewer` — focused on:
    - SQL correctness in migration 008
    - Rust idioms (no `unwrap()`, error propagation via `?`, naming conventions)
    - `async-trait` usage correct
    - `LoginConfig` validator derive works
    - Trait shape matches ADR 0005 exactly
    - Test fixture setup — does `Workspace::connect` from the auth crate work, or does it create a circular dep?
    - `cargo check --workspace --no-default-features` green

### Wave 3 — fixes + meta-files + commit + push (~30min, me)

11. Apply security + code findings inline.
12. Update `ROADMAP.md` §3.3 with the `[x]` items per §5.3.
13. Update `CLAUDE.md` "Estrutura de crates" moving `garraia-auth` to ativos.
14. Update `crates/garraia-workspace/README.md` mentioning migration 008.
15. Commit + push.
16. **Do NOT move GAR-391 to Done.** Comment in the issue linking the commit and listing 391b/c/d as pending.

**Total estimated: 3-5 hours.**

---

## 12. Definition of Done

- [ ] All §4 acceptance criteria checked.
- [ ] PR merged to `main`.
- [ ] Review verde from both `@security-auditor` and `@code-reviewer`.
- [ ] **GAR-391 NOT moved to Done** — comment added to the Linear issue noting that 391a (skeleton + login role) is merged and listing 391b/c/d as pending sub-issues.
- [ ] Final summary follows the new rule: "Linear — issues atualizadas" section with explicit ID, status, completedAt, and explicit flag for GAR-391 NOT being moved (with motive: 391a is one of four sub-slices).
- [ ] Next session can immediately start GAR-391b (real `verify_credential` + audit instrumentation) with no architectural ambiguity.

---

## 13. Open questions (preciso da sua resposta antes de começar)

1. **Migration 008 lives in `garraia-workspace/migrations/` ou `garraia-auth/migrations/`?** Recomendo **`garraia-workspace`** — consistente com o padrão existente (todas as migrations vivem lá), `sqlx::migrate!` já está wirado, smoke test extension é trivial. `garraia-auth` não tem seu próprio migrator e adicionar um agora dobra a complexity sem benefício. A role é workspace-level (afeta tabelas de workspace) e o ADR 0005 documenta isso. Confirma?

2. **`LoginPool::from_dedicated_config` validation method: `SELECT current_user` query OR query against `pg_roles` joined with the connection?** Recomendo **`SELECT current_user`** — mais simples, retorna exatamente o role da conexão atual, não precisa de subquery. Failure mode é claríssimo (`AuthError::WrongRole(actual)`). Confirma?

3. **`IdentityProvider` trait method bodies em 391a: `Err(AuthError::NotImplemented)` OR `unimplemented!()` macro?** Recomendo **`Err(AuthError::NotImplemented)`** — retorna erro estruturado em vez de panicar. Permite que callers de teste validem a falha sem `catch_unwind`. Confirma?

4. **`Principal` shape em 391a inclui `role: Option<String>` ou `role: Option<Role>` (enum tipado)?** Recomendo **`Option<String>`** em 391a — evita criar mais um enum só para descartar e re-criar em 391c. O enum `Role` (com variants Owner/Admin/Member/Guest/Child) entra em 391c quando o capability check `fn can(principal, action) -> bool` for implementado. Em 391a o `Principal` é só shape, não tem comportamento. Confirma?

5. **`Credential` enum em 391a: só `Internal { email, password }` OR também placeholder `OidcIdToken` para "documentar a porta aberta"?** Recomendo **só `Internal`** — adicionar variants vazios atrai linting `non_exhaustive` warnings e não documenta nada que o ADR 0005 já não documente. OIDC entra com seu próprio ADR (futuro 0009) + variant não-breaking. Confirma?

6. **`garraia-auth` tests usam testcontainer próprio OR compartilham o container do `garraia-workspace`?** Recomendo **container próprio em 391a** — `garraia-auth` deve ser um crate auto-contido para test purposes, mesmo que isso adicione ~15s de wall time para 3 tests. Future optimization (shared fixture) pode acontecer em 391c. Confirma?

7. **Adicionar `garraia-auth` na descrição do `garraia-workspace`/`AppState` agora OR esperar 391c?** Recomendo **esperar 391c** — em 391a o crate existe mas o gateway não usa. Wiring é responsabilidade de 391c quando o `Principal` extractor entrar. Confirma?

---

## 14. Impact on other components

### 14.1 GAR-391b (real `verify_credential` + audit + JWT issuance)

391a entrega o trait shape, o `LoginPool`, e a migration. 391b implementa:
- Real `verify_credential` body com PBKDF2/Argon2id dual-verify + `SELECT ... FOR NO KEY UPDATE OF ui` + `users.status = 'active'` check + audit_events insertion + lazy upgrade UPDATE
- Real `find_by_provider_sub` body
- Real `create_identity` body
- `audit_login` helper function with `RequestCtx` parameter
- `DUMMY_HASH` constant + constant-time anti-enumeration path
- JWT HS256 issuance + opaque random refresh token storage
- `AuthError::AccountSuspended`, `AuthError::AccountDeleted` variants if needed
- Login endpoint `POST /v1/auth/login` (or wherever the gateway routes auth)

391b can start immediately after 391a merges with zero refactor of trait/types.

### 14.2 GAR-391c (Axum extractor + RequirePermission + gateway wiring)

391c implements:
- `impl FromRequestParts<Arc<AppState>> for Principal` extracting JWT from `Authorization: Bearer ...` header
- `RequirePermission(Action)` extractor for route-level capability check
- `Role` enum (replacing the `Option<String>` placeholder in `Principal`)
- `fn can(principal: &Principal, action: Action) -> bool` implementation
- `RequestCtx` extraction from headers (`X-Forwarded-For`, `User-Agent`, `X-Request-ID`)
- AppState wiring to inject `Arc<dyn IdentityProvider>` into the gateway

391c can start immediately after 391b ships.

### 14.3 GAR-391d / GAR-392 (cross-group authz test suite)

391d/392 ships the 100+ scenario test suite documented in plan 0002 §3.3. This is a separate large chunk of work that depends on 391a+b+c being in place.

### 14.4 GAR-413 (`garraia-cli migrate workspace`)

GAR-413 unblocks once `garraia-auth::AuthError` and the `user_identities` schema are stable (already true via migration 001 + ADR 0005). The migration tool doesn't depend on `garraia-auth` directly — it runs as superuser and bypasses RLS.

---

## 15. Next recommended issue (after GAR-391a merged)

**GAR-391b — `verify_credential` real implementation + audit + JWT issuance.**

Estimated 5-8 hours. Will need its own plan (`plans/0011-gar-391b-verify-credential-impl.md`) covering:

- Real `verify_credential` with `SELECT ... FOR NO KEY UPDATE OF ui` row lock (per security review H1 of ADR 0005)
- PBKDF2 → Argon2id dual-verify + lazy upgrade in same transaction
- `users.status = 'active'` check + `AccountSuspended` error
- Audit instrumentation with `RequestCtx` (per ADR 0005 M1)
- `DUMMY_HASH` constant generated at build time + constant-time path
- JWT HS256 issuance via `jsonwebtoken` crate + 15-min access token + 30-day opaque refresh token (HMAC-SHA256 hash in `sessions`)
- `audit_events` actions: `login.success`, `login.failure_*`, `login.password_hash_upgraded`
- Anti-enumeration via dummy hash for both `failure_user_not_found` and `failure_account_suspended` (same timing profile)

391b is the largest of the four sub-slices (most logic, most risk) and benefits from 391a being in place to focus on the verification path without distractions.

**Alternative:** GAR-374 (ADR 0004 Object Storage) if you prefer a research detour. Still recommend 391b first — auth ROI is higher.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados e executo seguindo o §11. Se quiser cortar escopo (ex.: "skip o negative grant assert", "deixa LoginPool::from_dedicated_config para 391b"), me diga antes que eu toque em código.
