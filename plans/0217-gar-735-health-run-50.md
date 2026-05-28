# Plan 0217 — GAR-735: Health run 50 (2026-05-28 ~16:45 ET) — all surfaces clean, priority (i)

## Goal

Autonomous health & security routine — run 50. Scan all 4 security surfaces, rank by
priority ladder, act on the highest finding, or file a status note if nothing actionable.

## Result

Priority ladder exhausted at **(i)** — no actionable security work found.

## Actions Taken

### 1. Pending health/ PR resolved this run

PR #560 (`health/202605281645-run49-status-note`, GAR-734, plan 0216) had a merge conflict
in `plans/README.md` (row 0214 added by health branch vs. main lacking it after PR #555
merged). Conflict resolved locally, push to origin, CI passed 20/20, squash-merged as
`96fb68b`.

### 2. Routine/ PRs noted (NOT actioned — routine/ territory)

- PR #561 (`routine/202605281240-search-slice14-groups`, GAR-733) — open, 20/20 CI green,
  skipped per protocol.
- PR #556 (`routine/202605280630-search-slice13-users`, GAR-730) — open, skipped per
  protocol.

## Scan Summary

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #560, Secret Scan job success |
| Malware (cargo-deny) | ✅ none | cargo-deny green on PR #560 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — expiry 2026-07-31 |
| Open Dependabot PRs | ⚠️ 8 open, none security | #513 #515 #516 #517 #518 #519 #520 #522 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) + RUSTSEC-2024-0429 (glib) + RUSTSEC-2026-0097 (rand) suppressed, expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #560 |
| CI on main (`036578c`) | ✅ green | PR #555 GAR-728 docs bookkeeping merged, 20/20 via PR #560 CI |

## Priority Ladder

| Priority | Check | Result |
|---|---|---|
| (a) | Secret scanning active/unverified | ✅ none |
| (b) | Malware advisory | ✅ none |
| (c) | Critical Dependabot + patched | ✅ none |
| (d) | High Dependabot + patched | ⚠️ rsa HIGH — but UPSTREAM-BLOCKED (no first_patched_version in cargo graph; GAR-456 tracks) |
| (e) | Critical CodeQL | ✅ none |
| (f) | High CodeQL | ✅ none |
| (g) | CI failure on main (last 24h) | ✅ none — CI green |
| (h) | Medium alerts, low blast radius | ⚠️ glib MEDIUM — UPSTREAM-BLOCKED (GAR-513) |
| **(i)** | **None of above → status note** | **→ selected** |

## Bookkeeping

- Plan 0216 (GAR-734, run 49) merged via PR #560 → squash-merged as `96fb68b`
- Plan **0217** (this run, GAR-735) created
- `plans/README.md` — row 0216 → ✅ Merged, row 0217 added
- `docs/security/dependabot-status.md` — run 50 entry prepended
- GAR-734 marked Done in Linear
- GAR-735 created + marked In Progress

## Security Backlog (unchanged)

- **GAR-711** OpenTelemetry 0.26→0.32 (Backlog) — async-std unmaintained RUSTSEC-2025-0052;
  8 open Dependabot PRs (#513 #515 #516 #517 #518 #519 #520 #522) cover this upgrade but
  need coordinated work
- **GAR-456** rsa RUSTSEC-2023-0071 HIGH — upstream sqlx transitively pulls rsa; blocked on
  sqlx dropping the dep. Suppression expiry 2026-07-31
- **GAR-513** glib RUSTSEC-2024-0429 MEDIUM + rand RUSTSEC-2026-0097 LOW — expiry 2026-07-31
- **GAR-491** CodeQL ledger re-audit due 2026-08-01

## Acceptance Criteria

- [x] PR #560 (GAR-734 run 49) squash-merged as `96fb68b`, CI 20/20 green
- [x] Plan 0217 committed
- [x] plans/README.md updated
- [x] docs/security/dependabot-status.md updated
- [x] GAR-734 Done, GAR-735 In Progress → Done on merge

## References

- GAR-735: https://linear.app/chatgpt25/issue/GAR-735
- Previous run plan: plans/0216-gar-734-health-run-49.md
- docs/security/dependabot-status.md
- docs/security/codeql-suppressions.md
