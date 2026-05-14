//! Onboarding wizard for `garraia init` — plan 0126 (PR-A).
//!
//! Submodules:
//!
//! * [`env_detect`] — read-only probes of OS, root, RunPod, systemd, NVIDIA,
//!   Ollama, and well-known ports.
//! * [`local_stack`] — GPU-gated install + start helpers for Ollama plus
//!   install-hint printers for Chatterbox TTS and faster-whisper STT.
//! * [`config_writer`] — emits `config.yml` with three strategies
//!   (`FirstWrite`, `Backup`, `MergeUpdate`).
//! * [`prompts`] — `Prompter` trait + `DialoguerPrompter`.
//!
//! The orchestrator [`run_wizard`] composes the four submodules. The
//! non-interactive guard at the top is preserved verbatim from the
//! pre-split `wizard.rs` so CI invocations of `garraia init` continue
//! to exit early with the same hint message.

mod config_writer;
mod env_detect;
mod local_stack;
mod prompts;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};
use tracing::info;

use config_writer::{
    CloudLlmChoice, ExistingConfigStrategy, LocalLlmChoice, TelegramChoice, WizardOutcome,
    backup_path_for, build_app_config, write_config,
};
use env_detect::{EnvSnapshot, OllamaState};
use local_stack::{
    OLLAMA_PROVIDER_KEY, StdoutHints, install_ollama, print_stt_install_hints,
    print_tts_install_hints, pull_qwen3, start_ollama_systemd_or_nohup, voice_endpoints_summary,
};

/// Default cloud model — matches `chat.rs` (`openrouter/auto`).
const DEFAULT_OPENROUTER_MODEL: &str = "openrouter/auto";

/// `GARRAIA_BOOTSTRAP_LOCAL=0` disables the GPU/local-stack prompts even
/// when a GPU is detected. Any other value (or unset) keeps the prompts
/// gated by [`EnvSnapshot::supports_local_stack`].
fn local_bootstrap_enabled() -> bool {
    !matches!(std::env::var("GARRAIA_BOOTSTRAP_LOCAL").as_deref(), Ok("0"))
}

