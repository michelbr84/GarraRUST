# Plan 0115 — GAR-604: Chats DM Creation (`POST /v1/groups/{group_id}/chats` with `type='dm'`)

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-604](https://linear.app/chatgpt25/issue/GAR-604) — "REST /v1 chats slice 5: POST /v1/groups/{group_id}/chats with type='dm' (DM creation)" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Status:** ⏳ Draft — aprovado 2026-05-14 (Florida). Pré-requisitos validados neste plan §"Validações pré-plano".

**Goal:** Lift the `type='dm'` restriction in `POST /v1/groups/{group_id}/chats`. Ships idempotent DM creation: if the pair already has a DM in this group the endpoint returns 200 + the existing chat; otherwise 201 + the new one. Both users auto-enrolled in `chat_members` atomically. A new migration adds DB-level uniqueness (partial UNIQUE INDEX on sorted UUID pair) and CHECK constraint.

**Architecture:**

1. **Migration 019** (`019_chats_dm_pair.sql`): add `dm_user_a uuid` + `dm_user_b uuid` to `chats` (NULL for non-DM rows), a `CHECK` constraint enforcing sorted pair for `type='dm'`, and a partial `UNIQUE INDEX chats_dm_pair_unique ON chats(group_id, dm_user_a, dm_user_b) WHERE type = 'dm'`. No breaking change for existing `channel` rows.
2. **`CreateChatRequest`** in `chats.rs`: add optional `partner_user_id: Option<Uuid>`. Update `validate()` to allow `"dm"` when `partner_user_id` is `Some(...)`, and reject with distinct 400 messages when (a) `type='dm'` but `partner_user_id` is `None`, or (b) `partner_user_id` is `Some` but `type != "dm"`.
3. **`create_chat` handler**: branch on `body.chat_type == "dm"`:
   - **Channel path**: unchanged (existing logic).
   - **DM path**:
     a. Verify `partner_user_id != caller_user_id` (400 "cannot DM yourself").
     b. Verify partner is an active member of the group via `SELECT 1 FROM group_members WHERE group_id=$1 AND user_id=$2 AND status='active'` (404 "partner not found in group").
     c. Normalize pair: `dm_user_a = min(caller, partner)`, `dm_user_b = max(caller, partner)` (lexicographic on UUID bytes via `Ord`).
     d. Attempt `INSERT INTO chats (..., dm_user_a, dm_user_b) VALUES (...) RETURNING id, created_at`.
     e. On `sqlx::Error::Database` with SQLSTATE `23505` (unique_violation): SELECT the existing DM and return `200 OK`.
     f. On success: INSERT 2 `chat_members` rows (caller='owner', partner='member'), emit `ChatCreated` audit, COMMIT, return `201 Created`.
4. **`ChatResponse`**: add `dm_already_exists: bool` field (false on 201, true on 200-idempotent). Optional field (false by default) — backwards-compat.
5. **Tests** (`tests/rest_v1_chats.rs`): 6 new scenarios bundled in the same `#[tokio::test]` function as existing tests. D1–D6: happy path 201, idempotent 200, self-DM 400, missing partner 400, partner not in group 404, cross-group authz.
6. **Unit test update**: rename `create_chat_request_rejects_dm_in_slice1` → `create_chat_request_accepts_dm_with_partner_user_id` and flip expectation.

**Tech stack:** same as plan 0054. No new dependencies.

---

## Design invariants

1. **Uniqueness at DB level.** The partial UNIQUE INDEX (`chats_dm_pair_unique`) is the authoritative guard against duplicate DMs. Application-level check (SELECT before INSERT) is defense-in-depth for legible error messages, not the primary guard.
2. **Sorted UUID pair before INSERT.** `dm_user_a = min(caller_id, partner_id)` and `dm_user_b = max(...)` using Rust `Uuid`'s `Ord` impl (lexicographic on 128-bit value). This is deterministic regardless of who creates the DM.
3. **Idempotent — no 409.** Duplicate DM creation returns 200 + the existing chat object + `"dm_already_exists": true`. Clients can safely call POST twice without error handling.
4. **Both members enrolled atomically.** `chat_members` (caller='owner', partner='member') inserted in the same transaction as `chats`. On any failure the whole tx rolls back — no orphan DM chat.
5. **`name` optional for DMs.** Clients typically display the partner's `display_name` rather than a chat name. If omitted or empty, defaults to `""` (not NULL — `chats.name NOT NULL` constraint, migration 004:16).
6. **FORCE RLS invariant.** `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` both required before any INSERT/SELECT on `chats`, `chat_members`, `group_members`, `audit_events`. No deviation from the pattern established in plans 0054/0076.
7. **Audit metadata: no PII.** `WorkspaceAuditAction::ChatCreated` reused with `{ "type": "dm", "has_name": bool }`. Partner's user_id is NOT in the metadata (it IS in `chat_members` which is the entity trail).
8. **`partner_user_id` not in `CreateChatRequest` for channel type.** `deny_unknown_fields` is already set; `partner_user_id` added as `Option<Uuid>` with `#[serde(skip_serializing_if = "Option::is_none")]` on the response side. Channel requests that accidentally include `partner_user_id` get 400 from `validate()`.

---

## Validações pré-plano

- ✅ `chats.type CHECK (type IN ('channel', 'dm', 'thread'))` — migration 004:15 already includes 'dm'.
- ✅ `chats.name NOT NULL` — must send `""` for nameless DMs (no schema change needed).
- ✅ `chat_members` table exists and is JOIN-RLS scoped. INSERT of 2 rows (one per user) is safe in the same tx.
- ✅ `group_members` table visible via `garraia_app` role — `SELECT` granted in migration 007:70.
- ✅ `garraia_app` role has `INSERT, UPDATE, DELETE, SELECT ON ALL TABLES` (migration 007:70).
- ✅ `WorkspaceAuditAction::ChatCreated` exists in `garraia-auth/src/audit_workspace.rs` — no new variant needed.
- ✅ `Principal` extractor already validates group membership; partner membership is a separate DB check.
- ✅ `sqlx::Error::Database` exposes `code()` returning `Option<Cow<str>>`; `23505` detection is pattern-matched.
- ✅ Harness `seed_member_via_admin` exists in `tests/common/fixtures.rs` — can seed 2 users in the same group.
- ✅ Migration directory is `crates/garraia-workspace/migrations/`; 018 is the current last one.

---

## Out of scope

- `GET /v1/groups/{id}/dm/{partner_id}` (look up existing DM by partner) — natural follow-up, not this slice.
- `GET /v1/me/dms` (list all DMs across groups) — separate slice.
- WebSocket streaming — plan own issue.
- `type='thread'` — reserved for `message_threads` relationship (plan 0058 already handles threads).
- Typing indicators, reactions, mentions — §3.6 of ROADMAP.

---

## Rollback plan

Migration 019 adds nullable columns + a partial index. Rollback via `DROP INDEX IF EXISTS chats_dm_pair_unique; ALTER TABLE chats DROP COLUMN IF EXISTS dm_user_b, DROP COLUMN IF EXISTS dm_user_a; ALTER TABLE chats DROP CONSTRAINT IF EXISTS chats_dm_users_check;`. Existing channel chats unaffected. Handler change is a code-only revert.

---

## File structure

```
crates/garraia-workspace/migrations/
  019_chats_dm_pair.sql           [NEW — 40 LOC SQL]

crates/garraia-gateway/src/rest_v1/
  chats.rs                        [MODIFY — ~180 LOC delta]

crates/garraia-gateway/tests/
  rest_v1_chats.rs                [MODIFY — ~150 LOC delta]
```

---

## M1 tasks

### T1 — Migration 019: dm_user_a / dm_user_b + unique index

- [ ] Create `crates/garraia-workspace/migrations/019_chats_dm_pair.sql`:
  - `ALTER TABLE chats ADD COLUMN dm_user_a uuid;`
  - `ALTER TABLE chats ADD COLUMN dm_user_b uuid;`
  - `ALTER TABLE chats ADD CONSTRAINT chats_dm_users_check CHECK ((type = 'dm' AND dm_user_a IS NOT NULL AND dm_user_b IS NOT NULL AND dm_user_a <> dm_user_b AND dm_user_a < dm_user_b) OR (type <> 'dm' AND dm_user_a IS NULL AND dm_user_b IS NULL));`
  - `CREATE UNIQUE INDEX chats_dm_pair_unique ON chats(group_id, dm_user_a, dm_user_b) WHERE type = 'dm';`
  - `COMMENT ON COLUMN chats.dm_user_a IS '...'`
- [ ] Confirm smoke tests still pass: `cargo test -p garraia-workspace --test smoke 2>&1 | tail -10`

### T2 — `CreateChatRequest` extension + validate() update

- [ ] Add `partner_user_id: Option<Uuid>` field to `CreateChatRequest` (annotated `#[serde(default)]`).
- [ ] Update `validate()`:
  - `"dm"` → require `partner_user_id.is_some()` else `Err("type 'dm' requires 'partner_user_id'")`.
  - `"channel"` | `"thread"` → require `partner_user_id.is_none()` else `Err("'partner_user_id' is only valid for type 'dm'")`.
  - Remove the old `Err("type 'dm' is not yet supported...")` arm.
- [ ] Update unit test `create_chat_request_rejects_dm_in_slice1` → rename + update to verify DM is accepted with `partner_user_id` and rejected without.
- [ ] `cargo check -p garraia-gateway`

### T3 — `create_chat` handler: DM path

- [ ] After `body.validate()`, branch on `body.chat_type == "dm"`:
  - Extract `partner_user_id` (safe unwrap inside the dm branch — validate() guarantees it's Some).
  - 400 guard: `partner_user_id == principal.user_id` → `RestError::BadRequest("cannot create a DM with yourself")`.
  - Normalize: `let (dm_user_a, dm_user_b) = if principal.user_id < partner_user_id { (principal.user_id, partner_user_id) } else { (partner_user_id, principal.user_id) };`
  - Open tx + set_config (both vars, same as channel path).
  - Verify partner membership: `sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id=$1 AND user_id=$2 AND status='active')").bind(group_id).bind(partner_user_id).fetch_one(&mut *tx).await?`; if false → rollback + 404 `RestError::NotFound`.
  - Attempt INSERT with `dm_user_a`, `dm_user_b`.
  - Match error SQLSTATE `23505` → SELECT existing DM → return `(StatusCode::OK, Json(ChatResponse { dm_already_exists: true, ... }))`.
  - On success → INSERT 2 chat_members, audit (ChatCreated, `{type: "dm", has_name: !name.is_empty()}`), commit, return 201.
- [ ] `cargo check -p garraia-gateway`

### T4 — `ChatResponse` update

- [ ] Add `dm_already_exists: bool` field (default false via `#[serde(default)]` on response; skipped on serialize when false via `#[serde(skip_serializing_if = "std::ops::Not::not")]`).
- [ ] Channel path sets `dm_already_exists: false` (no change to behavior).

### T5 — Integration tests

- [ ] Add 6 scenarios to `crates/garraia-gateway/tests/rest_v1_chats.rs`:
  - D1. `POST 201` — happy path: caller + partner both in group, type='dm' + partner_user_id → 201 + check `dm_already_exists` is absent/false, `chat_members` has 2 rows (caller=owner, partner=member), audit row present.
  - D2. `POST 200` — second identical POST → 200 + `dm_already_exists: true` + same chat id.
  - D3. `POST 400` — self-DM: `partner_user_id = caller_user_id` → 400.
  - D4. `POST 400` — DM without `partner_user_id` → 400.
  - D5. `POST 404` — partner not in group → 404.
  - D6. Cross-group: caller in group A attempts DM with `X-Group-Id` of group B → 400/403.
- [ ] Verify existing C1–C5 and G1–G5 scenarios still pass.
- [ ] `cargo test -p garraia-gateway --test rest_v1_chats 2>&1 | tail -20`

### T6 — Clippy + fmt

- [ ] `cargo fmt --check` (fix if needed, commit `style: cargo fmt`)
- [ ] `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`

### T7 — Commit

- [ ] `git add` migration + modified files
- [ ] `git commit -m "feat(chats): GAR-604 — DM creation via POST /v1/groups/{id}/chats with type=dm"`
- [ ] Push + open PR

### T8 — Bookkeeping (post-merge)

- [ ] Add `- [x] POST /v1/groups/{group_id}/chats (type=dm) — plan 0115 / GAR-604, implementado 2026-05-14 (Florida)` to ROADMAP.md §3.4
- [ ] Update `plans/README.md` row 0115 with `✅ Merged YYYY-MM-DD via PR #NNN (sha)`
- [ ] Mark GAR-604 Done in Linear

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| SQLSTATE 23505 detection brittle | Low | Use `sqlx::error::DatabaseError::code()` + `"23505"` literal, same as signup_pool.rs |
| Migration 019 conflicts with existing CHECK on type | Low | CHECK is additive; existing rows have dm_user_a = NULL which satisfies `type <> 'dm'` branch |
| `chat_members` JOIN-RLS blocks partner INSERT | Low | `app.current_group_id` set before INSERT; chat row visible in same tx → JOIN succeeds |
| Race condition on concurrent DM creation | Mitigated | Unique index fires on the second INSERT; 23505 handling returns the winner |

---

## Acceptance criteria

- [ ] `POST /v1/groups/{id}/chats` with `{type: "dm", partner_user_id: "<uuid>"}` → 201 + `dm_already_exists` absent or false.
- [ ] Second identical POST → 200 + `dm_already_exists: true` + same `id`.
- [ ] Partner not in group → 404.
- [ ] `type: "dm"` without `partner_user_id` → 400.
- [ ] Self-DM → 400.
- [ ] Cross-group authz scenario passes.
- [ ] All previous `rest_v1_chats` scenarios green.
- [ ] Clippy clean, `cargo fmt --check` clean.

---

## Cross-references

- Plan 0054 §"Out of scope": deferred DM creation to a future slice.
- Plan 0076 (GAR-530): GET/PATCH/DELETE /v1/chats/{id} + member CRUD — shipped, foundational.
- `crates/garraia-auth/src/signup_pool.rs` — canonical pattern for SQLSTATE 23505 detection.
- `crates/garraia-workspace/migrations/004_chats_and_messages.sql:15` — existing `type CHECK`.
- `crates/garraia-gateway/src/rest_v1/chats.rs:81-83` — the restriction being lifted.

---

## Estimativa

- T1 (migration): 30 min
- T2 (request type update): 30 min
- T3 (handler DM path): 60 min
- T4 (response update): 15 min
- T5 (tests): 60 min
- T6 (clippy/fmt): 15 min
- T7 (commit/PR): 10 min
- **Total: ~3h 30 min**
