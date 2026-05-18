use garraia_common::{Error, Result};

/// Rolls back a skill to its previous git-tracked version.
///
/// History is stored in `<skills_dir>/_history/<name>-<sha>.md`.
/// Rollback is implemented via `git revert` of the promoting commit.
///
/// GAR-650 will implement the full versioning + rollback pipeline.
pub fn rollback(_skill_name: &str) -> Result<()> {
    Err(Error::Other(
        "Skill Versioning not yet implemented (GAR-650)".into(),
    ))
}
