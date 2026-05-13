# Plan 0109 — GAR-595: messages slice 6 — GET /v1/messages/{id} + GET /v1/messages/{id}/threads

## Goal

Add two read endpoints to the `/v1` messages surface:

* `GET /v1/messages/{message_id}` — fetch a single message by ID.
* `GET /v1/messages/{message_id}/threads` — list replies posted in the thread
  rooted at this message (plus the `ThreadResponse` metadata if a thread exists).

Together these complete the messages verb set and make thread replies readable
via the API. `POST /v1/messages/{id}/threads` (plan 0058 / GAR-509) can
already create threads, but without a GET there is no way to read the replies.

No schema migration required — all columns (`messages`, `message_threads`)
already exist in migration 004.

---

## Architecture

```
garraia-gateway/src/rest_v1/messages.rs   ← two new handlers + new response types
garraia-gateway/src/rest_v1/mod.rs        ← two new routes
crates/garraia-gateway/tests/rest_v1_messages_get_threads.rs  [NEW]
```

### Endpoint matrix

| Method | Path                                      | Auth                | Happy status |
|--------|-------------------------------------------|---------------------|--------------|
| `GET`  | `/v1/messages/{message_id}`               | Bearer + X-Group-Id | 200 OK       |
| `GET`  | `/v1/messages/{message_id}/threads`       | Bearer + X-Group-Id | 200 OK       |

---

## Tech stack

- Rust / Axum 0.8 — same as rest of `garraia-gateway`
- `sqlx::query` (Postgres, `garraia_app` pool) — all queries parameterised
- `garraia-auth`: `Principal` extractor, `can()`, `Action::ChatsRead`
- `utoipa` for OpenAPI annotations

---

## Design invariants

1. **FORCE RLS** — every handler opens a `pool.begin()` transaction and runs
   `SELECT set_config('app.current_user_id', $1, true)` and
   `SELECT set_config('app.current_group_id', $1, true)` before any SELECT.
2. **Cross-group → 404** — `messages.group_id = X-Group-Id` checked in
   WHERE clause; 0 rows returns 404, never 403, to avoid existence leaks.
3. **Soft-deleted → 404** — `deleted_at IS NULL` required on every message
   fetch. A deleted root message also hides its thread.
4. **No-thread → 200, not 404** — `GET …/threads` with no thread returns
   `{ thread: null, messages: [], next_cursor: null }`. The root message's
   existence is already verified.
5. **`X-Group-Id` required** — same guard as every other message handler.
6. **No audit** — read-only endpoints; no `audit_events` row emitted.
7. **Cursor order** — thread replies paginated `created_at ASC, id ASC`
   (oldest-first, consistent with chat history scroll direction for threads).

---

## Validações pré-plano

- [x] Migration 004 has `messages(thread_id uuid)` and `message_threads` table.
- [x] FORCE RLS on both tables active (migration 007).
- [x] `ThreadResponse` already defined in `messages.rs` (from plan 0058).
- [x] `MessageResponse` already defined in `messages.rs`.
- [x] `Action::ChatsRead` exists (see `can.rs`).
- [x] No `GET /v1/messages/{id}` or `GET /v1/messages/{id}/threads` routes in `mod.rs`.

---

## Out of scope

- Marking a thread as resolved (`PATCH message_threads`)
- Message reactions
- WebSocket push
- `DELETE /v1/messages/{id}/threads` (un-thread)

---

## Rollback

Pure handler addition. Remove the two routes from `mod.rs` and the new
handlers + types from `messages.rs`. No migration to revert.

---

## §12 Open questions

None — schema and authz foundation fully in place.

---

## File structure

```
plans/0109-gar-595-messages-get-single-threads.md       [this file]
crates/garraia-gateway/src/rest_v1/messages.rs          [+2 handlers, +2 types]
crates/garraia-gateway/src/rest_v1/mod.rs               [+2 routes]
crates/garraia-gateway/tests/rest_v1_messages_get_threads.rs  [NEW, 10 scenarios]
```

---

## M1 tasks

### T1 — test scaffold (failing)

- [ ] Create `tests/rest_v1_messages_get_threads.rs` with 10 scenarios (MG1–MG5,
      MT1–MT5) that compile but fail (handlers not yet wired).
- [ ] Commit: `test(messages): T1 — failing test scaffold for GAR-595`

### T2 — `get_message` handler

- [ ] Add `GetMessageResponse` type alias (reuse `MessageResponse`) and
      `get_message` handler in `messages.rs`.
- [ ] Wire `GET /v1/messages/{message_id}` in `mod.rs` (both `Full` and
      `NoAuth` layers).
- [ ] Commit: `feat(messages): T2 — GET /v1/messages/{id} handler (GAR-595)`

### T3 — `list_thread_messages` handler

- [ ] Add `ThreadMessagesResponse` struct and `list_thread_messages` handler
      in `messages.rs`.
- [ ] Wire `GET /v1/messages/{message_id}/threads` in `mod.rs`.
- [ ] Commit: `feat(messages): T3 — GET /v1/messages/{id}/threads handler (GAR-595)`

### T4 — green tests

- [ ] Run `cargo test -p garraia-gateway` locally (or verify in CI).
- [ ] Clippy strict: `cargo clippy ... --no-deps -- -D warnings`.
- [ ] Commit if any style fixes needed.

### T5 — ROADMAP + bookkeeping

- [ ] Update ROADMAP.md §3.4 Chats checklist: add `[x]` rows for both endpoints.
- [ ] Update plans/README.md row 0109: `🔄 In Progress` → `✅ Merged …`.
- [ ] Commit: `docs(plans): T5 bookkeeping — mark plan 0109 merged (GAR-595)`

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| thread_id circular-FK design means replies filtered by thread_id only, no group_id | Low | Medium | Defend-in-depth: explicit `AND group_id = $gid` on messages query |
| Cursor inversion (list_messages is DESC, threads list is ASC) | Low | Low | Explicit `ORDER BY created_at ASC, id ASC` in query |

---

## Acceptance criteria

- `GET /v1/messages/{id}` → 200 + `MessageResponse` for own-group message.
- `GET /v1/messages/{id}` → 404 for wrong-group message.
- `GET /v1/messages/{id}` → 404 for soft-deleted message.
- `GET /v1/messages/{id}/threads` with no thread → 200 + `{ thread: null, messages: [], next_cursor: null }`.
- `GET /v1/messages/{id}/threads` with thread + 2 replies → 200 with replies in ASC order.
- Cross-group message → 404.
- CI 18/18 green.

---

## Cross-references

- Plan 0055 (GAR-507) — messages slice 2: POST + GET /v1/chats/{id}/messages
- Plan 0058 (GAR-509) — messages slice 3: POST /v1/messages/{id}/threads
- Plan 0107 (GAR-592) — messages slice 5: PATCH + DELETE /v1/messages/{id}
- ROADMAP §3.4 "Chats" checklist

---

## Estimativa

Low: ~250 LOC + ~200 LOC tests = ~450 LOC total. 2–3 hours.
