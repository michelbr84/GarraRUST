//! Skill Safety Gate — placeholder for [GAR-649] (Urgent).
//!
//! Hard wall before any promotion (auto or manual). Will implement
//! `gate(&Skill) -> Result<(), crate::SafetyDenial>` covering the five
//! denial families: dangerous commands, critical paths, score threshold,
//! anti-flap, PII leak. See ADR 0010 §"Safety Gate (hard wall, sem bypass)".
//!
//! Shares the denylist primitive with `garraia-tools::safety_gate` from the
//! GarraMaxPower work (single source of truth across both surfaces).
//!
//! [GAR-649]: https://linear.app/chatgpt25/issue/GAR-649
