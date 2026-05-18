//! Skill Override — placeholder.
//!
//! CLI / Web UI surface for human approval workflows: `approve`, `reject`,
//! `lock`, `delete`. Documented in ADR 0010 §"Topologia de crates" as
//! `override.rs` and consumed by the Web Console panel in [GAR-651].
//!
//! Declared as `pub mod r#override` in `lib.rs` because `override` is a
//! reserved Rust keyword — the raw identifier preserves the file name
//! verbatim against the ADR.
//!
//! [GAR-651]: https://linear.app/chatgpt25/issue/GAR-651
