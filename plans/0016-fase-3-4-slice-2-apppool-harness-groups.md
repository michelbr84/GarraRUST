# Plan 0016 — Fase 3.4 Slice 2: AppPool + gateway test harness + authed `/v1/me` + `/v1/groups` skeleton

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-WS-API](https://linear.app/chatgpt25/team/GAR) (Fase 3.4 / API REST `/v1` + OpenAPI) — Slice 2 of the epic. Precondition for plan 0014 (GAR-391d app-layer cross-group matrix) becomes actually satisfied after **M3** of this plan ships.

**Status:** ✅ Merged 2026-04-15 (Florida) — M1 `3d2fc66` (PR #10), M2 `a6c337d` (PR #12), M3 `b139861` (PR #13), M4 `860312b` (PR #15), M5 `1404365` (PR #20). Corpo formal do plano (Tasks 0–17) entregue integralmente. Itens extras do comentário de índice original ficaram fora do escopo por decisão operacional 2026-04-15 — ver `plans/README.md`.

**Goal:** Entregar a infra de pool RLS-enforced (`AppPool`), o harness de integração do `garraia-gateway` reusável com Postgres real, a cobertura autenticada completa do `GET /v1/me`, e o primeiro par write/read handler de `/v1/groups` — resolvendo simultaneamente os follow-ups do review formal do PR #8.

**Architecture:**
1. Novo newtype `AppPool` em `garraia-auth` (irmão de `LoginPool`/`SignupPool`) que encapsula o pool `garraia_app` (NOLOGIN BYPASS → RLS-enforced via `FORCE ROW LEVEL SECURITY`). Acesso controlado (`pub(crate)` para o inner), `!Clone` assegurado via `static_assertions`, logging redacted via `Debug` manual.
2. `garraia-config::AppConfig` ganha campo `GARRAIA_APP_DATABASE_URL` (fail-soft, opcional).
3. `AppState` ganha `Option<Arc<AppPool>>`, wired no `server.rs` junto com os outros pools; `RestV1State` ganha `FromRef<Arc<AppPool>>`.
4. Harness de testes em `crates/garraia-gateway/tests/common/mod.rs` boota pgvector/pg16 via `testcontainers`, aplica migrations 001..010 via `garraia-workspace`, constrói `AppState` com os 3 pools + `JwtIssuer` determinístico, e devolve um `Router` pronto via `GatewayServer::build_router()` (novo getter) — sem subir socket de rede.
5. `serial_test` isola os testes que mutam env vars.
6. Handlers `POST /v1/groups` e `GET /v1/groups/{id}` usam `AppPool`, setam `SET LOCAL app.current_user_id = $1` antes de cada operação escopada, INSERT em `groups` (sem RLS) + INSERT em `group_members` (sob RLS, WITH CHECK policy autoriza `user_id = current setting`).
7. OpenAPI ganha `SecurityScheme::Bearer` registrado via `Modify` impl; nova rota `GET /docs/{*path}` na branch fail-soft (M-1 do review).

**Tech Stack:** Axum 0.8, `utoipa 5`, `utoipa-swagger-ui 9`, `garraia-auth` (novo `AppPool`), `garraia-workspace` (migrations 001..010 já existentes), `sqlx 0.8` (postgres/uuid/chrono), `testcontainers 0.23` + `testcontainers-modules 0.11` (já em dev-deps do gateway desde plan 0012), `serial_test 3` (novo dev-dep), `static_assertions`, `secrecy`.

**Milestones (marcos internos do plano):**
- **M1 — Infra foundation (Tasks 0–5):** `AppPool` newtype, `AppConfig` env var, wiring em `AppState`, `FromRef` em `RestV1State`. Zero endpoint novo.
- **M2 — Gateway test harness (Tasks 6–8):** `tests/common/mod.rs` com boot do container + seed helper + JWT issuer exposto; migração do teste fail-soft existente para usar `serial_test`; sem teste novo.
- **M3 — Authed `/v1/me` (Tasks 9–11):** 4 testes de integração autenticados + OpenAPI `SecurityScheme::Bearer` (fix N-2 do review) + teste wire do `/v1/openapi.json`.
- **M4 — `/v1/groups` skeleton (Tasks 12–15):** `POST /v1/groups`, `GET /v1/groups/{id}`, 6 testes de integração (happy/401/403/404/duplicate/validation).
- **M5 — Review follow-ups residuais (Tasks 16–17):** M-1 fail-soft `/docs/{*path}`, M-3 nit (`unconfigured_handler` → `impl IntoResponse`), H-1 doc note em `RestError::Internal`, N-3 log de fallback em `problem.rs`.

**Out of scope (fica para plan 0017+):**
- Outros endpoints de §3.4 (`PATCH /v1/groups/{id}`, invites, member role changes, chats, messages, memory, tasks)
- `files:*` (bloqueado por ADR 0004)
- Rate limiting dedicado `/v1`
- Contract tests via `schemathesis`
- Migration nova (plan 0016 reusa 001..010 existentes)

**Rollback plan:** Aditivo por task. Cada task é um commit independente; `git revert` commit-a-commit desfaz. `AppPool` é uma nova struct — se removida, nada externo a `garraia-auth`/`garraia-gateway` quebra (ainda não há caller em produção antes do merge). Harness de teste é isolado em `tests/common/`. Migrations não são tocadas.

**Pré-condição validada:** migration 007 já cria `garraia_app` NOLOGIN + FORCE RLS em 10 tabelas; o harness em `garraia-auth/tests/common/harness.rs` já demonstra que o `garraia_app` role pode ser promovido a LOGIN + ter `SET LOCAL app.current_user_id` funcionando contra `group_members` (validado empiricamente pela matriz RLS do plan 0013, 81 cenários). Este plano apenas formaliza em código de produção o que os testes do 0013 já provam operacionalmente.

**§12 Open questions (pré-start):**
1. **RLS WITH CHECK em `group_members` INSERT** — confirmar empiricamente no M1 que `SET LOCAL app.current_user_id = $creator; INSERT INTO group_members (group_id, user_id, role) VALUES ($new_group, $creator, 'owner')` é aceito pela policy atual. Se não for, adicionar policy ou usar `garraia_signup`/nova role dedicada. Verificação: `grep -n "group_members" crates/garraia-workspace/migrations/007_row_level_security.sql`.
2. **Promoção `garraia_app` NOLOGIN → LOGIN em produção** — o harness de teste faz `ALTER ROLE garraia_app LOGIN PASSWORD '...'`. Em produção, a decisão de se esse role deve ser LOGIN ou manter NOLOGIN + ser acessado via `SET ROLE` precisa ser documentada. Por ora o plan assume LOGIN em dev e defer a decisão de prod para um ADR amendment em plan 0017.
3. **Versão exata do `serial_test`** — fixar `serial_test = "3"` na root `Cargo.toml` como workspace dep.

---

## File Structure

**Criar:**
- `crates/garraia-auth/src/app_pool.rs` — newtype `AppPool` + `AppConfig`
- `crates/garraia-gateway/tests/common/mod.rs` — harness compartilhado
- `crates/garraia-gateway/tests/common/fixtures.rs` — seed helpers (user + group + JWT)
- `crates/garraia-gateway/tests/rest_v1_groups.rs` — testes integration `POST/GET /v1/groups`
- `crates/garraia-gateway/src/rest_v1/groups.rs` — handlers `POST/GET /v1/groups`

**Modificar:**
- `Cargo.toml` (workspace root) — adicionar `serial_test = "3"` nas workspace deps
- `crates/garraia-auth/src/lib.rs` — export `AppPool`, `AppConfig`
- `crates/garraia-config/src/lib.rs` (ou onde `AuthConfig` mora) — adicionar `GARRAIA_APP_DATABASE_URL` ao parser
- `crates/garraia-gateway/Cargo.toml` — adicionar `serial_test` a `[dev-dependencies]`
- `crates/garraia-gateway/src/state.rs` — `Option<Arc<AppPool>>` + `set_auth_components` ganha 4º arg
- `crates/garraia-gateway/src/server.rs` — construir `AppPool` junto dos outros dois
- `crates/garraia-gateway/src/rest_v1/mod.rs` — `RestV1State` ganha `app_pool: Arc<AppPool>` + novo `FromRef` + rota `GET /docs/{*path}` fail-soft + `POST /v1/groups` + `GET /v1/groups/{id}`
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — `SecurityAddon` modifier registrando `bearer`
- `crates/garraia-gateway/src/rest_v1/problem.rs` — doc note `.context()` policy + `tracing::warn!` no fallback
- `crates/garraia-gateway/tests/rest_v1_me.rs` — adicionar 4 testes autenticados + `#[serial]`
- `crates/garraia-gateway/src/lib.rs` — expor `build_router()` ou helper equivalente para o harness
- `plans/README.md` — adicionar entrada `0016`
- `ROADMAP.md` — marcar `[x] POST /v1/groups` e `[x] GET /v1/groups/{group_id}` em §3.4 Grupos (apenas quando M4 mergear)

**NÃO tocar:** migrations (reuso puro), `CLAUDE.md` (sem nova regra operacional), `docs/adr/*` (sem ADR amendment nesta slice), `mobile_auth.rs`, `mobile_chat.rs`, `auth_routes.rs`, `garraia-workspace` crate.

---

## M1 — Infra foundation

### Task 0: Registrar `0016` no índice `plans/README.md`

**Files:**
- Modify: `plans/README.md`

- [ ] **Step 1: Adicionar a linha do 0016**

Inserir após a linha atual do `0015`:

```markdown
| 0016 | [Fase 3.4 — Slice 2: AppPool + harness + authed `/v1/me` + `/v1/groups` skeleton](0016-fase-3-4-slice-2-apppool-harness-groups.md) | GAR-WS-API (destrava definitivamente GAR-391d) | ⏳ Aprovado 2026-04-14 |
```

- [ ] **Step 2: Commit**

```bash
git add plans/README.md plans/0016-fase-3-4-slice-2-apppool-harness-groups.md
git commit -m "docs(plans): add plan 0016 (Fase 3.4 slice 2)"
```

---

### Task 1: `AppPool` newtype em `garraia-auth`

**Files:**
- Create: `crates/garraia-auth/src/app_pool.rs`
- Modify: `crates/garraia-auth/src/lib.rs`

**Referência obrigatória:** abrir `crates/garraia-auth/src/login_pool.rs` e `crates/garraia-auth/src/signup_pool.rs` antes de escrever. O `AppPool` é irmão simétrico: mesmo padrão `static_assertions::assert_not_impl_any`, mesmo `Debug` manual redacted, mesma API (`from_dedicated_config`, `pool()` `pub(crate)`, `raw()` feature-gated para testes).

- [ ] **Step 1: Escrever o teste de round-trip primeiro (RED)**

`crates/garraia-auth/tests/app_pool_smoke.rs`:

```rust
//! AppPool smoke — validates the newtype can connect to a pgvector
//! container promoted garraia_app role and execute a SELECT 1.

mod common;
use common::harness::Harness;

#[tokio::test]
async fn app_pool_from_dedicated_config_connects_and_selects_one() {
    let h = Harness::get().await;
    // Harness already holds a raw `app_pool: PgPool`. Re-use its
    // admin_url + credentials to build an AppPool via the new
    // `AppPool::from_dedicated_config` API. The test fails if that API
    // does not exist yet.
    use garraia_auth::{AppConfig, AppPool};
    let cfg = AppConfig {
        database_url: h.admin_url.replace("postgres", "garraia_app").into(),
        max_connections: 4,
    };
    let pool = AppPool::from_dedicated_config(&cfg).await.expect("connect");
    let (one,): (i32,) = sqlx::query_as("SELECT 1").fetch_one(pool.pool()).await.unwrap();
    assert_eq!(one, 1);
}
```

> Nota: `pool()` será `pub(crate)`, então o teste precisa estar **dentro** do crate `garraia-auth`. Alternativa: expor um método `pub fn is_connected(&self) -> bool` que faz o SELECT internamente e retorna `true/false`. **Preferir a alternativa** para manter o `pool()` selado. Reescrever o teste para usar `pool.is_connected().await`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p garraia-auth --test app_pool_smoke`
Expected: FAIL — `AppPool` não existe.

- [ ] **Step 3: Implementar `app_pool.rs`**

```rust
//! `AppPool` — typed newtype over the `garraia_app` RLS-enforced role.
//!
//! Symmetric to `LoginPool` and `SignupPool` (ADR 0005). The inner pool
//! connects as `garraia_app`, which is NOLOGIN in production migrations
//! and must be promoted to LOGIN by an operator (dev/test) or accessed
//! via `SET ROLE` in prod. Tenant context is set per-transaction with
//! `SET LOCAL app.current_user_id = $1` before any RLS-scoped query.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::Row;
use static_assertions::assert_not_impl_any;
use thiserror::Error;

/// Configuration for the `garraia_app` pool. `database_url` is a full
/// Postgres connection string including the `garraia_app` role name.
#[derive(Debug)]
pub struct AppConfig {
    pub database_url: SecretString,
    pub max_connections: u32,
}

/// Typed newtype over a `PgPool` wired as `garraia_app`. The inner pool
/// is `pub(crate)` — downstream code must go through `pool()` which is
/// also `pub(crate)`, so `garraia-gateway` accesses the raw pool only
/// via the crate-internal re-export in `garraia-auth::pool_access`.
/// Cloning is forbidden at the type level: wrap in `Arc<AppPool>`.
pub struct AppPool {
    inner: PgPool,
}

assert_not_impl_any!(AppPool: Clone);

impl fmt::Debug for AppPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppPool")
            .field("inner", &"[REDACTED PgPool]")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum AppPoolError {
    #[error("failed to parse database URL")]
    Parse(#[source] sqlx::Error),
    #[error("failed to connect app pool")]
    Connect(#[source] sqlx::Error),
    #[error("expected role garraia_app, got {0}")]
    WrongRole(String),
}

impl AppPool {
    /// Build from a dedicated config. Validates the connected role is
    /// literally `garraia_app` via `SELECT current_user` — this mirrors
    /// the validation already done by `LoginPool::from_dedicated_config`.
    pub async fn from_dedicated_config(cfg: &AppConfig) -> Result<Self, AppPoolError> {
        let opts: PgConnectOptions = cfg
            .database_url
            .expose_secret()
            .parse()
            .map_err(AppPoolError::Parse)?;
        let inner = PgPoolOptions::new()
            .max_connections(cfg.max_connections)
            .connect_with(opts)
            .await
            .map_err(AppPoolError::Connect)?;

        let row = sqlx::query("SELECT current_user::text")
            .fetch_one(&inner)
            .await
            .map_err(AppPoolError::Connect)?;
        let current: String = row.get(0);
        if current != "garraia_app" {
            return Err(AppPoolError::WrongRole(current));
        }
        Ok(Self { inner })
    }

    /// Crate-internal raw pool accessor. Consumers in `garraia-gateway`
    /// use this via the `pool_access` re-export.
    pub(crate) fn pool(&self) -> &PgPool {
        &self.inner
    }

    /// Convenience: `SELECT 1` round-trip. Used by smoke tests only.
    #[doc(hidden)]
    pub async fn is_connected(&self) -> bool {
        sqlx::query("SELECT 1")
            .fetch_one(&self.inner)
            .await
            .is_ok()
    }
}
```

- [ ] **Step 4: Expor no `lib.rs`**

`crates/garraia-auth/src/lib.rs` — adicionar:

```rust
pub mod app_pool;
pub use app_pool::{AppConfig, AppPool, AppPoolError};
```

Nota: evitar colisão — `AppConfig` já existe em `garraia-config` como struct grande. Renomear a struct local para `AppPoolConfig` se houver conflito durante `cargo check`.

- [ ] **Step 5: Rodar o teste (GREEN)**

Run: `cargo test -p garraia-auth --test app_pool_smoke`
Expected: PASS.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p garraia-auth -- -D warnings`
Expected: sem warnings novos.

- [ ] **Step 7: Commit**

```bash
git add crates/garraia-auth/src/app_pool.rs crates/garraia-auth/src/lib.rs crates/garraia-auth/tests/app_pool_smoke.rs
git commit -m "feat(auth): add AppPool newtype for garraia_app role (plan 0016 t1)"
```

---

### Task 2: Extender `AuthConfig` com `app_database_url`

**Files:**
- Modify: `crates/garraia-config/src/lib.rs` (ou módulo específico de `AuthConfig`)

- [ ] **Step 1: Localizar `AuthConfig::from_env`**

Run: `rg -n "GARRAIA_LOGIN_DATABASE_URL" crates/garraia-config/src/`
Abrir o arquivo e adicionar o parsing da nova env var `GARRAIA_APP_DATABASE_URL`, seguindo o mesmo padrão das duas existentes.

- [ ] **Step 2: Adicionar o campo + parser**

```rust
pub struct AuthConfig {
    // ...campos existentes
    pub app_database_url: SecretString,
}

impl AuthConfig {
    pub fn from_env() -> Option<Self> {
        // ...leituras existentes
        let app_database_url = std::env::var("GARRAIA_APP_DATABASE_URL").ok()?.into();
        Some(Self {
            // ...campos existentes
            app_database_url,
        })
    }
}
```

> **Ponto de atenção sobre fail-soft:** hoje `AuthConfig::from_env` retorna `None` se qualquer uma das 4 env vars estiver ausente. Adicionar uma 5ª significa que gateways que ainda não configuraram `GARRAIA_APP_DATABASE_URL` vão entrar em fail-soft mesmo tendo `JWT_SECRET` + `REFRESH_HMAC` + `LOGIN_DB` + `SIGNUP_DB` setados — isso é um soft breaking change operacional. **Decisão:** manter o novo campo como `Option<SecretString>` e wirar `AppPool` só quando presente. Os handlers `/v1/groups` retornam 503 se `app_pool` for `None`. Isso preserva compat.

Revisar o código para: `pub app_database_url: Option<SecretString>`.

- [ ] **Step 3: Verificar build**

Run: `cargo check -p garraia-config && cargo check -p garraia-gateway`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-config/src/
git commit -m "feat(config): add optional GARRAIA_APP_DATABASE_URL (plan 0016 t2)"
```

---

### Task 3: Wire `AppPool` no `AppState` + `server.rs`

**Files:**
- Modify: `crates/garraia-gateway/src/state.rs`
- Modify: `crates/garraia-gateway/src/server.rs`

- [ ] **Step 1: Adicionar o campo em `AppState`**

Em `state.rs`, após `login_pool: Option<Arc<LoginPool>>`:

```rust
/// Shared `Arc<AppPool>` for `/v1` handlers that exercise the
/// RLS-enforced `garraia_app` pool. `pub(crate)` per security review
/// L-3 — only rest_v1 handlers access it, and always via the
/// `RestV1State` sub-state.
pub(crate) app_pool: Option<Arc<AppPool>>,
```

Inicialização em `AppState::new`: `app_pool: None`.

- [ ] **Step 2: Estender `set_auth_components`**

Mudar assinatura para aceitar `Option<Arc<AppPool>>`:

```rust
pub fn set_auth_components(
    &mut self,
    login_pool: Arc<LoginPool>,
    signup_pool: Arc<SignupPool>,
    jwt_issuer: Arc<JwtIssuer>,
    app_pool: Option<Arc<AppPool>>,
) {
    // ...lógica existente
    self.app_pool = app_pool;
    // ...
}
```

- [ ] **Step 3: Construir `AppPool` em `server.rs`**

Localizar o bloco `match garraia_config::AuthConfig::from_env()` (`state.rs:186`-ish conforme o plan 0015 já mapeou). Adicionar:

```rust
let app_pool_result = match cfg.app_database_url.as_ref() {
    Some(url) => {
        let cfg = garraia_auth::AppConfig {
            database_url: url.clone(),
            max_connections: 10,
        };
        match garraia_auth::AppPool::from_dedicated_config(&cfg).await {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::warn!(error = %e, "AppPool connect failed; /v1/groups will be 503");
                None
            }
        }
    }
    None => None,
};
```

Passar `app_pool_result` como 4º argumento de `set_auth_components`.

- [ ] **Step 4: Verificar build**

Run: `cargo check -p garraia-gateway`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/state.rs crates/garraia-gateway/src/server.rs
git commit -m "feat(gateway): wire AppPool into AppState (plan 0016 t3)"
```

---

### Task 4: `RestV1State` ganha `app_pool` + `FromRef`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1: Adicionar campo + FromRef + from_app_state**

```rust
use garraia_auth::AppPool;

#[derive(Clone)]
pub struct RestV1State {
    pub jwt_issuer: Arc<JwtIssuer>,
    pub login_pool: Arc<LoginPool>,
    pub app_pool: Arc<AppPool>,
}

impl RestV1State {
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            jwt_issuer: app.jwt_issuer.clone()?,
            login_pool: app.login_pool.clone()?,
            app_pool: app.app_pool.clone()?,
        })
    }
}

impl FromRef<RestV1State> for Arc<AppPool> {
    fn from_ref(s: &RestV1State) -> Self {
        s.app_pool.clone()
    }
}
```

> **Impacto operacional importante:** agora `RestV1State::from_app_state` retorna `None` se **qualquer um** dos 3 pools estiver ausente. Gateways que só configuraram `LOGIN_DB` + `SIGNUP_DB` (sem `APP_DB`) vão entrar em fail-soft para **todas** as rotas `/v1` — incluindo `/v1/me` que funcionava antes. **Mitigação:** separar em dois sub-states, `RestV1AuthState` (só jwt + login, para `/v1/me`) e `RestV1FullState` (com app_pool, para `/v1/groups`). Rotas escolhem qual state precisam. **Esta decisão é obrigatória** e fica nesta task.

Refinamento:

```rust
#[derive(Clone)]
pub struct RestV1AuthState {
    pub jwt_issuer: Arc<JwtIssuer>,
    pub login_pool: Arc<LoginPool>,
}

#[derive(Clone)]
pub struct RestV1FullState {
    pub auth: RestV1AuthState,
    pub app_pool: Arc<AppPool>,
}

impl FromRef<RestV1AuthState> for Arc<JwtIssuer> { /* ... */ }
impl FromRef<RestV1AuthState> for Arc<LoginPool> { /* ... */ }
impl FromRef<RestV1FullState> for Arc<JwtIssuer> { /* ... */ }
impl FromRef<RestV1FullState> for Arc<LoginPool> { /* ... */ }
impl FromRef<RestV1FullState> for Arc<AppPool>   { /* ... */ }
```

O `router()` passa a decidir entre 3 modos:
1. auth + app disponíveis → `/v1/me` + `/v1/groups/*` + `/docs`
2. só auth disponível → `/v1/me` + `/docs` (com `/v1/groups/*` em 503)
3. nada disponível → tudo em 503

- [ ] **Step 2: Refatorar `router()` para os 3 modos**

```rust
pub fn router(app_state: Arc<AppState>) -> Router {
    let auth = RestV1AuthState::from_app_state(&app_state);
    let full = auth.as_ref().and_then(|a| {
        app_state.app_pool.as_ref().map(|p| RestV1FullState {
            auth: a.clone(),
            app_pool: p.clone(),
        })
    });

    match (full, auth) {
        (Some(full), _) => {
            let me_router: Router = Router::new()
                .route("/v1/me", get(me::get_me))
                .with_state(full.auth.clone());
            let groups_router: Router = Router::new()
                .route("/v1/groups", post(groups::create_group))
                .route("/v1/groups/{id}", get(groups::get_group))
                .with_state(full);
            me_router
                .merge(groups_router)
                .merge(SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        }
        (None, Some(auth)) => {
            let me_router: Router = Router::new()
                .route("/v1/me", get(me::get_me))
                .with_state(auth);
            me_router
                .route("/v1/groups", post(unconfigured_handler))
                .route("/v1/groups/{id}", get(unconfigured_handler))
                .merge(SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()))
        }
        (None, None) => Router::new()
            .route("/v1/me", get(unconfigured_handler))
            .route("/v1/groups", post(unconfigured_handler))
            .route("/v1/groups/{id}", get(unconfigured_handler))
            .route("/v1/openapi.json", get(unconfigured_handler))
            .route("/docs", get(unconfigured_handler))
            .route("/docs/{*rest}", get(unconfigured_handler)),
    }
}
```

Nota: `/docs/{*rest}` resolve M-1 do review formal do PR #8 (trailing slash + assets estáticos).

- [ ] **Step 3: Atualizar `me::get_me` para `State<RestV1AuthState>`**

No `me.rs`:

```rust
pub async fn get_me(
    State(_state): State<RestV1AuthState>,
    principal: Principal,
) -> Result<Json<MeResponse>, RestError> { /* inalterado */ }
```

- [ ] **Step 4: Verificar build**

Run: `cargo check -p garraia-gateway`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/
git commit -m "feat(gateway): split RestV1State into AuthState/FullState (plan 0016 t4)"
```

---

### Task 5: Consolidação M1 — cargo check workspace

- [ ] **Step 1: Validar o workspace inteiro**

Run: `cargo check --workspace 2>&1 | tail -5`
Expected: `Finished` sem error.

- [ ] **Step 2: Rodar a suíte que já existia**

Run: `cargo test -p garraia-gateway --lib && cargo test -p garraia-gateway --test rest_v1_me && cargo test -p garraia-auth --test app_pool_smoke`
Expected: todos PASS. Nenhuma regressão.

Sem commit aqui — validação pura. M1 fechada.

---

## M2 — Gateway test harness

### Task 6: `tests/common/mod.rs` com harness compartilhado

**Files:**
- Create: `crates/garraia-gateway/tests/common/mod.rs`
- Modify: `Cargo.toml` (workspace root) — `serial_test = "3"`
- Modify: `crates/garraia-gateway/Cargo.toml` — `serial_test = { workspace = true }`

**Referência obrigatória:** copiar como base `crates/garraia-auth/tests/common/harness.rs`. O harness do gateway é **uma superset**: container + migrations + 3 pools + `AppState` + `Router` + `JwtIssuer` exposto para seed.

- [ ] **Step 1: Adicionar `serial_test` às deps**

Root `Cargo.toml`:

```toml
[workspace.dependencies]
# ...existentes
serial_test = "3"
```

`crates/garraia-gateway/Cargo.toml` em `[dev-dependencies]`:

```toml
serial_test = { workspace = true }
```

- [ ] **Step 2: Criar `tests/common/mod.rs`**

```rust
//! Integration test harness for garraia-gateway (plan 0016 M2).
//!
//! Process-wide shared testcontainer (pgvector/pg16), migrations 001..010
//! applied once, three typed pools (login/signup/app), JwtIssuer exposed,
//! and a pre-built `axum::Router` ready for `tower::ServiceExt::oneshot`.

#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use garraia_auth::{
    AppConfig as AuthPoolAppConfig, AppPool, JwtIssuer, LoginConfig, LoginPool,
    SignupConfig, SignupPool,
};
use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres as PgImage;
use tokio::sync::OnceCell;

pub mod fixtures;

static SHARED: OnceCell<Arc<Harness>> = OnceCell::const_new();

pub struct Harness {
    _container: ContainerAsync<PgImage>,
    pub admin_url: String,
    pub admin_pool: PgPool,
    pub login_pool: Arc<LoginPool>,
    pub signup_pool: Arc<SignupPool>,
    pub app_pool: Arc<AppPool>,
    pub jwt: Arc<JwtIssuer>,
    pub router: Router,
}

impl Harness {
    pub async fn get() -> Arc<Self> {
        SHARED
            .get_or_init(|| async { Arc::new(Self::boot().await.expect("harness boot")) })
            .await
            .clone()
    }

    async fn boot() -> anyhow::Result<Self> {
        // 1. Boot pgvector/pg16.
        let container = PgImage::default()
            .with_name("pgvector/pgvector")
            .with_tag("pg16")
            .start()
            .await?;
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let admin_pool = PgPool::connect(&admin_url).await?;

        // 2. Apply migrations 001..010 via garraia-workspace.
        garraia_workspace::Workspace::connect(&garraia_workspace::WorkspaceConfig {
            database_url: admin_url.clone().into(),
            max_connections: 4,
        })
        .await?;

        // 3. Promote garraia_app / garraia_login / garraia_signup to LOGIN
        //    with deterministic passwords. Exactly mirrors the pattern
        //    already validated by `crates/garraia-auth/tests/common/harness.rs`.
        for (role, pw) in [
            ("garraia_app", "test_app_pw"),
            ("garraia_login", "test_login_pw"),
            ("garraia_signup", "test_signup_pw"),
        ] {
            sqlx::query(&format!("ALTER ROLE {role} LOGIN PASSWORD '{pw}'"))
                .execute(&admin_pool)
                .await?;
        }

        // 4. Build the three typed pools.
        let app_pool = Arc::new(
            AppPool::from_dedicated_config(&AuthPoolAppConfig {
                database_url: format!("postgres://garraia_app:test_app_pw@{host}:{port}/postgres").into(),
                max_connections: 4,
            })
            .await?,
        );
        let login_pool = Arc::new(
            LoginPool::from_dedicated_config(&LoginConfig {
                database_url: format!("postgres://garraia_login:test_login_pw@{host}:{port}/postgres").into(),
                max_connections: 4,
            })
            .await?,
        );
        let signup_pool = Arc::new(
            SignupPool::from_dedicated_config(&SignupConfig {
                database_url: format!("postgres://garraia_signup:test_signup_pw@{host}:{port}/postgres").into(),
                max_connections: 4,
            })
            .await?,
        );

        // 5. Build JwtIssuer with a deterministic secret — same pattern
        //    as the auth_integration tests.
        let jwt = Arc::new(JwtIssuer::new_for_test("unit-test-jwt-secret-at-least-32-bytes"));

        // 6. Build AppState + Router via GatewayServer::build_router_for_test.
        let mut config = AppConfig::default();
        config.memory.enabled = false;
        let router = GatewayServer::build_router_for_test(
            config,
            login_pool.clone(),
            signup_pool.clone(),
            Some(app_pool.clone()),
            jwt.clone(),
        )
        .await?;

        Ok(Self {
            _container: container,
            admin_url,
            admin_pool,
            login_pool,
            signup_pool,
            app_pool,
            jwt,
            router,
        })
    }
}
```

> **Dependência crítica:** `GatewayServer::build_router_for_test` **não existe** hoje. Adicionar essa função em `crates/garraia-gateway/src/server.rs` como parte desta task. Ela deve fazer o mesmo caminho que `GatewayServer::new().run()` faz para construir o router, mas parar antes do `axum::serve` e retornar `Router`. `#[cfg(any(test, feature = "test-helpers"))]` para não vazar em binário prod, ou mais simples: `pub(crate)` + `#[doc(hidden)]` + chamada através de um wrapper em `garraia-gateway::test_support`.

Implementação sugerida em `server.rs` — **pública via feature** `test-helpers`:

```rust
#[cfg(feature = "test-helpers")]
pub async fn build_router_for_test(
    config: AppConfig,
    login_pool: Arc<LoginPool>,
    signup_pool: Arc<SignupPool>,
    app_pool: Option<Arc<AppPool>>,
    jwt: Arc<JwtIssuer>,
) -> anyhow::Result<axum::Router> {
    // Build AppState manually, call set_auth_components, return router().
    let state = Arc::new({
        let mut s = AppState::new(config, /* minimal runtime */, /* minimal channels */);
        s.set_auth_components(login_pool, signup_pool, jwt, app_pool);
        s
    });
    Ok(crate::router::build_router(state, /* defaults */))
}
```

Adicionar feature ao `Cargo.toml`:

```toml
[features]
test-helpers = []
```

Harness usa `garraia-gateway = { workspace = true, features = ["test-helpers"] }` em dev-deps.

- [ ] **Step 3: Criar `tests/common/fixtures.rs`**

```rust
//! Seed helpers for integration tests.

use uuid::Uuid;

use super::Harness;

/// Creates a user in the `users` table (via admin_pool to bypass RLS),
/// creates a `group`, creates a `group_members` row with role='owner',
/// and returns (user_id, group_id, jwt_token).
pub async fn seed_user_with_group(
    h: &Harness,
    email: &str,
) -> (Uuid, Uuid, String) {
    let user_id = Uuid::new_v4();
    let group_id = Uuid::new_v4();
    // INSERT via admin_pool — fixture setup is allowed to bypass RLS.
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status, created_at) \
         VALUES ($1, $2, 'Test User', 'active', now())",
    )
    .bind(user_id)
    .bind(email)
    .execute(&h.admin_pool)
    .await
    .expect("insert user");

    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by, created_at, updated_at) \
         VALUES ($1, 'Test Group', 'team', $2, now(), now())",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&h.admin_pool)
    .await
    .expect("insert group");

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, joined_at) \
         VALUES ($1, $2, 'owner', 'active', now())",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&h.admin_pool)
    .await
    .expect("insert group_members");

    let token = h.jwt.issue_access_for_test(user_id).expect("issue jwt");
    (user_id, group_id, token)
}
```

> **Dependência:** `JwtIssuer::new_for_test(secret)` e `JwtIssuer::issue_access_for_test(user_id)` não existem. Adicionar ambos em `garraia-auth` como métodos `#[doc(hidden)] pub fn` atrás de `#[cfg(any(test, feature = "test-support"))]`. Documentar que são restritos a testes.

- [ ] **Step 4: Validar o harness**

Run: `cargo test -p garraia-gateway --test rest_v1_me -- --nocapture`
Expected: o teste fail-soft existente ainda passa (porque `AppConfig::default()` não seta `APP_DB` env var → continua fail-soft). Não há teste novo ainda — só o harness foi criado.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/tests/common/ crates/garraia-gateway/Cargo.toml Cargo.toml crates/garraia-gateway/src/server.rs crates/garraia-auth/src/jwt.rs
git commit -m "test(gateway): add shared integration harness (plan 0016 t6)"
```

---

### Task 7: `rest_v1_me.rs` ganha `#[serial]` + não-harness

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_me.rs`

- [ ] **Step 1: Adicionar import + atributo**

```rust
use serial_test::serial;

#[tokio::test]
#[serial]
async fn get_v1_me_fails_soft_with_503_problem_details_when_auth_unconfigured() {
    // ...inalterado
}
```

Motivo: este teste mexe em env vars globais (`remove_var`). Qualquer teste futuro que também toque env deve ser `#[serial]`.

- [ ] **Step 2: Rodar**

Run: `cargo test -p garraia-gateway --test rest_v1_me`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_me.rs
git commit -m "test(gateway): serialize env-mutating rest_v1_me test (plan 0016 t7)"
```

---

### Task 8: M2 smoke — dry-run do harness

- [ ] **Step 1: Criar um teste trivial que inicia o harness**

Criar `crates/garraia-gateway/tests/harness_smoke.rs`:

```rust
//! Smoke test: harness boots and returns a usable Router.

mod common;

use common::Harness;

#[tokio::test]
async fn harness_boots_and_router_responds_to_openapi_json() {
    let h = Harness::get().await;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Rodar**

Run: `cargo test -p garraia-gateway --test harness_smoke -- --nocapture`
Expected: primeira execução baixa imagem pgvector/pg16 (~60s), aplica migrations, boota pools, retorna `/v1/openapi.json` com 200. PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/harness_smoke.rs
git commit -m "test(gateway): smoke test for integration harness (plan 0016 t8)"
```

---

## M3 — Authed `/v1/me` + OpenAPI bearer scheme

### Task 9: `SecurityAddon` modifier registrando `bearer` em `ApiDoc`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Adicionar `SecurityAddon` + modifiers**

```rust
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use super::me::MeResponse;
use super::problem::ProblemDetails;

pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "GarraIA REST /v1",
        version = "0.1.0",
        description = "Versioned GarraIA gateway REST surface (Fase 3.4)."
    ),
    paths(super::me::get_me),
    components(schemas(MeResponse, ProblemDetails)),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
```

- [ ] **Step 2: Verificar build + Swagger UI**

Run: `cargo check -p garraia-gateway && cargo test -p garraia-gateway --test harness_smoke`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "feat(gateway): register bearer security scheme in OpenAPI (plan 0016 t9)"
```

---

### Task 10: 4 testes autenticados de `GET /v1/me`

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_me.rs`

- [ ] **Step 1: Adicionar os 4 testes**

```rust
use common::{fixtures::seed_user_with_group, Harness};

mod common;

#[tokio::test]
async fn get_v1_me_without_bearer_returns_401_problem_details() {
    let h = Harness::get().await;
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/me")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_no_group_returns_200_without_group_id() {
    let h = Harness::get().await;
    let (user_id, _group_id, token) = seed_user_with_group(&h, "alice@example.com").await;
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["user_id"], user_id.to_string());
    assert!(v.get("group_id").is_none());
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_and_member_group_returns_200_with_role() {
    let h = Harness::get().await;
    let (user_id, group_id, token) = seed_user_with_group(&h, "bob@example.com").await;
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id.to_string())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["user_id"], user_id.to_string());
    assert_eq!(v["group_id"], group_id.to_string());
    assert_eq!(v["role"], "owner");
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_non_member_group_returns_403() {
    let h = Harness::get().await;
    let (_user_id, _group_id, token) = seed_user_with_group(&h, "carol@example.com").await;
    let foreign = uuid::Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", foreign.to_string())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}
```

- [ ] **Step 2: Rodar**

Run: `cargo test -p garraia-gateway --test rest_v1_me`
Expected: 1 (fail-soft) + 4 (authed) = 5 PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_me.rs
git commit -m "test(gateway): authed integration tests for GET /v1/me (plan 0016 t10)"
```

---

### Task 11: Teste wire de `/v1/openapi.json`

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_me.rs` (or new `tests/rest_v1_openapi.rs`)

- [ ] **Step 1: Adicionar teste**

```rust
#[tokio::test]
async fn openapi_spec_exposes_get_me_and_bearer_scheme() {
    let h = Harness::get().await;
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["info"]["version"], "0.1.0");
    assert!(v["paths"]["/v1/me"]["get"].is_object());
    assert_eq!(
        v["components"]["securitySchemes"]["bearer"]["scheme"],
        "bearer"
    );
    assert_eq!(
        v["components"]["securitySchemes"]["bearer"]["bearerFormat"],
        "JWT"
    );
}
```

- [ ] **Step 2: Rodar + Commit**

Run: `cargo test -p garraia-gateway --test rest_v1_me`
Expected: 6 PASS total.

```bash
git add crates/garraia-gateway/tests/rest_v1_me.rs
git commit -m "test(gateway): wire validation of /v1/openapi.json (plan 0016 t11)"
```

---

## M4 — `/v1/groups` skeleton

### Task 12: Handler `POST /v1/groups`

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/groups.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs` (`pub mod groups;`)
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs` (add to `paths(...)` + `schemas(...)`)

**Contrato:**
- Request: `POST /v1/groups` com body `{"name": string, "type": "family" | "team"}` (validação: `type = personal` é reservada, NÃO aceita via API — ver comentário em migration 001 linha 114)
- Auth: `Principal` com JWT válido; `X-Group-Id` **não** usado (cria um novo)
- Semântica: INSERT em `groups` (sem RLS) + INSERT em `group_members` (role='owner', status='active') como **uma transação** via `app_pool`
- Response 201: `{"id": uuid, "name": string, "type": string, "created_at": iso8601}`
- Errors: 400 (invalid type), 401 (no bearer), 503 (app_pool None)

- [ ] **Step 1: Escrever o teste primeiro (RED)**

`crates/garraia-gateway/tests/rest_v1_groups.rs`:

```rust
mod common;
use common::Harness;

#[tokio::test]
async fn post_v1_groups_creates_group_and_assigns_owner() {
    let h = Harness::get().await;
    // seed_user without pre-existing group
    let user_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, email, display_name, status, created_at) VALUES ($1, 'dave@example.com', 'D', 'active', now())")
        .bind(user_id)
        .execute(&h.admin_pool)
        .await
        .unwrap();
    let token = h.jwt.issue_access_for_test(user_id).unwrap();

    let body = serde_json::json!({"name": "New Group", "type": "team"}).to_string();
    let resp = h
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/groups")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let v: serde_json::Value = serde_json::from_slice(&http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes()).unwrap();
    let group_id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(v["name"], "New Group");
    assert_eq!(v["type"], "team");

    // Assert group_members row exists (via admin_pool, RLS bypass).
    let (role,): (String,) = sqlx::query_as("SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2")
        .bind(group_id).bind(user_id).fetch_one(&h.admin_pool).await.unwrap();
    assert_eq!(role, "owner");
}