/// Run the interactive onboarding wizard. Writes `config.yml` and
/// optionally stores credentials in the vault.
pub fn run_wizard(config_dir: &Path) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        print_non_interactive_hint(config_dir);
        return Ok(());
    }

    println!();
    println!("  GarraIA Setup Wizard");
    println!("  ----------------------");
    println!();

    // --- 1. Detect the environment -----------------------------------------
    let env = env_detect::detect();
    print_env_summary(&env);

    // --- 2. Existing config policy ----------------------------------------
    let config_path = config_dir.join("config.yml");
    let strategy = if config_path.exists() {
        let choices = &[
            "Backup the existing config and write a new one",
            "Merge / update (keep existing values; only add missing keys)",
            "Cancel — exit without changes",
        ];
        let pick = Select::new()
            .with_prompt(format!(
                "Existing config found at {}. What do you want to do?",
                config_path.display()
            ))
            .items(choices)
            .default(0)
            .interact()
            .context("existing-config decision cancelled")?;
        match pick {
            0 => ExistingConfigStrategy::Backup {
                backup_path: backup_path_for(config_dir),
            },
            1 => ExistingConfigStrategy::MergeUpdate,
            _ => {
                println!();
                println!("  Wizard cancelled — your config is unchanged.");
                return Ok(());
            }
        }
    } else {
        ExistingConfigStrategy::FirstWrite
    };

    // --- 3. Provider mode --------------------------------------------------
    // GPU + local bootstrap enabled → default to "local-first". Otherwise
    // cloud-only is the safe default.
    let local_available = env.supports_local_stack() && local_bootstrap_enabled();
    let (mode_idx, mode_default) = if local_available {
        (
            Select::new()
                .with_prompt("Which LLM mode?")
                .items([
                    "Local-first (Ollama on this GPU + cloud fallback)",
                    "Cloud-first (OpenRouter primary + Ollama fallback)",
                    "Cloud-only (OpenRouter — no local stack)",
                ])
                .default(0)
                .interact()
                .context("provider mode cancelled")?,
            "local",
        )
    } else {
        if env.has_nvidia && !local_bootstrap_enabled() {
            println!(
                "  GPU detected but GARRAIA_BOOTSTRAP_LOCAL=0 — skipping local-stack prompts."
            );
        }
        (2, "cloud-only")
    };
    let _ = mode_default;

    let mut openrouter_choice: Option<CloudLlmChoice> = None;
    let mut local_choice: Option<LocalLlmChoice> = None;
    let mut fallback_providers: Vec<String> = Vec::new();

    // --- 4. Cloud branch ---------------------------------------------------
    let want_cloud = matches!(mode_idx, 0..=2);
    let openrouter_api_key_plaintext = if want_cloud {
        collect_openrouter(&mut openrouter_choice)?
    } else {
        None
    };

    // --- 5. Local branch ---------------------------------------------------
    if matches!(mode_idx, 0 | 1) && local_available {
        collect_local_stack(&env, &mut local_choice)?;
    }

    // --- 6. Resolve default / fallback ordering ----------------------------
    let default_provider: String = match (local_choice.as_ref(), openrouter_choice.as_ref()) {
        (Some(_), Some(_)) if mode_idx == 0 => {
            fallback_providers = vec!["openrouter".to_string()];
            OLLAMA_PROVIDER_KEY.to_string()
        }
        (Some(_), Some(_)) if mode_idx == 1 => {
            fallback_providers = vec![OLLAMA_PROVIDER_KEY.to_string()];
            "openrouter".to_string()
        }
        (Some(_), None) => OLLAMA_PROVIDER_KEY.to_string(),
        (None, Some(_)) => "openrouter".to_string(),
        _ => {
            // Neither selected — emit a placeholder so the wizard
            // produces a valid `agent.default_provider`. The user can
            // edit later.
            "openrouter".to_string()
        }
    };

    // --- 7. Voice prompt (GPU-only) ----------------------------------------
    let voice_enabled = if env.has_nvidia && local_bootstrap_enabled() {
        let want_voice = Confirm::new()
            .with_prompt("Enable voice (Chatterbox TTS @ :7860 + Whisper STT @ :9090)?")
            .default(false)
            .interact()
            .context("voice prompt cancelled")?;
        if want_voice {
            println!();
            print_tts_install_hints(&mut StdoutHints);
            println!();
            print_stt_install_hints(&mut StdoutHints);
            println!();
            println!(
                "  Voice endpoints written to config: {}",
                voice_endpoints_summary()
            );
            println!();
        }
        want_voice
    } else {
        false
    };

    // --- 8. System prompt --------------------------------------------------
    let system_prompt_input: String = Input::new()
        .with_prompt("System prompt (optional)")
        .default("You are a helpful personal AI assistant.".to_string())
        .allow_empty(true)
        .interact_text()
        .context("system prompt input cancelled")?;
    let system_prompt = if system_prompt_input.is_empty() {
        None
    } else {
        Some(system_prompt_input)
    };

    // --- 9. Telegram ------------------------------------------------------
    println!();
    println!("  ── Channel Setup ──");
    println!();

    let setup_telegram = Confirm::new()
        .with_prompt("Do you want to connect GarraIA to Telegram?")
        .default(false)
        .interact()
        .context("telegram prompt cancelled")?;

    let mut telegram_token_plaintext: Option<String> = None;
    let mut telegram_token_for_vault: Option<String> = None;
    if setup_telegram {
        println!();
        println!("  To create a Telegram bot:");
        println!("  1. Open Telegram and talk to @BotFather");
        println!("  2. Send /newbot and follow the instructions");
        println!("  3. Copy the token (format: 123456789:ABCdef...)");
        println!();

        let token: String = Password::new()
            .with_prompt("Enter your Telegram bot token (or set TELEGRAM_BOT_TOKEN env var later)")
            .allow_empty_password(true)
            .interact()
            .context("telegram token input cancelled")?;
        let token = token.trim().to_string();

        if !token.is_empty() {
            let choices = &[
                "Store in encrypted vault (recommended)",
                "Store as plaintext in config.yml",
                "Skip storing (use env var)",
            ];
            let storage = Select::new()
                .with_prompt("How should the Telegram bot token be stored?")
                .items(choices)
                .default(0)
                .interact()
                .context("telegram storage choice cancelled")?;
            match storage {
                0 => telegram_token_for_vault = Some(token.clone()),
                1 => telegram_token_plaintext = Some(token.clone()),
                _ => {}
            }
        }
    }

    let telegram_choice = if setup_telegram {
        Some(TelegramChoice {
            plaintext_token: telegram_token_plaintext,
        })
    } else {
        None
    };

    // --- 10. Vault (cloud key + telegram token) ---------------------------
    let openrouter_for_vault =
        openrouter_api_key_plaintext.filter(|_| openrouter_should_use_vault(&openrouter_choice));
    let needs_vault = openrouter_for_vault.is_some() || telegram_token_for_vault.is_some();
    if needs_vault {
        open_or_create_vault(
            config_dir,
            openrouter_for_vault.as_deref(),
            telegram_token_for_vault.as_deref(),
        )?;
    }

    // --- 11. Build outcome + write config ---------------------------------
    let (host, port) = pick_host_port(&env);
    let outcome = WizardOutcome {
        host,
        port,
        default_provider,
        fallback_providers,
        openrouter: openrouter_choice,
        local_llm: local_choice,
        voice_enabled,
        system_prompt,
        telegram: telegram_choice,
    };

    // Sanity-check the outcome can serialize cleanly before we touch
    // the existing file. (`build_app_config` is also exercised in the
    // unit tests so this is a defense-in-depth check.)
    let _ = build_app_config(&outcome);
    let written = write_config(config_dir, &outcome, strategy)?;

    info!("config written to {}", written.display());

    // --- 12. Final summary -------------------------------------------------
    println!();
    println!("  Config written to {}", written.display());
    println!("  Next: `garraia start` to launch the gateway in the foreground.");
    println!("  Press Ctrl+C to stop. To run later in background: garraia start -d");
    println!("  Logs: {}/garraia.log", config_dir.display());
    if outcome.voice_enabled {
        println!("  Voice was enabled — see docs/voice.md to install Chatterbox + faster-whisper.");
    }
    println!();

    Ok(())
}

