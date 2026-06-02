# Plan 0255 — GAR-777: GET /v1/me/invites (caller-scoped pending group invites inbox)

**Status:** In Progress  
**Linear:** [GAR-777](https://linear.app/chatgpt25/issue/GAR-777)  
**Branch:** `routine/202506021230-me-invites-inbox`  
**Date:** 2026-06-02 (America/New_York)

---

## 1. Goal

Add `GET /v1/me/invites` — a cursor-paginated inbox of pending group invites addressed
to the authenticated caller's email address. Completes the `me/*` inbox family alongside
mentions (GAR-755), tasks (GAR-763), chats (GAR-765), files (GAR-767), and memory (GAR-770).

---

## 2. Architecture

### Table access

- `group_invites` — no FORCE RLS (token-based access model per migration 007 comment).
- `users` — no FORCE RLS; visible to `garraia_app` role.

Because neither table has FORCE RLS, **no transaction and no `SET LOCAL` context variables
are needed**. The handler runs a plain parameterized query directly against the pool,
isolated via `WHERE u.id = $principal_user_id`.

### SQL pattern

```sql
-- No-cursor branch:
SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at
FROM group_invites gi
JOIN users u ON u.email = gi.invited_email
WHERE u.id = $1
  AND gi.accepted_at IS NULL
  AND gi.expires_at > now()
ORDER BY gi.created_at DESC, gi.id DESC
LIMIT $2

-- Cursor branch (after_id provided):
SELECT gi.id, gi.group_id, gi.proposed_role, gi.created_at, gi.expires_at
FROM group_invites gi
JOIN users u ON u.email = gi.invited_email
WHERE u.id = $1
  AND gi.accepted_at IS NULL
  AND gi.expires_at > now()
  AND (gi.created_at, gi.id) < (
      SELECT gi2.created_at, gi2.id FROM group_invites gi2
      JOIN users u2 ON u2.email = gi2.invited_email
      WHERE gi2.id = $2 AND u2.id = $1
  )
ORDER BY gi.created_at DESC, gi.id DESC
LIMIT $3
```

### Security invariants

- **`token_hash` is never returned** — it is the Argon2id hash of the invite token.
- **`invited_email` is never returned** — PII; caller already knows their own email.
- **`group_name` is not returned** — invited users have no `group_members` row yet,
  so `groups` FORCE RLS (migration 018) would filter to 0 rows. Clients may call
  `GET /v1/groups/{id}` after accepting to retrieve group metadata.
- Isolation enforced at DB level via `WHERE u.id = $principal_user_id`, not RLS.

---

## 3. Tech stack

- Axum 0.8 `State<RestV1FullState>` + `Principal` extractor
- `sqlx::query_as` with tuple row type (no migration)
- `utoipa` for OpenAPI annotation + `IntoParams` / `ToSchema`
- `serde` with `skip_serializing_if = "Option::is_none"` on `next_cursor`

---

## 4. Out of scope

- Accepted, cancelled, or expired invites (history endpoint, separate slice).
- Group name resolution (requires separate fetch after RLS context set).
- Invite acceptance flow (already exists at `POST /v1/groups/{id}/invites/{token}/accept`).

---

## 5. No migration required

`group_invites` and `users` schemas are unchanged. Index
`group_invites_pending_email_idx ON group_invites(invited_email) WHERE accepted_at IS NULL`
(migration 001) supports the query's WHERE predicate.

---

## 6. Files changed

| File | Change |
|------|--------|
| `crates/garraia-gateway/src/rest_v1/me.rs` | Add `ListMyInvitesQuery`, `PendingInviteSummary`, `MyInvitesResponse`, `list_my_invites` handler + 7 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Register `.route("/v1/me/invites", ...)` in all 3 router branches |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Add path + component registration |
| `plans/README.md` | Add row 0255 |
| `ROADMAP.md` | Mark `GET /v1/me/invites` as done in §3.4 |
| `TODO.md` | Update completed section |

---

## 7. Acceptance criteria

- `cargo check -p garraia-gateway` passes.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes.
- `cargo test -p garraia-gateway` passes (7 new unit tests green).
- Route appears in `GET /v1/openapi.json`.
- `token_hash` and `invited_email` absent from all response shapes.

---

## 8. Cross-references

- Plan 0245 (GAR-765) — GET /v1/me/chats (same inbox family)
- Plan 0246 (GAR-767) — GET /v1/me/files (same inbox family)
- Plan 0249 (GAR-770) — GET /v1/me/memory (same inbox family)
- Migration 001 — `group_invites` schema + `group_invites_pending_email_idx`
- Migration 007 — `garraia_app` grant; `group_invites` intentionally excluded from FORCE RLS
- Migration 018 — `groups` FORCE RLS (explains why group_name is not returnable for pending invites)
