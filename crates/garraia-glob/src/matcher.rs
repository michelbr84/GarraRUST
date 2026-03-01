//! Glob pattern matcher using the glob crate

use glob::Pattern;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{GlobError, Result, DEFAULT_MAX_DEPTH, DEFAULT_MAX_FILES, DEFAULT_TIMEOUT_SECS};

/// Options for glob matching
#[derive(Debug, Clone)]
pub struct MatchOptions {
    /// Case insensitive matching
    pub case_sensitive: bool,
    /// Treat ? as single character (default true)
    pub dot: bool,
    /// Maximum directory depth
    pub max_depth: usize,
    /// Maximum files to process
    pub max_files: usize,
    /// Timeout in seconds
    pub timeout_secs: u64,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            dot: true,
            max_depth: DEFAULT_MAX_DEPTH,
            max_files: DEFAULT_MAX_FILES,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

/// Result of a glob match operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub path: String,
    pub is_dir: bool,
}

/// Glob pattern matcher
pub struct GlobMatcher {
    patterns: Vec<Pattern>,
    options: MatchOptions,
}

impl GlobMatcher {
    /// Create a new glob matcher with patterns
    pub fn new(patterns: Vec<String>, options: MatchOptions) -> Result<Self> {
        let mut compiled_patterns = Vec::new();

        for pattern in &patterns {
            // Check for path traversal
            if pattern.contains("..") {
                return Err(GlobError::PathTraversal(pattern.clone()));
            }

            // Compile pattern
            let glob_pattern = Pattern::new(pattern)
                .map_err(|e| GlobError::InvalidPattern(format!("{}: {}", pattern, e)))?;

            compiled_patterns.push(glob_pattern);
        }

        Ok(Self {
            patterns: compiled_patterns,
            options,
        })
    }

    /// Check if a path matches any of the patterns
    pub fn matches(&self, path: &str) -> bool {
        let opts = glob::MatchOptions::new();

        for pattern in &self.patterns {
            if pattern.matches_with(path, opts) {
                return true;
            }
        }
        false
    }

    /// Walk a directory and return matching paths
    pub fn walk(&self, root: &str) -> Result<Vec<MatchResult>> {
        let mut results = Vec::new();
        let mut file_count = 0;

        for entry in WalkDir::new(root)
            .max_depth(self.options.max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if file_count >= self.options.max_files {
                tracing::warn!(
                    "Max files limit reached: {}",
                    self.options.max_files
                );
                break;
            }

            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            if relative.is_empty() {
                continue;
            }

            // Check both the full path and relative path
            let path_str = path.to_string_lossy().replace('\\', "/");
            if self.matches(&relative) || self.matches(&path_str) {
                results.push(MatchResult {
                    path: relative,
                    is_dir: path.is_dir(),
                });
                file_count += 1;
            }
        }

        Ok(results)
    }

    /// Get the patterns
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.iter().map(|p| p.as_str().to_string()).collect()
    }
}

/// Check if a single path matches a single pattern
pub fn match_pattern(path: &str, pattern: &str) -> bool {
    let opts = glob::MatchOptions::new();
    if let Ok(pm) = Pattern::new(pattern) {
        pm.matches_with(path, opts)
    } else {
        false
    }
}
