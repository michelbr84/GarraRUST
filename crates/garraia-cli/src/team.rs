//! Agent Team MVP for GarraMaxPower (GAR-499).
//!
//! Three cooperating agents communicate via typed `std::sync::mpsc` channels
//! and drive a six-phase workflow:
//!
//! ```text
//! OrchestratorAgent
//!     │  (PhaseTask via orch→exec channel)
//!     ▼
//! ExecutorAgent   — runs NativeSkill from garraia-skills registry
//!     │  (ExecMsg via exec→rev channel)
//!     ▼
//! ReviewerAgent   — validates SkillRunOutput
//!     │  (ReviewMsg via rev→orch channel)
//!     ▼
//! OrchestratorAgent  — records PhaseResult, continues or halts
//! ```
//!
//! All agents run synchronously in a single thread in this MVP.
//! The channel seams make a future async upgrade transparent.

use std::sync::mpsc;

use garraia_skills::{NativeSkillRegistry, SkillRunOutput, SkillRunRequest, builtin_registry};

// ── public phase taxonomy ─────────────────────────────────────────────────────

/// Workflow phase executed by the agent team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamPhase {
    Brainstorm,
    Spec,
    Plan,
    Execute,
    Review,
    /// Terminal phase — appended only when all prior phases succeed.
    Finish,
}

impl TeamPhase {
    /// Human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Brainstorm => "Brainstorm",
            Self::Spec => "Spec",
            Self::Plan => "Plan",
            Self::Execute => "Execute",
            Self::Review => "Review",
            Self::Finish => "Finish",
        }
    }
}

// ── public agent roles ────────────────────────────────────────────────────────

/// Role of the agent that produced a `PhaseResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamRole {
    Orchestrator,
    Reviewer,
    Executor,
}

// ── public review outcome ─────────────────────────────────────────────────────

/// Decision emitted by the `ReviewerAgent` for a single skill output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    Accepted,
    NeedsRevision { reason: String },
    Rejected { reason: String },
}

impl ReviewDecision {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

// ── public result types ───────────────────────────────────────────────────────

/// Result for one pipeline phase.
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase: TeamPhase,
    pub role: TeamRole,
    pub output: SkillRunOutput,
    pub decision: ReviewDecision,
}

/// Final summary returned by `AgentTeam::run`.
#[derive(Debug, Clone)]
pub struct TeamSummary {
    pub goal: String,
    pub phases: Vec<PhaseResult>,
    /// `true` when every phase reached `ReviewDecision::Accepted`.
    pub completed: bool,
}

// ── internal message types ────────────────────────────────────────────────────

struct PhaseTask {
    phase: TeamPhase,
    skill_name: &'static str,
    goal: String,
}

enum ExecMsg {
    Completed {
        phase: TeamPhase,
        output: SkillRunOutput,
    },
    Failed {
        phase: TeamPhase,
        reason: String,
    },
}

struct ReviewMsg {
    phase: TeamPhase,
    output: SkillRunOutput,
    decision: ReviewDecision,
}

// ── workflow phase → skill mapping ────────────────────────────────────────────

const WORKFLOW: &[(TeamPhase, &str)] = &[
    (TeamPhase::Brainstorm, "brainstorm"),
    (TeamPhase::Spec, "write-spec"),
    (TeamPhase::Plan, "write-plan"),
    (TeamPhase::Execute, "pre-commit"),
    (TeamPhase::Review, "verify"),
];

// ── executor agent ────────────────────────────────────────────────────────────

struct ExecutorAgent<'a> {
    registry: &'a NativeSkillRegistry,
}

impl ExecutorAgent<'_> {
    /// Receive one `PhaseTask` from `task_rx`, run the matching skill, send
    /// the result to `reply_tx`.
    fn process(&self, task_rx: &mpsc::Receiver<PhaseTask>, reply_tx: &mpsc::Sender<ExecMsg>) {
        let Ok(task) = task_rx.try_recv() else { return };
        let req = SkillRunRequest {
            goal: task.goal.clone(),
            ..SkillRunRequest::default()
        };
        match self.registry.get(task.skill_name) {
            None => {
                reply_tx
                    .send(ExecMsg::Failed {
                        phase: task.phase,
                        reason: format!("skill '{}' not registered", task.skill_name),
                    })
                    .ok();
            }
            Some(skill) => match skill.run(&req) {
                Ok(output) => {
                    reply_tx
                        .send(ExecMsg::Completed {
                            phase: task.phase,
                            output,
                        })
                        .ok();
                }
                Err(e) => {
                    reply_tx
                        .send(ExecMsg::Failed {
                            phase: task.phase,
                            reason: e.to_string(),
                        })
                        .ok();
                }
            },
        }
    }
}

