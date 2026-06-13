# Plan 0329 — GAR-868: Health Run 125 (2026-06-13 ~04:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Record the health & security routine run 125 status note. All 4 security surfaces scanned; no actionable items found. Priority ladder exhausted at (i).

## Architecture

No code changes. Status note only. Housekeeping: resolved conflict in `tracking/202606130046-mark-0325-done` (PR #743) and updated `claude/festive-bell-r8l7jr` (PR #741).

## Tech stack

N/A — documentation only.

## Design invariants

- Never expose secret values in plan files or commit messages.
- Status notes are forward-only; no revert of main.

## Out of scope

- Any code change, migration, or feature work.
- Merging PR #742 (`routine/` prefix — roadmap routine territory).

## Rollback

N/A — doc-only commit. Revert the commit if incorrect.

## §12 Open questions

None.

## File structure

```
plans/0327-gar-868-health-run-125.md   ← this file
plans/README.md                         ← add row 0327
docs/security/dependabot-status.md     ← update run 125 header
```

## M1 Tasks

- [x] Create Linear issue GAR-868
- [x] Resolve conflict in PR #743 (tracking/202606130046-mark-0325-done)
- [x] Update PR #741 (claude/festive-bell-r8l7jr) via GitHub branch update API
- [x] Write plan file plans/0327-gar-868-health-run-125.md
- [x] Update plans/README.md with plan 0327 row
- [x] Update docs/security/dependabot-status.md run 125 entry
- [ ] Commit + push health/202606130445-run125-status-note
- [ ] Open PR, wait for CI green
- [ ] Squash-merge PR
- [ ] Mark GAR-868 Done in Linear

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| RUSTSEC-2023-0071 expiry 2026-07-31 | Low | Medium | Allowlisted with hard expiry; monitor monthly |
| CVE-2026-49975 (h2/hyper) | Unknown | High | Monitor RustSec feed; run 126 will re-check |

## Acceptance criteria

- Plan 0327 row in plans/README.md marked ✅ Merged with PR number and commit SHA.
- GAR-868 marked Done in Linear.
- dependabot-status.md updated with run 125 header.

## Cross-references

- Previous run: [GAR-867 (run 124)](0326-gar-867-health-run-124.md) — PR #744 merged as `76d6808`
- Housekeeping PR: #743 (tracking/202606130046-mark-0325-done) — conflict resolved
- Housekeeping PR: #741 (claude/festive-bell-r8l7jr) — branch updated

## Estimativa

< 30 min — doc-only, no compilation.
