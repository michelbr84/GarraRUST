# Plan 0079 — GAR-539: REST /v1 tasks slice 6 (task subscriptions API)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (garra-routine 2026-05-07, America/New_York)
**Data:** 2026-05-07 (America/New_York)
**Issue:** [GAR-539](https://linear.app/chatgpt25/issue/GAR-539)
**Branch:** `routine/202605071835-task-subscriptions-api`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land the task subscriptions REST API (ROADMAP §3.8 Tier 1), delivering three
endpoints on the `garraia_app` RLS-enforced pool — all keyed to the *current
authenticated user* (no `user_id` body parameter):

- `POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user
  subscribes to the task (201 on success; 409 if already subscribed; 404 if
  task not in this group).
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — current user
  unsubscribes (204 idempotent; 404 if task not in this group).
- `GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions` — list subscribers
  for a task (200 with array; 404 if task not in this group).

## §2 Architecture

### Schema (migration 006)

```sql
CREATE TABLE task_subscriptions (
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscribed_at timestamptz NOT NULL DEFAULT now(),
    muted         boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (task_id, user_id)
);
```

### RLS (migration 006)

`task_subscriptions` — **JOIN via tasks** (FORCE RLS, same class as
`task_assignees`, `task_label_assignments`):

```sql
CREATE POLICY task_subscriptions_through_tasks ON task_subscriptions
    USING (task_id IN (SELECT id FROM tasks));
```

Because `tasks` is filtered by `app.current_group_id`, this transitively scopes
subscriptions to the current group.

### Subject = principal

Unlike `task_assignees` (where any group member can be assigned by any other
member) and `task_labels` (where the actor passes the `label_id`), subscription
operations always act on the *current user*. POST and DELETE take no body, no
`user_id` parameter. This keeps the wire surface minimal and avoids the
question of "can Alice subscribe Bob to a task?".

### Cross-group safety

- POST and DELETE first verify `task_id` exists in the current group and is
  not soft-deleted. Cross-group `task_id` returns 404 (RLS would also filter,
  but the explicit check yields clear UX).
- GET requires the same check.
- 23505 unique violation on `(task_id, user_id)` PK on POST → 409.
- DELETE is idempotent (no 404 on missing row, only 404 on missing task).

### `muted` field

Default `false`. Exposed in GET response. Mutation path (PATCH to flip muted)
is **out of scope** for this slice — it's simple and can ship in a follow-up
if the digest worker (GAR-397) needs it.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs)
- `sqlx::query` / `sqlx::query_as` — no SQL string concat
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- 2 new `WorkspaceAuditAction` variants: `TaskSubscribed`, `TaskUnsubscribed`
- `utoipa` OpenAPI annotations

## §4 Design invariants

1. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
2. Audit metadata structural-only: `{ subscriber_user_id_len: 36 }` — never
   the user's email or display name.
3. Cross-group `task_id` returns 404 (explicit check + RLS).
4. DELETE is idempotent on the subscription row (204 either way), but still
   returns 404 if the task itself is missing from this group.
5. POST 409 on SQLSTATE `23505` for the PK `(task_id, user_id)`.
6. Subject is always `principal.user_id` — no `user_id` request body.
7. `muted` defaults to `false` on insert (DB default); response exposes it
   verbatim from the row.
8. No `unwrap()` outside tests.
9. GET response sorted by `subscribed_at ASC` for stable pagination.

## §5 Validações pré-plano

- [x] `task_subscriptions` schema in migration 006 (PK + ON DELETE CASCADE).
- [x] `task_subscriptions_through_tasks` FORCE RLS JOIN policy.
- [x] `Action::TasksWrite` / `Action::TasksRead` already in `action.rs`.
- [x] `set_rls_context` parameterized SET LOCAL pattern (plan 0056).
- [x] `audit_workspace_event` signature (plans 0077, 0078).
- [x] 23505 unique violation catch pattern (plan 0078).
- [x] No body for parameterless POST — Axum 0.8 accepts empty body when no
  `Json<T>` extractor present.

## §6 Scope

**In scope:**

- `POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions`
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions`
- `GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions`
- `WorkspaceAuditAction::TaskSubscribed` + `WorkspaceAuditAction::TaskUnsubscribed`
- 7 integration test scenarios (S1–S7)
- OpenAPI registration for the 3 endpoints + the new `SubscriptionResponse`

**Out of scope:**

- `PATCH /v1/groups/{group_id}/tasks/{task_id}/subscriptions/me/mute` (toggle
  `muted`) — separate slice, ships when digest worker (GAR-397) lands.
- Subscribing other users on someone else's behalf — never, by design.
- Bulk subscribe (subscribe-all-task-list) — separate slice.
- Notification fan-out to channels — owned by GAR-397 digest worker.
- WebSocket push of subscription events — GAR-396 parent scope.

## §7 Rollback

No schema changes. If the implementation is defective, revert the 3 route
registrations in `rest_v1/mod.rs`, the 3 handler functions in `tasks.rs`, the
2 audit variants in `audit_workspace.rs`, and the OpenAPI additions in
`openapi.rs`. No migration rollback needed.

## §8 Open questions

None. All design decisions are direct echoes of plan 0078 (labels) and plan
0077 (assignees), plus the design choice that subscription subject = principal.

## §9 File structure

```
crates/
  garraia-auth/
    src/
      audit_workspace.rs        ← +2 variants (TaskSubscribed, TaskUnsubscribed)
                                   + as_str() entries
                                   + matching distinct-strings test entries
  garraia-gateway/
    Cargo.toml                  ← [[test]] required-features for the new test
    src/
      rest_v1/
        mod.rs                  ← +2 route registrations (1 path, GET+POST+DELETE
                                   share the path, plus DELETE has same path)
                                   in fact: 1 .route() entry with .post().get().delete()
        tasks.rs                ← +3 handler functions + Subscription{Row,Response}
        openapi.rs              ← +3 path entries + SubscriptionResponse schema
    tests/
      rest_v1_task_subscriptions.rs  ← NEW — 7 integration scenarios (S1–S7)
plans/
  README.md                     ← +1 row for plan 0079
  0079-gar-539-task-subscriptions-api.md  ← THIS FILE
```

## §10 M1 tasks

### T1 — Add 2 WorkspaceAuditAction variants

- [ ] Add `TaskSubscribed`, `TaskUnsubscribed` to `audit_workspace.rs`.
- [ ] Add `as_str()` entries: `"task.subscribed"`, `"task.unsubscribed"`.
- [ ] Update the distinct-strings test (now 32 entries).
- [ ] Update the per-variant `as_str()` assertion test (add 2 new asserts).
- [ ] `cargo check -p garraia-auth` green.

### T2 — Implement 3 handler functions in tasks.rs

- [ ] `subscribe_to_task` (POST, 201 on success, 409 on 23505 PK violation,
      404 if task not in group).
- [ ] `unsubscribe_from_task` (DELETE, 204 idempotent on subscription row,
      404 if task not in group).
- [ ] `list_task_subscriptions` (GET, 200 with `Vec<SubscriptionResponse>`,
      ordered by `subscribed_at ASC`, 404 if task not in group).
- [ ] New types: `SubscriptionRow` (sqlx::FromRow) + `SubscriptionResponse`
      (Serialize + utoipa::ToSchema) with fields
      `{ task_id, user_id, subscribed_at, muted }`.
- [ ] All 3 handlers call `set_rls_context(&mut tx, principal.user_id, group_id)`
      before any FORCE-RLS table access.
- [ ] Audit metadata uses `{ subscriber_user_id_len: 36 }` — PII-safe.
- [ ] `cargo check -p garraia-gateway --features test-helpers` green.

### T3 — Register routes in rest_v1/mod.rs

- [ ] One `.route("/v1/groups/{group_id}/tasks/{task_id}/subscriptions", …)`
      block with `.post(subscribe_to_task).get(list_task_subscriptions).delete(unsubscribe_from_task)`.
- [ ] Marker comment: `// Plan 0079 (GAR-539) — task subscriptions API slice 6.`
- [ ] `cargo check -p garraia-gateway --features test-helpers` green.

### T4 — Register OpenAPI in openapi.rs

- [ ] Add `super::tasks::subscribe_to_task`, `list_task_subscriptions`,
      `unsubscribe_from_task` to the `paths(...)` macro arm.
- [ ] Add `SubscriptionResponse` to the `components(...)` macro arm.
- [ ] Add `SubscriptionResponse` to the `use super::tasks::{ ... }` import.
- [ ] `cargo check -p garraia-gateway --features test-helpers` green.

### T5 — Write integration tests (tests/rest_v1_task_subscriptions.rs)

Single bundled `#[tokio::test]` (avoid sqlx runtime-teardown race per plan 0016 M3).

- [ ] **S1.** POST 201 — Alice subscribes herself to her task; verify response
      shape (`task_id`, `user_id`, `subscribed_at`, `muted=false`); audit row
      `task.subscribed` with `resource_type=task_subscriptions`,
      `resource_id={task_id}`, metadata has `subscriber_user_id_len: 36`,
      no `user_id` value or PII.
- [ ] **S2.** POST 409 — second subscribe by same user → 409 Conflict.
- [ ] **S3.** GET 200 — list returns 1 entry with Alice as subscriber.
- [ ] **S4.** DELETE 204 — Alice unsubscribes; audit row `task.unsubscribed`;
      second DELETE on same task → 204 idempotent.
- [ ] **S5.** POST 404 — Bob (different group) tries to subscribe to Alice's
      task by passing the foreign `task_id` under his own group → 404.
- [ ] **S6.** POST 403 — path `group_id` ≠ principal `group_id` (alice's token,
      bob's group in path) → 403 (covered by `check_group_match`).
- [ ] **S7.** GET 200 — empty array when no subscribers (re-list after the
      unsubscribe in S4).
- [ ] `cargo test -p garraia-gateway --features test-helpers --test rest_v1_task_subscriptions`
      green against testcontainers Postgres.

### T6 — Cargo.toml `[[test]]` gate

- [ ] Add `[[test]] name = "rest_v1_task_subscriptions"` block with
      `required-features = ["test-helpers"]`. Mirrors the existing entry
      for `rest_v1_task_labels` / `rest_v1_task_assignees`.

### T7 — Workspace-wide clippy + fmt

- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- [ ] `cargo fmt --all -- --check` clean.

### T8 — Update plans/README.md

- [ ] Add row for plan 0079 (GAR-539, this plan) — initial state
      `⏳ In Progress`. Will flip to `✅ Merged` post-merge in a follow-up
      doc-only PR.

## §11 Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Empty-body POST/DELETE rejected by Axum at 415 | Low | Don't include `Json<T>` extractor; Axum default body parser accepts empty body when no extractor demands it. Verified with `unsubscribe_from_task` (DELETE) which is parameter-only. |
| Notification fan-out side-effects expected on subscribe | Out of scope | `task.subscribed` audit row only; no notification side effect this slice. |
| GET ordering instability across subsecond subscribes | Low | Order by `subscribed_at ASC` then `user_id ASC` as tiebreaker. |
| `muted` accidentally exposed and someone tries to set it on POST | Out of scope | POST takes no body; tests validate POST returns `muted=false`; mute toggle is a future PATCH. |

## §12 Acceptance criteria

- All ≥16 actual CI workflow checks green on the PR.
- 7 integration scenarios pass against testcontainers Postgres.
- Cross-group `task_id` returns 404 (S5).
- Duplicate subscribe returns 409 (S2).
- DELETE is idempotent — second DELETE returns 204 (S4 second call).
- Path-vs-principal `group_id` mismatch returns 403 (S6).
- Audit metadata never contains the subscriber email or display name.

## §13 Cross-references

- Plan 0078 (GAR-536, task labels) — same RLS class + 23505 + audit pattern.
- Plan 0077 (GAR-533, task assignees) — same JOIN policy class.
- Plan 0069 (GAR-520, task comments) — JOIN policy class.
- ROADMAP §3.8 Tier 1 — task_subscriptions schema.
- Migration 006 (`006_tasks_with_rls.sql`) — `task_subscriptions` definition.
- GAR-397 — digest worker that fans out to subscribers (downstream consumer).

## §14 Estimativa

**Baixa:** 1.0h | **Provável:** 1.5h | **Alta:** 2.5h
