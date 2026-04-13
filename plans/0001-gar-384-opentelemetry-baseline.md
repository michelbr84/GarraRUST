# Plan: GAR-384 — OpenTelemetry baseline (`garraia-telemetry`)

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-384](https://linear.app/chatgpt25/issue/GAR-384/crate-garraia-telemetry-com-opentelemetry-baseline)
> **Project:** Fase 2 — Performance, RAG & MCP
> **Labels:** `epic:otel`
> **Priority:** Urgent
> **Estimated session size:** 1 dia de trabalho focado (~6-8 horas)
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13

---

## 1. Goal (one sentence)

Criar o crate `garraia-telemetry` que instala, no startup do gateway, um pipeline OpenTelemetry + tracing de uma linha, de modo que toda requisição HTTP/WS/admin passe a emitir spans correlacionados (`request_id` → `auth.verify` → `agent.run` → `llm.call` → `db.persist`) e métricas Prometheus-style, sem quebrar nenhum comportamento atual.

---

## 2. Rationale — por que esse agora

1. **Não-bloqueante por ADR.** Diferente de `garraia-auth` (GAR-391, bloqueado por ADR 0005) ou `garraia-workspace` (GAR-407, bloqueado por ADR 0003), OTel é decisão consolidada da comunidade Rust (tracing + opentelemetry + tracing-opentelemetry).
2. **Aditivo, não destrutivo.** Não mexe em crypto, auth, storage ou DB. Só adiciona layers de `tracing_subscriber`. Downside máximo de bug: logs mais verbosos. Feature flag permite desligar inteiro.
3. **Alavancagem máxima.** Todo código que virá depois (vault, config, workspace, auth, storage) herda traces de graça. Instrumentar retroativamente custa 3-5x mais.
4. **Debuga os próximos issues.** Quando `garraia-config` (GAR-379) começar a refatorar cross-crate, ter traces prontos vai economizar horas de investigação.
5. **Baseline de SLO.** Os critérios de aceite de Fase 6 (chat p95 < 500ms, upload > 99%, auth < 100ms) só são mensuráveis com métricas já coletando há semanas antes do GA.
6. **Pedagógico.** Define o padrão (span names, attributes, redaction) que os 30+ issues seguintes vão replicar.

---

## 3. Scope & Non-Scope

### In scope

- Novo crate `crates/garraia-telemetry/` com API pública mínima: `init(config) -> Guard` + `shutdown()`.
- Integração em `garraia-gateway::main` e `bootstrap.rs`.
- Layer `tracing-opentelemetry` exportando OTLP (gRPC) para endpoint configurável.
- Layer Prometheus exportando `/metrics` no eixo admin (porta separada ou mesmo eixo com auth).
- Spans automáticos via `tower-http::TraceLayer` para todas as rotas Axum.
- Propagação de `request_id` (UUID v7) via `tower-http::request_id`.
- **PII redaction** no layer de logging: `Authorization`, `Cookie`, `*_api_key`, `password`, `token`, `jwt`, `secret` → `[REDACTED]`.
- 4 métricas iniciais (`garraia_requests_total`, `garraia_http_latency_seconds`, `garraia_errors_total`, `garraia_active_sessions`).
- Feature flag `telemetry` em `garraia-gateway/Cargo.toml` (default = on; permite opt-out em CLI single-user).
- docker-compose overlay `ops/compose.otel.yml` com Jaeger + Prometheus + Grafana para dev local.
- Documentação mínima: `docs/telemetry.md` (como ler traces, como adicionar span novo, como ver dashboard).
- 1 teste de integração que roda o gateway, faz 1 chamada `/health`, e assert que o Jaeger coletou ≥ 1 span com o `request_id` esperado.

### Out of scope (ficam para issues futuros)

- ❌ GAR-385 (Prometheus metrics completas — contadores por modelo/provider/tool). Apenas baseline (4 métricas). Issue separado.
- ❌ Dashboards Grafana prontos para cada subsistema (fica em `ops/grafana/` — issue futuro).
- ❌ Alerting rules.
- ❌ Distributed tracing cross-serviço (só instância única por enquanto).
- ❌ Exporters alternativos (Honeycomb, Datadog, New Relic) — OTLP é universal, config via env.
- ❌ Redaction avançada de PII em corpos de request/response (só headers nesta fatia).
- ❌ Instrumentação de `garraia-agents`, `garraia-db`, `garraia-channels` — ficam em issues filhas (`GAR-384-subN` a criar se aprovado).

---

## 4. Acceptance criteria (verificáveis)

Cada item abaixo será marcado em checklist no PR:

- [ ] `cargo check -p garraia-telemetry` verde.
- [ ] `cargo check --workspace` verde (sem regressão).
- [ ] `cargo clippy --workspace -- -D warnings` verde.
- [ ] `cargo test -p garraia-telemetry` verde (unit + 1 integração).
- [ ] Gateway inicia com `GARRAIA_OTEL_ENABLED=false` e se comporta idêntico à baseline atual (zero overhead detectável).
- [ ] Gateway inicia com `GARRAIA_OTEL_ENABLED=true GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` e emite spans sem panic.
- [ ] `GET /health` gera span raiz `http.request{method=GET, route=/health, status_code=200}` visível no Jaeger local.
- [ ] Um chat completion gera cadeia: `http.request` → `auth.verify` → `agent.run` → `llm.call{provider=...}` → `db.persist`.
- [ ] `GET /metrics` retorna ≥ 4 métricas Prometheus com contadores incrementando a cada request.
- [ ] Nenhum `Authorization: Bearer ...` aparece em logs capturados por `tracing-test` em nenhum path.
- [ ] Feature flag `telemetry` desligada compila e roda (código cercado por `#[cfg(feature = "telemetry")]`).
- [ ] `docker compose -f docker-compose.yml -f ops/compose.otel.yml up` sobe stack (gateway + jaeger + prometheus + grafana) em < 2 min numa máquina com docker limpo.
- [ ] `docs/telemetry.md` explica: (a) como ligar, (b) como ver um trace, (c) como adicionar span novo a uma função existente.
- [ ] Zero secret exposto em logs — verificado via `cargo test -p garraia-gateway --test redaction_smoke`.

---

## 5. File-level changes

### 5.1 Novo crate `crates/garraia-telemetry/`

```
crates/garraia-telemetry/
├── Cargo.toml
├── src/
│   ├── lib.rs              # API pública: init(), shutdown(), Guard, Config
│   ├── config.rs           # TelemetryConfig (serde + validator)
│   ├── tracer.rs           # pipeline tracing-opentelemetry (OTLP gRPC)
│   ├── metrics.rs          # pipeline metrics-exporter-prometheus
│   ├── redact.rs           # filter de headers/fields sensíveis
│   └── layers.rs           # tower-http::TraceLayer + request_id helpers
└── tests/
    └── smoke.rs            # 1 teste: init + emit span + shutdown limpo
```

### 5.2 Edits em crates existentes

- `Cargo.toml` (workspace root): adiciona `garraia-telemetry` ao `members`.
- `crates/garraia-gateway/Cargo.toml`:
  - adiciona dep `garraia-telemetry = { path = "../garraia-telemetry" }` atrás de feature flag `telemetry` (default = `["telemetry"]`).
- `crates/garraia-gateway/src/main.rs`:
  - logo no topo do `main()`, antes do bootstrap: `let _telemetry_guard = garraia_telemetry::init(config.telemetry.clone())?;`
  - remove `tracing_subscriber::fmt::init()` atual (substituído pelo init do crate novo).
- `crates/garraia-gateway/src/bootstrap.rs`:
  - passa config de telemetry via `AppState` (read-only).
- `crates/garraia-gateway/src/server.rs`:
  - adiciona `.layer(garraia_telemetry::layers::http_trace_layer())` e `.layer(garraia_telemetry::layers::request_id_layer())` na árvore Axum.
  - nova rota `GET /metrics` montada fora do router autenticado (admin-only via bearer ou unprotected local-only, decidir em review).
- `crates/garraia-agents/src/runtime.rs`:
  - adiciona `#[tracing::instrument(skip(self, req), fields(provider, model))]` em `process_message_*`.
- `crates/garraia-db/src/session_store.rs`:
  - adiciona `#[tracing::instrument(skip(self))]` em métodos públicos principais (hydrate/persist/fetch).

### 5.3 Config changes

- `.env.example`: descomenta e documenta as 4 vars OTel que já existem como placeholder (adicionadas no commit `d0046b8`).
- Nenhum novo secret. O endpoint OTLP não é sensível.

### 5.4 Infra (novos arquivos)

- `ops/compose.otel.yml` — overlay docker-compose com:
  - `jaegertracing/all-in-one:1.60` → portas 16686 (UI), 4317 (OTLP gRPC).
  - `prom/prometheus:v2.54` com `ops/prometheus.yml` mínimo.
  - `grafana/grafana:11.2` com datasource Jaeger + Prometheus pré-configurado.
- `ops/prometheus.yml` — scrape_config apontando para `garraia-gateway:3888/metrics`.
- `ops/grafana/provisioning/datasources/garraia.yml` — datasource provisioning.

### 5.5 Docs

- `docs/telemetry.md` — guia de 1-2 páginas.
- `CLAUDE.md` — já menciona `garraia-telemetry` como crate planejado; não precisa mudar.
- `ROADMAP.md` — não precisa mudar (item `[ ]` da Fase 2.3 ficará checkável após merge).

---

## 6. Dependencies / crate versions

Adicionar ao `crates/garraia-telemetry/Cargo.toml` (versões escolhidas por compatibilidade com tracing ecosystem 2026-Q2):

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }
tracing-opentelemetry = "0.27"
opentelemetry = { version = "0.26", features = ["trace", "metrics"] }
opentelemetry_sdk = { version = "0.26", features = ["rt-tokio", "trace", "metrics"] }
opentelemetry-otlp = { version = "0.26", features = ["grpc-tonic", "trace", "metrics"] }
opentelemetry-semantic-conventions = "0.26"
metrics = "0.23"
metrics-exporter-prometheus = "0.15"
tower-http = { version = "0.6", features = ["trace", "request-id", "util"] }
tower = "0.5"
axum = "0.8"
serde = { workspace = true, features = ["derive"] }
validator = { version = "0.18", features = ["derive"] }
uuid = { version = "1", features = ["v7"] }
thiserror = { workspace = true }
anyhow = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
tracing-test = "0.2"
```

**Verificar antes de tocar em código:** as versões acima precisam ser confirmadas via `cargo search` ou crates.io no início da execução, pois tracing-opentelemetry evolui rápido. Ajusto se houver mismatch com o workspace existente.

---

## 7. Test plan

### 7.1 Unit tests em `garraia-telemetry`

1. `redact::strips_authorization_header` — garante que header `Authorization` vira `[REDACTED]`.
2. `redact::strips_bearer_inside_json_body` — placeholder (fica em TODO, fora do escopo MVP).
3. `config::parses_from_env` — vars `GARRAIA_OTEL_*` mapeiam para struct.
4. `config::validates_endpoint_url` — URL inválida falha com erro legível.
5. `tracer::init_and_shutdown_idempotent` — chamar `init()` duas vezes não panica.

### 7.2 Integration test em `garraia-telemetry/tests/smoke.rs`

- Sobe `opentelemetry_sdk::testing::trace::InMemorySpanExporter`.
- `init(Config { backend: InMemory, ... })`.
- Emite `tracing::info_span!("test.span")`.
- Assert que o exporter coletou 1 span com o nome esperado.
- `shutdown()` limpo.

### 7.3 Integration test em `garraia-gateway/tests/redaction_smoke.rs`

- `tracing_test::traced_test` captura saída.
- Faz `POST /auth/login` com `Authorization: Bearer sekret`.
- `assert!(!logs_contain("sekret"))`.

### 7.4 Smoke manual (checklist no PR)

- [ ] `docker compose -f docker-compose.yml -f ops/compose.otel.yml up`
- [ ] `curl localhost:3888/health`
- [ ] Abrir `http://localhost:16686` (Jaeger UI) → encontrar service `garraia-gateway` → ver trace `GET /health`
- [ ] Abrir `http://localhost:9090` (Prometheus) → query `garraia_requests_total` → valor ≥ 1
- [ ] Abrir `http://localhost:3000` (Grafana) → datasource provisionado

