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
use garraia_agents::normalize_ollama_tag;
use local_stack::{
    OLLAMA_PROVIDER_KEY, StdoutHints, install_ollama, print_stt_install_hints,
    print_tts_install_hints, pull_model, start_ollama_systemd_or_nohup, voice_endpoints_summary,
};

/// Default cloud model — matches `chat.rs` (`openrouter/auto`).
const DEFAULT_OPENROUTER_MODEL: &str = "openrouter/auto";

/// One cloud provider the wizard can configure end-to-end.
///
/// `key` doubles as the `llm:` map key **and** the `provider:` type string
/// consumed by `build_agent_runtime`; `env_var` doubles as the
/// credential-vault entry name — the gateway resolves both under the same
/// identifier (see `garraia-gateway/src/bootstrap/config.rs`).
struct CloudProviderPreset {
    key: &'static str,
    name: &'static str,
    label: &'static str,
    env_var: &'static str,
    default_model: &'static str,
    /// `None` lets the provider client use its own default endpoint.
    base_url: Option<&'static str>,
    key_url: &'static str,
}

/// Presets offered by the "Which cloud AI provider?" select, in display
/// order. OpenRouter stays first (and default): one key fronts many models.
/// Default models mirror the provider crates' own defaults
/// (`garraia-agents/src/{openai,anthropic}.rs`).
const CLOUD_PROVIDER_PRESETS: &[CloudProviderPreset] = &[
    CloudProviderPreset {
        key: "openrouter",
        name: "OpenRouter",
        label: "OpenRouter (recommended — one key, many models)",
        env_var: "OPENROUTER_API_KEY",
        default_model: DEFAULT_OPENROUTER_MODEL,
        base_url: Some("https://openrouter.ai/api/v1"),
        key_url: "https://openrouter.ai/keys",
    },
    CloudProviderPreset {
        key: "openai",
        name: "OpenAI",
        label: "OpenAI (GPT models)",
        env_var: "OPENAI_API_KEY",
        default_model: "gpt-4o",
        base_url: None,
        key_url: "https://platform.openai.com/api-keys",
    },
    CloudProviderPreset {
        key: "anthropic",
        name: "Anthropic",
        label: "Anthropic (Claude models)",
        env_var: "ANTHROPIC_API_KEY",
        default_model: "claude-sonnet-4-5-20250929",
        base_url: None,
        key_url: "https://console.anthropic.com/settings/keys",
    },
];

/// Where the wizard persists a secret it just collected.
///
/// The ordering of the variants is the ordering of the prompt, and
/// [`SecretStorage::Config`] is deliberately first and default. Vaulting a
/// secret requires `GARRAIA_VAULT_PASSPHRASE` to be present in the *gateway's*
/// environment at every single start; the wizard cannot arrange that, so a
/// vault default meant `garraia init` → `garraia start` produced an encrypted
/// key the server could not open and a provider that silently never came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretStorage {
    /// Into `config.yml`, which [`write_config`] clamps to mode `0600`.
    Config,
    /// Into `credentials/vault.json`, AES-encrypted under a passphrase the
    /// operator must re-supply out-of-band on every start.
    Vault,
    /// Nowhere — the operator exports the env var themselves.
    Env,
}

/// Prompt for where to keep `what` (a human label like `"OpenRouter API key"`),
/// naming `env_var` in the skip option so the operator knows the exact variable.
fn prompt_secret_storage(what: &str, env_var: &str) -> Result<SecretStorage> {
    let choices = [
        "Store in config.yml (recommended — the file is chmod 0600)".to_string(),
        "Store in the encrypted vault (needs GARRAIA_VAULT_PASSPHRASE on every start)".to_string(),
        format!("Skip storing (I will export {env_var} myself)"),
    ];
    let picked = Select::new()
        .with_prompt(format!("How should the {what} be stored?"))
        .items(&choices)
        .default(0)
        .interact()
        .with_context(|| format!("{what} storage choice cancelled"))?;
    Ok(match picked {
        0 => SecretStorage::Config,
        1 => SecretStorage::Vault,
        _ => SecretStorage::Env,
    })
}

