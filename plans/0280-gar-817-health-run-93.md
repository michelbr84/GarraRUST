# Plan 0280 — GAR-817: Health Run 93 (2026-06-07 ~20:47 ET)

## Goal

Autonomous health & security scan run 93. Document results and merge status note. Priority **(i)** — all surfaces clean, no actionable security work.

## Architecture

Docs-only commit: plan file + dependabot-status.md run 93 note + plans/README.md rows.

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
plans/0280-gar-817-health-run-93.md       ← this file (new)
plans/README.md                            ← row 0277 marked ✅ Merged, row 0278 added
docs/security/dependabot-status.md        ← run 93 section prepended
```

## Tasks

- [x] T1: Check open PRs — PR #664 (routine/202606070621-post-thread-reply, GAR-811) skipped per protocol
- [x] T2: Sync main → `ab025c0`
- [x] T3: Scan all security surfaces (Secret/Malware/Dependabot/CodeQL/CI) — all clean
- [x] T4: Create GAR-817 Linear issue
- [x] T5: Write plan 0280
- [x] T6: Update plans/README.md (row 0277 → ✅ Merged, add row 0278)
- [x] T7: Update docs/security/dependabot-status.md
- [ ] T8: Commit + push on branch health/202606072047-run93-status-note
- [ ] T9: Open PR, wait for CI green, squash-merge
- [ ] T10: Mark GAR-817 Done in Linear

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CI flaky on docs-only commit | Low | Re-push if format/clippy fails |
| Plan number collision with routine/ PR #664 | Low | PR #664 has its own plan; health uses 0278 cleanly |

## Acceptance criteria

- PR CI: all checks green
- Squash-merged to main
- GAR-817 Done in Linear

## Cross-references

- Previous run: GAR-816 (run 92), PR #671, `be1ccdf5`
- Pending routine/ PR noted (NOT actioned): PR #664 (`routine/202606070621-post-thread-reply`, GAR-811)
- Dependabot alert #42: glib MEDIUM / RUSTSEC-2024-0429, GAR-513, suppressed expiry 2026-07-31
- RUSTSEC-2023-0071 (rsa): GAR-456, suppressed expiry 2026-07-31
- CodeQL ledger re-audit: GAR-491, due 2026-08-01
- ROADMAP.md §1.5 — security baseline (GAR-486 umbrella)

## Estimativa

< 5 min (docs-only, no compile).
