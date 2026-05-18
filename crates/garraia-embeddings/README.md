# garraia-embeddings

Embeddings & vector-search surface for GarraIA.

This crate ships the **trait surface** + **strong types** that the rest of the
workspace programs against. Concrete database wiring (`PgVectorStore` over
`sqlx` against migration 005) and real embedding model loaders
(`MxbaiProvider` via `candle`) land in follow-up slices — see
[plan 0145](../../plans/0145-gar-372-embeddings-scaffold.md) and
[ADR 0002](../../docs/adr/0002-vector-store.md) for the full plan.

## What this crate ships today

| Module | Status | Purpose |
|---|---|---|
| `types` | ✅ stable | `Scope`, `EmbeddingVector`, `Document`, `Chunk`, `SearchHit` |
| `error` | ✅ stable | typed `EmbeddingError` with PII-safe `Display` |
| `provider` | ✅ trait + 1 test impl | `EmbeddingProvider` async trait; `DeterministicProvider` for unit tests |
| `store` | ✅ trait only | `VectorStore` async trait, `SearchOptions` |
| `hybrid` | ✅ stable | `HybridQuery` typed builder for FTS+ANN+filter retrieval |

## What this crate **does not** ship (yet)

- No `PgVectorStore` — the real Postgres + pgvector implementation is a
  separate slice that requires the testcontainers harness already used by
  `garraia-workspace`.
- No `MxbaiProvider` / `OllamaProvider` / `CandleProvider` — real model loaders
  consume ADR 0001 backends and land per-provider.
- No wiring into `garraia-learning::retriever` or `garraia-agents` — the trait
  is published; consumers connect it in their own PRs.

## Quick start (downstream crate)

```rust,ignore
use std::sync::Arc;

use garraia_embeddings::{EmbeddingProvider, EmbeddingVector, HybridQuery, Scope};

async fn build_query<P>(provider: Arc<P>, text: &str) -> Result<HybridQuery, garraia_embeddings::EmbeddingError>
where
    P: EmbeddingProvider + 'static,
{
    let vector: EmbeddingVector = provider.embed(text).await?;
    HybridQuery::builder()
        .scope(Scope::User)
        .model_id(provider.model_id())
        .vector(vector)
        .text("monthly invoice")
        .limit(5)
        .build()
}
```

### Unit tests without a real model

`DeterministicProvider` (default-on via the `testing-provider` feature) is
suitable for unit tests in downstream crates:

```rust,ignore
use garraia_embeddings::{DeterministicProvider, EmbeddingProvider};

#[tokio::test]
async fn my_handler_calls_the_provider() {
    let provider = DeterministicProvider::new();
    let v = provider.embed("hello").await.unwrap();
    assert_eq!(v.as_slice().len(), 768);
    // The vector has no semantic meaning, but it IS deterministic and
    // dimensioned correctly — enough to test that your handler hands the
    // vector to the right caller.
}
```

## Invariants worth knowing

1. **Dimension is fixed at 768** (`EMBEDDING_DIM`). Matches `vector(768)` in
   migration 005 and `mxbai-embed-large-v1`. A different-dim model is a
   schema migration, not a runtime switch.
2. **Every query is tenant-scoped.** `VectorStore` methods take `Scope` +
   `Option<Uuid> group_id`. The trait shape makes "search everything"
   queries unrepresentable.
3. **`Scope::User` ↔ `group_id = None`** is enforced at `HybridQuery::build`
   time. Personal memories are not group-scoped (migration 005 has
   `memory_items.group_id` nullable specifically for this).
4. **`include_secret` defaults to `false`.** Mirrors the
   `sensitivity = 'secret'` invariant on `memory_items`. Opt-in is explicit
   per call.
5. **Errors never echo input.** `EmbeddingError::Display` uses static strings
   only — same PII safety contract as `garraia-telemetry::redact`.

## Feature flags

| Feature | Default | What it gates |
|---|---|---|
| `testing-provider` | ✅ on | `DeterministicProvider` + `sha2` dep |

Production builds may opt out (`default-features = false`) to drop the
test-only provider; the trait surface remains intact.

## Next concrete PR

`PgVectorStore` over `sqlx` against `memory_embeddings` (migration 005, HNSW
cosine). See [plan 0145](../../plans/0145-gar-372-embeddings-scaffold.md)
§"Next concrete PR" for the sequencing.
