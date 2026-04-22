# Plan 0046 — GAR-379 slice 3 (JWT secret + metrics token via `[auth]` config)

**Status:** Em espera (bloqueado por A-1 + A-2 merge)
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers, Lote A-3)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) — slice 3 de N
**Branch:** `feat/0046-gar-379-slice3-jwt-secret`
**Pré-requisitos:** plans 0044 + 0045 merged (A-1 + A-2) para evitar merge conflict em `AppState`/`AppConfig` reshuffle.
**Unblocks:** remove o último fallback inseguro em `mobile_auth.rs::jwt_secret()` (hardcoded `"garraia-insecure-default-jwt-secret-change-me"`) e consolida leituras de env diretas de `GARRAIA_JWT_SECRET`/`GARRAIA_METRICS_TOKEN` via `garraia-config`.

---

## 1. Goal

Completar o passo de GAR-379 **"Remoção de leituras diretas de env/files em outros crates"** focando em 2 secrets de runtime que hoje são lidos fora do `garraia-config::AuthConfig`:

1. `crates/garraia-gateway/src/mobile_auth.rs::jwt_secret()` faz `std::env::var("GARRAIA_JWT_SECRET")` / `std::env::var("GarraIA_VAULT_PASSPHRASE")` direto e cai em **hardcoded fallback** (`"garraia-insecure-default-jwt-secret-change-me"`) quando ambos ausentes. Este fallback vive para manter dev-workflows rodando, mas o warn_ logging atual não é suficiente — operators em produção precisam de **fail-closed** quando o secret falta.
2. `crates/garraia-gateway/src/metrics_auth.rs` (plan 0024) lê `GARRAIA_METRICS_TOKEN` direto do env. Mesmo pattern precisa centralizar via config.

Entrega:

1. **Fonte única** de verdade para o JWT secret: `AuthConfig::jwt_secret: SecretString` (já existe em `garraia-config::auth`). Extrair acesso via uma API pública estável do gateway: `AppState::jwt_signing_secret() → SecretString`.
2. `mobile_auth.rs::jwt_secret()` (hoje private fn) é **removida**. Todo call-site refatorado para ler de `state.jwt_signing_secret()`. Fail-closed: se `AppState.auth_config` for `None` (fail-soft mode de dev), o módulo mobile_auth **também** fail-softs para 503 no caller (alinhado com `/v1/auth/*` já no plan 0010). Sem hardcoded fallback.
3. `GarraIA_VAULT_PASSPHRASE` continua aceito como env var **documentada como fallback legacy apenas**; `garraia-config::auth::from_env` ganha fallback: se `GARRAIA_JWT_SECRET` não setado, tenta `GarraIA_VAULT_PASSPHRASE`. Isso elimina o env-read-direct do `mobile_auth.rs`.
4. `metrics_auth.rs` ganha uma camada idêntica: `AppState::metrics_auth_config()` consolida a leitura.
5. Nova seção `[auth]` **no arquivo de config** (`garraia-config::model::AppConfig`) **apenas com campos operacionais não-secrets** (e.g., `jwt_algorithm`, `access_token_ttl_secs`, `refresh_token_ttl_secs`, `metrics_token_ttl_hint_secs`). **Secrets continuam exclusivamente via env** por design: CLAUDE.md regra #6 veda commit de secrets + config files; `AuthConfig` já carrega secrets via `SecretString` + env — secrets em TOML/YAML file seriam regressão.
6. `config check` (plan 0035) ganha novas validações para o bloco `[auth]` não-secret.
7. Documentação: `docs/auth-config.md` novo, resumindo precedência `env > [auth] section > defaults` por campo (com cada secret explicitamente marcado "env-only, fail-closed").

