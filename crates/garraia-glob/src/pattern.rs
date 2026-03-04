//! GAR-248/250/251: Glob-to-regex compiler with full extglob support.
//!
//! The Rust `regex` crate uses an NFA/DFA engine — **no backtracking, always linear-time**.
//!
//! ## `!(pat)` implementation (no lookahead)
//!
//! Since `regex` does not support lookahead assertions, `!(pat)` is implemented
//! using **capture groups + post-match negation**:
//! - The main regex captures the segment(s) with `([^/]*)` (Picomatch) or `(.*)` (Bash greedy)
//! - After the main regex matches, each captured segment is checked against the inner pattern
//! - If the inner pattern matches the captured segment → overall match is rejected
//!
//! ## Supported syntax
//! - Standard globs: `*`, `**`, `?`, `[abc]`, `{a,b,c}`
//! - Extglob:  `!(pat)`, `@(pat)`, `?(pat)`, `*(pat)`, `+(pat)`
//! - Escapes:  `\*`, `\?`, etc.

use regex::Regex;

use crate::{GlobError, Result};

/// Glob matching mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum GlobMode {
    /// Picomatch-compatible (default): `*` never crosses `/`, `!(…)` is single-segment.
    #[default]
    Picomatch,
    /// Bash-style extglob; `!(…)` behaviour controlled by `bash_greedy_negated_extglob`.
    Bash,
}

/// Configuration for glob pattern compilation and matching.
#[derive(Debug, Clone)]
pub struct GlobConfig {
    /// Matching mode (default: [`GlobMode::Picomatch`]).
    pub mode: GlobMode,
    /// If `false` (default), `*` / `?` do NOT implicitly match dotfiles at segment start.
    pub dot: bool,
    /// **Bash mode only.** When `true`, `!(pat)` can match across `/` separators.
    /// Default `false` (safe, single-segment behaviour).
    pub bash_greedy_negated_extglob: bool,
    /// Case-sensitive matching (default: `true`).
    pub case_sensitive: bool,
}

impl Default for GlobConfig {
    fn default() -> Self {
        GlobConfig {
            mode: GlobMode::Picomatch,
            dot: false,
            bash_greedy_negated_extglob: false,
            case_sensitive: true,
        }
    }
}

/// A compiled glob pattern — always linear-time, no backtracking.
///
/// `!(pat)` is handled via capture groups + post-match negation; no lookahead is used.
#[derive(Debug, Clone)]
pub struct GlobPattern {
    source: String,
    main_regex: Regex,
    /// `(capture_group_1_index, inner_regex)` — one entry per `!(…)` in the pattern.
    negations: Vec<(usize, Regex)>,
    dot: bool,
}

