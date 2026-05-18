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
    "git push --force origin main",
    "git push --force-with-lease origin main",
    "git push -f origin main",
    "git push origin main --force",
    ":(){ :|:& };:",
    "mkfs.",
    "dd if=/dev/zero",
    "dd if=/dev/urandom",
    "chmod -r 777 /",
    "chmod 777 /",
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
    "deny.toml",
    ".gitleaksignore",
    "Cargo.lock",
];

/// Hard-wall Safety Gate.
///
/// Called before ANY skill promotion (auto or manual). Returns `Ok(())` only
/// when all checks pass. The first failing check short-circuits — callers must
/// fix all issues to eventually promote.
///
/// Checks (in order):
/// 1. Dangerous commands in body text.
/// 2. Critical paths in `critical_paths_touched`.
/// 3. Score below minimum threshold.
/// 4. Anti-flap: consecutive failure count.
/// 5. PII patterns in body text.
pub fn gate(skill: &Skill) -> Result<(), SafetyDenial> {
    check_dangerous_commands(skill)?;
    check_critical_paths(skill)?;
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
}
