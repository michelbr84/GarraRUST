//! GarraIA — Embeddings & vector search.
//!
//! This crate ships the public surface (traits + strong types) that the rest of
//! the GarraIA workspace programs against for embedding-based retrieval. It
//! intentionally contains **no** concrete database wiring — the
//! [`VectorStore`] trait is a definition; a real `PgVectorStore` over `sqlx`
//! lives in a follow-up slice.
//!
//! Design is fixed by [ADR 0002][adr-0002] (Accepted 2026-04-21). Briefly:
//!
//! - `pgvector` is the primary vector store.
//! - Embedding dimension is 768 (mxbai-embed-large-v1).
//! - Every retrieval is **scoped** by [`Scope`] + `group_id` — the trait
//!   shape makes cross-tenant queries impossible to express.
//!
//! ## Modules
//!
//! - [`types`] — [`Scope`], [`EmbeddingVector`], [`Document`], [`Chunk`],
//!   [`SearchHit`].
//! - [`error`] — typed [`EmbeddingError`].
//! - [`provider`] — [`EmbeddingProvider`] trait and (under
//!   `testing-provider` feature) [`DeterministicProvider`] for tests.
//! - [`store`] — [`VectorStore`] trait.
//! - [`hybrid`] — [`HybridQuery`] builder for FTS+ANN+filter Postgres CTE
//!   queries (ADR 0002 §Decisões item 4).
//!
//! [adr-0002]: https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0002-vector-store.md

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod hybrid;
pub mod provider;
pub mod store;
pub mod types;

pub use error::EmbeddingError;
pub use hybrid::HybridQuery;
pub use provider::EmbeddingProvider;
pub use store::{SearchOptions, VectorStore};
pub use types::{Chunk, Document, EMBEDDING_DIM, EmbeddingVector, Scope, SearchHit};

#[cfg(feature = "testing-provider")]
pub use provider::DeterministicProvider;

/// Convenience [`Result`] alias.
pub type Result<T> = core::result::Result<T, EmbeddingError>;
