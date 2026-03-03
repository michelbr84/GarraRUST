//! GAR-249/265: Known test vectors — cases from the Picomatch README and Bash extglob manual.
//!
//! References:
//! - <https://github.com/micromatch/picomatch#globbing-features>
//! - GNU Bash manual — extglob section

use garraia_glob::pattern::{GlobConfig, GlobMode, GlobPattern};

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
    GlobPattern::new(pat, &GlobConfig {
        mode: GlobMode::Bash,
        bash_greedy_negated_extglob: true,
        ..GlobConfig::default()
    }).unwrap()
}

// ── Picomatch — standard globs ─────────────────────────────────────────────

#[test]
fn pm_star_matches_basename() {
    let p = pm("*.js");
    assert!(p.matches("foo.js"));
    assert!(p.matches("bar.js"));
    assert!(!p.matches("foo.rs"));
    assert!(!p.matches("dir/foo.js"), "* should not cross /");
}

#[test]
fn pm_double_star_recursive() {
    let p = pm("**/*.js");
    assert!(p.matches("foo.js"));
    assert!(p.matches("dir/foo.js"));
    assert!(p.matches("a/b/c/foo.js"));
    assert!(!p.matches("foo.ts"));
}

#[test]
fn pm_double_star_middle() {
    let p = pm("foo/**/bar.js");
    assert!(p.matches("foo/bar.js"));
    assert!(p.matches("foo/a/bar.js"));
    assert!(p.matches("foo/a/b/bar.js"));
    assert!(!p.matches("baz/bar.js"));
}

#[test]
fn pm_question_mark_single_char() {
    let p = pm("?.js");
    assert!(p.matches("a.js"));
    assert!(!p.matches("ab.js"));
    assert!(!p.matches("a/b.js"));
}

#[test]
fn pm_char_class() {
    let p = pm("[abc].js");
    assert!(p.matches("a.js"));
    assert!(p.matches("c.js"));
    assert!(!p.matches("d.js"));
}

#[test]
fn pm_negated_char_class() {
    let p = pm("[!abc].js");
    assert!(p.matches("d.js"));
    assert!(!p.matches("a.js"));
}

#[test]
fn pm_brace_expansion() {
    let p = pm("*.{js,ts}");
    assert!(p.matches("foo.js"));
    assert!(p.matches("foo.ts"));
    assert!(!p.matches("foo.rs"));
}

#[test]
fn pm_brace_in_path() {
    let p = pm("src/{lib,bin}/**");
    assert!(p.matches("src/lib/a.rs"));
    assert!(p.matches("src/bin/main.rs"));
    assert!(!p.matches("src/other/a.rs"));
}

// ── Picomatch — dotfiles ───────────────────────────────────────────────────

#[test]
fn pm_star_does_not_match_dotfile_by_default() {
    let p = pm("*.js");
    // dot=false: * should not match hidden files
    assert!(!p.matches(".hidden.js"));
}

#[test]
fn pm_star_matches_dotfile_with_dot_true() {
    let p = pm_dot("*.js");
    assert!(p.matches(".hidden.js"));
}

#[test]
fn pm_explicit_dot_pattern_matches_dotfile() {
    let p = pm(".*.js");
    assert!(p.matches(".hidden.js"));
    assert!(!p.matches("visible.js"));
}

// ── Picomatch — extglob !(…) ───────────────────────────────────────────────

#[test]
fn pm_bang_basic_negation() {
    let p = pm("!(foo)");
    assert!(p.matches("bar"));
    assert!(p.matches("baz"));
    assert!(!p.matches("foo"));
}

#[test]
fn pm_bang_wildcard_inner() {
    let p = pm("!(*.js)");
    assert!(p.matches("foo.ts"));
    assert!(p.matches("README.md"));
    assert!(!p.matches("foo.js"));
}

#[test]
fn pm_bang_alternation_in_inner() {
    let p = pm("!(foo|bar)");
    assert!(p.matches("baz"));
    assert!(!p.matches("foo"));
    assert!(!p.matches("bar"));
}

#[test]
fn pm_bang_single_segment_only() {
    // !(test) in a path context only applies to a single segment
    let p = pm("src/!(test)");
    assert!(p.matches("src/main"));
    assert!(p.matches("src/util"));
    assert!(!p.matches("src/test"));
    assert!(!p.matches("src/a/main"), "should not cross /");
}

// ── Picomatch — extglob @(…) ───────────────────────────────────────────────

#[test]
fn pm_at_exact_match() {
    let p = pm("@(foo|bar).js");
    assert!(p.matches("foo.js"));
    assert!(p.matches("bar.js"));
    assert!(!p.matches("baz.js"));
    assert!(!p.matches("foobar.js"));
}

// ── Picomatch — extglob ?(…) ───────────────────────────────────────────────

#[test]
fn pm_question_optional() {
    let p = pm("?(foo)bar.js");
    assert!(p.matches("bar.js"));
    assert!(p.matches("foobar.js"));
    assert!(!p.matches("foofoobar.js"));
}

