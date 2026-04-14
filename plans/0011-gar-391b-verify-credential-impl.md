# Plan 0011: GAR-391b — `verify_credential` real impl + audit + JWT issuance

> **⚠️ Prereq estrutural satisfeito:** Wave 0 reconnaissance descobriu que
> `user_identities.hash_upgraded_at` não existia. Tratado separadamente em
> [plan 0011.5](0011.5-gar-391b-migration-009-hash-upgraded-at.md) /
> migration 009. **Não tentar implementar 391b antes do merge da
> migration 009.** O resto do schema necessário (`users.status`,
> `sessions.refresh_token_hash`, `audit_events`) já existe.
>
> **⚠️ Correção inline pendente para Wave 1 do 391b:** o §6.3 deste plano
> descreveu a `audit_events` com nomes errados. O schema real (migration
> 002) usa `actor_user_id` (não `actor_id`), `resource_type` (não
> `target_type`), `resource_id text` (não `target_id uuid`), `ip inet`
> e `user_agent text` como colunas top-level (não keys do jsonb), e
> `created_at` (não `occurred_at`). O `audit.rs` em 391b deve ser
> implementado contra o schema real, não contra a descrição deste §6.3.

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — sub-issue 391b (real verify + audit + JWT)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-authz`, `security`
> **Priority:** Urgent
> **Estimated session size:** 6-9 horas focado (maior das quatro fatias)
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-14
> **Depends on:** ✅ GAR-391a (skeleton + `LoginPool` + migration 008) + ✅ GAR-375 (ADR 0005 normativo) + ✅ GAR-407/408/390 (schema + RLS + tasks)
> **Unblocks:** **GAR-391c** (Axum extractor + `RequirePermission` + gateway wiring) + **GAR-391d / GAR-392** (cross-group authz suite)

---

## 1. Goal (one sentence)

Implementar o corpo real de `InternalProvider::verify_credential` em `garraia-auth`, cobrindo (a) lookup do `user_identities` via `LoginPool` BYPASSRLS com `SELECT ... FOR NO KEY UPDATE OF ui` para evitar race no lazy upgrade, (b) dual-verify Argon2id PHC nativo + PBKDF2 legado com upgrade transacional in-place, (c) checagem de `users.status = 'active'` (rejeitando `suspended`/`deleted`), (d) caminho constant-time de anti-enumeração via `DUMMY_HASH` Argon2id pré-computado em build script, (e) inserção de `audit_events` para `login.success`, `login.failure_user_not_found`, `login.failure_wrong_password`, `login.failure_account_suspended`, `login.password_hash_upgraded` recebendo `RequestCtx`, (f) emissão de JWT HS256 access token (15min) + refresh token opaco (32 bytes URL-safe) com `HMAC-SHA256` armazenado em `sessions`, e (g) endpoint `POST /v1/auth/login` em `garraia-gateway` montado dentro de uma feature-gated subrouter (sem wiring no `AppState` ainda — wiring fica em 391c). Tudo fechado por suite de testes integration cobrindo cada path positivo/negativo + duas regressões de timing.

---

## 2. Rationale — por que essa fatia segunda

1. **Skeleton já está no lugar.** GAR-391a entregou o trait, o `LoginPool`, a role `garraia_login`, os tipos. 391b pode focar 100% em verificação + audit + JWT sem touchar nada arquitetural.
2. **Crypto + DB lock + audit + JWT é a parte de maior risco do epic.** Concentrar tudo aqui isola o blast radius e mantém 391c (extractor + wiring) curto e baixo-risco.
3. **`SELECT ... FOR NO KEY UPDATE OF ui` foi flagrado pelo security review do ADR 0005** (H1 — race window entre verify e UPDATE do upgrade). Sem ele, dois logins concorrentes do mesmo PBKDF2 user podem disparar dois `UPDATE password_hash`, um deles sobrescrevendo o trabalho do outro com mesma resultado mas sem garantia de ordenação. O lock fecha a janela.
4. **Audit antes de extractor.** `audit_events` é load-bearing para forensics e LGPD — precisa estar no path crítico desde a primeira request real, não como retrofit.
5. **JWT antes de extractor.** O extractor (391c) precisa de tokens reais para validar — ship JWT primeiro evita ter que rebuildar `Principal::from_jwt` depois.
6. **Endpoint sob feature flag.** O endpoint `/v1/auth/login` entra sob feature gate `auth-v1` no `garraia-gateway` para ficar plumbed sem ser default-on. 391c remove a gate quando o extractor for wirado.

---

## 3. Scope & Non-Scope

### In scope

**Edits em `crates/garraia-auth/`:**

- `src/internal.rs` — substitui o `Err(NotImplemented)` de `verify_credential` pela implementação real (~150-200 linhas):
  - Lookup `user_identities` por `provider='internal'` AND `provider_sub=lower(email)` via `LoginPool`
  - `SELECT ... FOR NO KEY UPDATE OF ui` dentro de transação
  - JOIN com `users` para checar `status = 'active'`
  - Dispatch por prefixo do hash: `$argon2id$...` → Argon2id verify, `$pbkdf2-sha256$...` → PBKDF2 verify + lazy upgrade, qualquer outra coisa → `AuthError::UnknownHashFormat`
  - Constant-time path: se o user não existe OU está suspenso, continuar verificando contra `DUMMY_HASH` para igualar timing profile
  - `audit_events` insertion em todos os terminais
  - Retorno: `Ok(Some(user_id))` no sucesso, `Ok(None)` em qualquer falha que NÃO seja erro de storage
- `src/internal.rs` — implementa `find_by_provider_sub` real (lookup simples por `(provider, provider_sub)` retornando `Identity`)
- `src/internal.rs` — implementa `create_identity` real (INSERT em `user_identities` com Argon2id hash do password)
- `src/audit.rs` (NOVO) — `audit_login(pool, action, user_id_opt, request_ctx, error_label) -> Result<()>`. Centraliza o INSERT em `audit_events` com colunas `actor_id`, `action`, `target_type='user_identities'`, `target_id`, `metadata` (jsonb com ip/user_agent/request_id), `actor_label` cache.
- `src/hashing.rs` (NOVO) — `verify_argon2id(hash, password) -> Result<bool>`, `verify_pbkdf2(hash, password) -> Result<bool>`, `hash_argon2id(password) -> Result<String>`. Usa `argon2 = "0.5"` crate (RustCrypto, RFC 9106 first recommendation params: `m=64MiB, t=3, p=4`). PBKDF2 verify usa `pbkdf2 = "0.12"` + `sha2`. Centraliza erros.
- `src/jwt.rs` (NOVO) — `JwtIssuer::new(secret) -> Self`, `JwtIssuer::issue_access(user_id, group_id, exp_secs) -> Result<String>`, `JwtIssuer::verify_access(token) -> Result<Claims>`, `JwtIssuer::issue_refresh() -> RefreshToken { plaintext, hmac_hash }`. Usa `jsonwebtoken = "9"` (já no workspace) + `hmac = "0.12"` + `sha2` para refresh hash.
- `src/sessions.rs` (NOVO) — `SessionStore::new(login_pool)`, `SessionStore::issue(user_id, refresh_hmac, ip, user_agent, request_id, expires_at) -> Result<SessionId>`, `SessionStore::verify_refresh(refresh_plaintext) -> Result<Option<(SessionId, user_id)>>`, `SessionStore::revoke(SessionId) -> Result<()>`. INSERT/UPDATE em `sessions`.
- `src/error.rs` — adicionar variants `AccountSuspended`, `AccountDeleted`, `JwtIssue(jsonwebtoken::errors::Error)`, `Hashing(String)`. Atualizar doc warning (já existente) para nova variante `Hashing` (não embute password).
- `src/types.rs` — adicionar `LoginOutcome { user_id, access_token, refresh_token, expires_at }` para o endpoint compor a response.
- `src/lib.rs` — re-exports novos: `audit_login`, `JwtIssuer`, `JwtConfig`, `SessionStore`, `LoginOutcome`.
- `Cargo.toml` — novas deps: `argon2 = "0.5"`, `pbkdf2 = "0.12"`, `password-hash = "0.5"`, `sha2 = "0.10"`, `hmac = "0.12"`, `subtle = "2.6"` (para ct_eq do refresh hash), `secrecy = "0.10"` (`SecretString` para passwords e jwt secret), `serde_json` (já), `jsonwebtoken = { workspace = true }`. Build-time: `build-dependencies = { argon2 = "0.5" }` para gerar `DUMMY_HASH`.
- `build.rs` (NOVO) — gera `dummy_hash.rs` em `OUT_DIR` com a constante `DUMMY_HASH: &str` contendo um Argon2id PHC string de uma senha aleatória descartada. Build script roda uma vez por target; `include!(concat!(env!("OUT_DIR"), "/dummy_hash.rs"))` dentro de `hashing.rs`.

**Edits em `crates/garraia-workspace/`:**

- **NENHUMA migration nova** em 391b. O schema `user_identities`/`sessions`/`audit_events`/`users` já tem todas as colunas necessárias (validado em GAR-407/408/390). Se descobrirmos no meio do caminho que falta uma coluna (ex.: `users.status` não existe), abrimos sub-issue separada e bloqueamos 391b até resolver.
- `tests/migration_smoke.rs` — **intocado.** Integration tests do verify path vivem em `garraia-auth/tests/`.

**Edits em `crates/garraia-gateway/`:**

- `src/auth_routes.rs` (NOVO, feature-gated em `auth-v1`) — `POST /v1/auth/login` handler. Recebe `LoginRequest { email, password }`, monta `Credential::Internal`, extrai `RequestCtx` dos headers (`X-Forwarded-For`, `User-Agent`, `X-Request-ID`), chama `provider.verify_credential(...)`, em sucesso chama `JwtIssuer::issue_access` + `SessionStore::issue`, retorna `LoginResponse { access_token, refresh_token, expires_at }`. Em falha retorna `401 invalid credentials` (mesmo body para todos os modos de falha — anti-enumeração).
- `src/lib.rs` ou `src/server.rs` — adiciona `#[cfg(feature = "auth-v1")] mod auth_routes;` e merge da subrouter quando a feature está ativa. Wiring no `AppState` é deferido para 391c — em 391b o handler recebe `State<Arc<dyn IdentityProvider>>` via uma subrouter standalone que o teste de integração instancia diretamente.
- `Cargo.toml` — adicionar `[features] auth-v1 = ["dep:garraia-auth"]`, `[dependencies] garraia-auth = { workspace = true, optional = true }`.

