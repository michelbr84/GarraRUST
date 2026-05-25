# Plan 0182 — GAR-700: Message Attachments API

> `POST/GET/DELETE /v1/messages/{id}/attachments` — closes ROADMAP §3.6 `[ ] Anexos via message_attachments → files`.

## Goal

Add three REST handlers that let callers attach, list and detach files from messages.
The `message_attachments` join table (migration 020, GAR-697) is the prerequisite; this
plan adds the API surface on top.

## Architecture

- **Crate boundary:** `garraia-gateway` (handlers) + `garraia-auth` (two new audit variants).
- **Pool:** `AppPool` (`garraia_app` BYPASSRLS=false). Every handler opens a transaction
  and calls `set_rls_context` before any table access.
- **RLS class:** `message_attachments` uses JOIN via `messages` (migration 020 policy
  `message_attachments_through_messages`). The composition transparently filters to
  `app.current_group_id` — no explicit group predicate needed on the join table itself.
- **Pattern:** mirrors `task_attachments` (plan 0096 / GAR-572, `tasks/attachments.rs`).

## Tech stack

- Rust / Axum 0.8 / sqlx (Postgres)
- `garraia-auth` for `Principal`, `can()`, `audit_workspace_event`, `WorkspaceAuditAction`
- `utoipa` annotations for OpenAPI 3.1

## Design invariants

1. `SET LOCAL app.current_user_id = '<uuid>'` AND `app.current_group_id = '<uuid>'`
   before ANY access to `message_attachments`, `messages`, `files`, or `audit_events`.
2. No PII in audit metadata jsonb — carry UUIDs only (`message_id`, `file_id`).
3. Cross-group isolation: file from another group is invisible via RLS → 0 rows → **404**
   (not 403, to avoid leaking existence of resources in other tenants).
4. `attached_by_label` cached at insert (fetched from `users.display_name`); not leaked
   in audit metadata.
5. `DELETE` is idempotent: if attachment does not exist, return 204 (not 404). If the
   parent message itself does not exist, return 404.

## Validações pré-plano

- [x] Migration 020 creates `message_attachments` with the correct schema (plan 0179 / GAR-697).
- [x] `garraia_app` has `SELECT, INSERT, DELETE` on `message_attachments` (explicit GRANT in migration 020).
- [x] `WorkspaceAuditAction::TaskFileAttached/Detached` pattern confirmed in `audit_workspace.rs`.
- [x] `messages.rs` is 1 409 lines — handlers can be appended (still under 1 800 LOC soft limit; split to `messages/attachments.rs` in a follow-up Q11 slice if needed).

## Out of scope

- Uploading new files via this endpoint (callers must first upload via `POST /v1/groups/{id}/files`).
- Updating attachment metadata.
- `WITH CHECK` policy on `message_attachments` (not needed — the INSERT validates group
  membership via RLS on `messages` + explicit `WHERE group_id = $1` on `files`).

## Rollback

Pure Rust changes + no migration. Rollback = revert the PR. Migration 020 is already
committed (plan 0179) — this plan adds zero schema changes.

## §12 Open questions

None. Pattern fully proven by `task_attachments` (408 LOC, 3 endpoints, same RLS class).

## File structure

```
crates/
  garraia-auth/
    src/audit_workspace.rs          # +2 variants + as_str arm + test rows
  garraia-gateway/
    src/rest_v1/messages.rs         # +3 handlers appended (~200 LOC)
    src/rest_v1/mod.rs              # +3 route registrations
    tests/rest_v1_message_attachments.rs   # NEW — 8 integration scenarios
```

## M1 tasks

### T1 — Add `MessageFileAttached` / `MessageFileDetached` audit variants

- [ ] Add two variants to the `WorkspaceAuditAction` enum after `TaskFileDetached`.
- [ ] Add two arms to the `as_str()` match: `"message.file.attached"` / `"message.file.detached"`.
- [ ] Add entries to the test table (`#[cfg(test)] mod tests` in `audit_workspace.rs`).
- [ ] `cargo check -p garraia-auth` → green.