// ── Picomatch — extglob *(…) ───────────────────────────────────────────────

#[test]
fn pm_star_extglob_zero_or_more() {
    // Use non-dotfile baseline so dot=false doesn't interfere
    let p = pm("*(foo)test.js");
    assert!(p.matches("test.js"));          // zero foos
    assert!(p.matches("footest.js"));       // one foo
    assert!(p.matches("foofootest.js"));    // two foos
    assert!(!p.matches("bartest.js"));      // wrong prefix
}

// ── Picomatch — extglob +(…) ───────────────────────────────────────────────

#[test]
fn pm_plus_one_or_more() {
    let p = pm("+(foo|bar).js");
    assert!(p.matches("foo.js"));
    assert!(p.matches("bar.js"));
    assert!(p.matches("foobar.js"));
    assert!(!p.matches(".js"));
    assert!(!p.matches("baz.js"));
}

// ── Picomatch — nested extglob ─────────────────────────────────────────────

#[test]
fn pm_nested_extglob() {
    // +(foo|!(bar)) → one or more of "foo" or "not bar"
    let p = pm("+(foo|!(bar)).js");
    assert!(p.matches("foo.js"));
    assert!(p.matches("baz.js"));
    // "bar.js" — !(bar) doesn't match "bar", but +(…) can match zero "!(bar)" portions...
    // Actually with +(foo|!(bar)), "bar.js" would need at least one match of "foo" or "!(bar)".
    // "bar" fails !(bar) and fails "foo" → no match.
    assert!(!p.matches("bar.js"));
}

// ── Bash extglob — safe mode (default) ────────────────────────────────────

#[test]
fn bash_safe_bang_single_segment() {
    let p = bash("src/!(test)");
    assert!(p.matches("src/main"));
    assert!(!p.matches("src/test"));
    assert!(!p.matches("src/a/main"));
}

#[test]
fn bash_safe_star_extglob() {
    let p = bash("*(foo|bar)");
    assert!(p.matches(""));
    assert!(p.matches("foo"));
    assert!(p.matches("foobar"));
    assert!(!p.matches("baz"));
}

// ── Bash extglob — greedy mode (opt-in) ───────────────────────────────────

#[test]
fn bash_greedy_bang_crosses_slash() {
    // !(*.txt) greedy: match anything NOT ending in .txt, can span segments
    let p = bash_greedy("!(*.txt)");
    assert!(p.matches("foo.rs"));
    assert!(p.matches("a/b/foo.rs"));
    assert!(!p.matches("foo.txt"));
}

#[test]
fn bash_greedy_bang_with_wildcard() {
    let p = bash_greedy("!(test/*)");
    assert!(p.matches("src/main.rs"));
    assert!(!p.matches("test/main.rs"));
}

// ── Compatibility matrix (GAR-243) ─────────────────────────────────────────

/// Compatibility matrix: documents which patterns cross `/` and which do not.
///
/// | Pattern | Mode | `*` crosses `/`? | `**` recursive? | extglob `!(…)` crosses `/`? |
/// |---------|------|------------------|-----------------|----------------------------|
/// | `*.rs`  | PM   | No               | —               | —                          |
/// | `**/*.rs` | PM | —              | Yes             | —                          |
/// | `!(foo)` | PM  | —              | —               | No (single segment)        |
/// | `!(foo)` | Bash greedy | —     | —               | Yes                        |
#[test]
fn compat_matrix_star_no_slash_picomatch() {
    assert!(!pm("*.rs").matches("src/main.rs"));
}

#[test]
fn compat_matrix_double_star_recursive() {
    assert!(pm("**/*.rs").matches("a/b/c/main.rs"));
}

#[test]
fn compat_matrix_bang_single_segment_picomatch() {
    let p = pm("!(foo)");
    assert!(p.matches("bar"));
    assert!(!p.matches("a/bar")); // does not cross /
}

#[test]
fn compat_matrix_bang_greedy_bash() {
    let p = bash_greedy("!(foo)");
    assert!(p.matches("bar"));
    assert!(p.matches("a/bar")); // greedy: crosses /
}

// ── Escape sequences ──────────────────────────────────────────────────────

#[test]
fn escaped_star_is_literal() {
    let p = pm(r"\*.rs");
    assert!(p.matches("*.rs"));
    assert!(!p.matches("foo.rs"));
}

#[test]
fn escaped_dot_is_literal() {
    let p = pm(r"foo\.rs");
    assert!(p.matches("foo.rs"));
    assert!(!p.matches("fooXrs"));
}

// ── Windows path normalisation ────────────────────────────────────────────

#[test]
fn backslash_paths_normalised_to_forward_slash() {
    let p = pm("src/**/*.rs");
    assert!(p.matches("src\\a\\b\\main.rs"));
    assert!(p.matches("src/a/b/main.rs"));
}
