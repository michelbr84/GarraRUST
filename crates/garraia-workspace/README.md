# garraia-workspace

Postgres-backed multi-tenant workspace for GarraIA. Owns users, identities,
sessions, API keys, groups, group members, and group invites. The tenant root
is `groups`; downstream crates filter everything by `group_id` with RLS in
later migrations.

- Decision record: [`docs/adr/0003-database-for-workspace.md`](../../docs/adr/0003-database-for-workspace.md)
- Plan: [`plans/0003-gar-407-workspace-schema-bootstrap.md`](../../plans/0003-gar-407-workspace-schema-bootstrap.md)
- Linear: GAR-407

## Running the tests locally

Requires Docker Desktop running. The pgvector image is pulled on first run and
cached thereafter.

```bash
cargo test -p garraia-workspace
```

The integration test spins up a `pgvector/pgvector:pg16` container via
`testcontainers`, applies migrations 001, 002, 004, 005, 006, and 007, and
verifies schema shape, RBAC seed counts, single-owner partial unique index,
the audit_events survival paths (regular row + NULL-actor row), the pgvector
HNSW index plus an ANN nearest-neighbor query over memory_embeddings, 8
row-level-security cross-group isolation scenarios from migration 007
(positive read, cross-group block, unset-settings fail-closed, FORCE RLS vs
table owner, chat_members JOIN policy, memory_items user-scope isolation,
memory_embeddings recursive JOIN, audit_events dual policy), and the
migration 006 tasks Tier 1 block (subtask cascade semantics, compound FK
cross-group drift block, enum CHECK violation, RLS positive + cross-group
scenarios across all 8 task tables). Target wall time: under 30 seconds on
a warm cache.

## Required Postgres role privileges

Migration 001 calls `CREATE EXTENSION IF NOT EXISTS pgcrypto` and
`CREATE EXTENSION IF NOT EXISTS citext`. Both require either the `SUPERUSER`
attribute or the `CREATE` privilege on the database.

- **Dev / self-host**: connect `Workspace::connect` with `migrate_on_start = true`
  as a superuser role (e.g. the default `postgres` user). First-run applies the
  extensions once; subsequent runs no-op.
- **Hardened production**: run migrations **once** as a privileged migration role,
  then connect the application pool as a least-privilege role with only `USAGE`
  on `public` + `SELECT/INSERT/UPDATE/DELETE` on the tables. Set
  `migrate_on_start = false` for the app pool. A dedicated migration issue
  (follow-up after GAR-413) will document the exact `GRANT` statements.

Migration 006 creates the 8 tasks Tier 1 tables (`task_lists`, `tasks`,
`task_assignees`, `task_labels`, `task_label_assignments`, `task_comments`,
`task_subscriptions`, `task_activity`) with RLS FORCE embedded in the same
migration (no retrofit). Because 006 runs BEFORE 007 in lexicographic order,
it creates the `garraia_app` role idempotently and issues explicit GRANTs —
`ALTER DEFAULT PRIVILEGES` from 007 does not retroactively cover tables
created by earlier migrations.

Migration 007 enables `FORCE ROW LEVEL SECURITY` on 10 tenant-scoped tables
(`messages`, `chats`, `chat_members`, `message_threads`, `memory_items`,
`memory_embeddings`, `audit_events`, `sessions`, `api_keys`, `user_identities`)
using `current_setting('app.current_group_id', true)` and
`current_setting('app.current_user_id', true)` as request context. Both
values are wrapped in `NULLIF(..., '')` because custom GUCs return an empty
string (not NULL) when unset — without `NULLIF`, a `'' ::uuid` cast raises
SQLSTATE 22P02 and aborts the transaction instead of failing closed silently.
Every request touching these tables must
`SET LOCAL app.current_group_id = '<uuid>'` and
`SET LOCAL app.current_user_id = '<uuid>'` at transaction start —
`garraia-auth` (GAR-391) is the canonical caller.

### ⚠️ HARD BLOCKER for GAR-391 production rollout: login flow role

Because `user_identities` is itself under RLS and holds `password_hash`, the
**login flow cannot read it under the normal app pool role**. At login time
`app.current_user_id` is not yet known (that's what we're trying to
determine), so the `user_identities_owner_only` policy filters every row
and returns an empty result set. **An empty result here MUST NOT be treated
as "user not found" by login code** — it means "RLS blocked; unauthenticated"
which is semantically different from the definitive "no such user" answer.

Production deployments therefore REQUIRE one of:

1. A distinct Postgres role with `BYPASSRLS` attribute used exclusively by
   the login path, OR
2. A `SECURITY DEFINER` function that verifies credentials and returns a
   user id without exposing the underlying `user_identities` row.

Designing and granting this role is a **hard blocker** for GAR-391 going
to production. ADR 0005 (GAR-375 Identity Provider) must explicitly cover
this decision before any login endpoint ships. This is not a follow-up —
it is a pre-merge blocker for GAR-391.

## Scope (GAR-407, GAR-386, GAR-388, GAR-389, GAR-408, GAR-390)

Bootstrap only: migration 001 (users/groups) + migration 002 (RBAC roles,
permissions, role_permissions, audit_events, single-owner partial unique
index) + migration 004 (chats, chat_members, messages with Portuguese FTS,
message_threads) + migration 005 (memory_items with tri-level scope +
memory_embeddings with pgvector HNSW cosine index) + migration 006 (tasks
Tier 1 Notion-like: task_lists/tasks with compound FK + subtasks via self-FK,
task_assignees, task_labels + task_label_assignments, task_comments,
task_subscriptions, task_activity — all 8 with RLS FORCE embedded) +
migration 007 (FORCE RLS on 10 tenant-scoped tables with 3 policy classes —
direct, JOIN, dual — all fail-closed when `SET LOCAL app.current_*_id` is
unset) + connect/migrate helpers + smoke test. CRUD lands in later issues (GAR-393 API, GAR-391 auth).