### T2 — Write failing integration tests (red)

New file `crates/garraia-gateway/tests/rest_v1_message_attachments.rs` with 8 scenarios
(assertions against status codes + response body shape; fail because handlers don't exist):

| ID | Scenario |
|----|----------|
| MA1 | POST attach → 201 + correct body |
| MA2 | POST attach duplicate → 409 |
| MA3 | POST attach file from other group → 404 |
| MA4 | GET list → 200 + pagination |
| MA5 | GET list on non-existent message → 404 |
| MA6 | DELETE detach → 204 |
| MA7 | DELETE detach already-absent → 204 (idempotent) |
| MA8 | Cross-group: user from group B cannot list attachments of group A message → 403 |

### T3 — Implement handlers in `messages.rs`

Append to `crates/garraia-gateway/src/rest_v1/messages.rs`:

- `post_message_attachment(state, principal, Path(msg_id), Json(body))` → 201
- `list_message_attachments(state, principal, Path(msg_id), Query(params))` → 200
- `delete_message_attachment(state, principal, Path((msg_id, file_id)))` → 204

All three: `require_group_id`, `set_rls_context`, explicit 404 guard on message,
RLS-scoped file existence check, `audit_workspace_event`.

### T4 — Register routes in `mod.rs`

```rust
.route("/v1/messages/:message_id/attachments",
    post(messages::post_message_attachment).get(messages::list_message_attachments))
.route("/v1/messages/:message_id/attachments/:file_id",
    delete(messages::delete_message_attachment))
```

### T5 — Cargo check + clippy strict

```bash
SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip \
  cargo clippy --workspace --tests --exclude garraia-desktop \
  --features garraia-gateway/test-helpers --no-deps -- -D warnings
```

Fix all warnings before proceeding.

### T6 — Confirm tests green

```bash
SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip \
  cargo test -p garraia-gateway --test rest_v1_message_attachments
```

### T7 — Update ROADMAP.md

Flip `[ ] Anexos via message_attachments → files` → `[x]` in §3.6.
Add entry to §3.4 chats checklist.

### T8 — Commit plan, open PR, wait CI

Commit plan file + README row. Push, open PR `routine/202605251124-message-attachments-api`.
Wait for ≥16 checks green.

## Risk register

| Risk | Mitigation |
|------|------------|
| messages.rs exceeds 1 800 LOC with new handlers | If so, split to `rest_v1/messages/` sub-module in same PR |
| FORCE RLS on `message_attachments` blocks INSERT | Verified: `garraia_app` has explicit GRANT in migration 020 |
| `SET LOCAL` UUID injection | Uuid::Display produces 36 hex-dash chars; injection-safe by construction |

## Acceptance criteria

1. `POST /v1/messages/{id}/attachments` returns 201 with attachment body (MA1).
2. Duplicate attach returns 409 (MA2).
3. File from other group returns 404 (MA3).
4. `GET` returns paginated list joined to files (MA4).
5. `GET` on missing message returns 404 (MA5).
6. `DELETE` returns 204; second call also 204 (MA6, MA7).
7. Cross-group principal gets 403 (MA8).
8. All 20 CI checks green; `cargo clippy --workspace -- -D warnings` clean.
9. ROADMAP §3.6 `[x]` committed in same PR.

## Cross-references

- Plan 0179 / GAR-697 — migration 020 `message_attachments` (prerequisite)
- Plan 0096 / GAR-572 — `task_attachments` (template)
- Plan 0054 / GAR-506 — `chats` (establishes `set_rls_context` pattern)
- ROADMAP §3.6, §3.4 Chats checklist

## Estimativa

- T1: 20 min
- T2: 25 min
- T3: 60 min
- T4: 10 min
- T5+T6: 15 min
- T7+T8: 15 min
- **Total: ~2h 25min**
