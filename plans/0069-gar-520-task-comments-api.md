# Plan 0069 — GAR-520: REST /v1 tasks slice 3 (task comments API)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-520](https://linear.app/chatgpt25/issue/GAR-520)
**Branch:** `routine/202605051835-task-comments-api`
**Epic:** `epic:ws-api`
**Parent:** GAR-396

---

## §1 Goal

Land task comments REST API (ROADMAP §3.8), delivering three endpoints:

- `POST /v1/groups/{group_id}/tasks/{task_id}/comments` — create comment
- `GET /v1/groups/{group_id}/tasks/{task_id}/comments` — cursor-paginated list
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` — soft-delete

## §2 Architecture

`task_comments` uses FORCE RLS via the `task_comments_through_tasks` JOIN policy:
`USING (task_id IN (SELECT id FROM tasks))`. Since `tasks` itself is filtered by
`app.current_group_id`, this transitively scopes comments to the current group.

The SET LOCAL protocol (both `app.current_user_id` AND `app.current_group_id`)
is required for every transaction even though the comments policy is JOIN-based,
because the underlying `tasks` query uses those settings.

`author_label` is populated at POST time by querying `SELECT display_name FROM
users WHERE id = $1` inside the same transaction (same pattern as
`created_by_label` in task-list and task handlers).

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs)
- `sqlx::query_as` (no SQL string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- New `WorkspaceAuditAction` variants: `TaskCommentCreated`, `TaskCommentDeleted`
- `utoipa` OpenAPI annotations

## §4 Design invariants

1. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
2. Audit metadata is STRUCTURAL only: `body_len` not `body_md`.
3. Cross-group attempts return 404 (RLS JOIN filters; task not visible → comment
   INSERT fails or DELETE/SELECT returns 0 rows).
4. Soft-delete only — `deleted_at = now()`. No hard delete.
5. Already-deleted comment → 404 (not idempotent like task-list archive).
6. GET list returns only `deleted_at IS NULL` comments.
7. No `unwrap()` in production code.

## §5 Validações pré-plano

- [x] `task_comments` FORCE RLS migration 006 ✅
- [x] `task_comments_through_tasks` JOIN policy ✅
- [x] `Action::TasksWrite`, `Action::TasksRead`, `Action::TasksDelete` in action.rs ✅
- [x] `set_config` parameterized SQL pattern (plan 0056) ✅
- [x] `display_name` lookup pattern in tasks.rs line ~551 ✅
- [x] `audit_workspace_event` function signature confirmed ✅

## §6 Scope

**In scope:**
- `POST /v1/groups/{group_id}/tasks/{task_id}/comments`
- `GET /v1/groups/{group_id}/tasks/{task_id}/comments`
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}`

**Out of scope:**
- Comment editing (`PATCH comment`) — no `edited_at` update
- `task_subscriptions` / `task_activity` fan-out
- `@garra` mention dispatch
- RBAC per-comment authz (any group member may delete any comment for now)
- Pagination beyond cursor (no total count)

## §7 Affected files

```
crates/garraia-auth/src/audit_workspace.rs         (+2 variants + tests)
crates/garraia-gateway/src/rest_v1/tasks.rs        (+3 handlers, +5 DTOs, ~200 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs          (+2 route entries)
crates/garraia-gateway/src/rest_v1/openapi.rs      (+3 paths + schemas)
crates/garraia-gateway/tests/rest_v1_tasks.rs      (+8 scenarios)
plans/README.md                                    (+row 0069)
plans/0068-gar-518-tasks-api-slice2.md             (status → ✅ Merged)
ROADMAP.md                                         (§3.8 task API checkboxes)
```

## §8 Rollback plan

No schema migration. Revert branch on main. Fully reversible.

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| RLS JOIN not triggered (missing set_config) | Low | High | Both vars set at tx start; tests verify cross-group isolation |
| `author_label` empty string | Very Low | Low | display_name NOT NULL in users table; SELECT verified before INSERT |
| Comment body DoS (50k chars) | Very Low | None | CHECK in schema; body_md size validate at API layer |

## §10 Acceptance criteria

- `POST` returns 201 with `CommentResponse`; audit row `task.comment.created`
- `GET` returns 200 with list; excludes soft-deleted comments
- `DELETE` returns 204; comment no longer in GET; audit row `task.comment.deleted`
- Cross-group task returns 404 (RLS join blocks insert/select)
- Unknown task returns 404
- Already-deleted comment returns 404
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes

## §11 Cross-references

- Plan 0066 (GAR-516) — slice 1: task-list + task handlers
- Plan 0068 (GAR-518) — slice 2: single task GET + task-list PATCH/DELETE
- Plan 0056 — set_config parameterized SQL pattern
- Migration 006 (`task_comments` table + RLS)
- ROADMAP §3.8

## §12 Open questions

None.

## §13 Estimativa

- T1: Audit variants TaskCommentCreated + TaskCommentDeleted: 10 min
- T2: DTOs CommentRow, CommentResponse, CreateCommentRequest, ListCommentsQuery, ListCommentsResponse: 20 min
- T3: Handler create_task_comment: 25 min
- T4: Handler list_task_comments: 20 min
- T5: Handler delete_task_comment: 15 min
- T6: Route wiring + OpenAPI: 15 min
- T7: Tests (+8 scenarios): 35 min
- CI + follow-ups: 20 min
- **Total: ~2.5 hours**

## M1 Tasks

- [ ] T1: Add `TaskCommentCreated` + `TaskCommentDeleted` to `WorkspaceAuditAction`
- [ ] T2: DTOs in tasks.rs
- [ ] T3: Handler `create_task_comment`
- [ ] T4: Handler `list_task_comments`
- [ ] T5: Handler `delete_task_comment`
- [ ] T6: Route wiring + OpenAPI
- [ ] T7: Integration tests
