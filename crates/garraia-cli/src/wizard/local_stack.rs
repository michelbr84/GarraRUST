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

/// Run `ollama pull <QWEN3_MODEL_TAG>`. Requires Ollama on `$PATH` and
/// the daemon running. The user confirms before this is invoked.
pub fn pull_qwen3() -> Result<()> {
    println!("Pulling {QWEN3_MODEL_TAG} (≈9 GiB — once)…");
    let status = Command::new("ollama")
        .arg("pull")
        .arg(QWEN3_MODEL_TAG)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .status()
        .context("failed to spawn `ollama pull`")?;
    if !status.success() {
        anyhow::bail!("`ollama pull {QWEN3_MODEL_TAG}` exited with {status}");
    }
    Ok(())
}

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
}
