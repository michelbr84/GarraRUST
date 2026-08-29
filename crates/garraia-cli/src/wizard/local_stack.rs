//! Local AI stack helpers — plan 0126 §M1.4.
//!
//! GPU-gated install + start helpers for Ollama (`curl … | sh`) and the
//! Qwen3-14B GGUF model pull, plus install-hint printers for Chatterbox
//! TTS and faster-whisper STT.
//!
//! The wizard only invokes these helpers after an explicit `Confirm`
//! prompt. Auto-install of the Python TTS/STT stacks is intentionally
//! deferred to a follow-up plan — this module only writes endpoints
//! and prints copy-paste install commands for those.
//!
//! All shell-outs use argv-only `Command` (no `sh -c`), matching the
//! existing pattern in `crates/garraia-cli/src/update.rs`. The single
//! exception is the Ollama installer itself, which we invoke as
//! `sh -c "curl -fsSL https://ollama.com/install.sh | sh"` — the
//! upstream pattern; the URL is hard-coded and not user-derived, so
//! no injection surface.

#![allow(dead_code)] // M1.7 orchestrator wires these in; M1.4 ships
// the API + unit-testable hint printers.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use super::env_detect::EnvSnapshot;

/// The Qwen3 GGUF model tag the wizard pulls when the user opts into the
/// local stack. Spec-locked (plan 0126 §Decisions).
pub const QWEN3_MODEL_TAG: &str = "hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M";

/// Default Ollama tag offered by the wizard. `qwen3.8:latest` resolves to
/// `qwen3.8:27b` (Q4_K_M, ~18 GB, 262 144-token context, vision + tools).
///
/// Kept byte-identical to `garraia_agents::ollama::DEFAULT_MODEL` and to
/// `chat::hardcoded_default_model("ollama")` — a test in each of those two
/// modules pins its own copy, and `wizard::config_writer` pins this one.
pub const DEFAULT_OLLAMA_MODEL_TAG: &str = "qwen3.8:latest";

/// Identifier the wizard writes into `agent.default_provider` /
/// `agent.fallback_providers` for the local Ollama-backed LLM.
pub const OLLAMA_PROVIDER_KEY: &str = "ollama-qwen3";

/// `OpenAI-compatible` base URL exposed by Ollama on the default port.
pub const OLLAMA_OPENAI_BASE_URL: &str = "http://127.0.0.1:11434/v1";

/// Token Ollama treats as a wildcard API key for its OpenAI-compatible
/// endpoint — Ollama itself does no auth, so any non-empty string works.
pub const OLLAMA_API_KEY: &str = "ollama";

/// Where the wizard writes Ollama / TTS / STT log files and PID stamps.
fn nohup_dir(home: &Path) -> PathBuf {
    home.join(".garraia")
}

// ---------- Install gate ------------------------------------------------------

/// Top-level intent emitted by each prompt: install / start / skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallChoice {
    Install,
    Skip,
}

/// Run `curl -fsSL https://ollama.com/install.sh | sh`. Only invoked
/// when the user has confirmed and Ollama is not yet on `$PATH`.
pub fn install_ollama() -> Result<()> {
    println!("Installing Ollama (this can take a minute)…");
    let status = Command::new("sh")
        .arg("-c")
        .arg("curl -fsSL https://ollama.com/install.sh | sh")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .status()
        .context("failed to spawn `sh -c curl …| sh` for Ollama install")?;
    if !status.success() {
        anyhow::bail!("Ollama install script exited with status {status}");
    }
    println!("Ollama installed.");
    Ok(())
}

/// Run `ollama pull <tag>`. Requires Ollama on `$PATH` and the daemon
/// running. The user confirms (and picks the tag) before this is invoked.
///
/// Shelling out rather than using `POST /api/pull` is deliberate *here*:
/// the wizard has just installed and started a local daemon, `ollama` is on
/// `$PATH` by construction, and the upstream CLI's progress bar is nicer than
/// anything we would reimplement. The `chat`/`ask` path has different
/// constraints and uses the HTTP API instead — see
/// `garraia_agents::OllamaProvider::pull_model`.
pub fn pull_model(tag: &str) -> Result<()> {
    println!("Pulling {tag} (once)…");
    let status = Command::new("ollama")
        .arg("pull")
        .arg(tag)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .status()
        .context("failed to spawn `ollama pull`")?;
    if !status.success() {
        anyhow::bail!("`ollama pull {tag}` exited with {status}");
    }
    Ok(())
}

/// Back-compat wrapper for the plan-0126 default. Prefer [`pull_model`].
pub fn pull_qwen3() -> Result<()> {
    pull_model(QWEN3_MODEL_TAG)
}

