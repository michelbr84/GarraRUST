use crate::Skill;
use thiserror::Error;

/// Reason a skill was denied by the Safety Gate.
#[derive(Debug, Error, PartialEq)]
pub enum SafetyDenial {
    #[error("dangerous command detected: {0}")]
    DangerousCommand(String),

    #[error("skill touches critical path requiring security review: {0}")]
    CriticalPath(String),

    #[error("score {score:.2} is below minimum {minimum:.2}")]
    ScoreTooLow { score: f32, minimum: f32 },

    #[error("skill deprecated after {fail_count} consecutive failures (anti-flap)")]
    AntiFlapDeprecated { fail_count: u32 },

    #[error("potential PII detected in skill body: {0}")]
    PiiDetected(String),
}

/// Patterns that are never allowed in a skill body, regardless of context.
///
/// Checked case-insensitively against the full body text.
/// Hard wall: no `SafetyIntent` label can waive these.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $home",
    "rm -rf ${home}",
    ":drop table",
    ":drop database",
    "truncate table",
    "drop table",
    "drop database",
    "where 1=1",
    "git push --force origin main",
    "git push --force-with-lease origin main",
    "git push -f origin main",
    "git push origin main --force",
    ":(){ :|:& };:",
    "mkfs.",
    "dd if=/dev/zero",
    "dd if=/dev/urandom",
    "chmod 777 ",
    "chmod -r 777 ",
    "sudo ",
];

/// Paths that require explicit `security-audit-passed` label before promotion.
///
/// A skill is considered to touch a critical path if `critical_paths_touched`
/// contains any string that starts with one of these prefixes.
const CRITICAL_PATHS: &[&str] = &[
    "crates/garraia-auth/",
    "crates/garraia-security/",
    "crates/garraia-workspace/migrations/",
    ".github/workflows/",
    ".github/codeql-config.yml",
    "deny.toml",
    ".gitleaksignore",
    "Cargo.lock",
];

/// Caller-provided context for a safety-gate evaluation.
///
/// Labels represent explicit human approval signals (e.g. PR labels). The
/// `security-audit-passed` label is the **only** mechanism that may waive a
/// `CriticalPath` denial; it does NOT waive `DangerousCommand`, `ScoreTooLow`,
/// `AntiFlapDeprecated`, or `PiiDetected` — those remain hard walls.
#[derive(Debug, Default, Clone)]
pub struct SafetyIntent {
    pub labels: Vec<String>,
}

impl SafetyIntent {
    /// Label that signals an explicit @security-auditor review passed.
    pub const SECURITY_AUDIT_PASSED: &'static str = "security-audit-passed";

    pub fn has_security_audit_passed(&self) -> bool {
        self.labels.iter().any(|l| l == Self::SECURITY_AUDIT_PASSED)
    }
}

/// Hard-wall Safety Gate (default intent — no label waivers).
///
/// Equivalent to [`gate_with_intent`] called with `SafetyIntent::default()`.
/// Use when no caller-side approval labels apply (e.g. miner self-test).
pub fn gate(skill: &Skill) -> Result<(), SafetyDenial> {
    gate_with_intent(skill, &SafetyIntent::default())
}

/// Hard-wall Safety Gate.
///
/// Called before ANY skill promotion (auto or manual). Returns `Ok(())` only
/// when all checks pass. The first failing check short-circuits — callers must
/// fix all issues to eventually promote.
///
/// `intent.labels` may contain `security-audit-passed` to waive the
/// `CriticalPath` check (and only that one). All other checks remain hard
/// walls regardless of labels (ADR 0010 §"no dev-mode bypass").
///
/// Checks (in order):
/// 1. Dangerous commands in body text.
/// 2. Critical paths in `critical_paths_touched` (waivable by audit label).
/// 3. Score below minimum threshold.
/// 4. Anti-flap: consecutive failure count.
/// 5. PII patterns in body text.
pub fn gate_with_intent(skill: &Skill, intent: &SafetyIntent) -> Result<(), SafetyDenial> {
    check_dangerous_commands(skill)?;
    if !intent.has_security_audit_passed() {
        check_critical_paths(skill)?;
    }
    check_score(skill)?;
    check_anti_flap(skill)?;
    check_pii(skill)?;
    Ok(())
}

fn check_dangerous_commands(skill: &Skill) -> Result<(), SafetyDenial> {
    let body_lower = skill.body.to_lowercase();
    for pattern in DANGEROUS_PATTERNS {
        if body_lower.contains(*pattern) {
            return Err(SafetyDenial::DangerousCommand((*pattern).to_string()));
        }
    }
    Ok(())
}

fn check_critical_paths(skill: &Skill) -> Result<(), SafetyDenial> {
    for touched in &skill.frontmatter.critical_paths_touched {
        for critical in CRITICAL_PATHS {
            if touched.starts_with(critical) || touched == critical {
                return Err(SafetyDenial::CriticalPath(touched.clone()));
            }
        }
    }
    Ok(())
}

