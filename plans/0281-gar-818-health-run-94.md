# Plan 0281 — GAR-818: Health Run 94 (2026-06-07 ~21:01 ET)

## Goal

Autonomous health & security scan run 94. Priority **(i)** — all surfaces clean. No actionable security work found. Completes the bookkeeping for run 93's RUSTSEC-2026-0173 fix (PR #673 merged at `cf16c4e`).

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
plans/0281-gar-818-health-run-94.md       ← this file (new)
plans/README.md                            ← row 0280 marked ✅ Merged, row 0281 added
docs/security/dependabot-status.md        ← run 94 section prepended
```

## Tasks

- [x] T1: Merge pending PR #673 (GAR-817 run 93 fix — RUSTSEC-2026-0173 deny.toml suppress)
- [x] T2: Mark GAR-817 Done in Linear
- [x] T3: Sync main ��� `cf16c4e`
- [x] T4: Scan all security surfaces — all clean
- [x] T5: Create GAR-818 Linear issue
- [x] T6: Write plan 0281
- [x] T7: Update plans/README.md (row 0280 → ✅ Merged, add row 0281)
- [x] T8: Update docs/security/dependabot-status.md
- [ ] T9: Commit + push on branch health/202606072101-run94-status-note
- [ ] T10: Open PR, wait for CI green, squash-merge
- [ ] T11: Mark GAR-818 Done in Linear

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CI flaky on docs-only commit | Low | Re-push if format/clippy fails |

## Acceptance criteria

- PR CI: all checks green
- Squash-merged to main
- GAR-818 Done in Linear

## Cross-references

- Previous run: GAR-817 (run 93), PR #673, `cf16c4e` — fixed RUSTSEC-2026-0173
- Pending routine/ PR noted (NOT actioned): PR #664 (`routine/202606070621-post-thread-reply`, GAR-811)
- Dependabot alert #42: glib MEDIUM / RUSTSEC-2024-0429, GAR-513, suppressed expiry 2026-07-31
- RUSTSEC-2023-0071 (rsa): GAR-456, suppressed expiry 2026-07-31
- RUSTSEC-2026-0173 (proc-macro-error2): GAR-817, suppressed expiry 2026-07-31
- CodeQL ledger re-audit: GAR-491, due 2026-08-01
- ROADMAP.md §1.5 — security baseline (GAR-486 umbrella)

## Estimativa

< 5 min (docs-only, no compile).
