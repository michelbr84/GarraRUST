# Plan 0267 — GAR-802: GET /v1/groups/{group_id}/task-labels/{label_id}

**Goal:** Complete the CRUD for task labels by adding the missing GET single-item endpoint.
The list endpoint (`GET /v1/groups/{group_id}/task-labels`) exists; this slice adds the
per-item fetch that clients need when they have a `label_id` and need full detail.

**Linear:** [GAR-802](https://linear.app/chatgpt25/issue/GAR-802)  
**Parent epic:** [GAR-396](https://linear.app/chatgpt25/issue/GAR-396)  
**Branch:** `routine/202606061217-get-task-label`  
**Date:** 2026-06-06 (America/New_York)

---

## Architecture

Single handler `get_task_label` added to
`crates/garraia-gateway/src/rest_v1/tasks/labels.rs`.

- Auth: `Action::TasksRead` (same as list labels).
- Path params: `(group_id: Uuid, label_id: Uuid)`.
- DB: `SELECT … FROM task_labels WHERE id = $1 AND group_id = $2`
  via `fetch_optional` inside RLS-scoped transaction.
  → `None` = 404 (not found or cross-group guard, no existence leak).
- Response: `200 TaskLabelResponse` (reuses existing struct from create/patch).
- No audit event (read-only, consistent with `list_task_labels`).

## Tech stack

- Axum 0.8, sqlx 0.8, utoipa (OpenAPI), garraia-auth
- No migration needed (`task_labels` schema unchanged)

## Design invariants

- `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` before any FORCE-RLS table.
- `check_group_match` prevents cross-group UUID enumeration.
- No `unwrap()` outside tests.
- No SQL string concat; parameterized via sqlx.
- No audit event for read-only endpoint (consistent with `list_task_labels`).

## Out of scope

- Label reordering / position column.
- Any migration.
- Pagination (single-item fetch needs none).

## Rollback

Single handler addition + re-export + route registration. Revert the PR commit. No schema change.

---

## M1 — Implementation

- [x] T1: Add `get_task_label` handler + `#[utoipa::path]` annotation in `labels.rs`
- [x] T2: Update module doc comment + `pub use` exports in `tasks/mod.rs`
- [x] T3: Wire route `.get(tasks::get_task_label)` in all 3 `rest_v1/mod.rs` branches
       (full / fail-soft 503 / no-auth stub)
- [x] T4: Register `super::tasks::labels::get_task_label` in `rest_v1/openapi.rs` paths list
- [x] T5: Add 5 unit tests covering all response-shape invariants
- [x] T6: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] T7: Commit + push

## Risk register

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Route conflict with existing DELETE/PATCH | Baixa | Médio | Axum method-based routing — `.get().delete().patch()` on same path is standard |
| Cross-group label leakage | Baixa | Alto | `check_group_match` + RLS `group_id = $2` in WHERE |

## Acceptance criteria

- `GET /v1/groups/{group_id}/task-labels/{label_id}` returns 200 + `TaskLabelResponse`.
- Returns 404 for unknown `label_id` or label belonging to a different group (no existence leak).
- Returns 401/403 per standard auth rules.
- `cargo clippy --workspace` clean; all existing tests pass.
- OpenAPI schema includes the new GET path.

## Cross-references

- Plan 0078 / GAR-536 — task labels CRUD (create + list + delete + assign + remove-from-task).
- Plan 0266 / GAR-800 — PATCH task label (edit name/color).
- GAR-396 — API REST Tasks parent epic.
