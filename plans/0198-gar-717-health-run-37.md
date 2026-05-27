# Plan 0198 — GAR-717: Health Run 37 (2026-05-26 ~20:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 37. Full scan of all 4 security surfaces (secret scanning,
malware/cargo-deny, Dependabot, CodeQL). Priority ladder exhausted at **(i)** — all surfaces
clean, no actionable security work found.

## Architecture

Bookkeeping-only. No code changes. Updates:
- `plans/0198-gar-717-health-run-37.md` (this file)
- `plans/README.md` — mark plan 0197 merged, row 0198 added
- `docs/security/dependabot-status.md` — run 37 section prepended

## Tech Stack

n/a (documentation only)

## Design Invariants

- Never expose secret values.
- Never amend merged commits.
- health/ branch prefix maintained throughout.

## Out of Scope

- Any code change.
- Touching routine/ PRs (roadmap routine territory).

## Rollback

Doc-only PR — revert is safe at any point.

## Open Questions

None.

## File Structure

```
plans/
  0198-gar-717-health-run-37.md   ← this file
  README.md                        ← 0197 marked merged, row 0198 added
docs/security/
  dependabot-status.md             ← run 37 section prepended
```

## Tasks

- [x] T1: git fetch + pull main (tip: `95ed89b`)
- [x] T2: Merge pending health/ PR #541 (GAR-715 run 36) — 20/20 CI green, squash-merged `95ed89bc`
- [x] T3: Scan CI on main (20/20 green — PR #541 all checks pass)
- [x] T4: Scan secret scanning (gitleaks CI pass on PR #541)
- [x] T5: Scan Dependabot alerts (3 open, all upstream-blocked — unchanged from run 36)
- [x] T6: Scan CodeQL (all 3 Analyze jobs green on PR #541)
- [x] T7: Check open health/ PRs (none open after merging PR #541)
- [x] T8: Check open routine/ PRs (PR #540 GAR-716 search slice 9 — skipped per protocol; note: references plan 0197 which is now taken by GAR-715, requires renumbering on rebase)
- [x] T9: File Linear status note (GAR-717)
- [x] T10: Create health branch + plan file
- [x] T11: Update plans/README.md + dependabot-status.md
- [ ] T12: Commit + push + open PR
- [ ] T13: Wait for CI green + merge
- [ ] T14: Mark GAR-717 Done

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| CI transient failure | Low | Low | Re-push on flake |
| New RUSTSEC advisory appears between scan and merge | Very Low | Medium | Re-run scan after merge |
| Routine/ PR #540 plan-number collision (0197 taken) | Known | Low | Noted; roadmap routine handles on rebase |

## Acceptance Criteria

- Plan committed on health/ branch
- PR opened, all CI checks green (≥16 actual checks)
- Squash-merged to main
- GAR-717 marked Done
- dependabot-status.md updated with run 37 entry
- plans/README.md row 0197 marked merged + row 0198 added

## Cross-references

- GAR-717: https://linear.app/chatgpt25/issue/GAR-717
- Previous run: GAR-715 (run 36) — PR #541 squash-merged `95ed89bc` this run
- Routine/ PR #540: GAR-716 search slice 9 — open, behind main, skip (routine/ territory)
- Upstream-blocked alerts: GAR-456 (rsa), GAR-513 (glib+rand)
- Backlog: GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052)
- Suppression expiry: 2026-07-31
- CodeQL ledger re-audit due: 2026-08-01 (GAR-491)

## Estimativa

< 30 min (doc-only PR)
