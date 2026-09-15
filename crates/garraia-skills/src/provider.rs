//! Provider-backed async execution for native skills (GAR-498 follow-up).
//!
//! The sync [`NativeSkill::run`] is deterministic by design — it never
//! touches the network, so it stays testable and safe in any context. This
//! module adds the missing slice recorded in `TODO.md`: when a provider is
//! available, a skill can be executed **async** with the model filling in
//! the substantive content, while the deterministic output remains the
//! fallback and the scaffold.
//!
//! Dependency inversion: `garraia-skills` does NOT depend on
//! `garraia-agents` (that edge would pull the whole provider tree into
//! every skills consumer and risks cycles). Instead, callers implement the
//! tiny [`SkillCompleter`] trait — the CLI does this over `AgentRuntime`,
//! which keeps provider selection, retries and circuit breaking in one
//! place.

use crate::native::{NativeSkill, NativeSkillDefinition, SkillRunOutput, SkillRunRequest};
use garraia_common::Result;
use std::future::Future;
use std::pin::Pin;

/// Async completion seam between a native skill and an LLM provider.
///
/// Object-safe and `Send`-bound so it can be held across `.await` points
/// in the agent-team pipeline. Implementors own retry/fallback policy —
/// the skill only sees a prompt in, text out.
pub trait SkillCompleter: Send + Sync {
    fn complete<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

/// Build the prompt sent to the provider for one skill run.
///
/// Deterministic and pure (unit-tested): carries the skill identity, the
/// user goal, the argument contract and the deterministic guidance as the
/// scaffold the model must flesh out — never the other way around.
pub fn build_skill_prompt(
    def: &NativeSkillDefinition,
    request: &SkillRunRequest,
    guidance: &[String],
) -> String {
    let mut prompt = format!(
        "You are executing the native skill `{}` of the GarraIA agent team.\n\
         Skill purpose: {}\n\
         User goal: {}\n",
        def.name, def.description, request.goal
    );
    if !def.args.is_empty() {
        prompt.push_str("Arguments:\n");
        for arg in def.args {
            let req = if arg.required { "required" } else { "optional" };
            prompt.push_str(&format!("- {} ({}): {}\n", arg.name, req, arg.description));
        }
    }
    if !guidance.is_empty() {
        prompt.push_str("\nThe deterministic pipeline expects these outcomes:\n");
        for step in guidance {
            prompt.push_str(&format!("- {step}\n"));
        }
    }
    prompt.push_str(
        "\nRespond with the skill's substantive output for this phase only — \
         no preamble, no tool use, plain text the orchestrator can file.\n",
    );
    prompt
}

/// Execute `skill` asynchronously with optional provider backing.
///
/// Policy (fail-closed, documented in TODO.md):
///
/// - `dry_run = true` → deterministic only, the completer is **never**
///   invoked (no accidental network calls in dry runs).
/// - `completer = None` → deterministic output plus an explicit next step
///   telling the orchestrator to wire a provider; never a silent downgrade.
/// - `completer = Some` → the model text is attached as
///   [`SkillRunOutput::model_output`]; the deterministic scaffold is kept
///   so reviewers can diff guidance vs. model output.
pub async fn run_provider_backed(
    skill: &dyn NativeSkill,
    request: &SkillRunRequest,
    completer: Option<&dyn SkillCompleter>,
) -> Result<SkillRunOutput> {
    let mut output = skill.run(request)?;
    if request.dry_run {
        return Ok(output);
    }
    let Some(completer) = completer else {
        output.next_steps.push(
            "Provider-backed execution unavailable: no completer configured \
             (run `garra max-power` with a default provider to enable it)."
                .into(),
        );
        return Ok(output);
    };

    let def = skill.definition();
    let prompt = build_skill_prompt(&def, request, &output.next_steps);
    let text = completer.complete(&prompt).await?;
    output.model_output = Some(text);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::builtin_registry;
    use garraia_common::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeCompleter {
        calls: AtomicUsize,
        text: &'static str,
        fail: bool,
    }

    impl FakeCompleter {
        fn ok(text: &'static str) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                text,
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                text: "",
                fail: true,
            }
        }

        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SkillCompleter for FakeCompleter {
        fn complete<'a>(
            &'a self,
            _prompt: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = self.text;
            let fail = self.fail;
            Box::pin(async move {
                if fail {
                    Err(Error::Skill("provider down".into()))
                } else {
                    Ok(text.to_string())
                }
            })
        }
    }

    fn request(dry_run: bool) -> SkillRunRequest {
        SkillRunRequest {
            goal: "consertar o bug do login".into(),
            dry_run,
            ..SkillRunRequest::default()
        }
    }

    #[tokio::test]
    async fn dry_run_never_calls_the_completer() {
        let fake = FakeCompleter::ok("modelo disse algo");
        let registry = builtin_registry();
        let skill = registry.get("brainstorm").expect("brainstorm registered");
        let out = run_provider_backed(skill, &request(true), Some(&fake))
            .await
            .expect("dry run ok");
        assert_eq!(fake.count(), 0, "dry run must not touch the provider");
        assert!(out.model_output.is_none());
    }

    #[tokio::test]
    async fn provider_backed_run_attaches_model_output() {
        let fake = FakeCompleter::ok("opcao A: menor slice reversivel");
        let registry = builtin_registry();
        let skill = registry.get("brainstorm").expect("brainstorm registered");
        let out = run_provider_backed(skill, &request(false), Some(&fake))
            .await
            .expect("provider-backed run ok");
        assert_eq!(fake.count(), 1);
        assert_eq!(
            out.model_output.as_deref(),
            Some("opcao A: menor slice reversivel")
        );
        // Deterministic scaffold is preserved for review.
        assert!(!out.next_steps.is_empty());
    }

    #[tokio::test]
    async fn missing_completer_degrades_with_explicit_hint() {
        let registry = builtin_registry();
        let skill = registry.get("write-spec").expect("write-spec registered");
        let out = run_provider_backed(skill, &request(false), None)
            .await
            .expect("degraded run ok");
        assert!(out.model_output.is_none());
        let last = out.next_steps.last().expect("hint appended");
        assert!(last.contains("no completer configured"), "got: {last}");
    }

    #[tokio::test]
    async fn completer_error_propagates() {
        let fake = FakeCompleter::failing();
        let registry = builtin_registry();
        let skill = registry.get("write-plan").expect("write-plan registered");
        let err = run_provider_backed(skill, &request(false), Some(&fake))
            .await
            .expect_err("provider failure must propagate");
        assert!(err.to_string().contains("provider down"));
    }

    #[test]
    fn prompt_carries_skill_identity_goal_and_guidance() {
        let registry = builtin_registry();
        let skill = registry.get("brainstorm").expect("brainstorm registered");
        let def = skill.definition();
        let prompt = build_skill_prompt(&def, &request(false), &["step-1".into()]);
        assert!(prompt.contains("`brainstorm`"), "skill name missing");
        assert!(prompt.contains("consertar o bug do login"), "goal missing");
        assert!(prompt.contains("step-1"), "guidance missing");
        assert!(prompt.contains("required"), "arg contract missing");
    }
}
