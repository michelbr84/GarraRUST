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
    backup_path_for, build_app_config, gateway_api_key_for_host, host_is_loopback, write_config,
};
use env_detect::{EnvSnapshot, OllamaState};
use garraia_agents::normalize_ollama_tag;
use local_stack::{
    OLLAMA_PROVIDER_KEY, StdoutHints, install_ollama, print_stt_install_hints,
    print_tts_install_hints, pull_model, start_ollama_systemd_or_nohup, voice_endpoints_summary,
};

/// Default cloud model — re-exported from [`crate::defaults`], the single
/// source of truth shared with `chat.rs` and `mcp_server.rs` (issue #1180).
const DEFAULT_OPENROUTER_MODEL: &str = crate::defaults::DEFAULT_CLOUD_MODEL;

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
/// order. OpenRouter stays first (and default): one key fronts many models,
/// and issue #1180 makes it *the* official default provider of the project.
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
    // Issue #1180: the cloud default (OpenRouter + `z-ai/glm-5.3-flash`) is
    // the official default of every installation, and the local stack is the
    // *second* option — offered when the machine can host it, never
    // preselected. Cloud-first is therefore the highlighted entry even on a
    // GPU box; local-first stays one keypress away for whoever wants it.
    let local_available = env.supports_local_stack() && local_bootstrap_enabled();
    let (mode_idx, mode_default) = if local_available {
        // The displayed order is cloud-first, local-first, cloud-only. The
        // rest of this function reads `mode_idx` as 0 = local-first,
        // 1 = cloud-first, 2 = cloud-only, so remap here rather than
        // renumbering every downstream match arm.
        let picked = Select::new()
            .with_prompt("Which LLM mode?")
            .items([
                "Cloud-first (recommended — OpenRouter primary + Ollama fallback)",
                "Local-first (second option — Ollama on this GPU primary + cloud fallback)",
                "Cloud-only (OpenRouter / OpenAI / Anthropic — no local stack)",
            ])
            .default(0)
            .interact()
            .context("provider mode cancelled")?;
        match picked {
            0 => (1, "cloud-first"),
            1 => (0, "local"),
            other => (other, "cloud-only"),
        }
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
        // #1180: `mode_idx == 0` is local-first — there the local stack is
        // what the user just asked for, so the prompts stay preselected.
        // `mode_idx == 1` is cloud-first, where the local stack is the
        // *second* option: same prompts, but nothing preselected, so nobody
        // installs Ollama and downloads ~18 GB by pressing Enter through a
        // wizard whose primary answer is already OpenRouter.
        collect_local_stack(&env, local_stack_role(mode_idx), &mut local_choice)?;
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
            // edit later. #1180: the placeholder is the project default,
            // read from the shared constant instead of repeated here.
            crate::defaults::DEFAULT_CLOUD_PROVIDER.to_string()
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
    // #1241: um bind nao-loopback sai daqui com credencial. Em loopback a
    // chave e `None` e nada muda para quem instala no proprio laptop.
    let gateway_api_key = gateway_api_key_for_host(&host)?;
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
        gateway_api_key,
    };

    // Sanity-check the outcome can serialize cleanly before we touch
    // the existing file. (`build_app_config` is also exercised in the
    // unit tests so this is a defense-in-depth check.)
    let _ = build_app_config(&outcome);
    // Lido antes do `write_config`, que consome a estrategia: so o caminho
    // `Backup` troca uma credencial de operador que ja existia (#1241).
    let era_backup = matches!(strategy, ExistingConfigStrategy::Backup { .. });
    let written = write_config(config_dir, &outcome, strategy)?;

    // A credencial do gateway nao entra em log estruturado NEM na saida do
    // terminal: o `info!` abaixo reporta so o caminho, e o resumo manda ler o
    // arquivo (#1241).
    info!("config written to {}", written.path.display());

    // --- 12. Final summary -------------------------------------------------
    println!();
    println!("  Prontinho! 🎉 O Garra está configurado e pronto pra te ajudar.");
    println!("  Config salva em {}", written.path.display());
    println!("  Agora é só rodar `garraia start` pra eu entrar no ar.");
    println!("  Pra parar, é só Ctrl+C. Pra rodar em segundo plano: garraia start -d");
    println!("  Logs: {}/garraia.log", config_dir.display());
    if outcome.voice_enabled {
        println!("  Voice was enabled — see docs/voice.md to install Chatterbox + faster-whisper.");
    }
    // #1241 — o aviso de bind exposto é a última coisa que o operador lê. Ele
    // aponta para o arquivo; a credencial não é impressa (ver a doc de
    // `aviso_de_bind_exposto`).
    if written.gateway_api_key_written {
        println!();
        println!(
            "{}",
            aviso_de_bind_exposto(&outcome.host, outcome.port, &written.path)
        );
        if era_backup {
            println!("{AVISO_BACKUP_TROCOU_A_CREDENCIAL}");
        }
    } else if !host_is_loopback(&outcome.host) {
        // Merge sobre um config que já trazia credencial: nada foi gravado,
        // então não há segredo a imprimir — mas o operador ainda merece saber
        // que o bind alcança a rede.
        println!();
        println!(
            "{}",
            aviso_de_bind_exposto_com_chave_preservada(&outcome.host, outcome.port)
        );
    }
    println!();

    Ok(())
}

