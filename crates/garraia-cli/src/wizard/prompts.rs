//! `Prompter` trait + dialoguer wrapper — plan 0126 §M1.6.
//!
//! Thin layer over `dialoguer::{Select,Confirm,Input,Password}`. The
//! orchestrator in [`super::run_wizard`] depends on the trait so a
//! future test harness (or alternate-frontend wizard) can supply its
//! own prompter. The current PR ships only [`DialoguerPrompter`] —
//! orchestrator coverage is provided by the `tests/wizard_smoke.rs`
//! integration test which drives the real binary with
//! `GARRAIA_BOOTSTRAP_LOCAL=0` (non-interactive guard).

#![allow(dead_code)]

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};

/// Minimal prompt surface used by the wizard orchestrator. Each method
/// blocks until the user responds or cancels.
pub trait Prompter {
    /// Render a `Select` with `options` and return the chosen index.
    fn select(&self, prompt: &str, options: &[&str], default: usize) -> Result<usize>;
    /// Render a `Confirm` and return the boolean answer.
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;
    /// Render an `Input` with a default value; empty answers preserved.
    fn input(&self, prompt: &str, default: &str) -> Result<String>;
    /// Render a `Password`. When `confirmation` is `Some`, the second
    /// confirm prompt is shown and the answers must match.
    fn password(&self, prompt: &str, confirmation: Option<&str>) -> Result<String>;
}

/// Production implementation that talks to the real TTY.
pub struct DialoguerPrompter;

impl Prompter for DialoguerPrompter {
    fn select(&self, prompt: &str, options: &[&str], default: usize) -> Result<usize> {
        Select::new()
            .with_prompt(prompt)
            .items(options)
            .default(default)
            .interact()
            .context("selection cancelled")
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()
            .context("confirmation cancelled")
    }

    fn input(&self, prompt: &str, default: &str) -> Result<String> {
        Input::new()
            .with_prompt(prompt)
            .default(default.to_string())
            .allow_empty(true)
            .interact_text()
            .context("input cancelled")
    }

    fn password(&self, prompt: &str, confirmation: Option<&str>) -> Result<String> {
        let mut builder = Password::new();
        builder = builder.with_prompt(prompt).allow_empty_password(true);
        if let Some(confirm_prompt) = confirmation {
            builder = builder.with_confirmation(confirm_prompt, "Passphrases don't match");
        }
        builder.interact().context("password input cancelled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that `DialoguerPrompter` satisfies the
    /// `Prompter` trait. Functional coverage of the orchestrator is
    /// provided by `tests/wizard_smoke.rs`.
    #[allow(dead_code)]
    fn _trait_object_safety(_p: &dyn Prompter) {}

    #[test]
    fn dialoguer_prompter_implements_trait() {
        let _: Box<dyn Prompter> = Box::new(DialoguerPrompter);
    }
}
