# benches/database-poc — GAR-373 benchmark harness

> Ephemeral. Delete after ADR 0003 (`docs/adr/0003-database-for-workspace.md`) merges.

This crate is **intentionally isolated** from the main GarraIA workspace.
Its heavy dependencies (`testcontainers`, `sqlx`, `rusqlite`, `fake`, etc.)
must not bleed into the gateway build. It has its own `[workspace]` table
in `Cargo.toml` to prevent Cargo from auto-detecting it as a workspace member.

## Running

Postgres only (requires Docker Desktop running; pulls `pgvector/pgvector:pg16`):

```bash
cargo run --release -- postgres --iterations 3 --out results-postgres.json
```

SQLite only (no Docker required):

```bash
cargo run --release -- sqlite --iterations 3 --out results-sqlite.json
```

Both, merged:

```bash
cargo run --release -- all --iterations 3 --out results-merged.json
```

## Scenarios

| # | Scenario | Postgres | SQLite |
|---|---|---|---|
| B1 | Insert 100k messages batched | yes | yes |
| B2 | FTS query, common term, 1M rows | yes | yes |
| B3 | FTS query, rare term, 1M rows | yes | yes |
| B4 | ANN top-5 over 100k x 768d embeddings | yes | yes (sqlite-vec if available, else plain) |
| B5 | Hybrid FTS + ANN + group_id filter | yes | N/A |
| B6 | Cross-group RLS isolation | yes | N/A |
| B7 | Connection pool stress, 100 concurrent | yes | N/A |

## Output

The harness emits JSON that the aggregator (wave 2) turns into
`results.md` — a human-readable table linked from the ADR.
