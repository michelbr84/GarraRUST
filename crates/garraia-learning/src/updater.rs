//! Skill Auto-Updater — placeholder for [GAR-648].
//!
//! When the Evaluator detects an execution that out-performed the current
//! skill, the Updater builds a `learning/skill-<name>-vN-vN+1` branch with the
//! proposed diff and opens a PR via `gh`. **Never auto-merges** — human review
//! is part of the contract.
//!
//! [GAR-648]: https://linear.app/chatgpt25/issue/GAR-648
