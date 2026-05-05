# Plan 0056 — GAR-WS-CHAT Slice 3: `POST /v1/messages/{message_id}/threads`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-509](https://linear.app/chatgpt25/issue/GAR-509) — "REST /v1 chats slice 3: POST /v1/messages/{message_id}/threads" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Status:** ✅ Approved — 2026-05-05 (Florida). Pre-requisites validated in §"Validações pré-plano".

**Goal:** Deliver `POST /v1/messages/{message_id}/threads` — promotes a top-level message to a thread root by inserting a `message_threads` row. Reuses 100% of the foundation from plans 0016/0017/0019/0020/0054/0055 (AppPool + `Principal` + `can()` + audit + `set_config` parameterized SQL pattern). Zero new migration, zero new ADR, zero new capability — `Action::ChatsWrite` already exists.

**Architecture:**

1. New handler `create_thread` in `crates/garraia-gateway/src/rest_v1/messages.rs` (~130 LOC), following the exact pattern of `send_message`.
2. **`POST /v1/messages/{message_id}/threads`** — happy path:
   - `Principal` extractor 403s non-members. Handler validates `principal.group_id` via `X-Group-Id`.
   - `can(&principal, Action::ChatsWrite)`.
   - `tx = pool.begin()` → `set_config` user + group → SELECT `messages.chat_id, messages.group_id` WHERE `id = message_id AND group_id = principal.group_id AND deleted_at IS NULL` → 404 if 0 rows.
   - INSERT `message_threads (chat_id, root_message_id, title, created_by)` RETURNING `id, created_at`.
   - On SQLSTATE 23505 (`UNIQUE (root_message_id)`) → 409 Conflict.
   - audit `thread.created` with `{ has_title }` → COMMIT → 201.
3. **New `WorkspaceAuditAction::ThreadCreated`** variant in `garraia-auth/src/audit_workspace.rs`. Metadata: `{ has_title }` only — no message body, no title content (PII-safe).
4. **OpenAPI**: register 1 path + 2 schemas (`CreateThreadRequest`, `ThreadResponse`) in `openapi.rs`.
5. **Router**: 1 route in all 3 modes (mode 1 real; modes 2-3 fail-soft 503).
6. **Tests**: extend `tests/rest_v1_messages.rs` with 5 new scenarios (T1–T5) + 3 new cases in `authz_http_matrix.rs` (47–49).