---

## 8. Rollback plan

OTel é aditivo. Rollback em 3 níveis:

1. **Runtime:** `GARRAIA_OTEL_ENABLED=false` no `.env` → pipeline OTel não inicializa, zero overhead.
2. **Build:** `cargo build --no-default-features --features ...` sem `telemetry` → crate nem é linkado.
3. **Git:** `git revert d0046b8+1` reverte o commit do PR; `garraia-telemetry` some do workspace sem quebrar nada (nenhum outro crate depende dele diretamente fora do gateway).

Nenhuma migration de DB, nenhum secret novo, nenhum breaking change em API pública.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Versões de `tracing-opentelemetry` vs `opentelemetry` não alinhadas | **Alta** | bloqueia compile | Começar sessão validando via `cargo tree` antes de codar |
| Overhead de spans degrada latência | Baixa | médio | Sampling configurável via `GARRAIA_OTEL_SAMPLE_RATIO` (default 1.0 em dev, recomendar 0.1 em prod) |
| Exporter OTLP bloqueia se Jaeger offline | Média | médio | Usar `BatchSpanProcessor` com timeout curto + exporter em background task; falhas de export só viram log warning |
| Vazamento de PII em span attributes | Média | **alto** | Redact layer centralizado + teste `redaction_smoke` + code review por `@security-auditor` |
| `/metrics` exposto sem auth vaza cardinalidade do sistema | Baixa | baixo | Montar fora do router público; acessível só em `127.0.0.1` por padrão ou atrás de bearer admin |
| Breaking change em `tracing-subscriber` removendo `fmt::init()` | Baixa | baixo | Guardar compat via feature flag, remover em PR separado se necessário |

