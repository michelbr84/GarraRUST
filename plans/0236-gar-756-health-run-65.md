# Plan 0236 — GAR-756: Health Run 65 (2026-05-30 ~20:45 ET)

## Summary

Autonomous health & security run 65. All 4 security surfaces scanned — clean. Priority ladder
exhausted at **(i)**. Bookkeeping-only PR.

## Context

- **Run:** 65
- **Date:** 2026-05-30 ~20:45 ET (00:45 UTC 2026-05-31)
- **Branch:** `health/202605302045-run65-status-note`
- **Linear:** [GAR-756](https://linear.app/chatgpt25/issue/GAR-756)
- **Previous run:** GAR-754 (run 64) — PR #589 squash-merged as `fb9df70`, 2026-05-30
- **Main HEAD at scan time:** `fb9df70`

## Pre-flight: Open Branch Cleanup

- PR #588 (`bookkeeping/plan0234-gar753-merged`) closed as superseded — plans/README.md row 0234
  was already updated on main via PR #589 (health run 64 squash-merge). No code lost.
- Stale remote branches (`health/202605231000-gar513-deny-toml-hygiene`,
  `health/202605272045-run43-status-note`, `health/202605290045-run52-status-note`,
  `health/202605291649-run57-status-note`) — corresponding PRs already merged to main;
  branches are orphans, no action needed.

## Security Surface Scan

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI green on `fb9df70` (PR #589 squash-merge), Secret Scan job success |
| Malware (cargo/npm) | ✅ none | cargo-deny green on `fb9df70` |
| Dependabot alerts | ⚠️ upstream-blocked | rsa HIGH (GAR-456), glib MEDIUM + rand LOW (GAR-513), expiry 2026-07-31 |
| Open Dependabot PRs | ⚠️ 1 active, 0 security | #577 (benches PoC — astral-tokio-tar 0.6.1→0.6.2, non-workspace, deferred) |
| Security Audit (CI) | ✅ pass | 0 vulnerabilities, 19 unmaintained warnings (all deny.toml, unchanged) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 suppressed, expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on `fb9df70` (PR #589) |
| CI on main (`fb9df70`) | ✅ green | 20/20 CI checks confirmed green — PR #589 squash-merge |

## Priority Decision

**(i) — no actionable security item.**

Priority ladder exhausted. No critical/high advisories without suppressions, no CI failures on
main, no code scanning alerts without existing mitigations.

## Open Security Backlog (unchanged)

| Issue | Package | Severity | CVE/Rule | Suppression |
|---|---|---|---|---|
| GAR-456 | rsa | HIGH | RUSTSEC-2023-0071 | audit.toml, expiry 2026-07-31 |
| GAR-513 | glib | MEDIUM | RUSTSEC-2024-0429 | audit.toml, expiry 2026-07-31 |
| GAR-513 | rand | LOW | RUSTSEC-2026-0097 | audit.toml, expiry 2026-07-31 |
| GAR-711 | opentelemetry_sdk | — | OTel 0.26→0.32 upgrade | Backlog (9/20 CI failing on PR #515) |
| GAR-491 | CodeQL ledger | — | Re-audit due | 2026-08-01 |

## What Changed

- `plans/0236-gar-756-health-run-65.md`: this file
- `docs/security/dependabot-status.md`: run 65 status note added
- `plans/README.md`: row 0235 updated ✅ Merged (PR #589 / `fb9df70`) + row 0236 added

## Acceptance Criteria

- [x] PR #588 closed as superseded (row 0234 already in main via PR #589)
- [ ] All CI checks pass on health/202605302045-run65-status-note (≥19 non-legacy checks green)
- [ ] No secrets in diff (docs-only change)
- [ ] `docs/security/dependabot-status.md` updated with run 65 status note
- [ ] `plans/README.md` row 0235 → ✅ Merged (PR #589 / `fb9df70`) + row 0236 added
- [ ] Linear GAR-756 → Done
