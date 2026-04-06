//! Phase 2.2 — ToolContext: working directory resolution and path traversal
//! protection for tool execution within a project context.

use garraia_common::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Extended context for tool execution within a project.
///
/// Provides path resolution relative to a working directory and validates
/// that resolved paths do not escape the project sandbox.
#[derive(Debug, Clone)]
pub struct ProjectToolContext {
    /// The working directory for path resolution (project root).
    pub working_dir: Option<PathBuf>,
    /// The project ID this context is associated with.
    pub project_id: Option<String>,
    /// When true, paths are strictly confined to `working_dir`.
    pub sandbox_enabled: bool,
}

impl Default for ProjectToolContext {
    fn default() -> Self {
        Self {
            working_dir: None,
            project_id: None,
            sandbox_enabled: true,
        }
    }
}

impl ProjectToolContext {
    /// Create a new context with the given working directory.
    pub fn new(working_dir: Option<PathBuf>, project_id: Option<String>) -> Self {
        Self {
            working_dir,
            project_id,
            sandbox_enabled: true,
        }
    }

    /// Create a context with sandboxing disabled (for trusted sessions).
    pub fn unsandboxed(working_dir: Option<PathBuf>, project_id: Option<String>) -> Self {
        Self {
            working_dir,
            project_id,
            sandbox_enabled: false,
        }
    }

    /// Resolve a potentially relative path against the working directory.
    ///
    /// - If `relative` is absolute, it is returned as-is (but still validated
    ///   when `sandbox_enabled` is true).
    /// - If `relative` is relative and a `working_dir` is set, it is joined
    ///   to the working directory.
    /// - If no `working_dir` is set, the relative path is returned as-is.
    pub fn resolve_path(&self, relative: &str) -> Result<PathBuf> {
        let input = PathBuf::from(relative);

        let resolved = if input.is_absolute() {
            input
        } else if let Some(ref wd) = self.working_dir {
            wd.join(&input)
        } else {
            input
        };

        if self.sandbox_enabled {
            self.validate_path(&resolved)?;
        }

        Ok(resolved)
    }

    /// Validate that a path does not escape the working directory.
    ///
    /// This checks for `..` components and, when sandboxing is enabled,
    /// ensures the canonical path starts with the working directory.
    pub fn validate_path(&self, path: &Path) -> Result<()> {
        // Always reject `..` components to prevent path traversal.
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::Security("path traversal not allowed (contains '..')".into()));
        }

        // When sandboxing with a working directory, ensure the resolved path
        // stays within the working directory.
        if self.sandbox_enabled
            && let Some(ref wd) = self.working_dir {
                // Canonicalize both paths for comparison. If the file doesn't
                // exist yet (e.g. write target), canonicalize the parent
                // directory instead.
                let wd_canonical = wd.canonicalize().map_err(|e| {
                    Error::Agent(format!(
                        "cannot canonicalize working directory '{}': {e}",
                        wd.display()
                    ))
                })?;

                let path_canonical = if path.exists() {
                    path.canonicalize().map_err(|e| {
                        Error::Agent(format!(
                            "cannot canonicalize path '{}': {e}",
                            path.display()
                        ))
                    })?
                } else if let Some(parent) = path.parent() {
                    if parent.exists() {
                        let parent_canonical = parent.canonicalize().map_err(|e| {
                            Error::Agent(format!(
                                "cannot canonicalize parent '{}': {e}",
                                parent.display()
                            ))
                        })?;
                        parent_canonical.join(path.file_name().unwrap_or_default())
                    } else {
                        // Neither path nor parent exist — reject when sandboxed.
                        return Err(Error::Security(
                            "path parent directory does not exist inside sandbox".into(),
                        ));
                    }
                } else {
                    return Err(Error::Security("invalid path for sandbox validation".into()));
                };

                if !path_canonical.starts_with(&wd_canonical) {
                    return Err(Error::Security(format!(
                        "path '{}' escapes working directory '{}'",
                        path.display(),
                        wd.display()
                    )));
                }
            }

        Ok(())
    }

    /// Quick check whether a path is allowed under current sandbox rules.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        self.validate_path(path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_relative_with_working_dir() {
        let tmp = TempDir::new().unwrap();
        // Create the subdirectory so path validation can canonicalize the parent.
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
        let resolved = ctx.resolve_path("src/main.rs").unwrap();
        assert!(resolved.starts_with(tmp.path()));
        assert!(resolved.ends_with("src/main.rs"));
    }

    #[test]
    fn resolve_absolute_passes_through() {
        let tmp = TempDir::new().unwrap();
        let abs_path = tmp.path().join("foo.txt");
        // Create the file so canonicalize succeeds.
        std::fs::write(&abs_path, "").unwrap();

        let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
        let resolved = ctx.resolve_path(abs_path.to_str().unwrap()).unwrap();
        assert_eq!(
            resolved.canonicalize().unwrap(),
            abs_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        let tmp = TempDir::new().unwrap();
        let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
        let result = ctx.resolve_path("../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn unsandboxed_allows_anything() {
        let ctx = ProjectToolContext::unsandboxed(None, None);
        // Even `..` in the path should pass when sandbox is off.
        // (validate_path is not called)
        let result = ctx.resolve_path("../some/path");
        assert!(result.is_ok());
    }

    #[test]
    fn is_path_allowed_basic() {
        let tmp = TempDir::new().unwrap();
        let inside = tmp.path().join("hello.txt");
        std::fs::write(&inside, "").unwrap();

        let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
        assert!(ctx.is_path_allowed(&inside));
    }
}
