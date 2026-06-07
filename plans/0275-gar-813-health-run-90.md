# Plan 0275 — GAR-813: Health Run 90 (2026-06-07 ~04:45 ET)

## Goal

Autonomous health & security scan run 90. Document results and merge status note. Priority **(i)** — all surfaces clean, no actionable security work.

## Architecture

Docs-only commit: plan file + dependabot-status.md run 90 note + plans/README.md row.

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
plans/0275-gar-813-health-run-90.md       ← this file (new)
plans/README.md                            ← row 0274 → ✅ Merged, row 0275 added
docs/security/dependabot-status.md        ← run 90 section prepended
```

## Tasks

- [x] T1: Merge pending health/ PR (#665, GAR-812, run 89) — `75c311ab` ✅
- [x] T2: Pull updated main — `75c311ab`
- [x] T3: Create GAR-813 Linear issue
- [x] T4: Scan all security surfaces (Secret/Malware/Dependabot/CodeQL/CI) — all clean
- [x] T5: Write plan 0275
- [x] T6: Update plans/README.md
- [x] T7: Update docs/security/dependabot-status.md
- [ ] T8: Commit + push on branch health/202606070845-run90-status-note
- [ ] T9: Open PR, wait for CI green, squash-merge
- [ ] T10: Mark GAR-813 Done in Linear

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CI flaky on docs-only commit | Low | Re-push if format/clippy fails |

## Acceptance criteria

- PR CI: all 20 checks green
- Squash-merged to main
- GAR-813 Done in Linear

## Cross-references

- Previous run: GAR-812 (run 89), PR #665, `75c311ab`
- Dependabot alert #42: glib MEDIUM / RUSTSEC-2024-0429, GAR-513, suppressed expiry 2026-07-31
- RUSTSEC-2023-0071 (rsa): GAR-456, suppressed expiry 2026-07-31
- CodeQL ledger re-audit: GAR-491, due 2026-08-01
- ROADMAP.md §1.5 — security baseline (GAR-486 umbrella)

## Estimativa

< 5 min (docs-only, no compile).
