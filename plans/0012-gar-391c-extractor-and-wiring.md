# Plan 0012: GAR-391c — Axum extractor + RequirePermission + wiring + refresh/logout/signup endpoints

> **⚠️ Amendment 2026-04-13 (Wave 1.5 — post-execution corrections)**
>
> Three deviations from the original plan surfaced during multi-agent
> implementation:
>
> **Gap C correction.** The `Principal` extractor membership lookup
> (`SELECT role FROM group_members …`) requires `SELECT ON group_members`
> for `garraia_login`. This grant was absent from §3.1 and §6. Migration
> 010 was updated mid-execution (user approval: Option 1, 2026-04-13) to
> add `GRANT SELECT ON group_members TO garraia_login` plus the matching
> positive smoke assert, and `group_members` was removed from migration
> 008's negative grant matrix. §3.1 and §6 SQL skeleton are considered
> superseded by the shipped migration file. Acceptance criteria extended:
> `has_table_privilege('garraia_login', 'group_members', 'SELECT') = true`.
>
> **`RequirePermission` shape.** Due to Axum's unstable const generics
> over enums, `RequirePermission` does NOT implement `FromRequestParts`.
> It is a plain struct (`pub struct RequirePermission(pub Action)`) with
> an associated method `check()` and a free function `require_permission()`.
> Handlers call `RequirePermission::check(&principal, action)?` as an
> inline guard. Plan §3.2's "FromRequestParts impl" wording is
> superseded by this shape.
>
> **Action count.** Wave 0 reconnaissance found 22 permissions seeded in
> migration 002, not 21. The `docs.*` and `export.*` families were
> omitted from the plan's Action enum sketch. The shipped `action.rs`
> defines 22 variants. The `fn can()` table-driven test covers
> `5 × 22 = 110` cases (not 105). §3.2 and §4 are superseded by these
> numbers.

