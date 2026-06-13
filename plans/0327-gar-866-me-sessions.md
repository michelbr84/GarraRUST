# Plan 0327 — GAR-866: GET /v1/me/sessions + DELETE /v1/me/sessions/{session_id}

> **Status:** In Progress
> **Linear:** [GAR-866](https://linear.app/chatgpt25/issue/GAR-866)
> **Branch:** `routine/202606130026-me-sessions`
> **Parent plan:** 0325

## Goal

Add two session-management endpoints to the `me/` inbox API:

* `GET /v1/me/sessions` — cursor-paginated list of the caller's active sessions
  (not revoked, not expired). Enables mobile "Security → Active sessions" screen.
* `DELETE /v1/me/sessions/{session_id}` — revoke a specific session
  (set `revoked_at = now()`). Enables "Sign out from other devices".

No new migration needed: `sessions` table from migration 001 already exists.
FORCE RLS (`sessions_owner_only` policy, migration 007) guarantees cross-user
isolation at the DB layer.

## Architecture

Two handlers in `crates/garraia-gateway/src/rest_v1/me.rs` following the
established inbox pattern (GET /v1/me/chats, GET /v1/me/files, etc.):

**GET /v1/me/sessions**
1. `Principal` extractor — no `group_id` required (sessions are user-scoped).
2. Parse `after` (cursor UUID) + `limit` (default 20, max 100).
3. SET LOCAL `app.current_user_id` = caller's id; `app.current_group_id` = nil-uuid (convention).
4. Keyset query: `SELECT id, device_id, expires_at, created_at FROM sessions`
   `WHERE revoked_at IS NULL AND expires_at > now()`
   `AND (created_at, id) < ($cursor_ts, $cursor_id)` (when cursor is present)
   `ORDER BY created_at DESC, id DESC LIMIT $limit + 1`.
5. Return `MySessionsResponse { items: Vec<SessionSummary>, next_cursor: Option<Uuid> }`.

**DELETE /v1/me/sessions/{session_id}**
1. `Principal` extractor.
2. SET LOCAL both RLS configs.
3. `UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL`.
4. FORCE RLS ensures only the owner's session is updated. Rows-affected = 0 can mean:
   - Session belongs to another user (RLS filtered) → 404
   - Session doesn't exist → 404
   - Session already revoked → 204 (idempotent via separate check)
5. Emit `SessionRevoked` audit event.
6. Return 204 No Content.

Idempotency for DELETE: if rows-affected = 0, do a follow-up SELECT to distinguish
"not found / cross-user" (→ 404) from "already revoked" (→ 204).

## Tech stack

Rust (Axum 0.8), sqlx (Postgres), utoipa (OpenAPI), `garraia_auth::Principal`.

## Design invariants

- NO `unwrap()` outside tests.
- NO SQL string concat — `sqlx::query_as` with positional `$N` params.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` before SQL
  (even though only `current_user_id` is used by the sessions policy — convention).
- NO `refresh_token_hash` in any response body.
- FORCE RLS guarantees user isolation — no explicit `WHERE user_id = $caller` needed
  (defense-in-depth: keep the `WHERE` for clarity, rely on RLS for enforcement).
- No `group_id` required in request headers for this endpoint — sessions are user-scoped,
  not group-scoped. Return 400 if header provided but inconsistent? No — just ignore it.

## Validações pré-plano

- `sessions` columns: `id uuid PK`, `user_id uuid`, `refresh_token_hash text UNIQUE`,
  `device_id text`, `expires_at timestamptz`, `revoked_at timestamptz`,
  `created_at timestamptz` — confirmed from migration 001.
- `sessions_owner_only` FORCE RLS policy on `sessions` — confirmed from migration 007.
- `WorkspaceAuditAction` in `garraia-auth` — add `SessionRevoked` variant.
- Existing me.rs handlers (e.g. `list_my_chats`) use `RestV1FullState` — same state type.
- No `X-Group-Id` required — `Principal.group_id` will be `None`; we use nil-uuid for
  `app.current_group_id` SET LOCAL (matches how `get_me` works for group-agnostic calls).

## Out of scope

- Listing sessions belonging to another user (admin feature, separate epic).
- Revoking ALL sessions at once (logout-everywhere) — user can call DELETE per session.
- Session creation (handled by `POST /v1/auth/login`).
- Device registration / push token management.

## Rollback

Revert the PR. No migration to undo.

## §12 Open questions

None — acceptance criteria fully specified in GAR-866.

## File structure

```
crates/garraia-gateway/src/rest_v1/me.rs        ← list_my_sessions + revoke_my_session handlers + tests
crates/garraia-gateway/src/rest_v1/mod.rs       ← routes in all 3 branches + delete import
crates/garraia-gateway/src/rest_v1/openapi.rs   ← OpenAPI path registration
crates/garraia-auth/src/audit_workspace.rs      ← SessionRevoked variant
plans/0327-gar-866-me-sessions.md               ← this file
plans/README.md                                 ← row 0327 added
```

## M1 Tasks

- [x] T1: Write plan + create Linear issue GAR-866
- [ ] T2: Add `SessionRevoked` to `WorkspaceAuditAction` in `garraia-auth`
- [ ] T3: Implement `list_my_sessions` handler in `me.rs` + 6 unit tests
- [ ] T4: Implement `revoke_my_session` handler in `me.rs` + 6 unit tests
- [ ] T5: Wire routes in `mod.rs` (all 3 branches) + add `delete` to imports
- [ ] T6: Register paths in `openapi.rs`
- [ ] T7: Commit + push + open PR
- [ ] T8: Wait for CI green; fix any failures
- [ ] T9: Squash-merge; mark GAR-866 Done; update ROADMAP + plans/README.md

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `sessions` RLS policy doesn't cover our query shape | Low | Verified policy uses `app.current_user_id = user_id` — same pattern as api_keys |
| Nil-uuid for `app.current_group_id` causes policy error | Low | sessions_owner_only doesn't use group_id at all |
| Idempotent DELETE: already-revoked returns wrong code | Low | Follow-up SELECT distinguishes "revoked" (204) from "not found" (404) |

## Acceptance criteria

1. `GET /v1/me/sessions` → 200 + `MySessionsResponse` (only active sessions, no `refresh_token_hash`).
2. `GET /v1/me/sessions?after=<uuid>&limit=5` → correctly paginated.
3. `DELETE /v1/me/sessions/{session_id}` → 204 (success or already-revoked).
4. `DELETE /v1/me/sessions/{non_existent_id}` → 404.
5. `cargo clippy --workspace --exclude garraia-desktop` green.
6. 12 unit tests (6 per handler) pass.
7. Route wired in all 3 `mod.rs` branches; OpenAPI paths registered.

## Cross-references

- migration 001 — sessions table
- migration 007 — FORCE RLS `sessions_owner_only`
- plan 0245 / GAR-765 — GET /v1/me/chats (pattern reference)
- plan 0325 / GAR-864 — GET /v1/chats/{chat_id}/members/{user_id} (parent plan)
- `POST /v1/auth/logout` — single-session revoke via refresh token (complement)

## Estimativa

1–1.5h. ~250 LOC total (2 handlers + tests + routing + openapi + audit variant).
