//! `garra verify` — local idempotent validation pipeline (GAR-501).
//!
//! Runs five steps in sequence: `cargo fmt --check`, `cargo clippy`,
//! `cargo test`, `flutter analyze`, `gitleaks detect`.  Steps that require a
//! tool that is not in `PATH` are skipped gracefully; steps that fail return
//! exit code 2 (sysexits `EX_USAGE`).
//!
//! Exit codes:
//!   0  — all non-skipped steps passed.
//!   2  — at least one step failed.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Serialize;

pub mod exit_codes {
    pub const OK: i32 = 0;
    pub const STEP_FAILED: i32 = 2;
}

/// Outcome of a single verification step.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepOutcome {
    Pass,
    Fail { exit_code: i32 },
    Skipped { reason: String },
}

/// Result of one step, ready for human or JSON reporting.
#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub name: String,
    pub outcome: StepOutcome,
    /// Wall-clock duration in milliseconds.  Zero for skipped steps.
    pub duration_ms: u64,
    /// Captured output (only populated in `--json` mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl StepResult {
    fn failed(&self) -> bool {
        matches!(self.outcome, StepOutcome::Fail { .. })
    }
}

/// Top-level JSON report shape (documented in `docs/maxpower/verify-schema.json`).
#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    exit_code: i32,
    steps: &'a [StepResult],
}

/// Entry point for `garra verify`.
///
/// Returns the exit code that `main` should pass to `std::process::exit`.
pub fn run(json: bool, skip: &[String], workspace_root: &Path) -> i32 {
    let results = run_steps(skip, workspace_root, json);

    let any_failed = results.iter().any(|r| r.failed());
    let exit_code = if any_failed {
        exit_codes::STEP_FAILED
    } else {
        exit_codes::OK
    };

    if json {
        print_json_report(&results, exit_code);
    } else {
        print_human_summary(&results, exit_code);
    }

    exit_code
}

/// Detect whether `tool` is available in `PATH` by attempting a no-op invocation.
fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Build and run one step.  In capture mode (`json == true`) stdout+stderr are
/// collected; otherwise they are inherited so the user sees live output.
fn run_step(
    name: &'static str,
    program: &str,
    args: &[&str],
    cwd: &Path,
    capture: bool,
) -> StepResult {
    let start = Instant::now();

    let result = if capture {
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
    } else {
        // Print a header line so the user knows which step is running.
        println!();
        println!("── {name} ──────────────────────────────────────────────────");
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(out) => {
            let exit_code = out.status.code().unwrap_or(1);
            let outcome = if out.status.success() {
                StepOutcome::Pass
            } else {
                StepOutcome::Fail { exit_code }
            };
            let output = if capture {
                let mut buf = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    buf.push('\n');
                    buf.push_str(&stderr);
                }
                Some(buf)
            } else {
                None
            };
            StepResult {
                name: name.to_string(),
                outcome,
                duration_ms,
                output,
            }
        }
        Err(e) => {
            // Could not spawn the process at all (e.g. binary not found after check).
            StepResult {
                name: name.to_string(),
                outcome: StepOutcome::Fail { exit_code: 127 },
                duration_ms,
                output: Some(format!("spawn error: {e}")),
            }
        }
    }
}

fn skipped(name: &'static str, reason: &str) -> StepResult {
    StepResult {
        name: name.to_string(),
        outcome: StepOutcome::Skipped {
            reason: reason.to_string(),
        },
        duration_ms: 0,
        output: None,
    }
}

fn run_steps(skip: &[String], workspace_root: &Path, capture: bool) -> Vec<StepResult> {
    let mut results = Vec::new();

    // ── Step 1: cargo fmt --check ──────────────────────────────────────────
    if skip.iter().any(|s| s == "fmt") {
        results.push(skipped("fmt", "skipped via --skip"));
    } else {
        results.push(run_step(
            "fmt",
            "cargo",
            &["fmt", "--all", "--check"],
            workspace_root,
            capture,
        ));
    }

    // ── Step 2: cargo clippy ───────────────────────────────────────────────
    if skip.iter().any(|s| s == "clippy") {
        results.push(skipped("clippy", "skipped via --skip"));
    } else {
        results.push(run_step(
            "clippy",
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--tests",
                "--exclude",
                "garraia-desktop",
                "--no-deps",
                "--",
                "-D",
                "warnings",
            ],
            workspace_root,
            capture,
        ));
    }

    // ── Step 3: cargo test ─────────────────────────────────────────────────
    if skip.iter().any(|s| s == "test") {
        results.push(skipped("test", "skipped via --skip"));
    } else {
        results.push(run_step(
            "test",
            "cargo",
            &[
                "test",
                "--workspace",
                "--no-fail-fast",
                "--exclude",
                "garraia-desktop",
            ],
            workspace_root,
            capture,
        ));
    }

    // ── Step 4: flutter analyze ────────────────────────────────────────────
    if skip.iter().any(|s| s == "flutter") {
        results.push(skipped("flutter", "skipped via --skip"));
    } else {
        let mobile_dir = workspace_root.join("apps").join("garraia-mobile");
        if !mobile_dir.exists() {
            results.push(skipped("flutter", "apps/garraia-mobile not found"));
        } else if !tool_available("flutter") {
            results.push(skipped("flutter", "flutter not in PATH"));
        } else {
            results.push(run_step(
                "flutter",
                "flutter",
                &["analyze"],
                &mobile_dir,
                capture,
            ));
        }
    }

    // ── Step 5: gitleaks detect ────────────────────────────────────────────
    if skip.iter().any(|s| s == "gitleaks") {
        results.push(skipped("gitleaks", "skipped via --skip"));
    } else if !tool_available("gitleaks") {
        results.push(skipped("gitleaks", "gitleaks not in PATH"));
    } else {
        results.push(run_step(
            "gitleaks",
            "gitleaks",
            &["detect", "--source", ".", "--no-banner"],
            workspace_root,
            capture,
        ));
    }

    results
}

