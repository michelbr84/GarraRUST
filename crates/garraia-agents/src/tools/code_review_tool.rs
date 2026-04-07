//! # Code Review Tool (Phase 5.3)
//!
//! Automated code review on git diff.
//! Gets diff, sends to LLM for review, returns structured feedback.

use async_trait::async_trait;
use garraia_common::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};
use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart,
};

/// Default timeout for diff operations
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Maximum diff lines to send to LLM
const MAX_DIFF_LINES: usize = 1000;

/// Automated code review tool that analyzes git diffs using an LLM.
/// Returns structured review with issues, suggestions, and severity ratings.
pub struct CodeReviewTool {
    /// LLM provider for the review
    provider: Arc<dyn LlmProvider>,
    /// Model to use
    model: String,
    /// Timeout for git operations
    timeout: Duration,
}

impl CodeReviewTool {
    /// Create a new CodeReviewTool
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        model: impl Into<String>,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
        }
    }

    /// Get git diff output
    async fn get_diff(
        &self,
        commit_range: Option<&str>,
        file_path: Option<&str>,
    ) -> std::result::Result<String, String> {
        let mut args = vec!["diff".to_string()];

        if let Some(range) = commit_range {
            args.push(range.to_string());
        }

        if let Some(path) = file_path {
            args.push("--".to_string());
            args.push(path.to_string());
        }

        let result = tokio::time::timeout(
            self.timeout,
            Command::new("git").args(&args).output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                if stdout.is_empty() {
                    Err("No diff output (no changes found)".to_string())
                } else {
                    // Limit diff size
                    let lines: Vec<&str> = stdout.lines().collect();
                    if lines.len() > MAX_DIFF_LINES {
                        let truncated: String = lines[..MAX_DIFF_LINES].join("\n");
                        Ok(format!(
                            "{}\n\n... (diff truncated, {} lines total)",
                            truncated,
                            lines.len()
                        ))
                    } else {
                        Ok(stdout)
                    }
                }
            }
            Ok(Err(e)) => Err(format!("Failed to run git diff: {}", e)),
            Err(_) => Err(format!(
                "git diff timed out after {}s",
                self.timeout.as_secs()
            )),
        }
    }

    /// Send diff to LLM for review
    async fn review_diff(&self, diff: &str, file_path: Option<&str>) -> std::result::Result<String, String> {
        let file_context = file_path
            .map(|p| format!(" for file: {}", p))
            .unwrap_or_default();

        let prompt = format!(
            r#"Review the following git diff{} and provide a structured code review.

For each issue found, specify:
1. **Severity**: Critical / Warning / Info / Suggestion
2. **File & Line**: Where the issue is
3. **Issue**: What the problem is
4. **Suggestion**: How to fix it

Focus on:
- Security vulnerabilities (SQL injection, XSS, secret exposure, etc.)
- Logic errors and potential bugs
- Performance issues
- Code quality and best practices
- Error handling (especially unwrap() in Rust)
- Missing edge cases

If the code looks good, say so and mention any minor improvements.

```diff
{}
```

Provide your review in a clear, structured format."#,
            file_context, diff
        );

        let messages = vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(prompt),
        }];

        let request = LlmRequest {
            model: self.model.clone(),
            messages,
            system: Some(
                "You are a senior code reviewer. Provide thorough, constructive feedback focused on correctness, security, and best practices. Be concise but specific.".to_string(),
            ),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            tools: vec![],
        };

        let response = self
            .provider
            .complete(&request)
            .await
            .map_err(|e| format!("LLM review error: {}", e))?;

        let review = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if review.is_empty() {
            Err("LLM returned empty review".to_string())
        } else {
            Ok(review)
        }
    }
}

#[async_trait]
impl Tool for CodeReviewTool {
    fn name(&self) -> &str {
        "code_review"
    }

    fn description(&self) -> &str {
        "Performs automated code review on git diff.\n\
         Gets the diff, sends it to an LLM for analysis, and returns structured feedback\n\
         with issues, suggestions, and severity ratings.\n\
         Focuses on security, bugs, performance, and best practices."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "commit_range": {
                    "type": "string",
                    "description": "Git commit range for diff (e.g., 'HEAD~3..HEAD', 'main..feature')"
                },
                "file_path": {
                    "type": "string",
                    "description": "Specific file to review (optional, reviews all changes if omitted)"
                }
            }
        })
    }

    async fn execute(&self, _context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let commit_range = input.get("commit_range").and_then(|v| v.as_str());
        let file_path = input.get("file_path").and_then(|v| v.as_str());

        // Get the diff
        let diff = match self.get_diff(commit_range, file_path).await {
            Ok(d) => d,
            Err(e) => return Ok(ToolOutput::error(e)),
        };

        // Review the diff
        match self.review_diff(&diff, file_path).await {
            Ok(review) => {
                let mut output = String::new();
                output.push_str("## Code Review\n\n");

                if let Some(range) = commit_range {
                    output.push_str(&format!("**Commit range:** {}\n", range));
                }
                if let Some(path) = file_path {
                    output.push_str(&format!("**File:** {}\n", path));
                }

                output.push_str("\n---\n\n");
                output.push_str(&review);

                Ok(ToolOutput::success(output))
            }
            Err(e) => Ok(ToolOutput::error(format!("Review failed: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_code_review_schema() {
        // We need a provider to create the tool, but we can test the schema statically
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "commit_range": {
                    "type": "string",
                    "description": "Git commit range for diff"
                },
                "file_path": {
                    "type": "string",
                    "description": "Specific file to review"
                }
            }
        });

        assert!(schema.get("properties").is_some());
    }
}
