# Plan 0169 — GAR-513: Remove stale advisory-not-detected entries from deny.toml

**Status:** 🚧 In Progress
**Linear:** [GAR-513](https://linear.app/chatgpt25/issue/GAR-513)
**Branch:** `health/202605231000-gar513-deny-toml-hygiene`
**Health run:** 19 (2026-05-23 ~08:45 ET)

## Goal

Remove two stale `advisory-not-detected` entries (`RUSTSEC-2024-0429` glib and
`RUSTSEC-2026-0097` rand) from `deny.toml`. These entries now trigger
`advisory-not-detected` warnings in CI because cargo deny's advisory DB no
longer matches the affected crate versions (glib 0.18.5 / rand 0.7.3). The
`cargo audit` tool still matches both IDs; both remain in `audit.toml`.

## Architecture

Config-only change — no Rust code, no Cargo.toml, no Cargo.lock modifications.

| File | Change |
|---|---|
| `deny.toml` | Remove active `"RUSTSEC-2024-0429"` and `"RUSTSEC-2026-0097"` from `ignore` list; update SYNC NOTE header; add NOTE comments explaining the removal |
| `.cargo/audit.toml` | Update SYNC NOTE header; add DIVERGENCE NOTE to GAR-513 section documenting the honest asymmetry; keep both RUSTSEC entries (cargo audit still matches them) |
| `docs/security/dependabot-status.md` | Add run 19 section |

## Tech Stack

- `deny.toml` — cargo-deny configuration
- `.cargo/audit.toml` — cargo-audit configuration

## Design Invariants

1. `cargo audit --deny unsound` CI gate remains passing — both RUSTSEC IDs stay in `audit.toml`.
2. `cargo deny check advisories` CI gate no longer produces `advisory-not-detected` warnings for these IDs.
3. The honest asymmetry is documented in both files via DIVERGENCE NOTE and updated SYNC NOTE headers.
4. RUSTSEC IDs are never silently dropped — each removal has a NOTE comment with rationale.

## Out of Scope

- Structural fix for glib/rand unsound advisories (upstream-blocked; due 2026-07-31 per GAR-513).
- Password-hash / rand Dependabot alerts (upstream-blocked on argon2 ≥ 0.6 stable; tracked by GAR-669).

## Rollback

Revert the two RUSTSEC entries back into deny.toml `ignore` list. Since cargo
deny no longer matches them, this is safe but creates advisory-not-detected
noise again.

## Open Questions

None — the advisory-not-detected finding is deterministic (advisory DB drift,
not a bug in our config).

## File Structure

```
deny.toml                               ← remove 2 RUSTSEC entries, update comment
.cargo/audit.toml                       ← add DIVERGENCE NOTE, update SYNC NOTE
docs/security/dependabot-status.md     ← add run 19 section
plans/0169-gar-513-deny-toml-advisory-not-detected.md  ← this file
```

## Tasks

- [x] **T1** — Update `deny.toml` SYNC NOTE header (glib+rand moved to closed history)
- [x] **T2** — Replace `"RUSTSEC-2024-0429"` active entry in `deny.toml` with NOTE comment
- [x] **T3** — Replace `"RUSTSEC-2026-0097"` active entry in `deny.toml` with NOTE comment
- [x] **T4** — Update `audit.toml` SYNC NOTE header (glib+rand → audit-only residuals)
- [x] **T5** — Add DIVERGENCE NOTE to `audit.toml` GAR-513 section
- [x] **T6** — Add run 19 section to `docs/security/dependabot-status.md`
- [x] **T7** — Write this plan file
- [ ] **T8** — Update `plans/README.md` with plan 0169 row
- [ ] **T9** — Open PR, get CI green, merge
- [ ] **T10** — Update GAR-513 in Linear with attachment

## Risk Register

| Risk | Likelihood | Mitigation |
|---|---|---|
| `cargo audit` CI breaks after change | Very Low | Both RUSTSEC IDs remain in `audit.toml` |
| `cargo deny` starts matching these IDs again | Very Low | If DB updates, advisory-not-detected clears; ignore entries can be re-added atomically |
| Removes entries that mask a real new advisory | None | These specific IDs no longer match in cargo deny DB |

## Acceptance Criteria

1. `cargo deny check advisories` passes with 0 advisory-not-detected warnings for RUSTSEC-2024-0429 and RUSTSEC-2026-0097.
2. `cargo audit --deny unsound` CI gate still passes.
3. Full CI green (20/20 checks).

## Cross-References

- GAR-513: [glib+rand RUSTSEC carve-outs tracking](https://linear.app/chatgpt25/issue/GAR-513) (parent, In Progress, due 2026-07-31)
- Health run 18: prepared commits on `claude/focused-cray-BM98J` — absorbed into this plan
- Previous plan: [0167 — Q10.e whatsapp bootstrap](0167-gar-479-q10e-bootstrap-whatsapp.md)
- Suppression ledger: `docs/security/codeql-suppressions.md` (CodeQL, unchanged)
- Dependabot status: `docs/security/dependabot-status.md` (updated this run)

## Estimativa

< 30 min — config-only change, no code, no tests required.
