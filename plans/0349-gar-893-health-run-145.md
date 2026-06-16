# Plan 0349 — GAR-893: Health run 145 (2026-06-15 ~20:45 ET) — all surfaces clean, sqlx compat noted, priority (i)

> **Status:** In Progress
> **Linear:** [GAR-893](https://linear.app/chatgpt25/issue/GAR-893)
> **Branch:** `health/202606160045-run145-status-note`
> **Run:** 145 — 2026-06-15 ~20:45 ET / 2026-06-16T00:45 UTC

## Goal

Autonomous health & security routine run 145. All security surfaces clean. Notable: Dependabot
failed to auto-create a sqlx PR because workspace `Cargo.toml` uses `runtime-tokio-native-tls`
(removed in sqlx 0.9); current sqlx 0.8 is unaffected (CI green, no RUSTSEC). Priority **(i)** —
file status note and exit cleanly.

## Scan results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `d67563d` (2026-06-15T18:32Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success — advisories ok, bans ok, licenses ok, sources ok |
| Dependabot PRs | ⚠️ 4 non-security open | #781, #782, #783, #784 — all 20/20 CI-green, no CVEs (same as run 144) |
| Dependabot security alerts | ⚠️ 3 open, all upstream-blocked | rsa (RUSTSEC-2023-0071/GAR-456), glib (RUSTSEC-2024-0429/GAR-513), rand (RUSTSEC-2026-0097/GAR-513) — expiry 2026-07-31. No `first_patched_version` for any. |
| Security Audit (cargo-audit) | ✅ pass | Ran 2026-06-15T13:15Z on main; 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + 18 unmaintained IDs suppressed in deny.toml |
| CodeQL | ✅ pass | Analyze (rust) + (javascript-typescript) + (actions) success on main `d67563d` |
| Quality Ratchet | ✅ pass | CI success on main `d67563d` (2026-06-15T18:32Z) |
| CI on main (`d67563d`) | ✅ green | All workflow checks success (2026-06-15T18:32Z) |
| Workflow failures (last 7d) | ⚠️ 2 non-gate | Run #27555840008 "cargo in /. - Update" — Dependabot sqlx compat error (not our CI gate); Run #27541142471 Mutation Testing pilot — expected |

**Open PRs noted:** PR #779 (`routine/…`, GAR-890) — NOT touched (routine/ prefix).

## Notable observation — sqlx 0.9 compat (tech debt)

Dependabot run #27555840008 failed with:

```
package `garraia` depends on `sqlx` with feature `runtime-tokio-native-tls`
but `sqlx` does not have that feature [in 0.9.0].
available features: runtime-tokio, tls-native-tls, tls-rustls, ...
```

Root cause: `Cargo.toml` (workspace root) declares `sqlx = { version = "0.8", features = ["runtime-tokio-native-tls", ...] }`. The combined feature was removed in sqlx 0.9; the replacement is `runtime-tokio` + `tls-native-tls`. Note: `garraia-workspace/Cargo.toml` already uses the correct `runtime-tokio` feature.

**Impact today:** None — sqlx 0.8.x still ships `runtime-tokio-native-tls` as a compat alias (CI is green). cargo-audit reports 0 vulnerabilities for sqlx 0.8.

**Risk:** If a RUSTSEC advisory appears for sqlx 0.8, Dependabot cannot auto-create the fix PR. The workspace-root feature list will need to be split before sqlx can be upgraded.

**Tracking:** Not filing a separate GAR issue this cycle (no active CVE). Noted here for the next cycle that picks up sqlx.

## Priority decision

All priority tiers (a) through (h) evaluated — no actionable security work:

- (a) Secret scanning: ✅ none
- (b) Malware: ✅ none
- (c) Critical Dependabot with patch: ✅ none with `first_patched_version`
- (d) High Dependabot with patch: ✅ none (rsa/glib/rand all upstream-blocked)
- (e/f) CodeQL critical/high: ✅ none
- (g) CI failure last 24h: ✅ none in our CI gates (main CI green)
- (h) Medium Dependabot with patch: 4 PRs open but no CVE tags → maintenance only
- **(i) → file status note and exit cleanly**

## Dependabot PR analysis (non-security, unchanged from run 144)

| PR | Package | Version change | CI | Notes |
|---|---|---|---|---|
| #781 | @playwright/test | 1.60.0 → 1.61.0 | 20/20 ✅ | Dev dep only, no breaking changes |
| #782 | patch-and-minor group | uuid+chrono+regex+aws-sdk-s3+aws-smithy-types | 20/20 ✅ | All patch/minor, no breaking changes |
| #783 | lopdf | 0.40.0 → 0.41.0 | 20/20 ✅ | Minor — fixes text replacement bug, drops nom_locate |
| #784 | tower-http | 0.6.11 → 0.7.0 | 20/20 ✅ | MAJOR bump; verified safe per run 144 analysis |

## Deliverables

- [x] Linear issue GAR-893 filed (In Progress)
- [x] `plans/0349-gar-893-health-run-145.md` — this file
- [x] `plans/README.md` — 0349 row added
- [x] `docs/security/dependabot-status.md` — run 145 section prepended

## Next security backlog

- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31)
- glib RUSTSEC-2024-0429 (GAR-513, expiry 2026-07-31)
- rand RUSTSEC-2026-0097 (GAR-513, expiry 2026-07-31)
- sqlx `runtime-tokio-native-tls` compat (workspace root) — blocks future Dependabot sqlx updates (no RUSTSEC today)
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)
- Dependabot PRs #781–#784 all CI-green and ready to merge
