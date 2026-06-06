# Plan 0266 — GAR-800: PATCH /v1/groups/{group_id}/task-labels/{label_id}

**Goal:** Close the CRUD gap in task labels — POST/GET/DELETE exist (GAR-536 / plan 0078);
PATCH (edit name/color) is missing.

**Linear:** [GAR-800](https://linear.app/chatgpt25/issue/GAR-800)  
**Parent epic:** [GAR-396](https://linear.app/chatgpt25/issue/GAR-396)  
**Branch:** `routine/202506060020-task-label-patch`  
**Date:** 2026-06-06 (America/New_York)

---

## Architecture

Single handler `patch_task_label` added to `crates/garraia-gateway/src/rest_v1/tasks/labels.rs`.

- Auth: `Action::TasksWrite` (same as create/delete label).
- Request: `PatchTaskLabelRequest { name: Option<String>, color: Option<String> }`.
  At least one field must be present (400 if both absent).
- Validation: name 1–80 chars; color must match `#RRGGBB` regex.
- DB: `UPDATE task_labels SET name = COALESCE($3, name), color = COALESCE($4, color)
  WHERE id = $1 AND group_id = $2 RETURNING *`
  → 0 rows updated = 404 (not found or cross-group).
  → SQLSTATE 23505 = 409 (duplicate name within group).
- Audit: `WorkspaceAuditAction::TaskLabelEdited` with `{ "name_len": usize, "color": "#..." }`.
  PII-safe: no raw label name in metadata.
- Response: `200 TaskLabelResponse` (same struct as create).

## Tech stack

- Axum 0.8, sqlx 0.8, utoipa (OpenAPI), garraia-auth audit_workspace
- No migration needed (task_labels schema unchanged)

## Design invariants

- `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` before any FORCE-RLS table.
- No `unwrap()` outside tests.
- No SQL string concat; parameterized via sqlx.
- No PII in audit metadata.
- `check_group_match(path_group_id, group_id)` to prevent cross-group enumeration.

## Out of scope

- `GET /v1/groups/{group_id}/task-labels/{label_id}` (not in ROADMAP; list endpoint serves discovery).
- Label reordering / position column.
- Any migration.

## Rollback

Single-file handler addition + 1 enum variant. Revert the PR commit. No schema change.

---

## M1 — Implementation

- [ ] T1: Add `TaskLabelEdited` variant to `WorkspaceAuditAction` in `audit_workspace.rs`  
       (enum variant + `as_str()` arm `"task_label.edited"` + test assertion)
- [ ] T2: Add `PatchTaskLabelRequest`, `patch_task_label` handler in `labels.rs`
- [ ] T3: Wire route `.patch(tasks::patch_task_label)` in all 3 `mod.rs` branches
- [ ] T4: Register OpenAPI path + `PatchTaskLabelRequest` schema in `openapi.rs`
- [ ] T5: Unit tests (6): serialization, nil UUID round-trip, color validation, name len guard,
          optional-both-absent 400 shape, `TaskLabelResponse` all-fields
- [ ] T6: Update `plans/README.md`, `TODO.md`

## Risk register

| Risk | Mitigation |
|---|---|
| UNIQUE(group_id, name) 409 path missed | Test fixture with duplicate name → assert 409 |
| RLS blocks UPDATE (cross-group) | `check_group_match` + `WHERE id=$1 AND group_id=$2` → 0 rows → 404 |
| Both fields absent → silent no-op | Explicit 400 before hitting DB |

## Acceptance criteria

- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → 0 warnings.
- `PATCH /v1/groups/{g}/task-labels/{l}` with `{"color":"#FF0000"}` → 200 with updated color.
- `PATCH` with `{"name":"dup"}` when another label has that name → 409.
- `PATCH` with `{}` (both absent) → 400.
- `PATCH` with unknown `label_id` → 404.
- All 6 unit tests green.

## Cross-references

- Plan 0078 (GAR-536): original label CRUD (POST/GET/DELETE).
- Plan 0264 (GAR-795): analogous PATCH for task comment body.
- Plan 0265 (GAR-798): analogous single-resource GET pattern.

## Estimativa

~4h (250–350 LOC including tests).
