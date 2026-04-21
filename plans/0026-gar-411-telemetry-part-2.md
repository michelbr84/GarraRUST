# Plan 0026 — GAR-411 Telemetry hardening part 2 (init signature + cargo-audit CI + IAP headers + isolated-process test)

> **Follow-up direto do plan 0025** (merged 2026-04-21, PR #39). Fecha os itens deferidos do review: M3 (init signature API break), L5 (nightly cargo-audit workflow), SA-L-A (cloud-LB IAP headers), SA-L-E (isolated-process test for double-install race). Zero nova dependência Rust; adiciona 1 GitHub Action externa.

**Linear issue:** [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) — já marcada **Done** pelo plan 0025. Este plano é reabertura tática para fechar os 4 follow-ups que foram explicitamente deferidos no comentário de fechamento da issue. Não reabre a issue — trata como "comentário pós-merge" da própria GAR-411.

**Status:** Draft v1 — 2026-04-21.

## Goal

Completar o espírito original de GAR-411 fechando todos os 6 follow-ups do security review de GAR-384: os 4 entregues em 0025 (M5+L3+M2+M1) + os 4 deste plano (M3, L5, SA-L-A, SA-L-E). O resultado é um subsistema de telemetria completo contra o threat model de GAR-384.

## Scope

1. **M3 — `init()` signature change:** `pub fn init(config: TelemetryConfig) -> Result<Guard, Error>` → `pub fn init(config: TelemetryConfig) -> Guard`. Internamente: qualquer erro do tracer ou recorder é convertido em `tracing::warn!` + `Guard { tracer_provider: None, metrics_handle: None }` (degraded no-op guard). Alinha com o contrato fail-soft que o CLI já implementa via `.map_err(log).ok()`. Impacto: 3 call sites (CLI + 2 test files).

2. **L5 — Nightly `cargo audit` CI workflow:** novo arquivo `.github/workflows/cargo-audit.yml` rodando em `schedule: cron '0 7 * * *'` (3 AM America/New_York = ~7 UTC) + `workflow_dispatch`. **Usa `cargo install --locked cargo-audit` + `cargo audit --deny unsound --deny yanked`** em vez da action `rustsec/audit-check@v2` originalmente considerada — trade-off deliberado: a abordagem manual zera a superfície de supply-chain de action externa (zero `uses:` de terceiros além de `actions/checkout` + `dtolnay/rust-toolchain`, já usados no projeto), em troca de perder integração com GitHub Security tab (SARIF). O output continua visível como run vermelha no Actions tab + default email notification. Falha em qualquer vulnerabilidade detectada, em deps com aviso `unsound`, ou em crates yanked. Pull requests não disparam (evita block) — é job noturno de observabilidade.

3. **SA-L-A — Cloud-LB IAP headers:** adicionar `x-goog-iap-jwt-assertion` (GCP IAP), `cf-access-jwt-assertion` (Cloudflare Access), `x-ms-client-principal` (Azure Front Door), `x-forwarded-user` (generic SSO) à `REDACT_HEADERS`. Cobertura pré-protetiva para quando deploy target cloud-LB for declarado.

4. **SA-L-E — Isolated-process test for double-install race:** novo integration test binary `crates/garraia-telemetry/tests/idempotent_init_isolated.rs` exercitando exclusivamente o cenário "primeira chamada com `metrics_enabled=true` instala recorder; segunda chamada short-circuits". Por ser `.rs` separado em `tests/`, Cargo gera binário dedicado com `INIT_ONCE` / recorder global isolados do `idempotent_init.rs` e do `smoke.rs`. Garante cobertura determinística do strong-RED independente de ordem de teste.

## Non-scope

- Qualquer outro follow-up não-explicitamente deferido pelo plan 0025 (ex.: SA-L-B ULID/nanoid detection, SA-L-C cross-ref já implementado).
- Mudança em `TracerProvider` ou `PrometheusBuilder` wiring.
- Deploy cloud-LB concreto (apenas pré-proteção de headers).
- Refactor de `init_tracer` / `init_metrics` (as funções-filhas continuam retornando `Result`; a conversão fail-soft vive em `init()`).

## Tech stack

- `std::sync::OnceLock<()>` (já presente, plan 0025).
- `tracing::warn!` + `eprintln!` dual-emit (plan 0026 security audit F-1 — garante observabilidade mesmo sem subscriber configurado).
- `cargo install --locked cargo-audit` em `ubuntu-latest` GitHub-hosted runner (zero action de supply-chain externa).
- **Nenhuma nova dependência** em `Cargo.toml`.

## File structure

| File | Action | Responsibility |
|---|---|---|
| `crates/garraia-telemetry/src/lib.rs` | Modify | `init()` signature → `Guard`; internal error-to-log shim; docblock updates |
| `crates/garraia-telemetry/tests/idempotent_init.rs` | Modify | Drop `.expect(...)` — `init()` no longer returns `Result` |
| `crates/garraia-telemetry/tests/smoke.rs` | Modify | Drop `.expect(...)` |
| `crates/garraia-telemetry/tests/idempotent_init_isolated.rs` | Create | Single-test binary exercitando strong-RED scenario em process isolation |
| `crates/garraia-telemetry/src/redact.rs` | Modify | `REDACT_HEADERS` +4 IAP entries + test case |
| `crates/garraia-cli/src/main.rs` | Modify | Simplify `init_telemetry_guard()` — drop `.map_err().ok()`, return `Some(init(cfg))` |
| `.github/workflows/cargo-audit.yml` | Create | Nightly cron job running `rustsec/audit-check@v2` |
| `plans/0026-gar-411-telemetry-part-2.md` | Create | This plan file |
| `plans/README.md` | Modify | Index entry 0026 |

**No migrations. No schema changes. No new Rust crates. No new Rust dependencies.**

## Design invariants

1. **Fail-soft preserved everywhere** — init() returning `Guard` instead of `Result<Guard>` means the caller can't distinguish "telemetry off by config" from "telemetry tried but failed". The internal `tracing::warn!` captures the failure for operators; the gateway continues to serve traffic.
2. **Idempotency preserved** — `INIT_ONCE` short-circuit from plan 0025 remains intact; second call still returns empty `Guard`.
3. **Zero behavior change for success path** — when `init_tracer` and `init_metrics` both succeed, the returned `Guard` is byte-identical to plan 0025.
4. **CI flake isolation** — `cargo-audit.yml` runs independently of CI; failure does not block PR merges. Advisory surface only.
5. **REDACT_HEADERS monotonic** — only additions, never removals (plan 0025 invariant maintained).
6. **Isolated test binary isolates state** — the new `idempotent_init_isolated.rs` runs in its own `cargo test` process with fresh `INIT_ONCE` and fresh Prometheus recorder, so the strong-RED scenario is guaranteed reachable regardless of test scheduler ordering.

## Tasks

### Task 1 — RED test is compilation-level

Since `init()` currently returns `Result<Guard, Error>` and we're removing the `Result`, the RED phase is "tests fail to compile after the signature change until we remove `.expect()` / `.map_err()`". No new test needed before the fix — the existing callers serve as the RED surface.

### Task 2 — GREEN: change `init()` signature in `lib.rs`

```rust
pub fn init(config: TelemetryConfig) -> Guard {
    if INIT_ONCE.get().is_some() {
        tracing::warn!(
            "garraia_telemetry::init called more than once; returning no-op guard (first call wins)"
        );
        return Guard {
            tracer_provider: None,
            metrics_handle: None,
        };
    }
    let tracer_provider = match tracer::init_tracer(&config) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "OTLP tracer init failed; continuing without tracing");
            None
        }
    };
    let metrics_handle = match metrics::init_metrics(&config) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "Prometheus recorder init failed; continuing without metrics");
            None
        }
    };
    let _ = INIT_ONCE.set(());
    Guard { tracer_provider, metrics_handle }
}
```

Update callers:
- `crates/garraia-cli/src/main.rs`: `let guard = Some(init(telemetry_config.clone()));`
- `crates/garraia-telemetry/tests/smoke.rs`: `let guard = init(TelemetryConfig::default());`
- `crates/garraia-telemetry/tests/idempotent_init.rs`: remove `.expect(...)` from all 5 call sites.

### Task 3 — GREEN: `REDACT_HEADERS` +4 IAP entries + test

```rust
pub const REDACT_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "proxy-authorization",
    "x-forwarded-authorization",
    "x-original-authorization",
    "x-amzn-trace-id",
    // Plan 0026 (GAR-411 SA-L-A) — cloud-LB IAP headers
    "x-goog-iap-jwt-assertion",
    "cf-access-jwt-assertion",
    "x-ms-client-principal",
    "x-forwarded-user",
];
```

Add test ensuring case-insensitive match works for all 4 new headers.

### Task 4 — GREEN: isolated-process test

Create `crates/garraia-telemetry/tests/idempotent_init_isolated.rs`:

```rust
//! Plan 0026 (GAR-411 SA-L-E): isolated-process regression test for the
//! strong-RED scenario from plan 0025. This file is a dedicated `[[test]]`
//! binary — its own process, its own `INIT_ONCE`, its own Prometheus
//! recorder global. That isolation guarantees the scenario exercises the
//! first-install path deterministically, regardless of scheduler ordering
//! in sibling test files.
//!
//! Single test only — the whole point is to reach the first `init()` call
//! with the recorder definitively un-installed.

use garraia_telemetry::{TelemetryConfig, init};

#[test]
fn first_init_installs_then_second_short_circuits_in_isolation() {
    let cfg = TelemetryConfig {
        metrics_enabled: true,
        ..TelemetryConfig::default()
    };

    let _g1 = init(cfg.clone());  // MUST install recorder in this isolated process
    let _g2 = init(cfg);          // MUST short-circuit (empty Guard)
    // If plan 0025's OnceLock short-circuit regressed, the second call
    // would either clobber the tracer or return an Err-path. With the
    // plan 0026 signature change, Err-path becomes a silent warn!, which
    // would mean we never actually caught the regression — but the
    // INIT_ONCE short-circuit is what actually prevents the double-install,
    // so we rely on its presence rather than on Result observability.
}
```

### Task 5 — GREEN: `cargo-audit` workflow

Create `.github/workflows/cargo-audit.yml` using `cargo install` + `cargo audit` (deliberate deviation from `rustsec/audit-check@v2` — see §Scope L5 for rationale). Hardening additions endereçando security audit F-2B + F-6 + NIT-2/4 do code review:

- `permissions: contents: read` explicit (least-privilege principle, OSSF Scorecard).
- `concurrency: group: cargo-audit` (fixed string, not `${{ github.ref }}`) + `cancel-in-progress: false` (scheduled runs complete instead of being canceled by manual dispatch).
- `--version` pin documenta exatamente por que existe (reduz drift no CLI de `cargo-audit` entre releases).

### Task 6 — DOCS + index

Update `plans/README.md` with plan 0026 row.

## Acceptance criteria

1. `cargo test -p garraia-telemetry` → all green (expect 15 unit + 3 integration + 1 isolated + 2 smoke = 21 tests).
2. `cargo clippy -p garraia-telemetry --all-targets -- -D warnings` — clean.
3. `cargo fmt -p garraia-telemetry --check` — clean.
4. `cargo check --workspace` — green.
5. `init()` signature is `pub fn init(config: TelemetryConfig) -> Guard`.
6. All 3 callers compile and behave identically on success path.
7. `REDACT_HEADERS` has 13 entries including the 4 new IAP headers.
8. `.github/workflows/cargo-audit.yml` is syntactically valid (no new workflow validation error on push).
9. Strong-RED scenario captured in isolated test file — runs as separate `cargo test` binary.

## Rollback plan

Reversível. Task-by-task:
- Task 2: revert signature → `Result<Guard, Error>`. Re-add `.expect()` / `.map_err()` to 3 callers.
- Task 3: revert REDACT_HEADERS. Zero behavioral effect on any consumer today (list is still pre-protective).
- Task 4: delete isolated test file. Redundant but harmless if kept.
- Task 5: delete workflow file. Zero effect on CI gating (scheduled job).

Zero migrations, zero schema, zero breaking API change *beyond* the internal `init()` signature (which only 3 files consume).

## Open questions

None. All design decisions are forced by prior review comments (plan 0025 CR + SA) or by the existing contract in CLI/tests.

## Review plan

- **Code review** (`code-reviewer` agent): focus on call-site migration correctness + workflow YAML validity + isolated test signal strength.
- **Security audit** (`security-auditor` agent): validate that the fail-soft error swallow in `init()` doesn't hide an attack vector (e.g. attacker-controlled `OTLP_ENDPOINT` that intentionally fails to force telemetry off — acceptable because attacker would need env var write access which = full compromise); validate IAP headers list completeness; validate cargo-audit workflow threat model.
- Dois agentes rodam **em paralelo**.