#[tokio::test]
async fn post_v1_groups_rejects_personal_type_with_400() { /* body: type=personal → 400 */ }

#[tokio::test]
async fn post_v1_groups_without_bearer_returns_401() { /* ... */ }
```

- [ ] **Step 2: Implementar `groups.rs`**

```rust
//! /v1/groups handlers (plan 0016 M4).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use garraia_auth::{AppPool, Principal};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::problem::RestError;
use super::RestV1FullState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub created_at: DateTime<Utc>,
}

#[utoipa::path(
    post,
    path = "/v1/groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 201, description = "Group created", body = GroupResponse),
        (status = 400, description = "Invalid type or empty name", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_group(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), RestError> {
    // 1. Validate type.
    if !matches!(req.group_type.as_str(), "family" | "team") {
        return Err(RestError::BadRequest("invalid group type".into()));
    }
    if req.name.trim().is_empty() {
        return Err(RestError::BadRequest("name must not be empty".into()));
    }

    // 2. Transaction on app_pool with tenant context = creator.
    let mut tx = state.app_pool.pool_for_handlers()
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Set the RLS tenant context so group_members INSERT passes the
    // WITH CHECK policy. The user_id is the creator's.
    sqlx::query(&format!("SET LOCAL app.current_user_id = '{}'", principal.user_id))
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let (id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO groups (name, type, created_by) \
         VALUES ($1, $2, $3) \
         RETURNING id, created_at"
    )
    .bind(&req.name)
    .bind(&req.group_type)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'owner', 'active')"
    )
    .bind(id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit().await.map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(GroupResponse {
            id,
            name: req.name,
            group_type: req.group_type,
            created_at,
        }),
    ))
}
```

> **Dependência:** `AppPool` precisa expor um método `pool_for_handlers()` que retorne `&PgPool` **só para handlers dentro de `garraia-gateway`**. Como `pool()` é `pub(crate)` dentro de `garraia-auth`, expor via uma função `pub fn pool_for_handlers(&self) -> &PgPool` é a mudança mínima. **Alternativa** mais estrita: trait `PoolAccess` implementada em `garraia-auth` e re-exportada — mas é overkill nesta slice. Abrir como decisão no começo desta task.

> **`RestError::BadRequest`** — não existe hoje. Adicionar variante ao enum em `problem.rs`:

```rust
#[error("bad request: {0}")]
BadRequest(String),
// mapped to 400, title "Bad Request"
```

- [ ] **Step 3: Mount handler no router**

Em `rest_v1/mod.rs` na branch full state, adicionar `.route("/v1/groups", post(groups::create_group))`.

Em `openapi.rs`: adicionar `paths(super::me::get_me, super::groups::create_group)` e `schemas(..., CreateGroupRequest, GroupResponse)`.

- [ ] **Step 4: Rodar testes**

Run: `cargo test -p garraia-gateway --test rest_v1_groups`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/ crates/garraia-gateway/tests/rest_v1_groups.rs crates/garraia-auth/src/app_pool.rs
git commit -m "feat(gateway): POST /v1/groups with creator=owner (plan 0016 t12)"
```