// ---------- helpers -----------------------------------------------------------

fn print_non_interactive_hint(config_dir: &Path) {
    println!("Non-interactive environment detected.");
    println!(
        "To configure GarraIA, edit: {}/config.yml",
        config_dir.display()
    );
    println!();
    println!("Minimal config.yml example:");
    println!("---");
    println!("llm:");
    println!("  main:");
    println!("    provider: anthropic");
    println!("    api_key: sk-ant-...");
    println!("agent:");
    println!("  system_prompt: \"You are a helpful assistant.\"");
    println!("channels:");
    println!("  telegram:");
    println!("    type: telegram");
    println!("    enabled: true");
    println!("    # Set TELEGRAM_BOT_TOKEN env var or add bot_token here");
}

fn print_env_summary(env: &EnvSnapshot) {
    println!("  Environment:");
    println!(
        "    os: {:?} | root: {} | runpod: {} | systemd: {}",
        env.os, env.is_root, env.is_runpod, env.has_systemd
    );
    if env.has_nvidia {
        if let Some(gpu) = &env.gpu_summary {
            println!("    gpu: {gpu}");
        } else {
            println!("    gpu: detected");
        }
    } else {
        println!("    gpu: none (cloud-only mode will be the default)");
    }
    match &env.ollama {
        OllamaState::NotFound => println!("    ollama: not installed"),
        OllamaState::InstalledNotRunning => println!("    ollama: installed but daemon offline"),
        OllamaState::Running { models } => println!(
            "    ollama: running ({} model{})",
            models.len(),
            if models.len() == 1 { "" } else { "s" }
        ),
    }
    println!();
}

fn collect_openrouter(out: &mut Option<CloudLlmChoice>) -> Result<Option<String>> {
    let api_key: String = Password::new()
        .with_prompt(
            "Enter your OpenRouter API key (or leave blank to use OPENROUTER_API_KEY env var)",
        )
        .allow_empty_password(true)
        .interact()
        .context("OpenRouter key input cancelled")?;
    let api_key = api_key.trim().to_string();

    let storage_choice = if !api_key.is_empty() {
        let choices = &[
            "Store in encrypted vault (recommended)",
            "Store as plaintext in config.yml",
            "Skip storing (use env var)",
        ];
        Select::new()
            .with_prompt("How should the OpenRouter key be stored?")
            .items(choices)
            .default(0)
            .interact()
            .context("key storage choice cancelled")?
    } else {
        2 // skip — env var
    };

    let plaintext_for_config = if storage_choice == 1 && !api_key.is_empty() {
        Some(api_key.clone())
    } else {
        None
    };

    *out = Some(CloudLlmChoice {
        key: "openrouter".to_string(),
        model: DEFAULT_OPENROUTER_MODEL.to_string(),
        api_key_plaintext: plaintext_for_config,
    });

    // Return the cleartext only when the user picked vault — caller
    // forwards into the vault flow.
    if storage_choice == 0 && !api_key.is_empty() {
        Ok(Some(api_key))
    } else {
        Ok(None)
    }
}

