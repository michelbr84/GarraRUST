# Plan 0297 — GAR-836: Health Run 107 (2026-06-09 ~08:47 PM ET)

**Status:** Done
**Linear:** GAR-836
**Branch:** `health/202606092047-run107-status-note`
**Previous run:** GAR-833 / plan 0296 (run 106, ~12:45 ET 2026-06-09)

---

## Summary

Autonomous health & security routine — run 107.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- PR #706 (`routine/202506091815-docs-tier2-doc-pages`) open with routine/ prefix — skipped per protocol
- No open health/ PRs at scan time

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `d05a217` (2026-06-09T18:17Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | CI Security — cargo audit success |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (js-ts) all success |
| CI on main | ✅ green | All 15 CI jobs success on `d05a217` (2026-06-09T18:17Z) |

## Priority Decision

**(i)** — No critical, high, or medium actionable alerts. All known moderate alerts are allowlisted with rationale and expiry dates. No CI failures on main. No open health/ PRs to complete.

## Next Security Backlog

- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31) — no `first_patched_version` available upstream
- RUSTSEC-2024-0429 glib (GAR-513, expiry 2026-07-31) — audit.toml-only residual
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)

## Acceptance Criteria

- [x] Status note filed in Linear (GAR-836)
- [x] `docs/security/dependabot-status.md` updated with run 107 results
- [x] `plans/README.md` row added for plan 0297
- [x] PR merged to main with green CI
