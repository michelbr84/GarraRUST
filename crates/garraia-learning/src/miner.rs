//! Skill Miner — placeholder for [GAR-643].
//!
//! Will analyse session logs (telemetry traces, shell history) and detect
//! repeated patterns (≥ 3 occurrences in similar contexts) emitting candidate
//! skills to `~/.garra/skills/_candidates/<name>-<sha>.md`.
//!
//! See ADR 0010 §"Loop 1: Mine → Generate → Validate → Promote" and
//! §"Topologia de crates" for the design.
//!
//! [GAR-643]: https://linear.app/chatgpt25/issue/GAR-643