// ── reviewer agent ────────────────────────────────────────────────────────────

struct ReviewerAgent;

impl ReviewerAgent {
    /// Pure validation — does not touch channels.
    fn review(output: &SkillRunOutput) -> ReviewDecision {
        if output.summary.is_empty() {
            return ReviewDecision::Rejected {
                reason: "empty summary".into(),
            };
        }
        if output.next_steps.is_empty() {
            return ReviewDecision::NeedsRevision {
                reason: "skill produced no action steps".into(),
            };
        }
        ReviewDecision::Accepted
    }

    /// Receive one forwarded output from `fwd_rx`, validate it, send `ReviewMsg`
    /// to `decision_tx`.
    fn process(
        fwd_rx: &mpsc::Receiver<(TeamPhase, SkillRunOutput)>,
        decision_tx: &mpsc::Sender<ReviewMsg>,
    ) {
        let Ok((phase, output)) = fwd_rx.try_recv() else {
            return;
        };
        let decision = Self::review(&output);
        decision_tx
            .send(ReviewMsg {
                phase,
                output,
                decision,
            })
            .ok();
    }
}

// ── agent team ────────────────────────────────────────────────────────────────

/// Orchestrates three agents over a five-phase skill workflow.
pub struct AgentTeam {
    registry: NativeSkillRegistry,
}

impl AgentTeam {
    /// Create a team backed by the built-in `NativeSkillRegistry`.
    pub fn new() -> Self {
        Self {
            registry: builtin_registry(),
        }
    }

