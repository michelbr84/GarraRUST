//! [`HybridQuery`] — typed builder for hybrid retrieval queries.
//!
//! "Hybrid" here means combining:
//!
//! - Full-text search (Postgres `tsvector @@ tsquery`)
//! - Approximate nearest neighbor (`pgvector` HNSW cosine)
//! - Structural filters (`scope_type`, `group_id`, `sensitivity`, `kind`)
//!
//! ADR 0002 §Decisões item 4 specifies this is expressed as a single Postgres
//! CTE; this builder is the language-level surface that produces the typed
//! query value executed by the concrete `VectorStore` impl.
//!
//! The actual SQL string is **not** materialized in this crate. Producing it
//! is a concern of `PgVectorStore` (future PR) which has the connection
//! handle, the parameter binder, and the right place to run `EXPLAIN`.

use uuid::Uuid;

use crate::error::EmbeddingError;
use crate::types::{EmbeddingVector, Scope};

/// Typed builder for a hybrid retrieval query.
///
/// Required fields (the builder errors out at [`HybridQuery::build`] if any
/// are missing):
///
/// - `scope`
/// - `model_id`
/// - `vector`
///
/// `group_id` is **required when scope ≠ User** and **forbidden when scope =
/// User** (per migration 005's `memory_items.group_id` contract).
#[derive(Debug, Default, Clone)]
pub struct HybridQueryBuilder {
    scope: Option<Scope>,
    group_id: Option<Uuid>,
    model_id: Option<String>,
    vector: Option<EmbeddingVector>,
    text_query: Option<String>,
    kind: Option<String>,
    include_secret: bool,
    limit: u32,
}

impl HybridQueryBuilder {
    /// Start a new builder.
    pub fn new() -> Self {
        Self {
            limit: 5,
            ..Default::default()
        }
    }

