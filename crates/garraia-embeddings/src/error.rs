//! Typed errors for embedding and vector-search operations.
//!
//! `Display` impls in this module deliberately **omit** the embedding inputs
//! (text, chunk content, IDs). The same redaction discipline applies as for
//! `memory_items.content` (see migration 005 comments) — PII never leaks via
//! error strings.

use thiserror::Error;

/// All errors surfaced by this crate.
#[derive(Debug, Error)]
pub enum EmbeddingError {
    /// The embedding vector did not have the expected dimension.
    ///
    /// The actual dimension is reported; the offending vector is not.
    #[error("embedding vector dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// The dimension required by the [`crate::types::EmbeddingVector`] type
        /// (currently 768).
        expected: usize,
        /// The dimension observed at construction time.
        actual: usize,
    },

    /// A required field on a builder (e.g. [`crate::HybridQuery`]) was not set.
    ///
    /// The field name is included; user input is not.
    #[error("required field `{field}` missing")]
    MissingField {
        /// Name of the missing field.
        field: &'static str,
    },

    /// A required scope-bound parameter was missing or inconsistent.
    ///
    /// This is the catch-all for "you passed `Scope::Group` but no
    /// `group_id`" — the trait API tries to make these unrepresentable,
    /// but builder paths must still validate.
    #[error("invalid scope: {reason}")]
    InvalidScope {
        /// Static description of the violation; never user input.
        reason: &'static str,
    },

    /// The provider rejected an input (size, encoding, language, etc.).
    ///
    /// The reason is a static string — `Display` does not echo user content.
    #[error("embedding provider rejected input: {reason}")]
    ProviderRejected {
        /// Static description.
        reason: &'static str,
    },

    /// The provider failed transiently (network, model load, etc.).
    ///
    /// The underlying error is preserved for logs but not for `Display`.
    #[error("embedding provider failure")]
    ProviderFailure(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The vector store failed transiently (db down, timeout, etc.).
    #[error("vector store failure")]
    StoreFailure(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_does_not_leak_inputs() {
        // The Display string for ProviderRejected uses the static reason field
        // only — there is no path to embed user content in it.
        let err = EmbeddingError::ProviderRejected {
            reason: "input too long",
        };
        let s = format!("{err}");
        assert!(s.contains("input too long"));
        // Smoke: it does NOT contain anything we might have leaked.
        assert!(!s.contains("secret-token"));
    }

    #[test]
    fn dimension_mismatch_carries_numbers_not_content() {
        let err = EmbeddingError::DimensionMismatch {
            expected: 768,
            actual: 384,
        };
        let s = format!("{err}");
        assert!(s.contains("768"));
        assert!(s.contains("384"));
    }
}
