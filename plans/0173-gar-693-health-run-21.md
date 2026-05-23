# Health Run 21 — GAR-693

## Goal

Health & security routine run 21 (2026-05-23, ~16:45 ET). Merge pending
`health/` PRs from run 20 (conflict resolution + plan numbering fix),
confirm all security surfaces clean, file status note.

## Actions taken

### Pending health/ PRs resolved

| PR | Branch | Issue | Action |
|---|---|---|---|
| #487 | `chore/plan-0170-done-bookkeeping` | — | `behind` → updated via `update_pull_request_branch`, CI green, merged |
| #486 | `health/202605231245-run20-status-note` | GAR-692 | `dirty` (conflict in `plans/README.md`) → resolved + plan numbering fix (0171=GAR-498, 0172=GAR-692), CI green, merged |

### Plan numbering fix

Commit `c65e099` added `plans/0171-gar-498-native-skills-registry.md` to
main without a README entry. PR #486 (health run 20) had claimed `0171`
for GAR-692. Fixed: GAR-498 = 0171, GAR-692 = 0172, GAR-693 (this run)
= 0173.

## Security surface scan

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #486 + #487 |
| Malware (cargo/npm) | ✅ none | — |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — all expiry 2026-07-31 |
| Open Dependabot PRs | ✅ 0 | — |
| Security Audit (`cargo audit`) | ✅ pass | PR #486/#487 CI green |
| cargo-deny | ✅ pass | — |
| CodeQL (rust + js-ts + actions) | ✅ pass | — |
| CI on main (`c65e099`) | ✅ green | — |

## Priority

**(i)** — priority ladder exhausted, no actionable security work.

## Out of scope

- Upstream-blocked Dependabot alerts (GAR-456, GAR-513) — expiry 2026-07-31.
- CodeQL ledger re-audit — due 2026-08-01 (GAR-491).

## Cross-references

- Health run 20: GAR-692 (PR #486, merged via this run)
- Health run 19: GAR-513/plan 0169 (PR #484, `b3f62fd`)
- Upstream-blocked: GAR-456 (rsa), GAR-513 (glib+rand)
- Plan 0171: GAR-498 native skills registry (`c65e099`, 2026-05-23)
- Plan 0172: GAR-692 health run 20
- Plan 0173: GAR-693 health run 21 (this plan)
