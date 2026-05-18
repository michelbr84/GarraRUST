//! `garraia-learning` — Garra Learning Agent / Self-Improving Operations Manual.
//!
//! Scaffold for the [Garra Learning Agent epic (GAR-641)]. This crate currently
//! contains only the **shape** decided in [ADR 0010]: the 10 sub-modules listed
//! in `§Topologia de crates` and the base types referenced by the [acceptance
//! criteria]. Behaviour lands one issue at a time in sub-issues GAR-643..GAR-651.
//!
//! # What this crate provides today
//!
//! * Public type contracts every sub-component agrees on:
//!   [`Skill`], [`SkillScope`], [`SkillSource`], [`SkillScore`], [`SafetyDenial`].
//! * The [`BeforeAction`] trait — the hook an `AgentRuntime` will consume once
//!   the Skill Retriever ships (GAR-646). Until then [`NoopBeforeAction`] is the
//!   default implementation everywhere.
//!
//! # What this crate does NOT do yet
//!
//! Each module file in `src/` is a placeholder marker for its tracking issue.
//! The sub-issues add their dependencies (and behaviour) on demand — see the
//! comment block in `Cargo.toml`.
//!
//! [Garra Learning Agent epic (GAR-641)]: https://linear.app/chatgpt25/issue/GAR-641
//! [ADR 0010]: ../../../docs/adr/0010-garra-learning-agent.md
//! [acceptance criteria]: https://linear.app/chatgpt25/issue/GAR-642

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod evaluator;
pub mod generator;
pub mod miner;
pub mod r#override;
pub mod registry;
pub mod retriever;
pub mod safety;
pub mod updater;
pub mod versioning;

use serde::{Deserialize, Serialize};

/// Where a learnt skill is stored and to whom it applies.
///
/// Mirrors the two on-disk roots described in ADR 0010 §"Topologia de crates"
/// — `~/.garra/skills/` for the local user across projects, and
/// `.garra/skills/` checked into a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    /// `~/.garra/skills/` — applies to the local user across projects.
    Global,
    /// `.garra/skills/` — applies only inside this repository.
    Project,
}

/// How a skill entered the registry.
///
/// Used by the Updater (GAR-648) to decide whether a proposed change requires
/// human review (`Mined`) or is already a human-authored artefact (`Authored`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    /// Detected automatically by the Miner (GAR-643) from session logs.
    Mined,
    /// Authored by a human and committed straight to the registry.
    Authored,
    /// Imported from a signed tarball via `garraia-skills::SkillInstaller`.
    Imported,
}

/// Exponential moving-average score in `[0.0, 1.0]` used by the Evaluator.
///
/// Promotion through the Safety Gate (GAR-649) requires
/// `score.0 >= SkillScore::MIN_PROMOTE`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SkillScore(
    /// The score value, expected to live in `[0.0, 1.0]`.
    pub f32,
);

impl SkillScore {
    /// Default minimum score the Safety Gate accepts for auto-promotion.
    ///
    /// ADR 0010 §"Safety Gate" item 3 lists this as `min_promote_score`
    /// default `0.5`. The constant is the contract surface; the runtime
    /// (GAR-649) will read it from config and may override per-environment.
    pub const MIN_PROMOTE: f32 = 0.5;
}

/// Reason the Safety Gate refused to promote a skill.
///
/// `#[non_exhaustive]` because GAR-649 is expected to add variants as new
/// PII patterns and critical-path families are discovered — adding a new
/// variant must not be a breaking change for downstream matchers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SafetyDenial {
    /// The skill body or steps contain a hard-denied command pattern
    /// (e.g. `rm -rf /`, `git push --force` on `main`).
    #[error("skill contains a hard-denied command pattern")]
    DangerousCommand,
    /// The skill modifies a path that requires human review
    /// (e.g. `crates/garraia-auth/`, `.github/workflows/`, `deny.toml`).
    #[error("skill touches a path that requires human review")]
    CriticalPath,
    /// The skill score is below [`SkillScore::MIN_PROMOTE`].
    #[error("skill score is below the promotion threshold")]
    ScoreTooLow,
    /// The skill has failed Evaluator checks repeatedly (anti-flap).
    #[error("skill has failed evaluator checks repeatedly (anti-flap)")]
    AntiFlap,
    /// The skill content matches a PII redaction rule.
    #[error("skill contains potential PII")]
    PiiLeak,
}