// ---------- helpers -----------------------------------------------------------

/// Aviso de bind exposto impresso no fim do wizard (#1241).
///
/// **A credencial não aparece na saída.** O aviso diz onde ela está.
///
/// Por quê: a chave acabou de ser gravada no `config.yml` em modo 0600, então
/// o terminal nunca foi o único canal de recuperação — imprimir era
/// conveniência, e o custo é real (scrollback, `tee`/pipe da saída do `init`,
/// captura de stdout em automação, screenshot). A regra absoluta 6 do
/// `CLAUDE.md` não abre exceção para stdout, e o CodeQL apontou exatamente
/// este fluxo no #1252 (`rust/cleartext-logging`, HIGH): ele estava certo, e
/// a correção é parar de imprimir, não suprimir o alerta no ledger.
///
/// De quebra, a versão anterior afirmava "Guarde agora — ela não é impressa
/// de novo", o que era **falso**: a chave está no arquivo do operador. A
/// frase podia empurrá-lo a rodar `garra init` de novo por pânico — que é
/// justamente o caminho de merge onde mora o risco sobre o config existente.
///
/// Continua função pura porque é o único jeito de um teste afirmar o que a
/// saída interativa não deixa afirmar.
fn aviso_de_bind_exposto(host: &str, port: u16, config_path: &Path) -> String {
    let cabecalho = aviso_de_bind_exposto_cabecalho(host, port);
    let caminho = config_path.display();
    format!(
        "{cabecalho}\n\
         \x20 Gerei uma credencial de acesso e gravei em {caminho} (modo 0600).\n\
         \x20 Ela não é impressa aqui de propósito — saída de terminal vai parar em\n\
         \x20 scrollback, pipe e captura de automação. Leia o valor no campo\n\
         \x20 `gateway.api_key` desse arquivo e use assim:\n\
         \n\
         \x20     curl -H \"Authorization: Bearer <a chave do config>\" http://SEU-HOST:{port}/api/sessions\n\
         \n\
         \x20 /api/health e /api/capabilities seguem abertas — é por elas que o app\n\
         \x20 descobre este Garra antes de você digitar a chave."
    )
}

/// Linha extra do caminho `Backup` (#1241, code review do #1252).
///
/// "Backup the existing config and write a new one" e o **default** do
/// `Select`, e reconstruir o config troca uma `gateway.api_key` que ja
/// existia. Quem apertou Enter sem ler precisa saber que os clientes
/// antigos pararam de autenticar e onde esta o valor anterior.
const AVISO_BACKUP_TROCOU_A_CREDENCIAL: &str = "\x20 Se o config anterior ja tinha uma credencial de gateway, ela foi SUBSTITUIDA:\n\
     \x20 atualize seus clientes (app, scripts, reverse proxy). O valor antigo continua\n\
     \x20 no arquivo .bak- que acabei de criar ao lado do config.";

