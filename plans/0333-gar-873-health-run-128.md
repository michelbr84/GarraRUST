# Plan 0333 — GAR-873: Health Run 128 (2026-06-13 ~16:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Record the health & security routine run 128 status note. All security surfaces scanned; no actionable items found. Priority ladder exhausted at (i).

## Architecture

Doc-only change — no code, no schema, no deps.

## Tech Stack

- Plans: Markdown tracking files
- Linear: GAR-873 (In Progress → Done)

## Design Invariants

- Plan number 0333 (sequential after 0332)
- Branch prefix `health/` (never `routine/`)
- No secrets, no code changes

## Out of Scope

- Any code or schema changes
- Bumping suppression expiry dates (GAR-513 owns that, expiry 2026-07-31)

## Rollback

Delete branch + close PR. No persistent state changes.

## §12 Open Questions

None.

## File Structure

```
plans/0333-gar-873-health-run-128.md      ← this file
plans/README.md                            ← mark 0332 done, add row 0333
docs/security/dependabot-status.md        ← update header + add run 128 section
```

## M1: Status Note Tasks

- [x] Create plan file
- [x] Update plans/README.md
- [x] Update dependabot-status.md
- [x] Create branch health/202606131645-run128-status-note
- [ ] Push + open PR
- [ ] CI green → merge
- [ ] Mark GAR-873 Done

## Security Scan Results

**Cargo.lock crates scanned:** 1,073
**Advisory DB entries loaded:** 1,131
**Result: 0 vulnerabilities · 0 unsound · 18 allowed unmaintained warnings**

All 18 unmaintained-crate warnings are pre-tracked in `deny.toml` with documented owners and expiry dates. No new advisories since run 127.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `bca5a36` (2026-06-13T17:04Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success — advisories ok, bans ok, licenses ok, sources ok |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot security alerts | ⚠️ 1 moderate (RUSTSEC-2023-0071), allowlisted | rsa 0.9.10 — Marvin Attack. HS256-only invariant holds. Allowlisted expiry 2026-07-31. |
| Security Audit (cargo-audit) | ✅ pass | CI success on main `bca5a36`; 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | All 18 unmaintained IDs suppressed in deny.toml; RUSTSEC-2023-0071 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + (javascript-typescript) + (actions) success on main `bca5a36` (2026-06-13T17:04Z) |
| Quality Ratchet | ✅ pass | CI success on main `bca5a36` |
| CI on main (`bca5a36`) | ✅ green | All workflow checks success (2026-06-13T17:04Z) |
| Workflow failures (last 7d) | ✅ none | No failures in last 7 days |

## Risk Register

| Risk | Mitigation |
|------|-----------|
| Merge conflict in plans/README.md | Rebase onto main if needed |

## Acceptance Criteria

- PR merged to main with green CI
- GAR-873 marked Done
- plans/README.md row 0333 shows commit SHA + PR number

## Cross-references

- GAR-872 (previous health run 127) — PR #752 (`bca5a36`)
- GAR-513 (glib/rsa carve-out owner, expiry 2026-07-31)
- GAR-491 (CodeQL triage, re-audit due 2026-08-01)

## Estimativa

< 10 min doc-only
