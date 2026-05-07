# Plan 0079 — GAR-539: REST /v1 tasks slice 6: task subscriptions API

## Goal

Land three task-subscription endpoints on the `garraia_app` RLS-enforced pool,
completing the self-service "watch/unwatch a task" surface. The `task_subscriptions`
table already exists (migration 006, FORCE RLS via JOIN policy through `tasks`).

## Endpoints delivered

| Method | Path | Auth | Action |
|--------|------|------|--------|
| `POST` | `/v1/groups/{group_id}/tasks/{task_id}/subscriptions` | `TasksRead` | Subscribe caller to task (201 / 409 duplicate) |
| `DELETE` | `/v1/groups/{group_id}/tasks/{task_id}/subscriptions` | `TasksRead` | Unsubscribe caller (204, idempotent) |
| `GET` | `/v1/groups/{group_id}/tasks/{task_id}/subscriptions` | `TasksRead` | List subscribers |

Note: subscribing/unsubscribing only requires `TasksRead` — no write permission needed
to watch a task you can see. This matches the design intention that watching is low-privilege.

## Architecture

- **Crate:** `garraia-gateway` — `rest_v1/tasks.rs` (Slice 6 section)
- **Crate:** `garraia-auth` — add `TaskSubscribed` + `TaskUnsubscribed` to `WorkspaceAuditAction`
- **Router:** `rest_v1/mod.rs` — two new `.route()` calls
- **Tests:** `crates/garraia-gateway/tests/rest_v1_task_subscriptions.rs`

## Table schema (migration 006, already migrated)

```sql
CREATE TABLE task_subscriptions (
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscribed_at timestamptz NOT NULL DEFAULT now(),
    muted         boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (task_id, user_id)
);
-- RLS: task_subscriptions_through_tasks JOIN policy (group isolation via tasks.group_id)
```

## Tech stack

- Rust + Axum 0.8 + SQLx 0.8 + Postgres 16 + pgvector (testcontainers)
- `garraia-auth` for `Principal`, `can()`, `audit_workspace_event()`
- Parameterized `set_config()` for RLS context (plan 0056 pattern)

## Design invariants

1. `set_rls_context` called before every query (both `user_id` and `group_id`).
2. PK conflict on `(task_id, user_id)` → 409 Conflict (not 500).
3. DELETE is idempotent — no row found returns 204, not 404.
4. Audit: `task.subscribed` on POST, `task.unsubscribed` on DELETE (no audit for GET).
5. PII-safe metadata: no user email/name in audit payload; `user_id_len: 36` only.
6. App-layer group guard: `path_group_id == principal.group_id` or 403.
7. Cross-group guard: task must exist in the caller's group (enforced by RLS + task existence check).

## Out of scope

- `muted` field management (PUT/PATCH subscription) — deferred to GAR-397 digest worker
- Fan-out notifications — GAR-397
- Batch subscribe — not in schema

## Rollback

Docs + code only; no schema change. Revert by dropping the 3 handler functions, 2 router entries, 1 test file, and the 2 audit variants.

## File structure

```
crates/garraia-auth/src/audit_workspace.rs        ← +2 variants + 2 as_str() + 4 test rows
crates/garraia-gateway/src/rest_v1/tasks.rs       ← +~120 LOC Slice 6 section
crates/garraia-gateway/src/rest_v1/mod.rs         ← +2 route entries
crates/garraia-gateway/tests/rest_v1_task_subscriptions.rs  ← new, 8 scenarios
plans/0079-gar-539-task-subscriptions-api.md      ← this file
plans/README.md                                   ← new row
```

## M1 tasks

- [x] T1: Add `TaskSubscribed` + `TaskUnsubscribed` to `WorkspaceAuditAction` in `garraia-auth`
- [x] T2: Add Slice 6 handlers (`subscribe_task`, `unsubscribe_task`, `list_task_subscriptions`) in `tasks.rs`
- [x] T3: Wire routes in `mod.rs`
- [x] T4: Write integration test `rest_v1_task_subscriptions.rs` (8 scenarios: S1–S8)
- [x] T5: `cargo check -p garraia-gateway -p garraia-auth` green
- [x] T6: `cargo test -p garraia-gateway --test rest_v1_task_subscriptions` green
- [x] T7: `cargo clippy --workspace ...` green
- [x] T8: Update `plans/README.md` + ROADMAP.md checklist

## Acceptance criteria

- POST 201 on fresh subscription; 409 on duplicate; 404 if task not in group.
- DELETE 204 whether or not subscription existed.
- GET 200 returns list ordered by `subscribed_at ASC`.
- Cross-group injection: user from group B cannot subscribe to task in group A (403 path guard + RLS).
- `cargo test -p garraia-gateway --test rest_v1_task_subscriptions` green (8 scenarios).
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green.

## Cross-references

- GAR-539 Linear issue
- Plan 0077 (GAR-533) — task assignees (same pattern)
- Plan 0078 (GAR-536) — task labels (same pattern)
- Migration 006 — `task_subscriptions` table + RLS policy

## Estimativa

0.5 / 1 / 1.5 days