**Plano + Linear:**

- `plans/0011-gar-391b-verify-credential-impl.md` (este arquivo) committed junto.
- Comentar em GAR-391 (sem mover): "391b merged em commit `<hash>`. 391c (extractor + wiring), 391d (authz suite) ainda pending."
- **NÃO mover GAR-391 para Done.**

### Out of scope (deferido a 391c/d)

- ❌ **Axum `Principal` extractor** — 391c.
- ❌ **`FromRequestParts<Arc<AppState>> for Principal`** — 391c.
- ❌ **`RequirePermission(Action)` extractor** — 391c.
- ❌ **`Role` enum tipado substituindo `Option<String>` em `Principal`** — 391c.
- ❌ **`fn can(principal, action) -> bool` capability check** — 391c.
- ❌ **Wiring do `Arc<dyn IdentityProvider>` no `AppState` global do gateway** — 391c. Em 391b o endpoint usa uma subrouter standalone com state injetado por hand, escondida atrás da feature `auth-v1`.
- ❌ **Cross-group authz suite (≥100 cenários)** — 391d / GAR-392.
- ❌ **Refresh endpoint (`POST /v1/auth/refresh`)** — 391c. Em 391b o `SessionStore::verify_refresh` existe mas só é chamado por testes; o endpoint público entra com o extractor.
- ❌ **Logout endpoint (`POST /v1/auth/logout`)** — 391c.
- ❌ **Signup endpoint (`POST /v1/auth/signup`)** — 391c (pode ir para 391c.5 se ficar grande). `create_identity` real existe em 391b, só não tem rota.
- ❌ **OIDC adapter** — futuro ADR 0009.
- ❌ **Rate limiting** (`tower-governor`) — 391c ou follow-up dedicado.
- ❌ **Migração de `mobile_users` para `user_identities`** — GAR-413, executa depois que 391b/c shippam.
- ❌ **CAPTCHA / lockout após N failures** — fora do epic 391; futura issue de hardening.
- ❌ **mTLS / client cert auth** — futuro ADR.
- ❌ **`AppState` modifications no `garraia-gateway`** — 391c. 391b NÃO toca em `state.rs`, `bootstrap.rs`, ou qualquer outro AppState wiring. Adiciona apenas um módulo isolado sob feature.
- ❌ **Métricas Prometheus de login** — `garraia-telemetry` integração entra em 391c quando o extractor estiver no hot path.
- ❌ **Account recovery / password reset** — futuro epic.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check -p garraia-auth` verde.
- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --features auth-v1` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-auth --all-targets -- -D warnings` verde.
- [ ] `cargo clippy --workspace --features auth-v1 -- -D warnings` verde (no novo warning fora de garraia-auth/garraia-gateway).
- [ ] `cargo test -p garraia-auth` verde — todos os novos integration tests rodam.
- [ ] `cargo test -p garraia-workspace --test migration_smoke` ainda verde (não tocada).
- [ ] `cargo test -p garraia-gateway --features auth-v1` verde — endpoint integration test passa.
- [ ] **Build.rs gera DUMMY_HASH em compile time** — verificável por `cargo expand` ou inspeção do binário; teste unit valida que `DUMMY_HASH` começa com `$argon2id$`.
- [ ] **Argon2id parameters batem com RFC 9106 first recommendation** — `m_cost=64*1024 (64 MiB), t_cost=3, p_cost=4`. Teste unit valida via parsing do PHC string gerado por `hash_argon2id`.
- [ ] **Verify positivo Argon2id**: user com hash Argon2id passa, retorna `Some(user_id)` + audit row `login.success` + JWT válido + sessions row.
- [ ] **Verify positivo PBKDF2 com lazy upgrade**: user com hash PBKDF2 passa, retorna `Some(user_id)`, e DEPOIS da call o `password_hash` no DB começa com `$argon2id$`. Audit `login.password_hash_upgraded` é inserido junto com `login.success`.
- [ ] **Race regression test (lazy upgrade)**: dois `verify_credential` concorrentes para o mesmo PBKDF2 user — exatamente um faz upgrade, o outro vê o hash já atualizado e não faz UPDATE redundante. Verificável via contagem de `audit_events.action='login.password_hash_upgraded'` = 1, não 2. Garantido pelo `FOR NO KEY UPDATE OF ui`.
- [ ] **Verify negativo wrong password**: retorna `Ok(None)`, audit `login.failure_wrong_password`, NENHUM JWT emitido, NENHUM session row.
- [ ] **Verify negativo user not found**: retorna `Ok(None)`, audit `login.failure_user_not_found` com `actor_id=NULL`, **timing está dentro de 25% do timing do verify positivo Argon2id** (constant-time path). Teste tem 3 sub-runs e checa o desvio padrão.
- [ ] **Verify negativo account suspended**: user existe, password correto, mas `users.status='suspended'`. Retorna `Ok(None)`, audit `login.failure_account_suspended`. Timing também está dentro de 25%.
- [ ] **`AuthError::UnknownHashFormat`**: hash com prefixo desconhecido (ex.: `$bcrypt$...`) retorna o erro estruturado. NÃO retorna `Ok(None)` — esse é um erro de configuração, não de credencial.
- [ ] **JWT access token**: HS256, contém `sub=user_id`, `iat`, `exp` (15min), `iss="garraia-gateway"`. Verificável via decode com mesma secret.
- [ ] **Refresh token**: 32 bytes random URL-safe base64, hash via HMAC-SHA256 com chave separada da JWT secret, armazenado em `sessions.refresh_token_hash`. `verify_refresh` faz `subtle::ConstantTimeEq` para evitar timing leak.
- [ ] **`audit_events` row em sucesso**: `actor_id=user_id`, `action='login.success'`, `target_type='user_identities'`, `target_id=identity.id`, `metadata` jsonb com `ip`, `user_agent`, `request_id` (todos opcionais — `null` se ausentes), `actor_label` snapshot do email para survival LGPD.
- [ ] **`audit_events` row em falha**: `actor_id=NULL` para user_not_found; `actor_id=user_id` para outros modos; `metadata` sempre presente.
- [ ] **Endpoint `POST /v1/auth/login` integration test**: 200 com response body válido em sucesso; 401 com body `{"error":"invalid_credentials"}` em todos os modos de falha (mesmo body — anti-enumeração); 400 em request body malformado.
- [ ] **Anti-enumeração via response shape**: tester não consegue distinguir "user inexistente" de "wrong password" via body, status code, ou headers da response. Validado por integration test que faz 3 requests (user inexistente, user existente + wrong password, user suspended) e compara raw bytes da response.
- [ ] **PII redaction**: `tracing::error!` em qualquer path do login NÃO contém o password, NÃO contém o `database_url`, NÃO contém o JWT secret. Validado por integration test que captura span output.
- [ ] **`secrecy::SecretString` no caminho do password**: o `Credential::Internal.password` é envolto em `SecretString`, não em `String` puro. Drop zera a memória. (Breaking change em `Credential::Internal` — variant fica `Internal { email: String, password: SecretString }`.)
- [ ] **Forward-only**: nenhum `DROP`, nenhum `ALTER` destrutivo, nenhuma migration nova.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] **GAR-391 NÃO movida para Done** — comment em Linear linkando o commit e listando 391c/d como pending.
- [ ] `ROADMAP.md` §3.3 atualizado — items adicionais marcados `[x]` (verify_credential, JWT, audit_login, lazy upgrade, sessions store, login endpoint).
- [ ] `CLAUDE.md` "Estrutura de crates" — descrição de `garraia-auth` atualizada para refletir o estado pós-391b.
- [ ] `crates/garraia-auth/README.md` atualizado — seção "What ships in 391b" adicionada.

---

## 5. File-level changes

### 5.1 Edits em `crates/garraia-auth/`

```
crates/garraia-auth/
├── Cargo.toml                          (★ +deps de crypto/JWT/secrecy + build-deps)
├── build.rs                            (★ NEW — gera DUMMY_HASH)
├── README.md                           (★ +seção 391b)
├── src/
│   ├── lib.rs                          (★ +re-exports)
│   ├── error.rs                        (★ +variants AccountSuspended/AccountDeleted/JwtIssue/Hashing)
│   ├── types.rs                        (★ Credential.password → SecretString; +LoginOutcome)
│   ├── provider.rs                     (intocado — trait shape congelada)
│   ├── internal.rs                     (★ verify_credential/find_by_provider_sub/create_identity REAIS)
│   ├── login_pool.rs                   (★ +pub(crate) fn pool() -> &PgPool accessor)
│   ├── audit.rs                        (★ NEW — audit_login helper)
│   ├── hashing.rs                      (★ NEW — Argon2id + PBKDF2 + DUMMY_HASH)
│   ├── jwt.rs                          (★ NEW — JwtIssuer + Claims)
│   └── sessions.rs                     (★ NEW — SessionStore)
└── tests/
    ├── skeleton.rs                     (★ atualizado — internal_provider stubs viram smoke real;
    │                                     pode virar smoke.rs e ganhar cenários positivos)
    ├── verify_internal.rs              (★ NEW — Argon2id + PBKDF2 + lazy upgrade + race regression)
    ├── verify_negative.rs              (★ NEW — user not found / wrong password / suspended +
    │                                     timing variance check)
    ├── audit_trail.rs                  (★ NEW — audit_events insertion validation)
    ├── jwt_lifecycle.rs                (★ NEW — issue + verify + expiry)
    └── sessions_store.rs               (★ NEW — refresh issue + verify + revoke + ct_eq)
