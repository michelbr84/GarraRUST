pub mod evaluator;
pub mod generator;
pub mod miner;
pub mod registry;
pub mod retriever;
pub mod safety;
pub mod skill_override;
pub mod updater;
pub mod versioning;

pub use safety::{SafetyDenial, gate as safety_gate};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Provenance of a skill candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    /// Automatically mined from session logs.
    Mined,
    /// Written by a human.
    Authored,
    /// Imported from an external source / marketplace.
    Imported,
}

/// Persistence scope of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    /// Per-project: `.garra/skills/`.
    Project,
    /// Cross-project: `~/.garra/skills/`.
    Global,
}

/// Extended frontmatter for a learning-managed skill.
///
/// Distinct from `garraia_skills::SkillFrontmatter` to avoid coupling the base
/// parser crate to Learning Agent-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningSkillFrontmatter {
    pub name: String,
    pub version: String,
    #[serde(default = "default_source")]
    pub source: SkillSource,
    #[serde(default = "default_scope")]
    pub scope: SkillScope,
    /// Exponential moving average quality score, 0.0..=1.0.
    #[serde(default)]
    pub score: f32,
    /// Human-override: never auto-update when true.
    #[serde(default)]
    pub locked: bool,
    /// Paths flagged by the Evaluator as touched by this skill.
    #[serde(default)]
    pub critical_paths_touched: Vec<String>,
    /// Consecutive failure count tracked by the Evaluator.
    #[serde(default)]
    pub fail_count: u32,
    /// Set by the Registry when a skill is retired; preserved for history.
    #[serde(default)]
    pub deprecated: bool,
}

fn default_source() -> SkillSource {
    SkillSource::Mined
}

fn default_scope() -> SkillScope {
    SkillScope::Project
}

/// A skill managed by the Garra Learning Agent.
#[derive(Debug, Clone)]
pub struct Skill {
    pub frontmatter: LearningSkillFrontmatter,
    pub body: String,
    pub source_path: Option<PathBuf>,
}

impl Skill {
    /// Minimum promotion score (Safety Gate threshold).
    pub const MIN_PROMOTE_SCORE: f32 = 0.5;
    /// Consecutive failures before a skill is auto-deprecated (anti-flap).
    pub const ANTI_FLAP_THRESHOLD: u32 = 3;
}
