//! GAR-257: Unified repo scanner — walks a directory tree, applies include/exclude
//! glob patterns and `.gitignore`/`.garraignore` rules.
//!
//! ## Coverage logging (GAR-258)
//!
//! Set `RUST_LOG=garraia_glob::scanner=debug` to see per-path decisions:
//!
//! ```text
//! DEBUG garraia_glob::scanner  path="secret.key"  pattern="*.key"  source=".gitignore" → ignored
//! DEBUG garraia_glob::scanner  path="dist/app.js" pattern="dist/**"                    → excluded
//! DEBUG garraia_glob::scanner  path="src/main.rs" pattern="**/*.rs"                    → included
//! ```

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::{
    ignore::IgnoreFile,
    matcher::MatchResult,
    pattern::{GlobConfig, GlobPattern},
    Result, DEFAULT_MAX_DEPTH, DEFAULT_MAX_FILES,
};

/// A builder for configuring and executing a directory scan.
///
/// # Example
/// ```no_run
/// use garraia_glob::scanner::Scanner;
/// use garraia_glob::pattern::GlobConfig;
///
/// let results = Scanner::new("/repo", GlobConfig::default())
///     .include("**/*.rs").unwrap()
///     .exclude("**/target/**").unwrap()
///     .use_gitignore(true)
///     .scan()
///     .unwrap();
/// ```
pub struct Scanner {
    root: PathBuf,
    include: Vec<GlobPattern>,
    exclude: Vec<GlobPattern>,
    ignore_files: Vec<IgnoreFile>,
    use_gitignore: bool,
    use_garraignore: bool,
    max_depth: usize,
    max_files: usize,
    config: GlobConfig,
}

impl Scanner {
    /// Create a new scanner rooted at `root` with the given config.
    pub fn new(root: impl AsRef<Path>, config: GlobConfig) -> Self {
        Scanner {
            root: root.as_ref().to_path_buf(),
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_files: Vec::new(),
            use_gitignore: true,
            use_garraignore: true,
            max_depth: DEFAULT_MAX_DEPTH,
            max_files: DEFAULT_MAX_FILES,
            config,
        }
    }

    /// Add an include pattern. Only paths matching at least one include are returned.
    /// If no include patterns are added, all paths are included (subject to excludes).
    pub fn include(mut self, pattern: &str) -> Result<Self> {
        self.include.push(GlobPattern::new(pattern, &self.config)?);
        Ok(self)
    }

    /// Add an exclude pattern. Paths matching any exclude are omitted.
    pub fn exclude(mut self, pattern: &str) -> Result<Self> {
        self.exclude.push(GlobPattern::new(pattern, &self.config)?);
        Ok(self)
    }

    /// Whether to load and respect `.gitignore` files found during traversal.
    /// Default: `true`.
    pub fn use_gitignore(mut self, val: bool) -> Self {
        self.use_gitignore = val;
        self
    }

    /// Whether to load and respect `.garraignore` files found during traversal.
    /// Default: `true`.
    pub fn use_garraignore(mut self, val: bool) -> Self {
        self.use_garraignore = val;
        self
    }

    /// Override the maximum traversal depth (default: [`DEFAULT_MAX_DEPTH`]).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Override the maximum number of files to process (default: [`DEFAULT_MAX_FILES`]).
    pub fn max_files(mut self, n: usize) -> Self {
        self.max_files = n;
        self
    }

    /// Add an already-parsed [`IgnoreFile`] (for testing or pre-loaded configs).
    pub fn with_ignore(mut self, ignore: IgnoreFile) -> Self {
        self.ignore_files.push(ignore);
        self
    }

