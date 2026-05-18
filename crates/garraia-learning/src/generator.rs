use garraia_common::{Error, Result};

/// Drafts a skill document from a raw candidate using an LLM provider.
///
/// GAR-644 will implement this fully. Default provider: openrouter/free.
pub fn generate_from_candidate(_candidate_body: &str) -> Result<crate::Skill> {
    Err(Error::Other(
        "Skill Generator not yet implemented (GAR-644)".into(),
    ))
}
