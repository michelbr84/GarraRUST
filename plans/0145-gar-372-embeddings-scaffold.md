# Plan 0145 — GAR-372: Embeddings & Vector Search — Scaffold `garraia-embeddings`

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-372](https://linear.app/chatgpt25/issue/GAR-372)
**Branch:** `feat/garraia-embeddings-scaffold`
**Epic parent:** Fase 2.1 — RAG & Embeddings
**ADR reference:** [`0002-vector-store.md`](../docs/adr/0002-vector-store.md) (Accepted 2026-04-21)

---

## Goal

Deliver the **architecture slice** of Fase 2.1:

1. Create crate `crates/garraia-embeddings/` and add it to the workspace.
2. Define the public surface: traits + strong types that the rest of the codebase
   will program against — no concrete database wiring yet.
3. Provide a **deterministic, dependency-free** `EmbeddingProvider` implementation
   for unit tests downstream (so callers can be tested without running an
   embedding model).
4. Document limits and the next concrete PR (real `PgVectorStore` over `sqlx`).
5. `cargo check --workspace --all-targets` and `cargo clippy --workspace
   --all-targets -- -D warnings` stay green.

**Out of scope for this PR** (deferred to future sub-issues):

- `PgVectorStore` real `sqlx` implementation against `memory_embeddings`
  (migration 005). Requires DB integration tests harness — separate slice.
- Real `mxbai-embed-large-v1` provider via `candle` or `mistralrs`. The model
  loader lives in a separate crate (ADR 0001) and will plug into the trait.
- `LanceDbStore` future opt-in (ADR 0002 §Decisões specifies trigger-driven).
- Wiring into `garraia-learning::retriever` or `garraia-agents`. The retriever
  consumes this crate's trait but its construction site lives in their own
  PRs.

---

## Architecture

```text
crates/garraia-embeddings/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs       — public facade + re-exports
    ├── types.rs     — Scope, EmbeddingVector, Document, Chunk, SearchHit
    ├── error.rs     — typed EmbeddingError
    ├── provider.rs  — EmbeddingProvider trait + DeterministicProvider (test/dev)
    ├── store.rs     — VectorStore trait (definition only — no impl in this PR)
    └── hybrid.rs    — HybridQuery builder type
```

`Scope` mirrors the `memory_items.scope_type` column from migration 005
(`user` / `group` / `chat`). `EmbeddingVector` is a fixed-size 768-dim wrapper
(matches the `vector(768)` column from migration 005 and mxbai-embed-large-v1).

---

## Tech stack

- Rust edition 2024 (workspace default).
- `async-trait = "0.1"` — `EmbeddingProvider` and `VectorStore` are object-safe
  via `dyn Trait`; AFIT + `dyn` doesn't work on stable yet (same constraint as
  `garraia-storage::ObjectStore`, documented in plan 0037).
- `serde` + `serde_json` for type derives (transport over the wire when the
  trait is used across process boundaries — admin API + future MCP).
- `sha2 = "0.10"` (already transitive via `ring` ecosystem) — drives the
  deterministic test provider.
- `thiserror = "2"` (workspace) for `EmbeddingError`.
- `tracing = "0.1"` (workspace) for instrumentation hooks.
- **No** `sqlx`, **no** `tokio-postgres`, **no** `candle` — those land in
  follow-up slices when the concrete stores/providers are written.

---

## Design invariants

- `EmbeddingVector` is `[f32; 768]` (mxbai dimension per ADR 0002 §Decisões
  específicas item 2). Different-dim models require a new vector type or a
  generic over `const N: usize` — deferred (YAGNI until a second model lands).
- `VectorStore` operations are always scoped: every call carries `Scope` +
  `group_id`. App-layer code that talks to this trait CANNOT issue a query that
  spans tenants. This mirrors the RLS contract in migration 007.
- `EmbeddingProvider::embed_batch` is **not** optional. Callers ought to batch
  by default — single embeds are O(n) network round-trips that ruin RAG latency
  budgets. Providers may implement `embed_batch` as a fan-out over `embed` but
  the trait signature forces the call site to be batch-aware.
- `DeterministicProvider` is suitable **only for tests and dev fixtures**. It
  is `Cargo.toml`-gated behind a feature flag (`testing-provider`) that
  downstream crates enable in their `[dev-dependencies]`. Production builds
  that depend on this crate without enabling the feature will **not** compile
  in a deterministic embedding — a forcing function.

---

## Test plan (this PR)

Unit tests live in-tree (`#[cfg(test)] mod tests {}`):

- `Scope`: serialization round-trip (`user` / `group` / `chat`); rejection of
  invalid variants.
- `EmbeddingVector`: builder rejects non-768 vectors; equality is bitwise.
- `EmbeddingError`: `Display` does not leak content (PII safety, mirrors the
  redaction invariant on `memory_items.content`).
- `DeterministicProvider`: same input → same output, different inputs → different
  outputs (collision probability is statistically negligible but we don't depend
  on it cryptographically — the provider is for tests).
- `HybridQuery` builder: setting filter / scope / limit produces a fully-typed
  query value; missing required fields yield a typed error before reaching SQL.

No DB integration tests in this PR (no DB code).

---

## Validation gates

Before commit:

- [x] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo check --workspace --all-targets`
- [ ] `cargo test -p garraia-embeddings`
- [ ] `cargo test --workspace` (no regressions in other crates)

Before PR:

- [ ] All of the above + manual diff review
- [ ] ROADMAP §1.5 amended to reflect the scaffold delivery + next PR pointer

---

## Next concrete PR (recommended sequencing)

1. **PR 2 — `PgVectorStore` over sqlx**: real implementation of
   `VectorStore` for Postgres + pgvector against `memory_embeddings`.
   Requires DB integration test harness (testcontainers) — follow the pattern
   from `garraia-workspace` RLS matrix tests.
2. **PR 3 — `MxbaiProvider`** via candle (consumes ADR 0001 backend choice).
3. **PR 4 — Wire `garraia-learning::retriever`** to `EmbeddingProvider` +
   `VectorStore` (real, behind feature flag with `DeterministicProvider`
   fallback in dev).

Each step lands independently with its own test surface.

---

## Risks

- **Trait drift**: if PR 2/3 discovers `VectorStore::search` needs additional
  params (e.g., `ef_search` HNSW tuning), the trait signature changes and
  consumers must update. Mitigation: keep the trait surface minimal in this PR,
  add `SearchOptions` struct as the only extension vector.
- **Feature flag confusion**: `testing-provider` must be off in production
  builds of `garraia-gateway`. The CI matrix already enforces this for
  `garraia-storage`'s `storage-s3` flag — reuse the same pattern.
