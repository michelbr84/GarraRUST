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
`testcontainers`, applies migrations 001, 002, 004, 005, 006, 007, 008, 009, and 010, and
verifies schema shape, RBAC seed counts, single-owner partial unique index,
the audit_events survival paths (regular row + NULL-actor row), the pgvector
HNSW index plus an ANN nearest-neighbor query over memory_embeddings, 8
row-level-security cross-group isolation scenarios from migration 007
(positive read, cross-group block, unset-settings fail-closed, FORCE RLS vs
table owner, chat_members JOIN policy, memory_items user-scope isolation,
memory_embeddings recursive JOIN, audit_events dual policy), and the
migration 006 tasks Tier 1 block (subtask cascade semantics, compound FK
cross-group drift block, enum CHECK violation, RLS positive + cross-group
scenarios across all 8 task tables), and the migration 008 login-role
block (`garraia_login` BYPASSRLS attribute + 4 positive grants per ADR 0005
+ 10-table negative grant matrix + sequence USAGE negative assertion via
`information_schema.usage_privileges` — see GAR-391a), and the migration
009 column-add block (`user_identities.hash_upgraded_at` exists as
`timestamp with time zone`, nullable, no default — prereq for GAR-391b
lazy upgrade path, see plan 0011.5), and the migration 010 signup-role
block (`garraia_signup` BYPASSRLS/NOLOGIN attributes + 3 positive grants
on `users`/`user_identities`/`audit_events` + 5-table negative grant
matrix covering `sessions`/`messages`/`memory_items`/`tasks`/`groups` +
new `garraia_login` SELECT on `sessions` closing Gap A from GAR-391b —
see plan 0012 §3.1). Target wall time: under 30 seconds on a warm cache.

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

### ✅ Resolved: login + signup flow roles (GAR-391a/b/c)

The original GAR-408 callout flagged this as a HARD BLOCKER: `user_identities`
sits under FORCE RLS and holds `password_hash`, so the login flow could not
read it under the normal app pool role. ADR 0005 (GAR-375) chose Option 1
(distinct BYPASSRLS roles) and GAR-391a/b/c implemented it across three
migrations:

- **Migration 008** (GAR-391a) — creates `garraia_login NOLOGIN BYPASSRLS`
  with minimal grants for the verify path: `SELECT, UPDATE` on
  `user_identities`, `SELECT` on `users`, `INSERT, UPDATE` on `sessions`,
  `INSERT` on `audit_events`. Accessed exclusively via the
  `garraia-auth::LoginPool` newtype (private inner `PgPool`, runtime
  `current_user='garraia_login'` validation, `!Clone` enforced via
  `static_assertions`).
- **Migration 009** (GAR-391b prereq) — adds `user_identities.hash_upgraded_at`
  for the lazy upgrade path PBKDF2→Argon2id.
- **Migration 010** (GAR-391c) — creates `garraia_signup NOLOGIN BYPASSRLS`
  for the signup endpoint (Gap B) and adds `GRANT SELECT ON sessions TO
  garraia_login` (Gap A — `INSERT...RETURNING id` and `verify_refresh`
  needed SELECT) plus `GRANT SELECT ON group_members TO garraia_login`
  (Gap C — `Principal` extractor membership lookup). The signup role is
  accessed exclusively via the `garraia-auth::SignupPool` newtype with
  the same boundary contract.

The "0 rows = RLS blocked" anti-pattern flagged above is also addressed: the
`garraia_login` BYPASSRLS attribute means 0 rows now legitimately means
"user not found", and ADR 0005 §"Anti-patterns" #12 forbids the app pool
from ever reading `user_identities.password_hash`. See the ADR's
**Amendment 2026-04-13** for the full grant set updates.

## Scope (GAR-407, GAR-386, GAR-388, GAR-389, GAR-408, GAR-390, GAR-391a/b/c)

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
