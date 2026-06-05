# Plan 0264 — GAR-795: PATCH /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}

**Status:** In Progress
**Issue:** [GAR-795](https://linear.app/chatgpt25/issue/GAR-795)
**Branch:** `routine/202506051215-task-comment-patch`
**Epic:** GAR-396 (Tasks + WebSocket kanban)
**Labels:** `epic:ws-tasks`, `epic:ws-api`

---

## Goal

Close the CRUD gap in task comments by adding `PATCH` (edit body). `POST`, `GET`, and `DELETE` were delivered in plan 0069 / GAR-520. The `edited_at` column already exists in `task_comments` (migration 006) — no migration needed.

---

## Architecture

Single new handler `patch_task_comment` in `crates/garraia-gateway/src/rest_v1/tasks/comments.rs`, following the pattern of `patch_message` in `messages.rs` (plan 0107 / GAR-592).

**Sender-only guard**: returns 404 for comments authored by a different user (no existence leak — same invariant as messages PATCH). Admin override is intentionally excluded; task admins who need to moderate a comment should DELETE it and re-create, keeping audit trail clean.

---

## Tech stack

- Rust / Axum 0.8 / sqlx (Postgres)
- `garraia-auth`: `Action::TasksWrite`, `WorkspaceAuditAction::TaskCommentEdited` (new variant)
- `garraia-workspace`: `task_comments` table (migration 006) — `edited_at` column already present
- utoipa for OpenAPI annotations

---

## Design invariants

- `body_md` validated 1–50,000 chars (identical to CREATE).
- SQL UPDATE uses `WHERE ... AND author_user_id = $caller` — sender-only, atomic.
- `edited_at = now()` set in the same UPDATE statement.
- Audit metadata: `{ "body_len": N }` — body content is PII, never logged.
- `insert_task_activity` kind `"comment_edited"` for the kanban activity feed.
- No migration: `task_comments.edited_at` nullable timestamp already in migration 006.

---

## Validações pré-plano

- [x] `CommentRow` already fetches `edited_at` in GET list — no DB schema change.
- [x] `WorkspaceAuditAction::TaskCommentCreated` / `TaskCommentDeleted` exist; `TaskCommentEdited` does not.
- [x] `patch_message` pattern is well-understood and safe — reuse verbatim.
- [x] No circular dependency introduced.

---

## Out of scope

- Admin override for editing other users' comments (delete + recreate is the workflow).
- `PATCH` for task attachments, assignees, or labels (separate issues).
- Rate limiting per-comment edit (future).

---

## Rollback

This is an additive handler. Rollback = revert commits on branch before merge. No DB schema change means no migration rollback needed.

---

## File structure (changes)

```
crates/
  garraia-auth/src/audit_workspace.rs         — +TaskCommentEdited variant + as_str()
  garraia-gateway/src/rest_v1/
    tasks/
      comments.rs                              — +EditCommentRequest, +EditedCommentResponse,
                                                 +patch_task_comment handler, +≥6 unit tests
      mod.rs                                   — +pub use patch_task_comment, EditedCommentResponse,
                                                 +__path_patch_task_comment
    mod.rs                                     — +patch(tasks::patch_task_comment) in all 3 routers
    openapi.rs                                 — +patch_task_comment path, +EditedCommentResponse component
ROADMAP.md                                     — +[x] PATCH /v1/.../comments/{id}
plans/README.md                                — +row for plan 0264
TODO.md                                        — update Concluído nesta sessão
```

---

## M1 task checklist

- [x] T1: `WorkspaceAuditAction::TaskCommentEdited` variant in `audit_workspace.rs`
- [x] T2: `EditCommentRequest` + `EditedCommentResponse` types in `comments.rs`
- [x] T3: `patch_task_comment` handler (TDD: unit tests first → red → impl → green)
- [x] T4: Export + route wiring in `tasks/mod.rs` and `rest_v1/mod.rs` (all 3 routers)
- [x] T5: OpenAPI annotation + component registration in `openapi.rs`
- [x] T6: ROADMAP + plans/README + TODO update
- [x] T7: `cargo check -p garraia-gateway` + `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` pass

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| `TaskCommentEdited` enum variant breaks exhaustive match elsewhere | Low | Only `as_str()` match exists; grep confirms 2 sites |
| Clippy lint on new code | Low | Follow existing code style exactly |

---

## Acceptance criteria

1. `PATCH /v1/groups/{gid}/tasks/{tid}/comments/{cid}` → 200 + `{ id, task_id, body_md, edited_at }` for happy path.
2. → 404 for unknown, deleted, cross-tenant, or different-author comment.
3. → 400 for empty or >50k body.
4. `TaskCommentEdited` audit event in `audit_events` with `body_len`.
5. `task_activity` row with kind `"comment_edited"`.
6. Route live in all 3 router branches.
7. ≥ 6 unit tests green.
8. CI green (≥ 16 checks).

---

## Cross-references

- Plan 0069 / GAR-520 — task comments POST/GET/DELETE (foundation)
- Plan 0107 / GAR-592 — messages PATCH (pattern reference)
- Migration 006 — `task_comments.edited_at` column

---

## Estimativa

~200 LOC implementation + tests. ~1h implementation time.
