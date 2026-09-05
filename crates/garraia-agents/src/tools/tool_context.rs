//! Phase 2.2 — ToolContext: working directory resolution and path traversal
//! protection for tool execution within a project context.
//!
//! Issue #923 added the free functions at the bottom of this module. The
//! file tools used to do `PathBuf::from(input)` verbatim: no `~` expansion
//! anywhere in the crate, `ToolContext.working_dir` ignored, and an error
//! that dropped the path it had tried. On Termux that combination made the
//! agent tell a user a file "did not exist" while the filesystem MCP server
//! read the very same path — the agent had no way to know it had looked
//! somewhere else.

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
            return Err(Error::Security(
                "path traversal not allowed (contains '..')".into(),
            ));
        }

        // When sandboxing with a working directory, ensure the resolved path
        // stays within the working directory.
        if self.sandbox_enabled
            && let Some(ref wd) = self.working_dir
        {
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
                return Err(Error::Security(
                    "invalid path for sandbox validation".into(),
                ));
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

// ─── Path resolution for the file tools (issue #923) ───────────────────────
//
// Kept as free, *pure* functions with the environment passed in — the same
// shape as `termux_ld_preload` in the MCP manager, and for the same reason:
// the interesting cases (`~` on a machine with no HOME, a relative path with
// no working dir) are the ones you cannot reproduce by running the test suite
// on a normal developer box.

/// How a raw tool argument became a filesystem path. Carried into error
/// messages so a failure says *where* it looked, not just that it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOrigin {
    /// The input was already absolute.
    Absolute,
    /// A leading `~`, `~/` or `$HOME` was expanded.
    HomeExpanded,
    /// A relative input joined to the session's `working_dir`.
    JoinedWorkingDir,
    /// A relative input with no `working_dir` to join it to. It resolves
    /// against the *gateway process* CWD, which is almost never what the
    /// caller meant — this is the case worth naming out loud.
    RelativeToProcessCwd,
}

impl PathOrigin {
    /// A short clause explaining the resolution, for error messages.
    pub fn hint(self) -> &'static str {
        match self {
            PathOrigin::Absolute => "caminho absoluto",
            PathOrigin::HomeExpanded => "expandido a partir de ~/$HOME",
            PathOrigin::JoinedWorkingDir => "relativo ao working_dir da sessão",
            PathOrigin::RelativeToProcessCwd => {
                "relativo ao diretório de trabalho do processo do gateway \
                 (a sessão não tem working_dir) — informe um caminho absoluto \
                 ou use ~/"
            }
        }
    }
}

/// A tool argument after resolution, plus how it got there.
#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub origin: PathOrigin,
    /// The argument exactly as the model wrote it.
    pub requested: String,
}

impl ResolvedPath {
    /// `"'<resolved>' (pedido: '<raw>', <hint>)"` — the tail every file-tool
    /// error carries, so the model can tell "wrong path" from "no such file".
    pub fn describe(&self) -> String {
        format!(
            "'{}' (pedido: '{}', {})",
            self.path.display(),
            self.requested,
            self.origin.hint()
        )
    }
}

/// Expand a leading `~`, `~/…` or `$HOME/…` against `home`.
///
/// Only a *leading* segment is expanded, and only when `home` is known: a
/// path with `~` in the middle is a legitimate filename, and silently
/// rewriting it would be worse than leaving it alone. With `home` absent the
/// input is returned untouched, which fails later with a message naming the
/// literal `~` path — a comprehensible error rather than a wrong guess.
pub fn expand_home(raw: &str, home: Option<&Path>) -> (PathBuf, bool) {
    let Some(home) = home else {
        return (PathBuf::from(raw), false);
    };

    let rest = if raw == "~" || raw == "$HOME" {
        Some("")
    } else {
        raw.strip_prefix("~/")
            .or_else(|| raw.strip_prefix("$HOME/"))
    };

    match rest {
        Some("") => (home.to_path_buf(), true),
        Some(r) => (home.join(r), true),
        None => (PathBuf::from(raw), false),
    }
}

/// Resolve a raw tool path argument: expand `~`, then join a relative path to
/// the session's `working_dir` when there is one.
///
/// `..` is rejected before anything else, exactly as the file tools did
/// before — this is a resolution change, not a loosening of the security
/// posture. The check runs on the raw input so a `~/../etc` cannot smuggle a
/// traversal in through the expansion.
pub fn resolve_tool_path(
    raw: &str,
    working_dir: Option<&str>,
    home: Option<&Path>,
) -> Result<ResolvedPath> {
    if Path::new(raw)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(Error::Security("path traversal não permitido".into()));
    }

    let (expanded, was_home) = expand_home(raw, home);

    let (path, origin) = if was_home {
        (expanded, PathOrigin::HomeExpanded)
    } else if expanded.is_absolute() {
        (expanded, PathOrigin::Absolute)
    } else if let Some(wd) = working_dir.filter(|w| !w.is_empty()) {
        (Path::new(wd).join(&expanded), PathOrigin::JoinedWorkingDir)
    } else {
        (expanded, PathOrigin::RelativeToProcessCwd)
    };

    Ok(ResolvedPath {
        path,
        origin,
        requested: raw.to_string(),
    })
}

