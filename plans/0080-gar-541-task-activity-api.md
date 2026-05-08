# Plan 0080 — GAR-541: REST /v1 tasks slice 7 (task activity log)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-541](https://linear.app/chatgpt25/issue/GAR-541)
**Branch:** `routine/202605080024-task-activity-api`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land the task activity log for the tasks REST API (ROADMAP §3.8 Tier 1). The
`task_activity` table (migration 006, FORCE RLS, `group_id` direct policy) was
created but never written by any handler. This slice fills that gap:

**Activity writes** — INSERT into `task_activity` inside the existing
transaction of each mutation handler (no separate tx; writes are atomic with
the mutation):

| Handler | kind |
|---------|------|
| `create_task` | `created` |
| `patch_task` — when `status` changes | `status_changed` |
| `patch_task` — when `priority` changes | `priority_changed` |
| `patch_task` — when `due_at` changes | `due_changed` |
| `delete_task` (soft-delete) | `deleted` |
| `create_task_comment` | `commented` |
| `add_task_assignee` | `assigned` |
| `remove_task_assignee` | `unassigned` |
| `assign_task_label` | `labeled` |
| `remove_task_label_from_task` | `unlabeled` |

**New GET endpoint:**

- `GET /v1/groups/{group_id}/tasks/{task_id}/activity?cursor=<uuid>&limit=<n>`
  — cursor-paginated activity log (newest first), returns
  `[{id, kind, actor_label, payload, created_at}]`.

## §2 Architecture

### Schema (migration 006)

```sql
CREATE TABLE task_activity (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    group_id      uuid        NOT NULL,                       -- denormalized
    actor_user_id uuid,                                       -- NO FK (survives user deletion)
    actor_label   text        NOT NULL CHECK (length(actor_label) BETWEEN 1 AND 200),
    kind          text        NOT NULL
                  CHECK (kind IN (
                      'created', 'status_changed', 'priority_changed',
                      'assigned', 'unassigned', 'labeled', 'unlabeled',
                      'commented', 'due_changed', 'archived', 'deleted', 'restored'
                  )),
    payload       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now()
);
```

FORCE RLS: `task_activity_group_isolation` policy on `group_id` (direct,
same as `tasks`). The `actor_user_id` column is a plain UUID with no FK —
rows survive user hard-deletion, same pattern as `audit_events.actor_user_id`.

### Activity write helper

A private async function `insert_task_activity(tx, task_id, group_id,
actor_user_id, actor_label, kind, payload)` — one call site per kind.
No separate transaction; called inside the caller's existing `tx`.

### Payload shape (PII-safe)

| kind | payload |
|------|---------|
| `created` | `{}` |
| `status_changed` | `{"old": "todo", "new": "in_progress"}` |
| `priority_changed` | `{"old": "none", "new": "high"}` |
| `due_changed` | `{"set": true}` or `{"set": false}` (boolean only — no timestamp in PII) |
| `deleted` | `{}` |
| `commented` | `{"body_len": N}` |
| `assigned` | `{"assignee_id": "<uuid>"}` |
| `unassigned` | `{"assignee_id": "<uuid>"}` |
| `labeled` | `{"label_id": "<uuid>"}` |
| `unlabeled` | `{"label_id": "<uuid>"}` |

Note: UUIDs are safe (not PII). The `due_changed` payload carries only a
boolean to avoid leaking timestamps as metadata.

### actor_label resolution

Same pattern as `author_label` in `create_task_comment`:
```sql
SELECT display_name FROM users WHERE id = $1
```
This SELECT runs inside the same transaction (so it reads the
`garraia_app`-visible user row). actor_label is cached at insert time.

### GET /activity endpoint

- Validates `task_id` exists in group (same `tasks WHERE id=$1 AND group_id=$2
  AND deleted_at IS NULL` check).
- Cursor: UUID of last seen activity row (`created_at DESC, id DESC` ordering).
- Default `limit=50`, max `limit=100`.
- Returns `{ items: [...], next_cursor: uuid | null }`.

### Cross-group safety

- All writes happen inside an existing `tx` that has already called
  `set_rls_context(group_id)` — RLS enforces group isolation automatically.
- The GET endpoint calls `set_rls_context` and validates `task_id` in `group_id`
  before the SELECT.
- `group_id` on every inserted row is taken from `principal.group_id` (via
  `require_group_id`) — never from the body.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs)
- `sqlx::query` / `sqlx::query_as` — no SQL string concat
- `serde_json::json!` macro for payload values

## §4 Design invariants

1. Activity writes are **fire-and-forget within tx** — if `insert_task_activity`
   fails, the outer handler returns `Internal` error. We don't swallow.
2. `actor_label` is always a non-empty string (CHECK 1-200 chars). Use
   `display_name` from users; fall back to `"(unknown)"` only if the user
   row is not found (defensive — should never happen for a valid principal).
3. `group_id` in `task_activity` MUST equal `tasks.group_id` for the same task.
   The caller always passes `group_id` from `require_group_id(&principal)`.
4. `payload` must never contain PII: no `body_md`, no display names, no emails.

## §5 Validações pré-plano

- [x] `task_activity` table exists in migration 006 with FORCE RLS ✅
- [x] `kind` CHECK (12 values) covers all planned writes ✅
- [x] `set_rls_context` helper already exists in tasks.rs (line ~490) ✅
- [x] `author_label` SELECT pattern established in `create_task_comment` ✅
- [x] No existing writes to `task_activity` (confirmed via grep) ✅
- [x] GET endpoint follows same cursor pattern as `list_task_comments` ✅

## §6 Out of scope

- `archived`, `restored` kinds (task-list archival is a list-level event, not a
  task-level event — deferred).