/// Minimal in-memory representation of a learnt skill.
///
/// This is the **contract** surface — concrete on-disk frontmatter (with the
/// `last_used_at`, `last_diff_sha`, `embeddings_model`, `critical_paths_touched`
/// fields from ADR 0010 §"Formato de skill") arrives with GAR-645 (Registry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Stable identifier, kebab-case (e.g. `cleanup-merged-branches`).
    pub name: String,
    /// Whether this skill is global to the user or scoped to the project.
    pub scope: SkillScope,
    /// How this skill entered the registry.
    pub source: SkillSource,
    /// Current EMA score from the Evaluator.
    pub score: SkillScore,
}

/// Context passed to [`BeforeAction::before_action`] so a future Retriever
/// (GAR-646) can match by intent + scope hint.
///
/// Marked `#[non_exhaustive]` so additional hints (e.g. workspace path,
/// active provider, recent action history) can be added in GAR-646 without
/// breaking external implementors.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SkillRequestContext {
    /// Free-text description of what the agent is about to attempt.
    pub intent: String,
    /// Optional scope hint — if `None`, the Retriever considers both Global
    /// and Project skills.
    pub scope_hint: Option<SkillScope>,
}

/// Hook the `AgentRuntime` is expected to call before any agentic action.
///
/// Returning `Some(skill)` injects the matched skill into the runtime prompt.
/// Returning `None` (the default) preserves today's behaviour: the runtime
/// proceeds without consulting the learnt-skills registry.
///
/// The wiring on the `AgentRuntime` side intentionally lives in GAR-646 — this
/// scaffold only ships the trait so sub-issues 2..10 have a stable contract
/// to implement against.
pub trait BeforeAction {
    /// Inspect the upcoming action context and return the top-1 matching skill,
    /// or `None` if no skill should be injected.
    ///
    /// The default impl always returns `None` so any type can `impl BeforeAction
    /// for X {}` without thinking about retrieval until GAR-646.
    fn before_action(&self, _context: &SkillRequestContext) -> Option<Skill> {
        None
    }
}

/// No-op implementation of [`BeforeAction`]. Used by the runtime until the
/// Skill Retriever (GAR-646) ships.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBeforeAction;

impl BeforeAction for NoopBeforeAction {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_before_action_returns_none() {
        let hook = NoopBeforeAction;
        let ctx = SkillRequestContext {
            intent: "cleanup merged branches".into(),
            scope_hint: Some(SkillScope::Project),
        };
        assert!(hook.before_action(&ctx).is_none());
    }

    #[test]
    fn min_promote_is_half() {
        // Guard against accidental tuning of the Safety Gate threshold.
        // ADR 0010 §"Safety Gate" item 3 documents 0.5 as the default.
        assert!((SkillScore::MIN_PROMOTE - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn safety_denial_messages_are_distinct() {
        let variants = [
            SafetyDenial::DangerousCommand,
            SafetyDenial::CriticalPath,
            SafetyDenial::ScoreTooLow,
            SafetyDenial::AntiFlap,
            SafetyDenial::PiiLeak,
        ];
        let messages: Vec<String> = variants.iter().map(ToString::to_string).collect();
        let unique: std::collections::HashSet<&String> = messages.iter().collect();
        assert_eq!(
            unique.len(),
            messages.len(),
            "denial messages must be distinct"
        );
    }

    #[test]
    fn skill_scope_round_trips_serde() {
        for scope in [SkillScope::Global, SkillScope::Project] {
            let json = serde_json::to_string(&scope).unwrap();
            let back: SkillScope = serde_json::from_str(&json).unwrap();
            assert_eq!(scope, back);
        }
    }

    #[test]
    fn skill_source_round_trips_serde() {
        for source in [
            SkillSource::Mined,
            SkillSource::Authored,
            SkillSource::Imported,
        ] {
            let json = serde_json::to_string(&source).unwrap();
            let back: SkillSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, back);
        }
    }
}
