# 3. Database para Group Workspace (`garraia-workspace`)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (review: `@code-reviewer`, `@security-auditor`)
- **Date:** 2026-04-13
- **Tags:** fase-3, multi-tenant, security, ws-schema, gar-373
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-373](https://linear.app/chatgpt25/issue/GAR-373)
  - Plan: [`plans/0002-gar-373-adr-postgres-decision.md`](../../plans/0002-gar-373-adr-postgres-decision.md)
  - Benchmark: [`benches/database-poc/results.md`](../../benches/database-poc/results.md)
  - Research base: [`deep-research-report.md`](../../deep-research-report.md) (produced via web research early in Fase 3 planning; contains literature-grounded database comparisons. This ADR supersedes any latency estimates in that document with the empirical benchmark in `benches/database-poc/`. The research report remains authoritative for qualitative arguments about LGPD/GDPR architecture, schema patterns, and operational guidance it cites.)

---

## Context and Problem Statement

GarraIA today is single-user: the crate `garraia-db` wraps `rusqlite` and backs one local installation per binary. `SessionStore` holds a single `tokio::sync::Mutex<Connection>`.

The [AAA Roadmap Fase 3](../../ROADMAP.md#fase-3--group-workspace-famíliaequipe-multi-tenant--novo-12-20-semanas) turns GarraIA into a multi-tenant **Group Workspace** with:

- Groups with members, roles, invitations, audit trail
- Shared files scoped to `group_id` with versioning and soft delete
- Shared chats with threads, attachments, FTS
- AI memory with 3 scope levels (`User` / `Group` / `Chat`)
- Tasks & docs colaborativos (Notion-like, Fase 3.8)
- LGPD/GDPR compliance: data subject rights, retention policies, audit

We must commit to a database backend **before** any `garraia-workspace` migration or schema work begins. The choice shapes:

- **Schema design**: native RLS vs application-level isolation
- **Full-text search**: native vs external index vs sidecar
- **Vector search**: for AI memory retrieval at Fase 2.1 / 3.7
- **Operational properties**: backup/PITR/replication/HA
- **Rust ecosystem fit**: `sqlx` async + compile-time checks vs `rusqlite` sync
- **Migration path** for existing SQLite users of `garraia-db`
- **Self-host friendliness**: family deployments on modest hardware

A wrong choice here means either (a) rewriting most of Fase 3 mid-flight, or (b) carrying application-level hacks for multi-tenant isolation that violate `CLAUDE.md` regra 8 (ADR-first) and regra 10 (cross-group authorization tests).

This ADR chooses that backend and records the rationale, benchmark evidence, and migration plan.

---

## Decision Drivers

Ranked by weight for the Group Workspace use case:

1. **★★★★★ Multi-tenant isolation** — hard requirement. LGPD art. 46 and the §1.1 baseline of the ROADMAP demand demonstrable segregation by `group_id`. Defense-in-depth via database-level RLS is strongly preferred over app-only isolation.
2. **★★★★★ Vector ANN for AI memory** — hard requirement. Sub-20 ms p95 top-5 retrieval over ≥100k embeddings is a product-defining latency for the chat experience (§2.1 RAG, §3.7 shared memory).
3. **★★★★ Concurrent writers** — hard requirement. Family/team workloads expect tens to hundreds of concurrent users writing messages, uploading files, updating tasks. Single-writer serialization is a non-starter.
4. **★★★★ Operational maturity** — PITR, streaming replicas, hot standby, proven HA story, mature backup/restore tooling. Fase 6 SLOs (chat p95 < 500 ms, upload > 99 %) presume a database that doesn't surprise the SRE.
5. **★★★ Full-text search** — native (no external index sidecar) preferred for the MVP. Must scale to ~10M messages per instance without degradation (§3.6).
6. **★★★ Rust ecosystem fit** — `sqlx` offers compile-time query checking against a live schema, which is a correctness multiplier for the schema-heavy `garraia-workspace` crate. `rusqlite` is mature but synchronous and has no compile-time check.
7. **★★★ Migration path** — existing users of `garraia-db` with SQLite state (sessions, mobile_users, memory) must have a clear upgrade path. Not a showstopper if disruptive, but it shapes deploy ergonomics.
8. **★★ Self-host friendliness** — the North Star is local-first. The backend must run comfortably on a Raspberry Pi 4+ / modest VPS / desktop Docker.
9. **★★ Raw latency at small scale** — nice to have, but easily dominated by architectural drivers above.

---

## Considered Options

### A) PostgreSQL 16 + pgvector + pg_trgm *(recommended)*

**How it works:** stock Postgres 16 with the `vector` (pgvector ≥0.7) and `pg_trgm` extensions enabled. `tsvector + GIN` handles full-text search. `vector(768)` columns with HNSW cosine index handle AI memory. Row-level security via `CREATE POLICY ... USING (group_id = current_setting('app.current_group_id')::uuid)`. `sqlx 0.8` provides async queries with compile-time schema checking via `sqlx::query!`.

**Pros:**
- Native RLS closes cross-tenant leakage at the database layer (defense-in-depth).
- pgvector with HNSW clocks 5.53 ms p95 ANN top-5 at 100k × 768d (B4 benchmark) — 124x faster than naive SQLite.
- `tsvector` + GIN FTS scales linearly to tens of millions of rows.
- Hybrid queries (FTS + ANN + filter) compose naturally in one CTE (B5: 8.43 ms p95).
- Mature connection pooling (`sqlx::PgPoolOptions`), WAL archiving PITR, streaming replicas, logical replication, pg_dump/pg_restore, all well-documented.
- `sqlx::query!` catches SQL errors at `cargo check` time, not in production.
- Huge managed-service ecosystem (Supabase, Neon, RDS, Cloud SQL) for users who don't self-host.
- `testcontainers-modules/postgres` makes integration testing trivial.

**Cons:**
- Single-file simplicity is lost. Family on Raspberry Pi needs to run a Postgres container or install the server package.
- Raw insert latency and FTS latency on small datasets are ~10-40x slower than SQLite (B1/B2/B3 benchmark) due to network/IPC overhead — irrelevant at production scale but worth noting.
- Operating Postgres correctly (autovacuum tuning, backup rotation, upgrade paths) is genuinely more work than SQLite. Mitigated by `docker-compose.yml` defaults and clear docs.

**Benchmark (median of 3 iterations):**

| Scenario | Metric | Result |
|---|---|---|
| B1 insert 100k | throughput | 9,449 msg/s |
| B2 FTS common term | p95 | 1.49 ms |
| B3 FTS rare term | p95 | 1.15 ms |
| B4 ANN top-5 HNSW | p95 | **5.53 ms** |
| B5 hybrid FTS+ANN+group | p95 | 8.43 ms |
| B6 RLS cross-group | pass/fail | **PASS (0 rows leaked)** |
| B7 100 concurrent tasks | p95 wall | 2,123 ms |

### B) SQLite 3 + FTS5 (no sqlite-vec)

**How it works:** stay with `rusqlite 0.32` (already in the workspace via `garraia-db`), add FTS5 virtual tables for messages, store embeddings as BLOB and compute cosine similarity in Rust on retrieval.

**Pros:**
- Zero operational overhead: single `.db` file, trivial backup (file copy).
- Best raw latency for small datasets on local disk (B1: 85k msg/s, B2: 0.035 ms p95).
- Already in the workspace — no new Rust dep, no new container to manage.
- Self-host on literally anything that runs Rust.

**Cons — decisive:**
- **No row-level security.** All isolation must live in application code. Any auth bug becomes a tenant data leak. This alone disqualifies it for the multi-tenant workspace per LGPD art. 46.
- **WAL mode serializes writers.** 100 concurrent messages become a queue, not parallelism. Family chat becomes laggy under load.
- **No native ANN.** B4 benchmark showed plain cosine scan at 685 ms p95 — 124x slower than pgvector. Unusable for interactive memory retrieval.
- **FTS5 degrades past ~10M rows** without custom sharding, whereas `tsvector + GIN` is engineered for billions.
- **No hybrid query support** — FTS and ANN results must be merged in application code (B5 = N/A).
- `rusqlite` is synchronous; async integration requires `tokio::task::spawn_blocking` for every query, losing the compile-time correctness `sqlx::query!` offers.
- Operational story for backup/replication/PITR is manual and clumsy compared to Postgres.

**Benchmark:**

| Scenario | Metric | Result |
|---|---|---|
| B1 insert 100k | throughput | ~85,000 msg/s |
| B2 FTS `ut` | p95 | 0.035 ms |
| B3 FTS rare token | p95 | 0.070 ms |
| B4 ANN plain scan | p95 | **685.6 ms** |
| B5 hybrid | — | **N/A** |
| B6 RLS | — | **N/A** (no equivalent) |
| B7 pool stress | — | **N/A** (WAL single-writer) |

### C) SQLite 3 + sqlite-vec + FTS5

**How it works:** same as B, plus load `sqlite-vec` as an extension to get native vector similarity search.

**Pros:**
- Closes the B4 gap on ANN (sqlite-vec benchmarks show competitive performance at 100k).
- Keeps the operational simplicity of SQLite.

**Cons:**
- `sqlite-vec` is a native loadable extension. Deployment requires per-host binary distribution (Linux / macOS / Windows, x86_64 / arm64), loading via `db.load_extension("vec0", None)` which rusqlite supports but makes `cargo run` setup fragile.
- All the structural cons of option B remain: no RLS, WAL writer serialization, no hybrid queries, no native replication.
- Adds a native dep to a currently pure-Rust codebase (minus `rusqlite bundled`).
- Community is smaller; debugging production issues has less prior art.

**Verdict:** solves one sub-problem while leaving the four bigger ones (RLS, writers, hybrid, operations). Not worth the complexity tradeoff.

### D) CockroachDB

**How it works:** distributed SQL, Postgres wire-compatible, serializable isolation. Horizontal scaling out of the box.

**Pros:**
- Multi-region active/active if the roadmap ever needs it (Fase 7).
- Postgres-flavored SQL means `sqlx` works.
- Serializable by default eliminates a class of race bugs.

**Cons:**
- No `pgvector` — we'd need an external vector store sidecar, adding operational surface.
- `tsvector/GIN` support is partial / evolving; FTS story is weaker than Postgres native.
- Running CockroachDB as a self-hosted single-node for a family deployment is substantially heavier than Postgres (memory, CPU, disk).
- Community ecosystem is smaller; fewer hosted providers; smaller labor pool.
- Premature optimization: Fase 7 "multi-region active/active" is years away and can be reached via Postgres logical replication or cross-region managed services when needed.

### E) MySQL 8 / MariaDB

**How it works:** stock MySQL 8 or MariaDB with InnoDB. Full-text index via `FULLTEXT` + custom vector extensions (MyRocks, MyVector, or external).

**Pros:**
- Huge operational community, well understood by generalist SREs.
- Mature replication and HA stories.

**Cons:**
- No native equivalent to Postgres RLS. MySQL has "views with DEFINER" hacks but it's application-layer in practice.
- FTS story in MySQL is weaker than Postgres (no GIN, no tsvector, no ranking as flexible).
- No pgvector equivalent in the core. Vector search requires external sidecar or preview-grade plugins.
- `sqlx` supports MySQL but compile-time query checking for MySQL is less battle-tested than for Postgres.
- Historical preference among Rust web developers leans Postgres; onboarding docs and community examples are Postgres-first.

**Verdict:** mature, but doesn't buy us anything Postgres doesn't already offer, and loses RLS and pgvector. Rejected.

---

## Decision Outcome

**Chosen option: A) PostgreSQL 16 + pgvector + pg_trgm.**

### Rationale

Two drivers dominate the decision:

1. **Multi-tenant isolation via RLS is non-negotiable.** The LGPD compliance plan (§5.3) and the ROADMAP §3.3 require demonstrable defense-in-depth that a user of group A cannot read group B data even via direct ID access. Postgres `CREATE POLICY` delivers this at the database layer. Benchmark B6 empirically proved that `SET LOCAL app.current_group_id` + the policy returns zero rows when a group-A-scoped transaction asks for a known group-B message ID. SQLite, MySQL, and MariaDB have no equivalent — isolation becomes entirely an application concern, and any bug in `garraia-auth` becomes a tenant leak.

2. **AI memory ANN latency is product-defining.** B4 showed pgvector HNSW at 5.53 ms p95 vs plain SQLite scan at 685 ms p95 — a 124x gap. Even with `sqlite-vec`, the gap narrows but the other structural problems (no RLS, no hybrid queries, WAL single writer) remain. Postgres is the only option that solves the ANN problem *and* everything else in one coherent engine.

Supporting factors:

- **`sqlx 0.8` + Postgres** provides compile-time SQL checking, which for a schema-heavy crate like `garraia-workspace` is a correctness multiplier. `sqlx::query!("SELECT ... FROM groups WHERE id = $1", id)` fails `cargo check` if the column is renamed — this is exactly the failure mode Fase 3 migrations 001-007 will stress.

- **`testcontainers-modules/postgres` with the pgvector image** makes integration testing a one-liner (`Postgres::default().with_name("pgvector/pgvector").with_tag("pg16").start().await`). Wave 1a of this ADR benchmark proved the workflow end-to-end in CI-like conditions.

- **Operational maturity** is decades deep: WAL archiving for PITR, streaming replicas for HA, `pg_dump`/`pg_restore` for cold backups, logical replication for zero-downtime upgrades. Fase 6 SLOs depend on having this toolkit on the shelf.

- **Managed-service exit ramp** — if self-hosted Postgres becomes too much work for a user, they can migrate to Supabase / Neon / RDS / Cloud SQL without schema changes. Options B, C, and D don't have comparably mainstream managed offerings.

SQLite's wins on B1/B2/B3 (raw insert and FTS latency at small scale) are real but **do not matter for the Group Workspace**. They matter for `garraia-db` (single-user dev / CLI), which continues to use SQLite — this ADR does NOT deprecate it.

### Decision details

- **Engine:** PostgreSQL **16.x** (pinned to 16; 14 and 15 are out of scope for initial Fase 3).
- **Extensions required:** `vector` (pgvector 0.7+), `pg_trgm`, `uuid-ossp` (or `pgcrypto` for UUID generation).
- **Rust driver:** `sqlx 0.8` with features `["runtime-tokio", "postgres", "uuid", "chrono", "migrate"]`.
- **Connection pooling:** `sqlx::postgres::PgPoolOptions` with per-deployment `max_connections` tuning; start at 20 for small instances.
- **Migrations:** `sqlx::migrate!()` macro loading SQL files from `crates/garraia-workspace/migrations/`, forward-only per `CLAUDE.md` regra 9.
- **Schema isolation pattern:** every tenant-scoped table (`messages`, `files`, `memory_items`, `tasks`, `docs`, `audit_events`) enables RLS **and** is marked `FORCE ROW LEVEL SECURITY`, and defines a policy using `current_setting('app.current_group_id', true)::uuid`. Connections set this per-request via `SET LOCAL app.current_group_id = $1` inside a transaction started by the Axum extractor. Three operational pitfalls that migration 007 ([GAR-408](https://linear.app/chatgpt25/issue/GAR-408)) and the Axum extractor ([GAR-391](https://linear.app/chatgpt25/issue/GAR-391)) must handle explicitly:
  1. **Owner bypass.** Without `FORCE ROW LEVEL SECURITY`, the role that owns the table (typically the role running migrations, and commonly the same role the application pool uses) bypasses the policy entirely. Migration 007 must emit `ALTER TABLE ... FORCE ROW LEVEL SECURITY` for every tenant-scoped table, and the benchmark harness in `benches/database-poc/src/postgres_scenarios.rs` has been updated to include `FORCE` in its schema DDL.
  2. **NULL setting fail-closed.** `current_setting('app.current_group_id', true)` returns `NULL` when the variable is unset. `group_id = NULL` evaluates to `NULL` (not FALSE), which the policy treats as not-visible, so every row is filtered out. This is a safe default — it fails closed, not open — but it also silently masks application bugs where the extractor forgot to `SET LOCAL` at the top of a transaction. The Axum extractor must treat "no `group_id` in the Principal" as a 500, not quietly issue queries that return empty sets.
  3. **`SET LOCAL` scope.** `SET LOCAL` persists only until `COMMIT`/`ROLLBACK`. Every transaction that touches tenant-scoped tables must re-issue the `SET LOCAL` at its start. The connection pool must NOT reuse a cached setting across transactions.
  4. **RLS coverage across all tenant-scoped tables.** Migration 007 must `ENABLE` + `FORCE ROW LEVEL SECURITY` on **all** scoped tables (`messages`, `files`, `file_versions`, `memory_items`, `memory_embeddings`, `tasks`, `task_comments`, `docs`, `audit_events`), not only `messages`. The benchmark in `benches/database-poc/` only exercises RLS on `messages` as a representative sample; the B5 hybrid query filters `memory_embeddings` at the application layer via `WHERE group_id = $1` rather than via a policy, and that is a benchmark simplification — production must use RLS on `memory_embeddings` too. Tracked for migration 007 and follow-up GAR-389.
- **`garraia-db` (SQLite) is retained** for single-user CLI and dev workflows, and it continues to own the `CredentialVault` master key and other local-only secrets. It is NOT deprecated. A feature flag `backend-sqlite` on `garraia-workspace` is **explicitly rejected** (per plan open question #1): the Group Workspace is Postgres-only, no dual backend. This eliminates drift between dev and prod test paths.

### Consequences

#### Positive

- Defense-in-depth multi-tenant isolation at the database layer. Authz bugs in `garraia-auth` don't automatically become data leaks.
- Sub-10 ms AI memory retrieval at 100k embeddings, with clear headroom for 1M+.
- Single engine handles messages, FTS, vectors, audit, and hybrid queries — no sidecar.
- Compile-time SQL checking via `sqlx::query!` catches migration drift before runtime.
- Clear migration path to managed Postgres for users who outgrow self-hosting.
- Solid observability story: `pg_stat_statements`, `auto_explain`, and OpenTelemetry instrumentation (already in the workspace via GAR-384) all integrate cleanly.
- `docker-compose.yml` gets a `postgres` service (actually coming in Fase 6.1 via GAR-406); existing `ops/compose.otel.yml` infrastructure already handles service orchestration.

#### Negative

- Self-host setup is heavier. Raspberry Pi 4+ runs Postgres 16 without drama but it's not as effortless as SQLite single file.
- Users on the current SQLite-based `garraia-db` need a migration tool (see §"Migration Strategy" below).
- Operational responsibilities appear: backup rotation, upgrade planning, autovacuum monitoring. Mitigated by sensible defaults in `docker-compose.yml` and a future runbook (Fase 6.2 / GAR-OBS-GA).
- Raw latency on small datasets is ~10-40x slower than SQLite (B1/B2/B3). **This is acceptable**: the target workload is not "sub-ms single-row operations on tempfs" but "concurrent multi-user multi-tenant with ANN." Postgres pays a fixed overhead per query that is invisible under real production ratios.
- Dev onboarding gains a prerequisite: Docker Desktop or local Postgres install. Documented in `docker-compose.yml` and dev docs.

#### Neutral / mitigated

- **SQLite continues to exist.** `garraia-db` is untouched by this ADR. Only `garraia-workspace` (new crate per GAR-407) is Postgres.
- **pgvector HNSW is under active development.** Version 0.7+ is stable for our use; we pin the Docker image tag to prevent silent upgrades.
- **`sqlx` macro compilation requires a live database during `cargo check`** for `sqlx::query!` — solved via `SQLX_OFFLINE=true` + `sqlx prepare` committing `.sqlx/` metadata to git. This is a standard sqlx workflow and documented.

---

## Migration Strategy: SQLite (`garraia-db`) → Postgres (`garraia-workspace`)

**Context:** `garraia-db` holds existing user data across tables `sessions`, `messages`, `memory_facts`, `chat_sync`, `mobile_users`, etc. The Group Workspace introduces a **new** data model (`groups`, `group_members`, scoped `messages`, `files`, `tasks`, etc.) that does not 1:1 map to the old SQLite schema.

**Strategy: one-shot import, not dual-write.**

Dual-write was considered and rejected: it would require keeping both `garraia-db` and `garraia-workspace` in sync through every write path, effectively doubling the test surface and creating subtle consistency bugs. One-shot import is simpler, reversible (keep the SQLite file around), and matches the "forward-only migrations" principle of `CLAUDE.md` regra 9.

**Tool:** `garraia-cli migrate workspace --from-sqlite ./garraia.db --to-postgres $DATABASE_URL` (new subcommand to build as part of GAR-407 execution, not this ADR).

**Flow for an existing user upgrading to Fase 3:**

1. **Backup** — the command refuses to run without `--confirm-backup` or an existing backup of the SQLite file.
2. **Provision Postgres** — `docker-compose up postgres` with pgvector image, or point at a managed instance via `DATABASE_URL`.
3. **Run migrations** — `garraia-workspace` embeds `sqlx::migrate!` and applies schema 001-007 on first connect.
4. **Create a "personal" default group** — the single existing user becomes the owner of a synthetic `group` called `personal` (configurable name).
5. **Copy tables** — per-table, with type conversion and scope assignment:
   - `mobile_users` → `users` + `user_identities` (identity provider `internal`)
   - `sessions` → `sessions` (same shape, new IDs via uuid_v7)
   - `messages` → `messages` scoped to the `personal` group
   - `memory_facts` → `memory_items` with `scope_type = 'user'` for the single owner
   - `chat_sync` → `chats` + per-user membership
6. **Verify** — integrity checks (row counts, sample equality), then emit a report.
7. **Cutover** — the gateway now reads/writes Postgres for workspace features. `garraia-db` remains for CLI/single-user utilities that don't need workspace semantics.
8. **Rollback** — trivial: stop the gateway, restore the SQLite file from backup, revert to the pre-Fase-3 gateway binary. No destructive change to the SQLite file during migration.

**Rollout phases** (tracked as subtasks of GAR-407):

- **Phase 3a:** `garraia-workspace` + migrations + tests. No user-facing UI yet. Gateway optionally connects when `GARRAIA_WORKSPACE_DATABASE_URL` is set. Backwards compatible.
- **Phase 3b:** `garraia-cli migrate workspace` command. Opt-in import.
- **Phase 3c:** workspace features (files, chats, tasks) surface in the admin UI behind feature flag `workspace`.
- **Phase 3d (GA):** feature flag removed; `garraia-db` continues to exist for dev/CLI but production installs run both.

**Data residency:** single-user installs that opt not to run Postgres keep using `garraia-db`. This ADR does **not** force everyone onto Postgres — it specifies that `garraia-workspace` requires it, while `garraia-db` remains the single-user path.

---

## Validation

### Benchmark evidence

Full results: [`benches/database-poc/results.md`](../../benches/database-poc/results.md). Summary table reproduced here:

| # | Scenario | Postgres p95 | SQLite p95 | Winner |
|---|---|---:|---:|---|
| B1 | Insert 100k (throughput) | 9.4k msg/s | 85k msg/s | SQLite |
| B2 | FTS common term | 1.49 ms | 0.035 ms | SQLite |
| B3 | FTS rare term | 1.15 ms | 0.070 ms | SQLite |
| B4 | **ANN top-5 HNSW** | **5.53 ms** | **685 ms (single-scan wall)** | **Postgres (124x)** |
| B5 | Hybrid FTS+ANN+group | 8.43 ms | N/A | Postgres |
| B6 | **RLS cross-group isolation** | **PASS** | N/A | **Postgres (decisive)** |
| B7 | 100 concurrent tasks | 2.1 s wall | N/A | Postgres |

Run on Windows x86_64 / 20 CPUs / Docker Desktop / `pgvector/pgvector:pg16`. Seed 42 deterministic. Methodology: 3 iterations per latency scenario with median of per-iteration p95; **B6 and B7 run once** (pass/fail and single stress wall-time respectively); **B4 SQLite runs 1 full scan per iteration** (3 samples total) because the plain cosine scan is a whole-dataset operation — p50/p95/p99 collapse to a single wall-time and should be read as "one scan takes ~685 ms," not as a latency distribution. The B6 validity also depends on using `SET LOCAL ROLE app_user` inside the transaction — without the role switch, the superuser connection would bypass RLS for the table it owns and the test would trivially pass with no policy in place. Full methodology, caveats, and known asymmetries in `results.md`.

### Architectural capability matrix

| Capability | Postgres 16 + pgvector | SQLite + FTS5 |
|---|:-:|:-:|
| Row-Level Security | ✅ native `CREATE POLICY` | ❌ app-layer only |
| Concurrent multi-writer | ✅ MVCC | ❌ WAL serializes writers |
| Vector ANN native | ✅ HNSW | ⚠️ external extension |
| Hybrid FTS+ANN in one SQL | ✅ CTE | ❌ app-layer merge |
| Async Rust driver with compile-time checks | ✅ `sqlx::query!` | ⚠️ `rusqlite` sync |
| PITR + streaming replicas | ✅ native | ❌ manual |
| Managed service ecosystem | ✅ huge | ⚠️ limited |

### Unblocked issues

This ADR's merge unblocks:

- [GAR-407](https://linear.app/chatgpt25/issue/GAR-407) — `garraia-workspace` crate bootstrap + migration 001
- [GAR-386](https://linear.app/chatgpt25/issue/GAR-386) — migration 002 RBAC
- [GAR-387](https://linear.app/chatgpt25/issue/GAR-387) — migration 003 files
- [GAR-388](https://linear.app/chatgpt25/issue/GAR-388) — migration 004 chats + FTS
- [GAR-389](https://linear.app/chatgpt25/issue/GAR-389) — migration 005 memory + pgvector
- [GAR-390](https://linear.app/chatgpt25/issue/GAR-390) — migration 006 tasks
- [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) — migration 007 RLS (must emit `FORCE ROW LEVEL SECURITY` per §"Schema isolation pattern")
- [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — `garraia-auth` crate with Principal/Scope/RBAC (must guard against missing `app.current_group_id` with a 500, not silent empty reads)
- [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) — `/v1/groups` API routes

---

## Links

- Issue: [GAR-373](https://linear.app/chatgpt25/issue/GAR-373)
- Plan: [`plans/0002-gar-373-adr-postgres-decision.md`](../../plans/0002-gar-373-adr-postgres-decision.md)
- Benchmark harness: [`benches/database-poc/`](../../benches/database-poc/)
- Benchmark results: [`benches/database-poc/results.md`](../../benches/database-poc/results.md)
- Raw JSON: [`results-postgres.json`](../../benches/database-poc/results-postgres.json), [`results-sqlite.json`](../../benches/database-poc/results-sqlite.json)
- Research base: [`deep-research-report.md`](../../deep-research-report.md) §"Comparativo de bancos de dados"
- [PostgreSQL 16 documentation](https://www.postgresql.org/docs/16/)
- [pgvector repository](https://github.com/pgvector/pgvector)
- [sqlx documentation](https://docs.rs/sqlx/0.8/)
- [testcontainers-modules postgres](https://docs.rs/testcontainers-modules/0.11/testcontainers_modules/postgres/)
- [LGPD Lei 13.709/2018 art. 46](http://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm) — medidas de segurança
- [GDPR art. 32](https://gdpr-info.eu/art-32-gdpr/) — security of processing
