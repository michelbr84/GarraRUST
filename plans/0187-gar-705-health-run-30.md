# Plan 0187 — GAR-705: Health Run 30 (2026-05-25) — All Surfaces Clean, Priority (i)

## Goal

Autonomous health & security run 30. No actionable security work found — priority ladder exhausted at **(i)**. This plan documents the scan results and closes the bookkeeping for the run.

Also resolves the dirty-state PR #506 (`docs/gar-703-bookkeeping`) — merge conflict in `plans/README.md` fixed by adding the missing row 0186 and marking it ✅ Merged.

## Architecture / Context

Standard security health routine. Checks 4 surfaces: secret scanning (gitleaks), malware (cargo-deny), Dependabot advisories, CodeQL code scanning. Falls back to a priority (i) status note when nothing actionable is found.

## Tech Stack

- GitHub Actions CI (20-check suite)
- cargo-deny / cargo-audit (RUSTSEC triage)
- gitleaks (secret scanning)
- CodeQL (Analyze rust + js-ts + actions)
- Linear (GAR team issue tracker)

## Design Invariants

- Branch prefix: `health/` (never `routine/`)
- Never push to main directly
- Never commit secrets or expose PII
- Routine/ PRs are untouched

## Out of Scope

- Any active security fix (none needed — priority (i))
- Dependabot suppressions already in place (rsa/glib/rand — expiry 2026-07-31)
- Argon2 0.6 upgrade (blocked on stable release — GAR-669)

## Rollback

Docs-only change; no rollback needed. Reverting the PR would remove the run record only.

## Open Questions

None.

## File Structure

```
plans/0187-gar-705-health-run-30.md        ← this file
plans/README.md                            ← row 0186 marked ✅, row 0187 added
docs/security/dependabot-status.md        ← run 30 section prepended
```

## Tasks

- [x] T1: git fetch + checkout main + pull --ff-only
- [x] T2: List open PRs → identify routine/ (none open) + health/ (none pending)
         → found PR #506 (`docs/gar-703-bookkeeping`) dirty — resolved merge conflict
- [x] T3: Scan secret scanning — clean (gitleaks CI pass on PR #506 all 20 checks green)
- [x] T4: Scan malware / cargo-deny — clean (cargo-deny CI pass)
- [x] T5: Scan Dependabot alerts — 3 open, all UPSTREAM-BLOCKED (suppressed 2026-07-31)
- [x] T6: Scan CodeQL / code scanning — clean (Analyze rust + js-ts + actions all pass)
- [x] T7: Verify CI on main (`ec683e9`) — 20/20 green (PR #506 CI confirms baseline)
- [x] T8: Apply priority ladder → **(i)** — all clean
- [x] T9: Create Linear issue GAR-705
- [x] T10: Write plan 0187 (this file)
- [x] T11: Update plans/README.md
- [x] T12: Prepend run 30 section to docs/security/dependabot-status.md
- [ ] T13: Commit + push branch health/202605251645-run30-status-note
- [ ] T14: Merge PR #506 (dirty-state resolved) + open PR for health run 30 + wait for 20/20 CI green
- [ ] T15: Squash-merge + mark GAR-705 Done

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Conflict with PR #506 on plans/README.md | Low | PR #506 updates 0186 row; this PR also updates 0186 + adds 0187 — sequential merge resolves cleanly |
| New advisory emerges between scan and merge | Very Low | CI Security Audit + cargo-deny catch at merge time |

## Acceptance Criteria

- [ ] PR #506 merged with all 20 CI checks green
- [ ] Health run 30 PR merged with all 20 CI checks green
- [ ] GAR-705 marked Done
- [ ] plans/README.md rows 0186 + 0187 both show ✅ Merged

## Cross-References

- Previous run: GAR-704 (run 29, PR #507, `ec683e9`)
- Dependabot suppressions: docs/security/dependabot-status.md
- Active suppressions: deny.toml + audit.toml (rsa RUSTSEC-2023-0071)
- Suppression expiry: 2026-07-31 (rsa, glib, rand)
- Next milestone: argon2 ≥ 0.6 stable (GAR-669 Slices 3–4)
- CodeQL ledger re-audit: 2026-08-01 (GAR-491)
- PR #506 conflict fix: bookkeeping for GAR-703 / plan 0185 merge

## Estimativa

~15 min (bookkeeping-only + PR #506 conflict resolution, no code changes, no compilation).