---

## 10. Sequence of work (ordem proposta quando aprovado)

1. **Validação de versões** (~20 min)
   - `cargo search tracing-opentelemetry` → confirmar última compatível com `opentelemetry 0.26`.
   - Confirmar que `axum 0.8` + `tower-http 0.6` + `tower 0.5` estão alinhados no workspace.
   - Ajustar versões na tabela §6 se necessário.
2. **Scaffold do crate** (~30 min)
   - Criar diretório + `Cargo.toml` + `src/lib.rs` stub.
   - Adicionar ao workspace `members`.
   - `cargo check -p garraia-telemetry` verde com stub.
3. **Config + validação** (~30 min)
   - `src/config.rs` com `TelemetryConfig` + teste unit.
4. **Tracer pipeline** (~1.5 h)
   - `src/tracer.rs` com builder OTLP.
   - `init()` retorna `Guard` que chama `shutdown()` em `Drop`.
   - Unit test idempotência.
5. **Metrics pipeline** (~45 min)
   - `src/metrics.rs` com `metrics-exporter-prometheus`.
   - 4 métricas iniciais definidas via macros `counter!`/`histogram!`/`gauge!`.
6. **Redact layer** (~45 min)
   - `src/redact.rs` com filter de campos.
   - 2 unit tests.
