# Plan 0105 — GAR-580: GET /v1/groups (list user's groups)

**Status:** Em execução  
**Autor:** Claude Sonnet 4.6 (routine/202605120022-groups-list, 2026-05-12, America/New_York)  
**Issue Linear:** [GAR-580](https://linear.app/chatgpt25/issue/GAR-580)  
**Branch:** `routine/202605120022-groups-list`

---

## §1 Goal

Deliver `GET /v1/groups` — cursor-paginated list of every group where the authenticated
user is an **active** member. Closes GAR-580 (Groups REST API slice 3).

This is the only missing Groups endpoint in the Fase 3.4 checklist: every client app
needs to enumerate a user's groups to build navigation, group-switching UIs, etc.

---

## §2 Architecture

No schema change required — `groups` and `group_members` (migration 001) already hold
the data. The query is a JOIN:

```sql
SELECT g.id, g.name, g.type, g.created_at, g.created_by, gm.role, gm.joined_at
FROM group_members gm
JOIN groups g ON g.id = gm.group_id
WHERE gm.user_id = $user_id
  AND gm.status = 'active'
  [AND (gm.joined_at, gm.group_id) > ($cursor_joined_at, $cursor_group_id)]
  [AND gm.role = $role_filter]
ORDER BY gm.joined_at ASC, gm.group_id ASC
LIMIT $limit + 1
```

Compound keyset cursor `(joined_at, group_id)` is stable even when multiple groups share
the same `joined_at` timestamp (possible in bulk imports). Cursor is encoded as
`<joined_at_iso8601>/<group_id_uuid>` in the query string.

No `X-Group-Id` header required — this endpoint is inherently cross-group (listing all of
a user's groups). `groups` and `group_members` have no FORCE RLS, so no
`SET LOCAL app.current_group_id` is needed; `app.current_user_id` is set defensively.

---

## §3 Tech stack

- Rust / Axum 0.8 — handler in `crates/garraia-gateway/src/rest_v1/groups.rs`
- `sqlx` parameterized queries (no string concat)
- `utoipa` for OpenAPI 3.1 spec generation
- `garraia_auth::Principal` extractor (JWT-only, no `X-Group-Id`)

---

## §4 Design invariants

1. **No `X-Group-Id` header** — this is a cross-group listing endpoint; requiring a
   group header would be circular. The `Principal` extractor can work with only a JWT.
2. **Compound cursor** — `joined_at` alone is not unique; compound `(joined_at, group_id)`
   ensures stable pagination. Encoded as `<iso8601>/<uuid>` in the `cursor` param.
3. **Active-only** — only `status = 'active'` members are returned. Removed/banned
   users' memberships are invisible.
4. **`app.current_user_id` set in tx** — defensive (no FORCE RLS today), but ensures
   forward-compat if RLS is extended to `group_members`.
5. **PII-safe audit** — no user-identifying data in log/trace fields.
6. **Cross-group authz test** — a user in group A must not see group B's members via
   this endpoint (test T4 below).

---

## §5 Validações pré-plano

- [x] `groups` table exists: migration 001 ✅
- [x] `group_members` table exists with `status`, `role`, `joined_at`: migration 001 ✅
- [x] `Principal` extractor works without `X-Group-Id` (JWT-only path) — verified by
      `GET /v1/me` which uses `RestV1AuthState` with only JWT.
- [x] Cursor pagination pattern established in `list_members` / `list_invites` (plan 0097) ✅
- [x] No Linear duplicate: GAR-580 already exists as Backlog item ✅

---

## §6 Out of scope

- `DELETE /v1/groups/{id}` (group deletion)
- `GET /v1/groups` with `types=` filter (not needed by any current client)
- Pagination via `offset` (keyset is the project standard)
- Group soft-delete / archived groups (not in schema today)

---

## §7 Rollback plan

This is a pure addition: a new `GET` handler on an existing route path and a new OpenAPI
entry. Rollback = revert the commit. No schema change, no migration, no data risk.

---

## §8 File structure (changes)

```
crates/garraia-gateway/src/rest_v1/groups.rs      — new handler + DTOs
crates/garraia-gateway/src/rest_v1/mod.rs         — wire GET /v1/groups
crates/garraia-gateway/src/rest_v1/openapi.rs     — register list_groups
crates/garraia-gateway/tests/rest_v1_groups_list.rs  — integration tests
ROADMAP.md                                         — check off GAR-580
plans/README.md                                    — add plan row
```

---

## §9 M1 task list

- [x] T0 — bookkeeping: fix README + ROADMAP for plans 0101–0104 + GAR-574 items
- [ ] T1 — write integration test skeleton (red)
- [ ] T2 — implement `ListGroupsQuery`, `GroupListItem`, `ListGroupsResponse` DTOs
- [ ] T3 — implement `list_groups` handler with compound cursor
- [ ] T4 — add cross-group isolation test (user A cannot see user B's groups)
- [ ] T5 — wire route `GET /v1/groups` in `mod.rs`
- [ ] T6 — register in `openapi.rs`
- [ ] T7 — `cargo check -p garraia-gateway` + `cargo clippy` clean
- [ ] T8 — update ROADMAP.md + plans/README.md (mark GAR-580 done)

---

## §10 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Compound cursor parsing fails for edge timestamps | Low | Unit test with same-second joined_at |
| `Principal` extractor rejects JWT-only (no X-Group-Id) | Low | Covered by `GET /v1/me` precedent |
| Cross-group data leak via `user_id` join | Low | Explicit `WHERE gm.user_id = $principal.user_id` |

---

## §11 Acceptance criteria

1. `GET /v1/groups` with valid JWT returns 200 + list of user's active groups.
2. `?cursor=<iso8601>/<uuid>` advances the page correctly.
3. `?role=member` filters to only groups where the user has that role.
4. `?limit=2` returns at most 2 items + `next_cursor` when more exist.
5. User with no group memberships → 200 `{ items: [], next_cursor: null }`.
6. No JWT → 401.
7. Invalid cursor format → 400.
8. Cross-group: user A's token cannot list user B's groups.
9. `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` clean.
10. CI ≥16 checks all green.

---

## §12 Open questions

None — all resolved by precedent (plan 0097, `list_members` pattern).

---

## Cross-references

- GAR-580: https://linear.app/chatgpt25/issue/GAR-580
- Parent epic: GAR-393 (Groups API)
- Plan 0097 (list_members/list_invites pattern)
- ROADMAP.md §3.4 "Grupos"

---

## Estimativa

~200 LOC implementation + ~150 LOC tests. Low risk. 1–2 hours.
