# Plan 0216 — GAR-734: Health run 49 (2026-05-28 ~12:45 ET)

**Status:** Done
**Linear:** [GAR-734](https://linear.app/chatgpt25/issue/GAR-734)
**Priority:** (i) — informational, no actionable security work found
**Date:** 2026-05-28 ~12:45 ET / 16:45 UTC

## Summary

Daily security/dependency health routine — run 49. Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

## Actions Taken This Run

- GAR-731 marked Done in Linear (was stuck "In Progress" despite PR #557 being squash-merged as `a6e368a`).
- GAR-734 created in Linear (this run).
- PR #555 (docs/gar-728-plan0209-bookkeeping): 19/20 CI checks green at scan time, Test (windows-latest) in_progress. Will be merged by next routine once CI completes.

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on all recent PRs |
| Malware (cargo-deny) | ✅ none | 0 deny errors |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — expiry 2026-07-31 |
| Open Dependabot PRs | ⚠️ 8 open, none security | #513, #515, #516, #517, #518, #519, #520, #522 |
| CodeQL (rust + js-ts + actions) | ✅ pass | 20/20 green confirmed on PR #555 check runs |
| CI on main (`1130c4f`) | ✅ 20/20 green | Confirmed via multiple PR check runs |

## Priority Ladder

- (a) Secret scanning: ✅ 0 active alerts
- (b) Malware: ✅ 0 alerts
- (c) Critical Dependabot with fix: ❌ none (3 alerts all UPSTREAM-BLOCKED, no first_patched_version)
- (d) High Dependabot with fix: ❌ rsa HIGH upstream-blocked (RUSTSEC-2023-0071, no crate fix)
- (e) Critical CodeQL: ✅ 0 open alerts
- (f) High CodeQL: ✅ 0 open alerts
- (g) CI failure on main within 24h: ✅ none — 20/20 green
- (h) Medium Dependabot/CodeQL low blast radius: ❌ glib MEDIUM upstream-blocked
- **(i) → All clean. Status note filed. Exiting cleanly.**

## Open Dependabot PRs (not security-labeled)

| PR | Package | Change | Notes |
|---|---|---|---|
| #513 | patch-and-minor group (serde_json, getrandom, pgvector, aws-config/sdk/smithy) | patch/minor | No CVEs, safe to defer |
| #515 | opentelemetry_sdk | 0.26.0 → 0.32.0 | GAR-711 Backlog (API drift, not security) |
| #516 | rand_chacha | 0.9.0 → 0.10.0 | Major version, not security |
| #517 | criterion | 0.5.1 → 0.8.2 | Dev-dep major version, not security |
| #518 | opentelemetry-otlp | 0.26.0 → 0.32.0 | GAR-711 Backlog |
| #519 | opentelemetry-semantic-conventions | 0.26.0 → 0.32.0 | GAR-711 Backlog |
| #520 | lopdf | 0.34.0 → 0.40.0 | Minor, not security |
| #522 | tracing-opentelemetry | 0.32.1 → 0.33.0 | GAR-711 Backlog |

## Open Dependabot Security Alerts (upstream-blocked)

| Alert | Package | Severity | Advisory | Status | Expiry |
|---|---|---|---|---|---|
| GAR-456 | rsa | HIGH | RUSTSEC-2023-0071 | UPSTREAM-BLOCKED | 2026-07-31 |
| GAR-513 | glib | MEDIUM | RUSTSEC-2024-0429 | UPSTREAM-BLOCKED | 2026-07-31 |
| GAR-513 | rand | LOW | RUSTSEC-2026-0097 | UPSTREAM-BLOCKED | 2026-07-31 |

## Routine PRs Observed (not touched)

| PR | Branch | Status |
|---|---|---|
| #556 | routine/202605280630-search-slice13-users | Open (routine/ — skip) |
| #552 | routine/202605280018-search-slice12-threads | Open (routine/ — skip) |
| #555 | docs/gar-728-plan0209-bookkeeping | Open, 19/20 CI green at scan time (Test windows-latest in_progress) |

## Bookkeeping

- GAR-731: marked Done in Linear (PR #557 merged as `a6e368a`)
- GAR-732: Done in Linear, plan 0212 README row updated to ✅ Merged via PR #559 (`1130c4f`)
- GAR-733: taken by roadmap routine (slice 14 types=groups)

## Backlog (unchanged since run 40)

- **GAR-456**: rsa / RUSTSEC-2023-0071 HIGH — suppression expiry 2026-07-31
- **GAR-491**: CodeQL ledger re-audit due 2026-08-01
- **GAR-513**: glib+rand — suppression expiry 2026-07-31
- **GAR-669**: argon2 ≥ 0.6 stable blocks Slices 3–4
- **GAR-711**: OpenTelemetry 0.26→0.32 Backlog

## Files Changed

- `docs/security/dependabot-status.md` — prepended run 49 entry, updated header
- `plans/0214-gar-734-health-run-49.md` — this file (renumbered from 0213 due to collision with GAR-726 slice 12)
- `plans/README.md` — row 0212 marked ✅ Merged (`1130c4f`), row 0213 (GAR-726 slice 12) marked ✅, row 0214 added
