# Plan 0282 — GAR-819: Health Run 95 (2026-06-08 ~00:45 ET)

## Goal

Autonomous health & security scan run 95. Priority **(i)** — all surfaces clean. Housekeeping: mark plan 0281 ✅ Merged in README and update `docs/security/dependabot-status.md`.

## Architecture

Single-commit PR: status note docs only. No code changes.

## Tech stack

N/A — documentation only.

## Design invariants

- Never expose secret values (alert #42 referenced by number only)
- Never suppress a CodeQL alert as the first move
- Never touch routine/ PRs

## Out of scope

Any code changes — this run is status note only.

## Rollback

Delete the PR branch; no code changes to revert.

## Open questions

None.

## File Structure

```
plans/0282-gar-819-health-run-95.md       ← this file (new)
plans/README.md                            ← row 0281 marked ✅ Merged, row 0282 added
docs/security/dependabot-status.md        ← run 95 section prepended
```

## Tasks

- [x] T1: Verify GAR-818 Done in Linear (already Done at 01:26 UTC)
- [x] T2: Scan all security surfaces — all clean
- [x] T3: CI on main `4e2c2a4` fully green (CI run 27110943883, 2026-06-08T01:26Z, 15/15 success)
- [x] T4: Create GAR-819 Linear issue
- [x] T5: Write plan 0282
- [x] T6: Update plans/README.md (row 0281 → ✅ Merged, add row 0282)
- [x] T7: Update docs/security/dependabot-status.md
- [ ] T8: Commit + push on branch health/202606080045-run95-status-note
- [ ] T9: Open PR, wait for CI green, squash-merge
- [ ] T10: Mark GAR-819 Done in Linear

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CI flaky on docs-only commit | Low | Re-push if format/clippy fails |

## Acceptance criteria

- PR CI: all checks green
- Squash-merged to main
- GAR-819 Done in Linear

## Cross-references

- Previous run: GAR-818 (run 94), PR #676, `4e2c2a4` — all surfaces clean after run 93 RUSTSEC-2026-0173 fix
- Pending routine/ PRs noted (NOT actioned): PR #675 + #674 (`routine/`, GAR-814)
- Dependabot alert #42: glib MEDIUM / RUSTSEC-2024-0429, GAR-513, suppressed expiry 2026-07-31
- RUSTSEC-2023-0071 (rsa): GAR-456, suppressed expiry 2026-07-31
- RUSTSEC-2026-0173 (proc-macro-error2): GAR-817, suppressed expiry 2026-07-31
- CodeQL ledger re-audit: GAR-491, due 2026-08-01
- ROADMAP.md §1.5 — security baseline (GAR-486 umbrella)

## Estimativa

< 5 min (docs-only, no compile).
