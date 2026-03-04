//! garraia-glob: Glob matching engine for GarraRUST
//!
//! Provides glob pattern matching with:
//! - Picomatch-compatible semantics (fast, linear-time via NFA/DFA regex — no backtracking)
//! - Full extglob: `!(…)`, `@(…)`, `?(…)`, `*(…)`, `+(…)`
//! - Optional Bash-style greedy negation (`bash_greedy_negated_extglob`)
//! - `.gitignore` / `.garraignore` file support
//! - Unified repo scanner (respects ignore rules + glob_mode config)
//! - Path normalization (cross-platform)
//! - Performance guardrails (max depth, max files)

pub mod matcher;
pub mod ignore;
pub mod path;
pub mod pattern;
pub mod scanner;

pub use matcher::{GlobMatcher, MatchOptions, MatchResult};
pub use ignore::{IgnoreFile, IgnoreKind};
pub use path::normalize_path;
pub use pattern::{GlobConfig, GlobMode, GlobPattern};
pub use scanner::Scanner;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GlobError {
    #[error("Invalid glob pattern: {0}")]
    InvalidPattern(String),

    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Performance limit exceeded: {0}")]
    PerformanceLimit(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GlobError>;

/// Default maximum directory depth for traversal.
pub const DEFAULT_MAX_DEPTH: usize = 20;

/// Default maximum files to process.
pub const DEFAULT_MAX_FILES: usize = 10_000;

/// Default timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_glob() {
        let matcher = GlobMatcher::new(
            vec!["*.rs".to_string()],
            MatchOptions::default(),
        ).unwrap();

        assert!(matcher.matches("main.rs"));
        assert!(!matcher.matches("main.py"));
    }

    #[test]
    fn test_path_normalization() {
        assert_eq!(
            normalize_path("src\\main.rs"),
            "src/main.rs"
        );
    }

    #[test]
    fn test_glob_pattern_extglob() {
        let p = GlobPattern::new("!(foo).rs", &GlobConfig::default()).unwrap();
        assert!(p.matches("bar.rs"));
        assert!(!p.matches("foo.rs"));
    }

    #[test]
    fn test_glob_mode_default() {
        assert_eq!(GlobMode::default(), GlobMode::Picomatch);
    }
}
