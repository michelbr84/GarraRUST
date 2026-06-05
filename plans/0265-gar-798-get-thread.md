# Plan 0265 — GAR-798: GET /v1/threads/{thread_id}

**Status:** In Progress
**Issue:** [GAR-798](https://linear.app/chatgpt25/issue/GAR-798)
**Branch:** `routine/202506051820-get-thread`
**Epic:** GAR-509 (threads slice 3 / chats API)
**Labels:** `epic:ws-chat`, `epic:ws-api`

---

## Goal

Add `GET /v1/threads/{thread_id}` — returns the full `ThreadDetailResponse` for a
single message thread by ID. Closes the CRUD gap: the route
`/v1/threads/{thread_id}` is already half-wired via `.patch(chats::patch_thread)`
but lacks a GET handler. Mobile deep-links that store a `thread_id` (e.g. from a
push notification) cannot hydrate thread metadata without scanning the full
`GET /v1/chats/{chat_id}/threads` list.

---

## Architecture

Single new handler `get_thread` in
`crates/garraia-gateway/src/rest_v1/chats.rs`, placed immediately before the
existing `patch_thread` (they share the same URL path).

One SQL query (no second round-trip):

```sql
SELECT mt.id, mt.chat_id, mt.root_message_id, mt.title,
       mt.created_by, mt.resolved_at, mt.created_at
FROM   message_threads mt
JOIN   chats c ON c.id = mt.chat_id
WHERE  mt.id = $1 AND c.group_id = $2
```

The JOIN on `chats` provides the cross-tenant guard: threads whose chat belongs to
a different group silently become 404 (no existence leak).

---

## Tech stack

- Rust / Axum 0.8 / sqlx (Postgres)
- `garraia-auth`: `Action::ChatsRead` (existing)
- `garraia-workspace`: `message_threads` (migration 004, no change)
- utoipa for OpenAPI annotation

---

## Design invariants

- Returns 404 (not 403) for cross-group threads — no existence leak.
- RLS context (`app.current_user_id` + `app.current_group_id`) SET LOCAL inside
  the tx (FORCE RLS on `message_threads` via `message_threads_through_chats`
  policy, identical to `patch_thread`).
- Response shape is `ThreadDetailResponse` — the struct already used by
  `patch_thread`, no new type introduced.
- No audit event — read-only endpoint.
- No migration — `message_threads` table exists since migration 004.

---

## Validações pré-plano

- [x] `ThreadDetailResponse` struct already exists in `chats.rs` (line ~2007).
- [x] Route `/v1/threads/{thread_id}` already has `.patch(...)` in all 3 `mod.rs`
  branches; adding `.get(...)` is additive.
- [x] `message_threads` has `root_message_id` column (migration 004, confirmed in
  `patch_thread` line ~2095).
- [x] No existing `get_thread` function — zero naming conflict.

---

## Out of scope

- `DELETE /v1/threads/{thread_id}` (needs schema change: `deleted_at` column).
- Reply pagination inside this endpoint (covered by existing
  `GET /v1/messages/{message_id}/threads`).
- `reply_count` field in `ThreadDetailResponse` (would add a correlated subquery;
  deferred to a future enrichment slice).

---

## Rollback

The change is purely additive (new handler + route wiring). Reverting means
removing `get(chats::get_thread)` from the three `.route(...)` calls and deleting
the handler. No migration to undo.

---

## File Structure

```
crates/garraia-gateway/src/rest_v1/
  chats.rs        ← add get_thread handler + utoipa::path annotation
  mod.rs          ← add .get(chats::get_thread) in 3 branches
  openapi.rs      ← add super::chats::get_thread to paths + no new schema needed
```

---

## M1 — Implement GET /v1/threads/{thread_id}

- [ ] T1: Add `get_thread` handler in `chats.rs` (before `patch_thread`).
- [ ] T2: Wire `get(chats::get_thread)` in all 3 `mod.rs` branches.
- [ ] T3: Add `super::chats::get_thread` to `openapi.rs` paths list.
- [ ] T4: Add 6+ unit tests in `chats.rs` test module.
- [ ] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- [ ] T6: Commit, push, open PR.

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Cross-group leak via JOIN | Low | JOIN on `c.group_id = $2` + FORCE RLS prevents it |
| Naming conflict with existing `get_thread` | None | Grep confirms no such function |
| OpenAPI duplicate path | None | `patch_thread` already registered; utoipa merges GET+PATCH on same path |

---

## Acceptance criteria

- `GET /v1/threads/{id}` returns 200 + `ThreadDetailResponse` for a known thread
  in the caller's group.
- Returns 404 for an unknown thread or a thread in a different group.
- `cargo clippy --workspace` green.
- 6+ unit tests pass (serialization, cross-group 404, nil created_by, nil title,
  resolved thread, nil UUID round-trip).
- CI 20/20 green.
- ROADMAP.md §3.4 chats checklist updated.

---

## Cross-references

- Migration 004: `message_threads` schema
- Plan 0058 / GAR-509: create thread
- Plan 0225 / GAR-740: list threads in chat
- Plan 0227 / GAR-745: patch thread
- Plan 0261 / GAR-790: me/threads inbox

---

## Estimativa

- LOC: ~100 (handler) + ~20 (wiring) + ~15 (openapi) + ~60 (tests) ≈ 195 LOC total
- Time: ~30 min
