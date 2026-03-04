//! GAR-262/263: `garraia glob test` — interactive glob pattern tester.
//!
//! Compiles a glob pattern (picomatch or bash extglob) and tests it against:
//!  - Literal path strings supplied as arguments
//!  - Paths read from stdin (one per line, when no paths or `--dir` given)
//!  - A directory tree scan via [`Scanner`] (when `--dir` is given)
//!
//! Output shows matched / skipped files, a summary, and optionally the
//! compiled regex (for debugging the pattern compiler).

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use garraia_glob::{GlobConfig, GlobMode, GlobPattern, MatchResult, Scanner};

/// Run `garraia glob test`.
///
/// # Parameters
/// - `pattern`        — The glob pattern string (e.g. `"**/*.rs"`, `"!(*.log)"`)
/// - `paths`          — Literal path strings to test (skipped when `dir` is set)
/// - `dir`            — If set, scan this directory tree instead of literal paths
/// - `mode`           — `"picomatch"` (default) or `"bash"`
/// - `dot`            — Match dotfiles with `*` and `?`
/// - `greedy`         — Bash mode: allow `!(pat)` to cross `/`
/// - `ignore_case`    — Case-insensitive matching
/// - `no_gitignore`   — Disable `.gitignore` loading during scan
/// - `no_garraignore` — Disable `.garraignore` loading during scan
/// - `only_matched`   — Suppress unmatched lines from output
/// - `debug_regex`    — Print the compiled regex to stderr before results
/// - `json_output`    — Emit matched paths as a JSON array
#[allow(clippy::too_many_arguments)]
pub fn run_glob_test(
    pattern: &str,
    paths: &[String],
    dir: Option<&PathBuf>,
    mode: &str,
    dot: bool,
    greedy: bool,
    ignore_case: bool,
    no_gitignore: bool,
    no_garraignore: bool,
    only_matched: bool,
    debug_regex: bool,
    json_output: bool,
) -> Result<()> {
    // --- warn on no-op flag combinations ---
    if greedy && mode != "bash" {
        eprintln!("Warning: --greedy has no effect without --mode bash");
    }

    // --- build GlobConfig ---
    let glob_mode = if mode == "bash" { GlobMode::Bash } else { GlobMode::Picomatch };
    let config = GlobConfig {
        mode: glob_mode,
        dot,
        bash_greedy_negated_extglob: greedy,
        case_sensitive: !ignore_case,
    };

    // --- compile pattern ---
    let compiled = GlobPattern::new(pattern, &config)
        .with_context(|| format!("invalid glob pattern: {pattern}"))?;

    if debug_regex {
        eprintln!("[debug] pattern  : {pattern}");
        eprintln!("[debug] regex    : {}", compiled.regex_str());
        eprintln!("[debug] mode     : {mode}");
        eprintln!("[debug] dot      : {dot}");
        eprintln!("[debug] greedy   : {greedy}");
    }

    // --- collect entries to classify ---
    let all_entries: Vec<MatchResult> = if let Some(root) = dir {
        collect_from_dir(root, &config, !no_gitignore, !no_garraignore)?
    } else if !paths.is_empty() {
        paths
            .iter()
            .map(|p| MatchResult { path: p.clone(), is_dir: false })
            .collect()
    } else {
        collect_from_stdin()?
    };

    // --- classify ---
    let (matched, unmatched): (Vec<_>, Vec<_>) =
        all_entries.iter().partition(|r| compiled.matches(&r.path));

    // --- output ---
    if json_output {
        let json = serde_json::to_string_pretty(&matched)
            .context("failed to serialise results to JSON")?;
        println!("{json}");
        return Ok(());
    }

    // Human-readable output
    for r in &matched {
        let slash = if r.is_dir { "/" } else { "" };
        println!("  MATCH  {}{slash}", r.path);
    }
    if !only_matched {
        for r in &unmatched {
            println!("  skip   {}", r.path);
        }
    }

    println!();
    println!("Pattern : {pattern}");
    println!("Mode    : {mode}");
    if let Some(root) = dir {
        println!("Root    : {}", root.display());
    }
    println!("Matched : {}/{} entries", matched.len(), all_entries.len());

    Ok(())
}

/// Scan `root` with the given config and ignore settings; return all entries.
fn collect_from_dir(
    root: &Path,
    config: &GlobConfig,
    use_gitignore: bool,
    use_garraignore: bool,
) -> Result<Vec<MatchResult>> {
    if !root.exists() {
        anyhow::bail!("directory does not exist: {}", root.display());
    }
    if !root.is_dir() {
        anyhow::bail!("path is not a directory: {}", root.display());
    }

    let scanner = Scanner::new(root, config.clone())
        .use_gitignore(use_gitignore)
        .use_garraignore(use_garraignore);

    scanner.scan().with_context(|| format!("scanning directory: {}", root.display()))
}

/// Read one path per line from stdin and return as `MatchResult` list.
fn collect_from_stdin() -> Result<Vec<MatchResult>> {
    eprintln!("Reading paths from stdin (one per line, Ctrl-D / Ctrl-Z to finish)...");
    let stdin = io::stdin();
    let entries = stdin
        .lock()
        .lines()
        .filter_map(|line| {
            let path = line.ok()?.trim().to_string();
            if path.is_empty() { None } else { Some(MatchResult { path, is_dir: false }) }
        })
        .collect();
    Ok(entries)
}
