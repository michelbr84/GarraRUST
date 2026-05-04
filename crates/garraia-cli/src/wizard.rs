use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};
use garraia_config::{AgentConfig, AppConfig, ChannelConfig, GatewayConfig, LlmProviderConfig};
use tracing::info;

/// Run the interactive onboarding wizard. Writes config.yml and optionally
/// stores the API key in the credential vault.
pub fn run_wizard(config_dir: &Path) -> Result<()> {
    if !std::io::stdin().is_terminal() {
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
        return Ok(());
    }

    println!();
    println!("  GarraIA Setup Wizard");
    println!("  ----------------------");
    println!();

    // --- Provider selection ---
    let providers = &["anthropic", "openai", "openrouter", "sansa"];
    let selection = Select::new()
        .with_prompt("Select your LLM provider")
        .items(providers)
        .default(0)
        .interact()
        .context("provider selection cancelled")?;
    let provider = providers[selection];

    // --- API key ---
    let env_hint = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "sansa" => "SANSA_API_KEY",
        _ => "API_KEY",
    };

    let api_key: String = Password::new()
        .with_prompt(format!(
            "Enter your {provider} API key (or set {env_hint} env var later)"
        ))
        .allow_empty_password(true)
        .interact()
        .context("API key input cancelled")?;

    let api_key = api_key.trim().to_string();

    // --- Vault storage ---
    let store_in_vault = if !api_key.is_empty() {
        let choices = &[
            "Store in encrypted vault (recommended)",
            "Store as plaintext in config.yml",
            "Skip storing (use env var)",
        ];
        Select::new()
            .with_prompt("How should the API key be stored?")
            .items(choices)
            .default(0)
            .interact()
            .context("storage choice cancelled")?
    } else {
        2 // skip
    };

    // --- System prompt ---
    let system_prompt: String = Input::new()
        .with_prompt("System prompt (optional)")
        .default("You are a helpful personal AI assistant.".to_string())
        .allow_empty(true)
        .interact_text()
        .context("system prompt input cancelled")?;

    // --- Telegram channel setup ---
    println!();
    println!("  ── Channel Setup ──");
    println!();

    let setup_telegram = Confirm::new()
        .with_prompt("Do you want to connect GarraIA to Telegram?")
        .default(false)
        .interact()
        .context("telegram prompt cancelled")?;

    let mut telegram_token = String::new();
    let mut telegram_token_storage: usize = 2; // default: skip (env var)

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

        telegram_token = token.trim().to_string();

        if !telegram_token.is_empty() {
            let choices = &[
                "Store in encrypted vault (recommended)",
                "Store as plaintext in config.yml",
                "Skip storing (use env var)",
            ];
            telegram_token_storage = Select::new()
                .with_prompt("How should the Telegram bot token be stored?")
                .items(choices)
                .default(0)
                .interact()
                .context("telegram storage choice cancelled")?;
        }
    }

    // --- Build config ---
    let mut llm_config = LlmProviderConfig {
        provider: provider.to_string(),
        model: None,
        api_key: None,
        base_url: if provider == "openrouter" {
            Some("https://openrouter.ai/api/v1".to_string())
        } else {
            None
        },
        extra: Default::default(),
    };

    // --- Vault creation (shared for LLM key + Telegram token) ---
    let needs_vault = (store_in_vault == 0 && !api_key.is_empty())
        || (telegram_token_storage == 0 && !telegram_token.is_empty());

    let mut vault_opt = if needs_vault {
        let vault_path = config_dir.join("credentials").join("vault.json");
        // Try to open existing vault first, or create a new one
        if vault_path.exists() {
            let passphrase: String = Password::new()
                .with_prompt("Enter your existing vault passphrase")
                .interact()
                .context("passphrase input cancelled")?;
            match garraia_security::CredentialVault::open(&vault_path, &passphrase) {
                Ok(vault) => Some(vault),
                Err(e) => {
                    println!("  Warning: vault open failed ({e}), will store in config instead.");
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
                Ok(vault) => {
                    println!("  Vault created.");
                    println!("  Set GARRAIA_VAULT_PASSPHRASE env var for server mode.");
                    Some(vault)
                }
                Err(e) => {
                    println!(
                        "  Warning: vault creation failed ({e}), will store in config instead."
                    );
                    None
                }
            }
        }
    } else {
        None
    };

    // Store LLM API key
    match store_in_vault {
        0 if !api_key.is_empty() => {
            if let Some(ref mut vault) = vault_opt {
                vault.set(env_hint, &api_key);
                println!("  LLM API key encrypted in vault.");
            } else {
                // Vault failed — fall back to config
                llm_config.api_key = Some(api_key.clone());
            }
        }
        1 => {
            llm_config.api_key = Some(api_key.clone());
        }
        _ => {
            if !api_key.is_empty() {
                println!("  Set {env_hint} environment variable before starting the server.");
            }
        }
    }

    // Store Telegram token
    let mut telegram_channel_settings = std::collections::HashMap::new();
    if setup_telegram {
        match telegram_token_storage {
            0 if !telegram_token.is_empty() => {
                if let Some(ref mut vault) = vault_opt {
                    vault.set("TELEGRAM_BOT_TOKEN", &telegram_token);
                    println!("  Telegram bot token encrypted in vault.");
                } else {
                    // Vault failed — fall back to config
                    telegram_channel_settings.insert(
                        "bot_token".to_string(),
                        serde_json::Value::String(telegram_token.clone()),
                    );
                }
            }
            1 => {
                telegram_channel_settings.insert(
                    "bot_token".to_string(),
                    serde_json::Value::String(telegram_token.clone()),
                );
            }
            _ => {
                if !telegram_token.is_empty() {
                    println!(
                        "  Set TELEGRAM_BOT_TOKEN environment variable before starting the server."
                    );
                } else {
                    println!("  Telegram enabled. Set TELEGRAM_BOT_TOKEN env var before starting.");
                }
            }
        }
    }

    // Save vault if anything was stored
    if let Some(ref mut vault) = vault_opt {
        vault.save().context("failed to save vault")?;
    }

    // --- Build channels map ---
    let mut channels = std::collections::HashMap::new();
    if setup_telegram {
        channels.insert(
            "telegram".to_string(),
            ChannelConfig {
                channel_type: "telegram".to_string(),
                enabled: Some(true),
                settings: telegram_channel_settings,
            },
        );
    }

    let config = AppConfig {
        gateway: GatewayConfig::default(),
        llm: [("main".to_string(), llm_config)].into_iter().collect(),
        channels,
        agent: AgentConfig {
            system_prompt: if system_prompt.is_empty() {
                None
            } else {
                Some(system_prompt)
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let config_path = config_dir.join("config.yml");
    let yaml = serde_yaml::to_string(&config).context("failed to serialize config")?;
    std::fs::write(&config_path, &yaml)
        .context(format!("failed to write {}", config_path.display()))?;

    info!("config written to {}", config_path.display());
    println!();
    println!("  Config written to {}", config_path.display());
    println!("  Run `garra start` to launch the gateway.");
    if setup_telegram {
        println!("  Your Telegram bot will start receiving messages automatically.");
        println!("  The first user to message the bot becomes the owner.");
    }
    println!();

    Ok(())
}
