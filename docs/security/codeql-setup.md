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
`.github/codeql-config.yml` — solves both: we run our own
`cargo build --workspace --exclude garraia-desktop --locked` (the same
build line `ci.yml` already uses), and we set explicit `paths-ignore`
in the config file.

## What this PR adds

- **`.github/workflows/codeql.yml`** — advanced workflow. Two language
  jobs (`rust` with `build-mode: manual`, `javascript-typescript` with
  `build-mode: none`) on `ubuntu-latest`, triggered on push/PR to `main`
  + weekly Monday 09:00 UTC schedule. Action versions match `ci.yml`:
  `actions/checkout@v6`, `dtolnay/rust-toolchain@1.92`,
  `github/codeql-action/init@v3` + `analyze@v3`.
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
  are tracked separately as Linear sub-issues `GAR-XXX.4` (production
  paths: ~24 path-injection in `skills_handler.rs`/`skins_handler.rs`,
  ~8 sql-injection in `groups.rs`/`invites.rs`) and `GAR-XXX.5` (test
  fixtures + suppression convention).
- Suppression syntax for Rust CodeQL alerts is **not yet decided**.
  Unlike Java/JS/Python, CodeQL for Rust does not currently support
  inline `// codeql[...]` comments. Wave 2 (`GAR-XXX.5`) will validate
  the supported mechanism (path-based ignore vs custom query suite vs
  manual UI dismissal with justification) before any suppression is
  applied.
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

## See also

- `.github/workflows/codeql.yml` — workflow definition.
- `.github/codeql-config.yml` — paths-ignore.
- `.github/workflows/ci.yml` — source of the matching exclusions
  (`--exclude garraia-desktop`).
- `docs/security/secret-scanning-runbook.md` — companion runbook for
  the secret-scanning side of the security baseline.
- `docs/security/threat-model.md` — overall security model.