    /// Execute the scan and return all matching [`MatchResult`]s.
    pub fn scan(&self) -> Result<Vec<MatchResult>> {
        let mut results = Vec::new();
        let mut count = 0usize;

        // Load root-level ignore files up front.
        let mut root_ignores = self.ignore_files.clone();
        if self.use_gitignore {
            if let Ok(ig) = IgnoreFile::from_path(self.root.join(".gitignore")) {
                root_ignores.push(ig);
            }
        }
        if self.use_garraignore {
            // GAR-255: use from_garraignore_path so extglob patterns are enabled.
            if let Ok(ig) = IgnoreFile::from_garraignore_path(self.root.join(".garraignore")) {
                root_ignores.push(ig);
            }
        }

        for entry in WalkDir::new(&self.root)
            .max_depth(self.max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if count >= self.max_files {
                tracing::warn!(max = self.max_files, "scanner: max_files limit reached");
                break;
            }

            let abs_path = entry.path();
            let rel = abs_path
                .strip_prefix(&self.root)
                .unwrap_or(abs_path)
                .to_string_lossy()
                .replace('\\', "/");

            if rel.is_empty() {
                continue;
            }

            // Load per-directory ignore files (subdirectory .gitignore/.garraignore).
            let mut local_ignores = root_ignores.clone();
            if let Some(parent) = abs_path.parent() {
                if parent != self.root {
                    if self.use_gitignore {
                        if let Ok(ig) = IgnoreFile::from_path(parent.join(".gitignore")) {
                            local_ignores.push(ig);
                        }
                    }
                    if self.use_garraignore {
                        if let Ok(ig) =
                            IgnoreFile::from_garraignore_path(parent.join(".garraignore"))
                        {
                            local_ignores.push(ig);
                        }
                    }
                }
            }

            // ── GAR-258: coverage log — ignore rule with pattern + source attribution ─
            if let Some(ig) = local_ignores.iter().find(|ig| ig.is_ignored(&rel)) {
                let pat = ig.matching_pattern(&rel).unwrap_or("<unknown>");
                let src = ig.source.as_deref().unwrap_or("<pre-loaded>");
                tracing::debug!(
                    path = %rel,
                    pattern = pat,
                    source = src,
                    "scanner: path ignored"
                );
                continue;
            }

            // ── Apply include filter ───────────────────────────────────────────────
            let included = if self.include.is_empty() {
                true
            } else {
                self.include.iter().any(|p| p.matches(&rel))
            };

            if !included {
                continue;
            }

            // ── GAR-258: coverage log — which include pattern matched ──────────────
            if tracing::enabled!(tracing::Level::DEBUG) {
                if let Some(pat) = self.include.iter().find(|p| p.matches(&rel)) {
                    tracing::debug!(
                        path = %rel,
                        pattern = pat.source(),
                        "scanner: path included"
                    );
                }
            }

            // ── Apply exclude filter ───────────────────────────────────────────────
            if let Some(pat) = self.exclude.iter().find(|p| p.matches(&rel)) {
                tracing::debug!(
                    path = %rel,
                    pattern = pat.source(),
                    "scanner: path excluded"
                );
                continue;
            }

            results.push(MatchResult {
                path: rel,
                is_dir: abs_path.is_dir(),
            });
            count += 1;
        }

        Ok(results)
    }

    /// Convenience: scan and return only file paths (no directories).
    pub fn scan_files(&self) -> Result<Vec<String>> {
        Ok(self
            .scan()?
            .into_iter()
            .filter(|r| !r.is_dir)
            .map(|r| r.path)
            .collect())
    }
}