---

### Task 13: Handler `GET /v1/groups/{id}`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

**Contrato:**
- Request: `GET /v1/groups/{id}`, header `Authorization: Bearer ...` obrigatório, header `X-Group-Id: {id}` **obrigatório** e **deve coincidir** com o path param (senão 400)
- Auth: `Principal.group_id == path id` é verificado pelo extractor (ele retorna 403 se o caller não for membro). O handler apenas faz `SELECT`.
- Response 200: `{"id", "name", "type", "created_at", "created_by", "role"}` (role vem de `principal.role`)
- Errors: 400 (header mismatch), 401 (no bearer), 403 (not a member), 404 (group não existe — edge case raro, acontece se o ID é válido mas o grupo foi deletado)

- [ ] **Step 1: Teste primeiro**

```rust
#[tokio::test]
async fn get_v1_groups_by_id_returns_200_for_member() {
    let h = Harness::get().await;
    let (_user_id, group_id, token) = common::fixtures::seed_user_with_group(&h, "erin@example.com").await;
    let resp = h.router.clone().oneshot(
        axum::http::Request::builder()
            .uri(format!("/v1/groups/{group_id}"))
            .header("authorization", format!("Bearer {token}"))
            .header("x-group-id", group_id.to_string())
            .body(axum::body::Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = serde_json::from_slice(&http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["id"], group_id.to_string());
    assert_eq!(v["role"], "owner");
}

#[tokio::test]
async fn get_v1_groups_by_id_returns_400_on_header_path_mismatch() { /* ... */ }

#[tokio::test]
async fn get_v1_groups_by_id_returns_403_for_non_member() { /* ... */ }
```