> **Status:** ✅ Approved + executed (multi-agent waves)
> **Issue:** [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — sub-issue 391c (extractor + wiring)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-authz`, `security`
> **Priority:** Urgent
> **Estimated session size:** 5-8 horas focado (multi-agent waves wall ~100min)
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-391a (skeleton + login role) + ✅ plan 0011.5 / migration 009 + ✅ GAR-391b (verify path real + endpoint sob feature `auth-v1`)
> **Unblocks:** **GAR-391d / GAR-392** (cross-group authz suite — fechamento do epic GAR-391)

---

## 1. Goal (one sentence)

Wirar o `garraia-auth` real no `garraia-gateway` removendo a feature flag `auth-v1`, materializando o `Principal` extractor (com `Role` enum tipado + `fn can(&Principal, Action) -> bool` central + `RequirePermission(Action)` extractor que retorna `403` cedo), entregando os endpoints `/v1/auth/refresh`, `/v1/auth/logout` e `/v1/auth/signup` (este último via novo `SignupPool` newtype validado por `current_user='garraia_signup'`), aplicando migration `010_signup_role_and_session_select.sql` (cria `garraia_signup NOLOGIN BYPASSRLS` com grants mínimos + adiciona `GRANT SELECT ON sessions TO garraia_login` para fechar Gap A do 391b), reabilitando `SessionStore::issue/verify_refresh` no fluxo do endpoint, migrando o JWT secret para `garraia-config`, e expondo métricas Prometheus baseline (`garraia_auth_login_total{outcome}` + `garraia_auth_login_latency_seconds`) — tudo sob auditoria multi-agente em waves.

---

## 2. Rationale — por que essa fatia agora

1. **Verify path real já existe** (391b) mas só roda atrás de `auth-v1`. Sem extractor + wiring, o resto do gateway não consegue usar `Principal` e nenhuma rota REST de Fase 3.4 (`/v1/groups`, `/v1/chats`, etc.) pode começar.
2. **Refresh + logout + signup endpoints** estavam deferidos por dois motivos: (a) o extractor não existia, (b) o `garraia_login` role não tinha `SELECT ON sessions`. 391c resolve ambos no mesmo commit porque são intrinsecamente acoplados (refresh depende de SELECT, logout depende de UPDATE, signup depende de signup pool — todos juntos).
3. **`Role` enum + `fn can()`** é a fundação de todo o RBAC. 391d (suite cross-group) não pode shippar sem ele.
4. **Migration 010** fecha os dois gaps estruturais descobertos empiricamente em 391b. Forward-only, sob review de segurança independente.
5. **Métricas baseline** entram aqui antes de qualquer suite de carga porque retrofit é caro.
6. **Multi-agent waves** (acordado nesta sessão como padrão operacional) maximiza paralelismo sem perder rigor.

---

## 3. Scope & Non-Scope

### In scope

#### 3.1 Migration 010 — `signup_role_and_session_select.sql` (em `garraia-workspace`)

- **NOVO role** `garraia_signup NOLOGIN BYPASSRLS` (idempotente, `DO $$ ... $$` block).
- **Grants minimais** para `garraia_signup`:
  - `GRANT USAGE ON SCHEMA public TO garraia_signup;`
  - `GRANT SELECT, INSERT ON users TO garraia_signup;` (precisa de SELECT para checar duplicate email pré-INSERT e RETURNING id)
  - `GRANT SELECT, INSERT ON user_identities TO garraia_signup;` (mesmo motivo)
  - `GRANT INSERT ON audit_events TO garraia_signup;` (audit do signup attempt)
- **NEGATIVAS explícitas:** `garraia_signup` NÃO recebe acesso a `sessions`, `messages`, `chats`, `chat_members`, `message_threads`, `memory_*`, `tasks*`, `groups`, `group_*`, `roles`, `permissions`, `role_permissions`, `api_keys`. Smoke test valida cada negação.
- **NOVO grant para `garraia_login`** fechando Gap A do 391b:
  - `GRANT SELECT ON sessions TO garraia_login;`
  - Justificativa documentada inline: necessário para `INSERT ... RETURNING id` (Postgres exige SELECT nas colunas do RETURNING) e para `SessionStore::verify_refresh` (lookup por `refresh_token_hash`).
- **`COMMENT ON ROLE garraia_signup`** com blast-radius warning ("compromise = ability to create arbitrary identities").
- **Sem sequence grants** (mesmo critério da migration 008).
- **Forward-only**, sem DROP, sem ALTER destrutivo.

#### 3.2 `garraia-auth` — extractor + RBAC types + signup pool + redactor

**NOVOS arquivos:**

- `crates/garraia-auth/src/role.rs` — `Role` enum tipado:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum Role { Owner, Admin, Member, Guest, Child }

  impl Role {
      pub fn tier(self) -> u8 { /* 100/80/50/20/10 — match migration 002 seed */ }
      pub fn from_str(s: &str) -> Option<Self> { /* parse from group_members.role text */ }
  }
  ```
  Critério: bate exatamente com o CHECK enum de migration 001 (`group_members.role`) e os tiers seedados em migration 002 (`roles.tier`).

- `crates/garraia-auth/src/action.rs` — `Action` enum + `Capability` mapping:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Action {
      // tasks (already in seed)
      TasksRead, TasksWrite, TasksAssign, TasksDelete, TasksAdmin,
      // chats
      ChatsRead, ChatsWrite, ChatsModerate,
      // files (deferred but stub the variants)
      FilesRead, FilesWrite, FilesDelete, FilesShare,
      // memory
      MemoryRead, MemoryWrite, MemoryDelete,
      // members
      MembersManage, MembersInvite, MembersRemove,
      // group settings
      GroupRead, GroupWrite, GroupDelete,
  }
  ```
  Mapeamento `Role → HashSet<Action>` baseado no seed de migration 002 (verificado contra `roles.tier` + `permissions` table). Critério: 100% dos `role_permissions` (63 rows seedados) cobertos.

- `crates/garraia-auth/src/can.rs` — `pub fn can(principal: &Principal, action: Action) -> bool`:
  - Lê `principal.role` (typed); usa o mapeamento estático de `action.rs`; retorna `false` se `role.is_none()` (principal sem grupo ativo não pode nada de grupo).
  - Tem unit tests cobrindo o mapeamento role × action (5 roles × 21 actions = 105 cases) — generated table-driven test.

- `crates/garraia-auth/src/extractor.rs` — `FromRequestParts<Arc<AppState>> for Principal` + `RequirePermission(Action)`:
  - Extrai `Authorization: Bearer <jwt>` do header. Decodifica via `JwtIssuer::verify_access` (vem do `AppState`).
  - Resolve `group_id` opcional via header `X-Group-Id: <uuid>`.
  - Quando `group_id` está presente, valida membership via `SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2 AND status = 'active'` no `LoginPool` (usa o BYPASSRLS — é leitura de tenant management; alternativa seria criar mais um pool, custo > benefício para 391c).
  - Retorna `Principal { user_id: claims.sub, group_id: Some(group_id), role: Some(parsed_role) }` ou `Principal { user_id, group_id: None, role: None }` se sem header.
  - `RequirePermission(Action)` é um struct extractor que primeiro extrai `Principal` e depois chama `can(&principal, self.0)`; se false, retorna `(StatusCode::FORBIDDEN, "forbidden")`. Se sem JWT, retorna `(StatusCode::UNAUTHORIZED, "unauthenticated")`.
  - Axum 0.8: usa AFIT nativo `FromRequestParts` (sem `#[async_trait]` per CLAUDE.md convention).

- `crates/garraia-auth/src/signup_pool.rs` — `SignupPool` newtype:
  - Estrutura idêntica ao `LoginPool`: campo `inner: PgPool` privado, `pub(crate) fn pool()` accessor, `static_assertions::assert_not_impl_all!(SignupPool: Clone)`.
  - Constructor `from_dedicated_config(&SignupConfig)` validando `SELECT current_user::text == 'garraia_signup'`.
  - `SignupConfig { database_url: SecretString, max_connections: u32 }` com manual `Debug` redact + `validator` derive.
  - Doc comment apontando que este pool tem grants distintos do login pool — não substituível.

- `crates/garraia-auth/src/storage_redacted.rs` — wrapper para `AuthError::Storage` (M-3 deferral do 391b):
  - Newtype `RedactedStorageError(sqlx::Error)` com `Display` que strip qualquer `postgres://...` substring e qualquer `host=` segment do connection error.
  - `AuthError::Storage` agora wraps `RedactedStorageError` em vez de `sqlx::Error` direto.
  - Unit tests cobrindo: `sqlx::Error::Configuration` com URL embebida, `sqlx::Error::Io` com endereço, `sqlx::Error::PoolTimedOut` (sem leak natural).

**EDITS em arquivos existentes:**

- `crates/garraia-auth/src/types.rs` — `Principal.role: Option<String>` → `Option<Role>` (breaking change, ripple para tests).
- `crates/garraia-auth/src/internal.rs` — `create_identity` real (usa `SignupPool` agora):
  ```rust
  pub async fn create_identity_with_pool(
      signup_pool: &SignupPool,
      user_id: Uuid,
      credential: &Credential,
  ) -> Result<Uuid> {
      // INSERT INTO user_identities ... RETURNING id
      // dentro de tx com SELECT pré-INSERT para detectar duplicate email
  }
  ```
  O método trait `create_identity` continua deprecated/erro porque trait method não tem acesso ao SignupPool (que é separado). A operação real expõe via free function `signup_user(login_pool, signup_pool, email, password) -> Result<Uuid>` para o handler chamar.
- `crates/garraia-auth/src/lib.rs` — re-exports novos: `Role`, `Action`, `can`, `extractor::{Principal, RequirePermission}`, `SignupPool`, `SignupConfig`, `RedactedStorageError`, `signup_user`.
- `crates/garraia-auth/src/error.rs` — variants novas:
  - `Forbidden` — RBAC negou acesso.
  - `Unauthenticated` — JWT ausente ou inválido.
  - `DuplicateEmail` — signup tentando registrar email já existente.
- `crates/garraia-auth/src/jwt.rs` — adicionar helper `extract_bearer_token(headers: &HeaderMap) -> Option<&str>` que parsea `Authorization: Bearer ...`.
- `crates/garraia-auth/Cargo.toml` — adicionar `axum = { workspace = true }` em deps (necessário para `FromRequestParts` impl) + remover dev-dep `axum` se houver duplicação.

#### 3.3 `garraia-config` — JWT secret config wiring

**NOVO/EDIT:**

- `crates/garraia-config/src/auth.rs` (NOVO ou seção dentro de `lib.rs`) — `AuthConfig` struct:
  ```rust
  #[derive(Clone, Deserialize, Validate)]
  pub struct AuthConfig {
      pub jwt_secret: SecretString,         // env: GARRAIA_JWT_SECRET, ≥32 bytes
      pub refresh_hmac_secret: SecretString, // env: GARRAIA_REFRESH_HMAC_SECRET, ≥32 bytes
      pub login_database_url: SecretString,  // env: GARRAIA_LOGIN_DATABASE_URL
      pub signup_database_url: SecretString, // env: GARRAIA_SIGNUP_DATABASE_URL
  }
  ```
  Manual `Debug` redact em todos os campos. Loaded via `from_env()` com fallback documentado para `GARRAIA_VAULT_PASSPHRASE` apenas para o `jwt_secret` (compatibilidade com `mobile_auth.rs` enquanto a migração não fecha).

#### 3.4 `garraia-gateway` — wiring no `AppState` + endpoints + métricas + remove feature flag

**EDITS no `AppState`:**

- `crates/garraia-gateway/src/state.rs` — adicionar:
  ```rust
  pub auth_provider: Option<Arc<dyn IdentityProvider>>,
  pub jwt_issuer: Option<Arc<JwtIssuer>>,
  pub session_store: Option<Arc<SessionStore>>,
  pub signup_pool: Option<Arc<SignupPool>>,
  ```
  `Option` para tolerar config ausente em dev (gateway boota com warning, endpoints `/v1/auth/*` retornam 503 Service Unavailable).
- `crates/garraia-gateway/src/bootstrap.rs` — construir `LoginPool`, `SignupPool`, `JwtIssuer`, `SessionStore` na startup a partir do `AuthConfig`. Logging fail-soft.
- `crates/garraia-gateway/src/server.rs` — montar o router de `auth_routes` no path `/v1/auth/*` (default-on, sem feature flag).
- `crates/garraia-gateway/Cargo.toml` — **remove** `auth-v1` feature; `garraia-auth` e `secrecy` viram deps default; remove `optional = true`.

**EDITS em `auth_routes.rs`:**

- Trocar `AuthState` standalone por extractors `State<Arc<AppState>>`.
- Atualizar `POST /v1/auth/login` para:
  - Reativar `SessionStore::issue` (agora com SELECT grant) → response inclui `refresh_token` (como originalmente planejado).
  - Manter byte-identical 401 anti-enumeration.
- **NOVO `POST /v1/auth/refresh`:**
  - Body: `{ "refresh_token": "..." }`.
  - Verifica via `SessionStore::verify_refresh`. Se válido, emite novo access_token + (opcionalmente) rotaciona refresh_token (rotation default ON em 391c — reduz blast radius de leak).
  - Anti-enumeration: refresh_token inválido → 401 byte-identical com login failure.
- **NOVO `POST /v1/auth/logout`:**
  - Body: `{ "refresh_token": "..." }` (ou usa cookie se preferirem; v1 usa body para simplificar).
  - Chama `SessionStore::revoke` para o session_id correspondente. Idempotente.
  - 204 No Content sempre (anti-enumeration).
- **NOVO `POST /v1/auth/signup`:**
  - Body: `{ "email": "...", "password": "...", "display_name": "..." }`.
  - Chama `signup_user(...)` que usa `SignupPool` (não LoginPool).
  - Response: `201 Created` com `{user_id, access_token, refresh_token, expires_at}` (auto-login pós-signup).
  - Anti-enumeration: email já registrado → mesma response do happy path? **NÃO** — signup não precisa anti-enumeration porque o atacante já consegue enumerar via login (qualquer 401 confirma "user exists ou não"). Em vez disso retorna `409 Conflict` se email duplicado. Audit trail captura tentativas.
  - **Rate limiting:** signup é o endpoint mais abusável → flag para 391d ou follow-up dedicado. Em 391c apenas declarar a necessidade no doc + TODO.

**NOVO `auth_metrics.rs`:**

- `crates/garraia-gateway/src/auth_metrics.rs`:
  ```rust
  // Atrás de feature `telemetry` (já default-on)
  pub fn record_login(outcome: &'static str, latency_seconds: f64);
  pub fn record_refresh(outcome: &'static str);
  pub fn record_signup(outcome: &'static str);
  ```
  Outcomes possíveis: `success`, `failure_invalid_credentials`, `failure_account_inactive`, `failure_internal`, `failure_unknown_hash`, `failure_duplicate_email` (signup only).
  Métricas expostas: `garraia_auth_login_total{outcome}`, `garraia_auth_login_latency_seconds`, `garraia_auth_refresh_total{outcome}`, `garraia_auth_signup_total{outcome}`. Cardinalidade bounded (outcome ∈ enum fechado).

**EDITS em testes:**

- `crates/garraia-gateway/tests/auth_v1_login.rs` → renomear para `auth_login.rs` (sem o `_v1`); ajustar para usar o novo wiring via `AppState`. Remove cfg(feature="auth-v1") gates.
- NOVO `tests/auth_refresh.rs` — happy path, expired token, revoked session, wrong token → byte-identical 401.
- NOVO `tests/auth_logout.rs` — happy path (revoke), idempotent re-logout, unknown token (still 204).
- NOVO `tests/auth_signup.rs` — happy path (201 + tokens), duplicate email (409), weak password (validation TBD ou follow-up).
- NOVO `tests/extractor_authz.rs` — `Principal` extractor positive (valid JWT + valid group_id), missing JWT (401), invalid JWT (401), valid JWT + missing X-Group-Id (Principal sem role), valid JWT + group_id que user não pertence (403). `RequirePermission` wrapper teste para 1 capability (ex.: `TasksWrite` → admin OK, child denied).

#### 3.5 Plano + Linear

- Plan committed junto com a implementação.
- Comment em GAR-391 (sem mover): "391c merged em commit `<hash>`. 391d/GAR-392 (suite cross-group authz) ainda pending."
- **Mover GAR-391 para Done apenas quando 391d shippar.**

### Out of scope (deferido a 391d ou follow-ups)

- ❌ **Cross-group authz suite (≥100 cenários)** — GAR-392 / 391d. Plan dedicado depois.
- ❌ **Rate limiting** (`tower-governor` no signup/login/refresh) — follow-up dedicado pós-391c. Em 391c é apenas TODO documentado.
- ❌ **CAPTCHA / lockout após N failures** — futura issue de hardening.
- ❌ **OIDC / SAML adapters** — futuro ADR 0009.
- ❌ **Password reset / forgot password flow** — futura epic.
- ❌ **Account recovery via email** — mesmo.
- ❌ **mTLS / client cert auth** — futuro ADR.
- ❌ **`api_keys` flow real** (separado do JWT login) — fora deste epic. 391c não toca.
- ❌ **Migração de `mobile_users` → `user_identities`** — GAR-413 dedicada.
- ❌ **Substituir `mobile_auth.rs` legacy pelo novo `auth_routes.rs`** — fora de escopo. `mobile_auth.rs` continua co-existindo até GAR-413 / future cleanup.
- ❌ **Métricas detalhadas por grupo / per-user counters** — cardinality risk; baseline em 391c, breakdown depois.
- ❌ **Refresh token rotation reuse-detection** (RFC 6819 §5.2) — flag em 391c, implementação em hardening pass.
- ❌ **Distributed session revocation** (kill switch global) — single-instance em 391c.
- ❌ **JWT key rotation com kid header** — single-key em 391c, key rotation em hardening pass.
- ❌ **Audience claim validation** — single-audience em 391c.
- ❌ **`AuthConfig` reload reativo** (hot-reload do JWT secret) — restart-required em 391c.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde (com e sem `--no-default-features`).
- [ ] `cargo clippy -p garraia-auth --all-targets -- -D warnings` verde.
- [ ] `cargo clippy -p garraia-gateway --all-targets -- -D warnings` verde no novo código (warnings pré-existentes em `bootstrap.rs`/`mobile_chat.rs` permanecem fora de escopo).
- [ ] `cargo test -p garraia-auth` verde — todos os existentes + extractor tests + can() table-driven test (5 × 21 = 105 cases) + signup_pool tests + storage_redacted tests.
- [ ] `cargo test -p garraia-gateway --tests` (sem `--features auth-v1` porque a feature foi removida) → todos verdes incluindo: `auth_login`, `auth_refresh`, `auth_logout`, `auth_signup`, `extractor_authz`.
- [ ] `cargo test -p garraia-workspace --test migration_smoke` verde — extensão valida migration 010 (role garraia_signup BYPASSRLS, 4 grants positivos, 10+ grants negativos, novo SELECT do garraia_login em sessions).
- [ ] **Migration 010 aplica do zero** (após 001-009) sem erros em ≤ 200ms.
- [ ] `pg_roles` tem entry `garraia_signup` com `rolbypassrls = true` e `rolcanlogin = false`.
- [ ] `has_table_privilege('garraia_signup', 'user_identities', 'INSERT') = true`; mesmo para `users` SELECT/INSERT, `audit_events` INSERT.
- [ ] `has_table_privilege('garraia_signup', 'sessions', 'SELECT') = false` (negative — signup não vê sessions).
- [ ] `has_table_privilege('garraia_signup', 'messages', 'SELECT') = false` (negative — signup não vê chat data).
- [ ] `has_table_privilege('garraia_login', 'sessions', 'SELECT') = true` (NEW — fecha Gap A do 391b).
- [ ] **`Role` enum** parseado de `group_members.role` é roundtrip estável (text → Role → text).
- [ ] **`fn can(&Principal, Action)`** retorna o esperado para todas as 105 combinações (validado por table-driven test).
- [ ] **`Principal` extractor** roundtrip:
  - JWT válido + `X-Group-Id` válido + user é membro ativo → `Principal { user_id, group_id, role }` populado.
  - JWT válido + `X-Group-Id` ausente → `Principal { user_id, None, None }`.
  - JWT válido + `X-Group-Id` inválido (UUID malformado) → 400.
  - JWT válido + `X-Group-Id` válido mas user NÃO é membro → 403.
  - JWT ausente → 401.
  - JWT inválido (assinatura errada) → 401.
  - JWT expirado → 401.
- [ ] **`RequirePermission(TasksWrite)` extractor** integration:
  - Admin → passa.
  - Child → 403.
  - Sem `Principal` → 401.
- [ ] **Endpoint `POST /v1/auth/login`** continua retornando 200 + tokens (agora COM `refresh_token` reabilitado) + 401 byte-identical em todas as falhas (regressão do 391b).
- [ ] **Endpoint `POST /v1/auth/refresh`**:
  - Token válido → 200 + novo `access_token` + (rotacionado) novo `refresh_token`.
  - Token inválido → 401 byte-identical.
  - Token revogado → 401 byte-identical.
  - Token expirado → 401 byte-identical.
- [ ] **Endpoint `POST /v1/auth/logout`**:
  - Token válido → 204, `sessions.revoked_at` populado.
  - Token desconhecido → 204 (anti-enumeration via idempotência).
  - Re-logout do mesmo token → 204 idempotente.
- [ ] **Endpoint `POST /v1/auth/signup`**:
  - Email novo → 201 + `{user_id, access_token, refresh_token, expires_at}` + linha em `users` + linha em `user_identities` com hash Argon2id.
  - Email duplicado → 409 + audit row.
  - Conexão como `garraia_login` (não `garraia_signup`) → fail-loud na startup do bootstrap (boundary preservada).
- [ ] **`AuthConfig::from_env`** lê `GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL` e valida ≥32 bytes nos secrets.
- [ ] **Métricas Prometheus** acessíveis em `/metrics` (atrás da feature `telemetry` default-on):
  - `garraia_auth_login_total{outcome}` — counter, cardinality bounded.
  - `garraia_auth_login_latency_seconds` — histogram.
  - `garraia_auth_refresh_total{outcome}`, `garraia_auth_signup_total{outcome}`.
  - Validado por integration test que faz 1 request de cada outcome e checa `/metrics`.
- [ ] **Feature `auth-v1` REMOVIDA** do `garraia-gateway/Cargo.toml`. Default build inclui o auth router.
- [ ] **`AuthError::Storage` redactor wrapper**: integration test que injeta um `sqlx::Error::Configuration` com URL embebida e confirma que `format!("{e}")` NÃO contém `postgres://` nem credenciais.
- [ ] **Anti-enumeration regression**: `failure_modes_are_byte_identical` ainda passa após o wiring.
- [ ] **Race regression**: `concurrent_lazy_upgrade_emits_exactly_one_upgrade_row` ainda passa.
- [ ] **Smoke test wall time** garraia-workspace ≤ 35s.
- [ ] **Smoke test wall time** garraia-auth ≤ 5min (com fixture sharing via `tokio::sync::OnceCell` opcional — pode entrar em 391c se sobrar tempo, senão fica para follow-up).
- [ ] Review verde de `@security-auditor` + `@code-reviewer` + `@doc-writer` + acceptance-validator agent.
- [ ] **GAR-391 NÃO movida para Done** — comment em Linear linkando o commit e listando 391d como pending.
- [ ] `ROADMAP.md` §3.3 atualizado com `[x]` em todos os items entregues por 391c.
- [ ] `CLAUDE.md` "Estrutura de crates" atualizado refletindo o estado pós-391c.
- [ ] `crates/garraia-auth/README.md` atualizado com seção "What ships in 391c".
- [ ] `.env.example` atualizado com `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL` (comentadas, com docs inline).
- [ ] `docs/adr/0005-identity-provider.md` atualizado (ou ADR-0005-amendment) refletindo o `garraia_login` agora ter SELECT em sessions + a existência do `garraia_signup` role separado.

---

## 5. File-level changes (mapa)

```
crates/garraia-workspace/
├── migrations/
│   └── 010_signup_role_and_session_select.sql    # ★ NEW
└── tests/
    └── migration_smoke.rs                        # ★ EDIT (extension)
└── README.md                                     # ★ EDIT (mention 010)

crates/garraia-auth/
├── Cargo.toml                                    # ★ EDIT (axum dep)
├── README.md                                     # ★ EDIT (391c section)
├── src/
│   ├── lib.rs                                    # ★ EDIT (re-exports)
│   ├── role.rs                                   # ★ NEW
│   ├── action.rs                                 # ★ NEW
│   ├── can.rs                                    # ★ NEW
│   ├── extractor.rs                              # ★ NEW
│   ├── signup_pool.rs                            # ★ NEW
│   ├── storage_redacted.rs                       # ★ NEW
│   ├── error.rs                                  # ★ EDIT (Forbidden/Unauthenticated/DuplicateEmail/wrap Storage)
│   ├── types.rs                                  # ★ EDIT (Principal.role typed)
│   ├── internal.rs                               # ★ EDIT (signup_user helper, create_identity_with_pool)
│   └── jwt.rs                                    # ★ EDIT (extract_bearer_token helper)
└── tests/
    ├── extractor.rs                              # ★ NEW
    ├── signup_flow.rs                            # ★ NEW
    └── verify_internal.rs                        # ★ EDIT (refresh tokens reabilitados)

crates/garraia-config/
├── src/
│   └── auth.rs                                   # ★ NEW (or section in lib.rs)

crates/garraia-gateway/
├── Cargo.toml                                    # ★ EDIT (remove auth-v1, garraia-auth default)
├── src/
│   ├── lib.rs                                    # ★ EDIT (un-feature-gate auth_routes)
│   ├── state.rs                                  # ★ EDIT (auth fields)
│   ├── bootstrap.rs                              # ★ EDIT (build pools/issuer/store)
│   ├── server.rs                                 # ★ EDIT (mount router default)
│   ├── auth_routes.rs                            # ★ EDIT (new endpoints + AppState wiring)
│   └── auth_metrics.rs                           # ★ NEW
└── tests/
    ├── auth_login.rs                             # ★ RENAME from auth_v1_login.rs + edit
    ├── auth_refresh.rs                           # ★ NEW
    ├── auth_logout.rs                            # ★ NEW
    ├── auth_signup.rs                            # ★ NEW
    └── extractor_authz.rs                        # ★ NEW

# Meta-files
ROADMAP.md                                        # ★ EDIT (§3.3 [x])
CLAUDE.md                                         # ★ EDIT (estrutura de crates)
.env.example                                      # ★ EDIT (new env vars)
docs/adr/0005-identity-provider.md                # ★ EDIT (amendment)
plans/0012-gar-391c-extractor-and-wiring.md       # ★ NEW (this file)
```

---

## 6. Migration 010 SQL (skeleton — reference for impl-workspace-migration agent)

```sql
-- 010_signup_role_and_session_select.sql
-- GAR-391c — Signup pool role + session SELECT grant for refresh flow.
-- Plan:     plans/0012-gar-391c-extractor-and-wiring.md
-- ADR:      docs/adr/0005-identity-provider.md (will receive amendment in 391c)
-- Depends:  migrations 001 (users, user_identities, sessions, audit_events),
--           002 (audit_events shape), 008 (garraia_login role).
-- Forward-only. No DROP, no destructive ALTER.
--
-- Two purposes in one migration:
-- (a) Create dedicated `garraia_signup` BYPASSRLS role for the signup flow.
--     The login role MUST NOT be reused: signup needs INSERT on user_identities
--     and the login role's whole point is minimal credential-verification scope.
-- (b) Add `GRANT SELECT ON sessions TO garraia_login` to fix Gap A discovered
--     during GAR-391b (INSERT ... RETURNING id and verify_refresh both need
--     SELECT on the returned/queried columns).
--
-- ─── Threat model (signup role) ─────────────────────────────────────────
-- COMPROMISE OF garraia_signup = ability to create arbitrary identities.
-- Less critical than login pool (which exposes existing credentials) but
-- still a tenant-onboarding attack vector. Mitigation:
-- - Network isolation (separate Unix socket / firewall rule).
-- - Distinct vault entry (GAR-410).
-- - Rate limiting on signup endpoint (deferred to 391c follow-up).
-- - pgaudit on user_identities INSERT.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_signup') THEN
        CREATE ROLE garraia_signup NOLOGIN BYPASSRLS;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO garraia_signup;

-- Read users to detect duplicate-email + RETURNING id on INSERT.
GRANT SELECT, INSERT ON users TO garraia_signup;

-- Read user_identities to detect duplicate-provider_sub + RETURNING id on INSERT.
GRANT SELECT, INSERT ON user_identities TO garraia_signup;

-- Insert audit_events to log every signup attempt (success and failure).
GRANT INSERT ON audit_events TO garraia_signup;

-- ─── Login role: add SELECT on sessions (closes 391b Gap A) ───────────
-- INSERT ... RETURNING id requires SELECT on the returned columns.
-- verify_refresh requires SELECT on refresh_token_hash for lookup.
-- Both operations are part of the login flow (issue + verify), so the
-- grant is consistent with the login role's purpose.
GRANT SELECT ON sessions TO garraia_login;

COMMENT ON ROLE garraia_signup IS
    'BYPASSRLS dedicated role for the garraia-auth SIGNUP flow. NOLOGIN by '
    'default — production deployments must promote via ALTER ROLE WITH LOGIN '
    'PASSWORD. Compromise = ability to create arbitrary identities. NOT a '
    'substitute for garraia_login: this role has INSERT on users/user_identities '
    'but no read access to sessions or any tenant data. See ADR 0005 amendment '
    '(GAR-391c) and migration 010 comment block.';
```

---

## 7. Test plan summary

Five new test files + 1 extension. All run against pgvector/pg16 testcontainer.

1. **`crates/garraia-auth/tests/extractor.rs`** — 7 cases: valid JWT positive, missing JWT, invalid JWT, expired JWT, valid JWT + invalid group_id, valid JWT + non-member group_id, RequirePermission positive + negative.
2. **`crates/garraia-auth/tests/signup_flow.rs`** — 3 cases: happy path, duplicate email, signup pool role validation (constructor with wrong role fails).
3. **`crates/garraia-gateway/tests/auth_refresh.rs`** — 4 cases: happy + 3 failure modes (byte-identical 401).
4. **`crates/garraia-gateway/tests/auth_logout.rs`** — 3 cases: happy, idempotent, unknown token.
5. **`crates/garraia-gateway/tests/auth_signup.rs`** — 3 cases: happy, duplicate email, weak password (or skip if validation deferred).
6. **`crates/garraia-gateway/tests/extractor_authz.rs`** — integration of Principal + RequirePermission via a fake protected route.
7. **EXTENSION** to `crates/garraia-workspace/tests/migration_smoke.rs` — migration 010 block (role + 3 positive grants + 5 negative grants + new SELECT on sessions).
8. **EXTENSION** to `crates/garraia-auth/src/{can.rs}` — table-driven unit test (105 cases).

Test wall time budget: garraia-auth ≤ 5min, garraia-gateway ≤ 3min, garraia-workspace ≤ 35s.

---

## 8. Rollback plan

1. **Before merge:** close PR.
2. **After merge:** `git revert` is non-trivial because the wiring in `bootstrap.rs`/`server.rs` is interdependent. Forward-fix is the canonical path. The `auth-v1` feature flag was removed — to disable the auth router entirely, a new commit must either (a) re-introduce the feature flag or (b) gate the routes behind config (`auth.enabled = false`). Recommendation: ship `auth.enabled` config default-true in 391c so future kill-switch is one config change away.
3. **DB:** migration 010 is forward-only, additive (CREATE ROLE + GRANT). To "undo": `DROP ROLE garraia_signup; REVOKE SELECT ON sessions FROM garraia_login;` — must be done manually if needed, never via migration revert.

---

## 9. Risks & mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Wiring breaks `mobile_auth.rs` legacy endpoint | Medium | High | mobile_auth.rs continues to read `GARRAIA_JWT_SECRET` directly; the new `AuthConfig` reads the same env var. Tests verify mobile_auth still issues valid tokens. |
| Wave 1 parallel agents conflict in `lib.rs` re-exports | High | Low | Each agent edits a distinct labeled block (`// 391c-impl-A`, `// 391c-impl-B`); I coordinate the merge in Wave 1.5. |
| `Principal` extractor query (`SELECT role FROM group_members`) is on the hot path of every authenticated request — performance | Medium | Medium | The query is single-row indexed lookup. Cache via `dashmap` keyed on `(user_id, group_id)` deferred to 391d if profiling shows it. |
| `garraia_signup` role accidentally reused in production for non-signup queries | Low | High | Code review enforces `SignupPool` newtype (analogous to `LoginPool`); compile-time `!Clone` boundary. |
| `AuthError::Storage` redactor fails to strip a new sqlx error variant | Medium | Medium | Property-test or fuzz-style test in `storage_redacted.rs::tests` covers known variants; future variants caught by integration test that injects a synthetic error. |
| Refresh token rotation breaks active mobile sessions on first refresh | Medium | Medium | Rotation is opt-in via config flag default ON; mobile clients already handle rotation per `_AuthInterceptor`. |
| `migration_smoke.rs` extension adds significant wall time | Low | Low | +6 asserts, ~1s. Within budget. |
| `garraia-config::AuthConfig::from_env` race with existing `garraia-config` from_env that may set the same env vars | Low | Medium | `AuthConfig::from_env` is a separate constructor; existing config tests use isolated env vars. |
| Plan 0012 `Action` enum doesn't match the `permissions` table seed exactly | Medium | Medium | Wave 0 reconnaissance includes diffing `Action` enum proposal vs `migration 002 seed`. |
| Extractor query uses `LoginPool` (BYPASSRLS) for membership lookup — leaks tenant data? | Low | Medium | The query is `SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2 AND status = 'active'` — narrow, single-row, scoped to the authenticated user. Documented as acceptable. Long-term: dedicated read-only pool. |
| Multi-agent worktree merge has unexpected diff conflicts | Medium | Low | Wave 1.5 reconciliation step explicitly resolves them; if a conflict is non-trivial, pause + report. |

---

## 10. Sequence of work — multi-agent waves (when approved)

This is the operational rule the user established this session. Multi-agent execution is the default for any non-trivial delivery.

### Wave 0 — Reconnaissance (1 agent, sequential, ~5min wall)

- **`recon-391c`** (general-purpose, isolation: worktree)
  - Read migrations 001/002/008/009 to confirm: `users.status` enum, `user_identities` shape, `sessions` columns + indexes, `audit_events` shape, `group_members` shape + `role` CHECK enum, `roles` seed (5 rows), `permissions` seed (22 rows), `role_permissions` seed (63 rows).
  - Read `crates/garraia-config/src/lib.rs` to understand the existing config loading pattern.
  - Read `crates/garraia-gateway/src/{state,bootstrap,server,mobile_auth}.rs` to map every AppState touch point.
  - Cross-check the proposed `Action` enum + capability mapping against the actual `permissions` + `role_permissions` seed.
  - Confirm Gap A (`garraia_login` lacks SELECT on sessions) and Gap B (`garraia_login` lacks INSERT on user_identities) match the 391b finding.
  - **Output:** `recon-391c.md` report with: schema gaps (if any), capability mapping diff, AppState surface checklist, go/no-go gate.

**Gate:** if any new structural gap found, PAUSE and report (per user rule).

### Wave 1 — Implementation parallel (3 agents, ~25min wall)

- **`impl-workspace-migration`** (general-purpose, worktree A) — migration 010 + smoke test extension. Returns: diff + `cargo test -p garraia-workspace --test migration_smoke` green.
- **`impl-auth-types`** (general-purpose, worktree B) — `role.rs`, `action.rs`, `can.rs`, `extractor.rs`, `tests/extractor.rs`, edits to `types.rs`/`error.rs`/`lib.rs` (re-exports labeled). Returns: diff + `cargo check -p garraia-auth` green.
- **`impl-auth-impl`** (general-purpose, worktree C) — `signup_pool.rs`, `storage_redacted.rs`, `internal.rs::signup_user`, `tests/signup_flow.rs`, edits to `lib.rs` (labeled). Returns: diff + `cargo check -p garraia-auth` green.

### Wave 1.5 — Sync + gateway wiring (1 agent / me, sequential, ~25min wall)

- I merge worktrees A/B/C into `main` resolving labeled `lib.rs` blocks.
- Then **`impl-gateway-wiring`** (or me directly if conflicts are simple) — `state.rs`, `bootstrap.rs`, `server.rs`, `auth_routes.rs`, `auth_metrics.rs`, `Cargo.toml` (remove auth-v1), `garraia-config::auth.rs`, all integration tests in `garraia-gateway/tests/`. Returns: full workspace check + integration tests green.

### Wave 2 — Reviews parallel (4 agents, ~15min wall)

- **`@security-auditor`** — extractor, can(), JWT, signup pool, migration 010, redactor, refresh rotation, anti-enumeration regression.
- **`@code-reviewer`** — Rust idioms, no unwrap, ? propagation, async, Arc usage, sql binds, naming, dead code, feature flag clean removal.
- **`@doc-writer`** — ADR 0005 amendment consistency, doc comments completeness, README update, plan 0012 still valid.
- **`acceptance-validator`** (general-purpose) — runs the full `cargo test` matrix, validates each §4 acceptance criterion item-by-item, output green/red checklist.

All 4 in parallel via `run_in_background: true`.

### Wave 3 — Fixes (me, sequential, ~15min wall)

Apply review findings inline.

### Wave 4 — Docs/Linear audit parallel (2 agents, ~10min wall)

- **`doc-roadmap-audit`** — ROADMAP.md / CLAUDE.md / README.md / .env.example / .gitignore staleness check.
- **`linear-state-audit`** — Linear Fase 3 state via `mcp__claude_ai_Linear__list_issues`, identify what to comment, list issues NOT moved.

I apply doc updates manually + post Linear comment manually.

### Wave 5 — Commit + push (me, sequential, ~10min wall)

Single commit with the multi-agent wave outputs documented in the message.

**Total wall estimated:** ~100min. Vs single-agent ~3-4h. Speedup ~2x.

---

## 11. Definition of Done

- [ ] All §4 acceptance criteria green.
- [ ] PR merged em `main`.
- [ ] Review verde de @security-auditor + @code-reviewer + @doc-writer + acceptance-validator.
- [ ] **GAR-391 NÃO movida para Done** — comment em Linear listando 391d como pending.
- [ ] Final summary contém a seção obrigatória "Agentes realmente usados" + "Linear — issues atualizadas".
- [ ] 391d pode começar imediatamente sem refactor de extractor/can()/Role/AppState.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`Action` enum cobre tudo do `permissions` seed (22 rows) ou só Tasks + Chats por agora?** Recomendo **cobrir tudo** porque o mapeamento role × action é estático e o custo extra é zero linhas de Rust mas economiza um deferral. Confirma?

2. **`Principal` extractor query usa `LoginPool` (BYPASSRLS) para membership lookup?** Recomendo **sim** em 391c para evitar criar mais um pool. A query é narrow (single row, indexed). Long-term (391d ou Fase 3.4) podemos avaliar um read-only pool. Confirma?

3. **Refresh token rotation default ON ou OFF em 391c?** Recomendo **ON** (rotaciona em cada refresh) — reduz blast radius de leak. Mobile client já lida com rotation via interceptor. Confirma?

4. **`/v1/auth/signup` retorna 201 + tokens (auto-login) ou 201 sem tokens (require explicit login após signup)?** Recomendo **201 + tokens** (auto-login) — UX padrão moderno. Anti-enumeration via 409 explícito em duplicate email (não é problema porque login já vaza isso via 401 timing). Confirma?

5. **`mobile_auth.rs` legacy continua coexistindo ou é removido em 391c?** Recomendo **manter coexistindo** — remoção é GAR-413 (migração mobile_users → user_identities). 391c só wira o NEW path. Confirma?

6. **`AuthConfig` em `garraia-config` ou em arquivo próprio `garraia-auth-config`?** Recomendo **dentro de `garraia-config`** (seção `auth`) — alinhado com o pattern existente; evita criar mais um crate. Confirma?

7. **Métricas Prometheus baseline: bounded outcome enum ou wildcard?** Recomendo **bounded enum fechado** (success/failure_invalid_credentials/failure_account_inactive/failure_internal/failure_unknown_hash/failure_duplicate_email) para cardinalidade fixa. Confirma?

8. **Rate limiting do signup endpoint:** in-scope ou follow-up? Recomendo **follow-up** (`tower-governor` é trabalho separado, ~1h, melhor isolar). Em 391c apenas TODO documentado + audit cobrindo signup attempts. Confirma?

9. **Plan 0012.1 (sub-plan) ou inline para o ADR-0005 amendment?** Recomendo **inline** — o amendment é uma seção nova ("§Amendment 2026-04-13: Signup role + login session SELECT") no ADR existente, escrita pelo `@doc-writer` agent na Wave 4. Confirma?

10. **Worktree isolation no Wave 1 (paralelismo real) ou trabalho direto em `main` com discipline?** Recomendo **worktrees** — isolation é o ponto da regra multi-agent. Vale o overhead de minutos. Confirma?

---

## 13. Impact on other components

### 13.1 GAR-391d / GAR-392 (cross-group authz suite)

391d ganha tudo que precisa: `Principal` extractor real, `RequirePermission(Action)`, `Role` enum, `fn can()`, todos os endpoints `/v1/auth/*` reais. A suite cross-group pode começar imediatamente após 391c merge. Estimado 5-8h para os ≥100 cenários.

### 13.2 GAR-393 (REST `/v1/groups`)

Desbloqueado por 391c — toda rota REST de Fase 3.4 vai usar `RequirePermission(GroupRead/Write/Manage)`. Pode começar em paralelo com 391d.

### 13.3 GAR-413 (`garraia-cli migrate workspace`)

Sem mudança direta. Pode começar quando time permitir. A coexistência de `mobile_auth.rs` é compatível.

### 13.4 GAR-410 (vault separado para login pool credentials)

391c lê via env vars direto. Migração para vault fica para 391c follow-up ou GAR-410.

---

## 14. Next recommended issue (after GAR-391c merged)

**GAR-392 / 391d — Suite cross-group authz (≥100 cenários).** Plano dedicado `plans/0013-gar-392-cross-group-authz-suite.md`. Entrega o último critério de aceite do epic GAR-391 (§3.3 do roadmap).

Estimado 5-8h. Multi-agent waves: 1 recon agent (cataloga rotas + RBAC matrix) + 1 implementer (gera os ≥100 test cases) + 2 review agents (security + code) + acceptance validator. Em paralelo onde possível.

Após 391d: o epic GAR-391 fecha. O próximo passo natural é **GAR-393** (REST /v1/groups CRUD) ou um detour para o ADR 0004 (object storage).

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 10 open questions com os defaults recomendados, e executo seguindo o §10 multi-agent waves. O resumo final terá obrigatoriamente:

- **Resultado da entrega**
- **Amendment do plano 0012** (se houver)
- **ROADMAP / docs audit**
- **Agentes realmente usados** (formato fixo: agente, função, executou, artefato, findings, paralelo?)
- **Linear — issues atualizadas**
