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
    GlobPattern::new(
        pat,
        &GlobConfig {
            dot: true,
            ..GlobConfig::default()
        },
    )
    .unwrap()
}

fn bash(pat: &str) -> GlobPattern {
    GlobPattern::new(
        pat,
        &GlobConfig {
            mode: GlobMode::Bash,
            ..GlobConfig::default()
        },
    )
    .unwrap()
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
    assert!(p.matches("test.js")); // zero foos
    assert!(p.matches("footest.js")); // one foo
    assert!(p.matches("foofootest.js")); // two foos
    assert!(!p.matches("bartest.js")); // wrong prefix
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

// ── Regression: cases from Garra agent report (GAR-259 baseline) ──────────
//
// These pin the exact 3 behaviours that were incorrectly reported as broken.
// They call GlobPattern directly — the same engine used by `garraia glob test`
// and the Scanner — so a false-positive from any other search tool is not
// a bug here.

#[test]
fn regression_star_rs_does_not_cross_separator() {
    // `*.rs` must NOT match a path that contains a `/` — picomatch semantics.
    let p = pm("*.rs");
    assert!(p.matches("main.rs"), "*.rs should match bare filename");
    assert!(p.matches("lib.rs"));
    assert!(!p.matches("src/main.rs"), "*.rs must NOT cross /");
    assert!(!p.matches("a/b/c/foo.rs"), "*.rs must NOT cross nested /");
}

#[test]
fn regression_brace_expansion_rs_toml() {
    // `*.{rs,toml}` must match both extensions via brace expansion.
    let p = pm("*.{rs,toml}");
    assert!(p.matches("main.rs"), "*.{{rs,toml}} should match .rs");
    assert!(p.matches("Cargo.toml"), "*.{{rs,toml}} should match .toml");
    assert!(!p.matches("main.py"), "*.{{rs,toml}} should not match .py");
    assert!(
        !p.matches("src/main.rs"),
        "brace expansion still respects /"
    );
}

#[test]
fn regression_star_dotfile_default_and_dot_flag() {
    // `*` must NOT match `.gitignore` with default config (dot=false),
    // but MUST match with dot=true.
    let p_default = pm("*");
    assert!(p_default.matches("README.md"), "* matches normal file");
    assert!(
        !p_default.matches(".gitignore"),
        "* must not match dotfile by default"
    );

    let p_dot = pm_dot("*");
    assert!(
        p_dot.matches(".gitignore"),
        "* matches dotfile when dot=true"
    );
    assert!(
        p_dot.matches("README.md"),
        "* still matches normal files with dot=true"
    );
}

// ── Windows path normalisation ────────────────────────────────────────────

#[test]
fn backslash_paths_normalised_to_forward_slash() {
    let p = pm("src/**/*.rs");
    assert!(p.matches("src\\a\\b\\main.rs"));
    assert!(p.matches("src/a/b/main.rs"));
}

// ── Bash extglob — extended suite (GAR-252) ───────────────────────────────

/// `*(pat|pat)` — zero or more alternations, Bash safe mode.
#[test]
fn bash_star_zero_or_more_alternation() {
    let p = bash("*(foo|bar)");
    assert!(p.matches("")); // zero
    assert!(p.matches("foo")); // one
    assert!(p.matches("bar"));
    assert!(p.matches("foobar")); // two
    assert!(p.matches("barfoo"));
    assert!(p.matches("foofoo"));
    assert!(!p.matches("baz")); // not in alternation
}

/// `+(pat|pat)` — one or more alternations, Bash safe mode.
#[test]
fn bash_plus_one_or_more_alternation() {
    let p = bash("+(foo|bar).rs");
    assert!(p.matches("foo.rs"));
    assert!(p.matches("bar.rs"));
    assert!(p.matches("foobar.rs"));
    assert!(p.matches("barfoobar.rs"));
    assert!(!p.matches(".rs")); // zero — not allowed
    assert!(!p.matches("baz.rs"));
}

/// `@(pat|pat)` — exactly one alternative, Bash safe mode.
#[test]
fn bash_at_exactly_one() {
    let p = bash("@(main|lib).rs");
    assert!(p.matches("main.rs"));
    assert!(p.matches("lib.rs"));
    assert!(!p.matches("mainlib.rs")); // two → not one
    assert!(!p.matches("util.rs"));
}

/// `?(pat)` — zero or one occurrence, Bash safe mode.
#[test]
fn bash_question_optional_prefix() {
    let p = bash("src/?(test_)main.rs");
    assert!(p.matches("src/main.rs"));
    assert!(p.matches("src/test_main.rs"));
    assert!(!p.matches("src/test_test_main.rs")); // two — not allowed
}

/// Bash safe mode: `!(pat)` confined to a single segment (no slash crossing).
#[test]
fn bash_safe_bang_does_not_cross_slash() {
    let p = bash("src/!(generated)");
    assert!(p.matches("src/main"));
    assert!(p.matches("src/lib"));
    assert!(!p.matches("src/generated"));
    assert!(!p.matches("src/a/main")); // [^/]* stops at /
}

/// Bash greedy: `!(*.txt)` can span multiple path segments.
#[test]
fn bash_greedy_bang_multilevel() {
    let p = bash_greedy("!(*.txt)");
    assert!(p.matches("README.md"));
    assert!(p.matches("src/lib.rs"));
    assert!(p.matches("a/b/c/main.rs"));
    assert!(!p.matches("README.txt"));
    assert!(!p.matches("docs/notes.txt"));
}

/// Bash greedy: `!(test/*)` excludes paths under test/ only.
#[test]
fn bash_greedy_bang_path_prefix() {
    let p = bash_greedy("!(test/*)");
    assert!(p.matches("src/main.rs"));
    assert!(p.matches("benches/perf.rs"));
    assert!(!p.matches("test/main.rs"));
    assert!(!p.matches("test/unit/lib.rs"));
}

/// Combined `@(…)` and plain `*` — exactly-one directory, any file name.
#[test]
fn bash_combined_at_and_star() {
    let p = bash("@(src|lib)/*.rs");
    assert!(p.matches("src/main.rs"));
    assert!(p.matches("lib/util.rs"));
    assert!(!p.matches("bin/main.rs")); // prefix not in @(src|lib)
    assert!(!p.matches("src/sub/main.rs")); // * doesn't cross /
}

/// Nested extglob: `+(foo|!(bar))` — one or more of "foo" or "not bar".
#[test]
fn bash_nested_plus_with_bang() {
    let p = bash("+(foo|!(bar)).rs");
    assert!(p.matches("foo.rs"));
    assert!(p.matches("baz.rs")); // !(bar) matches "baz"
    assert!(!p.matches("bar.rs")); // !(bar) rejects "bar", foo doesn't match → no match
}

/// Case-insensitive Bash mode.
#[test]
fn bash_case_insensitive() {
    let p = GlobPattern::new(
        "+(FOO|BAR).rs",
        &GlobConfig {
            mode: GlobMode::Bash,
            case_sensitive: false,
            ..GlobConfig::default()
        },
    )
    .unwrap();
    assert!(p.matches("foo.rs"));
    assert!(p.matches("BAR.rs"));
    assert!(p.matches("fooBAR.rs")); // two alternations
}

/// `*(*.log)` with wildcard inside — zero or more `.log`-named segments.
#[test]
fn bash_star_inner_wildcard() {
    let p = bash("*(*.log)test");
    assert!(p.matches("test")); // zero occurrences
    assert!(p.matches("error.logtest")); // one
    assert!(p.matches("a.logb.logtest")); // two
    assert!(!p.matches("error.txttest")); // *.log doesn't match error.txt
}
