# Plan 0183 — GAR-701 Health Run 27 Status Note

## Goal

Record the outcome of health & security routine run 27 (2026-05-25 ~07:10 ET). All security surfaces scanned; priority ladder exhausted at (i) — no actionable work found.

## Architecture

Docs-only change. No Rust / Flutter / JS code touched.

## Tech Stack

- `docs/security/dependabot-status.md` — health-run section insert
- `plans/0183-gar-701-health-run-27.md` — this file
- `plans/README.md` — plan 0183 row added

## Design Invariants

- No secrets or PII in any file
- No code changes — doc bookkeeping only

## Out of Scope

- Any Rust, Flutter, or CI changes
- Merging or touching any `routine/` PRs

## Rollback

`git revert <commit>` — trivial; docs-only.

## Open Questions

None.

## File Structure

```
docs/security/dependabot-status.md   — run 27 section prepended
plans/0183-gar-701-health-run-27.md  — this file
plans/README.md                       — plan 0183 row added
```

## Tasks

- [x] T1 — Create Linear issue GAR-701
- [x] T2 — Create branch `health/202605250710-run27-status-note`
- [x] T3 — Write plan 0183 (this file)
- [x] T4 — Update `docs/security/dependabot-status.md` (run 27 section)
- [x] T5 — Update `plans/README.md` (add row 0183)
- [ ] T6 — Commit, push, open PR
- [ ] T7 — Wait for CI green
- [ ] T8 — Squash-merge PR
- [ ] T9 — Mark GAR-701 Done in Linear

## Risk Register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Merge conflict on dependabot-status.md or plans/README.md | Low-Medium | PRs #498, #501, #502 open; rebase after they merge if needed |
| CI failure on docs PR | Very low | Docs-only; fmt/clippy/test not affected |

## Acceptance Criteria

- Run 27 section present in `dependabot-status.md`
- Plan 0183 row in `plans/README.md`
- All 20 CI checks green on the PR
- GAR-701 marked Done

## Cross-References

- Previous run: GAR-699 (run 26, PR #501 open — 20/20 CI green)
- Open tracking: GAR-456 (rsa, HIGH, expiry 2026-07-31), GAR-513 (glib+rand, expiry 2026-07-31), GAR-491 (CodeQL ledger re-audit 2026-08-01)
- Plan 0181: reserved for GAR-699 health run 26 (PR #501, health/ branch)
- Plan 0182: reserved for GAR-700 message attachments API (PR #502, routine/ branch)
- `deny.toml` + `.cargo/audit.toml` — suppression rationale

## Dependency Scan Summary

| Package | Version | Advisory | Status |
|---|---|---|---|
| rsa | 0.9.10 | RUSTSEC-2023-0071 | Suppressed — sqlx-macros-core lockfile ghost, HS256-only invariant holds. Expires 2026-07-31 |
| glib | 0.18.5 | RUSTSEC-2024-0429 | Suppressed — Tauri-only, CI-excluded, zero server risk. Expires 2026-07-31 |
| rand | 0.7.3 | RUSTSEC-2026-0097 | Suppressed — build-time only (phf_codegen chain). Expires 2026-07-31 |
| rustls-webpki | 0.103.13 | — | Safe chain only (legacy 0.101/0.102 chains removed) |
| quinn-proto | 0.11.14 | — | Patched (GAR-457) |
| wasmtime | 44.0.2 | — | All 15 wasmtime RUSTSEC IDs closed (GAR-454) |
| h2 | 0.4.14 | — | No new advisory detected |
| rustls | 0.23.40 | — | No new advisory detected |
| openssl | 0.10.80 | — | No new advisory detected |
| ring | 0.17.14 | — | No new advisory detected |
| tokio | 1.52.3 | — | No new advisory detected |
| argon2 | 0.5.3 | — | Stable; 0.6.0-rc.8 RC still blocks GAR-669 |
| curve25519-dalek | 4.1.3 | — | No new advisory detected |

## Estimativa

< 30 min end-to-end (docs only).
