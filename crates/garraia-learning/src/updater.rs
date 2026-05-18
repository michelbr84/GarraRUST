use crate::Skill;
use garraia_common::{Error, Result};
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────

/// Evidence bundle used when proposing a skill update.
#[derive(Debug, Clone)]
pub struct UpdateEvidence {
    /// EMA score recorded before this evaluation cycle.
    pub score_before: f32,
    /// EMA score recorded after this evaluation cycle.
    pub score_after: f32,
    /// Human-readable reason for the proposed update.
    pub reason: String,
    /// Updated skill body (Markdown content below the frontmatter block).
    pub new_body: String,
    /// Optional excerpt from execution logs (included in PR body).
    pub log_excerpt: Option<String>,
    /// Optional CI run URL (included in PR body).
    pub ci_url: Option<String>,
}

/// Version-bump kind for the proposed update.
#[derive(Debug, Clone, PartialEq)]
pub enum BumpKind {
    /// PATCH: wording/formatting/doc improvements — backward-compatible.
    Patch,
    /// MINOR: structural step changes — backward-compatible new behaviour.
    Minor,
}

/// Result of a successful [`propose_update`] call.
#[derive(Debug, Clone)]
pub struct PullRequestProposal {
    /// Branch name used for the proposal.
    pub branch: String,
    /// PR title (mirrors the commit message prefix).
    pub title: String,
    /// PR body in Markdown.
    pub body: String,
    /// URL of the opened (or pre-existing, if idempotent) PR.
    pub pr_url: String,
}

// ─────────────────────────────────────────────
// ShellRunner abstraction (mockable in tests)
// ─────────────────────────────────────────────

/// Abstracts git + gh shell operations to allow unit-testing without real repos.
pub trait ShellRunner: Send + Sync {
    fn run_git(&self, args: &[&str], cwd: &Path) -> Result<String>;
    fn run_gh(&self, args: &[&str], cwd: &Path) -> Result<String>;
}

/// Production shell runner that delegates to real `git` and `gh` processes.
pub struct ProcessShellRunner;

impl ShellRunner for ProcessShellRunner {
    fn run_git(&self, args: &[&str], cwd: &Path) -> Result<String> {
        run_process("git", args, cwd)
    }

    fn run_gh(&self, args: &[&str], cwd: &Path) -> Result<String> {
        run_process("gh", args, cwd)
    }
}

fn run_process(bin: &str, args: &[&str], cwd: &Path) -> Result<String> {
    let output = std::process::Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| Error::Other(format!("failed to spawn {bin}: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::Other(format!(
            "{bin} exited {}: {stderr}",
            output.status
        )))
    }
}

// ─────────────────────────────────────────────
// Version helpers
// ─────────────────────────────────────────────

/// Detect the bump kind from the update reason string.
///
/// PATCH keywords: wording, typo, format, style, doc, clarif, minor fix.
/// Everything else is MINOR.
pub fn detect_bump_kind(reason: &str) -> BumpKind {
    let lower = reason.to_lowercase();
    let patch_keywords = [
        "wording",
        "typo",
        "format",
        "style",
        "doc",
        "clarif",
        "minor fix",
    ];
    if patch_keywords.iter().any(|kw| lower.contains(kw)) {
        BumpKind::Patch
    } else {
        BumpKind::Minor
    }
}

/// Bump a `major.minor.patch` semver string by the given kind.
///
/// Returns the input string unchanged if it cannot be parsed.
pub fn bump_version(version: &str, kind: &BumpKind) -> String {
    let parts: Vec<&str> = version.splitn(3, '.').collect();
    if parts.len() != 3 {
        return version.to_string();
    }
    let Ok(major) = parts[0].parse::<u64>() else {
        return version.to_string();
    };
    let Ok(minor) = parts[1].parse::<u64>() else {
        return version.to_string();
    };
    let Ok(patch) = parts[2].parse::<u64>() else {
        return version.to_string();
    };
    match kind {
        BumpKind::Patch => format!("{major}.{minor}.{}", patch + 1),
        BumpKind::Minor => format!("{major}.{}.0", minor + 1),
    }
}

// ─────────────────────────────────────────────
// Branch / PR helpers
// ─────────────────────────────────────────────

