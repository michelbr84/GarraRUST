# Plan 0025 — GAR-411: Telemetry hardening (REDACT_HEADERS + idempotent init + TLS docs + cardinality guards)

> **Narrow slice:** 4 of 6 follow-ups from the GAR-384 security review (plan 0001, merged 2026-04-13). Zero new dependencies, zero schema changes, zero breaking API changes. M3 (init signature) and L5 (nightly cargo-audit CI) explicitly out of scope — see §Non-scope.

**Linear issue:** [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) — "Telemetry hardening: TLS docs, cardinality guards, idempotent init" (Backlog → In Progress, Medium, labels `security` + `epic:otel`, project Fase 2 — Performance, RAG & MCP).

**Status:** Draft v1 — 2026-04-21.

**Relationship note:** continuação direta do security review de GAR-384 (plan 0001). Itens M5, L3, M2, M1 identificados pelo `security-auditor` na época como follow-ups "nice-to-have" mas foram deferidos para manter o slice baseline enxuto. Este plano fecha 4 deles. Os 2 restantes (M3 `init()` signature change + L5 nightly cargo-audit CI job) ficam para plan 0026+ — ambos têm blast radius maior (API break + CI surface).

**Goal:** fechar lacunas de PII/operational hygiene no crate `garraia-telemetry` sem mudar assinatura pública ou introduzir custo operacional novo. Entrega: headers sensíveis extras redated, `init()` verdadeiramente idempotente, warning TLS em produção, contrato de cardinalidade documentado + helper em debug.

## Scope

1. **M5 — Expand REDACT_HEADERS** — adicionar `x-forwarded-authorization` + `x-original-authorization` (+ `x-amzn-trace-id` como variante de trace-context) à lista em `redact.rs:3-10`. Reverse proxies (nginx, AWS ALB, Traefik) propagam tokens via esses headers; sem isso, logs estruturados podem vazar JWT.
2. **L3 — Truly idempotent `init()`** — a segunda chamada de `init()` hoje loga warning mas ainda substitui o tracer provider global (`global::set_tracer_provider()`). Fix: após o primeiro `init` bem-sucedido, a segunda chamada retorna imediatamente um `Guard` no-op (sem re-instalar tracer/recorder). Usa `OnceLock<()>` como flag. Preserva o warning para rastreabilidade operacional.
3. **M2 — OTLP TLS warning in docs** — nova §1.1 em `docs/telemetry.md` explicando que `opentelemetry-otlp` + `tonic` usam plaintext por default; `https://` endpoint + TLS do lado do collector é responsabilidade do operador. Callout explícito + exemplo `GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=https://otel.example.com:4317`.
4. **M1 — Bound route label cardinality** — documentar inline em `metrics.rs::inc_requests` / `record_latency` que o argumento `route` deve ser **template** (`/api/sessions/{id}/messages`), não path concreto (`/api/sessions/abc-uuid/messages`). Adicionar helper `debug_assert_route_template(route)` que dispara em `cfg(debug_assertions)` quando detecta padrões UUID ou numeric ID crus. Em release builds é no-op — zero overhead.

## Non-scope

- **M3 — `init()` signature change** (`Result<Guard, Error>` → `Guard`). API break; 2 call sites (CLI + smoke test) precisam migrar. Merece plan próprio para garantir que os unwrap_or_else dos callers sejam migrados de forma consistente e que o CLI startup continue fail-soft. **Deferido para plan 0026.**
- **L5 — Nightly `cargo audit` CI job**. Requer novo workflow file (`.github/workflows/cargo-audit.yml`), decisão de threshold (fail PR vs. apenas alertar), scheduling (cron + on-demand), e validação empírica rodando em GitHub Actions. Deferido para plan 0026+.
- Zero mudança em `metrics_exporter_prometheus` wiring (plan 0024 já endereçou `/metrics` security).
- Zero mudança em `rate_limiter` (plan 0022 endereçou XFF parsing).
- Zero mudança em providers/agents/auth.

## Tech stack