```

### 5.2 Edits em `crates/garraia-gateway/`

```
crates/garraia-gateway/
├── Cargo.toml                          (★ [features] auth-v1 + optional dep garraia-auth)
└── src/
    └── auth_routes.rs                  (★ NEW, #[cfg(feature = "auth-v1")] —
                                          POST /v1/auth/login + integration test fixture)
```

### 5.3 Edits em meta-files

- `Cargo.toml` (workspace root) — adicionar deps novas em `[workspace.dependencies]`: `argon2`, `pbkdf2`, `password-hash`, `sha2`, `hmac`, `subtle`, `secrecy`. (`jsonwebtoken` já está.)
- `ROADMAP.md` §3.3 — marcar `[x]` em: real `verify_credential`, JWT issuance, lazy upgrade, audit_login, sessions store, login endpoint.
- `CLAUDE.md` "Estrutura de crates" — atualizar bloco `garraia-auth/` reflitindo 391b.
- `crates/garraia-auth/README.md` — adicionar seção "What ships in 391b" antes da seção "Tests".

### 5.4 Zero edits em código fora dos arquivos listados

- `garraia-gateway/src/{state.rs, bootstrap.rs, server.rs}` — intocados (wiring é 391c).
- `garraia-workspace/migrations/` — intocados (sem migration nova).
- `garraia-workspace/tests/migration_smoke.rs` — intocado.
- `garraia-config` — intocado (config do JWT secret entra em 391c quando o gateway wirar de verdade; em 391b o secret é injetado por hand em testes via env var direta).

---

## 6. Behavioral details

### 6.1 `verify_credential` flowchart

```
verify_credential(Credential::Internal { email, password }) -> Result<Option<Uuid>>
│
├─ BEGIN TRANSACTION
│
├─ SELECT ui.id, ui.user_id, ui.password_hash, u.status
│  FROM user_identities ui
│  JOIN users u ON u.id = ui.user_id
│  WHERE ui.provider = 'internal'
│    AND ui.provider_sub = lower($email)
│  FOR NO KEY UPDATE OF ui
│
├─ IF row not found:
│  ├─ verify_argon2id(DUMMY_HASH, password)   # constant-time consume
│  ├─ audit_login(action="login.failure_user_not_found", actor_id=NULL, ctx)
│  ├─ COMMIT
│  └─ return Ok(None)
│
├─ IF u.status != 'active':
│  ├─ verify_argon2id(DUMMY_HASH, password)   # constant-time consume
│  ├─ audit_login(action="login.failure_account_suspended", actor_id=user_id, ctx)
│  ├─ COMMIT
│  └─ return Ok(None)
│
├─ IF password_hash starts_with "$argon2id$":
│  ├─ verify_argon2id(password_hash, password) -> bool
│  └─ goto VERIFY_RESULT
│
├─ IF password_hash starts_with "$pbkdf2-sha256$":
│  ├─ verify_pbkdf2(password_hash, password) -> bool
│  ├─ IF verified:
│  │  ├─ new_hash = hash_argon2id(password)
│  │  ├─ UPDATE user_identities SET password_hash = $new_hash, hash_upgraded_at = now()
│  │  │  WHERE id = $ui.id
│  │  └─ audit_login(action="login.password_hash_upgraded", actor_id=user_id, ctx)
│  └─ goto VERIFY_RESULT
│
├─ ELSE:
│  ├─ # unrecognized hash format — operational error, NOT a credential failure
│  ├─ audit_login(action="login.failure_unknown_hash", actor_id=user_id, ctx)
│  ├─ ROLLBACK
│  └─ return Err(AuthError::UnknownHashFormat)
│
├─ VERIFY_RESULT:
│  ├─ IF verified:
│  │  ├─ audit_login(action="login.success", actor_id=user_id, ctx)
│  │  ├─ COMMIT
│  │  └─ return Ok(Some(user_id))
│  └─ ELSE:
│     ├─ audit_login(action="login.failure_wrong_password", actor_id=user_id, ctx)
│     ├─ COMMIT
│     └─ return Ok(None)
```

**Notes:**
- The audit row is inserted INSIDE the same transaction so a rollback would erase it. Acceptable for v1; future hardening can move audit to a separate fire-and-forget channel.
- `FOR NO KEY UPDATE OF ui` (not `FOR UPDATE`) avoids blocking concurrent reads of `users` while still preventing concurrent UPDATE of the same `user_identities` row — exactly what the lazy upgrade race needs.
- `lower($email)` matches the citext column.
- The dummy hash verify in not-found / suspended paths is a **side-effect-free** computation purely for timing parity. Its result is discarded.

### 6.2 JWT shape

```rust
struct AccessClaims {
    sub: Uuid,            // user_id
    iat: i64,             // unix seconds
    exp: i64,             // unix seconds, iat + 900
    iss: &'static str,    // "garraia-gateway"
    // group_id is NOT in the access token — the extractor (391c) resolves
    // it per-request from the X-Group-Id header + group_members table.
}
```

- Algorithm: HS256.
- Secret: from env `GARRAIA_JWT_SECRET` (32 bytes minimum, validated at `JwtIssuer::new`). 391b reads via std::env directly; 391c wires through `garraia-config`.
- Refresh token: 32 random bytes from `rand::rngs::OsRng`, encoded as URL-safe base64 (no padding). HMAC-SHA256 with a SEPARATE env secret `GARRAIA_REFRESH_HMAC_SECRET`, hash stored in `sessions.refresh_token_hash` (already exists in migration 001 as `bytea`).

### 6.3 Audit row schema

```
audit_events:
  id            uuid    PK default gen_random_uuid()
  actor_id      uuid    NULL (NULL for user_not_found path)
  actor_label   text    NOT NULL (snapshot of email or "anonymous")
  action        text    NOT NULL (one of login.success/.failure_*/.password_hash_upgraded)
  target_type   text    NOT NULL ("user_identities")
  target_id     uuid    NULL (identity.id when known)
  group_id      uuid    NULL (auth events are not group-scoped)
  metadata      jsonb   NOT NULL ({"ip":..., "user_agent":..., "request_id":...})
  occurred_at   timestamptz NOT NULL default now()
