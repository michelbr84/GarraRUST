# Plan 0107 — GAR-592: messages slice 5 — PATCH + DELETE /v1/messages/{id}

## Goal

Add message **edit** (`PATCH /v1/messages/{message_id}`) and **soft-delete**
(`DELETE /v1/messages/{message_id}`) to the `/v1` messages surface. Both
endpoints use the `garraia_app` FORCE-RLS pool and follow the tenant-context
protocol already established by plan 0055.

No schema migration is required — `edited_at timestamptz` and
`deleted_at timestamptz` already exist in migration 004.

---

## Architecture

```
garraia-gateway/src/rest_v1/messages.rs   ← two new handlers
garraia-auth/src/audit_workspace.rs       ← two new WorkspaceAuditAction variants
garraia-gateway/src/rest_v1/mod.rs        ← two new routes
crates/garraia-gateway/tests/rest_v1_messages_edit_delete.rs  [NEW]
```

### Endpoint matrix

| Method   | Path                              | Auth              | Happy status |
|----------|-----------------------------------|-------------------|--------------|
| `PATCH`  | `/v1/messages/{message_id}`       | Bearer + X-Group-Id | 200 OK      |
| `DELETE` | `/v1/messages/{message_id}`       | Bearer + X-Group-Id | 204 No Content |

---

## Tech stack

- Rust / Axum 0.8 — same as rest of `garraia-gateway`
- `sqlx::query!` (Postgres, `garraia_app` pool) — all queries parameterised
- `garraia-auth`: `Principal` extractor, `can()`, `WorkspaceAuditAction`
- `utoipa` for OpenAPI annotations

---

## Design invariants

1. **FORCE RLS** — every handler opens a `pool.begin()` transaction and runs
   `SELECT set_config('app.current_user_id', $1, true)` and
   `SELECT set_config('app.current_group_id', $1, true)` before any DML.
