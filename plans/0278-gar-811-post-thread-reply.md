# Plan 0278 — GAR-811: POST /v1/threads/{thread_id}/messages (thread reply)

**Issue:** [GAR-811](https://linear.app/chatgpt25/issue/GAR-811)
**Branch:** `routine/202606070621-post-thread-reply`
**Status:** In progress

## 1. Context

Chat threads (introduced in plan 0058 / GAR-509) allow any message to spawn a
dedicated `message_threads` record with a `root_message_id`. Subsequent replies
are ordinary `messages` rows carrying `thread_id = <that thread's uuid>`.

Existing API surface (as of plan 0265 / GAR-798):
- `POST /v1/messages/{message_id}/threads` — create a thread from a message
- `GET /v1/messages/{message_id}/threads` — fetch thread detail via root message
- `GET /v1/chats/{chat_id}/threads` — paginated list of threads in a chat
- `GET /v1/threads/{thread_id}` — fetch single thread by thread id
- `PATCH /v1/threads/{thread_id}` — update thread title / resolved_at

**Missing:** `POST /v1/threads/{thread_id}/messages` — post a reply to an existing
thread. Users can see threads but have no way to add a second (or subsequent)
reply without this endpoint.

## 2. Schema

No new migration required. The `messages` table already has a `thread_id uuid`
column (plain UUID, no FK, migration 004 — avoids circular dependency with
`message_threads.root_message_id`).

```sql
-- Existing columns used:
-- messages.id uuid PK DEFAULT gen_random_uuid()
-- messages.chat_id uuid NOT NULL
-- messages.group_id uuid NOT NULL  (denormalized for RLS)
-- messages.sender_user_id uuid REFERENCES users(id)
-- messages.sender_label text
-- messages.body text CHECK (char_length(body) BETWEEN 1 AND 100000)
-- messages.thread_id uuid  ← set to the thread being replied to
-- messages.created_at timestamptz DEFAULT now()
```

## 3. HTTP contract

```
POST /v1/threads/{thread_id}/messages
Authorization: Bearer <access-token>
X-Group-Id: <group-uuid>
Content-Type: application/json

{
  "body": "string (1..100_000 chars, trimmed)",
  "reply_to_id": "uuid | null",
  "mentions": ["uuid", ...]
}

201 Created
{
  "id": "uuid",
  "chat_id": "uuid",
  "group_id": "uuid",
  "sender_user_id": "uuid",
  "sender_label": "string",
  "body": "string",
  "thread_id": "uuid",
  "reply_to_id": "uuid | null",
  "created_at": "ISO-8601 UTC"
}
```

Error cases:
- `400` — body fails `SendMessageRequest::validate()` (empty / whitespace / >100k)
- `403` — caller lacks `ChatsWrite` capability
- `404` — `thread_id` not found OR belongs to a different group
- `503` — auth unconfigured (fail-soft mode)

## 4. Implementation

### 4.1 `garraia-auth`: audit event

Add `ThreadReplied` variant to `WorkspaceAuditAction` enum in
`crates/garraia-auth/src/audit_workspace.rs`:

```rust
/// A reply was posted to a thread via
/// `POST /v1/threads/{thread_id}/messages` (plan 0278 / GAR-811).
ThreadReplied,
```

`as_str()` arm: `"thread.replied"`.

### 4.2 `garraia-gateway`: handler

New `send_thread_reply` function in `crates/garraia-gateway/src/rest_v1/messages.rs`:

```
1. Extract X-Group-Id header → 400 if missing/invalid
2. Gate: requires ChatsWrite capability → 403 if absent
3. validate() the request body (reuses SendMessageRequest)
4. BEGIN; SET LOCAL app.current_user_id + app.current_group_id
5. SELECT mt.chat_id FROM message_threads mt
   JOIN chats c ON c.id = mt.chat_id
   WHERE mt.id = $1 AND c.group_id = $2
   → 404 if 0 rows (thread not found or cross-group)
6. SELECT display_name FROM users WHERE id = $caller_id
7. INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body, thread_id)
   VALUES ($chat_id, $group_id, $caller_id, $label, $body, $thread_id)
   RETURNING id, created_at
8. INSERT audit_event: ThreadReplied, resource_type="messages",
   resource_id=new_message_id, metadata={thread_id, body_len}
9. COMMIT
10. publish_chat_event(chat_id, ChatEvent::MessageCreated { ... })
11. Return 201 + MessageResponse
```

### 4.3 `garraia-gateway`: route wiring

Add route in all three branches of `build_rest_v1_router` in
`crates/garraia-gateway/src/rest_v1/mod.rs`:

- **Mode 1 (full auth):** `post(messages::send_thread_reply)`
- **Mode 2 (fail-soft):** `post(unconfigured_handler)` — 503
- **Mode 3 (no auth):** `post(unconfigured_handler)` — 503

## 5. Tests

Six unit tests in `messages.rs` (no DB required):

| Test | Scenario |
|------|----------|
| `send_thread_reply_accepts_valid_body` | body = "hello" → validate() ok |
| `send_thread_reply_rejects_empty_body` | body = "" → validate() Err |
| `send_thread_reply_rejects_whitespace_body` | body = "   " → validate() Err |
| `send_thread_reply_rejects_body_over_100k_chars` | body = 100_001 chars → Err |
| `send_thread_reply_accepts_body_at_100k_chars` | body = 100_000 chars → ok |
| `send_thread_reply_body_trimmed_in_validate` | body = " hi " → validate() ok |

## 6. Files changed

| File | Change |
|------|--------|
| `crates/garraia-auth/src/audit_workspace.rs` | +`ThreadReplied` variant + `as_str` arm + tests |
| `crates/garraia-gateway/src/rest_v1/messages.rs` | +`send_thread_reply` handler + 6 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | +route in all 3 branches |
| `plans/README.md` | +row for plan 0278 |
| `ROADMAP.md` | mark `POST /v1/threads/{thread_id}/messages` done |

## 7. Security

- Cross-group isolation enforced via JOIN: `message_threads → chats → group_id`
  — a caller cannot reply to a thread in another group.
- `thread_id` written to `messages.thread_id` is the same UUID that was
  verified to belong to the caller's group in step 5.
- Body text is user-generated PII — only `body_len` is carried in the audit
  record (no body text in audit metadata).
- No new DB role or RLS policy needed: existing `messages` RLS (`FORCE`) +
  `garraia_app` role covers inserts with the set-local tenant context.
