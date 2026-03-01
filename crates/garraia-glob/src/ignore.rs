//! .gitignore/.garraignore file support

use std::path::Path;

use crate::Result;

/// Represents a .garraignore or .gitignore file
pub struct IgnoreFile {
    patterns: Vec<String>,
    negated: Vec<String>,
}

impl IgnoreFile {
    /// Parse an ignore file from contents
    pub fn parse(contents: &str) -> Self {
        let mut patterns = Vec::new();
        let mut negated = Vec::new();

        for line in contents.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Handle negation patterns
            if trimmed.starts_with('!') {
                negated.push(trimmed[1..].to_string());
            } else {
                patterns.push(trimmed.to_string());
            }
        }

        Self { patterns, negated }
    }

    /// Parse an ignore file from a path
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(Self::parse(&contents))
    }

    /// Check if a path should be ignored
    pub fn is_ignored(&self, path: &str) -> bool {
        use crate::matcher::match_pattern;

        // Check positive patterns first
        for pattern in &self.patterns {
            if match_pattern(path, pattern) {
                // Check if negated
                let negated_result = self
                    .negated
                    .iter()
                    .any(|neg| match_pattern(path, neg));

                if !negated_result {
                    return true;
                }
            }
        }

        false
    }

    /// Get patterns
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Get negated patterns
    pub fn negated(&self) -> &[String] {
        &self.negated
    }
}
