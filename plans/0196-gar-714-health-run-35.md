# Plan 0196 — GAR-714: Health Run 35 (2026-05-26 ~12:45 ET) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 35. Full scan of all 4 security surfaces (secret scanning,
malware/cargo-deny, Dependabot, CodeQL). Priority ladder exhausted at **(i)** — all surfaces
clean, no actionable security work found.

## Architecture

Bookkeeping-only. No code changes. Updates:
- `plans/0196-gar-714-health-run-35.md` (this file)
- `plans/README.md` — row 0196 added
- `docs/security/dependabot-status.md` — run 35 section prepended

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
  0196-gar-714-health-run-35.md   ← this file
  README.md                        ← row 0196 added
docs/security/
  dependabot-status.md             ← run 35 section prepended
```

## Tasks

- [x] T1: git fetch + pull main
- [x] T2: Scan CI on main (20/20 green — PR #534 `885ed2e`)
- [x] T3: Scan secret scanning (gitleaks CI pass)
- [x] T4: Scan Dependabot alerts (3 open, all upstream-blocked — unchanged)
- [x] T5: Scan CodeQL (all 3 Analyze jobs green)
- [x] T6: Check open health/ PRs (none)
- [x] T7: Check open routine/ PRs (PR #535 GAR-713 search-slice8 — skipped)
- [x] T8: File Linear status note (GAR-714)
- [x] T9: Create health branch + plan file
- [x] T10: Update plans/README.md + dependabot-status.md
- [x] T11: Commit + push + open PR (PR #536)
- [ ] T12: Wait for CI green + merge
- [ ] T13: Mark GAR-714 Done

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| CI transient failure | Low | Low | Re-push on flake |
| New RUSTSEC advisory appears between scan and merge | Very Low | Medium | Re-run scan after merge |

## Acceptance Criteria

- Plan committed on health/ branch
- PR opened, all CI checks green (≥16 actual checks)
- Squash-merged to main
- GAR-714 marked Done
- dependabot-status.md updated with run 35 entry
- plans/README.md row 0196 added

## Cross-references

- GAR-714: https://linear.app/chatgpt25/issue/GAR-714
- Previous run: GAR-712 (run 34) — PR #534 merged `885ed2e`
- Upstream-blocked alerts: GAR-456 (rsa), GAR-513 (glib+rand)
- Backlog: GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052)
- Suppression expiry: 2026-07-31
- CodeQL ledger re-audit due: 2026-08-01 (GAR-491)

## Estimativa

< 30 min (doc-only PR)