```

`actor_label` cache survives GDPR erasure (matches existing `*_label` survival pattern).

### 6.4 Endpoint contract

```
POST /v1/auth/login
Content-Type: application/json
Headers (optional, captured into RequestCtx):
  X-Forwarded-For: <ip>
  User-Agent: <ua>
  X-Request-ID: <opaque>

Body: { "email": "user@example.com", "password": "..." }

200 OK:
  { "access_token": "<jwt>",
    "refresh_token": "<base64>",
    "expires_at": "2026-04-14T03:00:00Z",
    "user_id": "<uuid>" }

401 Unauthorized (every failure mode):
  { "error": "invalid_credentials" }

400 Bad Request (malformed body):
  { "error": "invalid_request", "detail": "<field>" }

500 Internal Server Error (storage / config failure):
  { "error": "internal_error" }
```

The 401 body is **byte-identical** across user_not_found / wrong_password / suspended. Any header that varies (e.g., `Set-Cookie`) is forbidden in 391b.

---

## 7. Test plan

Six integration test files in `crates/garraia-auth/tests/`, plus one in `crates/garraia-gateway/tests/`. Every test spins its own pgvector container or shares via lazy_static fixture (open question §13.6).

### 7.1 `verify_internal.rs` — happy paths
- Argon2id user verifies successfully; `Ok(Some(user_id))`.
- PBKDF2 user verifies successfully; hash gets upgraded to Argon2id in same tx; `audit_events.action='login.password_hash_upgraded'` count = 1.
- Concurrent lazy upgrade race: spawn 5 tokio tasks calling `verify_credential` for the same PBKDF2 user. Exactly one upgrade event in `audit_events`, all 5 tasks return `Ok(Some(user_id))`.
- Argon2id parameters: parse the hash generated by `hash_argon2id` and assert `m=65536, t=3, p=4`.

### 7.2 `verify_negative.rs` — failure paths + timing
- User not found → `Ok(None)` + `failure_user_not_found` row + `actor_id IS NULL`.
- Wrong password → `Ok(None)` + `failure_wrong_password` row + `actor_id = user_id`.
- Suspended user → `Ok(None)` + `failure_account_suspended` row.
- Deleted user (`users.status='deleted'`) → same as suspended.
- Unknown hash prefix → `Err(UnknownHashFormat)`, **no audit row except** `failure_unknown_hash` (operational forensic).
- **Timing test:** measure 30 verify calls per branch (positive Argon2id, user_not_found, wrong_password, suspended); assert that p50 of each negative branch is within 25% of p50 positive Argon2id. Tolerant CI threshold; flagged as `#[ignore]` on slow runners and run via `cargo test -- --ignored` in dedicated CI job.