impl GlobPattern {
    /// Compile a glob pattern with the given [`GlobConfig`].
    pub fn new(pattern: &str, config: &GlobConfig) -> Result<Self> {
        let mut compiler = Compiler::new(config);
        let re_str = compiler.compile(pattern)?;

        let main_regex = Regex::new(&re_str)
            .map_err(|e| GlobError::InvalidPattern(format!("{pattern}: {e}")))?;

        // Compile inner negation patterns into Regex.
        let negations = compiler
            .negations
            .into_iter()
            .map(|(group_idx, inner_str)| {
                let re = Regex::new(&inner_str)
                    .map_err(|e| GlobError::InvalidPattern(format!("negation in '{pattern}': {e}")))?;
                Ok((group_idx, re))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(GlobPattern {
            source: pattern.to_string(),
            main_regex,
            negations,
            dot: config.dot,
        })
    }

    /// Returns `true` if `path` matches this pattern.
    pub fn matches(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");

        // Dot-file guard: reject hidden-component paths unless dot=true or pattern is explicit.
        if !self.dot
            && has_hidden_component(&normalized)
            && !self.source.starts_with('.')
            && !self.source.starts_with("**/")
            && self.source != "**"
        {
            return false;
        }

        // Main structural match.
        let caps = match self.main_regex.captures(&normalized) {
            Some(c) => c,
            None => return false,
        };

        // Post-match negation checks: each captured segment must NOT match its inner pattern.
        for (group_idx, inner_re) in &self.negations {
            let segment = caps.get(*group_idx).map_or("", |m| m.as_str());
            if inner_re.is_match(segment) {
                return false;
            }
        }

        true
    }

    /// The original source pattern string.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The compiled main regex string (useful for debugging).
    pub fn regex_str(&self) -> &str {
        self.main_regex.as_str()
    }
}

fn has_hidden_component(path: &str) -> bool {
    path.split('/').any(|seg| seg.starts_with('.') && seg.len() > 1)
}

// ── Compiler ─────────────────────────────────────────────────────────────────

struct Compiler<'a> {
    config: &'a GlobConfig,
    /// Next 1-based capture group index (group 0 = whole match).
    next_group: usize,
    /// Pending negations: (group_index, inner_re_str).
    negations: Vec<(usize, String)>,
    /// When `true`, bare `*` compiles to `.*` (matches slashes) instead of `[^/]*`.
    ///
    /// Activated while compiling the **inner** pattern of a greedy `!(…)` extglob so
    /// that `!(*.txt)` correctly rejects paths like `docs/notes.txt` (the inner `*.txt`
    /// must be able to match across path separators to cover the full captured segment).
    greedy_star: bool,
}

impl<'a> Compiler<'a> {
    fn new(config: &'a GlobConfig) -> Self {
        Compiler { config, next_group: 1, negations: Vec::new(), greedy_star: false }
    }

    fn compile(&mut self, pattern: &str) -> Result<String> {
        let prefix = if self.config.case_sensitive { "^" } else { "(?i)^" };
        let mut out = String::from(prefix);
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            i = self.compile_one(&chars, i, &mut out)?;
        }
        out.push('$');
        Ok(out)
    }

