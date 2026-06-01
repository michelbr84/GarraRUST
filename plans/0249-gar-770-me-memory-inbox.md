# Plan 0249 — GAR-770: GET /v1/me/memory (caller-scoped personal memory inbox)

**Issue:** [GAR-770](https://linear.app/chatgpt25/issue/GAR-770)
**Branch:** `routine/202506010615-me-memory-inbox`
**Epic:** `epic:ws-api` + `epic:ws-memory`
**Estimate:** ~150 LOC

---

## Goal

Add `GET /v1/me/memory` — a cursor-paginated inbox of the authenticated caller's
personal (scope_type='user') memory items. Rounds out the `GET /v1/me/*` inbox
series (tasks: plan 0242, chats: plan 0245, files: plan 0246).

---

## Architecture

No new migration needed. Reuses:
- `memory_items` table (migration 005): `id`, `kind`, `content`, `ttl_expires_at`,
  `created_at`, `created_by`, `deleted_at`, `scope_type = 'user'`, `group_id IS NULL`
- `pinned_at` column (migration 015)
- FORCE RLS `memory_items_group_or_self` policy (migration 007):
  Branch 2 fires for user-scope: `group_id IS NULL AND created_by = app.current_user_id`

RLS context: always SET LOCAL `app.current_user_id` + `app.current_group_id`.
For user-scope memories `group_id IS NULL`, so `app.current_group_id = Uuid::nil()` is
safe (branch 1 requires `group_id IS NOT NULL`; nil UUID won't match branch 1 either).

---

## Tech stack

- Axum 0.8 — existing handler pattern
- `utoipa` — OpenAPI annotation
- `sqlx::query_as` + 4-branch static SQL (cursor × kind filter)
- No new Cargo deps

---

## Design invariants

- scope_type is always `'user'`; no cross-scope leakage.
- Excludes soft-deleted items (`deleted_at IS NULL`).
- Excludes expired items (`ttl_expires_at IS NULL OR ttl_expires_at > now()`).
- `content_preview` = first 200 chars of `content` — avoids bulk PII exposure.
- `limit` clamped to [1, 100], default 50.
- Keyset pagination on `(created_at DESC, id DESC)`.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` (per CLAUDE.md §12).

---

## Out of scope

- Group or chat-scoped memories (use `GET /v1/memory?scope_type=...`).
- Filtering by `pinned` status (future slice).
- Filtering by `sensitivity` (future slice).
- Memory creation/update (existing endpoints).

---

## Rollback

Revert the `me.rs`, `mod.rs`, and `openapi.rs` changes. No migration to undo.

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs          — ListMyMemoryQuery + MyMemorySummary + MyMemoryResponse
                   + list_my_memory handler + 8 unit tests
  mod.rs         — route registration in all 3 mode branches
                   + fix missing /v1/me/chats stubs in mode 2+3
  openapi.rs     — import + register MyMemorySummary, MyMemoryResponse,
                   list_my_memory path

plans/README.md  — new row for plan 0249
ROADMAP.md       — [x] GET /v1/me/memory entry in §3.4
TODO.md          — update
```

---

## M1 tasks

- [x] T1: Implement types + handler in `me.rs`
- [x] T2: Register routes in `mod.rs` (mode 1 + fix mode 2 + mode 3 stubs)
- [x] T3: Register in `openapi.rs`
- [x] T4: 8 unit tests in `me.rs` `mod tests {}`
- [x] T5: Update `plans/README.md` + `ROADMAP.md` + `TODO.md`
- [x] T6: Commit + push + CI green

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| RLS leaks user-scope items of other users | `created_by = $1` in WHERE + RLS policy branch 2 both filter |
| PII in content exposed in bulk listing | `content_preview` = LEFT(content, 200) only |
| Expired items returned | `ttl_expires_at IS NULL OR ttl_expires_at > now()` predicate |

---

## Acceptance criteria

- `GET /v1/me/memory` returns only caller's personal memory (scope_type='user', created_by=caller).
- Soft-deleted and expired items are excluded.
- Cursor pagination works correctly.
- `kind` filter works (optional).
- 8 unit tests green.
- Route registered in all 3 mod.rs branches.
- OpenAPI spec includes the endpoint.
- All CI checks green.

---

## Cross-references

- Plan 0242 (GAR-763): GET /v1/me/tasks — same pagination pattern
- Plan 0246 (GAR-767): GET /v1/me/files — 4-branch cursor × optional-filter SQL
- Memory table: migration 005 + 015
- RLS: migration 007 `memory_items_group_or_self` dual policy

---

## Estimativa

~150 LOC Rust + 8 unit tests. No migration. Pattern is established.
