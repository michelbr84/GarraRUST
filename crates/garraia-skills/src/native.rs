//! Native GarraMaxPower skills (GAR-498).
//!
//! These skills are first-class Rust registry entries, not loose markdown
//! files. They currently produce deterministic dry-run guidance and safe
//! command plans; later slices can wire the same trait into `AgentRuntime`.

use garraia_common::{Error, Result, safety_gate};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillArgSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSkillDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub args: &'static [SkillArgSpec],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillRunRequest {
    pub goal: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCommand {
    pub description: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRunOutput {
    pub skill_name: String,
    pub summary: String,
    pub next_steps: Vec<String>,
    pub commands: Vec<SkillCommand>,
}

pub trait NativeSkill: Send + Sync {
    fn definition(&self) -> NativeSkillDefinition;
    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput>;
}

pub struct NativeSkillRegistry {
    skills: BTreeMap<&'static str, Box<dyn NativeSkill>>,
}

impl NativeSkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: BTreeMap::new(),
        }
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.insert_builtin(Box::new(BrainstormSkill));
        registry.insert_builtin(Box::new(WriteSpecSkill));
        registry.insert_builtin(Box::new(WritePlanSkill));
        registry.insert_builtin(Box::new(PreCommitSkill));
        registry.insert_builtin(Box::new(VerifySkill));
        registry
    }

    pub fn register<T>(&mut self, skill: T) -> Result<()>
    where
        T: NativeSkill + 'static,
    {
        let name = skill.definition().name;
        if self.skills.contains_key(name) {
            return Err(Error::Skill(format!(
                "native skill '{name}' already registered"
            )));
        }
        self.skills.insert(name, Box::new(skill));
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn NativeSkill> {
        self.skills.get(name).map(|skill| skill.as_ref())
    }

    pub fn skill_names(&self) -> Vec<&'static str> {
        self.skills.keys().copied().collect()
    }

    pub fn definitions(&self) -> Vec<NativeSkillDefinition> {
        self.skills
            .values()
            .map(|skill| skill.definition())
            .collect()
    }

    fn insert_builtin(&mut self, skill: Box<dyn NativeSkill>) {
        self.skills.insert(skill.definition().name, skill);
    }
}

impl Default for NativeSkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn builtin_registry() -> NativeSkillRegistry {
    NativeSkillRegistry::with_builtins()
}

const GOAL_ARGS: [SkillArgSpec; 1] = [SkillArgSpec {
    name: "goal",
    description: "Task, bug, feature, or project goal to process.",
    required: true,
}];

const PRE_COMMIT_ARGS: [SkillArgSpec; 1] = [SkillArgSpec {
    name: "goal",
    description: "Change scope to validate before commit.",
    required: false,
}];

struct BrainstormSkill;
struct WriteSpecSkill;
struct WritePlanSkill;
struct PreCommitSkill;
struct VerifySkill;

impl NativeSkill for BrainstormSkill {
    fn definition(&self) -> NativeSkillDefinition {
        NativeSkillDefinition {
            name: "brainstorm",
            description: "Explore options, constraints, and the smallest reversible slice.",
            args: &GOAL_ARGS,
        }
    }

    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput> {
        let goal = goal_or_default(request);
        Ok(output(
            "brainstorm",
            format!("Brainstorm options for: {goal}"),
            [
                "Name the user-visible outcome and non-goals.",
                "List 3 viable approaches with risk and reversibility.",
                "Select the smallest slice that proves the direction.",
            ],
            [],
        ))
    }
}

impl NativeSkill for WriteSpecSkill {
    fn definition(&self) -> NativeSkillDefinition {
        NativeSkillDefinition {
            name: "write-spec",
            description: "Turn a selected idea into concrete acceptance criteria.",
            args: &GOAL_ARGS,
        }
    }

    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput> {
        let goal = goal_or_default(request);
        Ok(output(
            "write-spec",
            format!("Draft acceptance criteria for: {goal}"),
            [
                "State the problem, scope, and explicit non-goals.",
                "Write observable acceptance criteria before implementation.",
                "Capture security, privacy, and rollback considerations.",
            ],
            [],
        ))
    }
}

impl NativeSkill for WritePlanSkill {
    fn definition(&self) -> NativeSkillDefinition {
        NativeSkillDefinition {
            name: "write-plan",
            description: "Break an accepted spec into safe implementation steps.",
            args: &GOAL_ARGS,
        }
    }

    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput> {
        let goal = goal_or_default(request);
        Ok(output(
            "write-plan",
            format!("Plan implementation for: {goal}"),
            [
                "Identify files/modules likely to change and ownership boundaries.",
                "Choose focused tests that cover the risky behavior first.",
                "Sequence edits so each step can be reviewed independently.",
            ],
            [],
        ))
    }
}

