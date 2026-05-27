# Plan 0203 — GAR-722: Health Run 40 (2026-05-27 ~07:15 ET) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 40. Full scan of all 4 security surfaces (secret scanning,
malware/cargo-deny, Dependabot, CodeQL). Priority ladder exhausted at **(i)** — all surfaces
clean, no actionable security work found.

## Architecture

Bookkeeping-only. No code changes. Updates:
- `plans/0203-gar-722-health-run-40.md` (this file)
- `plans/README.md` — plan 0202 (GAR-720) row marked ✅ Merged, plan 0203 row added
- `docs/security/dependabot-status.md` — run 40 section prepended

## Tech Stack

n/a (documentation only)

## Design Invariants

- Never expose secret values.
- Never amend merged commits.
- health/ branch prefix maintained throughout.

## Out of Scope

- Any code change.
- Touching routine/ PRs (roadmap routine territory — PR #543 GAR-718 search slice 10 chats).

## Rollback

Doc-only PR — revert is safe at any point.

## Open Questions

None.

## File Structure

```
plans/
  0203-gar-722-health-run-40.md   ← this file
  README.md                        ← plan 0202 marked ✅ Merged, row 0203 added
docs/security/
  dependabot-status.md             ← run 40 section prepended
```

## Tasks

- [x] T1: git fetch + check main (tip: `61d0514`)
- [x] T2: Check open health/ PRs — none open
- [x] T3: Scan CI on main (PR #543 check runs: 20/20 green)
- [x] T4: Scan secret scanning (gitleaks CI pass on PR #543)
- [x] T5: Run cargo audit --deny unsound locally — exit 0 (1098 advisories, 19 allowed unmaintained warnings)
- [x] T6: Scan Dependabot security alerts (3 open, all upstream-blocked — unchanged from run 39)
- [x] T7: Scan CodeQL (all 3 Analyze jobs green on PR #543)
- [x] T8: Check open routine/ PRs (PR #543 GAR-718 search slice 10 chats — 20/20 CI green, skipped per protocol)
- [x] T9: File Linear status note (GAR-722)
- [x] T10: Create health branch + plan file
- [x] T11: Update plans/README.md + dependabot-status.md

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Suppressed advisories unblocked before 2026-07-31 | Low | Medium | Re-evaluate at each run |
| argon2 stable release | Low | Low | Tracked in GAR-669 |

## Acceptance Criteria

- Plan 0203 committed on `health/202605270715-run40-status-note`
- plans/README.md row 0202 marked ✅ Merged, row 0203 added
- docs/security/dependabot-status.md run 40 section present
- PR opened, CI green (≥16 checks), squash-merged to main
- GAR-722 marked Done in Linear

## Cross-References

- Previous run: GAR-720 (plan 0202, PR #545, `61d0514`)
- 3 upstream-blocked alerts: rsa HIGH (GAR-456), glib MEDIUM + rand LOW (GAR-513)
- Suppression expiry: 2026-07-31 (`.cargo/audit.toml` + `deny.toml`)
- CodeQL ledger: `docs/security/codeql-suppressions.md`; re-audit due 2026-08-01 (GAR-491)
- OpenTelemetry upgrade: GAR-711 (Backlog)
- argon2 ≥ 0.6 stable: unblocks GAR-669 Slices 3–4

## Estimativa

~15 min (doc-only, no code).