    fn compile_one(&mut self, chars: &[char], i: usize, out: &mut String) -> Result<usize> {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        match (c, next) {
            // ── Escape ──────────────────────────────────────────────────
            ('\\', Some(nc)) => {
                push_literal(out, nc);
                Ok(i + 2)
            }

            // ── Extglob ?(…) ────────────────────────────────────────────
            ('?', Some('(')) => {
                let (inner, consumed) = parse_extglob(chars, i + 2)?;
                let inner_re = self.compile_extglob_inner(&inner)?;
                out.push_str(&format!("(?:{inner_re})?"));
                Ok(i + 2 + consumed + 1)
            }

            // ── Extglob *(…) ────────────────────────────────────────────
            ('*', Some('(')) => {
                let (inner, consumed) = parse_extglob(chars, i + 2)?;
                let inner_re = self.compile_extglob_inner(&inner)?;
                out.push_str(&format!("(?:{inner_re})*"));
                Ok(i + 2 + consumed + 1)
            }

            // ── Extglob +(…) ────────────────────────────────────────────
            ('+', Some('(')) => {
                let (inner, consumed) = parse_extglob(chars, i + 2)?;
                let inner_re = self.compile_extglob_inner(&inner)?;
                out.push_str(&format!("(?:{inner_re})+"));
                Ok(i + 2 + consumed + 1)
            }

            // ── Extglob @(…) ────────────────────────────────────────────
            ('@', Some('(')) => {
                let (inner, consumed) = parse_extglob(chars, i + 2)?;
                let inner_re = self.compile_extglob_inner(&inner)?;
                out.push_str(&format!("(?:{inner_re})"));
                Ok(i + 2 + consumed + 1)
            }

            // ── Extglob !(…) — GAR-248/251 ───────────────────────────────
            // Strategy: emit a capturing group; record negation for post-match check.
            ('!', Some('(')) => {
                let (inner, consumed) = parse_extglob(chars, i + 2)?;

                // In greedy bash mode, the inner pattern must also cross `/` so that
                // e.g. `!(*.txt)` correctly rejects `docs/notes.txt` (the captured `(.*)`
                // group holds `docs/notes.txt`, and the inner `*.txt` needs `.*\.txt`
                // rather than `[^/]*\.txt` to match it).
                let is_greedy = self.config.mode == GlobMode::Bash
                    && self.config.bash_greedy_negated_extglob;

                let saved = self.greedy_star;
                self.greedy_star = is_greedy;
                let inner_re = self.compile_extglob_inner(&inner)?;
                self.greedy_star = saved;

                let segment_re = if is_greedy { "(.*)" } else { "([^/]*)" };
                let group_idx = self.next_group;
                self.next_group += 1;

                out.push_str(segment_re);

                // The captured segment must NOT fully match inner_re.
                self.negations.push((group_idx, format!("^(?:{inner_re})$")));

                Ok(i + 2 + consumed + 1)
            }

            // ── Double-star ** ───────────────────────────────────────────
            ('*', Some('*')) => {
                let after_slash = chars.get(i + 2).copied() == Some('/');
                if after_slash {
                    // **/ → zero or more path segments
                    out.push_str("(?:.*/)?");
                    Ok(i + 3)
                } else {
                    // ** at end → everything
                    out.push_str(".*");
                    Ok(i + 2)
                }
            }

            // ── Single-star * ────────────────────────────────────────────
            ('*', _) => {
                // In greedy_star mode (inner of a greedy `!(…)`), * matches slashes.
                out.push_str(if self.greedy_star { ".*" } else { "[^/]*" });
                Ok(i + 1)
            }

            // ── Single-char ? ────────────────────────────────────────────
            ('?', _) => {
                out.push_str("[^/]");
                Ok(i + 1)
            }

            // ── Character class [abc] ────────────────────────────────────
            ('[', _) => {
                let (class_re, consumed) = parse_char_class(chars, i)?;
                out.push_str(&class_re);
                Ok(i + consumed)
            }

            // ── Brace expansion {a,b,c} ──────────────────────────────────
            ('{', _) => {
                let (alts, consumed) = parse_brace(chars, i)?;
                out.push('(');
                for (idx, alt) in alts.iter().enumerate() {
                    if idx > 0 {
                        out.push('|');
                    }
                    let alt_chars: Vec<char> = alt.chars().collect();
                    let mut j = 0;
                    while j < alt_chars.len() {
                        j = self.compile_one(&alt_chars, j, out)?;
                    }
                }
                out.push(')');
                self.next_group += 1; // brace expansion uses a capturing group
                Ok(i + consumed)
            }

            // ── Regex metacharacters ─────────────────────────────────────
            ('.', _) | ('^', _) | ('$', _) | ('+', _) | ('|', _)
            | ('(', _) | (')', _) | ('\\', _) => {
                push_literal(out, c);
                Ok(i + 1)
            }

            // ── Literal ──────────────────────────────────────────────────
            _ => {
                out.push(c);
                Ok(i + 1)
            }
        }
    }

    fn compile_extglob_inner(&mut self, inner: &str) -> Result<String> {
        let parts: Vec<&str> = inner.split('|').collect();
        if parts.len() == 1 {
            let chars: Vec<char> = inner.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            while i < chars.len() {
                i = self.compile_one(&chars, i, &mut out)?;
            }
            Ok(out)
        } else {
            let mut out = String::from("(?:");
            for (idx, part) in parts.iter().enumerate() {
                if idx > 0 {
                    out.push('|');
                }
                let chars: Vec<char> = part.chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    j = self.compile_one(&chars, j, &mut out)?;
                }
            }
            out.push(')');
            Ok(out)
        }
    }
}

fn push_literal(out: &mut String, c: char) {
    match c {
        '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']'
        | '{' | '}' | '\\' | '|' => {
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
    }
}

