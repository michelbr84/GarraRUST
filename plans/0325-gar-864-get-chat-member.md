# Plan 0325 — GAR-864: GET /v1/chats/{chat_id}/members/{user_id} — fetch single chat member

> **Status:** In Progress
> **Linear:** [GAR-864](https://linear.app/chatgpt25/issue/GAR-864)
> **Branch:** `routine/202606121915-get-chat-member`
> **Parent plan:** 0323

## Goal

Add `GET /v1/chats/{chat_id}/members/{user_id}` — fetch a single chat member's
detail. Closes the CRUD gap for chat members: DELETE and PATCH exist
(plan 0076 / GAR-530 + plan 0227 / GAR-745) but GET single-item was missing.
Mirrors the same gap closed for group members in plan 0286 / GAR-823.

## Architecture

Thin handler in `crates/garraia-gateway/src/rest_v1/chats.rs` following the
established single-resource-fetch pattern:

1. `ChatsRead` permission check.
2. SET LOCAL both `app.current_user_id` AND `app.current_group_id` for FORCE RLS.
3. Verify chat exists in caller's group (404 if not, no 403 leak).
4. SELECT `role, joined_at, muted, last_read_at` from `chat_members` WHERE
   `chat_id = $1 AND user_id = $2`.
5. Return 404 if no row (member not found / cross-group).
6. Return 200 + `ChatMemberDetailResponse` (same struct used by PATCH — reuse, no new type).

No new migration. Uses `chat_members` from migration 004; `muted` + `last_read_at`
added in plan 0227 (GAR-745) — already in main.

## Tech stack

Rust (Axum 0.8), sqlx (Postgres), utoipa (OpenAPI), `garraia_auth::Principal`.

## Design invariants

- NO `unwrap()` outside tests.
- NO SQL string concat — `sqlx::query_as` with positional `$N` params.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` before SQL.
- Cross-group attack fail-closed: `chats WHERE group_id = $caller_group` guard
  before querying `chat_members` — foreign chat_id returns 404, no info leak.
- No audit event on read-only GET.

## Validações pré-plano

- `chat_members` table has `chat_id`, `user_id`, `role`, `joined_at`, `muted`,
  `last_read_at` — confirmed from `patch_chat_member` handler.
- `ChatMemberDetailResponse` struct already defined (line ~2317 in `chats.rs`).
- Route slot `/v1/chats/{chat_id}/members/{user_id}` already registered with
  DELETE + PATCH — only need to add GET handler.
- `patch_chat_member` at openapi.rs:145 — add `get_chat_member` alongside.

## Out of scope

- Listing all members (covered by `list_chat_members` / plan 0076 / GAR-530).
- Modifying member fields (covered by `patch_chat_member` / plan 0227 / GAR-745).
- Cross-chat member enumeration.

## Rollback

Revert the PR. No migration to undo.

## §12 Open questions

None — acceptance criteria fully specified in GAR-864.

## File structure

```
crates/garraia-gateway/src/rest_v1/chats.rs     ← new get_chat_member handler + 6 unit tests
crates/garraia-gateway/src/rest_v1/mod.rs       ← add get(chats::get_chat_member) to 3 branches
crates/garraia-gateway/src/rest_v1/openapi.rs   ← add super::chats::get_chat_member to paths
plans/0324-gar-864-get-chat-member.md           ← this file
plans/README.md                                 ← row 0324 added, row 0323 updated
```

## M1 Tasks

- [x] T1: Write plan + create Linear issue GAR-864
- [ ] T2: Implement `get_chat_member` handler in `chats.rs` + 6 unit tests
- [ ] T3: Wire route in `mod.rs` (all 3 branches)
- [ ] T4: Register in `openapi.rs`
- [ ] T5: Commit + push + open PR
- [ ] T6: Wait for CI green; fix any failures
- [ ] T7: Squash-merge; mark GAR-864 Done
- [ ] T8: Update ROADMAP + plans/README.md row 0323/0324

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `chat_members` columns missing (muted/last_read) | Low | Verified in patch_chat_member |
| Route conflict with existing DELETE/PATCH | Low | Just add `.get()` to existing route tuple |

## Acceptance criteria

1. `GET /v1/chats/{chat_id}/members/{caller_user_id}` → 200 + `ChatMemberDetailResponse` (5 fields).
2. `GET /v1/chats/{chat_id}/members/{non_member_id}` → 404.
3. Archived chat or cross-group chat → 404.
4. Missing `X-Group-Id` → 400; no JWT → 401; not a group member → 403.
5. `cargo clippy --workspace` green. 6 unit tests pass.
6. Route wired in all 3 mod.rs branches; OpenAPI path registered.

## Cross-references

- plan 0076 / GAR-530 — chat member CRUD (list/add/remove)
- plan 0227 / GAR-745 — PATCH /v1/chats/{chat_id}/members/{user_id} (muted/last_read/role)
- plan 0286 / GAR-823 — GET /v1/groups/{group_id}/members/{user_id} (parallel gap)

## Estimativa

0.5–1h. Handler ~60 LOC, tests ~80 LOC, routing ~6 LOC, openapi ~1 LOC.
