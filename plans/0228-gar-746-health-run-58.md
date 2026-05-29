# Plan 0228 — GAR-746: Health Run 58 (2026-05-29 ~20:46 ET)

## Summary

Autonomous health & security run 58. All 4 security surfaces scanned — clean. Priority ladder exhausted at **(i)**. Bookkeeping-only PR.

## Context

- **Run:** 58
- **Date:** 2026-05-29 ~20:46 ET (Florida time)
- **Branch:** `health/202605300046-run58-status-note`
- **Linear:** [GAR-746](https://linear.app/chatgpt25/issue/GAR-746)
- **Previous run:** GAR-744 (run 57, ~12:49 ET 2026-05-29), docs merged via PR #574 as `3fa24d3`

## Pending Health PRs Resolved

- **PR #573** (`health/202605291649-run57-status-note`, GAR-744) — closed as superseded.
  PR #574 squash-merged `3fa24d3` already included the run 57 docs (plan 0226). PR #573 would have
  created a conflicting `plans/0225-gar-744-health-run-57.md` alongside the existing
  `plans/0225-gar-740-chat-threads-list.md`.

## Security Surface Scan

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on `3fa24d3`, Secret Scan job success |
| Malware (cargo/npm) | ✅ none | cargo-deny green on `3fa24d3` |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 4 open, none security | #513 (patch-and-minor), #515 (otel_sdk 0.32), #519 (otel-semantic-conventions 0.32), #522 (tracing-opentelemetry 0.33) |
| cargo audit (CI) | ✅ pass | 0 vulnerabilities, 19 unmaintained warnings (all deny.toml, unchanged since run 40) |
| cargo-deny | ✅ pass (CI) | RUSTSEC-2023-0071 (rsa) + RUSTSEC-2024-0429 (glib) + RUSTSEC-2026-0097 (rand) suppressed, expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on `3fa24d3` |
| CI on main (`3fa24d3`) | ✅ green | 20/20 checks confirmed (per PR #574 merge commit message) |

## CI on Main (`3fa24d3`) Detail

Commit `3fa24d3` ("chore: branch audit final — GAR-740 threads endpoint + health run 57 docs #574")
was squash-merged after PR #574 CI passed (20/20 green). The preceding commit `9586108`
("fix(deps): resolve Cargo.lock corruption + deprecations + MSRV violations from branch audit")
explicitly states "All 20 CI checks pass (MSRV 1.93, Clippy, Format, Build, Tests ×3, Coverage,
E2E, Playwright, cargo-deny, Security Audit, Dependency Review, Quality Ratchet, CodeQL ×3)."

## cargo audit Detail

CI (cargo-audit, Security Audit job): exit 0. **0 vulnerabilities.** 19 unmaintained warnings —
all pre-existing, all in deny.toml ignore list (GTK3/Tauri cluster, OTel async-std, Discord poise,
Tauri unic-*, Tauri fxhash). No new advisories since run 57.

## Open Dependabot PRs at Scan Time

| PR | Crate | Bump | CVE? | Notes |
|---|---|---|---|---|
| #513 | patch-and-minor group (serde_json, getrandom, pgvector, aws-*) | patch/minor | No | pgvector 0.4.2 previously blocked (pulls sqlx 0.9.0, MSRV 1.94) |
| #515 | opentelemetry_sdk | 0.26→0.32.1 | No | Major jump — previously reverted (GAR-711 Backlog) |
| #519 | opentelemetry-semantic-conventions | 0.26→0.32 | No | Major jump — OTel ecosystem, same as #515 |
| #522 | tracing-opentelemetry | 0.32.1→0.33.0 | No | Minor jump; [breaking] deadlock fix — OTel ecosystem |

None of the 4 open Dependabot PRs carry CVEs. All are routine version bumps blocked by compatibility
constraints (MSRV or OTel ecosystem coherence).

## Priority Ladder

```
(a) active leaked secret     → none
(b) malware advisory         → none
(c) critical Dependabot CVE  → none
(d) high Dependabot CVE      → rsa HIGH (GAR-456) upstream-blocked, deny.toml expiry 2026-07-31
(e) critical CodeQL          → none
(f) high CodeQL              → none
(g) CI failure on main       → none (20/20 green on `3fa24d3`)
(h) medium low-blast-radius  → none actionable
(i) STATUS NOTE ONLY         → ← selected
```

## Action Taken

Bookkeeping-only PR:
1. Closed PR #573 as superseded (comment + closed via GitHub API)
2. Created `plans/0228-gar-746-health-run-58.md` (this file)
3. Updated `plans/README.md` — rows 0225 + 0226 → ✅ Merged (`3fa24d3`), row 0228 added
4. Updated `docs/security/dependabot-status.md` — run 58 section prepended
5. Filed Linear GAR-746

## Security Backlog (unchanged)

- **GAR-456** — rsa/RUSTSEC-2023-0071 HIGH — upstream-blocked, suppression expiry 2026-07-31
- **GAR-513** — glib/RUSTSEC-2024-0429 MEDIUM + rand/RUSTSEC-2026-0097 LOW — upstream-blocked, expiry 2026-07-31
- **GAR-491** — CodeQL ledger re-audit due 2026-08-01
- **GAR-711** — OpenTelemetry 0.26→0.32 Backlog (blocked by MSRV/ecosystem coherence)

## Acceptance Criteria

- [x] All 4 security surfaces scanned and documented
- [x] PR #573 closed (superseded)
- [x] GAR-746 filed in Linear
- [ ] PR opened on `health/202605300046-run58-status-note`, CI green (20/20)
- [ ] Squash-merged to main
- [ ] GAR-746 → Done in Linear
