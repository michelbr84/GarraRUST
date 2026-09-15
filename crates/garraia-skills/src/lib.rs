pub mod hardware;
pub mod installer;
pub mod native;
pub mod parser;
pub mod provider;
pub mod scanner;

pub use hardware::{HardwareProvides, PresetEntry, SkillKind, validate_hardware};
pub use installer::SkillInstaller;
pub use native::{
    NativeSkill, NativeSkillDefinition, NativeSkillRegistry, SkillArgSpec, SkillCommand,
    SkillRunOutput, SkillRunRequest, builtin_registry,
};
pub use parser::{SkillDefinition, SkillFrontmatter, parse_skill, validate_skill};
pub use provider::{SkillCompleter, build_skill_prompt, run_provider_backed};
pub use scanner::SkillScanner;
