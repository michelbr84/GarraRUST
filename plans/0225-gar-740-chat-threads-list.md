# Plan 0225 — GAR-740: REST /v1 chats slice 6 — `GET /v1/chats/{chat_id}/threads`

## Goal

Add `GET /v1/chats/{chat_id}/threads` — cursor-paginated list of message threads
in a chat. Closes §3.6 "Threads (entidade dedicada)" in the ROADMAP.

## Scope

- `GET /v1/chats/{chat_id}/threads?after=<uuid>&limit=<n>&include_resolved=<bool>`
- Returns `ChatThreadsResponse { items: Vec<ChatThreadSummary>, next_cursor: Option<Uuid> }`
- `ChatThreadSummary`: `id`, `chat_id`, `root_message_id`, `title`, `created_by`,
  `created_at`, `resolved_at`, `reply_count`
- Cross-tenant guard: verify `chat_id` in `chats` matches caller's group → 404
- `include_resolved=false` (default): only unresolved threads (`resolved_at IS NULL`)
- `include_resolved=true`: all threads (resolved + unresolved)
- `reply_count` = COUNT of `messages.thread_id = thread.id AND deleted_at IS NULL`
- Keyset cursor on `(created_at DESC, id DESC)` — same pattern as `list_messages`
- No new migration — `message_threads` schema in migration 004, FORCE RLS in migration 007

## Design invariants

- `SET LOCAL app.current_user_id` AND `app.current_group_id` before any query (FORCE RLS)
- 404 (not 403) for cross-tenant attempts — no cross-tenant leak
- `reply_count` uses correlated subquery (avoids GROUP BY complexity with cursor)
- `limit` clamped to 1..50; default 20
- cursor subquery: `(mt.created_at, mt.id) < (SELECT created_at, id FROM message_threads WHERE id=$cursor AND chat_id=$1)`
  — returns empty result if cursor doesn't exist (safe fallback, same as messages)

## Implementation

### `chats.rs` additions

1. `ListChatThreadsQuery` struct (`after: Option<Uuid>`, `limit: Option<u32>`, `include_resolved: Option<bool>`)
2. `ChatThreadSummary` struct (7 fields above)
3. `ChatThreadsResponse` struct (`items`, `next_cursor`)
4. `ThreadListRow` type alias for sqlx FromRow
5. `list_chat_threads` handler with utoipa doc
6. Unit tests (5 tests)

### `mod.rs` addition

Wire `.route("/v1/chats/{chat_id}/threads", get(chats::list_chat_threads))` in
all three router branches (full, fail-soft, no-auth stub).

## Tests (5 unit)

| ID | Name | Expectation |
|----|------|-------------|
| U1 | `list_threads_limit_default` | None → 20 |
| U2 | `list_threads_limit_clamped` | 0 → 1, 100 → 50 |
| U3 | `list_threads_include_resolved_default` | None → false |
| U4 | `list_threads_include_resolved_true` | "true" → true |
| U5 | `list_threads_limit_max_boundary` | 50 → 50 |

## Risks

- Correlated subquery for `reply_count` may be slow on chats with thousands of threads.
  Acceptable for MVP; a materialized counter column can be added later.
- RLS on `messages` inside the correlated subquery: FORCE RLS applies when
  `app.current_user_id` and `app.current_group_id` are set — they are set in the
  same transaction before this query, so counts are correctly tenant-scoped.

## Tasks

- [x] T1: Add `ListChatThreadsQuery`, `ChatThreadSummary`, `ChatThreadsResponse` DTOs
- [x] T2: Implement `list_chat_threads` handler (cross-tenant check + two SQL paths)
- [x] T3: Add route in `mod.rs` (full + fail-soft + no-auth branches)
- [x] T4: Add 5 unit tests
- [x] T5: Update ROADMAP.md + plans/README.md
- [x] T6: Commit + push on `routine/202605291015-chat-threads-list`
- [x] T7: Open PR, await CI green, merge
- [x] T8: Mark GAR-740 Done in Linear