    /// Run the full pipeline for `goal` and return a `TeamSummary`.
    ///
    /// This method is infallible — phase failures are recorded inside
    /// `PhaseResult.decision` rather than propagated as errors.
    pub fn run(&self, goal: &str) -> TeamSummary {
        // Channels: orch→exec, exec→rev, rev→orch
        let (task_tx, task_rx) = mpsc::channel::<PhaseTask>();
        let (exec_tx, exec_rx) = mpsc::channel::<ExecMsg>();
        let (fwd_tx, fwd_rx) = mpsc::channel::<(TeamPhase, SkillRunOutput)>();
        let (rev_tx, rev_rx) = mpsc::channel::<ReviewMsg>();

        let executor = ExecutorAgent {
            registry: &self.registry,
        };

        let mut results: Vec<PhaseResult> = Vec::with_capacity(WORKFLOW.len() + 1);

        for &(phase, skill_name) in WORKFLOW {
            // 1. Orchestrator → Executor: dispatch task
            task_tx
                .send(PhaseTask {
                    phase,
                    skill_name,
                    goal: goal.to_string(),
                })
                .ok();

            // 2. Executor processes
            executor.process(&task_rx, &exec_tx);

            // 3. Orchestrator reads executor reply, forwards to Reviewer
            match exec_rx.try_recv() {
                Ok(ExecMsg::Completed { phase: p, output }) => {
                    fwd_tx.send((p, output)).ok();
                }
                Ok(ExecMsg::Failed { phase: p, reason }) => {
                    let output = SkillRunOutput {
                        skill_name: skill_name.to_string(),
                        summary: format!("execution failed: {reason}"),
                        next_steps: vec![],
                        commands: vec![],
                    };
                    results.push(PhaseResult {
                        phase: p,
                        role: TeamRole::Executor,
                        output,
                        decision: ReviewDecision::Rejected { reason },
                    });
                    break;
                }
                Err(_) => break,
            }

            // 4. Reviewer validates
            ReviewerAgent::process(&fwd_rx, &rev_tx);

            // 5. Orchestrator collects decision
            match rev_rx.try_recv() {
                Ok(ReviewMsg {
                    phase: p,
                    output,
                    decision,
                }) => {
                    let accepted = decision.is_accepted();
                    results.push(PhaseResult {
                        phase: p,
                        role: TeamRole::Reviewer,
                        output,
                        decision,
                    });
                    if !accepted {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        let completed =
            results.len() == WORKFLOW.len() && results.iter().all(|r| r.decision.is_accepted());

        if completed {
            results.push(PhaseResult {
                phase: TeamPhase::Finish,
                role: TeamRole::Orchestrator,
                output: SkillRunOutput {
                    skill_name: "finish".to_string(),
                    summary: format!("Pipeline completed for: {goal}"),
                    next_steps: vec![
                        "Open a PR and request review.".to_string(),
                        "Check CI green before merge.".to_string(),
                    ],
                    commands: vec![],
                },
                decision: ReviewDecision::Accepted,
            });
        }

        TeamSummary {
            goal: goal.to_string(),
            phases: results,
            completed,
        }
    }
}

impl Default for AgentTeam {
    fn default() -> Self {
        Self::new()
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(goal: &str) -> TeamSummary {
        AgentTeam::new().run(goal)
    }

    #[test]
    fn full_pipeline_completes_for_any_goal() {
        let summary = run("fix the login crash");
        assert!(summary.completed, "pipeline should complete");
    }

    #[test]
    fn full_pipeline_produces_six_phases() {
        let summary = run("add OAuth2 support");
        assert_eq!(summary.phases.len(), 6, "5 skill phases + Finish");
    }

    #[test]
    fn all_phase_decisions_are_accepted() {
        let summary = run("refactor the config module");
        for result in &summary.phases {
            assert!(
                result.decision.is_accepted(),
                "phase {:?} should be accepted",
                result.phase
            );
        }
    }

    #[test]
    fn goal_is_preserved_in_summary() {
        let goal = "improve test coverage for auth";
        let summary = run(goal);
        assert_eq!(summary.goal, goal);
    }

    #[test]
    fn phases_follow_workflow_order() {
        let summary = run("build a new dashboard");
        let expected = [
            TeamPhase::Brainstorm,
            TeamPhase::Spec,
            TeamPhase::Plan,
            TeamPhase::Execute,
            TeamPhase::Review,
            TeamPhase::Finish,
        ];
        let actual: Vec<TeamPhase> = summary.phases.iter().map(|r| r.phase).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn finish_phase_summary_contains_goal() {
        let goal = "document the API";
        let summary = run(goal);
        let finish = summary.phases.last().unwrap();
        assert_eq!(finish.phase, TeamPhase::Finish);
        assert!(finish.output.summary.contains(goal));
    }

    #[test]
    fn finish_phase_has_next_steps() {
        let summary = run("write tests for signup");
        let finish = summary.phases.last().unwrap();
        assert!(!finish.output.next_steps.is_empty());
    }

    #[test]
    fn reviewer_accepts_valid_output() {
        let output = SkillRunOutput {
            skill_name: "test".into(),
            summary: "Did something useful.".into(),
            next_steps: vec!["Next step.".into()],
            commands: vec![],
        };
        assert_eq!(ReviewerAgent::review(&output), ReviewDecision::Accepted);
    }

    #[test]
    fn reviewer_rejects_empty_summary() {
        let output = SkillRunOutput {
            skill_name: "test".into(),
            summary: String::new(),
            next_steps: vec!["step".into()],
            commands: vec![],
        };
        assert!(matches!(
            ReviewerAgent::review(&output),
            ReviewDecision::Rejected { .. }
        ));
    }

    #[test]
    fn reviewer_needs_revision_on_no_steps() {
        let output = SkillRunOutput {
            skill_name: "test".into(),
            summary: "Has summary but no steps.".into(),
            next_steps: vec![],
            commands: vec![],
        };
        assert!(matches!(
            ReviewerAgent::review(&output),
            ReviewDecision::NeedsRevision { .. }
        ));
    }

    #[test]
    fn agent_team_default_is_equivalent_to_new() {
        let a = AgentTeam::new().run("goal");
        let b = AgentTeam::default().run("goal");
        assert_eq!(a.completed, b.completed);
        assert_eq!(a.phases.len(), b.phases.len());
    }

    #[test]
    fn team_phase_label_covers_all_variants() {
        for phase in [
            TeamPhase::Brainstorm,
            TeamPhase::Spec,
            TeamPhase::Plan,
            TeamPhase::Execute,
            TeamPhase::Review,
            TeamPhase::Finish,
        ] {
            assert!(!phase.label().is_empty());
        }
    }

    #[test]
    fn review_decision_is_accepted_only_for_accepted() {
        assert!(ReviewDecision::Accepted.is_accepted());
        assert!(!ReviewDecision::NeedsRevision { reason: "x".into() }.is_accepted());
        assert!(!ReviewDecision::Rejected { reason: "x".into() }.is_accepted());
    }
}
