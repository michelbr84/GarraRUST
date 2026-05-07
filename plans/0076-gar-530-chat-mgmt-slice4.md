# Plan 0076 — GAR-530: Chat Management Slice 4

> **Status:** ✅ Merged 2026-05-07 via PR #181 (`b48a356`)
> **Linear:** [GAR-530](https://linear.app/chatgpt25/issue/GAR-530)
> **Branch:** `routine/202505070024-chat-mgmt-slice4`
> **Epic:** GAR-WS-CHAT / Fase 3.4

---

## 1. Goal

Complete individual chat management and chat member CRUD on the
`garraia_app` RLS-enforced pool. Slice 1 (plan 0054) landed
`POST/GET /v1/groups/{group_id}/chats`. This slice adds the
single-resource operations and member management needed for real clients.

---

## 2. Architecture

Extends `crates/garraia-gateway/src/rest_v1/chats.rs` with six handlers
following the established plan 0054/0056 pattern:
- Tenant context via `set_config()` (parameterized, tx-scoped)
- Cross-group 404 guard (not 403) on individual ops
- Audit metadata STRUCTURAL only (no name/topic/content PII)
- `garraia_auth::WorkspaceAuditAction` extended with 4 new variants

---

## 3. Tech Stack

- Rust + Axum 0.8 + sqlx + garraia_auth + utoipa
- Postgres 16 + pgvector, migrations 004 + 007
- Integration tests: tokio-test + testcontainers

---

## 4. Design Invariants

1. Both `app.current_user_id` AND `app.current_group_id` set before any
   RLS table access.
2. Individual chat ops: look up `group_id` from `chats` and verify it
   matches `principal.group_id` — 404 on mismatch (never 403).
3. Archived (`archived_at IS NOT NULL`) chats return 404 to all ops.
4. Cannot remove last owner from a chat — 409 guard.
5. `add_chat_member` verifies the target user is a group member before
   adding to chat (prevents silent cross-group member addition).
6. Audit metadata carries only structural data (counts, role, boolean
   flags) — never name/body/topic content.

---

## 5. Validações pré-plano

- [x] Schema `chats` + `chat_members` confirmed in migration 004
- [x] RLS policy `chats_group_isolation` confirmed in migration 007:89-94
- [x] RLS policy `chat_members_through_chats` (JOIN) confirmed in 007:99-112
- [x] `Action::ChatsRead`, `Action::ChatsWrite`, `Action::MembersManage`
      all exist in `garraia-auth`
- [x] Plan 0054 handlers established set_config pattern

---

## 6. Out of Scope

- DM chat creation (requires two-member UNIQUE constraint logic)
- WebSocket real-time member events
- `last_read_at` update (read receipts)
- Cursor pagination on members list (bounded by group size)

---

## 7. Rollback

Routes are additive; removing them from `mod.rs` is the rollback.
Audit action variants are append-only in a non-exhaustive-match enum
— adding them does not break existing match arms.

---

## 8. Open Questions

None blocking implementation.

---

## 9. File Structure

```
crates/garraia-auth/src/audit_workspace.rs   — +4 variants + as_str + tests
crates/garraia-gateway/src/rest_v1/chats.rs  — +6 handlers, +5 types
crates/garraia-gateway/src/rest_v1/mod.rs    — +6 routes (all 3 branches)
crates/garraia-gateway/tests/rest_v1_chat_mgmt.rs  — new, ≥12 scenarios
plans/0076-gar-530-chat-mgmt-slice4.md       — this file
plans/README.md                              — row added
```

---

## 10. Tasks

### T1 — Plan file
- [x] Write `plans/0076-gar-530-chat-mgmt-slice4.md`
- [x] Create Linear issue GAR-530

### T2 — Audit action variants
- [ ] Add `ChatUpdated`, `ChatArchived`, `ChatMemberAdded`, `ChatMemberRemoved`
      to `WorkspaceAuditAction` enum
- [ ] Add 4 `as_str` entries
- [ ] Add 4 assertions in the existing audit_workspace tests

### T3 — Types
- [ ] `ChatDetailResponse` (with `updated_at`)
- [ ] `PatchChatRequest` (name?, topic?)
- [ ] `ChatMemberResponse` (user_id, role, joined_at)
- [ ] `ChatMemberListResponse` ({ items })
- [ ] `AddChatMemberRequest` (user_id, role default='member')

### T4 — Handlers
- [ ] `get_chat` — GET /v1/chats/{chat_id}
- [ ] `patch_chat` — PATCH /v1/chats/{chat_id}
- [ ] `delete_chat` — DELETE /v1/chats/{chat_id} (archive, 204)
- [ ] `list_chat_members` — GET /v1/chats/{chat_id}/members
- [ ] `add_chat_member` — POST /v1/chats/{chat_id}/members
- [ ] `remove_chat_member` — DELETE /v1/chats/{chat_id}/members/{user_id}

### T5 — Route registration
- [ ] 6 routes in `RestV1FullState` branch
- [ ] 6 `unconfigured_handler` routes in auth-only branch
- [ ] 6 `unconfigured_handler` routes in no-auth branch

### T6 — Tests
- [ ] `tests/rest_v1_chat_mgmt.rs` with ≥12 scenarios (see §11)

### T7 — Lint + cargo check
- [ ] `cargo check -p garraia-auth -p garraia-gateway` green
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop \
      --features garraia-gateway/test-helpers --no-deps -- -D warnings` green

### T8 — Bookkeeping
- [x] PR merged, commit sha captured in this file (`b48a356`)
- [x] `plans/README.md` row updated

---

## 11. Acceptance Criteria / Test Scenarios

**GET /v1/chats/{chat_id}:**
- M1: 200 — owner can fetch a chat they created
- M2: 404 — archived chat returns 404
- M3: 404 — chat from a different group returns 404 (not 403)
- M4: 401 — missing bearer

**PATCH /v1/chats/{chat_id}:**
- M5: 200 — owner renames a chat; `updated_at` advances
- M6: 400 — empty body (no name or topic)

**DELETE /v1/chats/{chat_id}:**
- M7: 204 — owner archives a chat; subsequent GET returns 404

**GET /v1/chats/{chat_id}/members:**
- M8: 200 — list includes creator as owner

**POST /v1/chats/{chat_id}/members:**
- M9: 201 — add a second group member to chat
- M10: 409 — adding same user twice returns 409

**DELETE /v1/chats/{chat_id}/members/{user_id}:**
- M11: 204 — remove the added member
- M12: 409 — cannot remove last owner

---

## 12. Risk Register

| Risk | Mitigation |
|------|------------|
| RLS blocks INSERT into `chat_members` | JOIN policy via `chats` resolves correctly |
| Last-owner guard missed | Explicit COUNT check before DELETE |
| Cross-group member add | Verify target user is in `group_members` |

---

## 13. Cross-references

- Plan 0054 (GAR-506) — chats slice 1 (create + list)
- Plan 0055 (GAR-507) — messages slice 2 (send + list)
- Plan 0056 (GAR-508) — SET LOCAL SQL injection posture (set_config pattern)
- Migration 004 — `chats`, `chat_members` schema
- Migration 007 — RLS policies

---

## 14. Estimativa

≤ 400 LOC net (handlers + tests). 1 / 2 / 3 hours.
