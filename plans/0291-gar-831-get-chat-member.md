# Plan 0291 — GET /v1/chats/{chat_id}/members/{user_id} (GAR-831)

**Fase:** 3.6 — Chat compartilhado
**Epic:** `epic:ws-api`
**Linear:** [GAR-831](https://linear.app/chatgpt25/issue/GAR-831)
**Status:** In Progress
**Estimativa:** ~130 LOC + 6 unit tests

---

## Goal

Add `GET /v1/chats/{chat_id}/members/{user_id}` — fetch a single chat member by user UUID.
Closes the CRUD gap left by plan 0227 (POST/GET-list/PATCH/DELETE existed; single-resource
GET was absent).

---

## Architecture

- Handler `get_chat_member` in `crates/garraia-gateway/src/rest_v1/chats.rs`
- Route added via `.get(chats::get_chat_member)` to existing `/v1/chats/{chat_id}/members/{user_id}` entry (previously DELETE + PATCH only)
- All three router branches in `mod.rs` updated (full, auth-only stub, no-auth stub)
- OpenAPI path registered in `openapi.rs`
- No new migration — `chat_members` schema is complete (migration 004 + FORCE RLS)

## Design invariants

1. Auth gate: `Action::ChatsRead` — same as all read paths in this module.
2. FORCE RLS: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` inside tx.
3. Chat existence check first (404 if archived or cross-group — no cross-tenant leak).
4. 404 if `target_user_id` is not a member of the chat (no existence leak).
5. Reuses existing `ChatMemberDetailResponse` struct (same as `patch_chat_member`).

---

## Out of scope

- `display_name` / `avatar_url` join from `users` table (enrichment for future plan)
- Pagination (single-item endpoint)

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  chats.rs     +~130 LOC (handler + utoipa doc + 6 unit tests)
  mod.rs       +3 LOC per branch × 3 branches (wire GET)
  openapi.rs   +1 LOC (add get_chat_member path)
ROADMAP.md     +1 line tick
plans/README.md +1 row
```

---

## M1 tasks

- [x] Write plan file
- [x] Implement `get_chat_member` handler in `chats.rs`
- [x] Wire GET route in all 3 branches of `mod.rs`
- [x] Register path in `openapi.rs`
- [x] 6 unit tests (response struct serialization + field coverage)
- [x] Update `ROADMAP.md` (tick GAR-831)
- [x] Update `plans/README.md` (add row 0291)
- [x] `cargo check` + `cargo clippy` clean
- [ ] Commit + push
- [ ] PR + green CI
- [ ] Squash-merge

---

## Acceptance criteria

- `GET /v1/chats/{chat_id}/members/{user_id}` → 200 + `ChatMemberDetailResponse` for valid member
- 404 for unknown `user_id` (non-member)
- 404 for archived chat or cross-group chat
- 400 for missing X-Group-Id header
- 401 for missing/invalid JWT
- 403 for caller without `ChatsRead`
- Route wired in all 3 router branches
- 6 unit tests pass

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| RLS policy filters to 0 rows for cross-group | Desired behavior — chat existence check returns 404 first |
| `ChatMemberDetailResponse` struct change breaks PATCH | Struct is shared; any field changes affect both |

---

## Cross-references

- Plan 0227 (GAR-745) — `PATCH /v1/chats/{id}/members/{uid}` (same struct, same route prefix)
- Plan 0286 (GAR-823) — `GET /v1/groups/{id}/members/{user_id}` (same single-item GET pattern)
- Plan 0285 (GAR-820) — `GET /v1/groups/{id}/files/{file_id}/versions/{version}` (same pattern)
- Migration 004 — `chat_members` schema