- [ ] **Step 2: Implementar handler**

```rust
#[utoipa::path(
    get,
    path = "/v1/groups/{id}",
    params(
        ("id" = Uuid, Path, description = "Group UUID (must match X-Group-Id header)"),
    ),
    responses(
        (status = 200, body = GroupReadResponse),
        (status = 400, body = super::problem::ProblemDetails),
        (status = 401, body = super::problem::ProblemDetails),
        (status = 403, body = super::problem::ProblemDetails),
        (status = 404, body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_group(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<GroupReadResponse>, RestError> {
    match principal.group_id {
        Some(g) if g == id => {}
        Some(_) => return Err(RestError::BadRequest("X-Group-Id header and path id must match".into())),
        None => return Err(RestError::BadRequest("X-Group-Id header required".into())),
    }
    // Principal already confirmed membership via extractor.
    let row = sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>, Uuid)>(
        "SELECT id, name, type, created_at, created_by FROM groups WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(state.app_pool.pool_for_handlers())
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (id, name, group_type, created_at, created_by) = row.ok_or(RestError::NotFound)?;
    Ok(Json(GroupReadResponse {
        id,
        name,
        group_type,
        created_at,
        created_by,
        role: principal.role.map(|r| r.as_str().to_string()).unwrap_or_default(),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupReadResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub role: String,
}
```

