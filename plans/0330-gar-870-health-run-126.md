# Plan 0330 — GAR-870: Health Run 126 (2026-06-13 ~08:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Record the health & security routine run 126 status note. All 4 security surfaces scanned; no actionable items found. Priority ladder exhausted at (i). Housekeeping: rebased and merged PR #746 (GAR-868, run 125 status note) which had a merge conflict (plans/README.md — plans 0327+0328 landed between branch creation and now; plan renumbered 0327→0329).

## Architecture

Doc-only change — no code, no schema, no deps.

## Tech Stack

- Plans: Markdown tracking files
- Linear: GAR-870 (In Progress → Done)

## Design Invariants

- Plan number 0330 (sequential after 0329 introduced by PR #746)
- Branch prefix `health/` (never `routine/`)
- No secrets, no code changes

## Out of Scope

- Any code or schema changes
- Bumping suppression expiry dates (GAR-513 owns that, expiry 2026-07-31)

## Rollback

Delete branch + close PR. No persistent state changes.

## Open Questions

None.

## File Structure

```
plans/0330-gar-870-health-run-126.md   ← this file (new)
plans/README.md                         ← add row 0330
docs/security/dependabot-status.md     ← prepend run 126 section
```

## Tasks

- [x] M1: Create plan 0330 and branch health/202606130845-run126-status-note
- [x] M2: Update plans/README.md with row 0330
- [x] M3: Update docs/security/dependabot-status.md with run 126 section
- [ ] M4: Merge PR #746 (GAR-868 run 125 status note, rebased)
- [ ] M5: Open PR #NNN for this plan (base=main)
- [ ] M6: CI green → squash-merge
- [ ] M7: Mark GAR-870 Done in Linear

## Risk Register

| Risk | Mitigation |
|---|---|
| Another merge conflict in README.md | Rebase onto updated main before push |

## Acceptance Criteria

- PR merged, all 20 CI checks green
- plans/README.md row 0330 present on main
- docs/security/dependabot-status.md run 126 section at top
- GAR-870 Done in Linear

## Cross-References

- GAR-868: run 125 (PR #746, rebased + merged this run)
- GAR-513: RUSTSEC-2023-0071 + RUSTSEC-2024-0429 allowlist (expiry 2026-07-31)
- GAR-491: CodeQL ledger re-audit due 2026-08-01

## Estimativa

~5 min (doc-only, CI ~20 min)
