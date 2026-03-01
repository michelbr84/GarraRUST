//! garraia-glob: Glob matching engine for GarraRUST
//!
//! Provides glob pattern matching with:
//! - Picomatch-based matching (fast, secure, POSIX-compliant)
//! - .gitignore/.garraignore file support
//! - Path normalization (cross-platform)
//! - Performance guardrails

pub mod matcher;
pub mod ignore;
pub mod path;

pub use matcher::{GlobMatcher, MatchOptions, MatchResult};
pub use ignore::IgnoreFile;
pub use path::normalize_path;

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

/// Default maximum directory depth for traversal
pub const DEFAULT_MAX_DEPTH: usize = 20;

/// Default maximum files to process
pub const DEFAULT_MAX_FILES: usize = 10_000;

/// Default timeout in DEFAULT_MAX_FILES: seconds
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
}
