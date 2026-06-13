# Plan 0328 — GAR-869: DELETE /v1/me/sessions — revoke all active sessions

> **Status:** In Progress
> **Linear:** [GAR-869](https://linear.app/chatgpt25/issue/GAR-869)
> **Branch:** `routine/202606130618-revoke-all-sessions`
> **Parent plan:** 0327

## Goal

Add `DELETE /v1/me/sessions` — atomically revoke all of the caller's active
sessions ("sign out from all devices"). Natural companion to plan 0327 / GAR-866
which added list + single-revoke.

## Architecture

Thin handler in `crates/garraia-gateway/src/rest_v1/me.rs`:

1. `Principal` extractor (no `X-Group-Id` — sessions are user-scoped).
2. SET LOCAL `app.current_user_id` and `app.current_group_id` (nil-uuid for
   group) for FORCE RLS (`sessions_owner_only` migration 007).
3. `UPDATE sessions SET revoked_at = now() WHERE user_id = $1
    AND revoked_at IS NULL AND expires_at > now()` — bulk revoke.
4. Return 204 (no body). `rows_affected` tracked for audit metadata.
5. Emit `SessionsAllRevoked` audit event with `{count: N}` metadata.

New audit action `SessionsAllRevoked` added to `WorkspaceAuditAction` in
`garraia-auth` alongside the existing `SessionRevoked`.

## Tech stack

Rust (Axum 0.8), sqlx (Postgres), utoipa (OpenAPI), `garraia_auth::Principal`.

## Design invariants

- NO `unwrap()` outside tests.
- NO SQL string concat — raw string query with positional `$N` params.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` before SQL.
- `group_id` for `audit_workspace` → nil-uuid (sessions are user-scoped; passes
  `audit_events_group_or_self` WITH CHECK via branch 1 with nil-uuid SET LOCAL).
- All sessions revoked atomically in one UPDATE statement (no race between
  listing and revoking).
- No audit event if `rows_affected == 0` (nothing to record).
- `refresh_token_hash` never appears in any response.

## Validações pré-plano

- `sessions_owner_only` FORCE RLS (migration 007) ensures cross-user isolation.
- `SessionRevoked` in `audit_workspace.rs` at line ~692 — add `SessionsAllRevoked`
  alongside it.
- Route `/v1/me/sessions` already registered with `get(me::list_my_sessions)` —
  add `.delete(me::revoke_all_my_sessions)` chain.
- openapi.rs already has `super::me::list_my_sessions` at line 227 — add
  `super::me::revoke_all_my_sessions` below it.

## Out of scope

- Revoking only sessions older than a given date (deferred).
- Selective revoke-except-current (JWT access tokens have no session ID; cannot
  distinguish current session without adding jti claim — deferred to ADR).
- Invalidating in-flight access tokens (stateless JWT; they expire naturally in 15min).

## Rollback

Revert the PR. No migration to undo.

## §12 Open questions

None — acceptance criteria fully specified.

## File structure

```
crates/garraia-auth/src/audit_workspace.rs   ← add SessionsAllRevoked variant + as_str arm + 2 tests
crates/garraia-gateway/src/rest_v1/me.rs     ← new revoke_all_my_sessions handler + 6 unit tests
crates/garraia-gateway/src/rest_v1/mod.rs    ← .delete(me::revoke_all_my_sessions) in all 3 branches
crates/garraia-gateway/src/rest_v1/openapi.rs ← add super::me::revoke_all_my_sessions
plans/0328-gar-869-revoke-all-sessions.md    ← this file
plans/README.md                              ← row 0328 added
```

## M1 Tasks

- [x] T1: Write plan + create Linear issue GAR-869
- [ ] T2: Add `SessionsAllRevoked` to `audit_workspace.rs` (variant + as_str + 2 tests)
- [ ] T3: Implement `revoke_all_my_sessions` handler in `me.rs` + 6 unit tests
- [ ] T4: Wire route in `mod.rs` (all 3 branches)
- [ ] T5: Register in `openapi.rs`
- [ ] T6: Commit + push + open PR
- [ ] T7: Wait for CI green; fix any failures
- [ ] T8: Squash-merge; mark GAR-869 Done
- [ ] T9: Update ROADMAP §3.4 me/* + plans/README.md row 0328

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Conflict with revoke_my_session route | Low | Different route path — `/v1/me/sessions` vs `/{session_id}` |
| Audit `nil_uuid` failing WITH CHECK | Low | `SessionRevoked` already uses nil-uuid; same pattern |
| Large batch revoke performance | Very low | Single UPDATE, sessions table is small per user |

## Acceptance criteria

1. `DELETE /v1/me/sessions` → 204; all active caller sessions have `revoked_at` set.
2. No `refresh_token_hash` in any response.
3. `SessionsAllRevoked` audit event emitted with `count` metadata; 0-row case emits nothing.
4. Cross-user isolation: FORCE RLS ensures only caller's sessions affected.
5. Missing `X-Group-Id` → still works (user-scoped endpoint).
6. No JWT → 401.
7. `cargo clippy --workspace` green. 6 unit tests pass in me.rs + 2 in audit_workspace.rs.
8. Route wired in all 3 mod.rs branches; OpenAPI path registered.

## Cross-references

- plan 0327 / GAR-866 — GET /v1/me/sessions + DELETE /v1/me/sessions/{session_id}
- audit_workspace.rs line ~692 — SessionRevoked (parallel pattern)
- sessions_owner_only FORCE RLS — migration 007

## Estimativa

0.5h. Handler ~40 LOC, tests ~80 LOC, routing ~6 LOC, audit_workspace ~15 LOC.
