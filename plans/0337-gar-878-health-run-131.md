# Plan 0337 — GAR-878: Health Run 131 (2026-06-14 ~00:48 ET) — All Surfaces Clean, Priority (i)

## Goal

Record the health & security routine run 131 status note. All security surfaces scanned; no actionable items found. Priority ladder exhausted at (i).

## Architecture

Doc-only change — no code, no schema, no deps.

## Tech Stack

- Plans: Markdown tracking files
- Linear: GAR-878 (In Progress → Done)

## Design Invariants

- Plan number 0337 (sequential after 0336; 0335 is claimed by routine/ PR #757 for GAR-876)
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
plans/0337-gar-878-health-run-131.md      ← this file
plans/README.md                            ← add row 0337
docs/security/dependabot-status.md        ← update header + add run 131 section
```

## M1: Status Note Tasks

- [x] Create plan file
- [x] Update plans/README.md
- [x] Update dependabot-status.md
- [x] Create branch health/202606140448-run131-status-note
- [ ] Push + open PR
- [ ] CI green → merge
- [ ] Mark GAR-878 Done

## Security Scan Results

**CI run:** SHA `e6862c2`, 2026-06-14T01:54Z — all 15 jobs success.
**Result: 0 vulnerabilities · 0 unsound · 18 allowed unmaintained warnings**

All 18 unmaintained-crate warnings are pre-tracked in `deny.toml` with documented owners and expiry dates. No new advisories since run 130.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI Secret Scan job success on main `e6862c2` (2026-06-14T01:54Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success — advisories ok, bans ok, licenses ok, sources ok |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot security alerts | ⚠️ 1 moderate (RUSTSEC-2023-0071), allowlisted | rsa 0.9.10 — Marvin Attack. HS256-only invariant holds. Allowlisted expiry 2026-07-31. |
| Security Audit (cargo-audit) | ✅ pass | CI success on main `e6862c2`; 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | All 18 unmaintained IDs suppressed in deny.toml; RUSTSEC-2023-0071 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + (javascript-typescript) + (actions) success on main `e6862c2` (2026-06-14T01:54Z) |
| Quality Ratchet | ✅ pass | CI success on main `e6862c2` |
| CI on main (`e6862c2`) | ✅ green | All 15 workflow jobs success (2026-06-14T01:54Z) |
| Workflow failures (last 7d) | ✅ none | No failures in 20 consecutive runs |
| Open health/ PRs | ✅ none | No pending health/ branches |

## Open PRs

- PR #757 (`routine/202606140015-patch-me-password`): roadmap routine for GAR-876. NOT touched by health routine.

## Risk Register

| Risk | Mitigation |
|------|-----------|
| Merge conflict in plans/README.md with PR #757 | Rebase onto main if needed |

## Acceptance Criteria

- PR merged to main with green CI
- GAR-878 marked Done
- plans/README.md row 0337 shows commit SHA + PR number

## Cross-references

- GAR-877 (health run 130, 2026-06-14 ~00:45 ET) — PR #758 merged
- GAR-875 (health run 129, 2026-06-13 ~20:45 ET) — Linear-only status note (no PR)
- GAR-513 (glib/rsa carve-out owner, expiry 2026-07-31)
- GAR-491 (CodeQL triage, re-audit due 2026-08-01)

## Estimativa

< 10 min doc-only