**Tech stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{Action, Principal, can, WorkspaceAuditAction, audit_workspace_event}`, `sqlx 0.8` (postgres), `serde 1.0`, `chrono 0.4`, `uuid 1.x`. Test harness: `testcontainers` + `pgvector/pgvector:pg16`.

---

## Design invariants (non-negotiable for this slice)

1. **`message_threads` is under FORCE RLS via JOIN policy `message_threads_through_chats`.** Policy scopes by `chat_id IN (SELECT id FROM chats WHERE group_id = current_group_id)`. Handler MUST `set_config('app.current_group_id', $1, true)` before any read/write on `message_threads`.
2. **Message ownership check via `(id, group_id)` within the tx.** `message_id` in the path does not guarantee the message belongs to `principal.group_id`. Handler MUST `SELECT chat_id FROM messages WHERE id = $message_id AND group_id = $principal_group_id AND deleted_at IS NULL` inside the tx. 0 rows → 404 (not 403, to hide cross-tenant message existence — same pattern as chats and uploads).
3. **SQLSTATE 23505 on `UNIQUE (root_message_id)` → 409 Conflict.** Each message can be the root of at most one thread. Concurrent creation attempts resolve via DB uniqueness.
4. **Audit metadata: `{ has_title }` ONLY.** `messages.body` and `title` text are user-controlled and may contain PII. Only the structural boolean `has_title` is audit-safe.
5. **`set_config` pattern (not `format!` + SET LOCAL).** Following plan 0056 (GAR-508), all tenant-context SQL uses `sqlx::query("SELECT set_config('app.current_user_id', $1, true)").bind(uuid.to_string())` — no format! interpolation.
6. **`X-Group-Id` header required.** Same pattern as all prior `/v1/` endpoints.
7. **No UPDATE to `messages.thread_id`.** The root message's `thread_id` column stays NULL. That column tracks which thread a REPLY belongs to, not the root message. Thread root ownership is tracked via `message_threads.root_message_id`.

---

## Validações pré-plano (gate executed this session)

- ✅ `Action::ChatsWrite` exists — `crates/garraia-auth/src/action.rs:20`.
- ✅ `message_threads` table exists — migration 004 `crates/garraia-workspace/migrations/004_chats_and_messages.sql:101`.
- ✅ `message_threads FORCE RLS` + `message_threads_through_chats` policy — migration 007 lines 115-125.
- ✅ `UNIQUE (root_message_id)` on `message_threads` — migration 004 line 109.
- ✅ `RestError::Conflict` variant exists with 409 mapping — `problem.rs:50`.
- ✅ `set_config` pattern available from PR #126 / GAR-508 (merged before implementation).
- ✅ `WorkspaceAuditAction` extensible enum — `audit_workspace.rs:52`.
- ✅ `authz_http_matrix.rs` currently has 46 cases (41–46 for messages/chats, plan 0055).

---

## Out of scope

- GET /v1/messages/{id}/threads or GET /v1/threads/{id}
- POST replies into a thread (that would set `messages.thread_id`)
- Thread resolution (`resolved_at`)
- Thread deletion
- `message_attachments` — separate slice

---

## Rollback

No schema change. Handler additions are additive. Roll back by reverting the 3 files changed (`messages.rs`, `openapi.rs`, `mod.rs`) and the new audit variant. The `authz_http_matrix.rs` extension is also trivially reverted. No data migration required.

---

## §12 Open questions (all answered)

1. **Does `message_threads` need a WITH CHECK?** No — Postgres uses USING as WITH CHECK when none specified. The INSERT's `chat_id` must satisfy the USING clause (chat belongs to the right group), which it does because we read `chat_id` from the message in the same tx. ✅
2. **Should the root message's `thread_id` be set?** No — `messages.thread_id` tracks which thread a message is a REPLY in, not the root. The schema comment confirms: "NULL means top-level message." ✅
3. **Title validation?** Optional field (`Option<String>`). If provided, enforce non-empty after trim and max 500 chars. ✅

---

## File structure

```text
crates/garraia-auth/src/audit_workspace.rs   — +1 variant ThreadCreated, +1 test
crates/garraia-gateway/src/rest_v1/messages.rs — +CreateThreadRequest, +ThreadResponse, +create_thread handler
crates/garraia-gateway/src/rest_v1/openapi.rs  — +1 path, +2 schemas
crates/garraia-gateway/src/rest_v1/mod.rs      — +1 route in 3 modes
crates/garraia-gateway/tests/rest_v1_messages.rs — +5 scenarios (T1-T5)
crates/garraia-gateway/tests/authz_http_matrix.rs — +3 cases (47-49)
plans/0056-gar-ws-chat-slice3-threads.md        — this file
plans/README.md                                  — +1 row
```

---

## M1 tasks

- [ ] **T1 — `WorkspaceAuditAction::ThreadCreated`** — add variant to `audit_workspace.rs`, add `"thread.created"` string mapping, add 1 unit test for `as_str()`. Commit: `test(auth): add ThreadCreated audit variant (plan 0056, GAR-509)`. Run `cargo check -p garraia-auth` → green.
- [ ] **T2 — DTOs + validator** — add `CreateThreadRequest { title: Option<String> }` + `validate()` (title non-empty after trim if Some, max 500 chars) + `ThreadResponse { id, chat_id, root_message_id, title, created_by, created_at }` to `messages.rs`. Add unit tests for `CreateThreadRequest::validate`. Commit: `test(gateway): CreateThreadRequest validator unit tests (plan 0056, GAR-509)`. Run `cargo check -p garraia-gateway` → green.
- [ ] **T3 — `create_thread` handler** — implement the full handler in `messages.rs`:
  1. Extract group_id from `principal.group_id`, 400 if None.
  2. `can(&principal, Action::ChatsWrite)` → 403.
  3. `body.validate()` → 400.
  4. `pool.begin()` → `set_config` user+group.
  5. `SELECT chat_id FROM messages WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL` → 404.
  6. `INSERT INTO message_threads (chat_id, root_message_id, title, created_by) VALUES ($1,$2,$3,$4) RETURNING id, created_at` → match 23505 → 409.
  7. `audit_workspace_event(ThreadCreated, ...)`.
  8. `tx.commit()` → 201 `ThreadResponse`.
  Commit: `feat(gateway): POST /v1/messages/{id}/threads handler (plan 0056, GAR-509)`. Run `cargo check -p garraia-gateway` → green.
- [ ] **T4 — OpenAPI + router** — register path in `openapi.rs`, wire route in `mod.rs` (all 3 modes), export DTOs. Commit: `feat(gateway): wire POST /v1/messages/{id}/threads in router + OpenAPI (plan 0056, GAR-509)`. Run `cargo clippy --workspace --tests --features garraia-gateway/test-helpers --no-deps -- -D warnings` → clean.
- [ ] **T5 — Integration tests** — extend `rest_v1_messages.rs` with bundled scenarios:
  - T1: POST 201 happy path — assert response shape, DB row in `message_threads`, audit row with `{has_title:false}`.
  - T2: POST 201 with title — assert title stored, audit `{has_title:true}`.
  - T3: POST 409 — create thread twice on same message.
  - T4: POST 404 — message_id belongs to a different group.
  - T5: POST 401 — missing bearer.
  Commit: `test(gateway): integration tests for POST /v1/messages/{id}/threads (plan 0056, GAR-509)`. Run `cargo test -p garraia-gateway --features test-helpers -- rest_v1_messages` → green (requires Postgres).
- [ ] **T6 — authz matrix** — add 3 cases (47-49) to `authz_http_matrix.rs` for cross-group POST (3 × thread-create against another group's message → 404). Update case count assertion to 49. Commit: `test(gateway): extend authz matrix to 49 cases (plan 0056, GAR-509)`.
- [ ] **T7 — ROADMAP + README** — mark `POST /v1/messages/{message_id}/threads` as `[x]` in `ROADMAP.md §3.4`, add row to `plans/README.md`. Commit: `docs(plans): mark plan 0056 merged — GAR-509`.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `message_threads_through_chats` policy blocks INSERT | Low | High | Pre-plan validation confirmed policy allows INSERT via USING-as-WITH_CHECK; `chat_id` derived from message lookup in same tx |
| Race: two users create thread simultaneously | Low | Medium | `UNIQUE (root_message_id)` + SQLSTATE 23505 → 409 serializes concurrent attempts |
| Title PII in audit | Low | High | Invariant 4 — only `has_title` boolean in metadata |

---

## Acceptance criteria

- `POST /v1/messages/{id}/threads` with valid message → 201 + `{ id, chat_id, root_message_id, title, created_by, created_at }`.
- Duplicate POST on same message → 409.
- Cross-group message_id → 404.
- Missing bearer → 401.
- Missing `X-Group-Id` → 400.
- `cargo clippy --workspace --tests --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- CI 17/17 green.
- `authz_http_matrix` has exactly 49 cases.

---

## Cross-references

- ROADMAP.md §3.4 `[ ] POST /v1/messages/{message_id}/threads`
- Plan 0054 (GAR-506) — chats CRUD foundation
- Plan 0055 (GAR-507) — messages send/list foundation
- Plan 0056 (GAR-508) — `set_config` SQL injection fix (prerequisite for the new pattern)
- `docs/adr/0005-identity-provider.md` — AppPool + Principal pattern

---

## Estimativa

1 / 2 / 3 horas. ~280 LOC new code (handler + tests + authz matrix). Zero schema change.