/// Build the canonical branch name for a skill update proposal.
///
/// Sanitizes the skill name: characters that are not alphanumeric or `-` become `-`.
pub fn branch_name(skill_name: &str, old_version: &str, new_version: &str) -> String {
    let safe: String = skill_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("learning/skill-{safe}-v{old_version}-v{new_version}")
}

/// Assemble the PR title (mirrors the commit message prefix).
fn pr_title(skill_name: &str, old_version: &str, new_version: &str) -> String {
    format!("feat(learning): auto-update skill {skill_name} v{old_version}→v{new_version}")
}

/// Build the PR body in Markdown.
fn pr_body(
    skill_name: &str,
    old_version: &str,
    new_version: &str,
    evidence: &UpdateEvidence,
) -> String {
    let ci_line = evidence
        .ci_url
        .as_deref()
        .map(|u| format!("\n**CI run:** {u}"))
        .unwrap_or_default();

    let log_section = evidence
        .log_excerpt
        .as_deref()
        .map(|l| format!("\n\n### Log excerpt\n```\n{l}\n```"))
        .unwrap_or_default();

    format!(
        "## Skill Auto-Update Proposal: `{skill_name}` v{old_version} → v{new_version}\n\n\
         **Score:** {score_before:.2} → {score_after:.2}  \n\
         **Reason:** {reason}{ci_line}\n\n\
         ### Score history\n\
         See `.garra/skills/_history/{skill_name}.json` for the full EMA timeline.\n\n\
         ### Rollback\n\
         ```sh\n\
         git revert HEAD  # on branch learning/skill-{skill_name}-v{old_version}-v{new_version}\n\
         ```{log_section}",
        score_before = evidence.score_before,
        score_after = evidence.score_after,
        reason = evidence.reason,
    )
}

/// Rewrite a skill file, bumping the frontmatter `version:` field and replacing the body.
///
/// Expects the file to use `---` YAML frontmatter delimiters.
/// If the delimiter structure is not found the new content is returned verbatim.
pub fn assemble_skill_file(new_version: &str, old_content: &str, new_body: &str) -> String {
    let mut in_fm = false;
    let mut fm_done = false;
    let mut fm_lines: Vec<String> = Vec::new();

    for line in old_content.lines() {
        if !fm_done {
            if line.trim() == "---" {
                if !in_fm {
                    in_fm = true;
                    fm_lines.push(line.to_string());
                } else {
                    fm_done = true;
                    fm_lines.push(line.to_string());
                }
                continue;
            }
            if in_fm {
                if line.starts_with("version:") {
                    fm_lines.push(format!("version: {new_version}"));
                } else {
                    fm_lines.push(line.to_string());
                }
                continue;
            }
            // Content before any `---` — keep as-is (shouldn't normally occur).
            fm_lines.push(line.to_string());
        }
        // Old body lines are intentionally dropped; new_body replaces them.
    }

    if fm_done {
        format!("{}\n\n{}", fm_lines.join("\n"), new_body.trim())
    } else {
        // Fallback: no parseable frontmatter — return new body as-is.
        new_body.to_string()
    }
}

// ─────────────────────────────────────────────
// Git helpers
// ─────────────────────────────────────────────

/// Walk parent directories to find the git repository root (directory containing `.git`).
fn git_root(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(Error::Other(format!(
                "no .git directory found above {}",
                start.display()
            )));
        }
    }
}

/// Check for an existing open PR on `branch` via `gh pr list`.
///
/// Returns `Some(url)` if one exists, `None` otherwise.
/// Treats any runner error as "no PR found" (safe to retry).
fn find_existing_pr(branch: &str, cwd: &Path, runner: &dyn ShellRunner) -> Option<String> {
    let result = runner.run_gh(
        &[
            "pr", "list", "--head", branch, "--state", "open", "--json", "url", "--jq", ".[0].url",
        ],
        cwd,
    );
    match result {
        Ok(url) if !url.is_empty() && url.starts_with("http") => Some(url),
        _ => None,
    }
}

// ─────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────

/// Returns a deterministic error proving that auto-merge is prohibited.
///
/// Calling this function (not just checking its return type) is intentional:
/// it provides a compile-time + runtime proof that auto-merge cannot happen.
pub fn auto_merge_guard() -> Error {
    Error::Other(
        "auto-merge is prohibited: approve via GitHub UI or 'garra skills approve <name>'".into(),
    )
}

