# Plan 0135 — Q11.a: extract rest_v1/tasks/task_lists.rs (GAR-635)

**Status:** In Progress
**Issue:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `routine/202605161215-q11-tasks-slice1`
**Epic parent:** GAR-430

---

## Goal

Extract all task-list CRUD code (5 handlers + 6 types + 1 constant) from the
monolithic `rest_v1/tasks.rs` (4236 LOC) into a new focused sub-module
`rest_v1/tasks/task_lists.rs`. Zero behaviour change; re-exports in
`tasks/mod.rs` keep every `router.rs` call-site unchanged.

`tasks.rs` becomes `tasks/mod.rs` and shrinks from **4236 → ~3550 LOC** (~−686
lines extracted into `task_lists.rs`).

---

## Architecture

Follows the pattern established in Q9.b–Q9.g (admin sub-modules):

1. New directory `crates/garraia-gateway/src/rest_v1/tasks/`.
2. New file `tasks/task_lists.rs` — owns all task-list CRUD logic.
3. Existing `tasks.rs` → `tasks/mod.rs` — removes task-list code, adds
   `pub mod task_lists;` + re-exports so all existing call-sites compile unchanged.
4. The file `rest_v1/tasks.rs` is deleted (Rust will pick up `tasks/mod.rs`).

---

## Items extracted to `task_lists.rs`

| Item | Kind |
|---|---|
| `TaskListRow` | `struct` (private) |
| `ALLOWED_LIST_TYPES` | `const` (private) |
| `CreateTaskListRequest` | `pub struct` |
| `PatchTaskListRequest` | `pub struct` |
| `TaskListResponse` | `pub struct` |
| `TaskListSummary` | `pub struct` |
| `ListTaskListsResponse` | `pub struct` |
| `ListTaskListsQuery` | `pub struct` |
| `create_task_list` | `pub async fn` |
| `list_task_lists` | `pub async fn` |
| `get_task_list` | `pub async fn` |
| `patch_task_list` | `pub async fn` |
| `delete_task_list` | `pub async fn` |

---

## Import path conventions for `task_lists.rs`

- State type: `super::super::RestV1FullState`
- Error type: `super::super::problem::RestError`
- Shared utilities: `super::{check_group_match, require_group_id, set_rls_context, DEFAULT_LIMIT, MAX_LIMIT}`
- utoipa `body =` annotations: `super::super::problem::ProblemDetails`
- `option_nullable::deserialize`: `super::option_nullable::deserialize`
  (the `option_nullable` mod stays private in `tasks/mod.rs`)

---

## Out of scope

- Any functional change to task-list logic.
- Adding new tests (zero behaviour = same coverage).
- Touching `router.rs` (all public symbols re-exported from `tasks/mod.rs`).

---

## M1 tasks

- [x] T1: Create `plans/0135-gar-635-q11-tasks-modularize-slice1.md`
- [x] T1b: Update `plans/README.md` (add row 0135)
- [ ] T2: Create `rest_v1/tasks/task_lists.rs` with all 5 handlers + types
- [ ] T2b: Create `rest_v1/tasks/mod.rs` (stripped tasks.rs + module decl + re-exports)
- [ ] T2c: Delete `rest_v1/tasks.rs`
- [ ] T3: `cargo check -p garraia-gateway` green
- [ ] T4: `cargo test -p garraia-gateway` passes
- [ ] T5: `cargo clippy -p garraia-gateway --tests --no-deps -- -D warnings` clean
- [ ] T6: `cargo fmt -p garraia-gateway`
- [ ] T7: Commit + push

---

## Acceptance criteria

- New file `rest_v1/tasks/task_lists.rs` with the 5 task-list CRUD handlers.
- `rest_v1/tasks/mod.rs` re-exports all moved public symbols.
- `cargo check -p garraia-gateway` passes.
- `cargo test -p garraia-gateway --test '*'` passes.
- `cargo clippy -p garraia-gateway --tests --no-deps -- -D warnings` clean.
- No behavioral change — pure structural refactor.

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Missing re-export breaks router.rs | Low | `cargo check` catches immediately |
| Wrong `super::` depth in task_lists.rs imports | Low | Compiler error immediate |
| `option_nullable` not accessible from child module | Low | Use `super::option_nullable::deserialize` |
| Test referencing `TaskListRow` via `super::*` fails | Low | Move test to mod.rs if needed |

---

## Cross-references

- GAR-430 (umbrella: Quality Gates Phase 3.6)
- Prior: plan 0133 / GAR-475 (Q9.f secrets.rs)
- Prior: plan 0134 / GAR-634 (tokio unblock)
