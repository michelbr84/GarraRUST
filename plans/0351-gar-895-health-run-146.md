# Plan 0351 — GAR-895: Health run 146 (2026-06-16 ~00:45 ET) — RUSTSEC-2026-0182 active (wasmtime-wasi 45.0.0), fix in routine PR #787, priority (i)

> **Status:** In Progress
> **Linear:** [GAR-895](https://linear.app/chatgpt25/issue/GAR-895)
> **Branch:** `health/202606160445-run146-status-note`
> **Run:** 146 — 2026-06-16 ~00:45 ET / 2026-06-16T04:45 UTC

## Goal

Autonomous health & security routine run 146. RUSTSEC-2026-0182 (wasmtime-wasi 45.0.0,
GHSA-3p27-qvp9-27qf — WASIp1 `fd_renumber` fd leak) appeared in the RustSec DB between
health run 145 (~20:45 ET Jun 15) and this run, blocking all 4 open Dependabot PRs.
Routine PR #787 (`routine/202606160045-sqlx-feature-compat`) already implements the fix
(wasmtime 45.0.0 → 45.0.2 + sqlx feature compat, GAR-894). Per guardrail, health routine
must not touch `routine/` branches. Priority **(i)** — file status note and exit cleanly.

## Scan results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI Secret Scan success on all open PRs |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo or npm graph |
| Code scanning (CodeQL) | ✅ clean | Analyze (rust/js-ts/actions) all success on main + PRs |
| Dependabot security alerts | ⚠️ 1 active RUSTSEC | RUSTSEC-2026-0182 (wasmtime-wasi 45.0.0, GHSA-3p27-qvp9-27qf) — fd_renumber fd leak |
| Dependabot PRs | ⚠️ 4 open, CI failing | #781–#784 all failing cargo-deny + Security Audit due to RUSTSEC-2026-0182 |
| Workflow failures (main, 24h) | ✅ none | Last main CI run `d12478e` (2026-06-15T21:09 UTC) = all-success |
| CI on main (last run) | ✅ green | CI run 27576586129 success (2026-06-15T21:09Z, before advisory published) |
| Quality Ratchet | ✅ pass | CI success on main `d12478e` (2026-06-15T21:09Z) |

## Active vulnerability: RUSTSEC-2026-0182

**Advisory:** RUSTSEC-2026-0182 / GHSA-3p27-qvp9-27qf — "Leak in WASIp1 `fd_renumber` implementation"
**Affected crate:** wasmtime-wasi 45.0.0 (current in Cargo.lock, pulled via `garraia-plugins`)
**Patched versions:** >=45.0.2 OR >=44.0.3,<45 OR >=36.0.11,<37 OR >=24.0.10,<25
**Fix:** `cargo update -p wasmtime --precise 45.0.2` (entire wasmtime+cranelift family: 45.0.0 → 45.0.2)
**Published:** ~2026-06-15 (RustSec DB updated after health run 145 completed at 2026-06-16T00:45 UTC)

**Cargo-deny output:**
```
error[vulnerability]: Leak in WASIp1 `fd_renumber` implementation
   ┌─ /github/workspace/Cargo.lock:903:1
903 │ wasmtime-wasi 45.0.0 registry+...
    │ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━ security vulnerability detected
   ├ ID: RUSTSEC-2026-0182
   ├ wasmtime-wasi v45.0.0 └── garraia-plugins └── garraia
advisories FAILED, bans ok, licenses ok, sources ok
```

## Why health routine is not implementing the fix

Routine PR #787 (`routine/202606160045-sqlx-feature-compat`) already implements the fix:

- **Cargo.lock**: wasmtime+cranelift family 45.0.0 → 45.0.2 (via `cargo update -p wasmtime --precise 45.0.2`)
- **Cargo.toml**: Splits deprecated `runtime-tokio-native-tls` → `runtime-tokio` + `tls-native-tls` (GAR-894 sqlx compat)
- **CI status on PR #787 at run time:** Security Audit ✅, cargo-deny ✅, Format ✅, Clippy ✅, Coverage ✅, Analyze (rust/js-ts/actions) ✅, Dependency Review ✅, Quality Ratchet ✅, MSRV ✅, Test (macos) ✅ — remaining jobs (ubuntu/windows/Build/E2E/Playwright/Secret Scan) in-progress

Per guardrail: **health routine MUST NOT touch `routine/` branches**. Creating a parallel fix on a `health/` branch would conflict with PR #787 on Cargo.lock.

## Dependabot PR status

All 4 open Dependabot PRs are failing `cargo-deny` + `Security Audit` because their base
(main) has wasmtime-wasi 45.0.0 and the advisory appeared after the last successful main CI
run. None of these PRs introduce or remove wasmtime; the failure is inherited from the base.

| PR | Package | Version change | cargo-deny | Security Audit | Notes |
|---|---|---|---|---|---|
| #781 | @playwright/test | 1.60.0 → 1.61.0 | ❌ | ❌ | npm-only change; Cargo.lock unchanged |
| #782 | patch-and-minor group | uuid/chrono/regex/aws-sdk-s3/aws-smithy-types | ❌ | ❌ | All patch/minor Rust bumps; no wasmtime change |
| #783 | lopdf | 0.40.0 → 0.41.0 | ❌ | ❌ | Minor bump; fixes text-replace bug, drops nom_locate |
| #784 | tower-http | 0.6.11 → 0.7.0 | ❌ | ❌ | Major bump; verified safe in run 144 analysis |

Once PR #787 merges to main, Dependabot will auto-rebase these PRs and their CI should
pass the security gates.

## Priority decision

- (a) Secret scanning: ✅ none
- (b) Malware: ✅ none
- (c) Critical Dependabot with patch: RUSTSEC-2026-0182 IS the active vuln — fix already in routine PR #787 (health routine cannot duplicate)
- (d) High Dependabot with patch: same
- (e/f) CodeQL critical/high: ✅ none
- (g) CI failure last 24h on main: ✅ none (last CI run was success)
- (h) Medium with patch: 4 blocked Dependabot PRs — blocked by (c)/(d) fix in routine PR
- **(i) → file status note and exit cleanly**

## Deliverables

- [x] Linear issue GAR-895 filed (In Progress)
- [x] `plans/0351-gar-895-health-run-146.md` — this file
- [x] `plans/README.md` — 0349 marked merged, 0351 row added
- [x] `docs/security/dependabot-status.md` — run 146 section prepended

## Next security backlog

- **RUSTSEC-2026-0182** (wasmtime-wasi 45.0.0): Being fixed by routine PR #787 — verify merge in next health run
- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31)
- glib RUSTSEC-2024-0429 (GAR-513, expiry 2026-07-31)
- rand RUSTSEC-2026-0097 (GAR-513, expiry 2026-07-31 — note: rand 0.7.3 may still be in some lockfiles)
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)
- Dependabot PRs #781–#784: will unblock after PR #787 merges; next health run should merge #782 (patch-and-minor) and evaluate #784 (tower-http major)