> **`RestError::NotFound`** — adicionar variante ao enum (404).

- [ ] **Step 3: Mount + OpenAPI + test run + Commit**

```bash
cargo test -p garraia-gateway --test rest_v1_groups
git add crates/garraia-gateway/src/rest_v1/ crates/garraia-gateway/tests/rest_v1_groups.rs
git commit -m "feat(gateway): GET /v1/groups/{id} with membership check (plan 0016 t13)"
```

---

### Task 14: Expandir `ApiDoc` para os novos paths + schemas

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Atualizar macro**

```rust
#[derive(OpenApi)]
#[openapi(
    info(...),
    paths(
        super::me::get_me,
        super::groups::create_group,
        super::groups::get_group,
    ),
    components(schemas(
        MeResponse,
        ProblemDetails,
        super::groups::CreateGroupRequest,
        super::groups::GroupResponse,
        super::groups::GroupReadResponse,
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
```

- [ ] **Step 2: Atualizar teste wire**

Em `rest_v1_me.rs::openapi_spec_*`, assert que `/v1/groups` e `/v1/groups/{id}` estão em `paths`.

- [ ] **Step 3: Rodar + Commit**

```bash
cargo test -p garraia-gateway --test rest_v1_me
git add crates/garraia-gateway/src/rest_v1/openapi.rs crates/garraia-gateway/tests/rest_v1_me.rs
git commit -m "feat(gateway): add /v1/groups endpoints to OpenAPI spec (plan 0016 t14)"
```

