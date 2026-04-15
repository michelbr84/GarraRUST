//! Ignore-file support — `.gitignore` and `.garraignore`.
//!
//! ## Pattern semantics (GAR-254)
//!
//! Both file types follow [gitignore(5)](https://git-scm.com/docs/gitignore) conventions:
//!
//! - **Blank lines** and lines beginning with `#` are ignored (comments).
//! - A pattern that **does not contain `/`** is unanchored: it is matched against the
//!   path using a `**/` prefix, so `*.key` ignores `secret.key` *and* `a/b/secret.key`.
//! - A pattern **containing `/`** is anchored: matched against the full relative path
//!   from the directory that contains the ignore file.
//! - A leading `/` anchors the pattern to the root (the `/` is stripped before matching).
//! - A trailing `/` marks a directory-only pattern (the `/` is stripped; both the
//!   directory entry and all paths beneath it are ignored via a `/**` companion pattern).
//! - A leading `!` **negates** the pattern: a previously-ignored path is re-included.
//!   Negation is evaluated within a single file — patterns in other files are not affected.
//!
//! ## `.garraignore` additions (GAR-255)
//!
//! `.garraignore` supports all `.gitignore` syntax **plus** Bash-style extglob patterns:
//! `!(pat)`, `@(pat)`, `?(pat)`, `*(pat)`, `+(pat)`.
//!
//! Parse `.garraignore` files with [`IgnoreFile::from_garraignore_path`] (or set
//! `kind = IgnoreKind::Garra` when calling [`IgnoreFile::parse`]).
//!
//! Example — ignore everything except Rust sources:
//! ```ignore
//! !(*.rs)
//! ```
//!
//! ## Precedence between ignore files (GAR-256)
//!
//! When the [`Scanner`](crate::scanner::Scanner) loads multiple ignore files, each file
//! is evaluated **independently**.  A path is excluded when *any* active file marks it
//! as ignored.  Negation patterns inside one file do **not** override positive patterns
//! from another file.
//!
//! To explicitly allow a path that would otherwise be ignored, use
//! [`Scanner::exclude`](crate::scanner::Scanner) with an `!`-prefixed pattern or
//! simply avoid adding conflicting ignore files.

use std::path::Path;

use crate::{
    pattern::{GlobConfig, GlobPattern},
    Result,
};

// ── IgnoreKind ────────────────────────────────────────────────────────────────

/// Distinguishes whether an [`IgnoreFile`] was loaded from `.gitignore` or `.garraignore`.
///
/// | Kind | Extglob support |
/// |------|----------------|
/// | [`IgnoreKind::Git`]   | No — standard glob only |
/// | [`IgnoreKind::Garra`] | Yes — `!(…)`, `*(…)`, `+(…)`, `@(…)`, `?(…)` |
///
/// Both kinds apply the same anchoring and negation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreKind {
    /// `.gitignore` — standard glob patterns only.
    Git,
    /// `.garraignore` — standard globs plus Bash extglob.
    Garra,
}

// ── IgnoreFile ────────────────────────────────────────────────────────────────

/// A parsed `.gitignore` or `.garraignore` file.
///
/// Patterns are compiled once at construction time; [`is_ignored`](IgnoreFile::is_ignored)
/// performs only regex-execution (no allocation).
///
/// # Anchoring
///
/// | Raw pattern | Effective match rule |
/// |-------------|----------------------|
/// | `*.key`     | `**/*.key`  — matches at any depth |
/// | `target/`   | `**/target` or `**/target/**` — dir and its contents |
/// | `src/lib.rs`| `src/lib.rs` — full path from root |
/// | `/dist`     | `dist`  — anchored to root only |
///
/// # Dotfile matching
///
/// Ignore files implicitly match dotfiles (`.env`, `.secret.key`).
/// The `dot = true` config is applied during compilation.
///
/// # Example
///
/// ```
/// use garraia_glob::ignore::{IgnoreFile, IgnoreKind};
///
/// let ig = IgnoreFile::parse("*.key\n!important.key\n", IgnoreKind::Git);
/// assert!(ig.is_ignored("secret.key"));
/// assert!(ig.is_ignored("nested/dir/secret.key")); // unanchored: anywhere
/// assert!(!ig.is_ignored("important.key"));          // negated
/// ```
#[derive(Clone)]
pub struct IgnoreFile {
    /// Positive patterns — (entry pattern, contents pattern under that entry).
    positive: Vec<(GlobPattern, GlobPattern)>,
    /// Negation patterns — same dual structure.
    negated_compiled: Vec<(GlobPattern, GlobPattern)>,
    /// Original positive strings (for [`Self::patterns`]).
    raw_patterns: Vec<String>,
    /// Original negation strings without `!` (for [`Self::negated`]).
    raw_negated: Vec<String>,
    /// Whether extglob syntax is active for this file.
    kind: IgnoreKind,
    /// Human-readable source path (used in debug logs — see GAR-258).
    pub source: Option<String>,
}