### 7.3 `audit_trail.rs`
- Each terminal path inserts exactly the right row shape (action, actor_id, target_type, metadata jsonb keys, actor_label cached).
- `actor_label` is the email at the moment of audit, NOT a join (survives erasure).
- `metadata->>'ip'`, `metadata->>'user_agent'`, `metadata->>'request_id'` populated when present in `RequestCtx`, NULL when absent.

### 7.4 `jwt_lifecycle.rs`
- Issue → verify roundtrip with same secret.
- Issue → verify with WRONG secret returns `JwtIssue` error.
- Expired token (manually constructed with `exp < now`) returns expiry error.
- Algorithm confusion attack: token signed with `none` algorithm is rejected.
- Algorithm confusion attack: token signed with RS256 (asymmetric) is rejected when verifying with HS256 secret.

### 7.5 `sessions_store.rs`
- Issue refresh → row inserted with `refresh_token_hash` matching `HMAC-SHA256(plaintext)`.
- `verify_refresh(plaintext)` returns `Some((session_id, user_id))`.
- `verify_refresh(wrong_plaintext)` returns `None`.
- `verify_refresh` uses `subtle::ConstantTimeEq` (verifiable by code review only — no runtime test for constant-time).
- `revoke(session_id)` → subsequent `verify_refresh` returns `None`.

