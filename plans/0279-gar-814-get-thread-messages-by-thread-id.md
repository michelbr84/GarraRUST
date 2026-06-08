# Plan 0279 — GAR-814: GET /v1/threads/{thread_id}/messages

**Issue:** [GAR-814](https://linear.app/chatgpt25/issue/GAR-814)
**Branch:** `routine/202606071900-get-thread-messages`
**Status:** In progress

## 1. Context

The threads API currently exposes `GET /v1/messages/{message_id}/threads` to list
replies, requiring callers to know the `root_message_id`. When a client discovers
a thread via `GET /v1/chats/{chat_id}/threads` it only has the `thread_id`, so
reading replies requires two hops. This slice adds a direct path.

## 2. Schema

No new migration. The query joins `message_threads → chats` for cross-group
isolation (same as `send_thread_reply`).

## 3. HTTP contract

```
GET /v1/threads/{thread_id}/messages?after=<uuid>&limit=<n>
Authorization: Bearer <access-token>
X-Group-Id: <group-uuid>

200 OK
{
  "thread": {
    "id": "uuid",
    "chat_id": "uuid",
    "root_message_id": "uuid",
    "title": "string | null",
    "created_by": "uuid",
    "created_at": "ISO 8601"
  },
  "messages": [MessageSummary],
  "next_cursor": "uuid | null"
}

404  thread not found or not in caller's group (no existence leak)
400  missing X-Group-Id or invalid limit
401  missing / invalid JWT
403  caller not a member of the group
```

## 4. Design invariants

- Cross-group isolation: `JOIN message_threads mt JOIN chats c ON c.id = mt.chat_id WHERE c.group_id = $caller_group_id` + FORCE RLS `set_config`.
- 0 rows on thread lookup → 404 (no existence leak).
- `limit` default 50, max 100, < 1 → 400.
- `next_cursor` = last message's `id` when `messages.len() == limit`, else `null`.
- Reuses existing `ThreadMessagesResponse`, `ThreadResponse`, `MessageSummary` types.
- Capability gate: `ChatsRead`.

## 5. Files changed

| File | Change |
|------|--------|
| `crates/garraia-gateway/src/rest_v1/messages.rs` | +`get_thread_messages_by_id` handler (~140 LOC) + 5 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | add `get(messages::get_thread_messages_by_id)` to all 3 router branches |
| `plans/0279-gar-814-get-thread-messages-by-thread-id.md` | this file |
| `plans/README.md` | +row for plan 0279 |
| `ROADMAP.md` | mark endpoint done in §3.4 chats checklist |

## 6. Tasks

- [x] T1: Write handler `get_thread_messages_by_id` in `messages.rs` (TDD)
- [x] T2: Wire route in `mod.rs` (all 3 branches)
- [x] T3: 5+ unit tests green
- [x] T4: `cargo check -p garraia-gateway` clean
- [x] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` clean
- [x] T6: Update ROADMAP + plans/README

## 7. Acceptance criteria

- `cargo check -p garraia-gateway` clean.
- `cargo clippy --workspace ...` clean.
- 5+ unit tests for limit parsing and next_cursor logic.
- ROADMAP §3.4 + plans/README updated.
- CI green (all 16+ checks pass).
