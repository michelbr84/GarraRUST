# Plan 0197 — GAR-715: Health Run 36 (2026-05-26 ~12:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 36. Full scan of all 4 security surfaces (secret scanning,
malware/cargo-deny, Dependabot, CodeQL). Priority ladder exhausted at **(i)** — all surfaces
clean, no actionable security work found.

## Architecture

Bookkeeping-only. No code changes. Updates:
- `plans/0197-gar-715-health-run-36.md` (this file)
- `plans/README.md` — row 0197 added
- `docs/security/dependabot-status.md` — run 36 section prepended

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
  0197-gar-715-health-run-36.md   ← this file
  README.md                        ← row 0197 added
docs/security/
  dependabot-status.md             ← run 36 section prepended
```

## Tasks

- [x] T1: git fetch + pull main (tip: `0a820a0`)
- [x] T2: Scan CI on main (20/20 green — PR #511 `0a820a0`)
- [x] T3: Scan secret scanning (gitleaks CI pass)
- [x] T4: Scan Dependabot alerts (3 open, all upstream-blocked — unchanged)
- [x] T5: Scan CodeQL (all 3 Analyze jobs green)
- [x] T6: Check open health/ PRs (PR #536 GAR-714 was dirty — rebased clean, force-pushed, merged)
- [x] T7: Check open routine/ PRs (none relevant — skipped per protocol)
- [x] T8: Handle PR #538 docs bookkeeping (updated behind→current, CI green, merged)
- [x] T9: File Linear status note (GAR-715)
- [x] T10: Create health branch + plan file
- [x] T11: Update plans/README.md + dependabot-status.md
- [ ] T12: Commit + push + open PR
- [ ] T13: Wait for CI green + merge
- [ ] T14: Mark GAR-715 Done

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| CI transient failure | Low | Low | Re-push on flake |
| New RUSTSEC advisory appears between scan and merge | Very Low | Medium | Re-run scan after merge |

## Acceptance Criteria

- Plan committed on health/ branch
- PR opened, all CI checks green (≥16 actual checks)
- Squash-merged to main
- GAR-715 marked Done
- dependabot-status.md updated with run 36 entry
- plans/README.md row 0197 added

## Cross-references

- GAR-715: https://linear.app/chatgpt25/issue/GAR-715
- Previous run: GAR-714 (run 35) — PR #536 rebased + merged this run
- PR #538 docs bookkeeping: merged this run
- Upstream-blocked alerts: GAR-456 (rsa), GAR-513 (glib+rand)
- Backlog: GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052)
- Suppression expiry: 2026-07-31
- CodeQL ledger re-audit due: 2026-08-01 (GAR-491)

## Estimativa

< 30 min (doc-only PR)
