//! Skill Evaluator — placeholder for [GAR-647].
//!
//! Will collect objective post-execution signals (exit codes, cargo test pass
//! count, `gh pr checks`, diff stats, log scan) and feed them into the EMA
//! that powers [`crate::SkillScore`].
//!
//! See ADR 0010 §"Loop 2: Use → Evaluate → Update" for the score lifecycle.
//!
//! [GAR-647]: https://linear.app/chatgpt25/issue/GAR-647