fn check_score(skill: &Skill) -> Result<(), SafetyDenial> {
    let score = skill.frontmatter.score;
    let minimum = Skill::MIN_PROMOTE_SCORE;
    if score < minimum {
        return Err(SafetyDenial::ScoreTooLow { score, minimum });
    }
    Ok(())
}

fn check_anti_flap(skill: &Skill) -> Result<(), SafetyDenial> {
    let fail_count = skill.frontmatter.fail_count;
    if fail_count >= Skill::ANTI_FLAP_THRESHOLD {
        return Err(SafetyDenial::AntiFlapDeprecated { fail_count });
    }
    Ok(())
}

/// Scan for PII patterns without external regex dependencies.
///
/// Detects:
/// - Email-shaped strings (contains `@` surrounded by non-whitespace and a `.`).
/// - Long alphanumeric tokens (≥32 consecutive alnum chars — API key shaped).
/// - Absolute home paths (`/home/<something>/` or `/Users/<something>/`).
fn check_pii(skill: &Skill) -> Result<(), SafetyDenial> {
    let body = &skill.body;

    if let Some(reason) = detect_email(body) {
        return Err(SafetyDenial::PiiDetected(reason));
    }
    if let Some(reason) = detect_long_token(body) {
        return Err(SafetyDenial::PiiDetected(reason));
    }
    if let Some(reason) = detect_home_path(body) {
        return Err(SafetyDenial::PiiDetected(reason));
    }

    Ok(())
}

fn detect_email(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        if let Some(at_pos) = word.find('@') {
            let local = &word[..at_pos];
            let domain = &word[at_pos + 1..];
            if !local.is_empty() && domain.contains('.') && !domain.starts_with('.') {
                return Some("email-shaped token near '@'".to_string());
            }
        }
    }
    None
}

fn detect_long_token(text: &str) -> Option<String> {
    let mut run = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            run += 1;
            if run >= 32 {
                return Some("long alphanumeric token (≥32 chars) detected".to_string());
            }
        } else {
            run = 0;
        }
    }
    None
}

