//! Argv preprocessing for the `garra` / `garraia` binary.
//!
//! The CLI has no default subcommand at the clap level (`Cli::command` is a
//! plain `Commands`, not an `Option<Commands>`). Historically `main()` papered
//! over that by appending `chat` when argv held nothing but the program name,
//! which made bare `garra` open the REPL but left `garra --model qwen3.8`
//! failing with *unexpected argument*.
//!
//! This module generalises that trick: when argv carries **only flags**, the
//! [`DEFAULT_SUBCOMMAND`] is inserted at index 1 so the flags bind to `chat`.
//!
//! A global clap `--model` is deliberately *not* an option here: `Commands::Chat`
//! and `Commands::Ask` already declare their own `model` argument, and a global
//! arg is propagated into every subcommand — the duplicate name trips clap's
//! uniqueness assertion. A global `-m` would also collide with
//! `Commands::MaxPower`'s `--mode`. Rewriting argv keeps the derive tree
//! untouched and gets `--provider`, `--url` and `--timeout-secs` for free.

use std::ffi::OsString;

/// Subcommand injected when argv carries only flags.
pub(crate) const DEFAULT_SUBCOMMAND: &str = "chat";

/// Rewrite `args` so a flags-only invocation runs [`DEFAULT_SUBCOMMAND`].
///
/// `value_flags` lists the exact tokens (`--model`, `-m`, `--log-level`, …)
/// that consume the *following* argv entry. `main` derives it from the clap
/// command tree, so it cannot drift out of sync with the derives.
///
/// The rule is "inject iff nothing occupies the subcommand position" rather
/// than "inject iff argv[1] is not a known subcommand name". That is
/// deliberate: it preserves clap's `unrecognized subcommand 'chta' … did you
/// mean 'chat'?` diagnostics (a name list would rewrite that into the useless
/// `garra chat chta`), it has no drift risk against `#[cfg(feature = …)]`
/// subcommands, and it handles `garra --model chat` — a model literally named
/// `chat` — correctly.
///
/// Pure: no environment, no I/O, no clap. Returns `args` untouched whenever
/// the caller is asking for help or version, has already named a subcommand,
/// or passed something this scanner cannot reason about — clap then produces
/// its own (better) error.
pub(crate) fn inject_default_subcommand(
    mut args: Vec<OsString>,
    value_flags: &[&str],
) -> Vec<OsString> {
    if args.len() <= 1 {
        args.push(OsString::from(DEFAULT_SUBCOMMAND));
        return args;
    }

    let mut i = 1;
    while i < args.len() {
        let Some(tok) = args[i].to_str() else {
            // Non-UTF-8 cannot be a flag we know; let clap report it.
            return args;
        };

        match tok {
            // Top-level help/version must keep showing the top-level page.
            "-h" | "--help" | "-V" | "--version" => return args,
            // Everything after `--` is positional.
            "--" => return args,
            _ if tok.starts_with("--") => {
                if tok.contains('=') || !value_flags.contains(&tok) {
                    i += 1; // `--flag=value`, or a boolean long flag.
                } else {
                    i += 2; // `--flag value`
                }
            }
            _ if tok.starts_with('-') && tok.chars().count() > 1 => {
                // Char-safe: a short flag is `-` plus exactly one character.
                // An attached value (`-mqwen3.8`) is longer, so it consumes
                // only its own slot.
                let is_bare_short = tok.chars().count() == 2;
                let short: String = tok.chars().take(2).collect();
                if is_bare_short && value_flags.contains(&short.as_str()) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            // A bare token: the subcommand position is occupied. Clap decides.
            _ => return args,
        }
    }

    // Ran off the end without meeting a subcommand — argv is flags only.
    args.insert(1, OsString::from(DEFAULT_SUBCOMMAND));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors what `main::value_taking_flags()` derives from the clap tree.
    /// `main` has its own drift test pinning that derivation to this list.
    const FLAGS: &[&str] = &[
        "--log-level",
        "--model",
        "--provider",
        "--timeout-secs",
        "--url",
        "-m",
        "-p",
        "-u",
    ];

    fn v(xs: &[&str]) -> Vec<OsString> {
        xs.iter().map(OsString::from).collect()
    }

    fn injected(xs: &[&str]) -> Vec<String> {
        inject_default_subcommand(v(xs), FLAGS)
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    fn assert_injects(argv: &[&str]) {
        let out = injected(argv);
        assert_eq!(
            out.get(1).map(String::as_str),
            Some(DEFAULT_SUBCOMMAND),
            "expected `chat` injected into {argv:?}, got {out:?}"
        );
        assert_eq!(out.len(), argv.len() + 1, "{out:?}");
    }

    fn assert_untouched(argv: &[&str]) {
        assert_eq!(injected(argv), argv, "expected {argv:?} left alone");
    }

    #[test]
    fn bare_invocation_still_opens_chat() {
        // Regression guard for the pre-existing `args.len() == 1` behaviour.
        assert_eq!(injected(&["garra"]), vec!["garra", "chat"]);
    }

    #[test]
    fn injects_for_flag_only_invocations() {
        // The headline case.
        assert_injects(&["garra", "--model", "qwen3.8"]);
        assert_injects(&["garra", "-m", "qwen3.8"]);
        assert_injects(&["garra", "--model=qwen3.8"]);
        assert_injects(&["garra", "--debug"]);
        assert_injects(&["garra", "--log-level", "debug"]);
        assert_injects(&["garra", "--log-level=debug"]);
        assert_injects(&["garra", "--provider", "ollama", "--model", "qwen3.8"]);
        assert_injects(&["garra", "--url", "http://127.0.0.1:1234/v1"]);
        assert_injects(&["garra", "--timeout-secs", "300"]);
        assert_injects(&["garra", "--debug", "--model", "qwen3.8"]);
        assert_injects(&["garra", "-y", "--model", "qwen3.8"]);
    }

    #[test]
    fn injects_when_a_flag_value_looks_like_a_subcommand() {
        // `qwen3.8` could plausibly be named `chat`; the value must not be
        // mistaken for the subcommand.
        assert_eq!(
            injected(&["garra", "--model", "chat"]),
            vec!["garra", "chat", "--model", "chat"]
        );
        assert_eq!(
            injected(&["garra", "-p", "status"]),
            vec!["garra", "chat", "-p", "status"]
        );
    }

    #[test]
    fn injects_for_malformed_input_so_clap_reports_it() {
        // Missing value: clap emits "a value is required for '--model <MODEL>'".
        assert_injects(&["garra", "--model"]);
        // Unknown flag: clap emits "unexpected argument '--nope'".
        assert_injects(&["garra", "--nope"]);
    }

    #[test]
    fn leaves_explicit_subcommands_alone() {
        assert_untouched(&["garra", "chat"]);
        assert_untouched(&["garra", "chat", "--model", "qwen3.8"]);
        assert_untouched(&["garra", "chat", "--help"]);
        assert_untouched(&["garra", "ask", "hello"]);
        assert_untouched(&["garra", "start", "--port", "3888"]);
        assert_untouched(&["garra", "config", "check", "--json"]);
    }

    #[test]
    fn leaves_subcommands_preceded_by_flags_alone() {
        // The flag-value-vs-subcommand trap: `debug` is the value of
        // `--log-level`, and `chat` is the real subcommand.
        assert_untouched(&["garra", "--log-level", "debug", "chat"]);
        assert_untouched(&["garra", "--debug", "status"]);
        // A subcommand's own `-m` must survive untouched.
        assert_untouched(&["garra", "max-power", "-m", "new", "--goal", "x"]);
    }

    #[test]
    fn leaves_help_and_version_alone() {
        assert_untouched(&["garra", "--help"]);
        assert_untouched(&["garra", "-h"]);
        assert_untouched(&["garra", "--version"]);
        assert_untouched(&["garra", "-V"]);
        // clap's auto-generated `help` subcommand.
        assert_untouched(&["garra", "help"]);
        assert_untouched(&["garra", "help", "chat"]);
    }

    #[test]
    fn leaves_positional_marker_and_typos_alone() {
        assert_untouched(&["garra", "--"]);
        // Preserves clap's "did you mean 'chat'?" suggestion.
        assert_untouched(&["garra", "frobnicate"]);
    }

    #[test]
    fn is_idempotent() {
        let once = inject_default_subcommand(v(&["garra", "--model", "qwen3.8"]), FLAGS);
        let twice = inject_default_subcommand(once.clone(), FLAGS);
        assert_eq!(once, twice);
    }

    #[test]
    fn attached_short_value_injects() {
        // `-mqwen3.8` is a single token carrying its own value.
        assert_eq!(
            injected(&["garra", "-mqwen3.8"]),
            vec!["garra", "chat", "-mqwen3.8"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_argument_is_left_to_clap() {
        use std::os::unix::ffi::OsStringExt;

        let args = vec![OsString::from("garra"), OsString::from_vec(vec![0xff])];
        assert_eq!(inject_default_subcommand(args.clone(), FLAGS), args);
    }
}
