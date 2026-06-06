# Plan 0268 — GAR-805: Health run 85 status note

**Date:** 2026-06-06 ~12:47 ET
**Linear:** [GAR-805](https://linear.app/chatgpt25/issue/GAR-805)
**Branch:** `health/202606061247-run85-status-note`
**Priority ladder result:** (i) — no actionable alerts

---

## Goal

Record the outcome of autonomous health & security routine run 85. All surfaces clean; no fix work required.

## Housekeeping (pre-scan merges)

No open `health/` PRs to complete. No `routine/` PRs touched (roadmap routine territory):
- `routine/202506051820-get-thread` — skip
- `routine/202506060630-get-task-label` — skip

## Security Scan

| Surface | Evidence | Status |
|---------|----------|--------|
| Secret scanning | No open alerts on GitHub Secret Scanning | ✅ clean |
| Malware (cargo/npm graph) | cargo-deny CI check on main → success; no malware advisories | ✅ clean |
| Dependabot alerts | GitHub push revealed Dependabot alert #42 (moderate) on main — likely glib RUSTSEC-2024-0429 (GAR-513, MEDIUM, allowlisted expiry 2026-07-31) or related GHSA entry. cargo audit log: 0 vulnerabilities, 17 unmaintained warnings all allowlisted. HIGH `rsa` RUSTSEC-2023-0071 / GAR-456 also allowlisted. `rand` RUSTSEC-2026-0097 fully closed (run 82 / GAR-789). | ⚠️ alert #42 observed — tracked via GAR-513 |
| CodeQL — Analyze (rust) | CI on main → success; 0 open code-scanning alerts | ✅ clean |
| CodeQL — Analyze (javascript-typescript) | CI on main → success | ✅ clean |
| Security Audit (cargo audit) | Security Audit CI on main 2026-06-06T09:26 UTC → success | ✅ clean |
| Workflow runs on main (last 24h) | All 20 CI checks green on main; latest run 2026-06-06T13:40 UTC — all success | ✅ no failures |

## Priority Ladder

| Priority | Alert type | Finding |
|----------|-----------|---------|
| (a) | Secret scanning active/unverified | ❌ none |
| (b) | Malware advisory | ❌ none |
| (c) | Critical Dependabot + patch available | ❌ none |
| (d) | High Dependabot + patch available | ❌ none (rsa UPSTREAM-BLOCKED, no patch, expiry 2026-07-31) |
| (h) | Medium alert, low blast radius | ⚠️ Dependabot #42 (moderate) observed post-scan — matches known glib GAR-513 allowlist (expiry 2026-07-31); cargo audit clean; no first_patched_version available; treated as ongoing carve-out |
| (e) | Critical CodeQL with clear fix | ❌ none |
| (f) | High CodeQL with clear fix | ❌ none |
| (g) | Workflow failure on main (last 24h) | ❌ main is green (20/20) |
| (h) | Medium alert, low blast radius | ❌ none |
| **(i)** | **No actionable alerts → status note + exit** | ✅ **picked** |

## Acceptance Criteria

- [x] All 4 security surfaces scanned, clean
- [x] Linear GAR-805 filed
- [x] This plan committed to `health/202606061247-run85-status-note`
- [ ] PR opened, CI green, squash-merged to main
- [ ] GAR-805 marked Done

## Out of scope

Active implementation work — nothing to fix this run.

## Rollback

Status-note-only PR. Rollback = revert the doc commit. No code changed.

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| New alert appears between scan and merge | Low | CI re-runs Secret Scan + cargo-deny + CodeQL on this PR too |

## Cross-references

- Previous run: plan 0268 n/a — GAR-804 (run 84, ~08:45 ET Jun 6, all surfaces clean, merged PR #653 `e1488d1`)
- Deferred Dependabot: GAR-456 (`rsa` Marvin Attack), GAR-513 (`glib` unsoundness, Tauri-only)
- `rand` RUSTSEC-2026-0097 closed: health run 82 / GAR-789 / plan 0262 (2026-06-05)

## Estimativa

~15 min total (housekeeping + scan + status note).
