use garraia_common::{Error, Result};

/// Collects objective metrics after a skill execution (exit code, test pass count,
/// CI checks, diff stats, log scan). Updates `score` via EMA and increments
/// `fail_count` on failure.
///
/// GAR-647 will implement the full evaluator pipeline.
pub fn evaluate(_skill: &mut crate::Skill, _exit_code: i32) -> Result<()> {
    Err(Error::Other(
        "Skill Evaluator not yet implemented (GAR-647)".into(),
    ))
}