- `patch_task` writes for `title` or `description` changes (text diffs are
  expensive/PII-heavy — deferred).
- Fan-out to notification channels (GAR-397 digest worker).
- WebSocket push for real-time activity updates.
- PATCH task comment (edit) — separate slice.

## §7 Rollback

All writes are inside existing transactions. If the activity write fails, the
whole mutation rolls back. No migration change; no schema change.

## §8 File structure

```
crates/garraia-gateway/src/rest_v1/
  tasks.rs               — add helper + modify 10 handlers + new GET handler
  mod.rs                 — add GET /activity route + fail-soft 503 stubs
crates/garraia-gateway/tests/
  rest_v1_tasks_activity.rs   — integration tests (8 scenarios)
```

## §9 Tasks

### T1 — Activity write helper + `create_task` write

- [ ] Add `insert_task_activity(tx, task_id, group_id, actor_user_id,
  actor_label, kind, payload)` private async fn in tasks.rs.
- [ ] Add `actor_label` fetch (SELECT display_name) to `create_task`.
- [ ] Call `insert_task_activity` with kind `'created'` at end of `create_task`
  tx (before commit).
- [ ] `cargo check -p garraia-gateway` green.

### T2 — `patch_task` activity writes (status / priority / due)

- [ ] In `patch_task`, after the UPDATE query, detect which fields changed by
  comparing the PATCH body (non-null fields):
  - `status` field present → emit `status_changed` with `{old, new}`.
  - `priority` field present → emit `priority_changed` with `{old, new}`.
  - `due_at` present (including explicit null clear) → emit `due_changed` with
    `{set: bool}`.
- [ ] Fetch old values from the RETURNING clause or a pre-UPDATE SELECT.
- [ ] `cargo check -p garraia-gateway` green.

### T3 — `delete_task` + `create_task_comment` + comment body_len

- [ ] Add activity write `'deleted'` to `delete_task` (after soft-delete UPDATE).
- [ ] Add activity write `'commented'` to `create_task_comment` (after INSERT).
  payload: `{body_len: N}`.
- [ ] Both writes share the existing `actor_label` SELECT already in
  `create_task_comment`; `delete_task` needs its own SELECT.
- [ ] `cargo check -p garraia-gateway` green.

### T4 — Assignee + label activity writes

- [ ] `add_task_assignee` → kind `'assigned'`, payload `{assignee_id: uuid_str}`.
- [ ] `remove_task_assignee` → kind `'unassigned'`, payload `{assignee_id: uuid_str}`.
- [ ] `assign_task_label` → kind `'labeled'`, payload `{label_id: uuid_str}`.
- [ ] `remove_task_label_from_task` → kind `'unlabeled'`, payload `{label_id: uuid_str}`.
- [ ] `cargo check -p garraia-gateway` green.

### T5 — GET /activity endpoint + route wiring

- [ ] Add `ActivityRow` struct + `ActivityResponse` (id, kind, actor_label,
  payload, created_at).
- [ ] Add `list_task_activity` handler with cursor pagination (same pattern as
  `list_task_comments`).
- [ ] Wire route `GET /v1/groups/{group_id}/tasks/{task_id}/activity` in
  `mod.rs` (full-state block + fail-soft 503 stubs).
- [ ] `cargo check -p garraia-gateway` green.

### T6 — Integration tests

- [ ] `tests/rest_v1_tasks_activity.rs` with 8 scenarios:
  - A1: create_task → GET activity returns 1 row kind=`created`.
  - A2: patch_task status → GET activity returns `status_changed` row.
  - A3: patch_task priority → GET activity returns `priority_changed` row.
  - A4: delete_task → GET activity returns `deleted` row.
  - A5: comment on task → GET activity returns `commented` row.
  - A6: assign user → GET activity returns `assigned` row.
  - A7: cross-group: user in group A cannot GET activity of task in group B (404).
  - A8: cursor pagination — limit=1, verify next_cursor + second page.
- [ ] `cargo test -p garraia-gateway --test rest_v1_tasks_activity` green.

### T7 — Clippy + fmt

- [ ] `cargo fmt --all` clean.
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop
  --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

### T8 — Update ROADMAP.md + plans/README.md

- [ ] Mark `task_activity` writes + GET endpoint as `[x]` in ROADMAP §3.4.
- [ ] Add plan 0080 row to `plans/README.md` with status "Em execução".

## §10 Risk register

| Risk | Mitigation |
|------|-----------|
| `patch_task` needs old values for payload | Add pre-UPDATE SELECT or use RETURNING in a CTE |
| actor_label SELECT adds one round-trip per mutation | Acceptable — already done in `create_task_comment`; batch fetch if profiling shows cost |
| `task_activity` fill with `remove_task_label` which has no tx today | Verify if it uses a tx; add one if needed |

## §11 Acceptance criteria

1. After each mutation (create, patch status, patch priority, patch due_at,
   delete, comment, assign, unassign, label, unlabel), `GET …/activity` returns
   the corresponding `kind` row.
2. `payload` contains only the documented fields — no PII.
3. Cross-group test A7 returns 404 (task not visible via RLS).
4. Cursor pagination test A8 passes.
5. All 18 CI checks green on the PR.

## §12 Cross-references

- ROADMAP §3.4 Fase 3.4 — Tasks API Tier 1
- Migration 006 (`crates/garraia-workspace/migrations/006_tasks_with_rls.sql`)
- GAR-396 (parent epic: API REST Tasks + WebSocket kanban colaborativo)
- Plans 0066/0068/0069/0077/0078/0079 (previous tasks slices 1-6)

## §13 Estimativa

~400 LOC (handler modifications + new handler + tests). 1 sessão.