---

### Task 15: Marcar `ROADMAP.md` checkboxes

**Files:**
- Modify: `ROADMAP.md`

- [ ] **Step 1: Marcar os dois**

Em §3.4 Grupos, trocar:
- `- [ ] \`POST /v1/groups\`` → `- [x] \`POST /v1/groups\` — plan 0016, 2026-04-14`
- `- [ ] \`GET /v1/groups/{group_id}\`` → `- [x] \`GET /v1/groups/{group_id}\` — plan 0016, 2026-04-14`

Nenhum outro checkbox.

- [ ] **Step 2: Commit**

```bash
git add ROADMAP.md
git commit -m "docs(roadmap): mark /v1/groups skeleton shipped (plan 0016)"
```

---

## M5 — Review follow-ups residuais

### Task 16: M-3 nit + H-1 doc note + N-3 log fallback

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/problem.rs`

- [ ] **Step 1: `unconfigured_handler` → `impl IntoResponse`**

```rust
async fn unconfigured_handler() -> impl axum::response::IntoResponse {
    problem::RestError::AuthUnconfigured
}
```

- [ ] **Step 2: Doc note em `RestError::Internal`**

Adicionar acima da variante:

```rust
/// Internal error wrapper. **Callers MUST NOT `.context("...")` with
/// user-identifying data (email, user_id, hashes)** before converting
/// to `RestError::Internal`: the `Display` impl of `anyhow::Error` will
/// print the outermost context in the log span created by
/// `IntoResponse`, and that log line is operator-visible.
#[error("internal error")]
Internal(#[source] anyhow::Error),
```

- [ ] **Step 3: N-3 log no fallback**

Em `problem.rs::into_response`, trocar:

```rust
let json = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
```

por:

```rust
let json = serde_json::to_vec(&body).unwrap_or_else(|e| {
    tracing::warn!(error = %e, "problem.rs: fallback empty JSON body used");
    b"{}".to_vec()
});
```

- [ ] **Step 4: Build + Commit**

```bash
cargo check -p garraia-gateway
git add crates/garraia-gateway/src/rest_v1/
git commit -m "chore(gateway): review follow-ups H-1/M-3/N-3 from PR #8 (plan 0016 t16)"
```

---

### Task 17: M1 fail-soft `/docs/{*path}` — confirmação

Esta task já foi coberta pela Task 4 Step 2 (adicionamos `/docs/{*rest}` no branch `(None, None)`). Reconfirmar no commit final para não deixar ambiguidade. Sem novo commit se já está lá.

- [ ] **Step 1: Grep de confirmação**

Run: `rg -n "/docs/.*rest" crates/garraia-gateway/src/rest_v1/mod.rs`
Expected: linha única na branch fail-soft.

- [ ] **Step 2: Se ausente, adicionar + commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs
git commit -m "fix(gateway): cover /docs/* trailing paths in fail-soft mode (plan 0016 t17)"
```

---

## Validação final

### Task 18: Workspace check + clippy + suíte completa

- [ ] `cargo check --workspace`
- [ ] `cargo clippy --workspace -- -D warnings` → ignorar warnings pré-existentes (too-many-arguments, collapsible_if), rejeitar qualquer warning novo em `rest_v1`/`groups.rs`/`app_pool.rs`
- [ ] `cargo test -p garraia-auth` → incluir `app_pool_smoke`
- [ ] `cargo test -p garraia-gateway --lib` → unit tests de `rest_v1::*`
- [ ] `cargo test -p garraia-gateway --test rest_v1_me` → 6 tests (1 fail-soft + 4 authed + 1 openapi)
- [ ] `cargo test -p garraia-gateway --test rest_v1_groups` → ≥6 tests
- [ ] `cargo test -p garraia-gateway --test harness_smoke` → 1 test
- [ ] `cargo test -p garraia-gateway --test router_smoke_test` → 3 tests (legado, intacto)

Sem commit. Validação pura.

---

## §8 Rollback plan

Cada task é um commit independente. `git revert` commit-a-commit restaura estado anterior. Pontos críticos:
- **Task 1 (AppPool)** é a única mudança estrutural em `garraia-auth` — se revertida, tudo de M1-M4 quebra em cascata, mas o PR #8 (plan 0015) continua funcional porque ele não depende do AppPool.
- **Task 6 (harness)** assume a existência de `GatewayServer::build_router_for_test` — se revertida, os testes de integração novos quebram mas nada em prod.
- **Task 12/13 (`/v1/groups`)** pode ser revertida sem tocar em M1-M3 — os handlers ficam indisponíveis (503 via fail-soft).

Migrations: **nenhuma nova**, reuso puro. Sem risco de schema.
Env vars: `GARRAIA_APP_DATABASE_URL` é **opcional** — gateways sem ela continuam operando em fail-soft para `/v1/groups`.

---

## Self-review checklist (2026-04-14)

- **Spec coverage:** todos os 6 items do pedido do owner (harness / authed /v1/me / wire openapi / POST /v1/groups / GET /v1/groups/{id} / review follow-ups) têm tasks dedicadas. ✅
- **Placeholder scan:** nenhum TBD/TODO. Passos com código mostram o código. As Open Questions do §12 são pontos de verificação no código existente, não placeholders. ✅
- **Type consistency:** `RestError` ganha 2 variantes novas (`BadRequest`, `NotFound`) consistentes em Task 12/13/16. `RestV1AuthState`/`RestV1FullState` usados coerentemente em Task 4/12/13. `AppPool` exportado em Task 1, consumido em Task 3/4/6/12/13. ✅
- **Ambiguidade:** Task 6 Step 2 depende de `GatewayServer::build_router_for_test` que será criado na mesma task — explicitado. Task 12 depende de `AppPool::pool_for_handlers` — explicitado como decisão da task. ✅
- **Escopo:** 18 tasks, 5 marcos. Maior que o 0015 (9 tasks), mas proporcional ao escopo combinado que o owner pediu. Se execução mostrar que é demais para um único PR, **splitar em 0016a (M1+M2+M3) e 0016b (M4+M5)** é a fatia natural — documentada aqui como opção de contingência.