impl IgnoreFile {
    /// Parse ignore rules from a string.
    ///
    /// `kind` controls whether extglob syntax is recognised:
    /// - [`IgnoreKind::Git`]   — plain glob only
    /// - [`IgnoreKind::Garra`] — extglob enabled
    pub fn parse(contents: &str, kind: IgnoreKind) -> Self {
        let config = ignore_config();

        let mut raw_patterns = Vec::new();
        let mut raw_negated = Vec::new();
        let mut positive = Vec::new();
        let mut negated_compiled = Vec::new();

        for line in contents.lines() {
            let trimmed = line.trim();

            // Blank lines and comments.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Distinguish gitignore `!pat` negation from extglob `!(pat)` positive pattern.
            //
            // In `.garraignore` (Garra kind), `!(` is the extglob negation operator and
            // takes precedence — the pattern is positive.  All other `!…` are gitignore
            // negation markers.  In `.gitignore` (Git kind), `!` is always a negation.
            let is_negation = trimmed.starts_with('!')
                && !(kind == IgnoreKind::Garra && trimmed.starts_with("!("));

            if is_negation {
                let neg = &trimmed[1..];
                if let Some(compiled) = compile_pair(neg, &config) {
                    raw_negated.push(neg.to_string());
                    negated_compiled.push(compiled);
                }
            } else if let Some(compiled) = compile_pair(trimmed, &config) {
                raw_patterns.push(trimmed.to_string());
                positive.push(compiled);
            }
        }

        Self {
            positive,
            negated_compiled,
            raw_patterns,
            raw_negated,
            kind,
            source: None,
        }
    }