fn print_human_summary(results: &[StepResult], exit_code: i32) {
    println!();
    println!("── garra verify results ────────────────────────────────────────");
    for r in results {
        let (icon, detail) = match &r.outcome {
            StepOutcome::Pass => ("✓", format!("{}ms", r.duration_ms)),
            StepOutcome::Fail { exit_code } => {
                ("✗", format!("exit {exit_code}  {}ms", r.duration_ms))
            }
            StepOutcome::Skipped { reason } => ("–", reason.clone()),
        };
        println!("  {icon}  {:<12}  {detail}", r.name);
    }
    println!();
    if exit_code == exit_codes::OK {
        println!("  All checks passed.");
    } else {
        println!("  One or more checks failed (exit {exit_code}).");
    }
    println!();
}

fn print_json_report(results: &[StepResult], exit_code: i32) {
    let report = Report {
        ok: exit_code == exit_codes::OK,
        exit_code,
        steps: results,
    };
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("error serializing verify report: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pass(name: &'static str) -> StepResult {
        StepResult {
            name: name.to_string(),
            outcome: StepOutcome::Pass,
            duration_ms: 10,
            output: None,
        }
    }

    fn fail(name: &'static str, code: i32) -> StepResult {
        StepResult {
            name: name.to_string(),
            outcome: StepOutcome::Fail { exit_code: code },
            duration_ms: 5,
            output: None,
        }
    }

    fn skip(name: &'static str) -> StepResult {
        StepResult {
            name: name.to_string(),
            outcome: StepOutcome::Skipped {
                reason: "test".into(),
            },
            duration_ms: 0,
            output: None,
        }
    }

    #[test]
    fn exit_code_ok_when_all_pass() {
        let results = [pass("fmt"), pass("clippy"), skip("test")];
        let any_failed = results.iter().any(|r| r.failed());
        assert!(!any_failed);
    }

    #[test]
    fn exit_code_fail_when_one_fails() {
        let results = [pass("fmt"), fail("clippy", 1), skip("test")];
        let any_failed = results.iter().any(|r| r.failed());
        assert!(any_failed);
    }

    #[test]
    fn skipped_step_does_not_count_as_failure() {
        let r = skip("flutter");
        assert!(!r.failed());
        assert!(matches!(r.outcome, StepOutcome::Skipped { .. }));
    }

    #[test]
    fn json_report_shape() {
        let results = [pass("fmt"), fail("clippy", 2), skip("flutter")];
        let exit_code = exit_codes::STEP_FAILED;
        let report = Report {
            ok: false,
            exit_code,
            steps: &results,
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(v["ok"], false);
        assert_eq!(v["exit_code"], 2);
        assert!(v["steps"].is_array());
        assert_eq!(v["steps"][0]["name"], "fmt");
        assert_eq!(v["steps"][0]["outcome"]["kind"], "pass");
        assert_eq!(v["steps"][1]["outcome"]["kind"], "fail");
        assert_eq!(v["steps"][2]["outcome"]["kind"], "skipped");
    }

    #[test]
    fn skip_list_matching_is_exact() {
        let skip_list = ["fmt".to_string(), "clippy".to_string()];
        assert!(skip_list.iter().any(|s| s == "fmt"));
        assert!(skip_list.iter().any(|s| s == "clippy"));
        assert!(!skip_list.iter().any(|s| s == "test"));
        assert!(!skip_list.iter().any(|s| s == "flutter"));
    }

    #[test]
    fn tool_available_returns_false_for_nonexistent() {
        assert!(!tool_available("__nonexistent_tool_9999__"));
    }

    #[test]
    fn tool_available_returns_true_for_cargo() {
        assert!(tool_available("cargo"));
    }

    #[test]
    fn run_skips_all_steps_when_all_in_skip_list() {
        let all_steps = vec![
            "fmt".to_string(),
            "clippy".to_string(),
            "test".to_string(),
            "flutter".to_string(),
            "gitleaks".to_string(),
        ];
        // Use a non-existent workspace root — steps are all skipped so no
        // process is spawned; `Path::new(".")` is fine as fallback.
        let results = run_steps(&all_steps, Path::new("."), false);
        assert_eq!(results.len(), 5);
        for r in &results {
            assert!(
                matches!(r.outcome, StepOutcome::Skipped { .. }),
                "expected skipped for step {}, got {:?}",
                r.name,
                r.outcome
            );
        }
    }

    #[test]
    fn workspace_root_default_is_current_dir() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Just verify we can construct the mobile path without panicking.
        let mobile = cwd.join("apps").join("garraia-mobile");
        let _ = mobile.exists();
    }
}