/// The process home directory, or `None` when the environment has none.
///
/// Deliberately reads the environment rather than pulling in `dirs`: the
/// pure functions above take `home` as a parameter, so this is the whole
/// impure surface and a new dependency edge on `garraia-agents` would buy
/// nothing. `HOME` is always set inside Termux, which is the reported case.
pub fn process_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
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

    // ─── resolve_tool_path / expand_home (issue #923) ──────────────────────

    #[test]
    fn expands_a_leading_tilde() {
        let home = PathBuf::from("/data/data/com.termux/files/home");
        for raw in ["~/Documents/metas.md", "$HOME/Documents/metas.md"] {
            let (p, expanded) = expand_home(raw, Some(&home));
            assert!(expanded, "{raw} deveria expandir");
            assert_eq!(p, home.join("Documents/metas.md"));
        }
        let (p, expanded) = expand_home("~", Some(&home));
        assert!(expanded);
        assert_eq!(p, home);
    }

    /// Um `~` no meio do caminho é nome de arquivo legítimo. Reescrever seria
    /// pior que não expandir.
    #[test]
    fn only_expands_a_leading_tilde() {
        let home = PathBuf::from("/home/u");
        for raw in ["/etc/~/passwd", "backup~", "a/~/b", "~user/file"] {
            let (p, expanded) = expand_home(raw, Some(&home));
            assert!(!expanded, "{raw} não deveria expandir");
            assert_eq!(p, PathBuf::from(raw));
        }
    }

    /// Sem HOME o input passa intacto: o erro depois nomeia o caminho literal
    /// com `~`, que é compreensível — um palpite errado não seria.
    #[test]
    fn leaves_tilde_alone_without_a_home() {
        let (p, expanded) = expand_home("~/x", None);
        assert!(!expanded);
        assert_eq!(p, PathBuf::from("~/x"));
    }

    #[test]
    fn resolve_classifies_how_it_got_there() {
        let home = PathBuf::from("/home/u");

        let r = resolve_tool_path("/etc/hosts", None, Some(&home)).unwrap();
        assert_eq!(r.origin, PathOrigin::Absolute);
        assert_eq!(r.path, PathBuf::from("/etc/hosts"));

        let r = resolve_tool_path("~/notes.md", None, Some(&home)).unwrap();
        assert_eq!(r.origin, PathOrigin::HomeExpanded);
        assert_eq!(r.path, home.join("notes.md"));

        let r = resolve_tool_path("src/main.rs", Some("/work/proj"), Some(&home)).unwrap();
        assert_eq!(r.origin, PathOrigin::JoinedWorkingDir);
        assert_eq!(r.path, PathBuf::from("/work/proj/src/main.rs"));

        // O caso que mordeu na #923: relativo, sem working_dir.
        let r = resolve_tool_path("notes.md", None, Some(&home)).unwrap();
        assert_eq!(r.origin, PathOrigin::RelativeToProcessCwd);
        assert_eq!(r.path, PathBuf::from("notes.md"));
    }

    /// `working_dir: Some("")` é o mesmo que ausente — um working_dir vazio
    /// transformaria um caminho relativo em absoluto por acidente.
    #[test]
    fn empty_working_dir_counts_as_absent() {
        let r = resolve_tool_path("notes.md", Some(""), None).unwrap();
        assert_eq!(r.origin, PathOrigin::RelativeToProcessCwd);
        assert_eq!(r.path, PathBuf::from("notes.md"));
    }

    /// A checagem de `..` roda no input CRU, antes da expansão: um `~/../etc`
    /// não pode contrabandear traversal através do home.
    #[test]
    fn rejects_traversal_before_expanding() {
        let home = PathBuf::from("/home/u");
        for raw in ["../etc/passwd", "~/../../etc/passwd", "a/../../b"] {
            assert!(
                resolve_tool_path(raw, Some("/work"), Some(&home)).is_err(),
                "{raw} deveria ser rejeitado"
            );
        }
    }

    /// A mensagem de erro tem de conter o resolvido, o pedido e o porquê —
    /// era exatamente o que faltava para o agente não dizer "não existe".
    #[test]
    fn describe_names_resolution_and_request() {
        let r = resolve_tool_path("notes.md", None, None).unwrap();
        let d = r.describe();
        assert!(d.contains("notes.md"));
        assert!(d.contains("pedido:"));
        assert!(d.contains("working_dir"));
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
