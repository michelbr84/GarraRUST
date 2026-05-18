use garraia_common::{Error, Result};
use std::path::PathBuf;

/// Lists skills from both global (`~/.garra/skills/`) and project (`.garra/skills/`) scopes.
///
/// GAR-645 will implement the full dual-scope registry with lock-file support.
pub fn list_skills() -> Result<Vec<crate::Skill>> {
    Err(Error::Other(
        "Skill Registry not yet implemented (GAR-645)".into(),
    ))
}

/// Returns the global skills root: `~/.garra/skills/`.
pub fn global_skills_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::Other("HOME env not set".into()))?;
    Ok(PathBuf::from(home).join(".garra").join("skills"))
}

/// Returns the project skills root: `.garra/skills/` relative to CWD.
pub fn project_skills_dir() -> PathBuf {
    PathBuf::from(".garra").join("skills")
}

/// Promotes a skill candidate into the registry after Safety Gate passes.
///
/// GAR-645 will implement persist + git-track.
pub fn promote(_skill: &crate::Skill) -> Result<()> {
    Err(Error::Other(
        "Skill Registry promote not yet implemented (GAR-645)".into(),
    ))
}
