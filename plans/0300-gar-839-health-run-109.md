# Plan 0300 — GAR-839: Health Run 109 (2026-06-10 ~04:45 ET)

**Status:** Done
**Linear:** GAR-839
**Branch:** `health/202606100445-run109-status-note`
**Previous run:** GAR-838 / plan 0299 (run 108, ~00:45 ET 2026-06-10)

---

## Summary

Autonomous health & security routine — run 109.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- PR #710 (`health/202606100045-run108-status-note`): squash-merged as `8495527` — health run 108 status note / GAR-838. All 20 CI checks green before merge.
- PR #709 (`routine/202606100020-doc-pages-single-crud`) open with routine/ prefix — skipped per protocol.

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `8495527` (2026-06-10T01:35Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | 18 allowed warnings (unmaintained), 0 vulnerabilities |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + 18 unmaintained suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (js-ts) + Analyze (actions) all success |
| CI on main | ✅ green | All 15 CI jobs success on `8495527` (2026-06-10T01:32Z) |

## Priority Decision

**(i)** — No critical, high, or medium actionable alerts. All known moderate alerts are allowlisted with rationale and expiry dates. No CI failures on main. No open health/ PRs remaining.

## Next Security Backlog

- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31) — no `first_patched_version` available upstream
- RUSTSEC-2024-0429 glib (GAR-513, expiry 2026-07-31) — audit.toml-only residual
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)

## Acceptance Criteria

- [x] Status note filed in Linear (GAR-839)
- [x] `docs/security/dependabot-status.md` updated with run 109 results
- [x] `plans/README.md` row added for plan 0300
- [x] PR merged to main with green CI