/// One entry in the wizard's local-model picker.
pub struct ModelChoice {
    /// Ollama tag to pull, or `None` for the "type your own" / "skip" rows.
    pub tag: Option<&'static str>,
    /// Label shown in the `Select`.
    pub label: &'static str,
}

/// Curated local models offered by `garraia init`, best default first.
///
/// The user can always bypass this list entirely via the "outro modelo" row,
/// which accepts any Ollama tag (including `hf.co/…` registry refs).
pub const MODEL_CHOICES: &[ModelChoice] = &[
    ModelChoice {
        tag: Some(DEFAULT_OLLAMA_MODEL_TAG),
        label: "Qwen 3.8 27B — padrao (~18 GB, 256K de contexto, visao + tools)",
    },
    ModelChoice {
        tag: Some("qwen3:8b"),
        label: "Qwen 3 8B — leve (~5 GB, roda em GPU modesta)",
    },
    ModelChoice {
        tag: Some(QWEN3_MODEL_TAG),
        label: "Qwen 3 14B GGUF — o padrao anterior (~9 GB)",
    },
    ModelChoice {
        tag: Some("llama3.1"),
        label: "Llama 3.1 8B (~4.7 GB)",
    },
    ModelChoice {
        tag: None,
        label: "Outro — digitar a tag do Ollama",
    },
    ModelChoice {
        tag: None,
        label: "Pular o download por enquanto",
    },
];

/// Index of the "type your own tag" row in [`MODEL_CHOICES`].
pub const MODEL_CHOICE_CUSTOM: usize = 4;
/// Index of the "skip the download" row in [`MODEL_CHOICES`].
pub const MODEL_CHOICE_SKIP: usize = 5;

/// Start `ollama serve` so the OpenAI-compatible endpoint at
/// `http://127.0.0.1:11434/v1` accepts requests.
///
/// * When the host has systemd ([`EnvSnapshot::has_systemd`]), prefer
///   `systemctl --user start ollama` (Ollama 0.5+ ships a user unit).
/// * Otherwise fall back to `nohup ollama serve >> ~/.garraia/ollama.log
///   2>&1 &`, writing the child PID to `~/.garraia/ollama.pid`.
///
/// Unix-only — on non-Unix targets the function is a no-op that returns
/// `Ok(())` (the wizard's GPU branch is itself unix-only in practice).
pub fn start_ollama_systemd_or_nohup(env: &EnvSnapshot, home: &Path) -> Result<()> {
    if env.has_systemd {
        println!("Starting Ollama via systemd (--user)…");
        let status = Command::new("systemctl")
            .args(["--user", "start", "ollama"])
            .status()
            .context("failed to spawn `systemctl --user start ollama`")?;
        if status.success() {
            return Ok(());
        }
        eprintln!("systemd start failed (status {status}); falling back to nohup.");
    }
    #[cfg(unix)]
    {
        use std::io::Write as _;
        let dir = nohup_dir(home);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let log = dir.join("ollama.log");
        let pid_file = dir.join("ollama.pid");
        let log_handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .with_context(|| format!("failed to open {}", log.display()))?;
        let log_dup = log_handle
            .try_clone()
            .context("failed to dup ollama log fd for stderr")?;
        let child = Command::new("ollama")
            .arg("serve")
            .stdout(Stdio::from(log_handle))
            .stderr(Stdio::from(log_dup))
            .stdin(Stdio::null())
            .spawn()
            .context("failed to spawn `ollama serve`")?;
        let mut pid_handle = std::fs::File::create(&pid_file)
            .with_context(|| format!("failed to create {}", pid_file.display()))?;
        writeln!(pid_handle, "{}", child.id()).context("failed to write ollama pid")?;
        println!(
            "Ollama started (PID {}). Logs: {}",
            child.id(),
            log.display()
        );
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        eprintln!(
            "Skipping Ollama start: nohup fallback is unix-only. \
             Run `ollama serve` manually."
        );
    }
    Ok(())
}

// ---------- TTS / STT hint printers ------------------------------------------

/// Sink so the printer can be unit-tested without touching real stdout.
pub trait HintSink {
    fn writeln(&mut self, line: &str);
}

/// `Vec<String>`-backed sink used by tests.
#[derive(Default)]
pub struct CapturedHints {
    pub lines: Vec<String>,
}

impl HintSink for CapturedHints {
    fn writeln(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }
}

/// `println!`-backed sink used at runtime.
pub struct StdoutHints;
impl HintSink for StdoutHints {
    fn writeln(&mut self, line: &str) {
        println!("{line}");
    }
}

