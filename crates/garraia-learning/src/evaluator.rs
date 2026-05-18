use garraia_common::Result;

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

/// Objective signals collected after a skill execution.
#[derive(Debug, Clone)]
pub struct EvalSignals {
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Number of test cases that passed.
    pub tests_passed: u32,
    /// Number of test cases that failed.
    pub tests_failed: u32,
    /// Total lines changed in the diff produced by the skill.
    pub lines_changed: u32,
    /// Number of files touched by the skill.
    pub files_changed: u32,
    /// True if the output log contains "ERROR" (case-insensitive).
    pub log_has_errors: bool,
    /// True if the output log contains "panic" or "PANIC".
    pub log_has_panics: bool,
    /// Elapsed time in milliseconds (optional; not used in scoring yet).
    pub latency_ms: Option<u64>,
}

impl EvalSignals {
    /// Build signals from a simple exit-code-only context (no test runner).
    pub fn from_exit_code(exit_code: i32) -> Self {
        Self {
            exit_code,
            tests_passed: 0,
            tests_failed: 0,
            lines_changed: 0,
            files_changed: 0,
            log_has_errors: exit_code != 0,
            log_has_panics: false,
            latency_ms: None,
        }
    }
}

/// Qualitative outcome of an evaluation run.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalOutcome {
    Pass,
    Fail { reason: String },
    Deprecated { reason: String },
}

/// Result returned by [`evaluate`].
#[derive(Debug, Clone)]
pub struct EvalResult {
    /// Updated EMA score after this evaluation.
    pub new_score: f32,
    /// Updated consecutive failure count.
    pub fail_count: u32,
    /// Whether this evaluation triggered deprecation.
    pub deprecated: bool,
    /// Qualitative outcome with optional reason.
    pub outcome: EvalOutcome,
}

/// Configuration for the EMA score update and deprecation thresholds.
#[derive(Debug, Clone)]
pub struct EmaConfig {
    /// EMA smoothing factor: 0 < alpha ≤ 1. Higher = more reactive.
    pub alpha: f32,
    /// Score below this threshold triggers deprecation.
    pub deprecate_threshold: f32,
    /// Consecutive failures before anti-flap deprecation.
    pub anti_flap_threshold: u32,
}

impl Default for EmaConfig {
    fn default() -> Self {
        Self {
            alpha: 0.3,
            deprecate_threshold: 0.3,
            anti_flap_threshold: crate::Skill::ANTI_FLAP_THRESHOLD,
        }
    }
}

// ──────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────

/// Evaluate a skill execution and mutate the skill's score + fail_count.
///
/// Scoring rubric (weights):
/// - exit_code == 0:                              +0.40
/// - tests_failed == 0 && tests_passed > 0:       +0.30  (else +0.10 when no tests)
/// - !log_has_errors && !log_has_panics:           +0.20
/// - files_changed > 0:                           +0.10
///
/// EMA: `new_score = alpha * raw + (1 - alpha) * current_score`
///
/// Deprecation triggers:
/// - new_score < config.deprecate_threshold
/// - OR consecutive fail_count >= config.anti_flap_threshold
pub fn evaluate(
    skill: &mut crate::Skill,
    signals: &EvalSignals,
    config: &EmaConfig,
) -> Result<EvalResult> {
    let raw = compute_raw_score(signals);
    let new_score = update_ema(skill.frontmatter.score, raw, config.alpha);
    skill.frontmatter.score = new_score;

    let failure = is_failure(signals);
    if failure {
        skill.frontmatter.fail_count = skill.frontmatter.fail_count.saturating_add(1);
    } else {
        skill.frontmatter.fail_count = 0;
    }

    let anti_flap_fired = skill.frontmatter.fail_count >= config.anti_flap_threshold;
    let score_too_low = new_score < config.deprecate_threshold;
    let should_deprecate = anti_flap_fired || score_too_low;

    if should_deprecate {
        skill.frontmatter.deprecated = true;
    }

    let outcome = if should_deprecate {
        let reason = if anti_flap_fired {
            format!(
                "anti-flap: {} consecutive failures (threshold {})",
                skill.frontmatter.fail_count, config.anti_flap_threshold
            )
        } else {
            format!(
                "score {:.3} below deprecation threshold {:.3}",
                new_score, config.deprecate_threshold
            )
        };
        EvalOutcome::Deprecated { reason }
    } else if failure {
        let reason = failure_reason(signals);
        EvalOutcome::Fail { reason }
    } else {
        EvalOutcome::Pass
    };

    Ok(EvalResult {
        new_score,
        fail_count: skill.frontmatter.fail_count,
        deprecated: should_deprecate,
        outcome,
    })
}