### 7.6 `garraia-gateway/tests/auth_v1_login.rs`
- Happy path: POST with valid creds → 200 + body shape.
- Wrong password → 401 + `{"error":"invalid_credentials"}`.
- Unknown user → 401 + **byte-identical body**.
- Suspended user → 401 + **byte-identical body**.
- Malformed JSON → 400 + `invalid_request`.
- Verifies that the response body bytes for the three failure modes are equal (byte-equality assertion).

### What we are NOT testing in 391b
- Cross-group authz (391d / GAR-392).
- `Principal` extractor (391c).
- Refresh endpoint (391c).
- Logout (391c).
- Signup endpoint (391c).
- Rate limiting (future).
- Replay attack on JWT (covered by short expiry + revoke; a dedicated test is 391c).

---

## 8. Rollback plan

Three levels:

1. **Before merge:** close the PR.
2. **After merge, before 391c ships:** `git revert` the commit. The `auth-v1` feature is OFF by default, so default builds and the running gateway are unaffected. The `garraia-auth` crate reverts to the 391a stub state. No DB cleanup needed.
3. **After 391c ships:** rollback requires forward-fix (391c depends on the JWT issuer + verify_credential body). A revert here breaks the extractor; instead, ship a corrective patch.

The ABSENCE of any new migration in 391b is a deliberate rollback safety property: zero schema state to reverse.

---

## 9. Risks & mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Argon2id parameters too aggressive for low-end test runner (>500ms per verify, blowing test budget) | Medium | Medium | RFC 9106 first recommendation `(64MiB, t=3, p=4)` is feasible on 2GB CI runners. If timing tests flake, lower to `(32MiB, t=2, p=2)` ONLY for `cfg(test)` and document. |
| `secrecy::SecretString` breaking change to `Credential::Internal` ripples into 391a tests | Low | Low | 391a tests in `garraia-auth/tests/skeleton.rs` already use `password: "irrelevant".into()` — adapt to `SecretString::new("irrelevant".into())`. Mechanical fix. |
| `users.status` column doesn't exist in migration 001 | High if true | **Critical** | **Verify in plan §6.1 BEFORE wave 1.** Read `migrations/001_initial_users_groups.sql`. If absent, plan changes to add migration 009 OR moves status check to 391c. Open question §13.7. |
| `hash_upgraded_at` column doesn't exist in `user_identities` | Medium | Medium | Same — verify in plan, add migration if needed. |
| Lazy upgrade race test is flaky on Windows due to tokio task scheduling | Medium | Low | Use `tokio::task::JoinSet` with explicit barriers to make all 5 tasks hit the BEGIN at roughly the same time. If still flaky, document `#[ignore]` and run in dedicated CI. |
| JWT secret leakage in panic messages | Low | High | `JwtIssuer` holds `SecretString` for the secret, never `String`. Custom `Debug` impl. |
| Refresh token enumeration via timing on `verify_refresh` | Medium | Medium | `subtle::ConstantTimeEq` is required; code review enforces the call site. |
| `argon2` crate 0.5 has API drift from 0.4 (already in `garraia-security`?) | Low | Low | Check existing usage in `garraia-security`; align versions to avoid duplicate dep tree. Open question §13.5. |
| `password-hash` PHC string parser rejects valid PBKDF2 hashes from existing `mobile_users` migration tool | Medium | High | Test against fixture hashes from `garraia-security::credentials`. If incompatible, write a manual PBKDF2 PHC parser in `hashing.rs`. |
| Endpoint integration test deadlocks if both gateway and auth crate try to start their own testcontainer | Low | Medium | Share via lazy fixture per Open Question §13.6. |
| `audit_events` insertion fails (e.g., FK violation) and rolls back the whole verify path | Low | Medium | `actor_id` is nullable, `target_id` is nullable, no FK to `user_identities` (verified in migration 002). Smoke test exercises both NULL and non-NULL actor. |
| Build script `dummy_hash.rs` regenerates on every build, slowing incremental | Low | Low | `build.rs` writes the file deterministically with a fixed seed; cargo caches based on input file mtime. Acceptable. |

---

## 10. Sequence of work (when approved)

### Wave 0 — schema reconnaissance (~30min, me, BEFORE wave 1)

0. **Read** `migrations/001_initial_users_groups.sql` and verify:
   - `users.status` column exists (or document the gap).
   - `user_identities.password_hash`, `provider`, `provider_sub`, `hash_upgraded_at` columns exist.
   - `sessions.refresh_token_hash` column exists.
   - `audit_events` schema accepts the row shape from §6.3.
   If ANY of these is missing, **stop and write a sub-plan** to add migration 009 before continuing. Update Open Questions §13.7 with the answer.

