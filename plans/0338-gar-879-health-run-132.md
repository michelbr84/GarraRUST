# Plan 0338 — GAR-879: Health Run 132 (2026-06-14 ~02:27 ET) — All Surfaces Clean, Priority (i)

## Goal

Record the health & security routine run 132 status note. All security surfaces scanned post-merge of GAR-876 (`PATCH /v1/me/password`); no actionable items found. Priority ladder exhausted at (i).

## Architecture

Doc-only change — no code, no schema, no deps.

## Tech Stack

- Plans: Markdown tracking files
- Linear: GAR-879 (In Progress → Done)

## Design Invariants

- Plan number 0338 (sequential after 0337)
- Branch prefix `health/` (never `routine/`)
- No secrets, no code changes

## Out of Scope

- Any code or schema changes
- Bumping suppression expiry dates (GAR-513 owns that, expiry 2026-07-31)

## Rollback

Delete branch + close PR. No persistent state changes.

## §12 Open Questions

None.

## File Structure

```
plans/0338-gar-879-health-run-132.md      ← this file
plans/README.md                            ← add row 0338 + mark 0335 done
docs/security/dependabot-status.md        ← update header + add run 132 section
```

## M1: Status Note Tasks

- [x] Create plan file
- [x] Update plans/README.md
- [x] Update dependabot-status.md
- [x] Create branch health/202606140615-run132-status-note
- [ ] Push + open PR
- [ ] CI green → merge
- [ ] Mark GAR-879 Done

## Security Scan Results

**CI run:** SHA `524b685` (PR #757 head), 2026-06-14T06:32:07Z — all 20 jobs success.
**Post-merge main SHA:** `0c9a024` (GAR-876 squash merge), CI triggered 2026-06-14T06:32:26Z.
**Result: 0 vulnerabilities · 0 unsound · 18 allowed unmaintained warnings**

All 18 unmaintained-crate warnings are pre-tracked in `deny.toml` with documented owners and expiry dates. No new advisories since run 131.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI Secret Scan job success on `524b685` (2026-06-14T06:32:07Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success — advisories ok, bans ok, licenses ok, sources ok |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot security alerts | ⚠️ 1 moderate (RUSTSEC-2023-0071), allowlisted | rsa 0.9.10 — Marvin Attack. HS256-only invariant holds. Allowlisted expiry 2026-07-31. |
| Security Audit (cargo-audit) | ✅ pass | CI success on `524b685` (2026-06-14T06:32:07Z); 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | All 18 unmaintained IDs suppressed in deny.toml; RUSTSEC-2023-0071 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + (javascript-typescript) + (actions) success on `524b685` (2026-06-14T06:32:07Z) |
| Quality Ratchet | ✅ pass | CI success on `524b685` |
| CI post-merge main (`0c9a024`) | ✅ green | PR #757 CI: 20/20 checks success on `524b685`; squash merge CI triggered 2026-06-14T06:32:26Z |
| Workflow failures (last 7d) | ✅ none | No failures in 20 consecutive runs |
| Open health/ PRs | ✅ none | No pending health/ branches prior to this run |

## Open PRs at scan time

None (PR #757 merged as `0c9a024`).

## Risk Register

| Risk | Mitigation |
|------|-----------|
| PATCH /v1/me/password introduces new auth surface | All user_identities access through LoginPool BYPASSRLS; anti-enumeration; dual-verify Argon2id+PBKDF2; audit event emitted. PR CI: 20/20 green. |

## Acceptance Criteria

- PR merged to main with green CI
- GAR-879 marked Done
- plans/README.md row 0338 shows commit SHA + PR number

## Cross-references

- GAR-878 (health run 131, 2026-06-14 ~00:48 ET) — PR #760 merged
- GAR-877 (health run 130, 2026-06-14 ~00:45 ET) — PR #758 merged
- GAR-876 (PATCH /v1/me/password) — PR #757 merged as `0c9a024` 2026-06-14
- GAR-513 (glib/rsa carve-out owner, expiry 2026-07-31)
- GAR-491 (CodeQL triage, re-audit due 2026-08-01)

## Estimativa

< 10 min doc-only
