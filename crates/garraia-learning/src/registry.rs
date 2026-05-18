//! Skill Registry — placeholder for [GAR-645].
//!
//! Dual-scope wrapper over `garraia-skills::{SkillScanner, SkillInstaller}`:
//! global skills under `~/.garra/skills/` and per-project skills under
//! `.garra/skills/`. Owns the `_candidates/`, `_rejected/`, `_history/`,
//! `_locks/` sub-directories described in ADR 0010 §"Topologia de crates".
//!
//! [GAR-645]: https://linear.app/chatgpt25/issue/GAR-645