- `std::sync::OnceLock` (stable, zero-dep) para idempotência de `init()`.
- `debug_assert!` macro (core) para cardinality guards.
- Markdown puro para docs/telemetry.md.
- **Nenhuma nova dependência** em `Cargo.toml`.

## File structure

| File | Action | Responsibility |
|---|---|---|
| `crates/garraia-telemetry/src/redact.rs` | Modify | REDACT_HEADERS: +3 entries + 2 test cases |
| `crates/garraia-telemetry/src/lib.rs` | Modify | Swap `AtomicBool` → `OnceLock<()>` + short-circuit; truly idempotent |
| `crates/garraia-telemetry/src/metrics.rs` | Modify | Docblock contract + `debug_assert_route_template()` helper + call from `inc_requests`/`record_latency` |
| `crates/garraia-telemetry/tests/idempotent_init.rs` | Create | Integration test: 3× `init()` call must leave exactly 1 installed tracer |
| `docs/telemetry.md` | Modify | New §1.1 "TLS for OTLP exports" with `https://` recommendation |
| `plans/0025-gar-411-telemetry-hardening.md` | Create | This plan file |
| `plans/README.md` | Modify | Index entry 0025 |

**No migrations. No schema changes. No new crates. No new dependencies.**

## Design invariants

1. **Zero breaking change** — assinaturas públicas (`init`, `Guard`, `inc_requests`, `record_latency`, `REDACT_HEADERS` are `&[&str]`) preservadas. Callers não mudam.
2. **Fail-soft preserved** — se `init()` já rodou, a segunda call não explode nem bloqueia; retorna guard vazio e loga warning.
3. **Zero release-build overhead** — cardinality helper é `debug_assert!` puro.
4. **PII redaction monotonicamente crescente** — só *adicionamos* headers à lista, nunca removemos.
5. **Docs-as-safety-net** — o TLS warning é a única rede de proteção hoje (tonic não suporta fácil default-TLS); melhor no docs do que em código frágil.

## Tasks

### Task 1 — RED: integration test for truly idempotent init

Create `crates/garraia-telemetry/tests/idempotent_init.rs`:

```rust
//! Regression: `init()` must be idempotent. Three sequential calls should
//! leave exactly one tracer provider installed (not replace on every call).
//! Plan 0025 / GAR-411 L3.
use garraia_telemetry::{TelemetryConfig, init};

#[test]
fn three_inits_leave_one_provider() {
    let cfg = TelemetryConfig::default(); // enabled=false, metrics_enabled=false
    let _g1 = init(cfg.clone()).expect("first init ok");
    let _g2 = init(cfg.clone()).expect("second init must not error");
    let _g3 = init(cfg).expect("third init must not error");
    // If we reach here without panic, idempotency holds: the second+third
    // calls returned empty guards instead of re-installing providers.
}
```

Expected state before impl: test compiles but would fail if we asserted tracer identity. For this slice, the observable contract is "no error + no panic + no global clobber" — tests for that via the absence of panic on repeat calls with non-default configs (tracer/metrics enabled) are covered by a second scenario in this file.

### Task 2 — GREEN: swap AtomicBool to OnceLock short-circuit

Edit `crates/garraia-telemetry/src/lib.rs`:

- Replace `static INIT_ONCE: std::sync::atomic::AtomicBool` with `static INIT_ONCE: std::sync::OnceLock<()>`.
- In `init()`: after `INIT_ONCE.get().is_some()` check, log the warning **and return an empty Guard** (both `tracer_provider` and `metrics_handle` = `None`). Skip `tracer::init_tracer` + `metrics::init_metrics`.
- Call `INIT_ONCE.set(()).ok();` after success.

### Task 3 — GREEN: expand REDACT_HEADERS

Edit `crates/garraia-telemetry/src/redact.rs`:

- Add `"x-forwarded-authorization"`, `"x-original-authorization"`, `"x-amzn-trace-id"` to the array.
- Extend `strips_authorization_header_case_insensitive` test with new headers.
- Add explicit test ensuring case-insensitive matching still works for the new headers.

### Task 4 — GREEN: cardinality guard helper

