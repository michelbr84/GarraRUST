# Plan 0082 — GAR-544: REST /v1 tasks slice 8 (move task between lists)

**Status:** ✅ Merged 2026-05-08 via PR #214 (`6232ec1`)
**Autor:** Claude Opus 4.7 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-544](https://linear.app/chatgpt25/issue/GAR-544) — Done
**Branch:** `routine/202605081115-task-move-api` (deletada após merge)
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §0 Path-scheme amendment

GAR-544 spec wrote `POST /v1/groups/{group_id}/tasks/{task_id}:move`
(Google API verb-suffix style). Axum 0.8 / matchit 0.8 routes named
parameters `{name}` greedily over the entire URL segment between `/`
delimiters — `:move` would be absorbed into `task_id` (ex.: `task_id =
"<uuid>:move"`), so the literal Google-style suffix is unroutable
without writing a custom router.

**Decision:** use `POST /v1/groups/{group_id}/tasks/{task_id}/move`
(action sub-segment), matching the existing convention for tasks-API
sibling routes (`/comments`, `/assignees`, `/labels`, `/subscriptions`,
`/activity`). Linear issue GAR-544 amended via comment with this rationale.

## §1 Goal

Land the task-move REST endpoint (ROADMAP §3.4 / §3.8 Tier 1):

* `POST /v1/groups/{group_id}/tasks/{task_id}/move` — move a task to a
  different task list within the same group. Returns `200` with the
  updated task body (same shape as `GET /v1/groups/{group_id}/tasks/{task_id}`).

The operation updates `tasks.list_id` to point to a new `task_lists.id`.
No position/ordering column exists yet (migration 006 comment: "position
column (kanban ordering) → out of scope, future migration"), so this
slice only moves across lists, not within a list.

**Request body:** `{ "target_list_id": "<uuid>" }`

**Activity log entry:** kind = `'moved'` with payload
`{ "from_list_id": "<uuid>", "to_list_id": "<uuid>" }`. The
`task_activity.kind` CHECK constraint (migration 006) does not currently
include `'moved'` — this slice ships migration 016 to extend it.

**Audit event:** new `WorkspaceAuditAction::TaskMoved` → `"task.moved"`
with metadata `{ "from_list_id": "<uuid>", "to_list_id": "<uuid>" }`
(UUIDs are not PII).

## §2 Architecture

### Schema delta — migration 016

Forward-only `ALTER TABLE` to extend the `task_activity.kind` CHECK
constraint with `'moved'`:

```sql
ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;
ALTER TABLE task_activity
    ADD CONSTRAINT task_activity_kind_check
    CHECK (kind IN (
        'created', 'status_changed', 'priority_changed',
        'assigned', 'unassigned', 'labeled', 'unlabeled',
        'commented', 'due_changed', 'archived', 'deleted', 'restored',
        'moved'
    ));
```

The constraint name `task_activity_kind_check` is the Postgres default
for inline `CHECK (...)` on column `kind`. Verified empirically against
migration 006 (no explicit `CONSTRAINT name`). If the runtime name turns
out to differ, migration 016 will look it up via `pg_constraint`.

### Move handler (`move_task`)

Location: `crates/garraia-gateway/src/rest_v1/tasks.rs` (single new pub
async fn at the slice 8 section, mirroring `patch_task`'s shape).

```text
1. require_group_id(&principal) → group_id
2. check_group_match(path_group_id, group_id)
3. can(&principal, Action::TasksWrite) || 403
4. Begin tx; set_rls_context(user_id, group_id)
5. Pre-fetch current list_id:
     SELECT list_id FROM tasks
     WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL
   → 404 if None
6. Validate target list exists, same group, not archived:
     SELECT 1 FROM task_lists
     WHERE id = $1 AND group_id = $2 AND archived_at IS NULL
   → 404 if None
7. If old_list_id == target_list_id: skip UPDATE + activity (idempotent),
   re-SELECT TaskRow, return 200.
8. UPDATE tasks SET list_id = $1, updated_at = now()
   WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL
   RETURNING <full TaskRow>
9. Fetch actor_label (display_name)
10. insert_task_activity(kind='moved',
        payload={from_list_id, to_list_id})
11. audit_workspace_event(TaskMoved, metadata={from_list_id, to_list_id})
12. tx.commit()
13. Return 200 + TaskResponse::from(row)
```

The compound FK `(list_id, group_id) → task_lists(id, group_id)`
on `tasks` already enforces same-group at the DB layer — even if
the app layer were buggy, attempting to point `list_id` at a
list in another group would fail at INSERT/UPDATE time. The §2.6
SELECT is a defense-in-depth check that returns the human-friendly
`404` instead of a `23503` foreign-key error.

### Idempotency

If `target_list_id == current list_id`:

* No `UPDATE` is issued (avoids spurious `updated_at` bumps).
* No `task_activity` row is written (avoids polluting the timeline
  with no-op events).
* No audit event is logged.
* Response is still `200` with the unchanged task body.

This matches the convention from `delete_task` (already-deleted = 404)
and `patch_task` (no-op fields are ignored), favoring **observable
no-op** semantics over loud errors. Test M5 asserts this.

### `WorkspaceAuditAction::TaskMoved`

Add a new enum variant in
`crates/garraia-auth/src/audit_workspace.rs`:

```rust
TaskMoved => "task.moved",
```

Plus the existing test in `as_str_returns_stable_strings` and
`from_str_round_trips` gets an assertion line.

## §3 Tech stack

* Axum 0.8 + `RestV1FullState`
* `sqlx::query` / `sqlx::query_as` (Postgres) — no SQL string concat
* `serde_json::json!` macro for payload values
* `utoipa::path` for OpenAPI doc

## §4 Design invariants

1. `group_id` in path MUST equal `principal.group_id` (403 otherwise).
2. The handler MUST `set_rls_context` before any read or write — RLS
   filter is the primary cross-group isolation, not the app-layer SELECT.
3. The DB-layer compound FK `(list_id, group_id) → task_lists(id, group_id)`
   is the secondary line of defense — even a buggy handler cannot point
   `list_id` at a foreign-group list.
4. Activity payload contains only UUIDs (`from_list_id`, `to_list_id`).
   No PII (no list names, no task titles).
5. Audit metadata contains only UUIDs (same rule).
6. Idempotent self-move (target == current) returns 200 with no
   side effects (no `updated_at` bump, no activity, no audit).
7. Soft-deleted tasks return 404, not 200 (matches `delete_task` semantics).
8. Archived target lists return 404 (cannot move into a hidden bucket).

## §5 Validações pré-plano

- [x] `task_activity.kind` CHECK lacks `'moved'` — migration 016 needed ✅
  (verified `crates/garraia-workspace/migrations/006_tasks_with_rls.sql:204-208`).
- [x] Migration sequence is forward-only — last is 015 ✅.
- [x] `WorkspaceAuditAction` does NOT yet have `TaskMoved` ✅
  (verified `crates/garraia-auth/src/audit_workspace.rs:375-393`).
- [x] `set_rls_context`, `insert_task_activity`, `require_group_id`,
  `check_group_match` helpers exist ✅
  (`tasks.rs:490, 508, 534, 540`).
- [x] `Action::TasksWrite` exists in `garraia-auth::Action` ✅.
- [x] `TaskRow` / `TaskResponse` types reusable ✅
  (`tasks.rs:118, 348`).
- [x] Compound FK `(list_id, group_id) → task_lists(id, group_id)` on
  `tasks` exists ✅ (`006_tasks_with_rls.sql:93`).
- [x] Axum 0.8 / matchit 0.8 cannot route Google `:move` suffix —
  decision in §0 ✅.

## §6 Out of scope

* In-list reordering (`position` column does not exist).
* Moving across groups (forbidden by compound FK; out of scope for this
  slice — `403/404` is acceptable).
* Move between archived and unarchived state (use `PATCH` on
  `task_lists`).
* WebSocket fan-out for kanban UI (GAR-397 digest worker).
* Bulk move (`POST /v1/groups/{group_id}/tasks:move-many`) — separate slice.
* Subtask cascade (parent_task_id is preserved; subtasks keep their own
  list_id) — already handled by schema (no cascade on list_id).

## §7 Rollback

* Migration 016 is forward-only. To revert, write a 017 that drops `'moved'`
  from the CHECK *after* deleting any rows with that kind (the CHECK reject
  on existing data otherwise).
* Handler is additive (new pub fn + new route line). Removing the route
  + deleting the fn is a clean revert.
* Audit enum variant is additive. Removing it requires checking that no
  persisted `audit_events.action_type = 'task.moved'` rows reference it,
  but `WorkspaceAuditAction` enum is only used in writes — read paths
  treat `action_type` as opaque text.

## §8 File structure

```
crates/garraia-workspace/migrations/
  016_task_activity_moved_kind.sql    — NEW

crates/garraia-auth/src/
  audit_workspace.rs                  — add TaskMoved variant + as_str + tests

crates/garraia-gateway/src/rest_v1/
  tasks.rs                            — add MoveTaskRequest + move_task fn (~120 LOC)
  mod.rs                              — wire route in 3 blocks (full + 2 fail-soft)

crates/garraia-gateway/tests/
  rest_v1_task_move.rs                — NEW bundled test (~350 LOC, 8 scenarios)

ROADMAP.md                            — flip slice 8 from [ ] → [x]
plans/README.md                       — add row for plan 0082
```

## §9 Tasks

### T1 — Migration 016 (`task_activity.kind` CHECK adds `'moved'`)

- [ ] Create `crates/garraia-workspace/migrations/016_task_activity_moved_kind.sql`
  with `ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;`
  + recreate with `'moved'` appended.
- [ ] Append migration 016 to the embed list (verify how 014/015 are referenced).
- [ ] `cargo check -p garraia-workspace` green.

### T2 — `WorkspaceAuditAction::TaskMoved`

- [ ] Add variant + match arm in `audit_workspace.rs::as_str` →
  `"task.moved"`.
- [ ] Update `from_str` if it exists (round-trip).
- [ ] Update `as_str_returns_stable_strings` test with the new assertion.
- [ ] `cargo test -p garraia-auth` green.

### T3 — `move_task` handler + route wiring

- [ ] Add `MoveTaskRequest { target_list_id: Uuid }` struct with
  `#[derive(Serialize, Deserialize, ToSchema)]`.
- [ ] Add `pub async fn move_task(...)` in `tasks.rs` (~120 LOC) with
  `#[utoipa::path]` doc.
- [ ] Wire route `POST /v1/groups/{group_id}/tasks/{task_id}/move` in
  `mod.rs` (full-state block + 2 fail-soft 503 stubs).
- [ ] `cargo check -p garraia-gateway` green.

### T4 — Integration tests

- [ ] Create `tests/rest_v1_task_move.rs` with 8 scenarios in ONE
  `#[tokio::test] async fn rest_v1_task_move_scenarios()` (sqlx
  runtime-teardown rule from plan 0016 M3):

  * **M1.** Happy path: 2 lists in same group → POST move → 200 + task
    body shows new list_id; activity row kind=`moved` with
    `{from_list_id, to_list_id}`; audit row `task.moved` present.
  * **M2.** Idempotent self-move: target == current → 200, no new
    activity row written, no new audit row, `updated_at` unchanged.
  * **M3.** Unknown task_id → 404.
  * **M4.** Soft-deleted task → 404.
  * **M5.** Unknown target_list_id → 404.
  * **M6.** Archived target_list_id → 404.
  * **M7.** Cross-group target_list_id (list owned by group B) → 404
    (RLS filters; same SELECT returns 0 rows).
  * **M8.** Cross-group task (task in group B from group A's principal)
    → 404 (existing isolation pattern; matches T7/T9 in `rest_v1_tasks.rs`).
- [ ] `cargo test -p garraia-gateway --test rest_v1_task_move` green.

### T5 — Clippy + fmt

- [ ] `cargo fmt --all` clean.
- [ ] `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip
  cargo clippy --workspace --tests --exclude garraia-desktop
  --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

### T6 — Update ROADMAP.md + plans/README.md

- [ ] Mark slice 8 (`POST tasks/{id}/move`) as `[x]` in ROADMAP §3.4.
- [ ] Add plan 0082 row to `plans/README.md` with status "Em execução".

## §10 Risk register

| Risk | Mitigation |
|------|-----------|
| Constraint name `task_activity_kind_check` not the Postgres default | If `ALTER TABLE … DROP CONSTRAINT` errors, fall back to `DO $$ DECLARE n text; BEGIN SELECT conname INTO n FROM pg_constraint WHERE conrelid = 'task_activity'::regclass AND contype = 'c' AND pg_get_constraintdef(oid) LIKE '%kind%'; EXECUTE 'ALTER TABLE task_activity DROP CONSTRAINT ' \|\| n; END $$;`. |
| Compound FK rejects target_list_id before our SELECT | Keep the §2.6 SELECT — it gives a clean 404 instead of a 500-via-23503 leak. |
| Migration 016 vs 015 race during deployment | Forward-only; migrations are sequential — 016 only runs after 015. |
| Idempotent self-move masking bugs | Test M2 asserts no audit / no activity / no updated_at bump — bug-mask is detectable. |

## §11 Acceptance criteria

1. `POST /v1/groups/{gid}/tasks/{tid}/move` with valid `target_list_id`
   returns `200` + updated task; `tasks.list_id` is the new value.
2. Activity row kind=`'moved'` with payload
   `{from_list_id, to_list_id}` recorded.
3. Audit event `task.moved` with metadata
   `{from_list_id, to_list_id}` recorded.
4. Cross-group target → 404. Cross-group task → 404. Soft-deleted task → 404.
   Archived target list → 404.
5. Idempotent self-move → 200, no new activity, no new audit,
   `updated_at` unchanged.
6. All 17+ CI checks green on the PR.
7. `task_activity.kind` CHECK constraint includes `'moved'` (migration 016
   applied).

## §12 Open questions

* **Q1.** Should `move` allow targeting an archived list with an explicit
  `--unarchive` flag? **Decided: no** — out of scope; user must unarchive
  first via PATCH on `task_lists`.
* **Q2.** Should the activity row include `from_list_label` / `to_list_label`
  for display? **Decided: no** — UUIDs only; UI can JOIN against
  `task_lists` for names. Avoids stale labels and keeps payload PII-free.
* **Q3.** Should bulk move be in this slice? **Decided: no** — separate
  slice; spec is single-task only. Bulk surface needs its own design
  (atomicity, partial-failure semantics).
* **Q4.** What about subtasks? **Decided:** the schema does NOT cascade
  `list_id` from parent to children — moving the parent does not
  reparent subtasks. Documented in §6 out of scope.

## §13 Cross-references

* ROADMAP §3.4 Fase 3.4 — Tasks API Tier 1.
* Migration 006 (`crates/garraia-workspace/migrations/006_tasks_with_rls.sql`).
* GAR-396 (parent epic: API REST Tasks + WebSocket kanban colaborativo).
* Plans 0066/0068/0069/0077/0078/0079/0080 (previous tasks slices 1-7).
* `crates/garraia-auth/src/audit_workspace.rs` (audit enum).

## §14 Estimativa

~500 LOC total (~50 LOC migration, ~10 LOC enum, ~120 LOC handler,
~350 LOC tests). 1 sessão.
