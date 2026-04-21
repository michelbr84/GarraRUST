# Plan 0024 — GAR-412: /metrics endpoint auth (Bearer + IP ACL + startup fail-closed)

> **Narrow slice:** security hardening do subsistema de métricas Prometheus. Cobre as duas superfícies `/metrics` existentes hoje (listener dedicado do `garraia-telemetry` + rota pública do `router.rs`), ambas sem auth. Escopo tempo-delimitado, sem decisão de produto, sem schema change.

**Linear issue:** [GAR-412](https://linear.app/chatgpt25/issue/GAR-412) — "/metrics endpoint auth: bearer ou IP ACL quando bind != 127.0.0.1" (Backlog → In Progress, High, labels `security` + `epic:otel`, project Fase 2 — Performance, RAG & MCP).

**Status:** Draft — pendente aprovação do docs-draft PR.

**Relationship note:** follow-up H3 do security review de GAR-384 (OpenTelemetry baseline, plan 0001, mergeado 2026-04-13). Sem este slice, `/metrics` continua exposto sem auth em qualquer deploy que bind em não-loopback — leak de observabilidade interna (request counts, latências, active_sessions, error kinds).

**Goal:** adicionar autenticação configurável (Bearer token + IP allowlist) ao endpoint `/metrics`, com fail-closed startup quando bind não-loopback + auth não configurada. Dev ergonomics preservado — loopback + no-auth continua servindo 200 OK sem fricção.

## Architecture

### Before (current state)

Duas superfícies `/metrics` sem auth nenhuma:

```rust
// 1. Listener dedicado (garraia-telemetry/src/metrics.rs)
let handle = PrometheusBuilder::new()
    .with_http_listener(addr)   // sem middleware, sem auth
    .install_recorder()?;
```

```rust
// 2. Rota pública do gateway (garraia-gateway/src/router.rs:301)
.route("/metrics", get(crate::observability::prometheus_metrics_handler))
// sem layer de auth, exposta no listener principal do gateway
```

Se operador setar `GARRAIA_METRICS_BIND=0.0.0.0:9464` ou expuser a porta principal do gateway, `/metrics` vira público.

### After

Middleware unificado `metrics_auth_layer` aplicado nas duas superfícies:

```rust
pub async fn metrics_auth_layer(
    State(cfg): State<MetricsAuthConfig>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let peer_ip = peer.map(|ConnectInfo(sa)| sa.ip());
    let is_loopback = peer_ip.is_some_and(|ip| ip.is_loopback());

    // (a) dev ergonomics: loopback + no auth configured = OK
    if is_loopback && cfg.token.is_none() && cfg.allowlist.is_empty() {
        return next.run(req).await;
    }

    // (b) allowlist (se configurado): peer precisa estar dentro
    if !cfg.allowlist.is_empty() {
        let ok = peer_ip.is_some_and(|ip| cfg.allowlist.iter().any(|c| c.contains(&ip)));
        if !ok {
            return (StatusCode::FORBIDDEN, "metrics: peer not allowed").into_response();
        }
    }

    // (c) bearer (se configurado): Authorization: Bearer <token> obrigatório
    if let Some(expected) = cfg.token.as_deref() {
        let got = extract_bearer(&headers);
        if !got.is_some_and(|t| constant_time_eq(t.as_bytes(), expected.as_bytes())) {
            return (StatusCode::UNAUTHORIZED, "metrics: invalid token").into_response();
        }
    } else if cfg.allowlist.is_empty() && !is_loopback {
        // safety net — startup check deveria ter barrado esta combinação
        return (StatusCode::SERVICE_UNAVAILABLE, "metrics: auth not configured").into_response();
    }

    next.run(req).await
}
```

### Startup fail-closed

Em `garraia-telemetry::init_metrics` e no boot do `garraia-gateway`, se bind não-loopback **e** nem `GARRAIA_METRICS_TOKEN` nem `GARRAIA_METRICS_ALLOW` estão configurados:

- `tracing::error!` com instrução de fix explícita
- Retorna `Err(Error::Init)` para o caller
- Metrics subsystem **não sobe**, gateway principal **continua** saudável (fail-soft de telemetria preservado — invariant de GAR-384)

### Listener override

O `metrics-exporter-prometheus::PrometheusBuilder::with_http_listener(addr)` constrói um listener interno sem ponto de injeção para middleware. Substitute: chamar `install_recorder()` para obter o `PrometheusHandle` e servir `/metrics` via axum próprio:

```rust
let handle = PrometheusBuilder::new().install_recorder()?;
let app = Router::new()
    .route("/metrics", get(move || async move {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
            .body(Body::from(handle.render()))
            .unwrap()
    }))
    .layer(from_fn_with_state(cfg.clone(), metrics_auth_layer))
    .into_make_service_with_connect_info::<SocketAddr>();
tokio::spawn(async move { axum::serve(listener, app).await });
```

## Design invariants

1. **Dev ergonomics preservado** — bind default `127.0.0.1:9464` sem token/ACL = 200 OK sem fricção.
2. **Fail-soft de telemetria** — erro na inicialização do metrics server NÃO derruba o gateway principal.
3. **Fail-closed de config** — bind não-loopback sem auth configurada ⇒ metrics subsystem recusa subir com erro claro (não é silencioso).
4. **Timing-safe token comparison** — `subtle::ConstantTimeEq`, não `==` em `&[u8]`.
5. **Reuso total de CIDR logic** — `rate_limiter::parse_trusted_proxies` (pub desde plan 0022); zero nova lógica CIDR.
6. **Zero mudança observável no gateway principal** — `AppState`, `SharedState`, rotas `/v1/*`, `/admin/*`, `/auth/*` não tocadas.
7. **Secret redaction** — `GARRAIA_METRICS_TOKEN` entra no set sensível (mesmo tratamento de `GARRAIA_JWT_SECRET`); startup log mostra o modo (`"metrics: auth=token"`) mas nunca o valor.

## Tech Stack

- Axum 0.8 `State`, `ConnectInfo<SocketAddr>`, `from_fn_with_state`.
- `metrics-exporter-prometheus::{PrometheusBuilder, PrometheusHandle}` (já no workspace desde plan 0001).
- `ipnet::IpCidr` via `rate_limiter::parse_trusted_proxies` (já pub).
- `subtle::ConstantTimeEq` (transitivo via `garraia-auth` — adicionar direto em `garraia-gateway/Cargo.toml` se necessário).
- Nenhuma nova dependência direta esperada.

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-telemetry/src/config.rs` | Modify | `+ metrics_token: Option<String>` + `+ metrics_allowlist: Vec<String>` (strings raw; parse em runtime) + env loaders + tests |
| `crates/garraia-telemetry/src/metrics.rs` | Modify | Refactor `init_metrics` para axum custom server + middleware de auth |
| `crates/garraia-telemetry/src/lib.rs` | Modify (talvez) | Pub re-export de `MetricsAuthConfig` se compartilhado |
| `crates/garraia-telemetry/Cargo.toml` | Modify | Add `axum` + `tokio` deps se ainda não presentes |
| `crates/garraia-gateway/src/metrics_auth.rs` | Create | `MetricsAuthConfig` + `metrics_auth_layer` middleware + 6 unit tests |
| `crates/garraia-gateway/src/lib.rs` | Modify | `mod metrics_auth;` |
| `crates/garraia-gateway/src/router.rs` | Modify | `.layer()` na rota `/metrics` com middleware |
| `crates/garraia-gateway/src/server.rs` | Modify | Startup fail-closed check para main-listener `/metrics` (bind != loopback + no auth ⇒ erro) |
| `crates/garraia-gateway/src/state.rs` | Modify | `SharedState.metrics_auth_cfg: Arc<MetricsAuthConfig>` |
| `crates/garraia-gateway/Cargo.toml` | Modify | Add `subtle` se necessário + `[[test]]` block para novo integration test |
| `crates/garraia-gateway/tests/metrics_auth_integration.rs` | Create | 6 cenários de integration test (ver §Task 5) |
| `docs/telemetry.md` | Modify | Nova §6.1 "Security" + update §8 troubleshooting |
| `CLAUDE.md` | Modify | Documentar `GARRAIA_METRICS_TOKEN` + `GARRAIA_METRICS_ALLOW` como sensíveis |
| `.env.example` | Modify | Comentar 2 novas env vars abaixo de `GARRAIA_METRICS_BIND` |
| `plans/0024-gar-412-metrics-endpoint-auth.md` | Create | Este plan file |
| `plans/README.md` | Modify | Index entry 0024 |

**No migrations. No schema changes. No new crates.**

## Task 1 — TelemetryConfig: env vars

**Files:** `crates/garraia-telemetry/src/config.rs`

- [ ] Adicionar campos `metrics_token: Option<String>` + `metrics_allowlist: Vec<String>` a `TelemetryConfig`.
- [ ] `from_env()` lê `GARRAIA_METRICS_TOKEN` (string vazia ⇒ None) e `GARRAIA_METRICS_ALLOW` (comma-separated; string vazia ⇒ Vec vazio).
- [ ] `#[serde(default)]` para ambos (backward-compat com config files existentes).
- [ ] Unit test: happy path (token + 2 CIDRs); empty vars (None + Vec vazio); malformed CIDR guard via `validator` crate ou parse explícito.
- [ ] `cargo test -p garraia-telemetry` PASS.

## Task 2 — `metrics_auth_layer` middleware

**Files:** `crates/garraia-gateway/src/metrics_auth.rs` (new), `crates/garraia-gateway/src/lib.rs`

- [ ] `MetricsAuthConfig { token: Option<String>, allowlist: Vec<IpCidr> }`.
- [ ] `pub fn from_telemetry_config(cfg: &TelemetryConfig) -> Self` — parse dos CIDRs via `rate_limiter::parse_trusted_proxies`.
- [ ] Middleware `async fn metrics_auth_layer` seguindo §Architecture.
- [ ] Helper `extract_bearer(&HeaderMap) -> Option<&str>` reusando padrão de `auth_routes/extractor.rs`.
- [ ] Token comparison via `subtle::ConstantTimeEq::ct_eq`.
- [ ] 6 unit tests cobrindo a matriz:
  1. `loopback + no auth` ⇒ 200
  2. `non-loopback + no auth` ⇒ 503 (safety net — seria bloqueado antes pelo startup check)
  3. `allowlist match` ⇒ 200
  4. `allowlist miss` ⇒ 403
  5. `token match` ⇒ 200
  6. `token mismatch` ⇒ 401
- [ ] Startup log helper: `describe_mode(&cfg) -> &'static str` retornando `"loopback-only"`, `"token"`, `"allowlist"`, ou `"token+allowlist"`.
- [ ] `cargo test -p garraia-gateway --lib metrics_auth` PASS.

## Task 3 — `garraia-telemetry::init_metrics` custom server

**Files:** `crates/garraia-telemetry/src/metrics.rs`, `Cargo.toml`

- [ ] Add `axum`, `tokio`, `hyper` deps (o que faltar) a `garraia-telemetry/Cargo.toml`.
- [ ] Substituir `PrometheusBuilder::with_http_listener(addr).install_recorder()` por `install_recorder()` + servidor axum próprio.
- [ ] Axum router: `GET /metrics` retornando `handle.render()` com content-type `text/plain; version=0.0.4; charset=utf-8` (espelhar `observability.rs:309`).
- [ ] Aplicar `metrics_auth_layer` via `from_fn_with_state`.
- [ ] `spawn` do listener via `tokio::spawn`; handle do task **não** retornado (mesmo padrão do builder interno — fire-and-forget).
- [ ] Startup check: se bind não-loopback AND token None AND allowlist vazio ⇒ `tracing::error!` + retornar `Err(Error::Init(...))`.
- [ ] Startup log OK: `info!("metrics: listener bind={addr} auth={mode}")`.
- [ ] Fail-soft preservado: `garraia-telemetry::init()` trata o `Err` já (via `?` em `lib.rs:62`), mas o caller em `garraia-gateway::server` deve tratar como "metrics off" e continuar.
- [ ] `cargo test -p garraia-telemetry` PASS.

## Task 4 — Router `/metrics` + startup check no gateway

**Files:** `crates/garraia-gateway/src/router.rs`, `crates/garraia-gateway/src/server.rs`, `crates/garraia-gateway/src/state.rs`

- [ ] `SharedState` ganha `metrics_auth_cfg: Arc<MetricsAuthConfig>` (construído a partir da `TelemetryConfig` no boot).
- [ ] `router.rs:301` — adicionar `.layer(from_fn_with_state(state.metrics_auth_cfg.clone(), metrics_auth_layer))` à rota `/metrics`.
- [ ] `server.rs` — ao bind do listener principal do gateway, se bind != loopback AND não houver auth configurada no `/metrics` embedded route, decidir: **log warning** (é outro listener, gateway principal; não fail-closed aqui — o main listener tem outros motivos para bind em 0.0.0.0 e barrar o boot seria regressão para operadores).
    - **Racional:** o main listener serve muitas rotas (auth, v1, admin, ws); fail-closed deve ser restrito ao subsistema metrics dedicado (telemetry crate). Para a rota embedded `/metrics` do main router, warning claro + middleware faz fail-closed em runtime (503 se request chega sem auth).
- [ ] Gateway continua a subir; o middleware `metrics_auth_layer` é quem bloqueia requests indevidos.
- [ ] `cargo check -p garraia-gateway` + `cargo test -p garraia-gateway --lib` PASS.

## Task 5 — Integration tests

**Files:** `crates/garraia-gateway/tests/metrics_auth_integration.rs` (new), `crates/garraia-gateway/Cargo.toml`

- [ ] `[[test]] name = "metrics_auth_integration" required-features = ["test-helpers"]` em `Cargo.toml` (padrão plan 0022).
- [ ] 6 cenários:
  1. **loopback_no_auth_ok** — spawn server em 127.0.0.1:0 sem token/ACL; GET `/metrics` ⇒ 200 + content-type `text/plain`.
  2. **non_loopback_no_auth_startup_err** — spawn server em 0.0.0.0:0 sem token/ACL; `init_metrics` retorna `Err(Error::Init)`.
  3. **token_match_ok** — spawn com `GARRAIA_METRICS_TOKEN=dev`; GET `/metrics` com `Authorization: Bearer dev` ⇒ 200.
  4. **token_mismatch_401** — spawn com token `dev`; GET `/metrics` com `Authorization: Bearer wrong` ⇒ 401.
  5. **allowlist_match_ok** — spawn com `GARRAIA_METRICS_ALLOW=127.0.0.0/8`; GET `/metrics` do 127.0.0.1 ⇒ 200.
  6. **allowlist_miss_403** — spawn com `GARRAIA_METRICS_ALLOW=10.0.0.0/8`; GET `/metrics` do 127.0.0.1 ⇒ 403.
- [ ] Usar porta `0` (OS assignada) + ler `local_addr()` do listener; evita race.
- [ ] `cargo test -p garraia-gateway --features test-helpers --test metrics_auth_integration` PASS.

## Task 6 — Docs + env examples

**Files:** `docs/telemetry.md`, `CLAUDE.md`, `.env.example`

- [ ] `docs/telemetry.md`:
  - Nova seção §6.1 "Security" imediatamente após §6: 3 env vars (`GARRAIA_METRICS_ENABLED`, `GARRAIA_METRICS_TOKEN`, `GARRAIA_METRICS_ALLOW`), matriz de estados (loopback/non-loopback × token/allowlist/none ⇒ ação do middleware), recomendações de deploy (Prometheus scraper atrás de VPN usa allowlist; scraper externo usa token; loopback usa nada).
  - §8 Troubleshooting: nova entrada "`/metrics returns 401`" / "`/metrics returns 403`" / "`metrics subsystem refused to start`".
- [ ] `CLAUDE.md`:
  - Na Regra absoluta #6 (nunca logar secrets), adicionar `GARRAIA_METRICS_TOKEN` ao set sensível.
  - Nova mini-tabela de env vars (ou linha na tabela existente) com as 2 novas vars.
- [ ] `.env.example` abaixo da linha `# GARRAIA_METRICS_BIND=127.0.0.1:9464` (linha 133), adicionar:
  ```
  # GARRAIA_METRICS_TOKEN=<random-hex>
  # GARRAIA_METRICS_ALLOW=127.0.0.1/32,10.0.0.0/8
  ```
- [ ] `cargo test -p garraia-telemetry` + `cargo test -p garraia-gateway --lib` ainda PASS.

## Task 7 — Reviews + PR + CI + merge

- [ ] Rodar `@code-reviewer` + `@security-auditor` em paralelo.
- [ ] Endereçar blockers / HIGH em commit separado (padrão 0019-0023).
- [ ] Abrir PR `feat(gateway,telemetry): metrics endpoint auth (bearer + ACL) — plan 0024 (GAR-412)`.
- [ ] Body do PR cobre: scope, acceptance criteria, breaking-change note (deploys com `GARRAIA_METRICS_BIND=0.0.0.0` passam a precisar de token/ACL), rollback, 3 follow-ups deferred.
- [ ] Monitorar CI 9/9 green.
- [ ] Squash merge + delete branch remoto + delete worktree.
- [ ] Sync main.
- [ ] Linear GAR-412 → Done + comentário final.
- [ ] `plans/0024-*.md` + `plans/README.md` marcados como Merged.

## Acceptance criteria

1. `/metrics` (listener dedicado + rota embedded) retorna **401** sem Bearer correto quando `GARRAIA_METRICS_TOKEN` configurado.
2. `/metrics` retorna **403** quando peer fora do `GARRAIA_METRICS_ALLOW` configurado.
3. `/metrics` retorna **200** em deploy default (`127.0.0.1:9464`, sem token/ACL) — dev ergonomics preservado.
4. `garraia-telemetry::init_metrics` retorna **`Err(Error::Init)`** quando bind não-loopback + auth não configurada; gateway principal continua saudável.
5. `cargo fmt --check --all` clean.
6. `cargo clippy --workspace --no-deps --tests -- -D warnings` clean.
7. `cargo test -p garraia-telemetry` + `cargo test -p garraia-gateway --lib` + `cargo test -p garraia-gateway --features test-helpers --test metrics_auth_integration` PASS.
8. Integration test matrix cobre 6 cenários (token ±, allowlist ±, loopback ±, não-loopback sem auth ⇒ startup err).
9. `docs/telemetry.md` §6.1 existe + documenta 3 env vars + matriz de estados + deploy recommendations.
10. `CLAUDE.md` lista `GARRAIA_METRICS_TOKEN` como sensível.
11. CI 9/9 green no merge commit.
12. Code review APPROVE + security audit ≥ 8.5/10 sem blockers HIGH.

## Rollback

Additive + forward-compatible. Revert do squash commit restaura comportamento pré-0024 (`/metrics` sem auth). Zero schema change.

**Rollback parcial em produção** (se operador precisa do comportamento antigo explicitamente): setar `GARRAIA_METRICS_ALLOW=0.0.0.0/0` — opt-in explícito de "qualquer peer permitido", documenta a intenção insegura no próprio env.

## Open questions

- **OQ-1:** `metrics_token` como `String` raw em `TelemetryConfig` ou como `SecretString`? **Decisão:** `Option<String>` raw (`TelemetryConfig` já é Debug via derive + printado em startup logs — precisa de custom Debug impl para redact OR mover o campo para struct separada). V1: custom `Debug` impl em `TelemetryConfig` que redact `metrics_token`. Se ficar feio, refatorar em follow-up.
- **OQ-2:** Integration test do main-listener `/metrics` também precisa de 6 cenários ou só os 6 do listener dedicado bastam? **Decisão:** 6 cenários cobrem o middleware; o main-listener reusa o mesmo middleware com mesma config, então testar só o dedicado basta. Um smoke test adicional do main-listener é bonus. Se tempo permitir no T5, adicionar 2 cenários extras.
- **OQ-3:** Startup check deve ser "WARN + degrade silent" ou "ERROR + fail init" quando bind não-loopback + no auth? **Decisão:** ERROR + fail init para o metrics subsystem **apenas** (gateway principal segue). Operador tem que saber que metrics não está rodando; silent degrade esconde bugs de config.

## Follow-ups (explicit)

- **Plan 0025+** — admin XFF consolidation (`admin/middleware.rs::extract_ip` + ~25 call sites). Requires `ConnectInfo` propagation + product decision sobre admin audit-trail IP format.
- **Plan 0026+** — `/auth/*` migration de `rate_limit_layer` deprecated para `rate_limit_layer_authenticated`. Bloqueado por design (register/login mintam o token).
- **GAR-411 follow-up** (separate issue): Telemetry hardening — TLS docs, cardinality guards, idempotent init.
- **Backlog hygiene** — GAR-330..370 (Mobile Alpha) marcadas como Done no Linear, linkando aos commits históricos.
- **Opportunistic** — cache `GARRAIA_TRUSTED_PROXIES` parse em `SharedState` se profiling justificar (follow-up de 0022/0023).

## Relationship to other plans

- **Plan 0001 (GAR-384)** — estabeleceu OTel baseline + Prometheus `/metrics`. H3 do security review virou este GAR-412.
- **Plan 0022 (GAR-426)** — entregou `parse_trusted_proxies` + `real_client_ip` + `ipnet` no workspace. Reuso direto aqui.
- **Plan 0023 (GAR-427)** — fechou 1 de 3 `TODO(plan-0023+)` (api.rs session-IP). Este plan fecha uma superfície ortogonal (metrics exposure).
