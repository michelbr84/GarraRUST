# Plan 0184 — GAR-702: Health run 28 (2026-05-25) — all surfaces clean, priority (i)

**Linear:** [GAR-702](https://linear.app/chatgpt25/issue/GAR-702)
**Branch:** `health/202605250845-run28-status-note`
**Date:** 2026-05-25 (~10:25 ET)
**Status:** In Progress

---

## Context

Autonomous health & security routine — run 28. Picks up immediately after PR #503
(`health/202605250710-run27-status-note`, GAR-701) squash-merged as `ba8482b`.

## Scan results

### Priority ladder

| Level | Check | Result |
|---|---|---|
| (a) | Secret exposed in repo | ✅ none |
| (b) | Malware / supply-chain | ✅ none |
| (c) | Critical Dependabot (CVSS ≥ 9.0) | ✅ none actionable |
| (d) | High Dependabot (CVSS 7–9) | ⚠️ 1 open — rsa HIGH (GAR-456 Done, suppressed to 2026-07-31) |
| (e) | Critical CodeQL | ✅ none |
| (f) | High CodeQL | ✅ none |
| (g) | CI failure on main | ✅ green (20/20 on `ba8482b`) |
| (h) | Medium severity alerts | ⚠️ 1 open — glib MEDIUM (GAR-513, suppressed to 2026-07-31) |
| (i) | All clear | ✅ **SELECTED — status note only** |

**Decision: priority (i) — all actionable surfaces clean. File status note and exit.**

### Surface scan

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #503 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) — all suppressed until 2026-07-31 |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #503 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #503 |
| CI on main (`ba8482b`) | ✅ green | All 20 checks passed |

### Open PRs checked

- PR #502 (`routine/202605251124-message-attachments-api`, GAR-700): roadmap routine territory — skipped per protocol.
- No open `health/` PRs pending at scan time.

### Suppressed alerts (unchanged from runs 25–27)

| Alert | Severity | Crate | Reason | Expiry |
|---|---|---|---|---|
| rsa (RUSTSEC-2023-0071) | HIGH | rsa 0.9.x | Upstream-blocked (GAR-456 Done); deny.toml suppression | 2026-07-31 |
| glib (GHSA) | MEDIUM | glib | Upstream-blocked (GAR-513) | 2026-07-31 |
| rand (GHSA) | LOW | rand | Upstream-blocked (GAR-513) | 2026-07-31 |

### argon2 upstream

Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

## Implementation

Status note only — no code changes. Bookkeeping PR updating:

- `plans/0184-gar-702-health-run-28.md` (this file)
- `plans/README.md` — row 0183 marked ✅ Merged, row 0184 added
- `docs/security/dependabot-status.md` — run 28 section prepended

## Next security backlog

- argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4
- rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)
