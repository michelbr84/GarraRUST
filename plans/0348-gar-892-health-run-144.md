# Plan 0348 — GAR-892: Health run 144 (2026-06-15 ~12:45 ET) — 4 non-security Dependabot PRs all CI-green, priority (i)

> **Status:** In Progress  
> **Linear:** [GAR-892](https://linear.app/chatgpt25/issue/GAR-892)  
> **Branch:** `health/202606151645-run144-status-note`  
> **Run:** 144 — 2026-06-15 ~12:45 ET / 2026-06-15T16:45 UTC  

## Goal

Autonomous health & security routine run 144. Four non-security Dependabot PRs are open
and fully CI-green (20/20 checks each). No CVE-tagged alerts found. Priority **(i)** —
file status note and exit cleanly.

## Scan results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `f622d9c` (2026-06-15T13:28Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success — advisories ok, bans ok, licenses ok, sources ok |
| Dependabot PRs | ⚠️ 4 non-security open | #781, #782, #783, #784 — all 20/20 CI-green, no CVEs |
| Dependabot security alerts | ⚠️ 3 open, all upstream-blocked | rsa (RUSTSEC-2023-0071/GAR-456), glib (RUSTSEC-2024-0429/GAR-513), rand (RUSTSEC-2026-0097/GAR-513) — expiry 2026-07-31. No `first_patched_version` for any. |
| Security Audit (cargo-audit) | ✅ pass | CI success on main `f622d9c` (2026-06-15T13:28Z); 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + 18 unmaintained IDs suppressed in deny.toml |
| CodeQL | ✅ pass | Analyze (rust) + (javascript-typescript) + (actions) success on main `f622d9c` |
| Quality Ratchet | ✅ pass | CI success on main `f622d9c` |
| CI on main (`f622d9c`) | ✅ green | All workflow checks success (2026-06-15T13:28Z) |
| Workflow failures (last 7d) | ⚠️ 1 Dependabot internal run | Run #27555840008 "cargo in /. - Update #1413810699" failed — Dependabot's own update process, not our code |
| Mutation Testing | ⚠️ expected / not a gate | Run #27541142471 failed — mutation testing workflow, not a CI gate |

**Open PRs noted:** PR #779 (`routine/202606151222-gar890-delete-group`, GAR-890) — NOT touched (routine/ prefix).

## Priority decision

All priority tiers (a) through (h) evaluated — no actionable security work:

- (a) Secret scanning: ✅ none
- (b) Malware: ✅ none
- (c) Critical Dependabot with patch: ✅ none with `first_patched_version`
- (d) High Dependabot with patch: ✅ none (rsa/glib/rand all upstream-blocked)
- (e/f) CodeQL critical/high: ✅ none
- (g) CI failure last 24h: ✅ none in our CI gates
- (h) Medium Dependabot with patch: 4 PRs open but no CVE tags → maintenance only
- **(i) → file status note and exit cleanly**

## Dependabot PR analysis (non-security, safe to merge when ready)

| PR | Package | Version change | CI | Notes |
|---|---|---|---|---|
| #781 | @playwright/test | 1.60.0 → 1.61.0 | 20/20 ✅ | Dev dep only, no breaking changes |
| #782 | patch-and-minor group | uuid+chrono+regex+aws-sdk-s3+aws-smithy-types | 20/20 ✅ | All patch/minor, no breaking changes |
| #783 | lopdf | 0.40.0 → 0.41.0 | 20/20 ✅ | Minor — fixes text replacement bug + drops nom_locate |
| #784 | tower-http | 0.6.11 → 0.7.0 | 20/20 ✅ | MAJOR bump; breaking: ServeDir Backend trait + SizeAbove u16→u64. Use-sites: only `ServeDir::new()` + `CorsLayer` — unaffected. Confirmed by clippy + build + MSRV all passing. |

## Deliverables

- [x] Linear issue GAR-892 filed (In Progress)
- [x] `plans/0348-gar-892-health-run-144.md` — this file
- [x] `plans/README.md` — 0348 row added
- [x] `docs/security/dependabot-status.md` — run 144 section prepended

## Next security backlog

- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31)
- glib RUSTSEC-2024-0429 (GAR-513, expiry 2026-07-31)
- rand RUSTSEC-2026-0097 (GAR-513, expiry 2026-07-31)
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)
- Dependabot PRs #781–#784 all CI-green and ready to merge
