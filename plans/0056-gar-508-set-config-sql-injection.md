# Plan 0056 — GAR-508: Replace SET LOCAL format! with set_config() parameterized SQL

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (health routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-508](https://linear.app/chatgpt25/issue/GAR-508)
**Branch:** `health/202605050300-sql-inject-set-config`
**Pré-requisitos:** GAR-490 merged (PR #111 + #112).

## §1 Goal

Close remaining open CodeQL `rust/sql-injection` alerts in `rest_v1/` handlers by
replacing `sqlx::query(&format!("SET LOCAL app.current_X_id = '{uuid}'"))` with the
parameterized equivalent `sqlx::query("SELECT set_config('app.current_X_id', $1, true)")`.

`set_config(name, value, is_local=true)` is semantically identical to `SET LOCAL name = value`
(Postgres built-in, transaction-scoped, resets on commit/rollback). Unlike `SET LOCAL`, it accepts
a bind parameter for the value, eliminating the `format!` string interpolation that CodeQL flags.

## §2 Architecture

The `SET LOCAL` calls establish the RLS tenant context required by FORCE RLS policies:
- `app.current_user_id` — used by `audit_events_group_or_self` policy
- `app.current_group_id` — used by `chats_isolation`, `messages_isolation`, `group_members_isolation` etc.

These MUST be the first statement inside the transaction. This plan changes the *mechanism*
(format!→bind), not the *semantics* (still transaction-scoped, still first statement).

## §3 Tech stack

- `sqlx 0.8` with `Postgres` runtime — `sqlx::query("...").bind(uuid.to_string()).execute(&mut *tx)`
- `SELECT set_config(setting_name text, new_value text, is_local boolean) → text` — Postgres built-in
- No schema changes, no migration, no new crates

## §4 Design invariants

1. **Semantic equivalence**: `set_config('x', v, true)` ≡ `SET LOCAL x = v` in Postgres.
   Verified in Postgres docs §19.1.2 + §52.1 (set_config).
2. **Transaction-scoped**: `is_local = true` ensures the setting reverts when the tx commits
   or rolls back — same behavior as `SET LOCAL`.
3. **Uuid bind type**: `.bind(uuid.to_string())` passes a `text` value to Postgres; `set_config`
   expects `text`. No type mismatch.
4. **No user input reaches SQL text**: the SQL string literal is hardcoded; only the bind
   parameter (UUID value) is dynamic. CodeQL's taint analysis stops at bind params.
5. **Error handling preserved**: all existing `.map_err(|e| RestError::Internal(e.into()))?`
   chains are kept verbatim.

## §5 Out of scope

- Changing any business logic, query, RLS policy, or migration
- Adding a shared helper function (follow-up refactoring, not required here)
- Addressing other open CodeQL alerts (those are GAR-491.1 scope)
- Dependabot alerts (all upstream-blocked, tracked in `docs/security/dependabot-status.md`)

## §6 Rollback

`git revert <merge-sha>` — pure Rust source change (no schema, no config, no migration).
Behaviour identical before and after; rollback is safe at any time.

## §7 Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| `set_config` returns `text` row; `.execute()` discards it | None | None | sqlx execute() works on SELECT queries |
| Postgres privilege: `set_config` requires same `SET` privilege as `SET LOCAL` | Low | High | Same role (`garraia_app`) used in both cases; CI tests confirm |
| Missed occurrence | Low | Medium | `grep -rn "format!.*SET LOCAL"` post-implementation verify |
| CodeQL doesn't auto-close (alert stays open) | Low | Low | Can dismiss via ledger as fallback; CodeQL re-scans on PR merge |

## §8 Rollback plan

Revert commit on main is safe — no schema/data dependency. CI must be green before merge.

## §9 File structure (changes)

```
crates/garraia-gateway/src/rest_v1/
  groups.rs    — 7 call-sites: 5 user_id (multi-line) + 2 group_id (single-line)
  invites.rs   — 2 call-sites: 1 user_id (multi-line) + 1 group_id (single-line)
  chats.rs     — 4 call-sites: 2 user_id (multi-line) + 2 group_id (single-line)
  messages.rs  — 4 call-sites: 2 user_id (multi-line) + 2 group_id (single-line)
  uploads.rs   — 2 call-sites inside set_rls_context() helper
plans/
  0056-gar-508-set-config-sql-injection.md   ← this file
  README.md                                   ← +1 row
```

Total: 19 format!→bind replacements across 5 files.

## §10 M1 tasks

- [x] M1: Write plan 0056 + create GAR-508 + branch health/202605050300-sql-inject-set-config
- [ ] M2: Replace 7 SET LOCAL calls in groups.rs
- [ ] M3: Replace 2 SET LOCAL calls in invites.rs
- [ ] M4: Replace 4 SET LOCAL calls in chats.rs
- [ ] M5: Replace 4 SET LOCAL calls in messages.rs
- [ ] M6: Replace 2 SET LOCAL calls in uploads.rs (set_rls_context helper)
- [ ] M7: `cargo check -p garraia-gateway` green
- [ ] M8: `cargo clippy --workspace --exclude garraia-desktop --no-deps -D warnings` green
- [ ] M9: `cargo test -p garraia-gateway` green
- [ ] M10: `grep -rn 'format!.*SET LOCAL' crates/garraia-gateway/src/rest_v1/` → empty
- [ ] M11: Update plans/README.md row 0056
- [ ] M12: Commit + push + open PR
- [ ] M13: CI 17/17 green
- [ ] M14: Squash-merge + mark GAR-508 Done

## §11 Acceptance criteria

1. `grep -rn "format!.*SET LOCAL\|SET LOCAL.*format!" crates/garraia-gateway/src/rest_v1/` → no output
2. `cargo check -p garraia-gateway` exits 0
3. `cargo clippy --workspace --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` exits 0
4. `cargo test -p garraia-gateway` exits 0 (including cross-group authz tests)
5. CI 17/17 checks all green on PR head commit
6. CodeQL `rust/sql-injection` alerts for the affected files auto-close after next scan

## §12 Open questions

None — semantics of `set_config(..., true)` vs `SET LOCAL` are well-documented in Postgres.

## §13 Cross-references

- GAR-490 — Wave 1 path-injection (predecessor; sql-injection "PR B" was scoped but not executed)
- GAR-491 — Wave 2 fixtures + suppression convention (established the ledger mechanism)
- GAR-486 — umbrella (closed 2026-05-04)
- `docs/security/codeql-suppressions.md` — ledger (not touched by this plan; real fix avoids dismissal)

## §14 Estimativa

~1h — mechanical replacement, no logic change, CI already configured.
