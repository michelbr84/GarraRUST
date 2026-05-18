use garraia_common::{Error, Result};

/// Approves a skill candidate for promotion to the registry.
///
/// After approval the Safety Gate still runs — approval gates human intent,
/// Safety Gate gates hard rules.
///
/// GAR-645 (Skill Registry) will wire the full approve flow.
pub fn approve(_skill_name: &str) -> Result<()> {
    Err(Error::Other(
        "Skill approve not yet implemented (GAR-645)".into(),
    ))
}

/// Rejects a skill candidate, moving it to `_rejected/<name>-<ts>.md`.
pub fn reject(_skill_name: &str) -> Result<()> {
    Err(Error::Other(
        "Skill reject not yet implemented (GAR-645)".into(),
    ))
}

/// Locks a skill so it is never auto-updated.
/// Future update proposals still open PRs, but only via human trigger.
pub fn lock(_skill_name: &str) -> Result<()> {
    Err(Error::Other(
        "Skill lock not yet implemented (GAR-645)".into(),
    ))
}

/// Permanently deletes a skill from the registry.
pub fn delete(_skill_name: &str) -> Result<()> {
    Err(Error::Other(
        "Skill delete not yet implemented (GAR-645)".into(),
    ))
}