fn detect_home_path(text: &str) -> Option<String> {
    for prefix in &["/home/", "/Users/"] {
        if let Some(pos) = text.find(prefix) {
            let after = &text[pos + prefix.len()..];
            let segment_end = after.find('/').unwrap_or(after.len());
            let username = &after[..segment_end];
            if !username.is_empty() {
                return Some(format!("absolute home path detected ({prefix}{username}/)"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LearningSkillFrontmatter, Skill, SkillScope, SkillSource};

    fn make_skill(body: &str) -> Skill {
        Skill {
            frontmatter: LearningSkillFrontmatter {
                name: "test-skill".to_string(),
                version: "0.1.0".to_string(),
                source: SkillSource::Mined,
                scope: SkillScope::Project,
                score: 0.8,
                locked: false,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            },
            body: body.to_string(),
            source_path: None,
        }
    }

    #[test]
    fn clean_skill_passes() {
        let skill = make_skill("# Cleanup\n1. git fetch --all --prune\n2. list merged branches");
        assert!(gate(&skill).is_ok());
    }

    #[test]
    fn rm_rf_root_blocked() {
        let skill = make_skill("rm -rf / # never do this");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
        assert!(err.to_string().contains("rm -rf /"));
    }

    #[test]
    fn rm_rf_tilde_blocked() {
        let skill = make_skill("Run: rm -rf ~ to clean up");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn drop_table_blocked() {
        let skill = make_skill("exec(':DROP TABLE users')");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn drop_table_case_insensitive() {
        let skill = make_skill("DROP TABLE sessions;");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn force_push_main_blocked() {
        let skill = make_skill("git push --force origin main");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn critical_path_auth_blocked() {
        let mut skill = make_skill("Modifies auth");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
        assert!(err.to_string().contains("garraia-auth"));
    }

    #[test]
    fn critical_path_migrations_blocked() {
        let mut skill = make_skill("Runs migration");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-workspace/migrations/015_foo.sql".to_string()];
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
    }

    #[test]
    fn critical_path_workflow_blocked() {
        let mut skill = make_skill("Edits CI");
        skill.frontmatter.critical_paths_touched = vec![".github/workflows/ci.yml".to_string()];
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
    }

    #[test]
    fn score_too_low_blocked() {
        let mut skill = make_skill("Good skill content that passes other checks.");
        skill.frontmatter.score = 0.3;
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::ScoreTooLow { .. }));
        assert!(err.to_string().contains("0.30"));
        assert!(err.to_string().contains("0.50"));
    }

    #[test]
    fn score_exactly_threshold_passes() {
        let skill = make_skill("Good skill that passes all checks.");
        // default score = 0.8 which is > 0.5; test with exactly 0.5
        let mut skill_exact = skill.clone();
        skill_exact.frontmatter.score = 0.5;
        assert!(gate(&skill_exact).is_ok());
    }

    #[test]
    fn anti_flap_threshold_blocked() {
        let mut skill = make_skill("Skill that keeps failing");
        skill.frontmatter.fail_count = 3;
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::AntiFlapDeprecated { .. }));
    }

    #[test]
    fn anti_flap_below_threshold_passes() {
        let mut skill = make_skill("Skill with some failures but below threshold.");
        skill.frontmatter.fail_count = 2;
        assert!(gate(&skill).is_ok());
    }

    #[test]
    fn pii_email_blocked() {
        let skill = make_skill("Contact user@example.com for support");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::PiiDetected(_)));
    }

    #[test]
    fn pii_long_token_blocked() {
        let token = "a".repeat(32);
        let skill = make_skill(&format!("API_KEY={token}"));
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::PiiDetected(_)));
    }

    #[test]
    fn pii_home_path_blocked() {
        let skill = make_skill("config is at /home/michelbr/config.toml");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::PiiDetected(_)));
    }

    #[test]
    fn dangerous_command_short_circuits_before_score() {
        let mut skill = make_skill("rm -rf /");
        skill.frontmatter.score = 0.1; // would also fail ScoreTooLow
        let err = gate(&skill).unwrap_err();
        // DangerousCommand should fire first
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    // ── GAR-649 RED tests: new denylist patterns ─────────────────────────

    #[test]
    fn delete_from_where_1_eq_1_blocked() {
        let skill = make_skill("DELETE FROM users WHERE 1=1;");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn chmod_dash_r_777_any_path_blocked() {
        let skill = make_skill("chmod -R 777 /var/data");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn chmod_777_any_path_blocked() {
        let skill = make_skill("chmod 777 ~/.ssh");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn sudo_blocked() {
        let skill = make_skill("Run: sudo rm package.deb");
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn codeql_config_critical_path_blocked() {
        let mut skill = make_skill("Tweaks CodeQL");
        skill.frontmatter.critical_paths_touched = vec![".github/codeql-config.yml".to_string()];
        let err = gate(&skill).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
    }

    // ── GAR-649 RED tests: SafetyIntent waiver semantics ─────────────────

    #[test]
    fn intent_default_blocks_critical_path() {
        let mut skill = make_skill("Modifies auth");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let intent = SafetyIntent::default();
        let err = gate_with_intent(&skill, &intent).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
    }

    #[test]
    fn intent_security_audit_label_waives_critical_path() {
        let mut skill = make_skill("Modifies auth, reviewed by @security-auditor");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let intent = SafetyIntent {
            labels: vec!["security-audit-passed".to_string()],
        };
        // Label waives ONLY CriticalPath — rest of gate still runs
        assert!(gate_with_intent(&skill, &intent).is_ok());
    }

    #[test]
    fn intent_label_does_not_waive_dangerous_command() {
        let mut skill = make_skill("rm -rf /");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let intent = SafetyIntent {
            labels: vec!["security-audit-passed".to_string()],
        };
        let err = gate_with_intent(&skill, &intent).unwrap_err();
        // Hard wall: dangerous command short-circuits before critical-path check.
        assert!(matches!(err, SafetyDenial::DangerousCommand(_)));
    }

    #[test]
    fn intent_label_does_not_waive_score_threshold() {
        let mut skill = make_skill("Good content reviewed by security.");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        skill.frontmatter.score = 0.3;
        let intent = SafetyIntent {
            labels: vec!["security-audit-passed".to_string()],
        };
        let err = gate_with_intent(&skill, &intent).unwrap_err();
        assert!(matches!(err, SafetyDenial::ScoreTooLow { .. }));
    }

    #[test]
    fn intent_label_does_not_waive_pii() {
        let mut skill = make_skill("Contact user@example.com (reviewed by security).");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let intent = SafetyIntent {
            labels: vec!["security-audit-passed".to_string()],
        };
        let err = gate_with_intent(&skill, &intent).unwrap_err();
        assert!(matches!(err, SafetyDenial::PiiDetected(_)));
    }

    #[test]
    fn intent_dev_mode_label_does_not_waive_critical_path() {
        // Per ADR 0010: no "dev mode" bypass. Only the explicit
        // security-audit-passed label waives CriticalPath.
        let mut skill = make_skill("Modifies auth");
        skill.frontmatter.critical_paths_touched =
            vec!["crates/garraia-auth/src/lib.rs".to_string()];
        let intent = SafetyIntent {
            labels: vec!["dev-mode".to_string(), "skip-safety".to_string()],
        };
        let err = gate_with_intent(&skill, &intent).unwrap_err();
        assert!(matches!(err, SafetyDenial::CriticalPath(_)));
    }
}