/// Mesmo aviso para o caso em que o merge **preservou** a credencial que o
/// operador já tinha: nada novo foi gravado, então nada de segredo é impresso.
fn aviso_de_bind_exposto_com_chave_preservada(host: &str, port: u16) -> String {
    format!(
        "{}\n  A credencial que já estava no config foi mantida — siga usando ela.",
        aviso_de_bind_exposto_cabecalho(host, port)
    )
}

fn aviso_de_bind_exposto_cabecalho(host: &str, port: u16) -> String {
    format!("  Atenção: o gateway vai ouvir em {host}:{port} — alcançável pela rede.")
}

fn print_non_interactive_hint(config_dir: &Path) {
    println!("Non-interactive environment detected.");
    println!(
        "To configure GarraIA, edit: {}/config.yml",
        config_dir.display()
    );
    println!();
    println!("Minimal config.yml example:");
    println!("---");
    // #1241: este e o caminho de container/CI, justamente o que tem mais
    // chance de rodar com HOST=0.0.0.0 — e sem credencial de gateway o gate
    // de /api/* e /ws fica desligado. O exemplo minimo tem que dizer isso,
    // porque aqui nao ha wizard para mintar a credencial.
    println!("gateway:");
    println!("  host: 127.0.0.1   # 0.0.0.0 expoe /api/* e /ws a rede inteira");
    println!("  api_key: <32 bytes aleatorios em hex>   # exigido se host nao for loopback");
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

/// #1180 — what the local stack *is* in the run being configured, which is
/// the only thing that decides whether its prompts come preselected.
///
/// The distinction is not cosmetic: preselected means a user who presses
/// Enter through the wizard installs Ollama and pulls ~18 GB. That is the
/// right default when they just chose local-first, and the wrong one when
/// they chose cloud-first and the local stack is merely on offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalStackRole {
    /// Local-first: the user picked the local stack as their primary.
    Primary,
    /// Cloud-first: local is the second option, opt-in only.
    Fallback,
}

impl LocalStackRole {
    fn is_primary(self) -> bool {
        matches!(self, LocalStackRole::Primary)
    }

    /// Default answer of the "Ollama is not installed. Install it now?"
    /// confirm. `false` on the cloud-first path: installing is a yes the
    /// user has to type.
    fn installs_ollama_by_default(self) -> bool {
        self.is_primary()
    }

    /// Row preselected in the local-model picker. Row 0 is
    /// `qwen3.8:latest` (~18 GB) and picking it starts the pull
    /// immediately, so cloud-first lands on the "skip the download" row.
    fn default_model_row(self) -> usize {
        if self.is_primary() {
            0
        } else {
            local_stack::MODEL_CHOICE_SKIP
        }
    }
}

/// #1180 — which role the local stack plays for a given wizard mode.
///
/// `mode_idx` is the remapped index the rest of this module uses:
/// 0 = local-first, 1 = cloud-first, 2 = cloud-only. Only 0 and 1 ever
/// reach `collect_local_stack`; 2 is treated as cloud-first here so the
/// mapping is total and can never fall open onto the ~18 GB default.
fn local_stack_role(mode_idx: usize) -> LocalStackRole {
    if mode_idx == 0 {
        LocalStackRole::Primary
    } else {
        LocalStackRole::Fallback
    }
}

