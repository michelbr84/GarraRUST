# Plan 0211 — GAR-726: REST /v1 search slice 12 — `types=threads` message thread title FTS

## Goal

Add `types=threads` to `GET /v1/search`, enabling callers to search message thread
titles using full-text search on the `message_threads` table.
Twelfth slice of the `/v1` unified search surface (Fase 3.4 / §3.9 "Busca unificada").

## Architecture

No new migration. `message_threads` table already exists since migration 004
(`004_chats_and_messages.sql`). FORCE RLS via `message_threads_through_chats` policy
(migration 007) scopes results through `chats → groups`.

Query pattern:
```sql
SELECT mt.id,
       ts_rank(
           to_tsvector('simple', mt.title),
           websearch_to_tsquery('simple', $1)
       )::real AS score,
       mt.title,
       c.group_id,
       mt.chat_id,
       mt.created_by,
       mt.created_at
FROM   message_threads mt
JOIN   chats c ON c.id = mt.chat_id
WHERE  to_tsvector('simple', mt.title) @@ websearch_to_tsquery('simple', $1)
  AND  c.group_id = $2
  AND  mt.title IS NOT NULL
ORDER BY score DESC, mt.created_at DESC, mt.id DESC
LIMIT $3
```

`title IS NOT NULL` guard: threads with no title have no searchable content.
`c.group_id = $2` is defense-in-depth alongside the RLS JOIN policy.

## Tech stack

- Crate: `garraia-gateway` (only changed file: `src/rest_v1/search.rs`)
- No new dependencies, no migration, no config change.

## Design invariants

1. Group scope only — `message_threads` are scoped via `chats → groups`; no user/chat scope.
2. `title IS NOT NULL` guard — null titles produce no searchable vector.
3. `sender_user_id` = `created_by` (thread author).
4. `chat_id` = `mt.chat_id` (thread belongs to a chat — explicitly returned).
5. `kind` = null (no useful discriminant like task status or chat type).
6. `excerpt` = `title` (the only searchable text field).
7. JOIN through `chats` (not a direct `group_id` column on `message_threads`).

## Validações pré-plano

- [x] `message_threads` table exists since migration 004.
- [x] `message_threads_through_chats` FORCE RLS policy in migration 007 scopes via JOIN.
- [x] `message_threads.title` is nullable text — need `IS NOT NULL` guard.
- [x] `message_threads.created_by` FK to `users.id` — consistent with other row types.
- [x] `message_threads.chat_id` FK to `chats.id` — enables GROUP scoping via JOIN.
- [x] Next plan number is 0209 (0200-0208 used by slice 10 + health runs 38-44 + slice 11).
- [x] GAR-726 Linear issue exists and is In Progress.

## Out of Scope

- New migration (not needed — schema already in place).
- `from_date`/`to_date`/`author_id` filters for threads (future slice).
- GIN index on `message_threads.title` (runtime FTS sufficient for initial slice).
- Resolved threads filter (`resolved_at IS NULL`) — not scoped here.

## Rollback

Revert the diff to `search.rs`. No migration to roll back.

## File Structure

```
crates/garraia-gateway/src/rest_v1/search.rs  ← only file changed
plans/0209-gar-726-search-slice12-threads.md  ← this file
plans/README.md  ← row added
ROADMAP.md  ← checklist row added
```

## M1 Tasks

- [x] T1: Add `Thread` variant to `SearchResultType` enum.
- [x] T2: Add `include_threads: bool` field to `ValidatedSearch` struct.
- [x] T3: Parse `"threads"` in the `types` loop; update "unknown type" error message; update "at least one" guard.
- [x] T4: Add group-scope validation for threads (chat/user scope → 400).
- [x] T5: Add `ThreadSearchRow` struct.
- [x] T6: Add `fetch_threads()` async function (JOIN through chats for group_id, title IS NOT NULL guard).
- [x] T7: Add `if validated.include_threads { ... }` handler block.
- [x] T8: Update module doc comment (plans list, scope range, slice 12 paragraph).
- [x] T9: Update handler `#[utoipa::path]` error matrix doc comment.
- [x] T10: Update `SearchQuery.types` doc comment to include `threads`.
- [x] T11: Add 6 unit tests.
- [x] T12: Update ROADMAP.md checklist + plans/README.md row.

## Risk register

| Risk | Mitigation |
|------|------------|
| `title IS NULL` producing empty tsvector match | `title IS NOT NULL` guard in WHERE |
| Cross-group leakage | Explicit `c.group_id = $2` + FORCE RLS via JOIN to chats |
| Chat scope rejected without clear message | Explicit 400 at parse_and_validate |

## Acceptance criteria

- `cargo check -p garraia-gateway --features test-helpers` → 0 errors.
- `cargo clippy -p garraia-gateway --features test-helpers --no-deps -- -D warnings` → clean.
- 6 unit tests pass: `types_threads_group_scope_accepted`, `types_threads_chat_scope_rejected`, `types_threads_user_scope_rejected`, `types_threads_and_chats_group_scope_accepted`, `types_threads_and_task_lists_group_scope_accepted`, `types_all_nine_group_scope_accepted`.
- All prior tests (76) continue to pass (82 total after this slice).
- No new migration.

## Cross-references

- Parent issue: [GAR-726](https://linear.app/chatgpt25/issue/GAR-726)
- Epic: GAR-WS-SEARCH / Fase 3.4 § "Busca unificada"
- Builds on: plan 0208 / GAR-721 (slice 11, task_lists)
- Migration source: `crates/garraia-workspace/migrations/004_chats_and_messages.sql`
- RLS policy source: `crates/garraia-workspace/migrations/007_row_level_security.sql:114-126`

## Estimativa

- LOC: ~120 (struct + fn + handler block + tests)
- Tempo: 30 min
