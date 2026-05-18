use garraia_common::{Error, Result};

/// Analyzes session logs and detects repeated patterns that could become skills.
///
/// GAR-643 will implement this fully. Returns an empty candidate list until then.
pub fn mine_from_log(_log_path: &std::path::Path) -> Result<Vec<crate::Skill>> {
    Err(Error::Other(
        "Skill Miner not yet implemented (GAR-643)".into(),
    ))
}
