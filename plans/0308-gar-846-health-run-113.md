# Plan 0308 — GAR-846: Health Run 113 Status Note

**Date:** 2026-06-10 ~20:45 ET (Florida)
**Run:** 113
**Priority reached:** (i) — no actionable security work found
**Branch:** `health/202606102049-health-run-113`

---

## Goal

Document the security health scan for run 113 and confirm all surfaces are clean.

## Actions Taken

### Completed from previous run
- **PR #716** (`health/202606101651-gar844-garra-routine-trigger-retry`) was open with all 20 CI checks green and `mergeable_state: clean`.
- Squash-merged as `3f33d5ab9eb16020ce237436e1895ec858806077` — fix(ci): GAR-844 garra-routine-trigger retry on transient 401.

### Skipped (not this routine's territory)
- **PR #717** (`routine/202606101815-doc-page-versions`) — roadmap routine branch, not touched per hard rules.

## Security Surfaces Scanned

| Surface | Method | Result |
|---------|--------|--------|
| Secret Scan (gitleaks) | CI job `Secret Scan (gitleaks)` on run 27296689846 | ✅ Clean |
| Malware / cargo audit | CI job `Security Audit` + nightly `Security — cargo audit` (run 27271395722) | ✅ 0 vulnerabilities; 18 allowed unmaintained warnings |
| Dependabot / cargo deny | CI job `cargo-deny` on run 27296689846 | ✅ All advisories suppressed in deny.toml |
| Code scanning (CodeQL) | `Analyze (rust)` + `Analyze (javascript-typescript)` on CodeQL run 27296689832 | ✅ Both passing |
| CI main (all jobs) | Run 27296689846 (15 jobs on sha 343597a) + 3f33d5ab post-merge | ✅ All green |

## Advisory Snapshot

### Active ignores in audit.toml

| Advisory | Crate | Kind | Owner | Expiry |
|----------|-------|------|-------|--------|
| RUSTSEC-2023-0071 | rsa 0.9.10 | Marvin Attack timing sidechannel | GAR-456 | 2026-07-31 |
| RUSTSEC-2024-0429 | glib 0.18.5 | VariantStrIter unsound | GAR-513 | 2026-07-31 |

### Notable unmaintained warnings (deny.toml, no audit.toml entry needed)

| Advisory | Crate | Owner | Expiry |
|----------|-------|-------|--------|
| RUSTSEC-2026-0173 | proc-macro-error2 2.0.1 | GAR-817 | 2026-07-31 |
| RUSTSEC-2024-0370 | proc-macro-error 1.0.4 | GAR-817 | 2026-07-31 |
| RUSTSEC-2024-0388 | derivative | GAR-430 | 2026-07-31 |
| RUSTSEC-2024-041[1-20] | gtk-rs × 10 | GAR-430 | 2026-07-31 |
| RUSTSEC-2025-007[5,80,81,98,100] | unic-* × 5 | GAR-430 | 2026-07-31 |

## Priority Ladder Result

- (a) Secret scanning — no active alerts
- (b) Malware — clean
- (c) Critical Dependabot — none
- (d) High Dependabot — none
- (e) Critical CodeQL — none
- (f) High CodeQL — none
- (g) CI failures on main (last 24h) — only GAR-844 transient 401 (resolved via PR #716, merged this run)
- (h) Medium alerts — none
- **(i) ← reached** — filing status note, exiting cleanly

## Acceptance Criteria

- [x] GAR-846 filed in Linear (team GAR, label epic:sec-harden, priority Low)
- [x] PR #716 squash-merged (GAR-844 fix)
- [x] This plan file committed and pushed
- [x] plans/README.md updated with row for plan 0308

## Cross-References

- GAR-844 (PR #716): fix(ci) garra-routine-trigger retry
- GAR-845 (PR #717): routine/ branch — doc-pages versions (not touched)
- GAR-843 (plan 0305, PR #715): previous health run 112
- GAR-456: rsa deferred until 2026-07-31
- GAR-513: glib deferred until 2026-07-31
- GAR-817: proc-macro-error2 deferred until 2026-07-31