fn parse_extglob(chars: &[char], start: usize) -> Result<(String, usize)> {
    let mut depth: i32 = 1;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[start..i].iter().collect();
                    return Ok((inner, i - start));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(GlobError::InvalidPattern("unclosed extglob '('".into()))
}

fn parse_char_class(chars: &[char], start: usize) -> Result<(String, usize)> {
    let mut i = start + 1;
    let mut class = String::from("[");

    if chars.get(i).copied() == Some('!') {
        class.push('^');
        i += 1;
    }
    if chars.get(i).copied() == Some(']') {
        class.push(']');
        i += 1;
    }

    while i < chars.len() {
        let c = chars[i];
        class.push(c);
        i += 1;
        if c == ']' {
            return Ok((class, i - start));
        }
    }
    Err(GlobError::InvalidPattern("unclosed character class '['".into()))
}

fn parse_brace(chars: &[char], start: usize) -> Result<(Vec<String>, usize)> {
    let mut i = start + 1;
    let mut depth: i32 = 1;
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();

    while i < chars.len() {
        match chars[i] {
            '{' => {
                depth += 1;
                current.push('{');
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    parts.push(current.clone());
                    return Ok((parts, i - start + 1));
                }
                current.push('}');
            }
            ',' if depth == 1 => {
                parts.push(current.clone());
                current.clear();
            }
            c => current.push(c),
        }
        i += 1;
    }
    Err(GlobError::InvalidPattern("unclosed brace expansion '{'".into()))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pm(pat: &str) -> GlobPattern {
        GlobPattern::new(pat, &GlobConfig::default()).unwrap()
    }

    fn pm_dot(pat: &str) -> GlobPattern {
        GlobPattern::new(pat, &GlobConfig { dot: true, ..GlobConfig::default() }).unwrap()
    }

    fn bash(pat: &str) -> GlobPattern {
        GlobPattern::new(pat, &GlobConfig { mode: GlobMode::Bash, ..GlobConfig::default() }).unwrap()
    }

    fn bash_greedy(pat: &str) -> GlobPattern {
        GlobPattern::new(
            pat,
            &GlobConfig {
                mode: GlobMode::Bash,
                bash_greedy_negated_extglob: true,
                ..GlobConfig::default()
            },
        )
        .unwrap()
    }

    // ── Standard glob ────────────────────────────────────────────────────

    #[test]
    fn star_does_not_cross_separator() {
        let p = pm("src/*.rs");
        assert!(p.matches("src/main.rs"));
        assert!(!p.matches("src/a/main.rs"));
    }

    #[test]
    fn double_star_recursive() {
        let p = pm("src/**/*.rs");
        assert!(p.matches("src/main.rs"));
        assert!(p.matches("src/a/main.rs"));
        assert!(p.matches("src/a/b/c/main.rs"));
        assert!(!p.matches("lib/main.rs"));
    }

    #[test]
    fn double_star_standalone() {
        let p = pm("**");
        assert!(p.matches("anything"));
        assert!(p.matches("a/b/c"));
    }

    #[test]
    fn double_star_prefix() {
        let p = pm("**/*.rs");
        assert!(p.matches("main.rs"));
        assert!(p.matches("src/main.rs"));
        assert!(p.matches("a/b/main.rs"));
        assert!(!p.matches("main.py"));
    }

    #[test]
    fn question_mark() {
        let p = pm("src/?.rs");
        assert!(p.matches("src/a.rs"));
        assert!(!p.matches("src/ab.rs"));
        assert!(!p.matches("src/a/b.rs"));
    }

    #[test]
    fn brace_expansion() {
        let p = pm("*.{rs,toml}");
        assert!(p.matches("Cargo.toml"));
        assert!(p.matches("main.rs"));
        assert!(!p.matches("main.py"));
    }

    #[test]
    fn char_class() {
        let p = pm("[abc].rs");
        assert!(p.matches("a.rs"));
        assert!(p.matches("b.rs"));
        assert!(!p.matches("d.rs"));
    }

    #[test]
    fn negated_char_class() {
        let p = pm("[!abc].rs");
        assert!(p.matches("d.rs"));
        assert!(!p.matches("a.rs"));
    }

    // ── Extglob ──────────────────────────────────────────────────────────

    #[test]
    fn extglob_at_exact() {
        let p = pm("@(foo|bar).rs");
        assert!(p.matches("foo.rs"));
        assert!(p.matches("bar.rs"));
        assert!(!p.matches("baz.rs"));
        assert!(!p.matches("foobar.rs"));
    }

    #[test]
    fn extglob_question_optional() {
        let p = pm("?(foo)bar.rs");
        assert!(p.matches("foobar.rs"));
        assert!(p.matches("bar.rs"));
        assert!(!p.matches("foofoobar.rs"));
    }

    #[test]
    fn extglob_star_zero_or_more() {
        let p = pm("*(foo)test.rs");
        assert!(p.matches("test.rs"));      // zero occurrences
        assert!(p.matches("footest.rs"));   // one
        assert!(p.matches("foofootest.rs")); // two
        assert!(!p.matches("bartest.rs"));
    }

    #[test]
    fn extglob_plus_one_or_more() {
        let p = pm("+(foo)test.rs");
        assert!(p.matches("footest.rs"));
        assert!(p.matches("foofootest.rs"));
        assert!(!p.matches("test.rs")); // zero not allowed
    }

    #[test]
    fn extglob_bang_picomatch_single_segment() {
        let p = pm("!(foo)");
        assert!(p.matches("bar"));
        assert!(p.matches("foobar"));
        assert!(!p.matches("foo"));
    }

    #[test]
    fn extglob_bang_picomatch_does_not_cross_slash() {
        let p = pm("src/!(test)");
        assert!(p.matches("src/main"));
        assert!(!p.matches("src/test"));
        assert!(!p.matches("src/a/main")); // !(test) won't match "a/main" — [^/]* stops at /
    }

    #[test]
    fn extglob_bang_bash_greedy_crosses_slash() {
        let p = bash_greedy("!(*.txt)");
        assert!(p.matches("main.rs"));
        assert!(p.matches("a/b/main.rs"));
        assert!(!p.matches("main.txt"));
    }

    #[test]
    fn extglob_bang_bash_safe_single_segment() {
        let p = bash("src/!(test)");
        assert!(p.matches("src/main"));
        assert!(!p.matches("src/test"));
        assert!(!p.matches("src/a/main"));
    }

    #[test]
    fn extglob_bang_alternation() {
        let p = pm("!(foo|bar)");
        assert!(p.matches("baz"));
        assert!(!p.matches("foo"));
        assert!(!p.matches("bar"));
    }

    // ── Dotfile handling ─────────────────────────────────────────────────

    #[test]
    fn star_does_not_match_dotfile_by_default() {
        let p = pm("*.rs");
        assert!(!p.matches(".hidden.rs"));
    }

    #[test]
    fn star_matches_dotfile_with_dot_true() {
        let p = pm_dot("*.rs");
        assert!(p.matches(".hidden.rs"));
    }

    // ── Windows path normalisation ────────────────────────────────────────

    #[test]
    fn windows_paths_normalised() {
        let p = pm("src/**/*.rs");
        assert!(p.matches("src\\a\\b\\main.rs"));
    }

    // ── Case insensitive ─────────────────────────────────────────────────

    #[test]
    fn case_insensitive() {
        let p = GlobPattern::new(
            "*.RS",
            &GlobConfig { case_sensitive: false, ..GlobConfig::default() },
        )
        .unwrap();
        assert!(p.matches("main.rs"));
        assert!(p.matches("MAIN.RS"));
    }
}