7. **Layers prontos para Axum** (~30 min)
   - `src/layers.rs` com `http_trace_layer()` + `request_id_layer()`.
8. **Integração no gateway** (~1 h)
   - Edits em `main.rs`, `server.rs`, `bootstrap.rs`.
   - Feature flag `telemetry`.
   - `cargo check --workspace` verde.
9. **Instrumentação mínima** (~45 min)
   - `#[tracing::instrument]` em `AgentRuntime::process_message_*`.
   - `#[tracing::instrument]` em `SessionStore` principais.
10. **Infra docker-compose** (~30 min)
    - `ops/compose.otel.yml` + `ops/prometheus.yml` + `ops/grafana/provisioning/...`
11. **Integration tests** (~45 min)
    - `tests/smoke.rs` no crate + `tests/redaction_smoke.rs` no gateway.
12. **Docs** (~30 min)
    - `docs/telemetry.md` de 1-2 páginas.
13. **Smoke manual + screenshots** (~20 min)
    - docker compose up → Jaeger → Prometheus → Grafana → screenshots pro PR.
14. **Clippy pass + review round** (~20 min)
    - `cargo clippy --workspace -- -D warnings`.
    - Self-review + spawn `@code-reviewer` agent.
15. **Commit + PR** (~10 min)

**Total estimado: 8-9 horas de trabalho focado.**

Se estourar o orçamento em mais de 50%, pausamos, commitamos parcial atrás da feature flag e retomamos em sessão seguinte sem bloquear os outros issues urgentes.

---

## 11. Definition of Done

- [ ] Todos os 13 itens do §4 (acceptance criteria) marcados.
- [ ] PR aberto no GitHub com link para este `plan.md`.
- [ ] Review verde de `@code-reviewer` e `@security-auditor`.
- [ ] Screenshots do Jaeger + Prometheus + Grafana anexados ao PR.
- [ ] Issue GAR-384 movida para **In Review** no Linear.
- [ ] Após merge: GAR-384 → **Done**; criar sub-issues para instrumentação de `garraia-agents`, `garraia-db`, `garraia-channels` se ainda não cobertas.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`/metrics` auth:** quer exposto só em `127.0.0.1` (default seguro) ou atrás do bearer admin existente? Recomendo **127.0.0.1 only** no default; quem quiser expor fora muda via config.
2. **Feature flag default:** `telemetry = on` por padrão (assume que todo mundo quer OTel) ou `off` (opt-in)? Recomendo **`on`** — Jaeger/Prometheus só ligam se `GARRAIA_OTEL_ENABLED=true` em runtime, então default compile-on é seguro.
3. **Sampling default em dev:** `1.0` (100%, tudo) ou `0.1` (10%)? Recomendo **1.0 em dev, 0.1 documentado como recomendação prod**.
4. **Já instrumento `garraia-channels` nesta sessão ou deixo pra issue separada?** Recomendo **deixar pra issue separada** — mantém essa fatia cirúrgica e testável.

---

## 13. Next recommended issue (depois de GAR-384 merged)

Com OTel no ar, a próxima fatia ótima é uma das duas abaixo (escolha sua):

- **GAR-373 ADR Postgres** — destrava toda a Fase 3. Research + decisão. ~4 horas.
- **GAR-379 `garraia-config`** — refactor cross-crate já tendo traces prontos pra debug. ~2-3 dias.

Recomendação minha: **GAR-373 primeiro** (curto e destrava 7+ outros issues), depois GAR-379.

---

**Aguardando sua aprovação.** Se aprovar como está, começo pelo passo 1 do §10. Se quiser ajustar escopo (ex.: "deixa de fora o metrics por enquanto", "adiciona instrumentação de channels também", "troca Jaeger por Tempo"), me diga antes que eu toque em código.