**O que NÃO entra neste slice:**
- Refactor dos outros env reads (`OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, etc.) — slices subsequentes de GAR-379.
- Remoção do `GarraIA_VAULT_PASSPHRASE` legacy — continua como fallback (zero breaking change).
- Persistência de secrets em vault externo (HashiCorp Vault, AWS Secrets Manager) — outra issue.
- Config-file support para secrets (deliberadamente vedado — ver §5.1).
- Migração de `mobile_users` / `mobile_auth` SQLite → Postgres (esse é GAR-413 stage futuro, não 379).

## 2. Non-goals

- Zero mudança no schema do Postgres.
- Zero mudança nos endpoints `/auth/*` do gateway (payload, headers, status codes).
- Zero dependência Rust nova.
- Zero breaking API change para operators já usando `GARRAIA_JWT_SECRET` + `GARRAIA_REFRESH_HMAC_SECRET` + `GARRAIA_METRICS_TOKEN` — continuam funcionando idênticos.
- Zero mudança no formato JWT emitido (HS256, claims, TTL).
- Zero mudança nos testes de `mobile_auth` existentes (cobertos, passem antes e depois).

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-gateway/src/state.rs` — novo field `auth_config: Option<Arc<AuthConfig>>` em `AppState`; getters `jwt_signing_secret()` + `metrics_auth_config()`.
- `crates/garraia-gateway/src/bootstrap.rs` — propaga `AuthConfig` para `AppState` (já vem de `AuthConfig::from_env`); adota helper compartilhado.
- `crates/garraia-gateway/src/mobile_auth.rs` — remove `jwt_secret()` private fn; todo call-site (`issue_jwt`, `issue_jwt_pub`, validate paths) consome `state.jwt_signing_secret()`. Fail-closed quando ausente (returns 503 via `RestError::AuthUnconfigured` no handler).
- `crates/garraia-gateway/src/metrics_auth.rs` — lê `MetricsAuthConfig` do `AppState` ao invés de `std::env::var`.
- `crates/garraia-config/src/auth.rs` — acrescenta fallback `GarraIA_VAULT_PASSPHRASE` em `from_env()` e `require_from_env()`.
- `crates/garraia-config/src/model.rs` — nova struct `AuthSection { jwt_algorithm, access_token_ttl_secs, refresh_token_ttl_secs, metrics_token_ttl_hint_secs }` embedada em `AppConfig.auth: AuthSection`.
- `crates/garraia-config/src/loader.rs` — parse `[auth]` TOML + `auth:` YAML block.
- `crates/garraia-config/src/check.rs` — validações para `jwt_algorithm in {"HS256"}`, TTLs em faixas razoáveis (access ≤ 24h, refresh ≥ access), coerência com env.
- `.env.example` — atualiza comentários sobre `GARRAIA_JWT_SECRET` + `GarraIA_VAULT_PASSPHRASE` como fallback legacy.
- `docs/auth-config.md` — novo arquivo (§1 abaixo referência).
- `plans/0046-gar-379-slice3-jwt-secret.md` (este arquivo).
- `plans/README.md` — entrada 0046.
- `CLAUDE.md` — atualiza mensão de `garraia-config` e regra #6 (adicionar `GarraIA_VAULT_PASSPHRASE` ao lado de `GARRAIA_JWT_SECRET`).

**Arquivos novos:**

- `docs/auth-config.md` — single-page reference para operators.

Zero nova dependência Rust.

## 4. Acceptance criteria

1. `cargo check --workspace --exclude garraia-desktop` verde.
2. `cargo fmt --check --all` verde.
3. `cargo clippy --workspace --all-targets -- -D warnings` verde.
4. `cargo test -p garraia-gateway --lib` + `cargo test -p garraia-gateway --tests` verde.
5. `cargo test -p garraia-config --lib` verde (novos unit tests em `model` + `check`).
6. Grep por `std::env::var("GARRAIA_JWT_SECRET")` no workspace retorna **apenas** `crates/garraia-config/src/auth.rs`.
7. Grep por `std::env::var("GarraIA_VAULT_PASSPHRASE")` retorna **apenas** `crates/garraia-config/src/auth.rs`.
8. Grep por `std::env::var("GARRAIA_METRICS_TOKEN")` retorna **apenas** `crates/garraia-config/src/auth.rs` (ou módulo dedicado de metrics config).
9. Grep por `garraia-insecure-default-jwt-secret-change-me` retorna **zero** matches.
10. `POST /auth/login` em dev (sem `GARRAIA_JWT_SECRET` configurado) retorna **503 Service Unavailable** com problem-details consistente — **não** um JWT assinado por fallback inseguro.
11. `POST /auth/login` com `GARRAIA_JWT_SECRET` configurado continua funcionando idêntico ao comportamento pré-0046.
12. `POST /auth/login` com `GarraIA_VAULT_PASSPHRASE` configurado (e `GARRAIA_JWT_SECRET` ausente) funciona igual — fallback legacy preservado.
13. `GET /metrics` continua funcionando com `GARRAIA_METRICS_TOKEN` configurado (plan 0024 baseline).
14. `garraia config check` emite validações novas: TTLs em faixa, jwt_algorithm conhecido.
15. `docs/auth-config.md` inclui a matriz completa de precedência (env > [auth] section > default) por campo, marcando cada secret como "env-only, fail-closed".
16. Unit test novo: `mobile_auth::issue_jwt` retorna `Err(AuthConfigMissing)` quando `AppState.auth_config` é `None`.
17. Unit test novo: `AuthConfig::from_env` prefere `GARRAIA_JWT_SECRET` sobre `GarraIA_VAULT_PASSPHRASE` quando ambos setados.
18. `@code-reviewer` APPROVE.
19. `@security-auditor` APPROVE ≥ 8.0/10.
20. CI 9/9 green.
21. Linear GAR-379 comentada (slice 3/N done).

## 5. Design rationale

### 5.1 Secrets **never** em config file

CLAUDE.md regra #6 veda logar/expor secrets. Por extensão: **nunca** committar secrets em config files. Mesmo com `SecretString`, se o arquivo é commitable, o secret vaza.

Design: `[auth]` section carrega **apenas** knobs operacionais (algoritmo, TTLs, hints). Secrets (jwt_secret, refresh_hmac_secret, metrics_token) permanecem **env-only** via `AuthConfig::from_env`. Precedência:

| Campo | env | config file | default | Fail mode |
|---|---|---|---|---|
| `jwt_secret` | ✅ `GARRAIA_JWT_SECRET` (fallback `GarraIA_VAULT_PASSPHRASE`) | ❌ vedado | ❌ | 503 handler |
| `refresh_hmac_secret` | ✅ `GARRAIA_REFRESH_HMAC_SECRET` | ❌ vedado | ❌ | 503 handler |
| `metrics_token` | ✅ `GARRAIA_METRICS_TOKEN` | ❌ vedado | ❌ | listener não sobe |
| `jwt_algorithm` | ✅ (opcional) | ✅ `[auth] jwt_algorithm = "HS256"` | `"HS256"` | validation error em `check` |
| `access_token_ttl_secs` | ✅ (opcional) | ✅ `[auth] access_token_ttl_secs = 900` | `900` (15min) | validation error |
| `refresh_token_ttl_secs` | ✅ (opcional) | ✅ `[auth] refresh_token_ttl_secs = 604800` | `604800` (7 days) | validation error |

### 5.2 Fail-closed em dev

`mobile_auth.rs` hoje tem fallback hardcoded `"garraia-insecure-default-jwt-secret-change-me"` + `warn!` (plan 0036 audit SEC-H-2 já observou que o warn não evita o JWT inseguro). Remoção:

```rust
// antes (plan 0036)
fn jwt_secret() -> String {
    if let Ok(v) = std::env::var("GARRAIA_JWT_SECRET") { return v; }
    if let Ok(v) = std::env::var("GarraIA_VAULT_PASSPHRASE") { return v; }
    warn!(...);
    "garraia-insecure-default-jwt-secret-change-me".to_string()  // REMOVER
}

// depois
// (removido — callers usam state.jwt_signing_secret() → Result<SecretString, AuthConfigMissing>;
//  handler mapeia Err → RestError::AuthUnconfigured → 503)
```

Isso matches `/v1/auth/*` já no plan 0010: sem `AuthConfig`, 503. Mobile endpoints (`/auth/*`) adotam mesmo pattern — zero exceção.

Dev workflow preservado: `.env.example` + `scripts/setup.sh` lembram de setar `GARRAIA_JWT_SECRET` (já fazem; warn existente só será fail-closed ao invés de fallback).

### 5.3 `GarraIA_VAULT_PASSPHRASE` como fallback em `from_env`

Hoje duplicado (mobile_auth + AuthConfig) — fallback **só** no mobile_auth. Centralizar:

```rust
// AuthConfig::from_env (novo)
let jwt = std::env::var("GARRAIA_JWT_SECRET")
    .or_else(|_| std::env::var("GarraIA_VAULT_PASSPHRASE"))
    .ok()?;
```

Zero breaking change; mesmo flag legacy continua válido. Doc `.env.example` atualizado.

### 5.4 `AppState.auth_config` tipo

Opção A: `Option<Arc<AuthConfig>>` (se None, gateway em fail-soft mode).
Opção B: `AuthConfig` direto (require at construction).

Escolha: **A**. Mantém fail-soft já estabelecido por `bootstrap.rs::AuthConfig::from_env → Ok(None) → WARN`. Gateway sobe mas auth endpoints 503. Refactor de `mobile_auth` alinha com isso.

### 5.5 Config check validations

Novas regras em `check::run_check`:

- `auth.jwt_algorithm in {"HS256"}` (outras futuras). Outside → **Severity::Error**.
- `auth.access_token_ttl_secs in [60..=86400]` (1 min .. 24 h). Outside → **Error**.
- `auth.refresh_token_ttl_secs in [60..=2_592_000]` (1 min .. 30 days) AND `>= access_token_ttl_secs`. Outside → **Error**.
- `auth.metrics_token_ttl_hint_secs` (opcional, 0 = indefinido). Anything positive aceito; doc-only.
- Cross-check: se `GARRAIA_JWT_SECRET`/`GarraIA_VAULT_PASSPHRASE` em env + `[auth]` no file → **Info** "env override aplicado".
- Cross-check: se **nenhum** env secret + running in `--strict` → **Error** "jwt_secret ausente; `/auth/*` + `/v1/auth/*` responderão 503".

### 5.6 PII-safe `Debug`

`AuthSection` (não-secret, derive `Debug` ok). `AuthConfig` já tem Debug manual redatado (plan 0011). Nenhuma mudança.

### 5.7 `metrics_auth` integration

Plan 0024 (`metrics_auth.rs`) lê `GARRAIA_METRICS_TOKEN` + `GARRAIA_METRICS_ALLOW` via `std::env::var` dentro do próprio módulo. Refactor:

- `garraia-config::auth` ganha também `metrics_token: Option<SecretString>` + `metrics_allow_cidrs: Vec<IpNet>`.
- `AppState.metrics_auth_config()` devolve um handle.
- `metrics_auth::MetricsAuthConfig::from_env` removido em favor de `from_auth_config(auth: &AuthConfig)`.

### 5.8 Backward compat

Todos os env vars antigos (`GARRAIA_JWT_SECRET`, `GARRAIA_METRICS_TOKEN`, `GARRAIA_METRICS_ALLOW`, `GarraIA_VAULT_PASSPHRASE`) funcionam idênticos. Clientes + deploys não precisam mudar nada.

## 6. Security review triggers

- **SEC-H fail-closed removal of hardcoded default**: integration test `login_without_jwt_secret_returns_503` guardando a regressão.
- **SEC-H one-place-to-read**: grep CI step ou clippy lint custom? → grep no PR description (não viável como CI rule sem `cargo deny` customization). Manual pre-merge check.
- **SEC-M `GarraIA_VAULT_PASSPHRASE` still works**: integration test cobre.
- **SEC-M `config check` error surfacing**: unit test `check.rs` cobrindo as 4 regras novas.
- **SEC-L `SecretString` exposure**: novas APIs retornam `SecretString` não raw `String`. `Debug` remains redacted.
- **SEC-L env precedence doc**: `docs/auth-config.md` é autoridade — tem que estar claro que arquivo não sobrepõe env.
- **SEC-L metrics_token migration**: mesmo pattern do jwt — fail-closed sem env.

## 7. Testing strategy

### 7.1 Unit

- `AuthConfig::from_env` com ambos `GARRAIA_JWT_SECRET` + `GarraIA_VAULT_PASSPHRASE` setados → prefere `GARRAIA_JWT_SECRET`.
- `AuthConfig::from_env` com só `GarraIA_VAULT_PASSPHRASE` → usa.
- `AuthConfig::from_env` sem nenhum → `Ok(None)` (fail-soft).
- `AuthSection` defaults: HS256, 900 s, 604800 s.
- `check::run_check` pega `jwt_algorithm = "RS256"` como Error.
- `check::run_check` pega `access_ttl > refresh_ttl` como Error.
- `state.jwt_signing_secret()` retorna `Err(AuthConfigMissing)` quando `AppState.auth_config = None`.

### 7.2 Integration

- `login_without_jwt_secret_returns_503` — env limpo, sem `[auth]` override. POST `/auth/login` → 503.
- `login_with_legacy_passphrase_works` — `GarraIA_VAULT_PASSPHRASE` setado (sem `GARRAIA_JWT_SECRET`) → login funciona, JWT emitido assinado com passphrase.
- `login_with_standard_env_works` — baseline.
- `metrics_endpoint_without_token_503` (já plan 0024, aqui regression guard).

### 7.3 Contract

- `plans/README.md` + `docs/auth-config.md` + `.env.example` sincronizados — smoke grep matches.

## 8. Rollback plan

Pura reversão via `git revert <commit>`. Operators em produção não perdem secrets (env vars intactos). Se algum dev estivesse dependendo do hardcoded fallback inseguro → após revert, mesma coisa. Se estivesse rodando com `GARRAIA_JWT_SECRET` setado → idêntico pós-revert.

Não há migração DB. Não há mudança de wire format.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Dev sem `GARRAIA_JWT_SECRET` quebra `/auth/*` após upgrade | BAIXO | `scripts/setup.sh` já valida. `.env.example` loud + 503 message é educativa. |
| CI test inconsistente (env var vazamento entre testes) | BAIXO | Testes usam `std::env::set_var` + `remove_var` com `once_cell` guards (plan 0036 pattern). |
| Refactor mobile_auth quebra TOTP/OAuth paths (eles usam `issue_jwt_pub`) | MÉDIO | Grep exaustivo em PR; oauth.rs + totp.rs refatorados no mesmo commit. |
| `GarraIA_VAULT_PASSPHRASE` users não sabem que foi centralizado e confundem com break | BAIXO | `docs/auth-config.md` + `.env.example` + release note no PR descrição. |
| `[auth]` section em TOML conflita com env → ambiguidade | BAIXO | §5.1 + `check` cross-check emite Info quando detecta override. |
| `AuthSection::jwt_algorithm` aceita só "HS256" mas operator escreve "HS384" | BAIXO | `check` Error + doc. |
| Metrics refactor regressa plan 0024 (dedicated listener) | MÉDIO | Integration test plan 0024 re-executado; mesmo env surface. |
| Refresh HMAC secret fail-closed não documentado — operator sobe só jwt_secret | BAIXO | `AuthConfig::from_env` já exige refresh_hmac_secret (atual); zero change. |

## 10. Open questions

- **Q1**: Adicionar lint custom em `cargo-deny`/`clippy` para detectar `std::env::var("GARRAIA_*_SECRET")` fora de `garraia-config`? → **Não neste slice**; sanity check é manual via grep CI step (futuro). Produto pode adicionar se o workspace crescer.
- **Q2**: Merge do `AuthSection` com o bloco `gateway.session_*` existente? → **Não**; `gateway.session_ttl_secs` é session LLM (plano 0202), não auth JWT. Confusão semântica evitada mantendo blocos separados.
- **Q3**: Deprecar `GarraIA_VAULT_PASSPHRASE` nesta slice? → **Não**; zero breaking change. Deprecation + timeline em slice futuro.

## 11. Future work

- **Slice 4**: refactor `OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` → `providers.{name}.api_key: SecretString` em `AuthConfig` (ou `ProvidersSecrets`).
- **Slice 5**: deprecar `GarraIA_VAULT_PASSPHRASE` com 1 release de warning + 1 release de error.
- **Vault integration**: HashiCorp Vault / AWS Secrets Manager adapter via trait `SecretSource`.
- **Lint custom**: `#[deny(env_var_outside_config)]` attribute + clippy wrapper.

## 12. Work breakdown

| Task | Arquivo | Estimativa |
|---|---|---|
| T1 | `AuthConfig::from_env` fallback `GarraIA_VAULT_PASSPHRASE` + unit tests | 25 min |
| T2 | `AuthSection` em `model.rs` + loader TOML/YAML + `check.rs` rules | 45 min |
| T3 | `AppState.auth_config` field + getters (jwt_signing_secret, metrics_auth_config) | 30 min |
| T4 | Refactor `mobile_auth.rs` (remove jwt_secret fn; `issue_jwt(&state)` signature) | 60 min |
| T5 | Refactor `metrics_auth.rs` para consumir `AuthConfig` | 40 min |
| T6 | Refactor `oauth.rs` + `totp.rs` call-sites | 30 min |
| T7 | `docs/auth-config.md` redação | 45 min |
| T8 | `.env.example` + `CLAUDE.md` regra #6 + plans/README.md entrada | 20 min |
| T9 | Unit tests (§7.1) | 40 min |
| T10 | Integration tests (§7.2) | 60 min |
| T11 | Review pass + findings | 60 min |

Total estimado: ~7h. Executado sequencialmente após A-1 + A-2 merged para evitar conflito em `AppState`.

## 13. Definition of done

- [ ] Todos os `Acceptance criteria` §4 verdes.
- [ ] Grep checks `#6, #7, #8, #9` limpos.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.0/10.
- [ ] CI 9/9 green.
- [ ] PR aberto com link para este plan.
- [ ] PR merged em `main`.
- [ ] Linear GAR-379 atualizada (comentário slice 3/N done).
- [ ] `plans/README.md` entrada 0046 marcada `✅`.
- [ ] `CLAUDE.md` atualizado (regra #6 + menção garraia-config).
- [ ] `docs/auth-config.md` publicado.
- [ ] `.env.example` atualizado.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
