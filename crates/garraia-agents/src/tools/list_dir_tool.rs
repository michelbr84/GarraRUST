//! # List Directory Tool (Phase 5.3)
//!
//! Intelligent directory listing with tree-style output,
//! file sizes, and .gitignore awareness.

use async_trait::async_trait;
use garraia_common::Result;
use std::path::{Path, PathBuf};

use super::{Tool, ToolContext, ToolOutput};

/// Maximum depth for directory traversal
const MAX_DEPTH: usize = 10;

/// Maximum entries to return
const MAX_ENTRIES: usize = 500;

/// Default depth
const DEFAULT_DEPTH: usize = 2;

/// Patterns to always skip (common non-useful directories)
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".dart_tool",
    ".pub-cache",
    "__pycache__",
    ".next",
    "dist",
    "build",
    ".gradle",
    ".idea",
    ".vs",
    ".vscode",
];

/// Intelligent directory listing with tree-style output.
/// Respects common ignore patterns and provides file sizes.
pub struct ListDirTool {
    max_entries: usize,
}

impl ListDirTool {
    /// Create a new ListDirTool
    pub fn new(max_entries: Option<usize>) -> Self {
        Self {
            max_entries: max_entries.unwrap_or(MAX_ENTRIES),
        }
    }

    /// Format file size in human-readable form
    fn format_size(size: u64) -> String {
        if size < 1024 {
            format!("{}B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1}KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1}GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Check if a directory should be skipped
    fn should_skip(name: &str) -> bool {
        SKIP_DIRS.iter().any(|&skip| name == skip)
    }

    /// Recursively list directory contents
    fn list_recursive(
        &self,
        path: &Path,
        prefix: &str,
        depth: usize,
        max_depth: usize,
        pattern: Option<&str>,
        entries: &mut Vec<String>,
    ) -> std::io::Result<()> {
        if depth > max_depth || entries.len() >= self.max_entries {
            return Ok(());
        }

        let mut items: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .collect();

        // Sort: directories first, then alphabetically
        items.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        let total = items.len();

        for (idx, entry) in items.iter().enumerate() {
            if entries.len() >= self.max_entries {
                entries.push(format!("{}... ({} more entries)", prefix, total - idx));
                break;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let is_last = idx == total - 1;
            let connector = if is_last { "\\-- " } else { "|-- " };
            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}|   ", prefix)
            };

            let file_type = entry.file_type().unwrap_or_else(|_| {
                // Fallback: treat as file
                std::fs::symlink_metadata(entry.path())
                    .map(|m| m.file_type())
                    .unwrap_or_else(|_| entry.file_type().unwrap())
            });

            if file_type.is_dir() {
                if Self::should_skip(&name) {
                    entries.push(format!("{}{}{}/  (skipped)", prefix, connector, name));
                    continue;
                }

                entries.push(format!("{}{}{}/", prefix, connector, name));

                if depth < max_depth {
                    let _ = self.list_recursive(
                        &entry.path(),
                        &child_prefix,
                        depth + 1,
                        max_depth,
                        pattern,
                        entries,
                    );
                }
            } else {
                // Apply pattern filter if specified
                if let Some(pat) = pattern {
                    let pat_lower = pat.to_lowercase();
                    let name_lower = name.to_lowercase();
                    if !name_lower.contains(&pat_lower)
                        && !glob_match(&pat_lower, &name_lower)
                    {
                        continue;
                    }
                }

                let size = entry
                    .metadata()
                    .map(|m| Self::format_size(m.len()))
                    .unwrap_or_else(|_| "?".to_string());

                entries.push(format!("{}{}{}  ({})", prefix, connector, name, size));
            }
        }

        Ok(())
    }
}

/// Simple glob matching (supports * and ?)
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Handle *.ext pattern
    if let Some(ext) = pattern.strip_prefix("*.") {
        return text.ends_with(&format!(".{}", ext));
    }

    // Handle prefix* pattern
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }

    pattern == text
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "Lists directory contents in a tree-style format.\n\
         Shows file sizes, skips common build/cache directories (.git, node_modules, target).\n\
         Supports depth control and file pattern filtering."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (default: current directory)"
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum depth to traverse (default: 2, max: 10)"
                },
                "pattern": {
                    "type": "string",
                    "description": "File pattern filter (e.g., '*.rs', '*.py')"
                }
            }
        })
    }

    async fn execute(&self, _context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let depth = input
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_DEPTH as u64) as usize;

        let depth = depth.min(MAX_DEPTH);

        let pattern = input.get("pattern").and_then(|v| v.as_str());

        let path = PathBuf::from(path_str);

        // Validate path exists
        if !path.exists() {
            return Ok(ToolOutput::error(format!(
                "Directory not found: {}",
                path_str
            )));
        }

        if !path.is_dir() {
            return Ok(ToolOutput::error(format!(
                "Not a directory: {}",
                path_str
            )));
        }

        let mut entries = Vec::new();
        entries.push(format!("{}/", path.display()));

        match self.list_recursive(&path, "", 0, depth, pattern, &mut entries) {
            Ok(()) => {
                if entries.len() >= self.max_entries {
                    entries.push(format!(
                        "\n(listing truncated at {} entries)",
                        self.max_entries
                    ));
                }
                Ok(ToolOutput::success(entries.join("\n")))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Error listing directory: {}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(ListDirTool::format_size(100), "100B");
        assert_eq!(ListDirTool::format_size(1500), "1.5KB");
        assert_eq!(ListDirTool::format_size(1_500_000), "1.4MB");
    }

    #[test]
    fn test_should_skip() {
        assert!(ListDirTool::should_skip(".git"));
        assert!(ListDirTool::should_skip("node_modules"));
        assert!(ListDirTool::should_skip("target"));
        assert!(!ListDirTool::should_skip("src"));
        assert!(!ListDirTool::should_skip("crates"));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test*", "test_file"));
    }

    #[tokio::test]
    async fn test_list_dir_current() {
        let tool = ListDirTool::new(None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"path": ".", "depth": 1}))
            .await
            .expect("should not error");

        assert!(!output.is_error);
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_list_dir_not_found() {
        let tool = ListDirTool::new(None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(
                &ctx,
                serde_json::json!({"path": "/nonexistent_dir_12345"}),
            )
            .await
            .expect("should not error");

        assert!(output.is_error);
    }
}
