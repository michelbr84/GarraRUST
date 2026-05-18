//! [`VectorStore`] trait — the abstraction over the vector storage backend.
//!
//! ADR 0002 fixes Postgres + pgvector as the primary store. The concrete
//! `PgVectorStore` implementation lives in a follow-up slice and is gated on a
//! DB integration test harness. This module ships only the trait so the rest
//! of the codebase can program against it today.

use async_trait::async_trait;
use uuid::Uuid;

use crate::error::EmbeddingError;
use crate::types::{EmbeddingVector, Scope, SearchHit};

/// Optional tuning knobs for [`VectorStore::search`].
///
/// Defaults are the right answer for the common case (top-5 within a single
/// `group_id`). Power-user knobs land here as the surface matures — keep
/// additions backwards-compatible (new fields default to `None`).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// HNSW `ef_search` — increase for higher recall at the cost of latency.
    /// `None` defaults to the pgvector default (40 in pgvector 0.7).
    pub ef_search: Option<u32>,

    /// Hard cap on cosine distance. Hits with distance > this value are not
    /// returned. `None` returns all top-k regardless of distance.
    pub max_distance: Option<f32>,
}

/// The persistence + retrieval surface for embedding vectors.
///
/// Every method takes a [`Scope`] + `group_id` pair. The combination mirrors
/// the RLS policy on `memory_items` from migration 007 (FORCE RLS on
/// `group_id`, with `scope_type` filtering enforced by the policy USING
/// clause). The trait shape makes cross-tenant queries impossible to
/// express — there is no `search_all_tenants` method.
///
/// Implementations MUST:
///
/// 1. Use the embedding's `model_id` (from [`crate::EmbeddingProvider::model_id`])
///    as the persistence partition. Different models live in different rows
///    of `memory_embeddings`.
/// 2. Refuse to return hits from rows whose `sensitivity = 'secret'` unless
///    the caller passes an explicit opt-in (a future `SearchOptions` field).
/// 3. Apply `ttl_expires_at` filtering: rows past their TTL are excluded
///    even before the cleanup worker has hard-deleted them.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Insert (or upsert) the embedding for a single `memory_items` row.
    ///
    /// `group_id` is required when `scope` is [`Scope::Group`] or
    /// [`Scope::Chat`]; for [`Scope::User`] it MUST be `None` (per migration
    /// 005 `memory_items.group_id` is NULL for personal memories).
    async fn insert(
        &self,
        memory_item_id: Uuid,
        scope: Scope,
        group_id: Option<Uuid>,
        model_id: &str,
        embedding: &EmbeddingVector,
    ) -> Result<(), EmbeddingError>;

    /// Top-k nearest neighbor search scoped to a single tenant + scope.
    ///
    /// Returns up to `limit` hits, sorted by ascending cosine distance.
    async fn search(
        &self,
        scope: Scope,
        group_id: Option<Uuid>,
        model_id: &str,
        query: &EmbeddingVector,
        limit: u32,
        options: SearchOptions,
    ) -> Result<Vec<SearchHit>, EmbeddingError>;

    /// Delete embeddings for a `memory_items` row.
    ///
    /// Typically driven by `memory_items.deleted_at` becoming non-NULL or
    /// by a TTL sweep. Idempotent — deleting twice is not an error.
    async fn delete(&self, memory_item_id: Uuid, model_id: &str) -> Result<(), EmbeddingError>;
}
