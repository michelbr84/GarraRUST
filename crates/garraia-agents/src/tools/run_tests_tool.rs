//! # Run Tests Tool (Phase 5.3)
//!
//! Executes test suites and reports results.
//! Detects test framework automatically: cargo test, flutter test, npm test, pytest.

use async_trait::async_trait;
use garraia_common::Result;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

/// Default timeout for test execution
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum output size
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Detected test framework
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFramework {
    Cargo,
    Flutter,
    Npm,
    Pytest,
    Unknown,
}

/// Executes test suites and reports results.
/// Auto-detects the test framework based on project files.
pub struct RunTestsTool {
    timeout: Duration,
}

impl RunTestsTool {
    /// Create a new RunTestsTool
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
        }
    }

    /// Detect the test framework in the given directory
    fn detect_framework(working_dir: &Path) -> TestFramework {
        if working_dir.join("Cargo.toml").exists() {
            TestFramework::Cargo
        } else if working_dir.join("pubspec.yaml").exists() {
            TestFramework::Flutter
        } else if working_dir.join("package.json").exists() {
            TestFramework::Npm
        } else if working_dir.join("pytest.ini").exists()
            || working_dir.join("setup.py").exists()
            || working_dir.join("pyproject.toml").exists()
        {
            TestFramework::Pytest
        } else {
            TestFramework::Unknown
        }
    }

    /// Build the test command for the detected framework
    fn build_command(
        framework: TestFramework,
        test_name: Option<&str>,
        working_dir: &Path,
    ) -> (Command, String) {
        match framework {
            TestFramework::Cargo => {
                let mut cmd = Command::new("cargo");
                cmd.arg("test");
                if let Some(name) = test_name {
                    // Check if it looks like a crate name (e.g., "-p garraia-agents")
                    if name.starts_with("-p ") {
                        let parts: Vec<&str> = name.splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            cmd.arg("-p").arg(parts[1]);
                        }
                    } else {
                        cmd.arg(name);
                    }
                }
                cmd.arg("--").arg("--color=never");
                cmd.current_dir(working_dir);
                (cmd, "cargo test".to_string())
            }
            TestFramework::Flutter => {
                let mut cmd = Command::new("flutter");
                cmd.arg("test");
                if let Some(name) = test_name {
                    cmd.arg(name);
                }
                cmd.current_dir(working_dir);
                (cmd, "flutter test".to_string())
            }
            TestFramework::Npm => {
                let mut cmd = Command::new("npm");
                cmd.arg("test");
                if let Some(name) = test_name {
                    cmd.arg("--").arg(name);
                }
                cmd.current_dir(working_dir);
                (cmd, "npm test".to_string())
            }
            TestFramework::Pytest => {
                let mut cmd = Command::new("python");
                cmd.arg("-m").arg("pytest");
                if let Some(name) = test_name {
                    cmd.arg(name);
                }
                cmd.arg("-v");
                cmd.current_dir(working_dir);
                (cmd, "pytest".to_string())
            }
            TestFramework::Unknown => {
                // Try cargo test as default for Rust projects
                let mut cmd = Command::new("cargo");
                cmd.arg("test");
                cmd.arg("--").arg("--color=never");
                cmd.current_dir(working_dir);
                (cmd, "cargo test (default)".to_string())
            }
        }
    }

    /// Parse test output for summary
    fn parse_summary(framework: TestFramework, output: &str) -> String {
        match framework {
            TestFramework::Cargo => {
                // Look for "test result:" line
                let mut summary = String::new();
                for line in output.lines() {
                    if line.contains("test result:") {
                        summary.push_str(line.trim());
                        summary.push('\n');
                    } else if line.contains("FAILED") || line.contains("failures:") {
                        summary.push_str(line.trim());
                        summary.push('\n');
                    }
                }
                if summary.is_empty() {
                    "No test summary found in output.".to_string()
                } else {
                    summary
                }
            }
            _ => {
                // Generic: look for pass/fail patterns
                let lines: Vec<&str> = output.lines().collect();
                let last_lines: Vec<&str> = lines.iter().rev().take(10).copied().collect();
                last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
            }
        }
    }

    /// Truncate output if needed
    fn truncate(output: &str) -> String {
        if output.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while end > 0 && !output.is_char_boundary(end) {
                end -= 1;
            }
            let mut truncated = output[..end].to_string();
            truncated.push_str("\n... (output truncated)");
            truncated
        } else {
            output.to_string()
        }
    }
}

#[async_trait]
impl Tool for RunTestsTool {
    fn name(&self) -> &str {
        "run_tests"
    }

    fn description(&self) -> &str {
        "Executes the project test suite and reports results.\n\
         Auto-detects test framework: cargo test, flutter test, npm test, pytest.\n\
         Returns pass/fail counts and failed test details."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "test_name": {
                    "type": "string",
                    "description": "Specific test name or filter (optional). For cargo: test function name or '-p crate_name'"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for test execution (default: current directory)"
                },
                "framework": {
                    "type": "string",
                    "enum": ["cargo", "flutter", "npm", "pytest", "auto"],
                    "description": "Test framework to use (default: auto-detect)"
                }
            }
        })
    }

    async fn execute(&self, _context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let test_name = input.get("test_name").and_then(|v| v.as_str());
        let working_dir_str = input
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let working_dir = std::path::PathBuf::from(working_dir_str);

        if !working_dir.exists() {
            return Ok(ToolOutput::error(format!(
                "Working directory not found: {}",
                working_dir_str
            )));
        }

        // Detect or use specified framework
        let framework_str = input.get("framework").and_then(|v| v.as_str());
        let framework = match framework_str {
            Some("cargo") => TestFramework::Cargo,
            Some("flutter") => TestFramework::Flutter,
            Some("npm") => TestFramework::Npm,
            Some("pytest") => TestFramework::Pytest,
            _ => Self::detect_framework(&working_dir),
        };

        let (mut cmd, framework_name) = Self::build_command(framework, test_name, &working_dir);

        // Execute with timeout
        let result = tokio::time::timeout(self.timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combined = String::new();
                combined.push_str(&format!("Framework: {}\n", framework_name));
                combined.push_str(&format!(
                    "Exit code: {}\n\n",
                    output.status.code().unwrap_or(-1)
                ));

                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                // Add summary at the end
                let summary = Self::parse_summary(framework, &combined);
                combined.push_str("\n--- Summary ---\n");
                combined.push_str(&summary);

                let is_success = output.status.success();
                let content = Self::truncate(&combined);

                if is_success {
                    Ok(ToolOutput::success(content))
                } else {
                    Ok(ToolOutput::error(content))
                }
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!(
                "Failed to execute {}: {}",
                framework_name, e
            ))),
            Err(_) => Ok(ToolOutput::error(format!(
                "Tests timed out after {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_framework() {
        // Current dir should have Cargo.toml
        let framework = RunTestsTool::detect_framework(Path::new("."));
        assert_eq!(framework, TestFramework::Cargo);
    }

    #[test]
    fn test_parse_cargo_summary() {
        let output = r#"
running 3 tests
test test_one ... ok
test test_two ... ok
test test_three ... FAILED

failures:

---- test_three stdout ----
assertion failed

failures:
    test_three

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let summary = RunTestsTool::parse_summary(TestFramework::Cargo, output);
        assert!(summary.contains("FAILED"));
        assert!(summary.contains("test result:"));
    }

    #[test]
    fn test_run_tests_schema() {
        let tool = RunTestsTool::new(None);
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }
}
