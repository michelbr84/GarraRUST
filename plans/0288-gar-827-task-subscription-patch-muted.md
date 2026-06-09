# Plan 0288 — GAR-827: PATCH /v1/groups/{group_id}/tasks/{task_id}/subscriptions — update muted flag

## Goal

Add `PATCH /v1/groups/{group_id}/tasks/{task_id}/subscriptions` to let the
authenticated caller toggle their own subscription's `muted` boolean in-place.
Closes the CRUD gap in task subscriptions (POST/GET/DELETE already exist via
plan 0079 / GAR-539; PATCH was missing).

## Architecture

Additive handler in `crates/garraia-gateway/src/rest_v1/tasks/subscriptions.rs`.
No migration required — `task_subscriptions.muted boolean NOT NULL DEFAULT false`
already exists (migration 006).

Single UPDATE-RETURNING query under FORCE RLS context. Returns 404 when the
caller has no subscription row for this task (no existence leak — same behaviour
as unsubscribe's idempotent DELETE).

## Tech stack

- Rust / Axum 0.8
- `sqlx::query_as!` pattern: UPDATE…RETURNING
- `garraia_auth::{Action, Principal, can}` for authz
- No audit event: muted is a notification preference, not a membership change
  (contrast with TaskSubscribed / TaskUnsubscribed which track membership)
- utoipa annotation for OpenAPI

## Design invariants

- SET LOCAL `app.current_user_id` AND `app.current_group_id` before any SQL
  (FORCE RLS requirement).
- Only the calling user's own subscription row is touched
  (`WHERE task_id = $1 AND user_id = $caller_id`).
- No existence leak: 404 for "not subscribed" AND for "task not in group" (both
  yield 0 rows from UPDATE RETURNING under RLS).
- Auth: `TasksWrite` (consistent with subscribe/unsubscribe mutation semantics).

## Validações pré-plano

- [x] `task_subscriptions.muted` column confirmed in migration 006.
- [x] POST/GET/DELETE subscription handlers reviewed in `subscriptions.rs`.
- [x] No existing PATCH handler on this route (confirmed via grep).
- [x] Route only present in branch-1 (full) of `mod.rs`; branches 2+3 missing
      subscriptions entirely — will be added as unconfigured_handler stubs.

## Out of scope

- Cursor-paginación or filtering for GET subscriptions.
- Audit event for muted preference change.
- Bulk-mute (mute all subscriptions for a task).
- Cross-user muting (admin silencing another user's notifications).

## Rollback

Handler-only addition. Removing the PATCH route and re-deploying restores
prior behaviour; no schema change to undo.

## File structure (changes)

```
crates/garraia-gateway/src/rest_v1/tasks/subscriptions.rs  (+80 lines)
  PatchSubscriptionRequest struct + patch_task_subscription handler + 6 tests

crates/garraia-gateway/src/rest_v1/tasks/mod.rs           (+3 lines)
  pub use subscriptions::{..., patch_task_subscription, PatchSubscriptionRequest,
                          __path_patch_task_subscription, ...}

crates/garraia-gateway/src/rest_v1/mod.rs                 (+14 lines)
  branch-1: .patch(tasks::patch_task_subscription) on subscriptions route
  branch-2: add subscriptions stub (fix oversight)
  branch-3: add subscriptions stub (fix oversight)

crates/garraia-gateway/src/rest_v1/openapi.rs             (+3 lines)
  paths: super::tasks::subscriptions::patch_task_subscription
  components: PatchSubscriptionRequest

ROADMAP.md                                                 (+1 line)
  §3.8 Tasks REST API: [x] PATCH /v1/.../subscriptions

plans/README.md                                            (+1 row)
```

## M1 tasks

- [x] T1: Add `PatchSubscriptionRequest` + `patch_task_subscription` + tests to `subscriptions.rs`
- [x] T2: Wire route in `mod.rs` (all 3 branches) + fix branch-2/3 oversight
- [x] T3: Register in `openapi.rs`
- [x] T4: Update `tasks/mod.rs` re-exports
- [x] T5: Update ROADMAP.md and plans/README.md
- [x] T6: `cargo clippy` clean; push + open PR

## Risk register

| Risk | Mitigation |
|---|---|
| RLS filters caller's own row for wrong group_id | same SET LOCAL pattern as all other task handlers |
| UPDATE RETURNING returns wrong row | WHERE clause pins both `task_id` AND `user_id = $caller_id` |
| Branch-2/3 oversight fix changes 404→503 for subscriptions stubs | correct: route exists, just unconfigured |

## Acceptance criteria

- `PATCH .../subscriptions` with `{"muted": true}` → 200 `muted: true`
- `PATCH .../subscriptions` with `{"muted": false}` → 200 `muted: false`
- Not subscribed → 404
- `cargo clippy --workspace --tests --exclude garraia-desktop -- -D warnings` clean
- `cargo test` passes

## Cross-references

- plan 0079 / GAR-539 — original POST/GET/DELETE subscription implementation
- Migration 006 `task_subscriptions.muted`
- Linear: [GAR-827](https://linear.app/chatgpt25/issue/GAR-827)
- ROADMAP §3.8 Tasks REST API

## Estimativa

0.5 h (todos os padrões já estabelecidos — cópia + adaptação do DELETE handler)
