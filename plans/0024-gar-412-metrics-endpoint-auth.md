# Plan 0024 — GAR-412: /metrics endpoint auth (Bearer + IP ACL + startup fail-closed)

> **Narrow slice:** security hardening do subsistema de métricas Prometheus. Cobre as duas superfícies `/metrics` existentes hoje (listener dedicado do `garraia-telemetry` + rota pública do `router.rs`), ambas sem auth. Escopo tempo-delimitado, sem decisão de produto, sem schema change.

**Linear issue:** [GAR-412](https://linear.app/chatgpt25/issue/GAR-412) — "/metrics endpoint auth: bearer ou IP ACL quando bind != 127.0.0.1" (Backlog → In Progress, High, labels `security` + `epic:otel`, project Fase 2 — Performance, RAG & MCP).

**Status:** Draft v2 — revisão 1 aplicada 2026-04-20 endereçando dois itens do review do PR #37:
1. Ownership de `metrics_auth_layer` decidido explicitamente em §Architecture → "Crate ownership" (middleware vive em `garraia-gateway`; `garraia-telemetry` fica HTTP-free; sem ciclo de crate).
2. Semântica fail-closed unificada em §Architecture → "Fail-closed semantics — 2 listeners, 2 camadas" (dedicated = startup; embedded = runtime; divergência intencional, documentada).

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

### Crate ownership (resolves review item #1)

Para evitar acoplamento ruim ou dependência circular entre `garraia-telemetry` ↔ `garraia-gateway`, fica **decidido e explícito**:

| Responsabilidade | Crate dono | Justificativa |
|---|---|---|
| **Coleta** de métricas (recorder) + helpers de counter/gauge/histogram | `garraia-telemetry` | Já é hoje; escopo puramente sem HTTP |
| Expor `install_recorder() -> PrometheusHandle` | `garraia-telemetry` | Permite qualquer caller servir ou renderizar sem HTTP ambient |
| **Serving HTTP** do endpoint (listener dedicado ou rota embedded) | `garraia-gateway` | Gateway já é o único crate com axum/tokio e `SharedState` |
| `MetricsAuthConfig` + `metrics_auth_layer` middleware | `garraia-gateway` | Depende do runtime HTTP; vive onde o HTTP vive |
| Spawn do listener dedicado em `GARRAIA_METRICS_BIND` | `garraia-gateway::server` | Boot unificado junto com o listener principal |
| Startup fail-closed **do listener dedicado** | `garraia-gateway::server` | Mesmo crate que faz o spawn; decisão atômica |

**Consequência direta:** `garraia-telemetry` fica **sem dependência de axum/hyper/tower**. O crate `garraia-gateway` já depende de `garraia-telemetry` (relação vigente); a nova direção se mantém, sem ciclo.

**Anti-padrão rejeitado:** fazer `garraia-telemetry` importar `garraia-gateway::metrics_auth` — criaria ciclo e poluiria a telemetria com runtime HTTP. **Anti-padrão rejeitado #2:** criar crate novo `garraia-metrics-auth` só para o middleware — scope creep desnecessário para um slice narrow de ~200-300 LoC.

### Fail-closed semantics — 2 listeners, 2 camadas (resolves review item #2)

Existem **duas superfícies `/metrics` distintas**, cada uma com seu **modo próprio de fail-closed**, deliberadamente diferentes porque servem papéis diferentes:

| Superfície | Listener | Semântica fail-closed | Ponto de bloqueio |
|---|---|---|---|
| **Listener dedicado** (`GARRAIA_METRICS_BIND`, default `127.0.0.1:9464`) | Spawn condicional em `garraia-gateway::server` boot | **Startup-time:** se `GARRAIA_METRICS_ENABLED=true` AND bind não-loopback AND (`GARRAIA_METRICS_TOKEN` unset AND `GARRAIA_METRICS_ALLOW` vazio) ⇒ **não spawna** o listener + `tracing::error!("metrics auth not configured, metrics listener disabled")`. Gateway principal continua saudável (fail-soft de telemetria, invariant de GAR-384). | Antes do listener subir |
| **Rota embedded `/metrics`** do main gateway listener (`router.rs:301`) | Sempre ativa quando gateway sobe | **Runtime-only:** o middleware `metrics_auth_layer` aplicado à rota devolve **503 `metrics: auth not configured`** quando peer não-loopback AND token unset AND allowlist vazio; 401 em token mismatch; 403 em allowlist miss. Main listener **nunca falha boot** por causa de config de metrics — serve auth/v1/admin/ws e muitos motivos legítimos para bind em 0.0.0.0. | No request handler, via middleware |

**Por que assim:** o listener dedicado existe *só* para scrape Prometheus — se ele não pode ser auth'd, não faz sentido subir. O main listener serve o produto inteiro — derrubar o boot por config de observabilidade seria regressão operacional. Ambos cumprem "fail-closed", mas em camadas apropriadas.

**Consistência verificada:** `§B.1.4`, `§H` (acceptance criteria), `Task 4` e o integration test `non_loopback_no_auth_startup_err` referem exclusivamente o **listener dedicado**. A rota embedded é coberta por testes de runtime (401/403/503 response) no mesmo integration binary.

### Listener spawn (reference implementation em `garraia-gateway`)

`garraia-telemetry::install_recorder()` retorna apenas o `PrometheusHandle`. **Quem serve HTTP é o gateway:**

```rust
// crates/garraia-gateway/src/metrics_exporter.rs (novo)
use garraia_telemetry::install_recorder;

pub async fn spawn_dedicated_metrics_listener(
    cfg: MetricsAuthConfig,
    bind: SocketAddr,
) -> Result<(), MetricsExporterError> {
    // Fail-closed check antes do bind
    if !bind.ip().is_loopback() && cfg.token.is_none() && cfg.allowlist.is_empty() {
        tracing::error!(
            addr = %bind,
            "metrics auth not configured for non-loopback bind; listener disabled \
             (set GARRAIA_METRICS_TOKEN or GARRAIA_METRICS_ALLOW)"
        );
        return Err(MetricsExporterError::AuthNotConfigured);
    }

    let handle = install_recorder()?;  // garraia-telemetry, no HTTP
    let app = Router::new()
        .route("/metrics", get(move || {
            let handle = handle.clone();
            async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
                    .body(Body::from(handle.render()))
                    .unwrap()
            }
        }))
        .layer(from_fn_with_state(cfg.clone(), metrics_auth_layer))
        .into_make_service_with_connect_info::<SocketAddr>();

    let listener = TcpListener::bind(bind).await?;
    tracing::info!(addr = %bind, mode = describe_mode(&cfg), "metrics dedicated listener up");
    tokio::spawn(async move { axum::serve(listener, app).await.ok(); });
    Ok(())
}
```

O `server.rs` chama `spawn_dedicated_metrics_listener(...)` dentro de um `if cfg.metrics_enabled { ... }`; qualquer `Err` é logado e **não derruba** o gateway (preserva fail-soft).

## Design invariants

1. **Dev ergonomics preservado** — bind default `127.0.0.1:9464` sem token/ACL = 200 OK sem fricção.
2. **Fail-soft do gateway principal** — erro na inicialização do metrics listener dedicado NÃO derruba o gateway; main listener sobe independente.
3. **Fail-closed em 2 camadas (intencional)** — **listener dedicado** faz fail-closed no **startup** (não spawna quando insegura); **rota embedded** faz fail-closed em **runtime** (middleware retorna 401/403/503). Ambas são "negam acesso quando config inseguro", em camadas apropriadas a cada listener.
4. **Sem dependência reversa** — `garraia-telemetry` **não** depende de axum/hyper/tower nem de `garraia-gateway`. Toda lógica HTTP/auth vive em `garraia-gateway`. Zero ciclo de crate.
5. **Timing-safe token comparison** — `subtle::ConstantTimeEq`, não `==` em `&[u8]`.
6. **Reuso total de CIDR logic** — `rate_limiter::parse_trusted_proxies` (pub desde plan 0022); zero nova lógica CIDR.
7. **Zero mudança observável no gateway principal** — `AppState`, `SharedState`, rotas `/v1/*`, `/admin/*`, `/auth/*` não tocadas (exceto por `SharedState.metrics_auth_cfg: Arc<MetricsAuthConfig>` adicional, opacamente aplicado à rota `/metrics`).
8. **Secret redaction** — `GARRAIA_METRICS_TOKEN` entra no set sensível (mesmo tratamento de `GARRAIA_JWT_SECRET`); startup log mostra o modo (`"metrics: auth=token"`) mas nunca o valor.

## Tech Stack

- Axum 0.8 `State`, `ConnectInfo<SocketAddr>`, `from_fn_with_state`.
- `metrics-exporter-prometheus::{PrometheusBuilder, PrometheusHandle}` (já no workspace desde plan 0001).
- `ipnet::IpCidr` via `rate_limiter::parse_trusted_proxies` (já pub).
- `subtle::ConstantTimeEq` (transitivo via `garraia-auth` — adicionar direto em `garraia-gateway/Cargo.toml` se necessário).
- Nenhuma nova dependência direta esperada.

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-telemetry/src/config.rs` | Modify | `+ metrics_token: Option<String>` + `+ metrics_allowlist: Vec<String>` (strings raw; parse consumido pelo gateway) + env loaders + tests |
| `crates/garraia-telemetry/src/metrics.rs` | Modify | Pequeno: refatorar `init_metrics` para devolver `Option<PrometheusHandle>` via `PrometheusBuilder::install_recorder()`. **Remove** `with_http_listener()`. Zero HTTP, zero axum, zero middleware. |
| `crates/garraia-telemetry/src/lib.rs` | Modify | Re-export `PrometheusHandle` se necessário. **Não** re-exporta nada de HTTP/auth. |
| `crates/garraia-telemetry/Cargo.toml` | (sem mudança) | **Não adiciona** axum/hyper/tower — telemetry fica HTTP-free. |
| `crates/garraia-gateway/src/metrics_auth.rs` | Create | `MetricsAuthConfig` + `metrics_auth_layer` middleware + `describe_mode()` + 6 unit tests |
| `crates/garraia-gateway/src/metrics_exporter.rs` | Create | `spawn_dedicated_metrics_listener()` — bind TCP, wrap `PrometheusHandle::render()` num axum router com `metrics_auth_layer`, **faz o fail-closed startup check** da superfície dedicada |
| `crates/garraia-gateway/src/lib.rs` | Modify | `mod metrics_auth; mod metrics_exporter;` |
| `crates/garraia-gateway/src/router.rs` | Modify | `.layer(from_fn_with_state(state.metrics_auth_cfg.clone(), metrics_auth_layer))` na rota `/metrics` existente (linha 301). Zero mudança de startup do main listener. |
| `crates/garraia-gateway/src/server.rs` | Modify | Chamar `metrics_exporter::spawn_dedicated_metrics_listener(...)` condicional a `metrics_enabled`. Qualquer `Err` do spawn é logado e **não derruba** o boot (fail-soft). **Sem** startup check para o main listener. |
| `crates/garraia-gateway/src/state.rs` | Modify | `SharedState.metrics_auth_cfg: Arc<MetricsAuthConfig>` |
| `crates/garraia-gateway/Cargo.toml` | Modify | Add `subtle` se ainda não transitivo + `[[test]]` block para novo integration test |
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

## Task 3 — `garraia-telemetry::init_metrics` downgrade para recorder-only

**Files:** `crates/garraia-telemetry/src/metrics.rs`, `crates/garraia-telemetry/src/lib.rs`

**Escopo intencionalmente pequeno:** telemetry deixa de servir HTTP e volta ao seu papel puro de *coleta*.

- [ ] Refatorar `init_metrics(cfg) -> Result<Option<PrometheusHandle>, Error>` para usar `PrometheusBuilder::install_recorder()` (não mais `with_http_listener`). Retorna o handle para o caller consumir.
- [ ] Remover qualquer referência a `addr`/bind no `init_metrics` (a responsabilidade de bind saiu do crate).
- [ ] `garraia-telemetry/Cargo.toml` **não muda** — permanece sem axum/hyper/tower.
- [ ] Re-export `PrometheusHandle` em `lib.rs` se ainda não.
- [ ] Atualizar teste `disabled_config_returns_none` (continua válido — `None` quando `metrics_enabled == false`).
- [ ] `cargo test -p garraia-telemetry` PASS.
- [ ] `cargo check -p garraia-telemetry --no-default-features` PASS — confirma ausência de deps HTTP.

## Task 4 — Dedicated metrics listener no gateway (startup fail-closed)

**Files:** `crates/garraia-gateway/src/metrics_exporter.rs` (new), `crates/garraia-gateway/src/server.rs`, `crates/garraia-gateway/src/lib.rs`

- [ ] Criar módulo `metrics_exporter` com `spawn_dedicated_metrics_listener(cfg: MetricsAuthConfig, bind: SocketAddr, handle: PrometheusHandle) -> Result<(), MetricsExporterError>`.
- [ ] A função faz, em ordem:
  1. **Startup fail-closed check** — se `!bind.ip().is_loopback()` AND `cfg.token.is_none()` AND `cfg.allowlist.is_empty()` ⇒ `tracing::error!` com instrução de fix explícita, retorna `Err(MetricsExporterError::AuthNotConfigured)`. **Não spawna**.
  2. Monta axum `Router` com `GET /metrics` retornando `handle.render()` + content-type `text/plain; version=0.0.4; charset=utf-8`.
  3. Aplica `.layer(from_fn_with_state(cfg.clone(), metrics_auth_layer))`.
  4. `TcpListener::bind(bind).await?`.
  5. `tracing::info!(addr = %bind, mode = describe_mode(&cfg), "metrics dedicated listener up")`.
  6. `tokio::spawn(async move { axum::serve(listener, app).await.ok(); })`.
- [ ] `server.rs` — após `telemetry::init()`, se `cfg.metrics_enabled == true` e o handle é `Some`, chamar `spawn_dedicated_metrics_listener(auth_cfg, bind, handle).await`. Qualquer `Err` do spawn é **apenas logado** (fail-soft — gateway segue).
- [ ] `cargo check -p garraia-gateway` PASS.

## Task 5 — Main listener /metrics route: runtime-only fail-closed

**Files:** `crates/garraia-gateway/src/router.rs`, `crates/garraia-gateway/src/state.rs`

- [ ] `SharedState` ganha `metrics_auth_cfg: Arc<MetricsAuthConfig>` (construído a partir da `TelemetryConfig` no boot).
- [ ] `router.rs:301` — adicionar `.layer(from_fn_with_state(state.metrics_auth_cfg.clone(), metrics_auth_layer))` à rota `/metrics` existente.
- [ ] **`server.rs` NÃO ganha startup check para o main listener.** A rota embedded `/metrics` depende exclusivamente do middleware para bloquear requests indevidos em runtime (503 quando peer não-loopback + no auth; 401 token mismatch; 403 allowlist miss).
- [ ] **Racional** (para o PR body): main listener serve auth/v1/admin/ws/openclaw; derrubar boot por config de observabilidade seria regressão operacional. Fail-closed em runtime via middleware é a semântica correta para essa superfície.
- [ ] `cargo test -p garraia-gateway --lib metrics_auth` PASS (as 6 unit tests do middleware já cobrem 401/403/503/200).

## Task 6 — Integration tests

**Files:** `crates/garraia-gateway/tests/metrics_auth_integration.rs` (new), `crates/garraia-gateway/Cargo.toml`

- [ ] `[[test]] name = "metrics_auth_integration" required-features = ["test-helpers"]` em `Cargo.toml` (padrão plan 0022).
- [ ] 6 cenários. Cada um anota explicitamente qual das duas superfícies está testando (**dedicated** = listener spawned via `spawn_dedicated_metrics_listener`; **embedded** = rota `/metrics` do main router):
  1. **loopback_no_auth_ok** (**dedicated**) — chamar `spawn_dedicated_metrics_listener(empty_cfg, "127.0.0.1:0".parse(), handle)`; GET `/metrics` no listener spawned ⇒ 200 + content-type `text/plain; version=0.0.4`.
  2. **dedicated_non_loopback_no_auth_startup_err** (**dedicated**) — chamar `spawn_dedicated_metrics_listener(empty_cfg, "0.0.0.0:0".parse(), handle)` retorna `Err(MetricsExporterError::AuthNotConfigured)` **sem** spawnar; nenhuma porta fica aberta.
  3. **token_match_ok** (**dedicated**) — spawn com `MetricsAuthConfig { token: Some("dev"), allowlist: empty }` em `0.0.0.0:0`; GET `/metrics` com `Authorization: Bearer dev` ⇒ 200.
  4. **token_mismatch_401** (**dedicated**) — spawn com token `dev`; GET `/metrics` com `Authorization: Bearer wrong` ⇒ 401.
  5. **allowlist_match_ok** (**dedicated**) — spawn com `allowlist: ["127.0.0.0/8"]` em `0.0.0.0:0`; GET `/metrics` do 127.0.0.1 ⇒ 200.
  6. **allowlist_miss_403** (**dedicated**) — spawn com `allowlist: ["10.0.0.0/8"]` em `0.0.0.0:0`; GET `/metrics` do 127.0.0.1 ⇒ 403.
- [ ] **Bonus scenario (embedded, se T6 caber)**: boot um main gateway router com `metrics_auth_cfg` vazio, fazer request a `/metrics` simulando peer não-loopback (via `ConnectInfo` injection no test harness) ⇒ 503. Garante que main listener **sobe** mesmo com config insegura, e o middleware é quem bloqueia.
- [ ] Usar porta `0` (OS assignada) + ler `local_addr()` do listener antes de conectar; evita race.
- [ ] `cargo test -p garraia-gateway --features test-helpers --test metrics_auth_integration` PASS.

## Task 7 — Docs + env examples

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

## Task 8 — Reviews + PR + CI + merge

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

1. **Listener dedicado** (`GARRAIA_METRICS_BIND`): retorna **401** sem Bearer correto quando `GARRAIA_METRICS_TOKEN` configurado.
2. **Listener dedicado**: retorna **403** quando peer fora do `GARRAIA_METRICS_ALLOW`.
3. **Listener dedicado**: retorna **200** em deploy default (`127.0.0.1:9464`, sem token/ACL) — dev ergonomics preservado.
4. **Listener dedicado**: `spawn_dedicated_metrics_listener` retorna **`Err(MetricsExporterError::AuthNotConfigured)`** quando bind não-loopback + auth não configurada; **nenhum socket é aberto**; gateway principal continua saudável (fail-soft).
5. **Rota embedded `/metrics`** do main listener: o main gateway **sobe normalmente** com config insegura; requests não-loopback sem auth recebem **503 em runtime** via middleware; com auth configurada, 401/403/200 aplicam-se simétricos ao listener dedicado.
6. Nenhum ciclo de dependência: `garraia-telemetry` continua sem axum/hyper/tower (`cargo check -p garraia-telemetry --no-default-features` passa).
7. `cargo fmt --check --all` clean.
8. `cargo clippy --workspace --no-deps --tests -- -D warnings` clean.
9. `cargo test -p garraia-telemetry` + `cargo test -p garraia-gateway --lib` + `cargo test -p garraia-gateway --features test-helpers --test metrics_auth_integration` PASS.
10. Integration test matrix cobre 6 cenários **todos no listener dedicado**; bonus embedded opcional se couber.
11. `docs/telemetry.md` §6.1 existe + documenta 3 env vars + matriz de estados + deploy recommendations + **distinção explícita** entre fail-closed startup (dedicated) e fail-closed runtime (embedded).
12. `CLAUDE.md` lista `GARRAIA_METRICS_TOKEN` como sensível.
13. CI 9/9 green no merge commit.
14. Code review APPROVE + security audit ≥ 8.5/10 sem blockers HIGH.

## Rollback

Additive + forward-compatible. Revert do squash commit restaura comportamento pré-0024 (`/metrics` sem auth). Zero schema change.

**Rollback parcial em produção** (se operador precisa do comportamento antigo explicitamente): setar `GARRAIA_METRICS_ALLOW=0.0.0.0/0` — opt-in explícito de "qualquer peer permitido", documenta a intenção insegura no próprio env.

## Open questions

- **OQ-1:** `metrics_token` como `String` raw em `TelemetryConfig` ou como `SecretString`? **Decisão:** `Option<String>` raw (`TelemetryConfig` já é Debug via derive + printado em startup logs — precisa de custom Debug impl para redact OR mover o campo para struct separada). V1: custom `Debug` impl em `TelemetryConfig` que redact `metrics_token`. Se ficar feio, refatorar em follow-up.
- **OQ-2:** Integration test do main-listener `/metrics` precisa de 6 cenários próprios ou só os 6 do listener dedicado bastam? **Decisão:** 6 cenários do dedicated cobrem exaustivamente o middleware + startup. O main-listener reusa a **mesma** `MetricsAuthConfig` + **mesmo** `metrics_auth_layer`, então comportamento runtime (401/403/503/200) é idêntico por construção. T6 inclui **1 bonus scenario opcional para o embedded** (boot do main gateway + peer não-loopback simulado ⇒ 503) como smoke de wiring — garante que a rota recebeu `.layer()` do middleware. Bonus cai se review apertar escopo.
- **OQ-3:** Ownership do `MetricsAuthConfig` — em `garraia-telemetry` ou `garraia-gateway`? **Decisão (resolves review item #1):** `garraia-gateway::metrics_auth` é o dono. `TelemetryConfig` continua carregando as env vars (`metrics_token`, `metrics_allowlist`) como strings raw; `MetricsAuthConfig` parseia os CIDRs via `rate_limiter::parse_trusted_proxies` e fica em gateway junto do middleware e do HTTP runtime. Telemetria fica HTTP-free. Zero ciclo de crate.
- **OQ-4:** Fail-closed semantics — startup err em ambos listeners ou diferenciar? **Decisão (resolves review item #2):** diferenciar intencionalmente. Dedicated = startup fail-closed (não spawna). Embedded = runtime fail-closed (middleware 401/403/503). Racional no §"Fail-closed semantics — 2 listeners, 2 camadas".

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