/// Local branch of the wizard. Issue #1180: this is the project's *second*
/// option — the fallback the runtime reaches for when the cloud default is
/// unavailable, or the primary only when the user explicitly picked
/// local-first in the mode prompt above.
///
/// `role` carries that distinction into every default below: on the
/// cloud-first path the install prompt answers "no" and the model picker
/// lands on "skip", so the ~18 GB download is an explicit yes, never the
/// consequence of holding Enter.
fn collect_local_stack(
    env: &EnvSnapshot,
    role: LocalStackRole,
    out: &mut Option<LocalLlmChoice>,
) -> Result<()> {
    if role.is_primary() {
        println!("  Local stack — your primary: Garra runs on this machine and");
        println!("  falls back to the cloud provider you just configured.");
    } else {
        println!("  Local stack — the second option: Garra falls back to it when");
        println!("  the cloud default is unreachable (see `agent.fallback_providers`).");
        println!("  Nothing here is preselected: the cloud is already your default.");
    }

    // Ollama install gate ---------------------------------------------------
    if matches!(env.ollama, OllamaState::NotFound) {
        let install = Confirm::new()
            .with_prompt(
                "Ollama is not installed. Install it now via the official script (curl … | sh)?",
            )
            .default(role.installs_ollama_by_default())
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
    // #1180: cloud-first lands on "skip". Row 0 is `qwen3.8:latest` (~18 GB)
    // and the pull starts as soon as it is picked, so it may only be the
    // preselected answer when the user asked for a local-first install.
    let picked = Select::new()
        .with_prompt("Qual modelo local o Garra deve usar (segunda opcao / fallback)?")
        .items(&labels)
        .default(role.default_model_row())
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- #1241: fiacao do mint da credencial de gateway -------------------
    //
    // `gateway_api_key_for_host` tem teste proprio em `config_writer`, mas o
    // PONTO DE MONTAGEM (`run_wizard`, onde o host escolhido vira o campo
    // `gateway_api_key` do `WizardOutcome`) nao tinha cobertura nenhuma:
    // trocar aquela linha por `gateway_api_key: None` deixava os 41 testes do
    // wizard verdes e devolvia a issue #1241 inteira.
    //
    // `run_wizard` exige TTY e uma dezena de prompts, entao nao da para
    // chama-lo de um teste. A cobertura vem em duas camadas: um teste de
    // COMPORTAMENTO sobre o par de funcoes que ele compoe, e um teste que
    // varre o proprio fonte para fixar o elo entre as duas — o mesmo idioma
    // que o `spinner.rs` e o `garraia-desktop-core::detect` ja usam neste
    // repo para call sites que nenhum teste alcanca.

    fn env_de_teste(is_root: bool, is_runpod: bool) -> EnvSnapshot {
        EnvSnapshot {
            os: env_detect::OsId::Linux {
                distro: "debian".into(),
                version: "13".into(),
            },
            is_root,
            is_runpod,
            has_systemd: false,
            has_nvidia: false,
            gpu_summary: None,
            ollama: env_detect::OllamaState::NotFound,
            ports: env_detect::PortReport::default(),
        }
    }

    /// Comportamento: o host que o wizard escolhe e a credencial que ele
    /// minta tem que andar juntos. Servidor (root/RunPod) => `0.0.0.0` COM
    /// credencial; laptop => loopback SEM credencial.
    #[test]
    fn host_de_servidor_sai_com_credencial_e_laptop_sai_sem() {
        for (is_root, is_runpod) in [(true, false), (false, true), (true, true)] {
            let (host, _porta) = pick_host_port(&env_de_teste(is_root, is_runpod));
            assert_eq!(host, "0.0.0.0", "root={is_root} runpod={is_runpod}");
            let chave =
                config_writer::gateway_api_key_for_host(&host).expect("CSPRNG do sistema no teste");
            assert!(
                chave.is_some(),
                "bind exposto tem que sair do wizard COM credencial"
            );
        }

        let (host, _porta) = pick_host_port(&env_de_teste(false, false));
        assert_eq!(host, "127.0.0.1");
        assert!(
            config_writer::gateway_api_key_for_host(&host)
                .expect("CSPRNG do sistema no teste")
                .is_none(),
            "o laptop nao pode ganhar credencial: nada muda para quem instala local"
        );
    }

    /// Fiacao: fixa que `run_wizard` de fato liga as duas pontas acima, e que
    /// a credencial nao escapa para a saida do terminal.
    ///
    /// Varredura de fonte e um instrumento grosseiro — ela nao roda o codigo,
    /// e uma renomeacao honesta a quebra. Ela esta aqui porque a alternativa
    /// hoje e ZERO cobertura num ponto onde a regressao e "o gateway volta
    /// para a internet sem credencial". Quem renomear, renomeie aqui tambem.
    #[test]
    fn run_wizard_monta_o_outcome_com_a_credencial_do_host() {
        let fonte = include_str!("mod.rs");
        let corpo = fonte
            .split_once("pub fn run_wizard(")
            .expect("run_wizard existe neste arquivo")
            .1
            .split_once("\n// ---------- helpers")
            .expect("run_wizard termina antes do bloco de helpers")
            .0;
        let literal = corpo
            .split_once("let outcome = WizardOutcome {")
            .expect("run_wizard monta um WizardOutcome")
            .1
            .split_once("};")
            .expect("o literal do WizardOutcome fecha")
            .0;

        assert!(
            corpo.contains("gateway_api_key_for_host(&host)"),
            "run_wizard tem que mintar a credencial a partir do host RESOLVIDO"
        );
        assert!(
            literal.contains("gateway_api_key,"),
            "o WizardOutcome tem que receber a credencial mintada, por shorthand"
        );
        assert!(
            !literal.contains("gateway_api_key:"),
            "nenhum valor literal no campo: ele so pode vir de gateway_api_key_for_host"
        );

        // #1252 / CodeQL `rust/cleartext-logging`: a credencial esta em escopo
        // dentro de `run_wizard` (`outcome.gateway_api_key`) e o resumo final
        // e um monte de `println!`. Nenhum deles pode toca-la.
        for linha in corpo.lines() {
            let l = linha.trim_start();
            if l.starts_with("//") || l.starts_with("///") {
                continue;
            }
            assert!(
                !(l.contains("println!") && l.contains("gateway_api_key")),
                "a credencial nao pode ir para stdout — mande ler o config: {linha}"
            );
        }
        let bloco_resumo = corpo
            .split_once("--- 12. Final summary")
            .expect("o resumo final existe")
            .1;
        assert!(
            !bloco_resumo.contains("outcome.gateway_api_key"),
            "o resumo final nao pode nem alcancar a credencial"
        );
    }

    /// #1180 — o caminho cloud-first nao pode empurrar o download de ~18 GB.
    /// `collect_local_stack` roda nos dois modos (0 = local-first,
    /// 1 = cloud-first) e antes desta correcao os dois viam `.default(true)`
    /// no confirm de instalar o Ollama e a linha 0 (`qwen3.8:latest`, ~18 GB,
    /// com `pull_model` imediato) preselecionada no seletor de modelo. Quem
    /// escolhia "Cloud-first (recommended)" e seguia apertando Enter baixava
    /// o modelo local do mesmo jeito.
    #[test]
    fn cloud_first_nao_preseleciona_o_download_local() {
        let papel = local_stack_role(1);
        assert_eq!(papel, LocalStackRole::Fallback);
        assert!(
            !papel.installs_ollama_by_default(),
            "no caminho cloud-first instalar o Ollama tem que ser um sim explicito"
        );
        assert_eq!(
            papel.default_model_row(),
            local_stack::MODEL_CHOICE_SKIP,
            "no caminho cloud-first o seletor tem que cair na linha de pular o download"
        );
        assert!(
            local_stack::MODEL_CHOICES[papel.default_model_row()]
                .tag
                .is_none(),
            "a linha preselecionada no cloud-first nao pode ter tag (tag = pull imediato)"
        );
    }

    /// O espelho: quem escolheu local-first pediu o modelo local, entao ali
    /// os defaults continuam onde estavam — instalar sim, linha 0 do seletor.
    #[test]
    fn local_first_mantem_os_defaults_de_antes() {
        let papel = local_stack_role(0);
        assert_eq!(papel, LocalStackRole::Primary);
        assert!(papel.installs_ollama_by_default());
        assert_eq!(papel.default_model_row(), 0);
        assert_eq!(
            local_stack::MODEL_CHOICES[papel.default_model_row()].tag,
            Some(local_stack::DEFAULT_OLLAMA_MODEL_TAG),
            "local-first continua caindo no modelo local padrao do projeto"
        );
    }

    /// O mapa e total: qualquer indice que nao seja o local-first cai em
    /// `Fallback`. Um modo novo no seletor nunca pode herdar o download por
    /// acidente.
    #[test]
    fn todo_modo_que_nao_e_local_first_e_fallback() {
        for idx in [1usize, 2, 3, 99] {
            assert_eq!(
                local_stack_role(idx),
                LocalStackRole::Fallback,
                "mode_idx {idx} devia ser fallback"
            );
        }
    }

    /// #1241 — o resumo imprime a credencial **uma vez so**, dentro da linha
    /// de `curl` que o operador vai copiar. Duas ocorrencias seriam duas
    /// chances de a chave vazar para o scrollback de alguem.
    #[test]
    fn o_aviso_de_bind_exposto_manda_ler_o_config_em_vez_de_imprimir() {
        let caminho = std::path::PathBuf::from("/root/.garraia/config.yml");
        let saida = aviso_de_bind_exposto("0.0.0.0", 3888, &caminho);

        // A propriedade inverteu (auditoria do #1252 + CodeQL HIGH): antes
        // este teste afirmava que a chave saia "uma vez so"; agora afirma que
        // ela NAO sai. A assinatura e a primeira linha de defesa — reintroduzir
        // um parametro de credencial aqui nem compila.
        assert!(
            !saida.chars().collect::<Vec<_>>().windows(32).any(|j| j
                .iter()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())),
            "nenhuma corrida longa de hex pode aparecer na saida:\n{saida}"
        );
        assert!(
            saida.contains("/root/.garraia/config.yml"),
            "o aviso tem que dizer em que ARQUIVO a chave esta:\n{saida}"
        );
        assert!(
            saida.contains("gateway.api_key"),
            "e em que CAMPO desse arquivo:\n{saida}"
        );
        assert!(
            saida.contains("Authorization: Bearer <a chave do config>"),
            "a linha de curl continua util, com placeholder no lugar do segredo:\n{saida}"
        );
        assert!(
            saida.contains("0.0.0.0:3888"),
            "o aviso nomeia o bind:\n{saida}"
        );
        assert!(
            saida.contains("/api/health") && saida.contains("/api/capabilities"),
            "o aviso diz quais rotas seguem abertas:\n{saida}"
        );
        assert!(
            !saida.contains("não é impressa de novo"),
            "a frase antiga era falsa: a chave esta no config do operador:\n{saida}"
        );
    }

    /// A linha extra do `Backup` tem que dizer as tres coisas que importam:
    /// que a credencial mudou, que os clientes precisam ser atualizados, e
    /// onde esta o valor antigo. Sem imprimir segredo nenhum.
    #[test]
    fn o_aviso_de_backup_diz_que_a_credencial_antiga_foi_trocada() {
        let t = AVISO_BACKUP_TROCOU_A_CREDENCIAL;
        assert!(t.contains("SUBSTITUIDA"), "{t}");
        assert!(t.contains("atualize seus clientes"), "{t}");
        assert!(t.contains(".bak-"), "{t}");
        assert!(!t.contains("Bearer"), "nada de credencial aqui: {t}");
    }

    /// O caminho do merge que preservou a chave do operador: avisa do bind,
    /// mas nao inventa nem imprime segredo nenhum.
    #[test]
    fn o_aviso_com_chave_preservada_nao_imprime_segredo() {
        let saida = aviso_de_bind_exposto_com_chave_preservada("0.0.0.0", 3888);
        assert!(saida.contains("0.0.0.0:3888"));
        assert!(
            !saida.contains("Bearer"),
            "sem chave gravada nao ha header para oferecer:\n{saida}"
        );
    }

    /// #1180 — o placeholder de `agent.default_provider` (nem nuvem nem
    /// local escolhidos) e a constante compartilhada, nao um literal solto.
    #[test]
    fn o_placeholder_do_default_provider_e_a_constante() {
        assert_eq!(crate::defaults::DEFAULT_CLOUD_PROVIDER, "openrouter");
        assert_eq!(
            DEFAULT_OPENROUTER_MODEL,
            crate::defaults::DEFAULT_CLOUD_MODEL
        );
    }
}