    /// Parse a `.gitignore` file from disk (standard glob patterns).
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let contents = std::fs::read_to_string(path_ref)?;
        let mut ig = Self::parse(&contents, IgnoreKind::Git);
        ig.source = Some(path_ref.to_string_lossy().replace('\\', "/"));
        Ok(ig)
    }

    /// Parse a `.garraignore` file from disk (extglob patterns enabled — GAR-255).
    ///
    /// In addition to all standard gitignore patterns, `.garraignore` files may use
    /// Bash-style extglob: `!(pat)`, `*(pat)`, `+(pat)`, `@(pat)`, `?(pat)`.
    pub fn from_garraignore_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let contents = std::fs::read_to_string(path_ref)?;
        let mut ig = Self::parse(&contents, IgnoreKind::Garra);
        ig.source = Some(path_ref.to_string_lossy().replace('\\', "/"));
        Ok(ig)
    }

    /// Returns `true` if `path` (relative, forward-slash normalised) should be ignored.
    ///
    /// A path is ignored when it matches a positive pattern **and** is not matched by
    /// any negation pattern in the same file.
    pub fn is_ignored(&self, path: &str) -> bool {
        for (entry_pat, contents_pat) in &self.positive {
            if entry_pat.matches(path) || contents_pat.matches(path) {
                let negated = self
                    .negated_compiled
                    .iter()
                    .any(|(ep, cp)| ep.matches(path) || cp.matches(path));
                if !negated {
                    return true;
                }
            }
        }
        false
    }

    /// Returns `Some(source)` of the first positive pattern that matches `path`,
    /// together with the pattern string — useful for coverage logs (GAR-258).
    pub fn matching_pattern(&self, path: &str) -> Option<&str> {
        self.positive
            .iter()
            .zip(self.raw_patterns.iter())
            .find(|((ep, cp), _)| ep.matches(path) || cp.matches(path))
            .map(|(_, raw)| raw.as_str())
    }

    /// Original positive pattern strings (without leading `#` comment lines).
    pub fn patterns(&self) -> &[String] {
        &self.raw_patterns
    }

    /// Original negation strings (without the leading `!`).
    pub fn negated(&self) -> &[String] {
        &self.raw_negated
    }

    /// The [`IgnoreKind`] of this file.
    pub fn kind(&self) -> IgnoreKind {
        self.kind
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// `GlobConfig` used for all ignore-pattern compilation.
///
/// `dot = true` so patterns like `*.env` or `*.key` match dotfiles such as `.env`.
fn ignore_config() -> GlobConfig {
    GlobConfig {
        dot: true,
        ..GlobConfig::default()
    }
}

/// Compile a raw ignore pattern into an (entry, contents) pair.
///
/// The *entry* pattern matches the file/dir itself.
/// The *contents* pattern matches everything beneath a matched directory.
///
/// For extglob patterns (`!(…)`, `*(…)`, etc.) we do NOT append `/**`, because the
/// `{base}/**` expansion would incorrectly capture directory path components as extglob
/// segments — e.g. `**/!(*.rs)/**` against `src/main.rs` would capture `src` (not `.rs`)
/// and erroneously mark `main.rs` as ignored.  Instead both entry and contents use the
/// same compiled pattern so per-path matching is always correct.
fn compile_pair(raw: &str, config: &GlobConfig) -> Option<(GlobPattern, GlobPattern)> {
    let base = anchor(raw);
    let entry = GlobPattern::new(&base, config).ok()?;
    let contents = if has_extglob(raw) {
        // Extglob handles its own semantics — reuse the entry pattern.
        entry.clone()
    } else {
        GlobPattern::new(&format!("{base}/**"), config).ok()?
    };
    Some((entry, contents))
}

/// Returns `true` if `pattern` contains any extglob operator (`!(`, `*(`, `+(`, `@(`, `?(`).
fn has_extglob(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    chars
        .windows(2)
        .any(|w| matches!(w[0], '!' | '*' | '+' | '@' | '?') && w[1] == '(')
}

/// Apply gitignore-style anchoring to a raw pattern.
///
/// | Input          | Output (effective) | Notes                         |
/// |----------------|--------------------|-------------------------------|
/// | `*.key`        | `**/*.key`         | Unanchored — any depth        |
/// | `target/`      | `**/target`        | Trailing `/` stripped         |
/// | `src/lib.rs`   | `src/lib.rs`       | Contains `/` — anchored       |
/// | `/dist`        | `dist`             | Leading `/` stripped (root)   |
fn anchor(pattern: &str) -> String {
    // Strip trailing `/` (directory marker — handled by the contents pattern).
    let pat = pattern.trim_end_matches('/');
    // Strip leading `/` (root anchor — already anchored once we drop `**/`).
    if let Some(stripped) = pat.strip_prefix('/') {
        stripped.to_string()
    } else if pat.contains('/') {
        // Already contains a separator — anchored to root as-is.
        pat.to_string()
    } else {
        // No separator — match at any depth.
        format!("**/{pat}")
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn git(contents: &str) -> IgnoreFile {
        IgnoreFile::parse(contents, IgnoreKind::Git)
    }

    fn garra(contents: &str) -> IgnoreFile {
        IgnoreFile::parse(contents, IgnoreKind::Garra)
    }

    // ── anchor helper ──────────────────────────────────────────────────────

    #[test]
    fn anchor_unanchored() {
        assert_eq!(anchor("*.key"), "**/*.key");
    }

    #[test]
    fn anchor_trailing_slash() {
        assert_eq!(anchor("target/"), "**/target");
    }

    #[test]
    fn anchor_with_slash() {
        assert_eq!(anchor("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn anchor_leading_slash() {
        assert_eq!(anchor("/dist"), "dist");
    }

    // ── is_ignored ─────────────────────────────────────────────────────────

    #[test]
    fn simple_extension_pattern() {
        let ig = git("*.key\n");
        assert!(ig.is_ignored("secret.key"));
        assert!(!ig.is_ignored("main.rs"));
    }

    #[test]
    fn unanchored_matches_subdirectory() {
        let ig = git("*.key\n");
        // Must match at any depth (gitignore-style anchoring fix — GAR-255).
        assert!(ig.is_ignored("a/b/secret.key"));
        assert!(ig.is_ignored("nested/very/deep/file.key"));
    }

    #[test]
    fn negation_re_includes_within_same_file() {
        let ig = git("*.log\n!keep.log\n");
        assert!(ig.is_ignored("error.log"));
        assert!(!ig.is_ignored("keep.log")); // negated
    }

    #[test]
    fn directory_pattern_ignores_contents() {
        let ig = git("target/\n");
        assert!(ig.is_ignored("target")); // the dir entry itself
        assert!(ig.is_ignored("target/debug/foo.exe")); // contents beneath
    }

    #[test]
    fn anchored_pattern() {
        let ig = git("src/generated.rs\n");
        assert!(ig.is_ignored("src/generated.rs"));
        assert!(!ig.is_ignored("lib/generated.rs")); // anchored — different root
    }

    #[test]
    fn root_anchored_leading_slash() {
        let ig = git("/dist\n");
        assert!(ig.is_ignored("dist"));
        assert!(!ig.is_ignored("a/dist")); // rooted — only at root
    }

    #[test]
    fn dotfile_matched_by_default() {
        let ig = git("*.env\n");
        assert!(ig.is_ignored(".env"));
        assert!(ig.is_ignored("a/b/.env"));
    }

    #[test]
    fn comment_and_blank_lines_skipped() {
        let ig = git("# This is a comment\n\n*.tmp\n");
        assert_eq!(ig.patterns().len(), 1);
        assert!(ig.is_ignored("scratch.tmp"));
    }

    // ── GAR-255: .garraignore extglob ─────────────────────────────────────

    #[test]
    fn garraignore_extglob_bang() {
        // !(*.rs) — ignore everything except .rs files
        let ig = garra("!(*.rs)\n");
        assert!(ig.is_ignored("Cargo.toml")); // not .rs → ignored
        assert!(!ig.is_ignored("main.rs")); // .rs → NOT ignored
    }

    #[test]
    fn garraignore_extglob_star() {
        // *(log) — matches zero or more "log" repetitions
        let ig = garra("*(log).txt\n");
        assert!(ig.is_ignored(".txt")); // zero logs → **/.txt
        assert!(ig.is_ignored("log.txt"));
        assert!(ig.is_ignored("loglog.txt"));
    }
}
