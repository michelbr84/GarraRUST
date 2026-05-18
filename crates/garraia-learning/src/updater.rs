use garraia_common::{Error, Result};

/// Proposes a skill update by opening a git branch + PR via `gh`.
///
/// Branch naming convention: `learning/skill-<name>-v<old>-v<new>`.
/// Never auto-merges — always requires human approval.
///
/// GAR-648 will implement the full updater flow.
pub fn propose_update(_skill: &crate::Skill, _new_body: &str) -> Result<()> {
    Err(Error::Other(
        "Skill Updater not yet implemented (GAR-648)".into(),
    ))
}