// ──────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────

fn compute_raw_score(signals: &EvalSignals) -> f32 {
    let mut score = 0.0_f32;

    if signals.exit_code == 0 {
        score += 0.40;
    }

    if signals.tests_failed == 0 && signals.tests_passed > 0 {
        score += 0.30;
    } else if signals.tests_failed == 0 {
        score += 0.10;
    }

    if !signals.log_has_errors && !signals.log_has_panics {
        score += 0.20;
    }

    if signals.files_changed > 0 {
        score += 0.10;
    }

    score.clamp(0.0, 1.0)
}

fn update_ema(current: f32, raw: f32, alpha: f32) -> f32 {
    let alpha = alpha.clamp(0.0, 1.0);
    (alpha * raw + (1.0 - alpha) * current).clamp(0.0, 1.0)
}

fn is_failure(signals: &EvalSignals) -> bool {
    signals.exit_code != 0 || signals.log_has_panics || signals.tests_failed > 0
}

fn failure_reason(signals: &EvalSignals) -> String {
    let mut parts = Vec::new();
    if signals.exit_code != 0 {
        parts.push(format!("exit_code={}", signals.exit_code));
    }
    if signals.log_has_panics {
        parts.push("panic in log".into());
    }
    if signals.tests_failed > 0 {
        parts.push(format!("{} test(s) failed", signals.tests_failed));
    }
    if parts.is_empty() {
        "unknown failure".into()
    } else {
        parts.join(", ")
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LearningSkillFrontmatter, Skill, SkillScope, SkillSource};

    fn make_skill(score: f32) -> Skill {
        Skill {
            frontmatter: LearningSkillFrontmatter {
                name: "eval-test".into(),
                version: "0.1.0".into(),
                source: SkillSource::Mined,
                scope: SkillScope::Project,
                score,
                locked: false,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            },
            body: String::new(),
            source_path: None,
        }
    }

    fn success_signals() -> EvalSignals {
        EvalSignals {
            exit_code: 0,
            tests_passed: 5,
            tests_failed: 0,
            lines_changed: 10,
            files_changed: 2,
            log_has_errors: false,
            log_has_panics: false,
            latency_ms: Some(120),
        }
    }

    fn failure_signals() -> EvalSignals {
        EvalSignals {
            exit_code: 1,
            tests_passed: 0,
            tests_failed: 2,
            lines_changed: 0,
            files_changed: 0,
            log_has_errors: true,
            log_has_panics: false,
            latency_ms: None,
        }
    }

    #[test]
    fn raw_score_full_success() {
        let score = compute_raw_score(&success_signals());
        assert!((score - 1.0).abs() < 1e-5, "expected 1.0, got {score}");
    }

    #[test]
    fn raw_score_full_failure() {
        let score = compute_raw_score(&failure_signals());
        assert!((score - 0.0).abs() < 1e-5, "expected 0.0, got {score}");
    }

    #[test]
    fn raw_score_no_tests_partial() {
        let s = EvalSignals {
            exit_code: 0,
            tests_passed: 0,
            tests_failed: 0,
            lines_changed: 5,
            files_changed: 1,
            log_has_errors: false,
            log_has_panics: false,
            latency_ms: None,
        };
        let score = compute_raw_score(&s);
        // exit(0.40) + no_tests(0.10) + no_errors(0.20) + files(0.10) = 0.80
        assert!((score - 0.80).abs() < 1e-5, "expected 0.80, got {score}");
    }

    #[test]
    fn raw_score_panic_in_log_loses_log_bonus() {
        let s = EvalSignals {
            exit_code: 0,
            tests_passed: 5,
            tests_failed: 0,
            lines_changed: 3,
            files_changed: 1,
            log_has_errors: false,
            log_has_panics: true,
            latency_ms: None,
        };
        let score = compute_raw_score(&s);
        // exit(0.40) + tests(0.30) + no log bonus + files(0.10) = 0.80
        assert!((score - 0.80).abs() < 1e-5, "expected 0.80, got {score}");
    }

    #[test]
    fn ema_from_zero_with_perfect_score() {
        // current=0.0, raw=1.0, alpha=0.3 → 0.3
        let result = update_ema(0.0, 1.0, 0.3);
        assert!((result - 0.30).abs() < 1e-5, "expected 0.30, got {result}");
    }

    #[test]
    fn ema_stable_at_one() {
        assert!((update_ema(1.0, 1.0, 0.3) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ema_clamps_to_valid_range() {
        assert!((update_ema(0.0, 0.0, 0.3) - 0.0).abs() < 1e-5);
        assert!((update_ema(1.0, 1.0, 1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn evaluate_success_increases_score() {
        let mut skill = make_skill(0.5);
        let result = evaluate(&mut skill, &success_signals(), &EmaConfig::default()).unwrap();
        assert!(result.new_score > 0.5);
        assert_eq!(result.fail_count, 0);
        assert!(!result.deprecated);
        assert_eq!(result.outcome, EvalOutcome::Pass);
    }

    #[test]
    fn evaluate_failure_increments_fail_count() {
        let mut skill = make_skill(0.8);
        let result = evaluate(&mut skill, &failure_signals(), &EmaConfig::default()).unwrap();
        assert_eq!(result.fail_count, 1);
        assert!(!result.deprecated);
        assert!(matches!(result.outcome, EvalOutcome::Fail { .. }));
    }

    #[test]
    fn evaluate_success_resets_fail_count() {
        let mut skill = make_skill(0.7);
        skill.frontmatter.fail_count = 2;
        let result = evaluate(&mut skill, &success_signals(), &EmaConfig::default()).unwrap();
        assert_eq!(result.fail_count, 0);
        assert_eq!(skill.frontmatter.fail_count, 0);
    }

    #[test]
    fn evaluate_three_failures_triggers_anti_flap() {
        let mut skill = make_skill(0.8);
        let cfg = EmaConfig::default();
        evaluate(&mut skill, &failure_signals(), &cfg).unwrap();
        let r2 = evaluate(&mut skill, &failure_signals(), &cfg).unwrap();
        assert!(!r2.deprecated);
        let r3 = evaluate(&mut skill, &failure_signals(), &cfg).unwrap();
        assert!(r3.deprecated);
        assert!(skill.frontmatter.deprecated);
        assert!(matches!(r3.outcome, EvalOutcome::Deprecated { .. }));
    }

    #[test]
    fn evaluate_low_score_triggers_deprecation() {
        let mut skill = make_skill(0.0);
        let result = evaluate(&mut skill, &failure_signals(), &EmaConfig::default()).unwrap();
        assert!(result.deprecated);
        assert!(skill.frontmatter.deprecated);
    }

    #[test]
    fn evaluate_score_above_threshold_not_deprecated() {
        let mut skill = make_skill(0.6);
        let result = evaluate(&mut skill, &success_signals(), &EmaConfig::default()).unwrap();
        assert!(!result.deprecated);
        assert_eq!(result.outcome, EvalOutcome::Pass);
    }

    #[test]
    fn evaluate_mutates_skill_score_in_place() {
        let mut skill = make_skill(0.5);
        evaluate(&mut skill, &success_signals(), &EmaConfig::default()).unwrap();
        assert!(skill.frontmatter.score > 0.5);
    }

    #[test]
    fn from_exit_code_zero_not_error() {
        let s = EvalSignals::from_exit_code(0);
        assert!(!s.log_has_errors);
    }

    #[test]
    fn from_exit_code_nonzero_sets_error() {
        let s = EvalSignals::from_exit_code(1);
        assert!(s.log_has_errors);
    }

    #[test]
    fn failure_reason_includes_all_signals() {
        let s = EvalSignals {
            exit_code: 2,
            tests_passed: 0,
            tests_failed: 3,
            lines_changed: 0,
            files_changed: 0,
            log_has_errors: false,
            log_has_panics: true,
            latency_ms: None,
        };
        let reason = failure_reason(&s);
        assert!(reason.contains("exit_code=2"));
        assert!(reason.contains("panic"));
        assert!(reason.contains("3 test(s) failed"));
    }
}