    /// Set the retrieval scope (required).
    pub fn scope(mut self, scope: Scope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Bind the query to a `group_id`. Required for `Scope::Group` and
    /// `Scope::Chat`; forbidden for `Scope::User`.
    pub fn group_id(mut self, group_id: Uuid) -> Self {
        self.group_id = Some(group_id);
        self
    }

    /// Set the embedding model partition (required).
    pub fn model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Set the dense query vector (required).
    pub fn vector(mut self, vector: EmbeddingVector) -> Self {
        self.vector = Some(vector);
        self
    }

    /// Optional FTS clause. When `None`, the query is pure ANN.
    pub fn text(mut self, query: impl Into<String>) -> Self {
        self.text_query = Some(query.into());
        self
    }

    /// Optional `memory_items.kind` filter (`"fact"`, `"profile"`, etc.).
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Opt in to including `sensitivity = 'secret'` rows in the result set.
    /// Defaults to **false** to enforce the retrieval invariant documented
    /// on the `memory_items.sensitivity` column.
    pub fn include_secret(mut self, include: bool) -> Self {
        self.include_secret = include;
        self
    }

    /// Top-k limit. Defaults to 5.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    /// Validate the builder and produce a [`HybridQuery`] value.
    pub fn build(self) -> Result<HybridQuery, EmbeddingError> {
        let scope = self
            .scope
            .ok_or(EmbeddingError::MissingField { field: "scope" })?;
        let model_id = self
            .model_id
            .ok_or(EmbeddingError::MissingField { field: "model_id" })?;
        let vector = self
            .vector
            .ok_or(EmbeddingError::MissingField { field: "vector" })?;

        match (scope, self.group_id) {
            (Scope::User, Some(_)) => {
                return Err(EmbeddingError::InvalidScope {
                    reason: "group_id must be None for Scope::User",
                });
            }
            (Scope::User, None) => { /* ok */ }
            (Scope::Group | Scope::Chat, None) => {
                return Err(EmbeddingError::InvalidScope {
                    reason: "group_id required for Scope::Group / Scope::Chat",
                });
            }
            (Scope::Group | Scope::Chat, Some(_)) => { /* ok */ }
        }

        if self.limit == 0 {
            return Err(EmbeddingError::InvalidScope {
                reason: "limit must be >= 1",
            });
        }

        Ok(HybridQuery {
            scope,
            group_id: self.group_id,
            model_id,
            vector,
            text_query: self.text_query,
            kind: self.kind,
            include_secret: self.include_secret,
            limit: self.limit,
        })
    }
}

/// A fully-validated hybrid query, ready to be executed by a `VectorStore`
/// implementation.
#[derive(Debug, Clone)]
pub struct HybridQuery {
    /// Retrieval scope.
    pub scope: Scope,
    /// Group binding (`None` only when [`scope`](Self::scope) is
    /// [`Scope::User`]).
    pub group_id: Option<Uuid>,
    /// Embedding model partition.
    pub model_id: String,
    /// Dense query vector.
    pub vector: EmbeddingVector,
    /// Optional FTS clause.
    pub text_query: Option<String>,
    /// Optional `memory_items.kind` filter.
    pub kind: Option<String>,
    /// Whether to include `sensitivity = 'secret'` rows.
    pub include_secret: bool,
    /// Top-k limit.
    pub limit: u32,
}

impl HybridQuery {
    /// Convenience constructor returning a fresh [`HybridQueryBuilder`].
    pub fn builder() -> HybridQueryBuilder {
        HybridQueryBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EMBEDDING_DIM;

    fn fake_vector() -> EmbeddingVector {
        EmbeddingVector::try_from_vec(vec![0.0; EMBEDDING_DIM]).unwrap()
    }

    #[test]
    fn build_succeeds_with_minimum_fields_user_scope() {
        let q = HybridQuery::builder()
            .scope(Scope::User)
            .model_id("deterministic-sha256-768")
            .vector(fake_vector())
            .build()
            .unwrap();
        assert!(matches!(q.scope, Scope::User));
        assert_eq!(q.limit, 5);
        assert!(!q.include_secret);
    }

    #[test]
    fn build_requires_group_id_for_group_scope() {
        let err = HybridQuery::builder()
            .scope(Scope::Group)
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap_err();
        assert!(matches!(err, EmbeddingError::InvalidScope { .. }));
    }

    #[test]
    fn build_requires_group_id_for_chat_scope() {
        let err = HybridQuery::builder()
            .scope(Scope::Chat)
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap_err();
        assert!(matches!(err, EmbeddingError::InvalidScope { .. }));
    }

    #[test]
    fn build_rejects_group_id_on_user_scope() {
        let err = HybridQuery::builder()
            .scope(Scope::User)
            .group_id(Uuid::nil())
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap_err();
        assert!(matches!(err, EmbeddingError::InvalidScope { .. }));
    }

    #[test]
    fn build_accepts_group_scope_with_group_id() {
        let q = HybridQuery::builder()
            .scope(Scope::Group)
            .group_id(Uuid::nil())
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap();
        assert_eq!(q.group_id, Some(Uuid::nil()));
    }

    #[test]
    fn build_rejects_missing_required_fields() {
        // Missing scope.
        let err = HybridQuery::builder()
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            EmbeddingError::MissingField { field: "scope" }
        ));

        // Missing model_id.
        let err = HybridQuery::builder()
            .scope(Scope::User)
            .vector(fake_vector())
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            EmbeddingError::MissingField { field: "model_id" }
        ));

        // Missing vector.
        let err = HybridQuery::builder()
            .scope(Scope::User)
            .model_id("m")
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            EmbeddingError::MissingField { field: "vector" }
        ));
    }

    #[test]
    fn build_rejects_zero_limit() {
        let err = HybridQuery::builder()
            .scope(Scope::User)
            .model_id("m")
            .vector(fake_vector())
            .limit(0)
            .build()
            .unwrap_err();
        assert!(matches!(err, EmbeddingError::InvalidScope { .. }));
    }

    #[test]
    fn include_secret_defaults_off() {
        let q = HybridQuery::builder()
            .scope(Scope::User)
            .model_id("m")
            .vector(fake_vector())
            .build()
            .unwrap();
        assert!(!q.include_secret);
    }
}