2. **`body_tsv` is GENERATED ALWAYS AS** — never appears in UPDATE column list.
3. **Sender-only PATCH** — `WHERE id = $1 AND group_id = $2 AND sender_user_id = $3 AND deleted_at IS NULL`. Returns 404 if 0 rows (hides existence of other tenants' messages).
4. **DELETE: sender OR Admin/Owner** — SQL resolves this with two conditions joined by OR, using the `principal.role` tier: `sender_user_id = caller_id OR (principal.role.tier() >= 80)`. Returns 404 on 0 rows updated.
5. **Audit metadata is structural** — `body_len` (char count), no body content.
6. **`X-Group-Id` required** — same guard as existing message handlers.
7. **Idempotent DELETE** — sending DELETE on an already-deleted message returns 404.

---

## Validações pré-plano

- [x] Migration 004 already has `edited_at timestamptz` and `deleted_at timestamptz`.
- [x] `WorkspaceAuditAction` has no `MessageEdited` or `MessageDeleted` variants yet.
- [x] No `PATCH /v1/messages/{id}` or `DELETE /v1/messages/{id}` routes in `mod.rs`.
- [x] `Action::ChatsWrite` exists and is the right capability gate (all 5 roles).
- [x] `Role` enum has `tier()` method (see `can.rs`) — Owner=100, Admin=80 → tier ≥ 80 is the admin-override threshold for delete.

---

## Out of scope

- Bulk-delete or admin-only purge
- Message reactions / emoji
- `GET /v1/messages/{id}` single-message fetch
- WebSocket push on edit/delete
- Hard delete (LGPD erasure path lives in GAR-400 export/delete)

---

## Rollback

Pure handler + audit enum addition. If needed, remove the two routes from `mod.rs`
and the two enum variants from `audit_workspace.rs`. No migration to revert.

---

## §12 Open questions

| # | Question | Decision |
|---|----------|----------|
| 1 | Should PATCH return full message or minimal diff? | Full `EditedMessageResponse` (id, body, edited_at) — consistent with existing response shapes. |
| 2 | Admin override for PATCH (edit others' messages)? | Out of scope for slice 5. Sender-only edit is standard chat behaviour. |
| 3 | Role tier threshold for delete override? | Admin (tier ≥ 80): Owner + Admin. Member/Guest/Child cannot delete others' messages. |

---

## File structure

```
crates/garraia-auth/src/audit_workspace.rs          ← +2 variants + 2 match arms + 2 test rows
crates/garraia-gateway/src/rest_v1/messages.rs      ← +2 structs + 2 utoipa handlers (~180 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs           ← +2 route registrations (auth + test-helpers)
crates/garraia-gateway/tests/rest_v1_messages_edit_delete.rs  [NEW ~250 LOC]
```

**Estimated total delta:** ~430 LOC added, ~10 LOC modified.

---

## M1 tasks

### T1 — audit variants

- [ ] Add `MessageEdited` and `MessageDeleted` to `WorkspaceAuditAction` enum in `audit_workspace.rs`
- [ ] Add match arms `"message.edited"` / `"message.deleted"` to `as_str()`, `from_str()`, and `audit_all_action_strings_are_unique` test row vectors

### T2 — PATCH handler

- [ ] Add `PatchMessageRequest { body: String }` struct with `validate()` (non-empty, ≤ 100k chars)
- [ ] Add `EditedMessageResponse { id, body, edited_at, group_id }` struct
- [ ] Implement `patch_message` handler:
  - Group-Id header guard
  - `can(ChatsWrite)` capability check
  - `validate()` body
  - tx begin + `set_config` (user_id + group_id)
  - `UPDATE messages SET body = $1, edited_at = now() WHERE id = $2 AND group_id = $3 AND sender_user_id = $4 AND deleted_at IS NULL RETURNING id, body, edited_at`
  - 404 on 0 rows
  - `audit_workspace_event(MessageEdited, …, json!({ "body_len": … }))`
  - tx commit, return 200 + body
- [ ] Add `utoipa::path` annotation

### T3 — DELETE handler

- [ ] Implement `delete_message` handler:
  - Group-Id header guard
  - `can(ChatsWrite)` capability check
  - tx begin + `set_config` (user_id + group_id)
  - Build WHERE clause from role tier:
    - If `principal.role.map(|r| r.tier()).unwrap_or(0) >= 80`: admin path — `WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL`
    - Else: sender path — `WHERE id = $1 AND group_id = $2 AND sender_user_id = $3 AND deleted_at IS NULL`
  - `UPDATE messages SET deleted_at = now() WHERE … RETURNING id`
  - 404 on 0 rows
  - `audit_workspace_event(MessageDeleted, …, json!({ "admin_override": is_admin }))`
  - tx commit, return 204
- [ ] Add `utoipa::path` annotation

### T4 — router wiring

- [ ] In `mod.rs` auth-router: add `.route("/v1/messages/:message_id", patch(messages::patch_message).delete(messages::delete_message))`
- [ ] In `mod.rs` test-helpers router: add same (no-auth stub)
- [ ] In `mod.rs` fail-soft router: add same (503 stub)

### T5 — integration tests

- [ ] Create `crates/garraia-gateway/tests/rest_v1_messages_edit_delete.rs`
- [ ] Scenarios:
  - `ME1` edit own message — 200 + body updated + edited_at present
  - `ME2` edit non-existent message — 404
  - `ME3` edit message sent by another user — 404 (sender check)
  - `ME4` edit already-deleted message — 404
  - `ME5` edit with empty body — 400
  - `MD1` delete own message — 204
  - `MD2` delete already-deleted message — 404
  - `MD3` delete message sent by another user as Member — 404
  - `MD4` delete message sent by another user as Admin — 204 (admin override)
  - `MD5` cross-group: message in group B invisible to group A caller — 404
- [ ] Run `cargo test -p garraia-gateway --test rest_v1_messages_edit_delete` — all green

### T6 — workspace-wide lint and format

- [ ] `cargo fmt --check --all`
- [ ] `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`

### T7 — push + PR

- [ ] `git push origin routine/<branch>` after each task commit
- [ ] Open PR via GitHub MCP (base=main)
- [ ] Wait for all CI checks green

### T8 — bookkeeping

- [ ] Update `ROADMAP.md §3.4` — add two `[x]` lines for PATCH + DELETE `/v1/messages/{id}`
- [ ] Add plan row to `plans/README.md`
- [ ] Mark GAR-592 In Progress then Done in Linear

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `body_tsv` GENERATED regen on UPDATE causes unexpected latency | Low | Low | Postgres handles GENERATED ALWAYS AS atomically; no extra round-trip |
| Admin-override delete leaks cross-group message existence | Medium | High | `AND group_id = $2` in WHERE ensures RLS + explicit FK prevent cross-group leak |
| `role.tier()` missing or name mismatch | Low | Medium | Verified `can.rs` — use `principal.role.map(\|r\| r.tier()).unwrap_or(0)` |

---

## Acceptance criteria

1. `cargo test -p garraia-gateway --test rest_v1_messages_edit_delete` — ≥ 10 scenarios, all pass.
2. `cargo clippy --workspace … -D warnings` — zero warnings.
3. CI: all 18 checks green.
4. `ROADMAP.md §3.4` has `[x] PATCH /v1/messages/{message_id}` and `[x] DELETE /v1/messages/{message_id}`.
5. Cross-group isolation test `MD5` present and passing.

---

## Cross-references

- Plan 0055 (GAR-507) — messages slice 2, established tenant-context protocol
- Plan 0076 (GAR-530) — chats slice 4, admin-override delete pattern
- GAR-400 — LGPD hard-delete (out of scope for this plan)
- Migration 004 — `edited_at` and `deleted_at` columns

---

## Estimativa

- Low: 2h | Provável: 3h | Alta: 5h
- Branch: `routine/202605121215-messages-edit-delete`