impl NativeSkill for PreCommitSkill {
    fn definition(&self) -> NativeSkillDefinition {
        NativeSkillDefinition {
            name: "pre-commit",
            description: "Prepare safe validation commands before commit.",
            args: &PRE_COMMIT_ARGS,
        }
    }

    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput> {
        let commands = safe_commands([
            ("format check", "cargo fmt --check"),
            ("strict clippy", "cargo clippy --workspace -- -D warnings"),
            ("workspace tests", "cargo test --workspace"),
            ("secret scan when installed", "gitleaks detect --no-git"),
        ])?;
        let goal = goal_or_default(request);
        Ok(output(
            "pre-commit",
            format!("Validate change scope before committing: {goal}"),
            [
                "Inspect the diff for secrets, generated churn, and unrelated files.",
                "Run the safe validation commands in order.",
                "Commit only the coherent scope that passed or document blockers.",
            ],
            commands,
        ))
    }
}

impl NativeSkill for VerifySkill {
    fn definition(&self) -> NativeSkillDefinition {
        NativeSkillDefinition {
            name: "verify",
            description: "Delegate to the local garra verify pipeline.",
            args: &PRE_COMMIT_ARGS,
        }
    }

    fn run(&self, request: &SkillRunRequest) -> Result<SkillRunOutput> {
        let commands = safe_commands([("Garra validation pipeline", "garra verify --json")])?;
        let goal = goal_or_default(request);
        Ok(output(
            "verify",
            format!("Run the canonical verification pipeline for: {goal}"),
            [
                "Use the JSON report as the stable machine-readable result.",
                "Treat skipped optional tools as evidence to report, not success.",
                "Do not mark a change validated until failing required steps are fixed.",
            ],
            commands,
        ))
    }
}

fn goal_or_default(request: &SkillRunRequest) -> String {
    let trimmed = request.goal.trim();
    if trimmed.is_empty() {
        "the requested change".to_string()
    } else {
        trimmed.to_string()
    }
}

fn output<S, I, C>(name: &str, summary: String, next_steps: I, commands: C) -> SkillRunOutput
where
    S: AsRef<str>,
    I: IntoIterator<Item = S>,
    C: IntoIterator<Item = SkillCommand>,
{
    SkillRunOutput {
        skill_name: name.to_string(),
        summary,
        next_steps: next_steps
            .into_iter()
            .map(|step| step.as_ref().to_string())
            .collect(),
        commands: commands.into_iter().collect(),
    }
}

fn safe_commands<const N: usize>(commands: [(&str, &str); N]) -> Result<Vec<SkillCommand>> {
    commands
        .into_iter()
        .map(|(description, command)| {
            safety_gate::safety_gate(command).map_err(|err| {
                Error::Skill(format!(
                    "native skill command '{description}' rejected by safety gate: {err}"
                ))
            })?;
            Ok(SkillCommand {
                description: description.to_string(),
                command: command.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_exposes_gar_498_mvp_skills() {
        let registry = builtin_registry();
        assert_eq!(
            registry.skill_names(),
            vec![
                "brainstorm",
                "pre-commit",
                "verify",
                "write-plan",
                "write-spec"
            ]
        );
    }

    #[test]
    fn builtin_definitions_have_descriptions_and_args() {
        let registry = builtin_registry();
        let definitions = registry.definitions();

        assert_eq!(definitions.len(), 5);
        for definition in definitions {
            assert!(!definition.description.is_empty());
            assert!(!definition.args.is_empty());
        }
    }

    #[test]
    fn get_returns_registered_native_skill() {
        let registry = builtin_registry();
        let skill = registry.get("write-spec").unwrap();

        assert_eq!(skill.definition().name, "write-spec");
    }

    #[test]
    fn run_brainstorm_returns_deterministic_steps() {
        let registry = builtin_registry();
        let output = registry
            .get("brainstorm")
            .unwrap()
            .run(&SkillRunRequest {
                goal: "improve uploads".to_string(),
                dry_run: true,
                ..SkillRunRequest::default()
            })
            .unwrap();

        assert_eq!(output.skill_name, "brainstorm");
        assert!(output.summary.contains("improve uploads"));
        assert_eq!(output.next_steps.len(), 3);
        assert!(output.commands.is_empty());
    }

    #[test]
    fn pre_commit_commands_pass_safety_gate() {
        let registry = builtin_registry();
        let output = registry
            .get("pre-commit")
            .unwrap()
            .run(&SkillRunRequest::default())
            .unwrap();

        assert_eq!(output.commands.len(), 4);
        for command in output.commands {
            safety_gate::safety_gate(&command.command).unwrap();
        }
    }

    #[test]
    fn verify_delegates_to_garra_verify_json() {
        let registry = builtin_registry();
        let output = registry
            .get("verify")
            .unwrap()
            .run(&SkillRunRequest::default())
            .unwrap();

        assert_eq!(output.commands[0].command, "garra verify --json");
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        struct DuplicateBrainstorm;

        impl NativeSkill for DuplicateBrainstorm {
            fn definition(&self) -> NativeSkillDefinition {
                NativeSkillDefinition {
                    name: "brainstorm",
                    description: "duplicate",
                    args: &GOAL_ARGS,
                }
            }

            fn run(&self, _request: &SkillRunRequest) -> Result<SkillRunOutput> {
                unreachable!()
            }
        }

        let mut registry = builtin_registry();
        let err = registry.register(DuplicateBrainstorm).unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }
}
