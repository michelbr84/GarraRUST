use garraia_common::{Error, Result};

/// Finds relevant skills for a query using embedding-based similarity search.
///
/// Requires `garraia-embeddings` (Fase 2.1, GAR-372). Returns empty list until then.
/// GAR-646 will implement the full retriever.
pub fn retrieve(_query: &str) -> Result<Vec<crate::Skill>> {
    Err(Error::Other(
        "Skill Retriever not yet implemented — requires garraia-embeddings (GAR-646)".into(),
    ))
}
