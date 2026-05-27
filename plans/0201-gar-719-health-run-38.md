# Plan 0201 — GAR-719: Health Run 38 (2026-05-27 ~00:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 38. Full scan of all 4 security surfaces (secret scanning,
malware/cargo-deny, Dependabot, CodeQL). Priority ladder exhausted at **(i)** — all surfaces
clean, no actionable security work found.

## Architecture

Bookkeeping-only. No code changes. Updates:
- `plans/0201-gar-719-health-run-38.md` (this file)
- `plans/README.md` — fix plan 0199 (GAR-716) row to ✅ Merged, add plan 0201
- `docs/security/dependabot-status.md` — run 38 section prepended

## Tech Stack

n/a (documentation only)

## Design Invariants

- Never expose secret values.
- Never amend merged commits.
- health/ branch prefix maintained throughout.

## Out of Scope

- Any code change.
- Touching routine/ PRs (roadmap routine territory — PR #543 GAR-718 search slice 10).

## Rollback

Doc-only PR — revert is safe at any point.

## Open Questions

None.

## File Structure

```
plans/
  0201-gar-719-health-run-38.md   ← this file
  README.md                        ← plan 0199 marked ✅ Merged, row 0201 added
docs/security/
  dependabot-status.md             ← run 38 section prepended
```

## Tasks

- [x] T1: git fetch + pull main (tip: `d6d0487`)
- [x] T2: Check open health/ PRs — none open
- [x] T3: Scan CI on main (PR #543 check runs: all completed checks green)
- [x] T4: Scan secret scanning (gitleaks CI pass on PR #543)
- [x] T5: Scan Dependabot alerts (3 open, all upstream-blocked — unchanged from run 37)
- [x] T6: Scan CodeQL (all 3 Analyze jobs green on PR #543)
- [x] T7: Check open routine/ PRs (PR #543 GAR-718 search slice 10 — skipped per protocol)
- [x] T8: File Linear status note (GAR-719)
- [x] T9: Create health branch + plan file
- [x] T10: Update plans/README.md + dependabot-status.md
- [ ] T11: Commit + push + open PR
- [ ] T12: Wait for CI green + merge
- [ ] T13: Mark GAR-719 Done

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| CI transient failure | Low | Low | Re-push on flake |
| New RUSTSEC advisory appears between scan and merge | Very Low | Medium | Re-run scan after merge |
| Routine/ PR #543 merges before this health PR | Low | None | Routine/ and health/ never conflict |

## Acceptance Criteria

- Plan committed on health/ branch
- PR opened, all CI checks green (≥16 actual checks)
- Squash-merged to main
- GAR-719 marked Done
- dependabot-status.md updated with run 38 entry
- plans/README.md plan 0199 row corrected to ✅ Merged

## Cross-References

- GAR-717 (run 37) — previous health run, all surfaces clean, PR #542 merged `d36d5f4`
- GAR-716 (plan 0199) — search slice 9 merged `d6d0487`, bookkeeping fixed this run
- GAR-718 (plan 0200) — routine/ PR #543 in progress, skipped
- GAR-456 — rsa HIGH RUSTSEC-2023-0071, upstream-blocked
- GAR-513 — glib MEDIUM + rand LOW, upstream-blocked, expiry 2026-07-31
- GAR-711 — OpenTelemetry 0.26→0.32 upgrade, Backlog
- GAR-491 — CodeQL ledger re-audit due 2026-08-01

## Estimativa

~15 min total (doc-only run).
