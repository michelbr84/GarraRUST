//! Safe git/gh repo workflow operations for GarraMaxPower (GAR-496).
//!
//! Wraps `git` and `gh` CLI calls with pre-flight safety checks:
//! - Refuses to push to protected branches (`main`, `master`, `release/*`).
//! - Requires a clean working tree before creating feature branches.
//! - Reports current branch and tree status for operator awareness.
//!
//! The [`GitRunner`] trait allows injecting a mock in unit tests so no real
//! git process is spawned.

use std::path::{Path, PathBuf};

/// Branch patterns that must never receive direct pushes or force operations.
const PROTECTED_PATTERNS: &[&str] = &["main", "master", "release/"];

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    /// The target branch is protected; direct push is refused.
    ProtectedBranch { branch: String },
    /// The working tree has uncommitted changes.
    DirtyWorkingTree { summary: String },
    /// A git or gh command exited non-zero.
    CommandFailed { cmd: String, stderr: String },
    /// Output from a command could not be parsed.
    ParseError(String),
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProtectedBranch { branch } => write!(
                f,
                "branch '{branch}' is protected — create a feature branch first"
            ),
            Self::DirtyWorkingTree { summary } => {
                write!(f, "working tree is not clean:\n{summary}")
            }
            Self::CommandFailed { cmd, stderr } => {
                write!(f, "command '{cmd}' failed: {stderr}")
            }
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for WorkflowError {}

// ── GitRunner trait ──────────────────────────────────────────────────────────

/// Abstraction over git/gh command execution, enabling in-process mocking in tests.
pub trait GitRunner: Send + Sync {
    /// Run a command in `root` (first element of `args` is the program).
    /// Returns trimmed stdout on success, or `WorkflowError::CommandFailed` on
    /// non-zero exit.
    fn run(&self, root: &Path, args: &[&str]) -> Result<String, WorkflowError>;
}

// ── Production runner ────────────────────────────────────────────────────────

/// Delegates to [`std::process::Command`].  No shell involved; args are passed
/// as separate strings, preventing injection.
pub struct ProcessRunner;

impl GitRunner for ProcessRunner {
    fn run(&self, root: &Path, args: &[&str]) -> Result<String, WorkflowError> {
        let (program, rest) = args
            .split_first()
            .ok_or_else(|| WorkflowError::ParseError("empty args slice".into()))?;
        let out = std::process::Command::new(program)
            .args(rest)
            .current_dir(root)
            .output()
            .map_err(|e| WorkflowError::CommandFailed {
                cmd: args.join(" "),
                stderr: e.to_string(),
            })?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(WorkflowError::CommandFailed {
                cmd: args.join(" "),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            })
        }
    }
}

// ── Safety helpers ───────────────────────────────────────────────────────────

/// Returns `true` when `branch` must not receive direct pushes.
///
/// Protected: `main`, `master`, and any branch whose name starts with `release/`.
pub fn is_protected_branch(branch: &str) -> bool {
    PROTECTED_PATTERNS
        .iter()
        .any(|pat| branch == *pat || branch.starts_with(pat))
}

// ── RepoWorkflow ─────────────────────────────────────────────────────────────

/// Encapsulates safe git/gh operations for the GarraMaxPower pipeline.
///
/// Use [`RepoWorkflow::new`] for production code and
/// [`RepoWorkflow::with_runner`] to inject a [`MockRunner`] in tests.
pub struct RepoWorkflow<R: GitRunner = ProcessRunner> {
    root: PathBuf,
    runner: R,
}

impl RepoWorkflow<ProcessRunner> {
    /// Create a workflow rooted at `root` using the real process runner.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            runner: ProcessRunner,
        }
    }
}

// Methods below are the forward-facing API wired by GAR-498/499 (Skills MVP /
// Agent team MVP). They are intentionally unused from production code today;
// `#[allow(dead_code)]` prevents -D warnings from treating them as dead while
// they are only exercised via `MockRunner` in the test suite.
#[allow(dead_code)]
impl<R: GitRunner> RepoWorkflow<R> {
    /// Create a workflow with an injected runner (for testing).
    pub fn with_runner(root: PathBuf, runner: R) -> Self {
        Self { root, runner }
    }