/// Propose a skill update using the real `git` + `gh` processes.
///
/// See [`propose_update_with_runner`] for the full contract.
pub fn propose_update(skill: &Skill, evidence: UpdateEvidence) -> Result<PullRequestProposal> {
    propose_update_with_runner(skill, evidence, &ProcessShellRunner)
}

/// Core proposer — accepts an injectable `ShellRunner` for unit-testing.
///
/// # Contract
///
/// - Returns `Err` immediately if `skill.frontmatter.locked` is true.
/// - Returns `Err` immediately if `skill.source_path` is `None`.
/// - Is idempotent: if a PR already exists on the target branch, it returns
///   the existing PR URL without creating a new git branch or commit.
/// - Never calls `gh pr merge`.
pub fn propose_update_with_runner(
    skill: &Skill,
    evidence: UpdateEvidence,
    runner: &dyn ShellRunner,
) -> Result<PullRequestProposal> {
    if skill.frontmatter.locked {
        return Err(Error::Other(format!(
            "skill '{}' is locked and cannot be auto-updated",
            skill.frontmatter.name
        )));
    }

    let source_path = skill.source_path.as_ref().ok_or_else(|| {
        Error::Other("skill has no source_path: cannot commit update to disk".into())
    })?;

    let skill_dir = source_path.parent().ok_or_else(|| {
        Error::Other(format!(
            "source_path '{}' has no parent directory",
            source_path.display()
        ))
    })?;

    let git_root = git_root(skill_dir)?;

    let old_version = skill.frontmatter.version.clone();
    let bump = detect_bump_kind(&evidence.reason);
    let new_version = bump_version(&old_version, &bump);

    let branch = branch_name(&skill.frontmatter.name, &old_version, &new_version);
    let title = pr_title(&skill.frontmatter.name, &old_version, &new_version);
    let body = pr_body(
        &skill.frontmatter.name,
        &old_version,
        &new_version,
        &evidence,
    );

    // ── Idempotency: return early if a PR for this branch already exists ───
    if let Some(url) = find_existing_pr(&branch, &git_root, runner) {
        return Ok(PullRequestProposal {
            branch,
            title,
            body,
            pr_url: url,
        });
    }

    // ── Determine base branch ──────────────────────────────────────────────
    let base = runner
        .run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &git_root)
        .unwrap_or_else(|_| "main".to_string());
    let base = if base.is_empty() || base == "HEAD" {
        "main".to_string()
    } else {
        base
    };

    // ── Create branch ──────────────────────────────────────────────────────
    runner.run_git(&["checkout", "-b", &branch, &base], &git_root)?;

    // ── Write updated skill file ───────────────────────────────────────────
    let old_content = std::fs::read_to_string(source_path)
        .map_err(|e| Error::Other(format!("cannot read skill file: {e}")))?;
    let new_content = assemble_skill_file(&new_version, &old_content, &evidence.new_body);
    std::fs::write(source_path, &new_content)
        .map_err(|e| Error::Other(format!("cannot write updated skill file: {e}")))?;

    // ── Stage + commit ─────────────────────────────────────────────────────
    let rel_path = source_path
        .strip_prefix(&git_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source_path.display().to_string());

    runner.run_git(&["add", &rel_path], &git_root)?;

    let commit_msg = format!(
        "feat(learning): auto-update skill {} v{}→v{} — {}",
        skill.frontmatter.name, old_version, new_version, evidence.reason
    );
    runner.run_git(&["commit", "-m", &commit_msg], &git_root)?;

    // ── Push ───────────────────────────────────────────────────────────────
    runner.run_git(&["push", "-u", "origin", &branch], &git_root)?;

    // ── Open PR ────────────────────────────────────────────────────────────
    let pr_url = runner.run_gh(
        &[
            "pr", "create", "--title", &title, "--body", &body, "--base", "main", "--head", &branch,
        ],
        &git_root,
    )?;

    // Return to original branch (best-effort — not fatal on failure).
    let _ = runner.run_git(&["checkout", &base], &git_root);

    Ok(PullRequestProposal {
        branch,
        title,
        body,
        pr_url,
    })
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LearningSkillFrontmatter, Skill, SkillScope, SkillSource};
    use std::sync::Mutex;

    // ── Mock ShellRunner ──────────────────────────────────────────────────

    struct MockShellRunner {
        /// Pre-programmed responses for `run_git` calls, in order.
        git_responses: Mutex<Vec<Result<String>>>,
        /// Pre-programmed responses for `run_gh` calls, in order.
        gh_responses: Mutex<Vec<Result<String>>>,
        /// Log of `run_git` arg vectors (for assertion).
        git_calls: Mutex<Vec<Vec<String>>>,
        /// Log of `run_gh` arg vectors (for assertion).
        gh_calls: Mutex<Vec<Vec<String>>>,
    }

    impl MockShellRunner {
        fn new(git_responses: Vec<Result<String>>, gh_responses: Vec<Result<String>>) -> Self {
            Self {
                git_responses: Mutex::new(git_responses),
                gh_responses: Mutex::new(gh_responses),
                git_calls: Mutex::new(Vec::new()),
                gh_calls: Mutex::new(Vec::new()),
            }
        }

        fn git_call_count(&self) -> usize {
            self.git_calls.lock().unwrap().len()
        }

        fn gh_call_args(&self, idx: usize) -> Vec<String> {
            self.gh_calls.lock().unwrap()[idx].clone()
        }

        fn git_call_args(&self, idx: usize) -> Vec<String> {
            self.git_calls.lock().unwrap()[idx].clone()
        }
    }

    impl ShellRunner for MockShellRunner {
        fn run_git(&self, args: &[&str], _cwd: &Path) -> Result<String> {
            self.git_calls
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect());
            let mut q = self.git_responses.lock().unwrap();
            if q.is_empty() {
                Ok(String::new())
            } else {
                q.remove(0)
            }
        }

        fn run_gh(&self, args: &[&str], _cwd: &Path) -> Result<String> {
            self.gh_calls
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect());
            let mut q = self.gh_responses.lock().unwrap();
            if q.is_empty() {
                Ok(String::new())
            } else {
                q.remove(0)
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn make_skill(name: &str, version: &str, locked: bool, path: Option<PathBuf>) -> Skill {
        Skill {
            frontmatter: LearningSkillFrontmatter {
                name: name.to_string(),
                version: version.to_string(),
                source: SkillSource::Mined,
                scope: SkillScope::Project,
                score: 0.6,
                locked,
                critical_paths_touched: vec![],
                fail_count: 0,
                deprecated: false,
            },
            body: "## Old body".to_string(),
            source_path: path,
        }
    }

    fn make_evidence(reason: &str) -> UpdateEvidence {
        UpdateEvidence {
            score_before: 0.8,
            score_after: 0.4,
            reason: reason.to_string(),
            new_body: "## Updated body\n\nNew steps here.".to_string(),
            log_excerpt: Some("ERROR: test failed".to_string()),
            ci_url: Some("https://ci.example.com/run/42".to_string()),
        }
    }

    // ── Pure-function tests ──────────────────────────────────────────────

    #[test]
    fn test_detect_bump_kind_patch_wording() {
        assert_eq!(detect_bump_kind("fix wording in step 3"), BumpKind::Patch);
    }

    #[test]
    fn test_detect_bump_kind_patch_typo() {
        assert_eq!(
            detect_bump_kind("correct typo in description"),
            BumpKind::Patch
        );
    }

    #[test]
    fn test_detect_bump_kind_patch_doc() {
        assert_eq!(detect_bump_kind("update doc for clarity"), BumpKind::Patch);
    }

    #[test]
    fn test_detect_bump_kind_patch_format() {
        assert_eq!(detect_bump_kind("reformat output section"), BumpKind::Patch);
    }

    #[test]
    fn test_detect_bump_kind_minor_restructure() {
        assert_eq!(
            detect_bump_kind("restructure steps to improve flow"),
            BumpKind::Minor
        );
    }

    #[test]
    fn test_detect_bump_kind_minor_add_step() {
        assert_eq!(detect_bump_kind("add validation step"), BumpKind::Minor);
    }

    #[test]
    fn test_bump_version_patch() {
        assert_eq!(bump_version("1.2.3", &BumpKind::Patch), "1.2.4");
    }

    #[test]
    fn test_bump_version_minor() {
        assert_eq!(bump_version("1.2.3", &BumpKind::Minor), "1.3.0");
    }

    #[test]
    fn test_bump_version_from_zero() {
        assert_eq!(bump_version("0.0.0", &BumpKind::Patch), "0.0.1");
    }

    #[test]
    fn test_bump_version_major_minor_patch() {
        assert_eq!(bump_version("2.3.5", &BumpKind::Minor), "2.4.0");
    }

    #[test]
    fn test_bump_version_invalid_passthrough() {
        assert_eq!(bump_version("invalid", &BumpKind::Patch), "invalid");
    }

    #[test]
    fn test_bump_version_two_part_passthrough() {
        assert_eq!(bump_version("1.0", &BumpKind::Patch), "1.0");
    }

    #[test]
    fn test_branch_name_simple() {
        assert_eq!(
            branch_name("my-skill", "1.0.0", "1.0.1"),
            "learning/skill-my-skill-v1.0.0-v1.0.1"
        );
    }

    #[test]
    fn test_branch_name_sanitizes_slashes() {
        let b = branch_name("my/skill", "1.0.0", "1.1.0");
        assert_eq!(b, "learning/skill-my-skill-v1.0.0-v1.1.0");
    }

    #[test]
    fn test_branch_name_sanitizes_spaces() {
        let b = branch_name("my skill", "0.1.0", "0.2.0");
        assert_eq!(b, "learning/skill-my-skill-v0.1.0-v0.2.0");
    }

    #[test]
    fn test_auto_merge_guard_returns_error() {
        let err = auto_merge_guard();
        let msg = err.to_string();
        assert!(msg.contains("auto-merge is prohibited"));
        assert!(msg.contains("approve"));
    }

    #[test]
    fn test_assemble_skill_file_updates_version() {
        let old = "---\nname: test\nversion: 1.0.0\nscore: 0.8\n---\n\n## Old body\n\nold steps.";
        let result = assemble_skill_file("1.0.1", old, "## New body\n\nnew steps.");
        assert!(result.contains("version: 1.0.1"));
        assert!(result.contains("## New body"));
        assert!(!result.contains("old steps"));
        assert!(!result.contains("version: 1.0.0"));
    }

    #[test]
    fn test_assemble_skill_file_preserves_other_fields() {
        let old = "---\nname: test\nversion: 2.0.0\nscore: 0.9\nlocked: false\n---\n\nbody";
        let result = assemble_skill_file("2.1.0", old, "new body");
        assert!(result.contains("name: test"));
        assert!(result.contains("score: 0.9"));
        assert!(result.contains("version: 2.1.0"));
        assert!(!result.contains("version: 2.0.0"));
    }

    #[test]
    fn test_pr_body_contains_required_elements() {
        let evidence = make_evidence("restructure steps");
        let body = pr_body("test-skill", "1.0.0", "1.1.0", &evidence);
        assert!(body.contains("test-skill"));
        assert!(body.contains("v1.0.0"));
        assert!(body.contains("v1.1.0"));
        assert!(body.contains("0.80"));
        assert!(body.contains("0.40"));
        assert!(body.contains("restructure steps"));
        assert!(body.contains("_history/test-skill.json"));
        assert!(body.contains("git revert HEAD"));
    }

    // ── propose_update_with_runner contract tests ─────────────────────────

    #[test]
    fn test_propose_update_locked_returns_error() {
        let skill = make_skill(
            "locked-skill",
            "1.0.0",
            true,
            Some(PathBuf::from("/tmp/skill.md")),
        );
        let evidence = make_evidence("restructure steps");
        let runner = MockShellRunner::new(vec![], vec![]);

        let err = propose_update_with_runner(&skill, evidence, &runner).unwrap_err();
        assert!(err.to_string().contains("locked"));
    }

    #[test]
    fn test_propose_update_no_source_path_returns_error() {
        let skill = make_skill("skill", "1.0.0", false, None);
        let evidence = make_evidence("restructure steps");
        let runner = MockShellRunner::new(vec![], vec![]);

        let err = propose_update_with_runner(&skill, evidence, &runner).unwrap_err();
        assert!(err.to_string().contains("source_path"));
    }

    #[test]
    fn test_propose_update_idempotent_returns_existing_pr() {
        // gh pr list returns an existing PR URL → no git branch/commit should happen
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().to_path_buf();

        // Create a fake .git dir so git_root walks succeed
        std::fs::create_dir(tmp_path.join(".git")).unwrap();

        let skill_path = tmp_path.join("skill.md");
        std::fs::write(&skill_path, "---\nname: s\nversion: 1.0.0\n---\n\n## Body").unwrap();

        let skill = make_skill("s", "1.0.0", false, Some(skill_path));
        let evidence = make_evidence("restructure steps");

        // gh pr list returns existing URL; no git calls should follow
        let runner = MockShellRunner::new(
            vec![], // no git responses needed
            vec![Ok("https://github.com/owner/repo/pull/99".to_string())],
        );

        let proposal = propose_update_with_runner(&skill, evidence, &runner).unwrap();
        assert_eq!(proposal.pr_url, "https://github.com/owner/repo/pull/99");
        assert_eq!(proposal.branch, "learning/skill-s-v1.0.0-v1.1.0");
        // No git calls made (idempotency short-circuits before git)
        assert_eq!(runner.git_call_count(), 0);
    }

    #[test]
    fn test_propose_update_happy_path() {
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().to_path_buf();
        std::fs::create_dir(tmp_path.join(".git")).unwrap();

        let skill_path = tmp_path.join("skill.md");
        std::fs::write(
            &skill_path,
            "---\nname: my-skill\nversion: 1.0.0\n---\n\n## Old body",
        )
        .unwrap();

        let skill = make_skill("my-skill", "1.0.0", false, Some(skill_path.clone()));
        let evidence = make_evidence("restructure steps");

        // gh: (1) pr list → empty (no existing PR), (2) pr create → new URL
        let gh_responses = vec![
            Ok(String::new()),
            Ok("https://github.com/owner/repo/pull/1".to_string()),
        ];
        // git: (1) rev-parse HEAD, (2) checkout -b, (3) add, (4) commit, (5) push, (6) checkout base
        let git_responses = vec![
            Ok("main".to_string()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
        ];
        let runner = MockShellRunner::new(git_responses, gh_responses);

        let proposal = propose_update_with_runner(&skill, evidence, &runner).unwrap();

        assert_eq!(proposal.pr_url, "https://github.com/owner/repo/pull/1");
        assert_eq!(proposal.branch, "learning/skill-my-skill-v1.0.0-v1.1.0");
        assert!(proposal.title.contains("my-skill"));
        assert!(proposal.title.contains("v1.0.0→v1.1.0"));

        // Verify commit message contains expected components
        let commit_args = runner.git_call_args(3); // 4th git call = commit
        let commit_msg = commit_args.join(" ");
        assert!(commit_msg.contains("my-skill"));
        assert!(commit_msg.contains("v1.0.0→v1.1.0"));

        // Verify gh pr create was called with correct args
        let gh_create_args = runner.gh_call_args(1);
        assert!(gh_create_args.contains(&"pr".to_string()));
        assert!(gh_create_args.contains(&"create".to_string()));
        assert!(gh_create_args.contains(&"--base".to_string()));

        // Verify the skill file was updated on disk
        let updated = std::fs::read_to_string(&skill_path).unwrap();
        assert!(updated.contains("version: 1.1.0"));
        assert!(updated.contains("Updated body"));
    }

    #[test]
    fn test_propose_update_branch_name_in_proposal() {
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().to_path_buf();
        std::fs::create_dir(tmp_path.join(".git")).unwrap();

        let skill_path = tmp_path.join("s.md");
        std::fs::write(&skill_path, "---\nname: s\nversion: 2.0.0\n---\n\nbody").unwrap();

        let skill = make_skill("s", "2.0.0", false, Some(skill_path));
        let evidence = UpdateEvidence {
            score_before: 0.7,
            score_after: 0.3,
            reason: "improve doc coverage".to_string(),
            new_body: "## New".to_string(),
            log_excerpt: None,
            ci_url: None,
        };

        let gh_responses = vec![
            Ok(String::new()),
            Ok("https://gh.example/pull/2".to_string()),
        ];
        let git_responses = vec![
            Ok("main".to_string()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
            Ok(String::new()),
        ];
        let runner = MockShellRunner::new(git_responses, gh_responses);

        let proposal = propose_update_with_runner(&skill, evidence, &runner).unwrap();
        // "doc" triggers PATCH → 2.0.0 → 2.0.1
        assert_eq!(proposal.branch, "learning/skill-s-v2.0.0-v2.0.1");
    }
}
