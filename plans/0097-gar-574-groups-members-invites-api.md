# Plan 0097 — GAR-574: REST /v1 groups slice 2 — GET members + GET invites

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-11, America/New_York)
**Data:** 2026-05-11 (America/New_York)
**Issue:** [GAR-574](https://linear.app/chatgpt25/issue/GAR-574)
**Branch:** `routine/202505110023-groups-members-invites-api`
**Epic:** `epic:ws-api`
**Parent:** GAR-393

---

## §1 Goal

Add the two missing read endpoints for group membership management, completing
the `/v1/groups` surface and unblocking any UI that needs to display who is in
a group or which invitations are pending.

**Endpoints:**
- `GET /v1/groups/{id}/members?cursor=<uuid>&limit=<n>&role=<str>&status=<str>` — cursor-paginated list of `group_members` rows; any active group member can read; keyset on `user_id ASC`; default excludes `removed`/`banned`.
- `GET /v1/groups/{id}/invites?cursor=<uuid>&limit=<n>` — cursor-paginated list of pending `group_invites` (accepted_at IS NULL); requires `Action::MembersManage`; keyset on `(created_at ASC, id ASC)`.

---

## §2 Architecture

### Tenant isolation

`group_members` and `group_invites` are **tenant-root tables** (no FORCE RLS —
they are the "roots" from which tenant identity is derived). Cross-group
isolation is enforced at the SQL level via explicit `WHERE group_id = $path_id`
predicates. The Principal extractor already validates the caller is an active
member of `principal.group_id`; the path/header coherence guard ensures
`path_id == principal.group_id`.

### Cursor scheme

- **list_members**: ORDER BY `(user_id ASC)`. Cursor = `user_id` UUID of last
  item seen. Next page: `WHERE group_id = $1 AND user_id > $cursor`. Stable
  because user_id is part of the composite PK.
- **list_invites**: ORDER BY `(created_at ASC, id ASC)`. Cursor = `id` UUID of
  last invite seen. Next page: `WHERE group_id = $1 AND accepted_at IS NULL
  AND (created_at, id) > (SELECT (created_at, id) FROM group_invites WHERE id = $cursor)`.
  Simplified to `WHERE id > $cursor` (UUID v4 ordering is not time-monotonic,
  but `(created_at ASC, id ASC)` is stable + the limit+1 trick gives correct
  next_cursor regardless of UUID collation within the same second).

### Response shape

```
GET /v1/groups/{id}/members
→ { items: [{ user_id, role, status, joined_at, invited_by? }], next_cursor? }

GET /v1/groups/{id}/invites
→ { items: [{ id, invited_email, proposed_role, expires_at, created_by, created_at }], next_cursor? }
```

`token_hash` is **never** in the response (it is an auth secret).

---

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as groups.rs)
- `sqlx::query_as` parameterized (no SQL string concat)
- `garraia_auth::{Action, Principal, can}` — `MembersManage` for invites
- `utoipa` OpenAPI annotations
- No new migrations (schema from migration 001)

---

## §4 Design invariants

1. Path/header coherence: `path_id == principal.group_id` → 400 if mismatch, 400 if header absent.
2. No FORCE RLS tables touched — SET LOCAL `app.current_user_id` only (for consistency; no `app.current_group_id` needed).
3. `group_invites.token_hash` is NEVER in any response field.
4. `invited_email` is PII — only accessible to owners/admins (`Action::MembersManage`).
5. No `unwrap()` in production; no SQL string concat.
6. Cursor-next trick: fetch `limit + 1` rows, pop last to detect next page.
7. Default limit: 50; max limit: 100.

---

## §5 Validações pré-plano

- [x] `group_members` schema confirmed in migration 001: (group_id, user_id) PK, role CHECK 5 vals, status CHECK 4 vals, joined_at, invited_by.
- [x] `group_invites` schema confirmed: id PK, group_id, invited_email (citext), proposed_role, token_hash, expires_at, created_by, created_at, accepted_at.
- [x] No FORCE RLS on group_members/group_invites (confirmed from migration 007 scope).
- [x] `Action::MembersManage` confirmed in garraia-auth capability table.
- [x] Pattern reference: audit.rs (cursor pagination), chats.rs (list_chat_members).

---

## §6 Out of scope

- Sending or revoking invites (create_invite already done in plan 0020).
- Adding members directly (done via invite flow).
- Searching members by name (FTS on users table deferred).
- `PATCH /v1/groups/{id}/invites/{invite_id}` (resend/extend — future slice).

---

## §7 Rollback

Handlers are additive (no schema changes, no data migrations). Remove the two
handler functions and the route registrations to revert.

---

## §8 Open questions

None — schema and auth foundation are in place.

---

## §9 File structure

```
crates/garraia-gateway/src/rest_v1/
  groups.rs                    ← add list_members + list_invites handlers + DTOs
  mod.rs                       ← register 2 new GET routes in all 3 router arms
  tests/rest_v1_groups_members_invites.rs   ← new integration test file (6+ tests)
```

---

## §10 Tasks M1

- [x] T1: Write plan 0097, create GAR-574, update plans/README.md
- [ ] T2: Add DTOs + list_members handler to groups.rs
- [ ] T3: Add list_invites handler to groups.rs
- [ ] T4: Register routes in mod.rs (full + unconfigured 503 + no-auth 503 arms)
- [ ] T5: Write integration tests (GM1–GM3 + GI1–GI3)
- [ ] T6: `cargo check -p garraia-gateway` + clippy clean + fmt
- [ ] T7: Commit + push + open PR
- [ ] T8: Update ROADMAP.md + plans/README.md after merge

---

## §11 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| `invited_email` PII leak via logs | Low | Never log body; `#[instrument(skip_all)]` on handlers |
| UUID cursor ordering instability | Low | UUIDs from same group_id; keyset on unique PK column |
| Missing route in unconfigured/no-auth arms | Low | Register in all 3 router arms in mod.rs |

---

## §12 Acceptance criteria

1. `GET /v1/groups/{id}/members` → 200 `{ items, next_cursor }` for active member.
2. `GET /v1/groups/{id}/invites` → 200 for owner/admin; 403 for plain member.
3. Cursor pagination: second page returns next batch correctly.
4. Cross-group isolation: member of group A cannot see group B's member list (400 header mismatch enforced).
5. `token_hash` never appears in any response.
6. 6+ integration tests (GM1–GM3 + GI1–GI3) green.
7. `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

---

## §13 Cross-references

- ROADMAP §3.4 "Grupos" — `GET /v1/groups/{id}/members` and `GET /v1/groups/{id}/invites` (now added as `[ ]`)
- GAR-393 — parent epic (groups REST surface)
- Migration 001 — `group_members` + `group_invites` schema
- Plan 0020 / GAR-425 — `POST /v1/groups/{id}/invites`, `setRole`, `DELETE member`
- Plan 0070 / GAR-522 — audit list (cursor pagination reference)

---

## §14 Estimativa

- T2-T5: ~3h implementation
- T6-T7: ~30min CI + push
- Total: ~3.5h