### Wave 1 — implementation (~5-7h, single agent)

1. Add new deps to `crates/garraia-auth/Cargo.toml` + workspace root.
2. Write `build.rs` generating `DUMMY_HASH`.
3. Implement `src/hashing.rs` (Argon2id + PBKDF2 + hash_argon2id + DUMMY_HASH include).
4. Implement `src/audit.rs` (`audit_login` helper).
5. Implement `src/jwt.rs` (`JwtIssuer`, `AccessClaims`, refresh helpers).
6. Implement `src/sessions.rs` (`SessionStore`).
7. Add `pub(crate) fn pool() -> &PgPool` to `LoginPool`.
8. Refactor `Credential::Internal.password` to `SecretString`. Update `tests/skeleton.rs` mechanically.
9. Implement real `verify_credential`, `find_by_provider_sub`, `create_identity` in `internal.rs`.
10. Add `AuthError::AccountSuspended/Deleted/JwtIssue/Hashing` variants.
11. Add `LoginOutcome` to `types.rs` and re-exports to `lib.rs`.
12. Write integration tests `verify_internal.rs`, `verify_negative.rs`, `audit_trail.rs`, `jwt_lifecycle.rs`, `sessions_store.rs`.
13. Add feature `auth-v1` + optional dep + `auth_routes.rs` + integration test `auth_v1_login.rs` in `garraia-gateway`.
14. Run `cargo check --workspace --features auth-v1`, `cargo clippy ... -- -D warnings`, `cargo test -p garraia-auth`, `cargo test -p garraia-gateway --features auth-v1`. Iterate.

### Wave 2 — parallel review (~30-45min wall, 2 agents background)

