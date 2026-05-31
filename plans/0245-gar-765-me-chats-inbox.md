# Plan 0245 — GET /v1/me/chats (caller-scoped chat inbox)

**Issue:** [GAR-765](https://linear.app/chatgpt25/issue/GAR-765)
**Slice:** Chats slice 12 / `/v1/me` namespace extension
**Branch:** `routine/202605311818-me-chats-inbox`
**Estimativa:** ~200 LOC, ~2h

---

## Goal

Add `GET /v1/me/chats` — a cursor-paginated inbox returning all non-archived chats where the
authenticated caller holds a `chat_members` row, scoped to a specific group.

Follows the established pattern of `GET /v1/me/mentions` (plan 0237) and
`GET /v1/me/tasks` (plan 0242).

---

## Architecture

```
GET /v1/me/chats?group_id=<uuid>[&after=<chat_id>&limit=<n>&type=channel|dm|thread]
  │
  ├── Principal extractor → user_id
  ├── BEGIN tx (app pool)
  ├── SET LOCAL app.current_user_id
  ├── SET LOCAL app.current_group_id
  ├── SELECT c.id, c.group_id, c.name, c.type, cm.role, cm.joined_at,
  │          cm.muted, cm.last_read_at, c.archived_at
  │   FROM chat_members cm
  │   JOIN chats c ON cm.chat_id = c.id
  │   WHERE cm.user_id = $1
  │     AND c.group_id = $2
  │     AND c.archived_at IS NULL
  │     [AND c.type = $3]
  │     [cursor: AND (cm.joined_at, cm.chat_id) < (subquery)]
  │   ORDER BY cm.joined_at DESC, cm.chat_id DESC
  │   LIMIT $N
  ├── COMMIT
  └── 200 JSON { items, next_cursor }
```

---

## Tech stack

- Rust / Axum 0.8 (`garraia-gateway`)
- `sqlx` async Postgres (non-macro `query_as` — consistent with plan 0242 pattern)
- utoipa OpenAPI annotations
- No new migration — `chats` + `chat_members` from migration 004; RLS from migration 007

---

## Design invariants

1. **RLS enforced in-tx**: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id`
   both required before any query — `chats_group_isolation` + `chat_members_through_chats`
   (migration 007) filter automatically.
2. **group_id required**: same as tasks/mentions — can't omit because RLS needs it.
3. **Non-archived only**: `c.archived_at IS NULL` by default; no flag to include archived.
4. **Keyset cursor on (joined_at DESC, chat_id DESC)**: stable even under concurrent inserts.
5. **No PII in cursor**: cursor is just the `chat_id` UUID of the last item.
6. **type filter validated**: accepted values: `channel`, `dm`, `thread`. Any other → 400.
7. **No unwrap() in production code**.
8. **No SQL string concat**: all params via `.bind()`.

---

## Validações pré-plano

- [x] `chat_members` table has `user_id`, `chat_id`, `role`, `joined_at`, `muted`, `last_read_at` (migration 004)
- [x] `chats` table has `id`, `group_id`, `name`, `type`, `archived_at` (migration 004)
- [x] RLS policies `chats_group_isolation` + `chat_members_through_chats` exist (migration 007)
- [x] Pattern established in `me.rs` via `list_my_tasks` + `list_my_mentions`
- [x] No competing issue found in Linear search

---

## Out of scope

- Including archived chats (separate slice if ever needed)
- Unread count (requires counting messages vs `last_read_at` — deferred)
- Cross-group chat list (blocked by RLS needing `app.current_group_id`)
- WebSocket / SSE stream of chat updates

---

## Rollback

PR is non-destructive (additive handler + route). Rolling back means removing the route from
`mod.rs` and the handler block from `me.rs`. No migration to reverse.

---

## §12 Open questions

None — pattern is fully established.

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs               ← add ListMyChatsQuery, ChatMembershipSummary, MyChatsMembershipResponse,
                         list_my_chats handler
  mod.rs              ← add .route("/v1/me/chats", get(me::list_my_chats))
```

---

## M1 Tasks

- [ ] **T1** — Write unit tests for `list_my_chats` (query validation, type filter, cursor)
- [ ] **T2** — Implement `ListMyChatsQuery`, `ChatMembershipSummary`, `MyChatsMembershipResponse`
- [ ] **T3** — Implement `list_my_chats` handler with 4-branch cursor×type query
- [ ] **T4** — Register route `/v1/me/chats` in `mod.rs`
- [ ] **T5** — `cargo clippy` clean; `cargo test -p garraia-gateway` green
- [ ] **T6** — Update `ROADMAP.md` §3.4 chats + §3.6 checklist
- [ ] **T7** — Update `plans/README.md` row
- [ ] **T8** — Update `TODO.md`

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| RLS policy returns 0 rows if group_id wrong | Low | Unit test with cross-group guard |
| Cursor subquery returns NULL → safe empty result | Covered | Same pattern as list_my_tasks |
| Type filter SQL injection | None | Always via `.bind()` with validation guard |

---

## Acceptance criteria

1. `GET /v1/me/chats?group_id=<valid>` → 200 with caller's chats in that group, newest-joined first.
2. `?type=channel` filters to channels only; `?type=invalid` → 400.
3. Cursor pagination: `?after=<chat_id>` returns the next page correctly.
4. Caller NOT in group → empty list (RLS returns 0 rows; not a 403 — consistent with tasks/mentions).
5. Archived chats excluded.
6. 7+ unit tests green.

---

## Cross-references

- Plan 0237 (GAR-755) — `GET /v1/me/mentions` (model)
- Plan 0242 (GAR-763) — `GET /v1/me/tasks` (model)
- Migration 004 — `chats` + `chat_members` schema
- Migration 007 — RLS policies (`chats_group_isolation`, `chat_members_through_chats`)
- ROADMAP §3.4 Chats + §3.6 Chat compartilhado

---

*Created 2026-05-31 (America/New_York)*
