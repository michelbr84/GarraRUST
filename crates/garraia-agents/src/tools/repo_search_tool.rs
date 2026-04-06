//! # Repo Search Tool (Phase 5.3)
//!
//! Searches code semantically using grep + file pattern matching.
//! Returns matching file paths, line numbers, and context.

use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

/// Maximum output size in bytes
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

/// Default timeout for search operations
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Default maximum results returned
const DEFAULT_MAX_RESULTS: usize = 50;

/// Default context lines around matches
const DEFAULT_CONTEXT_LINES: u32 = 2;

/// Searches code in a repository using grep and file pattern matching.
/// Returns matching file paths with line numbers and surrounding context.
pub struct RepoSearchTool {
    timeout: Duration,
    max_results: usize,
}

impl RepoSearchTool {
    /// Create a new RepoSearchTool
    pub fn new(timeout_secs: Option<u64>, max_results: Option<usize>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            max_results: max_results.unwrap_or(DEFAULT_MAX_RESULTS),
        }
    }

    /// Truncate output if too large
    fn truncate_output(&self, output: &str) -> String {
        if output.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while end > 0 && !output.is_char_boundary(end) {
                end -= 1;
            }
            let mut truncated = output[..end].to_string();
            truncated.push_str("\n\n... (output truncated)");
            truncated
        } else {
            output.to_string()
        }
    }
}

#[async_trait]
impl Tool for RepoSearchTool {
    fn name(&self) -> &str {
        "repo_search"
    }

    fn description(&self) -> &str {
        "Searches code in the repository using pattern matching.\n\
         Finds files containing the query string, returns file paths, line numbers, and context.\n\
         Supports file pattern filtering (e.g., '*.rs', '*.py')."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string (regex supported)"
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.rs', '**/*.ts')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 50)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines around each match (default: 2)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, _context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parameter 'query' is required".into()))?;

        if query.is_empty() {
            return Ok(ToolOutput::error("query cannot be empty"));
        }

        let file_pattern = input.get("file_pattern").and_then(|v| v.as_str());
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64) as usize;
        let context_lines = input
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_CONTEXT_LINES as u64) as u32;

        // Build and execute the search command
        let mut cmd = Command::new("rg");
        cmd.arg("--line-number")
            .arg("--no-heading")
            .arg("--color")
            .arg("never")
            .arg("-C")
            .arg(context_lines.to_string())
            .arg("--max-count")
            .arg(max_results.to_string());

        if let Some(pattern) = file_pattern {
            cmd.arg("--glob").arg(pattern);
        }

        cmd.arg(query).arg(".");

        let result = tokio::time::timeout(self.timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if stdout.is_empty() && output.status.code() == Some(1) {
                    // rg returns exit code 1 when no matches found
                    return Ok(ToolOutput::success("No matches found."));
                }

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() && !output.status.success() {
                    combined.push_str("\nSTDERR: ");
                    combined.push_str(&stderr);
                }

                if combined.is_empty() {
                    combined = "No matches found.".to_string();
                }

                Ok(ToolOutput::success(self.truncate_output(&combined)))
            }
            Ok(Err(e)) => {
                // rg not found, try grep as fallback
                let mut grep_cmd = Command::new(if cfg!(target_os = "windows") {
                    "findstr"
                } else {
                    "grep"
                });

                if cfg!(target_os = "windows") {
                    grep_cmd.arg("/s").arg("/n").arg(query).arg("*.*");
                } else {
                    grep_cmd
                        .arg("-rn")
                        .arg("--color=never")
                        .arg("-C")
                        .arg(context_lines.to_string())
                        .arg(query)
                        .arg(".");
                }

                let fallback = tokio::time::timeout(self.timeout, grep_cmd.output()).await;

                match fallback {
                    Ok(Ok(output)) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        if stdout.is_empty() {
                            Ok(ToolOutput::success("No matches found."))
                        } else {
                            Ok(ToolOutput::success(self.truncate_output(&stdout)))
                        }
                    }
                    Ok(Err(e2)) => Ok(ToolOutput::error(format!(
                        "Search failed (rg: {}, grep: {})",
                        e, e2
                    ))),
                    Err(_) => Ok(ToolOutput::error(format!(
                        "Search timed out after {}s",
                        self.timeout.as_secs()
                    ))),
                }
            }
            Err(_) => Ok(ToolOutput::error(format!(
                "Search timed out after {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_search_schema() {
        let tool = RepoSearchTool::new(None, None);
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert_eq!(
            schema["required"].as_array().map(|a| a.len()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn test_repo_search_empty_query() {
        let tool = RepoSearchTool::new(None, None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let result = tool
            .execute(&ctx, serde_json::json!({"query": ""}))
            .await
            .expect("should not error");

        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_repo_search_missing_query() {
        let tool = RepoSearchTool::new(None, None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
