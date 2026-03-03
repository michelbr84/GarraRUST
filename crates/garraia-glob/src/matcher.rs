//! Glob pattern matcher — now backed by the `GlobPattern` regex engine.

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{
    pattern::{GlobConfig, GlobPattern},
    GlobError, Result, DEFAULT_MAX_DEPTH, DEFAULT_MAX_FILES, DEFAULT_TIMEOUT_SECS,
};

/// Options for glob matching (legacy surface — new code should use [`GlobConfig`]).
#[derive(Debug, Clone)]
pub struct MatchOptions {
    /// Case-sensitive matching.
    pub case_sensitive: bool,
    /// If `false`, patterns do NOT implicitly match dotfiles.
    pub dot: bool,
    /// Maximum directory depth.
    pub max_depth: usize,
    /// Maximum files to process.
    pub max_files: usize,
    /// Timeout in seconds (reserved — not yet enforced).
    pub timeout_secs: u64,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: true,
            dot: false,
            max_depth: DEFAULT_MAX_DEPTH,
            max_files: DEFAULT_MAX_FILES,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

impl From<&MatchOptions> for GlobConfig {
    fn from(opts: &MatchOptions) -> GlobConfig {
        GlobConfig {
            case_sensitive: opts.case_sensitive,
            dot: opts.dot,
            ..GlobConfig::default()
        }
    }
}

/// Result of a glob match or scan operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub path: String,
    pub is_dir: bool,
}

/// Glob pattern matcher.
pub struct GlobMatcher {
    patterns: Vec<GlobPattern>,
    options: MatchOptions,
}

impl GlobMatcher {
    /// Create a new glob matcher with patterns.
    pub fn new(patterns: Vec<String>, options: MatchOptions) -> Result<Self> {
        let config = GlobConfig::from(&options);
        let mut compiled = Vec::new();

        for pattern in &patterns {
            if pattern.contains("..") {
                return Err(GlobError::PathTraversal(pattern.clone()));
            }
            compiled.push(GlobPattern::new(pattern, &config)?);
        }

        Ok(Self { patterns: compiled, options })
    }

    /// Check if a path matches any of the patterns.
    pub fn matches(&self, path: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
    }

    /// Walk a directory and return matching paths.
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
                tracing::warn!(max = self.options.max_files, "GlobMatcher: max_files limit reached");
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

    /// Get the pattern source strings.
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.iter().map(|p| p.source().to_string()).collect()
    }
}

/// Check if a single path matches a single pattern (uses default [`GlobConfig`]).
pub fn match_pattern(path: &str, pattern: &str) -> bool {
    if let Ok(p) = GlobPattern::new(pattern, &GlobConfig::default()) {
        p.matches(path)
    } else {
        false
    }
}
