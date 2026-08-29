# CodeQL Setup Runbook

> Status: established 2026-04-30 (PR C of the Green Security Baseline
> sprint, plan `personal-api-key-revogada-vectorized-matsumoto` §Step 2).
> Linear: GAR-XXX umbrella, sub-issue 3.
> Scope: how GarraRUST runs CodeQL static analysis, why we use advanced
> setup, and the one-time toggle procedure required to migrate from
> GitHub-native default setup.

## Background

Until 2026-04-30, GarraRUST relied on **GitHub-native default setup** for
CodeQL. Default setup is convenient — no workflow file, GitHub manages
language detection, autobuild, and scheduling — but it has two
dealbreakers for this repo:

1. **Autobuild fails on `crates/garraia-desktop`** (Tauri). The crate's
   `build.rs` depends on the WebView2 SDK on Windows and GTK/glib on
   Linux. GitHub-hosted runners don't have these by default. The
   "Code scanning configuration error" banner in the Security tab
   tracked back to this autobuild failure, not to a real analysis
   problem.
2. **No path-level exclusion control.** Default setup scans the entire
   workspace. Excluding the desktop crate, the bench PoC, and the
   Playwright E2E fixtures is not configurable through the UI.

Advanced setup — a checked-in `.github/workflows/codeql.yml` plus
`.github/codeql-config.yml` — solves both via two complementary
mechanisms:

1. **`build-mode: none` (buildless extraction).** CodeQL for Rust does
   NOT support `build-mode: manual` (verified empirically against run
   `25176031230` on the first attempt of this workflow: `"Rust does not
   support the manual build mode. Please try using one of the following
   build modes instead: none."`). Buildless Rust extraction means we do
   NOT need to run `cargo build` — the analyzer reads source files
   directly. This eliminates the autobuild surface that broke default
   setup.
2. **Explicit `paths-ignore` in the config file** so analysis still
   excludes `crates/garraia-desktop`, `apps/garraia-mobile`, `benches`,
   and `tests/playwright` — same exclusion list `ci.yml` /
   `cargo-audit.yml` / `mutants.yml` already use.

## What this PR adds

- **`.github/workflows/codeql.yml`** — advanced workflow. Two language
  jobs (`rust` and `javascript-typescript`, both with `build-mode: none`)
  on `ubuntu-latest`, triggered on push/PR to `main` + weekly Monday
  09:00 UTC schedule. Action versions match `ci.yml`:
  `actions/checkout@v6`, `github/codeql-action/init@v3` +
  `analyze@v3`. (No Rust toolchain install needed — buildless
  extraction reads sources directly.)
- **`.github/codeql-config.yml`** — `paths-ignore` for
  `crates/garraia-desktop/**`, `apps/garraia-mobile/**`, `benches/**`,
  `tests/playwright/**`. Mirrors the exclusion set used by `ci.yml`,
  `cargo-audit.yml`, and `mutants.yml`.
- **This runbook**.

## One-time toggle: disable default setup BEFORE merging this PR

GitHub does not allow advanced setup and default setup to coexist
silently. If both are active, SARIF uploads collide under the same
category and one of them errors out.

**Procedure** (must be done in GitHub UI; gh API supports the same
endpoint but the user should explicitly authorize this destructive
toggle):

1. Open `https://github.com/michelbr84/GarraRUST/settings/security_analysis`.
2. Scroll to **Code scanning** → **Default setup**.
3. Click **Disable**. Confirm.
4. Verify state via API:
   ```bash
   gh api repos/michelbr84/GarraRUST/code-scanning/default-setup \
     --jq '.state'
   # expected: not-configured
   ```
5. Merge this PR. The new workflow runs on the merge commit and on
   every subsequent push/PR to `main`.

**API alternative** (only if the user explicitly authorizes the gh CLI
to disable default setup):

```bash
gh api -X PATCH repos/michelbr84/GarraRUST/code-scanning/default-setup \
  -f state=not-configured
```

This is reversible: if advanced setup misbehaves, default setup can be
re-enabled the same way.

## Why these specific exclusions

| Path | Why excluded |
|---|---|
| `crates/garraia-desktop/**` | Tauri. Build requires WebView2 / GTK absent from GitHub-hosted runners. Already excluded from `ci.yml`, `cargo-audit.yml`, `mutants.yml`. Local-only build via `scripts/build-installer.ps1`. |
| `apps/garraia-mobile/**` | Flutter. CodeQL JS/TS would only see Dart-generated artifacts, which are out of scope. |
| `benches/**` | PoC bench harness, ephemeral per CLAUDE.md. Has its own `[workspace]` and would confuse CodeQL build resolution. |
| `tests/playwright/**` | Playwright TypeScript fixtures — scanned by their own runner. CodeQL JS/TS focuses on admin UI source. |