Edit `crates/garraia-telemetry/src/metrics.rs`:

- Expand the module-level docblock with a "Cardinality contract" section stating that callers MUST pass route templates (e.g. `/api/sessions/{id}/messages`), never concrete paths (e.g. `/api/sessions/8f2c…/messages`).
- Add `fn debug_assert_route_template(route: &str)` that, under `cfg(debug_assertions)`, checks for UUID-looking segments (36-char `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` pattern) and long all-numeric segments, panicking with an explanatory message. In release: `#[inline(always)] fn ...(_: &str) {}` (no-op).
- Call it from `inc_requests` and `record_latency`.
- Add 3 unit tests: valid template passes, UUID segment panics in debug, numeric ID segment panics in debug (guarded by `#[cfg(debug_assertions)]`).

### Task 5 — DOCS: TLS warning for OTLP exports

Edit `docs/telemetry.md`:

Insert new subsection `### 1.1 TLS for OTLP exports` right after the `.env example` block. Content:

- Default behavior: `tonic` uses HTTP/2 cleartext (`http://`) unless the endpoint scheme is `https://`.
- Risk: PII in span attributes (e.g. `http.url` capturing query strings with tokens) traverses the wire unencrypted.
- Recommendation: set `GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=https://…` in production. Your collector (Jaeger, OTel Collector) must terminate TLS.
- For local dev: `http://localhost:4317` is fine — the collector runs on the same host.
- Example config snippet.

### Task 6 — Wire plan 0025 into index

Edit `plans/README.md` with new row:

```
| 0025 | [GAR-411 — Telemetry hardening](0025-gar-411-telemetry-hardening.md) | [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) | Draft 2026-04-21 |
```

Update to Merged status post-merge.

## Acceptance criteria

1. `cargo test -p garraia-telemetry` passa (incluindo novo `idempotent_init.rs` integration test + novos unit tests em `redact.rs` e `metrics.rs`).
2. `cargo clippy --workspace -- -D warnings` sem novos warnings.
3. Chamar `init(cfg)` 3× em sequência não entra em pânico e não clobra tracer provider.
4. `redact_header_value("X-Forwarded-Authorization", "Bearer abc")` retorna `"[REDACTED]"`.
5. `inc_requests("/api/sessions/abc-def-1234-5678-90abcdef1234/messages", 200)` panica em debug build, no-op em release (verificável por `#[cfg(debug_assertions)]` test).
6. `docs/telemetry.md` §1.1 menciona `https://`, `tonic` plaintext default, e recomendação de TLS.
7. Zero mudança em `Cargo.toml` (dependências).
8. Zero callers de `init()` precisam mudar (API preservada).

## Rollback plan

Reversível. Cada tarefa é um commit independente. Se qualquer um quebrar:

- Task 2 (OnceLock): reverter para `AtomicBool` + `.swap()` — comportamento antigo volta.
- Task 3 (REDACT_HEADERS): reverter o array para 6 entries. Zero efeito colateral.
- Task 4 (cardinality helper): reverter metrics.rs; os callers voltam a não ter guard.
- Task 5 (docs): reverter markdown; zero código afetado.

Tudo é forward-only via commits; zero migration, zero schema, zero data loss path.

## Open questions

Nenhuma. Scope totalmente local ao crate `garraia-telemetry` + 1 doc file. Nenhuma decisão externa necessária.

## Review plan

- **Code review agent** (`code-reviewer`): revisar os 4 commits sequenciais (Task 2, 3, 4, 5) com foco em: correctness de `OnceLock`, robustez do `debug_assert_route_template` (não gerar falso-positivo em paths legítimos como `/v1/groups/{group_id}` que tem literal "group_id"), estilo consistente com plans 0019–0024.
- **Security audit agent** (`security-auditor`): validar que a expansão de REDACT_HEADERS cobre os vetores conhecidos de header-rewriting em reverse proxies; validar que a guard de idempotência é race-free (duas threads chamando `init` simultaneamente).
- Dois agentes rodam **em paralelo** na mesma branch.