/// Print copy-paste install instructions for Chatterbox Multilingual TTS.
/// Aligns with `voice.tts_endpoint = http://127.0.0.1:7860` (plan 0126).
pub fn print_tts_install_hints<S: HintSink>(sink: &mut S) {
    sink.writeln("  TTS — Chatterbox Multilingual (listens on :7860):");
    sink.writeln("    pip install chatterbox-tts");
    sink.writeln("    chatterbox-tts serve --host 127.0.0.1 --port 7860");
    sink.writeln(
        "  Garra will reach it at http://127.0.0.1:7860 (configured in voice.tts_endpoint).",
    );
}

/// Print copy-paste install instructions for faster-whisper STT.
/// Aligns with `voice.stt_endpoint = http://127.0.0.1:9090` (plan 0126).
pub fn print_stt_install_hints<S: HintSink>(sink: &mut S) {
    sink.writeln("  STT — faster-whisper-server (listens on :9090):");
    sink.writeln("    pip install faster-whisper-server");
    sink.writeln("    fwsh serve --host 127.0.0.1 --port 9090");
    sink.writeln(
        "  Garra will reach it at http://127.0.0.1:9090 (configured in voice.stt_endpoint).",
    );
}

/// One-line summary of what the wizard wrote when the user opted into
/// voice but did not auto-install TTS/STT. Used in the final summary
/// block at the end of `run_wizard`.
pub fn voice_endpoints_summary() -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "voice.tts_endpoint=http://127.0.0.1:7860 (chatterbox) | voice.stt_endpoint=http://127.0.0.1:9090 (faster-whisper)"
    );
    s
}

// ---------- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_hints_mention_chatterbox_and_port_7860() {
        let mut sink = CapturedHints::default();
        print_tts_install_hints(&mut sink);
        let combined = sink.lines.join("\n");
        assert!(
            combined.contains("Chatterbox"),
            "missing Chatterbox label:\n{combined}"
        );
        assert!(combined.contains(":7860"), "missing port hint:\n{combined}");
        assert!(
            combined.contains("pip install"),
            "missing pip command:\n{combined}"
        );
    }

    #[test]
    fn stt_hints_mention_faster_whisper_and_port_9090() {
        let mut sink = CapturedHints::default();
        print_stt_install_hints(&mut sink);
        let combined = sink.lines.join("\n");
        assert!(
            combined.contains("faster-whisper"),
            "missing faster-whisper label:\n{combined}"
        );
        assert!(combined.contains(":9090"), "missing port hint:\n{combined}");
    }

    #[test]
    fn voice_endpoints_summary_includes_both_providers() {
        let s = voice_endpoints_summary();
        assert!(s.contains("chatterbox"));
        assert!(s.contains("faster-whisper"));
        assert!(s.contains(":7860"));
        assert!(s.contains(":9090"));
    }

    #[test]
    fn constants_match_plan_0126() {
        // Spec-locked. If any of these change, plan 0126 §Decisions
        // must be amended in lockstep — the gateway config and
        // README/docs reference the exact strings below.
        assert_eq!(QWEN3_MODEL_TAG, "hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M");
        assert_eq!(OLLAMA_PROVIDER_KEY, "ollama-qwen3");
        assert_eq!(OLLAMA_OPENAI_BASE_URL, "http://127.0.0.1:11434/v1");
        assert_eq!(OLLAMA_API_KEY, "ollama");
    }

    #[test]
    fn default_tag_matches_the_rest_of_the_codebase() {
        // Three modules carry this string; each pins its own copy so a
        // one-sided edit fails loudly instead of drifting.
        use garraia_agents::LlmProvider as _;
        assert_eq!(DEFAULT_OLLAMA_MODEL_TAG, "qwen3.8:latest");
        assert_eq!(
            garraia_agents::OllamaProvider::new(None, None).configured_model(),
            Some(DEFAULT_OLLAMA_MODEL_TAG)
        );
    }

    #[test]
    fn model_choices_are_well_formed() {
        // The default must be offered first — `Select::default(0)` picks it.
        assert_eq!(MODEL_CHOICES[0].tag, Some(DEFAULT_OLLAMA_MODEL_TAG));
        // The two special rows carry no tag, and their indices are what
        // `collect_local_stack` branches on.
        assert!(MODEL_CHOICES[MODEL_CHOICE_CUSTOM].tag.is_none());
        assert!(MODEL_CHOICES[MODEL_CHOICE_SKIP].tag.is_none());
        assert_eq!(MODEL_CHOICES.len(), MODEL_CHOICE_SKIP + 1);
        // Every other row must be a pullable tag.
        for (i, c) in MODEL_CHOICES.iter().enumerate() {
            if i != MODEL_CHOICE_CUSTOM && i != MODEL_CHOICE_SKIP {
                assert!(c.tag.is_some(), "row {i} ({}) needs a tag", c.label);
            }
            assert!(!c.label.is_empty());
        }
    }
}