## What we did NOT change

- The 90 existing CodeQL alerts are NOT triaged in this PR. Triage waves
  are tracked separately as Linear sub-issues `GAR-490` (Wave 1, production
  paths: ~16 path-injection in `skills_handler.rs`/`skins_handler.rs`,
  8 sql-injection in `groups.rs`/`invites.rs`) and `GAR-491` (Wave 2, test
  fixtures + suppression convention).
- **AMENDMENT 2026-05-01 (GAR-491):** suppression mechanism for Rust CodeQL
  alerts has now been decided. Rust CodeQL still does NOT support inline
  `// codeql[...]` comments (PR github/codeql#21638 is open without merge).
  The chosen mechanism is **REST API dismissal + a versioned ledger** —
  see [`docs/security/codeql-suppressions.md`](codeql-suppressions.md) for
  the human-readable ledger and
  [`docs/security/codeql-suppressions.json`](codeql-suppressions.json) for
  the machine-readable source consumed by
  [`scripts/security/codeql-reapply-dismissals.sh`](../../scripts/security/codeql-reapply-dismissals.sh).
  Wave 2 (`GAR-491`) entrega a convenção + script + 6 dismissals
  individualmente justificados; a empirical proof (persistência do
  dismissal de `credentials.rs:49` entre re-análises CodeQL) é o gate
  obrigatório antes do batch dos 5 restantes. **Sem fallback global**:
  se a prova falhar, abort + nova decisão (sem `query-filters: exclude`
  por rule-id).
- The `query_suite` defaults to `default` (was the same in default
  setup). Switching to `extended` or `security-extended` is a separate
  decision that surfaces more alerts; not appropriate while we still
  have 90 unresolved.

## Verification after merge

```bash
# 1. Workflow ran at least once
gh run list --workflow=codeql.yml --limit 3

# 2. CodeQL analyses succeeded with no error string
gh api repos/michelbr84/GarraRUST/code-scanning/analyses \
  --jq '.[0:3] | .[] | {ref, tool: .tool.name, error, results_count}'

# 3. "Code scanning configuration error" banner no longer appears in
#    the GitHub Security tab.

# 4. Default setup is off
gh api repos/michelbr84/GarraRUST/code-scanning/default-setup \
  --jq '.state'
# expected: not-configured
```

## Triage planning (next sessions)

`GAR-XXX.4` and `GAR-XXX.5` carry the actual alert resolution. Wave 1
prioritizes production code paths; Wave 2 covers test fixtures and
locks in the suppression convention. Both reference the alert numbers
captured in the Security tab and avoid bulk-dismissal anti-patterns.

## A onda de 2026-08-28: extractor Rust e escopo de teste

Em 2026-08-28 o Security tab passou a mostrar **56 alertas Critical** de uma vez,
sem nenhuma mudança em `codeql.yml`, em `codeql-config.yml` ou de código de
segurança na janela.

**Causa:** o bundle do CodeQL nos runners subiu de **2.26.3 para 2.26.4**, e isso
destravou o extractor de Rust. Comparando os logs do job `Analyze (rust)` do
mesmo workflow, na mesma branch:

| Run (`main`) | CodeQL | Extraídos com erro | Extraídos sem erro |
|---|---|---:|---:|
| [`32170604886`](https://github.com/michelbr84/GarraRUST/actions/runs/32170604886) — 2026-08-18T18:22Z | 2.26.3 | 303 | 118 |
| [`33233416706`](https://github.com/michelbr84/GarraRUST/actions/runs/33233416706) — 2026-08-29T04:16Z | 2.26.4 | 3 | 422 |

O log novo diz `CodeQL scanned 425 out of 425 Rust files`. A cobertura saiu de
~28% para ~99% — 3,6× mais código analisado. Os alertas não foram regressão: era
código que já existia e que o CodeQL passou a enxergar.

### Por que quase tudo era código de teste

204 dos 434 arquivos `.rs` de `crates/` têm um módulo `#[cfg(test)]` **inline**,
e o extractor Rust liga `cfg(test)` incondicionalmente — no fonte do
`github/codeql`, `rust/extractor/src/config.rs::to_cfg_overrides` faz
`enabled_cfgs.insert(to_cfg_override("test"))` sem condição. Todo fixture de
teste (senha literal, salt, chave de HMAC) virou sink analisável de uma vez.

Das 78 ocorrências de `rust/hard-coded-cryptographic-value` (security-severity
**9.8**, Critical) no workspace, **69 eram código de teste**: 59 em `#[cfg(test)]`
inline e 10 em `crates/*/tests/`.

### A correção: escopo, não supressão

`.github/workflows/codeql.yml`, no **nível do job** `analyze`:

```yaml
jobs:
  analyze:
    env:
      CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES: "-test"
```

> **O nível importa, e essa foi a primeira tentativa errada.** Colocar a env no
> step `Initialize CodeQL` **não funciona**. Com `build-mode: none` a extração
> acontece dentro de `Perform CodeQL analysis` — 811 s no run
> [`33233416706`](https://github.com/michelbr84/GarraRUST/actions/runs/33233416706),
> contra 10 s do init — então uma env no escopo do init nunca chega ao
> extractor. O sintoma foi um alerta apontando para um `assert!` dentro de um
> `#[cfg(test)]` no run
> [`33239057983`](https://github.com/michelbr84/GarraRUST/actions/runs/33239057983),
> com o log mostrando 339 arquivos extraídos: os 85 de `crates/*/tests/` tinham
> sumido (o `paths-ignore` funcionou) mas os módulos inline continuavam lá.

Opção oficial, documentada em `rust/codeql-extractor.yml` do `github/codeql`:
*"Comma-separated list of cfg settings to enable, or disable if prefixed with
`-`."* Complementada por `crates/*/tests/**` no `paths-ignore` do
`codeql-config.yml`, que cobre os testes de integração em arquivos próprios (o
`-test` sozinho não os remove, porque não são gated por `cfg(test)`).

Isto **não** é supressão: não toca em `codeql-suppressions.json`, não faz
`PATCH state=dismissed`, e fica versionado e revisável no diff. Resolve
exatamente o bloqueio registrado em
[`codeql-suppressions.md`](codeql-suppressions.md) §1 em 2026-05 — *"paths-ignore
silencia arquivo inteiro; os testes do GarraRUST são INLINE dentro de produção"*.

Helpers atrás de `#[cfg(feature = "test-helpers")]` **continuam** analisados: são
feature, não `cfg(test)`.

### Como validar em runs futuros

No log do job `Analyze (rust)`, comparar contra a baseline de
`33233416706` (425/425, 3 com erro):

```text
CodeQL scanned N out of M Rust files
| Total number of Rust files that were extracted with errors   |  ... |
| Total number of Rust files that were extracted without error |  ... |
```

Se a cobertura de **produção** cair, reverter — o objetivo é remover teste, não
perder alcance.

## Triagem programática

`GET /repos/{owner}/{repo}/code-scanning/alerts` exige o escopo
`security_events`, que tokens de PAT/integração normalmente não têm — foi
exatamente o que impediu diagnosticar a onda de 2026-08-28 pela API. O
`GITHUB_TOKEN` de um job com `permissions: security-events: read` tem.

[`.github/workflows/codeql-triage.yml`](../../.github/workflows/codeql-triage.yml)
(`workflow_dispatch`) roda
[`scripts/security/codeql-alert-report.py`](../../scripts/security/codeql-alert-report.py)
e publica:

- no **job summary**, uma tabela `rule_id | severidade | total | produção | teste`;
- como **artifact**, `codeql-alerts.json` (lista crua) e `codeql-alerts.md`.

A classificação produção-vs-teste é heurística — caminho de teste, ou linha do
alerta dentro de um `#[cfg(test)]` — e serve para dimensionar uma onda, não para
dispensar triagem individual.

Localmente, com um token que tenha o escopo:

```bash
GITHUB_TOKEN=... python3 scripts/security/codeql-alert-report.py \
    --repo michelbr84/GarraRUST --severity critical --state open
```

## See also

- `.github/workflows/codeql.yml` — workflow definition.
- `.github/codeql-config.yml` — paths-ignore.
- `.github/workflows/ci.yml` — source of the matching exclusions
  (`--exclude garraia-desktop`).
- `docs/security/secret-scanning-runbook.md` — companion runbook for
  the secret-scanning side of the security baseline.
- `docs/security/threat-model.md` — overall security model.