/// Tell the operator, unmissably, that a vaulted secret is inert until the
/// passphrase reaches the gateway. Printed as a block rather than a one-line
/// `println!` because the previous single line scrolled away behind the wizard's
/// remaining output and the boot log, and operators never saw it.
fn print_vault_passphrase_warning() {
    println!();
    println!("  ┌─────────────────────────────────────────────────────────────────┐");
    println!("  │  ⚠  ATENÇÃO — leia antes de iniciar o Garra                     │");
    println!("  └─────────────────────────────────────────────────────────────────┘");
    println!("  Seus segredos foram cifrados no cofre. O servidor NÃO consegue");
    println!("  abri-lo sozinho: ele precisa da mesma senha, via variável de");
    println!("  ambiente, em TODA inicialização. Sem ela o Garra sobe, mas os");
    println!("  provedores e canais ficam desligados.");
    println!();
    println!("    export GARRAIA_VAULT_PASSPHRASE='<a senha que você acabou de criar>'");
    println!();
    println!("  Para que isso sobreviva a reboots, coloque a linha acima no seu");
    println!("  ~/.bashrc, ou num EnvironmentFile da sua unit systemd.");
    println!();
}

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
    println!("  Oi! 👋 Vamos configurar o Garra juntos — leva só um minutinho.");
    println!("  ----------------------------------------------------------------");
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
                    "Cloud-first (cloud provider primary + Ollama fallback)",
                    "Cloud-only (OpenRouter / OpenAI / Anthropic — no local stack)",
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

    let mut cloud_choice: Option<CloudLlmChoice> = None;
    let mut local_choice: Option<LocalLlmChoice> = None;
    let mut fallback_providers: Vec<String> = Vec::new();

    // --- 4. Cloud branch ---------------------------------------------------
    let want_cloud = matches!(mode_idx, 0..=2);
    let cloud_secret_for_vault = if want_cloud {
        collect_cloud_provider(&mut cloud_choice)?
    } else {
        None
    };

    // --- 5. Local branch ---------------------------------------------------
    if matches!(mode_idx, 0 | 1) && local_available {
        collect_local_stack(&env, &mut local_choice)?;
    }

    // --- 6. Resolve default / fallback ordering ----------------------------
    let default_provider: String = match (local_choice.as_ref(), cloud_choice.as_ref()) {
        (Some(_), Some(cloud)) if mode_idx == 0 => {
            fallback_providers = vec![cloud.key.clone()];
            OLLAMA_PROVIDER_KEY.to_string()
        }
        (Some(_), Some(cloud)) if mode_idx == 1 => {
            fallback_providers = vec![OLLAMA_PROVIDER_KEY.to_string()];
            cloud.key.clone()
        }
        (Some(_), None) => OLLAMA_PROVIDER_KEY.to_string(),
        (None, Some(cloud)) => cloud.key.clone(),
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
    // Plan 0250 (GAR-771): leaving this empty now gives Garra its warm default
    // persona automatically (resolved at runtime). Only fill it in to give Garra
    // a *custom* personality.
    println!();
    println!("  Personalidade: deixe em branco e eu já falo com o jeitinho do Garra.");
    println!("  (Preencha só se quiser me dar uma personalidade diferente.)");
    let system_prompt_input: String = Input::new()
        .with_prompt("Personalidade do Garra (opcional)")
        .default(String::new())
        .allow_empty(true)
        .interact_text()
        .context("system prompt input cancelled")?;
    let system_prompt = if system_prompt_input.trim().is_empty() {
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
            // Same defect as the OpenRouter key: a vault default left the
            // channel silently offline on every restart, because the gateway
            // has no passphrase to open the vault with.
            match prompt_secret_storage("Telegram bot token", "TELEGRAM_BOT_TOKEN")? {
                SecretStorage::Config => telegram_token_plaintext = Some(token.clone()),
                SecretStorage::Vault => telegram_token_for_vault = Some(token.clone()),
                SecretStorage::Env => {}
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
    // `collect_cloud_provider` / the Telegram prompt hand back a cleartext
    // secret only when the operator explicitly chose the vault, so no further
    // filtering is needed here.
    let needs_vault = cloud_secret_for_vault.is_some() || telegram_token_for_vault.is_some();
    if needs_vault {
        open_or_create_vault(
            config_dir,
            cloud_secret_for_vault
                .as_ref()
                .map(|(entry, key)| (entry.as_str(), key.as_str())),
            telegram_token_for_vault.as_deref(),
        )?;
        print_vault_passphrase_warning();
    }

    // --- 11. Build outcome + write config ---------------------------------
    let (host, port) = pick_host_port(&env);
    let outcome = WizardOutcome {
        host,
        port,
        default_provider,
        fallback_providers,
        cloud: cloud_choice,
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
    println!("  Prontinho! 🎉 O Garra está configurado e pronto pra te ajudar.");
    println!("  Config salva em {}", written.display());
    println!("  Agora é só rodar `garraia start` pra eu entrar no ar.");
    println!("  Pra parar, é só Ctrl+C. Pra rodar em segundo plano: garraia start -d");
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

/// Cloud branch: pick one of [`CLOUD_PROVIDER_PRESETS`], then collect and
/// route its API key. Returns `Some((vault_entry_name, cleartext))` only when
/// the operator explicitly chose the vault — the caller forwards that pair
/// into the vault flow.
fn collect_cloud_provider(out: &mut Option<CloudLlmChoice>) -> Result<Option<(String, String)>> {
    let labels: Vec<&str> = CLOUD_PROVIDER_PRESETS.iter().map(|p| p.label).collect();
    let picked = Select::new()
        .with_prompt("Which cloud AI provider?")
        .items(&labels)
        .default(0)
        .interact()
        .context("cloud provider choice cancelled")?;
    let preset = &CLOUD_PROVIDER_PRESETS[picked];

    println!("  No key yet? Create one at {}", preset.key_url);
    let api_key: String = Password::new()
        .with_prompt(format!(
            "Enter your {} API key (or leave blank to use the {} env var)",
            preset.name, preset.env_var
        ))
        .allow_empty_password(true)
        .interact()
        .with_context(|| format!("{} key input cancelled", preset.name))?;
    let api_key = api_key.trim().to_string();

    let storage = if api_key.is_empty() {
        SecretStorage::Env
    } else {
        prompt_secret_storage(&format!("{} API key", preset.name), preset.env_var)?
    };

    let plaintext_for_config = match storage {
        SecretStorage::Config => Some(api_key.clone()),
        SecretStorage::Vault | SecretStorage::Env => None,
    };

    *out = Some(CloudLlmChoice {
        key: preset.key.to_string(),
        provider: preset.key.to_string(),
        model: preset.default_model.to_string(),
        base_url: preset.base_url.map(str::to_string),
        api_key_plaintext: plaintext_for_config,
    });

    // Return the cleartext only when the user picked vault — caller
    // forwards into the vault flow.
    match storage {
        SecretStorage::Vault => Ok(Some((preset.env_var.to_string(), api_key))),
        SecretStorage::Config | SecretStorage::Env => Ok(None),
    }
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

    // Pick + pull the local model -------------------------------------------
    let mut choice = LocalLlmChoice::default();
    let labels: Vec<&str> = local_stack::MODEL_CHOICES.iter().map(|c| c.label).collect();
    let picked = Select::new()
        .with_prompt("Qual modelo local o Garra deve usar?")
        .items(&labels)
        .default(0)
        .interact()
        .context("local model selection cancelled")?;

    let tag: Option<String> = if picked == local_stack::MODEL_CHOICE_SKIP {
        None
    } else if picked == local_stack::MODEL_CHOICE_CUSTOM {
        // Free-form: any Ollama tag, including `hf.co/…` registry refs.
        let typed: String = Input::new()
            .with_prompt("Tag do Ollama (ex.: qwen3.8:latest, llama3.1, hf.co/user/repo:Q4_K_M)")
            .interact_text()
            .context("custom model tag prompt cancelled")?;
        match normalize_ollama_tag(&typed) {
            Some(t) => Some(t),
            None => {
                println!("  '{typed}' nao parece uma tag do Ollama — pulando o download.");
                None
            }
        }
    } else {
        local_stack::MODEL_CHOICES[picked]
            .tag
            .map(|t| t.to_string())
    };

    match tag {
        Some(tag) => {
            // The picked tag is what lands in config.yml, whether or not the
            // pull succeeds — a failed download is recoverable with a later
            // `ollama pull`, but a config pointing at the wrong model is not
            // something the user would think to check.
            choice.model = tag.clone();
            if pull_model(&tag).is_err() {
                println!("  Download falhou — rode `ollama pull {tag}` depois para concluir.");
            }
        }
        None => {
            println!(
                "  Sem download agora. O config vai apontar para {} — rode `ollama pull {}` quando quiser.",
                choice.model, choice.model
            );
        }
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

    *out = Some(choice);
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
    // `(vault entry name, cleartext)` — the entry name is the provider's
    // env-var identifier, which is also what the gateway looks the secret
    // up under at boot.
    cloud_secret: Option<(&str, &str)>,
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
                // The passphrase reminder is printed once, as a block, by
                // `print_vault_passphrase_warning` after this function returns.
                println!("  Vault created.");
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
        if let Some((entry, key)) = cloud_secret {
            vault.set(entry, key);
            println!("  Cloud provider API key encrypted in vault (entry {entry}).");
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