fn openrouter_should_use_vault(choice: &Option<CloudLlmChoice>) -> bool {
    matches!(
        choice,
        Some(c) if c.api_key_plaintext.is_none()
    )
}

fn collect_local_stack(env: &EnvSnapshot, out: &mut Option<LocalLlmChoice>) -> Result<()> {
    // Ollama install gate ---------------------------------------------------
    if matches!(env.ollama, OllamaState::NotFound) {
        let install = Confirm::new()
            .with_prompt(
                "Ollama is not installed. Install it now via the official script (curl … | sh)?",
            )
            .default(true)
            .interact()
            .context("ollama install prompt cancelled")?;
        if install {
            install_ollama()?;
        } else {
            println!(
                "  Skipping Ollama install — local LLM will not be available until you install it."
            );
            return Ok(());
        }
    }

    // Pull Qwen3 ------------------------------------------------------------
    let pull = Confirm::new()
        .with_prompt(format!(
            "Pull the Qwen3-14B GGUF model ({})? (≈9 GiB, one-time)",
            local_stack::QWEN3_MODEL_TAG
        ))
        .default(true)
        .interact()
        .context("qwen3 pull prompt cancelled")?;
    if pull {
        pull_qwen3()?;
    } else {
        println!(
            "  Skipping model pull — run `ollama pull {}` later if you change your mind.",
            local_stack::QWEN3_MODEL_TAG
        );
    }

    // Start Ollama (if not already running) ---------------------------------
    if !env.ollama.is_running() {
        let start = Confirm::new()
            .with_prompt("Start the Ollama daemon now?")
            .default(true)
            .interact()
            .context("ollama start prompt cancelled")?;
        if start {
            let home = dirs::home_dir().unwrap_or_else(|| Path::new(".").to_path_buf());
            start_ollama_systemd_or_nohup(env, &home)?;
        }
    }

    *out = Some(LocalLlmChoice::default());
    Ok(())
}

fn pick_host_port(env: &EnvSnapshot) -> (String, u16) {
    let host = if env.is_server_like() {
        "0.0.0.0".to_string()
    } else {
        "127.0.0.1".to_string()
    };
    let port = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(3888);
    (host, port)
}

fn open_or_create_vault(
    config_dir: &Path,
    openrouter_key: Option<&str>,
    telegram_token: Option<&str>,
) -> Result<()> {
    let vault_path = config_dir.join("credentials").join("vault.json");
    let mut vault_opt = if vault_path.exists() {
        let passphrase: String = Password::new()
            .with_prompt("Enter your existing vault passphrase")
            .interact()
            .context("passphrase input cancelled")?;
        match garraia_security::CredentialVault::open(&vault_path, &passphrase) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!(
                    "  Warning: vault open failed ({e}); secrets will fall back to env vars."
                );
                None
            }
        }
    } else {
        let passphrase: String = Password::new()
            .with_prompt("Set a vault passphrase")
            .with_confirmation("Confirm passphrase", "Passphrases don't match")
            .interact()
            .context("passphrase input cancelled")?;
        match garraia_security::CredentialVault::create(&vault_path, &passphrase) {
            Ok(v) => {
                println!("  Vault created.");
                println!("  Set GARRAIA_VAULT_PASSPHRASE env var for server mode.");
                Some(v)
            }
            Err(e) => {
                eprintln!(
                    "  Warning: vault creation failed ({e}); secrets will fall back to env vars."
                );
                None
            }
        }
    };

    if let Some(vault) = vault_opt.as_mut() {
        if let Some(key) = openrouter_key {
            vault.set("OPENROUTER_API_KEY", key);
            println!("  OpenRouter API key encrypted in vault.");
        }
        if let Some(tg) = telegram_token {
            vault.set("TELEGRAM_BOT_TOKEN", tg);
            println!("  Telegram bot token encrypted in vault.");
        }
        vault.save().context("failed to save vault")?;
    }
    Ok(())
}

// Silence unused-imports in case future refactors drop a re-export.
#[allow(dead_code)]
fn _unused_imports(_: HashMap<String, String>) {}
