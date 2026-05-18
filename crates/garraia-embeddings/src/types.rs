//! Strong types shared by [`crate::EmbeddingProvider`] and
//! [`crate::VectorStore`].
//!
//! Three invariants are enforced at the type level:
//!
//! 1. **Dimension.** [`EmbeddingVector`] only ever holds `EMBEDDING_DIM`
//!    floats (currently 768). Construction validates this.
//! 2. **Scope.** Every retrieval call carries a [`Scope`] enum and a
//!    `group_id`. The combination mirrors the `memory_items` row contract from
//!    migration 005 and the RLS policy from migration 007.
//! 3. **Document → chunks → embeddings** is the pipeline shape — callers can't
//!    persist an embedding without an originating chunk and document id.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::EmbeddingError;

/// Embedding vector dimension. Matches `vector(768)` in migration 005 and
/// `mxbai-embed-large-v1`.
pub const EMBEDDING_DIM: usize = 768;

/// Retrieval scope. Mirrors `memory_items.scope_type` from migration 005.
///
/// The semantic precedence (Chat > Group > User when multiple scopes
/// intersect) is enforced by the **caller**, not by this enum — this type
/// just identifies which scope a single request runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Personal memory — visible only to the creator.
    User,
    /// Shared within a group.
    Group,
    /// Bound to a specific chat / channel.
    Chat,
}

impl Scope {
    /// String representation as stored in the `memory_items.scope_type` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Group => "group",
            Scope::Chat => "chat",
        }
    }
}

/// Fixed-dimension embedding vector.
///
/// Constructed via [`EmbeddingVector::try_from_vec`] which validates the
/// dimension. Equality is bitwise on `f32` — callers that want
/// distance-based equality should use cosine similarity at the application
/// layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingVector(Vec<f32>);

impl EmbeddingVector {
    /// Build from a `Vec<f32>`, returning [`EmbeddingError::DimensionMismatch`]
    /// when the length is not [`EMBEDDING_DIM`].
    pub fn try_from_vec(v: Vec<f32>) -> Result<Self, EmbeddingError> {
        if v.len() != EMBEDDING_DIM {
            return Err(EmbeddingError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                actual: v.len(),
            });
        }
        Ok(Self(v))
    }

    /// Borrowed slice — useful for serialization into pgvector's text format
    /// or for cosine-similarity helpers.
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// Consume the vector and return the inner `Vec<f32>`.
    pub fn into_inner(self) -> Vec<f32> {
        self.0
    }
}

/// A document submitted for embedding — the unit of provenance.
///
/// A document is split into [`Chunk`]s by the caller (chunking strategy is
/// out of scope for this crate; the agents/learning layer owns it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Stable identifier — typically the originating `memory_items.id` or
    /// `messages.id`.
    pub id: Uuid,
    /// Document text. Embedded content; never logged via tracing fields
    /// without a redaction layer (see [`crate::error`] for the same rule).
    pub text: String,
    /// Soft tag — `"memory"`, `"message"`, `"skill"`, etc. Free-form so
    /// callers can categorize without bumping the crate.
    pub kind: String,
}

/// A fragment of a [`Document`] suitable for embedding in a single forward
/// pass of the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// Foreign key to the originating [`Document::id`].
    pub document_id: Uuid,
    /// Sequential chunk index within the document, starting at 0.
    pub index: u32,
    /// Chunk text. Same redaction rules as [`Document::text`].
    pub text: String,
}

/// A search result from [`crate::VectorStore::search`].
///
/// `distance` is in cosine-distance units (0.0 = identical, up to 2.0 =
/// opposite). Callers comparing thresholds across providers must agree on
/// the distance metric beforehand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// The `memory_items.id` (or equivalent) of the matched row.
    pub item_id: Uuid,
    /// Cosine distance.
    pub distance: f32,
    /// Whichever scope this row belongs to. Useful when the caller queried
    /// `Scope::Group` but the row's resolved scope is `Chat` (allowed when
    /// `chat.group_id` matches and the chat is included).
    pub scope: Scope,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_serializes_lowercase() {
        let s = serde_json::to_string(&Scope::User).unwrap();
        assert_eq!(s, "\"user\"");
        let s = serde_json::to_string(&Scope::Group).unwrap();
        assert_eq!(s, "\"group\"");
        let s = serde_json::to_string(&Scope::Chat).unwrap();
        assert_eq!(s, "\"chat\"");
    }

    #[test]
    fn scope_round_trip() {
        for sc in [Scope::User, Scope::Group, Scope::Chat] {
            let s = serde_json::to_string(&sc).unwrap();
            let back: Scope = serde_json::from_str(&s).unwrap();
            assert_eq!(sc, back);
        }
    }

    #[test]
    fn scope_rejects_invalid_string() {
        let err = serde_json::from_str::<Scope>("\"admin\"").unwrap_err();
        // Make sure deserialization actually rejected it (rather than silently
        // defaulting). Any error here is acceptable.
        assert!(err.is_data() || err.is_syntax());
    }

    #[test]
    fn scope_as_str_matches_migration_005() {
        // These three literals appear verbatim in migration 005's CHECK
        // constraint. If we change them, that migration must also change.
        assert_eq!(Scope::User.as_str(), "user");
        assert_eq!(Scope::Group.as_str(), "group");
        assert_eq!(Scope::Chat.as_str(), "chat");
    }

    #[test]
    fn embedding_vector_requires_exact_dimension() {
        let v = vec![0.0; EMBEDDING_DIM];
        let ev = EmbeddingVector::try_from_vec(v).unwrap();
        assert_eq!(ev.as_slice().len(), EMBEDDING_DIM);

        let v = vec![0.0; EMBEDDING_DIM - 1];
        let err = EmbeddingVector::try_from_vec(v).unwrap_err();
        assert!(matches!(
            err,
            EmbeddingError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                actual,
            } if actual == EMBEDDING_DIM - 1
        ));

        let v = vec![0.0; EMBEDDING_DIM + 1];
        let err = EmbeddingVector::try_from_vec(v).unwrap_err();
        assert!(matches!(err, EmbeddingError::DimensionMismatch { .. }));
    }

    #[test]
    fn embedding_vector_equality_is_bitwise() {
        let a = EmbeddingVector::try_from_vec(vec![1.0; EMBEDDING_DIM]).unwrap();
        let b = EmbeddingVector::try_from_vec(vec![1.0; EMBEDDING_DIM]).unwrap();
        assert_eq!(a, b);
        let mut c_vec = vec![1.0; EMBEDDING_DIM];
        c_vec[0] = 1.0000001;
        let c = EmbeddingVector::try_from_vec(c_vec).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn embedding_dim_matches_migration_005() {
        // Migration 005 declares `embedding vector(768)`. If this changes we
        // have a multi-step DB migration to plan (see ADR 0002 supersession
        // path).
        assert_eq!(EMBEDDING_DIM, 768);
    }
}
