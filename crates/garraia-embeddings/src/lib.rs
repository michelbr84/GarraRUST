//! GarraIA — Embeddings & vector search.
//!
//! # ⚠️ Este crate NÃO é o sistema de embeddings em uso (#949)
//!
//! Nada no workspace depende dele: nenhum `Cargo.toml` o declara como
//! dependência e nenhum `.rs` o referencia. Ele é o scaffold da Fase 2.1
//! (plan 0145), e o que ele descreve — pgvector, 768 dimensões fixas,
//! mxbai-embed-large-v1 — **não** é o que roda hoje.
//!
//! O caminho vivo é outro, e são dois módulos em dois crates diferentes:
//!
//! - `garraia_agents::embeddings` tem o `EmbeddingProvider` que o runtime
//!   chama de verdade. É um trait **diferente** e incompatível com o daqui.
//! - `garraia_db::vector_store` guarda os vetores em **sqlite-vec**, em
//!   tabelas virtuais `vec0` chamadas `vec_embeddings_{dims}` — uma por
//!   dimensão, criadas sob demanda. A dimensão é derivada do vetor que o
//!   provider devolve, não fixada em 768.
//!
//! A tabela `memory_embeddings` da migration 005 (pgvector + HNSW) existe e
//! está correta, mas nenhum código de produção lê ou escreve nela ainda.
//!
//! Se você abriu este crate procurando como o GarraIA faz busca semântica,
//! está no arquivo errado. Ver a Amendment de 2026-09-06 na [ADR 0002][adr-0002],
//! que registra a divergência e as duas saídas em aberto (remover este crate
//! ou alinhá-lo). Enquanto isso não se decide, **não** trate a `EMBEDDING_DIM`
//! daqui como verdade sobre o sistema.
//!
//! ---
//!
//! This crate ships the public surface (traits + strong types) that the rest of
//! the GarraIA workspace programs against for embedding-based retrieval. It
//! intentionally contains **no** concrete database wiring — the
//! [`VectorStore`] trait is a definition; a real `PgVectorStore` over `sqlx`
//! lives in a follow-up slice.
//!
//! Design **as scaffolded** follows [ADR 0002][adr-0002] (Accepted 2026-04-21) —
//! see the divergence warning above before relying on any of it. Briefly:
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
