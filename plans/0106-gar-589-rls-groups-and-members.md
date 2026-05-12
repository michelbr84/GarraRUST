# Plan 0106 — GAR-589: FORCE RLS on `groups` + `group_members` tables

## Goal

Add database-level Row-Level Security (FORCE RLS) to the `groups` and `group_members`
tables, making the `app.current_user_id` / `app.current_group_id` session variables
that several handlers already set **load-bearing** rather than forward-compat stubs.

Closes [GAR-589](https://linear.app/chatgpt25/issue/GAR-589).

## Context

Migration 007 (plan 0007, GAR-408) added FORCE RLS to 10 tenant-scoped tables but
intentionally left `groups` and `group_members` out (see migration 007 comment lines
15-25: "app-layer join is cleaner in v1"). PR #273 / plan 0105 (GAR-580) introduced
`GET /v1/groups` which sets `app.current_user_id` "as a defensive forward-compat hook".
GAR-589 was filed immediately after merge to make that hook load-bearing.

Today, cross-group isolation for these tables relies exclusively on `WHERE gm.user_id = $1`
in handlers. FORCE RLS adds defense-in-depth: even if a handler bug omits the WHERE clause,
the database enforces isolation at the role level.

## Architecture

### Migration 018 — new policies

**`groups` table**:
- `ENABLE + FORCE ROW LEVEL SECURITY`
- Policy `groups_member_access` (FOR ALL, PERMISSIVE):
  - USING: `id IN (SELECT group_id FROM group_members WHERE user_id = current_user_id AND status = 'active')`
  - WITH CHECK: same subquery **OR** `created_by = current_user_id`
    (allows INSERT of a new group before the creator's `group_members` row exists)

**`group_members` table**:
- `ENABLE + FORCE ROW LEVEL SECURITY`
- Policy `group_members_visible` (FOR ALL, PERMISSIVE):
  - USING: `group_id = current_group_id OR user_id = current_user_id`
  - WITH CHECK: same (symmetric — allows both group-scoped writes and self-membership)

### Handler code changes

| Handler | Problem | Fix |
|---------|---------|-----|
| `get_group` | No tx, no SET LOCAL → silently fails with FORCE RLS on `groups` | Wrap in `pool.begin()` + `set_config('app.current_user_id')` + `set_config('app.current_group_id')` |
| `list_members` | Missing `app.current_group_id` → only caller's own row visible | Add `set_config('app.current_group_id', id, true)` after existing `set_config('app.current_user_id')` |

All other group handlers already set the necessary context variables.

## Tech stack

- PostgreSQL 16 FORCE RLS (same as migration 007)
- `sqlx::migrate!` (forward-only migration)
- Axum 0.8 handlers in `crates/garraia-gateway/src/rest_v1/groups.rs`

## Design invariants

1. **Forward-only**: no DROP TABLE, no destructive ALTER.
2. **Fail-closed**: missing `SET LOCAL` → empty results, not cross-tenant leak.
3. **No BYPASSRLS**: `garraia_app` role must always go through RLS. The `garraia_login` and `garraia_signup` BYPASSRLS roles do not touch `groups` or `group_members`.
4. **No circular dependency**: `groups` policy references `group_members` (which has its own FORCE RLS). `group_members` policy does NOT reference `groups`. Directionality is one-way.
5. **Idempotent CREATE POLICY**: use `DROP POLICY IF EXISTS` before `CREATE POLICY` (same pattern as migration 013).
6. **Explicit WITH CHECK** (plan 0021 lesson): always set WITH CHECK explicitly, never rely on Postgres implicit fallback from USING.

## Validações pré-plano

- [x] Migration slot 018 is free (`ls migrations/` confirms 017 is the latest).
- [x] `groups` and `group_members` are currently NOT under FORCE RLS (confirmed in migration 007 comment, line 15-25).
- [x] `get_group` FIXME acknowledged in source (groups.rs line 420-426).
- [x] `list_members` missing `set_config('app.current_group_id')` confirmed (groups.rs line 1400-1410).
- [x] No other handler INSERTs into `group_members` with a third-party `user_id` outside of a `SET LOCAL app.current_group_id` context. (Exception: `create_group` INSERTs own membership — covered by WITH CHECK Branch 2.)

## Out of scope

- `group_invites` — token-based access, handled by separate endpoint. Left for a future slice.
- RBAC capabilities `groups.read`, `groups.write` — app-layer authz already enforces via Principal extractor.
- Updating the 81-case RLS matrix in `garraia-auth/tests/rls_matrix.rs` with `groups`/`group_members` rows — tracked as follow-up in GAR-589 acceptance criteria item 4. **This plan delivers the migration + handler fixes only** (the matrix extension is a separate concern that requires test-support Harness changes beyond the migration scope).

## Rollback

Postgres DDL is transactional: if the migration fails mid-run, it rolls back entirely.
Manual rollback (only if needed post-deploy): `ALTER TABLE groups DISABLE ROW LEVEL SECURITY; ALTER TABLE group_members DISABLE ROW LEVEL SECURITY;`

## File structure

```
crates/garraia-workspace/migrations/018_rls_groups_and_members.sql  [NEW]
crates/garraia-gateway/src/rest_v1/groups.rs                        [EDIT: get_group, list_members]
plans/0106-gar-589-rls-groups-and-members.md                        [THIS FILE]
plans/README.md                                                      [EDIT: add row]
```

## Tasks

- [x] M1: Write `018_rls_groups_and_members.sql`
- [x] M2: Fix `get_group` — add tx + SET LOCAL user_id + group_id
- [x] M3: Fix `list_members` — add SET LOCAL group_id
- [x] M4: `cargo check -p garraia-gateway` green
- [x] M5: `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` green
- [x] M6: Push + open PR

## Risk register

| Risk | Mitigation |
|------|-----------|
| `get_group` test returns 404 after FORCE RLS | Covered by existing `v1_groups_scenarios` test hitting the fixed handler. |
| INSERT into `groups` fails (new group not in `group_members` yet) | WITH CHECK Branch 2 (`created_by = current_user_id`) covers the INSERT window. |
| `set_member_role` / `delete_member` UPDATE fails WITH CHECK | Both set `app.current_group_id`; after UPDATE `group_id` unchanged → Branch 1 passes. |
| `list_groups` broken by `group_members` FORCE RLS | `list_groups` sets `app.current_user_id`; Branch 2 of `group_members` policy passes. `groups` policy subquery also evaluates under Branch 2 → ✓. |
| Performance regression from RLS subqueries | Subquery is indexed (`group_members.user_id` is FK with index). Acceptable for v1 scale. |

## Acceptance criteria

1. `cargo test -p garraia-gateway --test rest_v1_groups -- --nocapture` green.
2. `cargo test -p garraia-gateway --test rest_v1_groups_list -- --nocapture` green.
3. `cargo test -p garraia-gateway --test rest_v1_groups_members_invites -- --nocapture` green.
4. `git grep 'FORCE ROW LEVEL SECURITY' crates/garraia-workspace/migrations/018_*` shows both `groups` and `group_members`.
5. `git grep 'group_members has no FORCE RLS' crates/garraia-gateway/src/rest_v1/groups.rs` returns 0 hits (comments updated).

## Cross-references

- GAR-589 (Linear issue)
- Migration 007 (original RLS setup — plan 0007 / GAR-408)
- Migration 013 (WITH CHECK pattern for explicit policy — plan 0021 / GAR-425)
- Plan 0105 / GAR-580 (`GET /v1/groups` — triggered this follow-up)
- CLAUDE.md rule 10: cross-group authz tests required before merge

## Estimativa

0.5 / 1 / 1.5 hours. Two handler edits + one SQL migration file. All scaffolding in place.
