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
`testcontainers`, applies migrations 001 and 002, and verifies schema shape,
RBAC seed counts, single-owner partial unique index, and the audit_events
survival paths (regular row + NULL-actor row). Target wall time: under 15
seconds on a warm cache.

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

## Scope (GAR-407, GAR-386)

Bootstrap only: migration 001 (users/groups) + migration 002 (RBAC roles,
permissions, role_permissions, audit_events, single-owner partial unique
index) + connect/migrate helpers + smoke test. CRUD lands in later issues
(GAR-393 API, GAR-391 auth).
