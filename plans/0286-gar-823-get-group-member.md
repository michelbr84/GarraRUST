# Plan 0286 — GET /v1/groups/{group_id}/members/{user_id} (GAR-823)

**Fase:** 3.4 — API REST `/v1`
**Epic:** `epic:ws-api`
**Linear:** [GAR-823](https://linear.app/chatgpt25/issue/GAR-823)
**Status:** In Progress
**Estimativa:** ~150 LOC + 6 unit tests

---

## Goal

Add `GET /v1/groups/{group_id}/members/{user_id}` — fetch a single group member by user UUID.
Completes the members CRUD alongside list (GAR-574), setRole (plan 0020), and delete (plan 0020).

---

## Architecture

- Handler in `crates/garraia-gateway/src/rest_v1/groups.rs`
- Route added to existing `/v1/groups/{id}/members/{user_id}` router entry (currently DELETE-only)
- OpenAPI path in `openapi.rs`
- No new migration — `group_members` schema is complete (migration 001 + FORCE RLS migration 018)

## Design invariants

1. No capability gate beyond group membership — `user_id`, `role`, `status`, `joined_at`, `invited_by` are non-PII fields visible to all group members.
2. FORCE RLS: SET LOCAL both `app.current_user_id` and `app.current_group_id` inside tx.
3. 404 for non-members (no existence leak for cross-group callers).
4. Reuses existing `MemberSummary` response type.

---

## Out of scope

- Pagination (single-item endpoint)
- `display_name` / `avatar_url` from `users` join (users table is in scope for future enrichment)

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  groups.rs        +~120 LOC (handler + utoipa doc + 6 unit tests)
  mod.rs           +2 LOC (wire GET to existing route)
  openapi.rs       +1 LOC (add get_member path)
ROADMAP.md         +1 line tick
plans/README.md    +1 row
```

---

## M1 tasks

- [x] Write plan file
- [ ] Implement `get_member` handler in `groups.rs`
- [ ] Wire GET route in `mod.rs`
- [ ] Register path in `openapi.rs`
- [ ] Update `ROADMAP.md` (tick GAR-823)
- [ ] Update `plans/README.md` (add row 0286)
- [ ] `cargo clippy` clean
- [ ] Commit + push

---

## Acceptance criteria

- `GET /v1/groups/{id}/members/{user_id}` → 200 + MemberSummary for valid member
- 404 for unknown user_id or cross-group user
- 400 for missing/mismatched X-Group-Id
- Route wired in all 3 router branches
- 6 unit tests pass

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| RLS policy filters query to 0 rows for cross-group | Desired behavior — 404 is correct |

---

## Cross-references

- Plan 0097 (GAR-574) — list members
- Plan 0257 (GAR-780) — get_invite (same single-item GET pattern)
- Migration 018 (GAR-589) — FORCE RLS on group_members
