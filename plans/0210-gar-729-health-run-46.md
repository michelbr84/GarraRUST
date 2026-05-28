# Plan 0210 — GAR-729: Health Run 46 (2026-05-28 ~00:45 ET) — all surfaces clean, priority (i)

## Goal

Bookkeeping PR for health & security routine run 46. No actionable security work found — priority ladder exhausted at (i).

## Architecture

Docs-only changes:
- `plans/0210-gar-729-health-run-46.md` — this file
- `plans/README.md` — row 0209 marked Merged + row 0210 added
- `docs/security/dependabot-status.md` — run 46 section prepended

## Tech stack

N/A (docs-only)

## Design invariants

- Never expose secret values in commit messages, PR bodies, or logs
- Security backlog items only referenced by GAR number
- health/ branch prefix maintained (never routine/)

## Out of scope

- Any security fix (none actionable this run)
- Any code change

## Rollback

Revert the 3 docs files. No code change to roll back.

## §12 Open questions

None.

## File Structure

```
plans/
  0210-gar-729-health-run-46.md   ← this file (new)
  README.md                        ← row 0209 → Merged + row 0210 added
docs/security/
  dependabot-status.md             ← run 46 section prepended
```

## M1 Tasks

- [x] T1: Create plan file (this file)
- [x] T2: Update plans/README.md
- [x] T3: Update docs/security/dependabot-status.md
- [x] T4: Commit + push on health/202605280500-run46-status-note
- [ ] T5: Open PR, await CI green, merge
- [ ] T6: Mark GAR-729 Done in Linear

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| CI flake | Low | Low | Re-run if needed |
| Routine PR #552 naming collision (plan 0209) | Medium | Low | Routine will rebase; not this routine's concern |

## Acceptance criteria

- PR opens with head=health/202605280500-run46-status-note, base=main
- All CI checks green
- Squash-merged to main
- GAR-729 Done in Linear

## Cross-references

- Previous run: GAR-728 / plan 0209 / PR #553 (`c573a3e`)
- Security backlog: GAR-456 (rsa HIGH, expiry 2026-07-31), GAR-513 (glib+rand, expiry 2026-07-31), GAR-491 (CodeQL ledger), GAR-669 (argon2), GAR-711 (OTel)
- Routine PR skipped: PR #552 `routine/202605280018-search-slice12-threads` (GAR-726) — open, behind main, naming collision with plan 0209
- Dependabot PRs open (8, none security): #513, #515, #516, #517, #518, #519, #520, #522

## Estimativa

~5 min (docs-only).