/// Build a [`Scanner`] from config values (GAR-261 integration point).
pub fn scanner_from_config(
    root: impl AsRef<Path>,
    mode_str: &str,
    dot: bool,
    use_gitignore: bool,
) -> Scanner {
    let mode = match mode_str {
        "bash" => crate::pattern::GlobMode::Bash,
        _ => crate::pattern::GlobMode::Picomatch,
    };
    let config = GlobConfig {
        mode,
        dot,
        ..GlobConfig::default()
    };
    Scanner::new(root, config).use_gitignore(use_gitignore)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignore::IgnoreKind;
    use std::fs;
    use tempfile::TempDir;

    fn make_tree(tmp: &TempDir, paths: &[&str]) {
        for p in paths {
            let full = tmp.path().join(p);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, "").unwrap();
        }
    }

    // ── Basic scanner behaviour ────────────────────────────────────────────

    #[test]
    fn scan_all_rs_files() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            &tmp,
            &["src/main.rs", "src/lib.rs", "Cargo.toml", "src/sub/mod.rs"],
        );

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .include("**/*.rs")
            .unwrap()
            .use_gitignore(false)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert_eq!(results.len(), 3);
        assert!(results.iter().any(|p| p == "src/main.rs"));
        assert!(results.iter().any(|p| p == "src/sub/mod.rs"));
    }

    #[test]
    fn exclude_target_directory() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["src/main.rs", "target/debug/garraia.exe"]);

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .include("**/*.rs")
            .unwrap()
            .exclude("target/**")
            .unwrap()
            .use_gitignore(false)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "src/main.rs");
    }

    #[test]
    fn gitignore_respected() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["src/main.rs", "secret.key"]);
        fs::write(tmp.path().join(".gitignore"), "*.key\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(true)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert!(!results.iter().any(|p| p.ends_with(".key")));
        assert!(results.iter().any(|p| p == "src/main.rs"));
    }

    #[test]
    fn no_include_returns_all() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["a.rs", "b.toml", "c.py"]);

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(false)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert_eq!(results.len(), 3);
    }

    // ── GAR-256: precedence & anchoring tests ─────────────────────────────

    /// Unanchored `*.key` in gitignore must match files at any depth.
    #[test]
    fn gitignore_unanchored_matches_subdirectory() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            &tmp,
            &["src/main.rs", "config/prod.key", "config/nested/db.key"],
        );
        fs::write(tmp.path().join(".gitignore"), "*.key\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(true)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert!(!results.iter().any(|p| p.ends_with(".key")));
        assert!(results.iter().any(|p| p == "src/main.rs"));
    }

    /// Negation `!keep.log` re-includes within the same ignore file.
    #[test]
    fn negation_within_file_re_includes() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["error.log", "keep.log", "src/main.rs"]);
        fs::write(tmp.path().join(".gitignore"), "*.log\n!keep.log\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(true)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert!(
            !results.iter().any(|p| p == "error.log"),
            "error.log should be ignored"
        );
        assert!(
            results.iter().any(|p| p == "keep.log"),
            "keep.log should be re-included"
        );
        assert!(results.iter().any(|p| p == "src/main.rs"));
    }

    /// GAR-256: Negation in `.garraignore` does NOT override a `.gitignore` positive.
    /// Each ignore file is evaluated independently — additive, not overriding.
    #[test]
    fn cross_file_negation_does_not_override() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["secret.key", "src/main.rs"]);
        fs::write(tmp.path().join(".gitignore"), "*.key\n").unwrap();
        // garraignore negates — but cannot override another file's positive rule
        fs::write(tmp.path().join(".garraignore"), "!secret.key\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(true)
            .use_garraignore(true)
            .scan_files()
            .unwrap();

        assert!(
            !results.iter().any(|p| p == "secret.key"),
            "gitignore rule must win"
        );
    }

    /// Directory pattern `target/` ignores the dir entry AND all contents.
    #[test]
    fn directory_pattern_ignores_dir_and_contents() {
        let tmp = TempDir::new().unwrap();
        make_tree(
            &tmp,
            &[
                "src/main.rs",
                "target/debug/app.exe",
                "target/release/app.exe",
            ],
        );
        fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(true)
            .use_garraignore(false)
            .scan_files()
            .unwrap();

        assert!(!results.iter().any(|p| p.starts_with("target/")));
        assert!(results.iter().any(|p| p == "src/main.rs"));
    }

    /// GAR-255: `.garraignore` supports extglob — `!(*.rs)` ignores non-rs files.
    #[test]
    fn garraignore_extglob_ignores_non_rs() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["src/main.rs", "Cargo.toml", "README.md"]);
        fs::write(tmp.path().join(".garraignore"), "!(*.rs)\n").unwrap();

        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(false)
            .use_garraignore(true)
            .scan_files()
            .unwrap();

        assert!(results.iter().any(|p| p == "src/main.rs"));
        assert!(!results.iter().any(|p| p == "Cargo.toml"));
        assert!(!results.iter().any(|p| p == "README.md"));
    }

    /// Pre-loaded `IgnoreFile` via `with_ignore` works correctly.
    #[test]
    fn with_ignore_preloaded() {
        let tmp = TempDir::new().unwrap();
        make_tree(&tmp, &["a.rs", "b.txt"]);

        let ig = IgnoreFile::parse("*.txt\n", IgnoreKind::Git);
        let results = Scanner::new(tmp.path(), GlobConfig::default())
            .use_gitignore(false)
            .use_garraignore(false)
            .with_ignore(ig)
            .scan_files()
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "a.rs");
    }
}