    /// Return the name of the current git branch (`HEAD`).
    pub fn current_branch(&self) -> Result<String, WorkflowError> {
        self.runner
            .run(&self.root, &["git", "rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Return `true` when the working tree has no uncommitted changes.
    pub fn is_clean(&self) -> Result<bool, WorkflowError> {
        let out = self
            .runner
            .run(&self.root, &["git", "status", "--porcelain"])?;
        Ok(out.is_empty())
    }

    /// Create a feature branch from the current `HEAD`.
    ///
    /// Pre-condition: the working tree must be clean.  Fails with
    /// [`WorkflowError::DirtyWorkingTree`] otherwise.
    ///
    /// Note: branching from `main` is intentionally allowed (common workflow);
    /// callers that want to warn the operator can call [`Self::current_branch`]
    /// + [`is_protected_branch`] separately.
    pub fn create_branch(&self, name: &str) -> Result<(), WorkflowError> {
        let status = self
            .runner
            .run(&self.root, &["git", "status", "--porcelain"])?;
        if !status.is_empty() {
            return Err(WorkflowError::DirtyWorkingTree { summary: status });
        }
        self.runner
            .run(&self.root, &["git", "checkout", "-b", name])?;
        Ok(())
    }

    /// Push `branch` to `origin` with `-u` (set upstream).
    ///
    /// Safety: refuses with [`WorkflowError::ProtectedBranch`] when `branch`
    /// matches any protected pattern.  This prevents accidental direct pushes to
    /// `main`, `master`, or `release/*`.
    pub fn push_branch(&self, branch: &str) -> Result<(), WorkflowError> {
        if is_protected_branch(branch) {
            return Err(WorkflowError::ProtectedBranch {
                branch: branch.to_string(),
            });
        }
        self.runner
            .run(&self.root, &["git", "push", "-u", "origin", branch])?;
        Ok(())
    }

    /// Open a pull request via `gh pr create`.
    ///
    /// Returns the PR URL on success.  `base` is the target branch (typically
    /// `main`).  Requires `gh` CLI to be authenticated.
    pub fn open_pr(&self, title: &str, body: &str, base: &str) -> Result<String, WorkflowError> {
        self.runner.run(
            &self.root,
            &[
                "gh", "pr", "create", "--title", title, "--body", body, "--base", base,
            ],
        )
    }
}

// ── Preflight summary ─────────────────────────────────────────────────────────

/// Collect current-branch and clean status for display in `garra max-power`.
///
/// Returns `None` silently when we are not inside a git repository (or `git`
/// is not in `PATH`), so the caller can skip the preflight block.
pub fn preflight_summary() -> Option<PreflightSummary> {
    let wf = RepoWorkflow::new(std::env::current_dir().ok()?);
    let branch = wf.current_branch().ok()?;
    let clean = wf.is_clean().ok()?;
    Some(PreflightSummary { branch, clean })
}

/// Result of a preflight git check.
pub struct PreflightSummary {
    pub branch: String,
    pub clean: bool,
}

impl PreflightSummary {
    /// One-liner for display in the pipeline menu / route output.
    pub fn display(&self) -> String {
        let branch_label = if is_protected_branch(&self.branch) {
            format!(
                "{} ⚠️  (protected — create a feature branch first)",
                self.branch
            )
        } else {
            format!("{} ✓", self.branch)
        };
        let clean_label = if self.clean {
            "clean ✓".to_string()
        } else {
            "dirty ⚠️  (commit or stash changes first)".to_string()
        };
        format!("branch: {branch_label}  |  tree: {clean_label}")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory mock: maps "arg0 arg1 arg2 …" → Result<String, WorkflowError>.
    struct MockRunner {
        responses: HashMap<String, Result<String, WorkflowError>>,
    }

    impl MockRunner {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }

        fn with(mut self, args: &[&str], result: Result<&str, WorkflowError>) -> Self {
            let key = args.join(" ");
            self.responses.insert(key, result.map(str::to_string));
            self
        }
    }

    impl GitRunner for MockRunner {
        fn run(&self, _root: &Path, args: &[&str]) -> Result<String, WorkflowError> {
            let key = args.join(" ");
            self.responses.get(&key).cloned().unwrap_or_else(|| {
                Err(WorkflowError::CommandFailed {
                    cmd: key.clone(),
                    stderr: format!("MockRunner: no entry for '{key}'"),
                })
            })
        }
    }

    fn workflow(runner: MockRunner) -> RepoWorkflow<MockRunner> {
        RepoWorkflow::with_runner(PathBuf::from("/repo"), runner)
    }

    // ── is_protected_branch ──────────────────────────────────────────────────

    #[test]
    fn protected_branch_main() {
        assert!(is_protected_branch("main"));
    }

    #[test]
    fn protected_branch_master() {
        assert!(is_protected_branch("master"));
    }

    #[test]
    fn protected_branch_release_prefix() {
        assert!(is_protected_branch("release/1.0"));
        assert!(is_protected_branch("release/v2.3.4"));
    }

    #[test]
    fn feature_branch_not_protected() {
        assert!(!is_protected_branch("feat/login"));
        assert!(!is_protected_branch("fix/upload-crash"));
        assert!(!is_protected_branch("routine/20260521-foo"));
        assert!(!is_protected_branch("develop"));
    }

    // ── current_branch ───────────────────────────────────────────────────────

    #[test]
    fn current_branch_returns_name() {
        let runner = MockRunner::new().with(
            &["git", "rev-parse", "--abbrev-ref", "HEAD"],
            Ok("feat/my-feature"),
        );
        let wf = workflow(runner);
        assert_eq!(wf.current_branch().unwrap(), "feat/my-feature");
    }

    // ── is_clean ─────────────────────────────────────────────────────────────

    #[test]
    fn is_clean_when_status_empty() {
        let runner = MockRunner::new().with(&["git", "status", "--porcelain"], Ok(""));
        assert!(workflow(runner).is_clean().unwrap());
    }

    #[test]
    fn not_clean_when_status_has_output() {
        let runner =
            MockRunner::new().with(&["git", "status", "--porcelain"], Ok(" M src/main.rs"));
        assert!(!workflow(runner).is_clean().unwrap());
    }

    // ── create_branch ────────────────────────────────────────────────────────

    #[test]
    fn create_branch_succeeds_on_clean_tree() {
        let runner = MockRunner::new()
            .with(&["git", "status", "--porcelain"], Ok(""))
            .with(&["git", "checkout", "-b", "feat/new"], Ok(""));
        assert!(workflow(runner).create_branch("feat/new").is_ok());
    }

    #[test]
    fn create_branch_fails_on_dirty_tree() {
        let runner = MockRunner::new().with(&["git", "status", "--porcelain"], Ok(" M src/lib.rs"));
        let err = workflow(runner).create_branch("feat/new").unwrap_err();
        assert!(matches!(err, WorkflowError::DirtyWorkingTree { .. }));
    }

    // ── push_branch ──────────────────────────────────────────────────────────

    #[test]
    fn push_branch_refuses_main() {
        let runner = MockRunner::new();
        let err = workflow(runner).push_branch("main").unwrap_err();
        assert!(matches!(err, WorkflowError::ProtectedBranch { .. }));
    }

    #[test]
    fn push_branch_refuses_master() {
        let runner = MockRunner::new();
        let err = workflow(runner).push_branch("master").unwrap_err();
        assert!(matches!(err, WorkflowError::ProtectedBranch { .. }));
    }

    #[test]
    fn push_branch_refuses_release_branch() {
        let runner = MockRunner::new();
        let err = workflow(runner).push_branch("release/1.0").unwrap_err();
        assert!(matches!(err, WorkflowError::ProtectedBranch { .. }));
    }

    #[test]
    fn push_branch_succeeds_for_feature_branch() {
        let runner = MockRunner::new().with(&["git", "push", "-u", "origin", "feat/login"], Ok(""));
        assert!(workflow(runner).push_branch("feat/login").is_ok());
    }

    // ── open_pr ──────────────────────────────────────────────────────────────

    #[test]
    fn open_pr_returns_url() {
        let runner = MockRunner::new().with(
            &[
                "gh",
                "pr",
                "create",
                "--title",
                "fix: auth crash",
                "--body",
                "Details here.",
                "--base",
                "main",
            ],
            Ok("https://github.com/owner/repo/pull/42"),
        );
        let url = workflow(runner)
            .open_pr("fix: auth crash", "Details here.", "main")
            .unwrap();
        assert_eq!(url, "https://github.com/owner/repo/pull/42");
    }

    // ── PreflightSummary::display ─────────────────────────────────────────────

    #[test]
    fn display_clean_feature_branch() {
        let s = PreflightSummary {
            branch: "feat/login".into(),
            clean: true,
        };
        let out = s.display();
        assert!(out.contains("feat/login ✓"));
        assert!(out.contains("clean ✓"));
    }

    #[test]
    fn display_warns_on_protected_branch() {
        let s = PreflightSummary {
            branch: "main".into(),
            clean: true,
        };
        let out = s.display();
        assert!(out.contains("⚠️"));
        assert!(out.contains("protected"));
    }
}