15. `@security-auditor` — focado em:
    - Argon2id parameters batem com RFC 9106 first recommendation
    - Constant-time anti-enumeration funciona empiricamente (timing tests)
    - `SELECT ... FOR NO KEY UPDATE OF ui` é o nível de lock correto e está dentro da mesma tx do UPDATE
    - JWT algoritmo HS256 não é vulnerável a algorithm confusion (`none`, RS256)
    - Refresh token: 32 bytes random, HMAC com chave separada, `subtle::ConstantTimeEq` no compare
    - `secrecy::SecretString` em todos os caminhos do password (Credential, JwtIssuer secret, refresh secret)
    - `audit_events` cobre todos os terminals + `actor_label` survival
    - Endpoint 401 body é byte-identical entre os 3 modos de falha
    - Tracing spans não vazam password / JWT / database URL
    - GDPR: `actor_label` cache + `actor_id ON DELETE SET NULL` honra erasure
    - Anti-pattern coverage do ADR 0005 §"Anti-patterns" (#1, #5, #6, #7, #11)

16. `@code-reviewer` — focado em:
    - SQL correctness do verify query + lazy upgrade UPDATE
    - Tx scope: BEGIN antes do SELECT, COMMIT depois do audit
    - `?` propagation correto, sem `unwrap()` em produção
    - `argon2` + `pbkdf2` + `password-hash` API usage idiomático
    - `jsonwebtoken` API usage correto (validation params, leeway)
    - `tokio` test fixtures não vazam containers
    - Feature `auth-v1` no gateway compila com e sem o flag
    - `cargo check --workspace --no-default-features` green
    - `Credential::Internal.password: SecretString` ripple bem aplicado
    - Naming + module organization

### Wave 3 — fixes + meta-files + commit + push (~45min, me)

17. Aplicar findings inline.
18. Atualizar `ROADMAP.md` §3.3 com `[x]` adicional.
19. Atualizar `CLAUDE.md` "Estrutura de crates".
20. Atualizar `crates/garraia-auth/README.md` com seção "What ships in 391b".
21. Commit + push.
22. **Não mover GAR-391 para Done.** Comment em Linear linkando o commit.

**Total estimated: 6-9 horas** (mais Wave 0 reconnaissance opcional).

---

## 11. Definition of Done

- [ ] Todos os §4 acceptance criteria checked.
- [ ] PR merged em `main`.
- [ ] Review verde de `@security-auditor` + `@code-reviewer`.
- [ ] **GAR-391 NÃO movida para Done** — comment em Linear listando 391c/d como pending.
- [ ] Final summary segue a regra: seção "Linear — issues atualizadas" obrigatória com ID, status, completedAt, e flag explícita para GAR-391 NÃO movida com motivo.
- [ ] 391c pode começar imediatamente sem refactor de trait, types, ou JWT shape.
- [ ] `garraia-auth/README.md` documenta o estado pós-391b com clareza para o próximo desenvolvedor.

---

## 12. Risks register summary (TL;DR)

**Top 3 risks:**
1. `users.status` column missing → wave 0 reconnaissance is mandatory.
2. Argon2id timing too slow for CI → fallback parameters under `cfg(test)` ready.
3. PBKDF2 PHC parser incompatible with existing `mobile_users` hashes → fixture-based test against `garraia-security::credentials` output.

---

## 13. Open questions (preciso da sua resposta antes de começar)

1. **`users.status` column existence.** Wave 0 reconnaissance vai confirmar empiricamente. Se NÃO existir, **opção A** (recomendada): adicionar migration `009_user_status.sql` em 391b mesmo (forward-only ADD COLUMN com default `'active'`). **Opção B:** mover account suspended check para 391c e shippar 391b sem ele. Recomendo **opção A** — o check está no scope do ADR 0005 §"Account status" e é trivial. Confirma?

2. **`hash_upgraded_at` column existence.** Mesmo padrão. Se não existir, adicionar como ADD COLUMN nullable na mesma migration 009 da Q1. Recomendo incluir junto. Confirma?

3. **Refresh token endpoint.** Em 391b, `SessionStore::verify_refresh` é exercitado só por integration tests do crate, não por endpoint público. Recomendo manter — endpoint vira parte de 391c quando o extractor estiver pronto. Confirma?

4. **JWT secret source.** Em 391b lê via `std::env::var("GARRAIA_JWT_SECRET")` direto, validando length ≥32. Em 391c migra para `garraia-config`. Recomendo manter env direta agora — 391c já vai mexer em config wiring de qualquer jeito. Confirma?

5. **`argon2` crate version alignment.** `garraia-security::credentials` já usa PBKDF2 via `ring`. Adicionar `argon2 = "0.5"` (RustCrypto, separado do `ring`) em `garraia-auth`. NÃO compartilhar com `garraia-security` — manter encapsulado em `garraia-auth` evita acoplamento desnecessário. Recomendo. Confirma?

6. **Testcontainer fixture sharing.** 391a usa container próprio por teste (3 containers × 12s = 36s wall). Em 391b são ~15 testes, então container-por-teste seria 180s+. Recomendo `tokio::sync::OnceCell` lazy-init compartilhado entre todos os tests do mesmo binary (cada test recria o schema via `TRUNCATE` ou roda em transação rollback-only). Reduz para ~25s wall. Confirma? (Se preferir manter o padrão de container-por-teste, dizer agora.)

7. **Schema gaps recovery.** Se Wave 0 descobrir mais de 2 colunas faltando, a recomendação é **pausar 391b** e abrir um plan dedicado `0011.5` para a migration 009 antes de continuar. Confirma o gate?

8. **Endpoint sob feature flag vs sub-router permanente.** Em 391b o `auth_routes.rs` está sob `#[cfg(feature = "auth-v1")]`. **Alternativa:** sempre compilado mas só montado quando uma config flag está true. Recomendo **feature flag** porque é mais hermético e evita custo de compilação em build default. 391c remove a feature flag e wira no AppState principal. Confirma?

9. **Audit em tx vs fire-and-forget.** Em 391b o `audit_events` insertion mora dentro da mesma tx do verify. Vantagem: atomic. Desvantagem: rollback erases audit. **Alternativa:** insertion fora da tx via channel. Recomendo **dentro da tx** em 391b — simples, atomic, e o rollback acontece só em casos de erro de storage onde a falta de audit é o menor problema. Hardening pode mover para canal separado em 391c+. Confirma?

10. **`Credential::Internal.password` breaking change para `SecretString`.** Vai exigir patch mecânico em `tests/skeleton.rs` do 391a (3 sites). Confirma que pode quebrar a assinatura?

---

## 14. Impact on other components

### 14.1 GAR-391c (Axum extractor + RequirePermission + gateway wiring)

391b entrega a base de tudo que 391c precisa: `IdentityProvider` real, JWT issuer, refresh tokens, audit. 391c implementa:

- `impl FromRequestParts<Arc<AppState>> for Principal` extraindo JWT do header `Authorization: Bearer <token>`
- Wiring de `Arc<dyn IdentityProvider>` no `AppState` real
- Remoção da feature `auth-v1` (rota vira default-on)
- `Role` enum tipado substituindo `Option<String>` em `Principal`
- `RequirePermission(Action)` extractor + `fn can(principal, action) -> bool`
- `RequestCtx` extraction real dos headers
- `POST /v1/auth/refresh`, `POST /v1/auth/logout`, `POST /v1/auth/signup` (último pode virar 391c.5)
- Migração de config do JWT secret de env var direta para `garraia-config`
- Métricas Prometheus de login

391c pode começar sem refactor da trait, sem refactor do JWT shape.

### 14.2 GAR-391d / GAR-392 (cross-group authz suite)

Sem mudança — depende de 391c. 391d ship sem tocar em 391b.

### 14.3 GAR-413 (`garraia-cli migrate workspace` — `mobile_users` → `user_identities`)

391b consolida o formato canônico de `password_hash` (`$argon2id$...` PHC). GAR-413 produz hashes PBKDF2 PHC compatíveis com `verify_pbkdf2`. Lazy upgrade vai migrar usuários ao primeiro login pós-migração — esse é o caminho documentado em ADR 0005.

### 14.4 GAR-410 (vault separado para login pool credentials)

Sem mudança — 391b lê env vars direto. 391c integra com vault via `garraia-config` quando o wiring entrar.

---

## 15. Next recommended issue (after GAR-391b merged)

**GAR-391c — Axum extractor + RequirePermission + gateway wiring + refresh/logout/signup endpoints.**

Estimado 4-6 horas. Plano dedicado `plans/0012-gar-391c-extractor-and-wiring.md`. Vai cobrir:

- `FromRequestParts<Arc<AppState>> for Principal` + JWT extraction
- `RequirePermission(Action)` extractor
- `Role` enum + `fn can()`
- `RequestCtx` extraction de headers
- AppState wiring (`Arc<dyn IdentityProvider>`, `Arc<JwtIssuer>`, `Arc<SessionStore>`)
- Remoção da feature `auth-v1` no gateway
- Endpoints `/v1/auth/refresh`, `/v1/auth/logout`, `/v1/auth/signup`
- Migração de config do JWT secret para `garraia-config`
- Métricas Prometheus baseline de login
- Rate limiting via `tower-governor` (talvez)

Depois de 391c: GAR-391d / GAR-392 (suite authz) + GAR-413 (migration tool).

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 10 open questions com os defaults recomendados, executo Wave 0 reconnaissance primeiro (pode bloquear ou redirecionar o plano), e sigo §10. Se quiser cortar escopo (ex.: deixar JWT issuance para 391c, ou splitar endpoint para 391c), me diga antes que eu toque em código.
