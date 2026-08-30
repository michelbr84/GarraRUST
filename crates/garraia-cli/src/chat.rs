//! GarraIA interactive chat REPL.
//!
//! `garraia chat` or just `garra` opens a local-first AI assistant
//! that streams responses from Ollama (offline) or cloud providers (online).

use std::future::Future;
use std::io::{self, BufRead, Write as _};
use std::sync::Arc;

use anyhow::{Context, Result};
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, ChatMessage, ChatRole, FileReadTool, FileWriteTool,
    LlmProvider, MessagePart, OllamaProvider, OpenAiProvider, normalize_ollama_tag,
    tools::git_diff_tool::GitDiffTool,
};
use garraia_config::AppConfig;
use tokio::sync::mpsc;

use std::path::Path;

/// ANSI color helpers
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Print the Garra chat banner.
pub fn print_chat_banner(provider: &str, model: &str, mode: &str) {
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!("{CYAN}{BOLD}╭──────────────────────────────────────────────╮{RESET}");
    println!(
        "{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}      {YELLOW}{BOLD}_~^~^~_{RESET}                                {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}   {YELLOW}{BOLD}\\) /  o o  \\ (/{RESET}   {GREEN}{BOLD}GarraIA v{version}{RESET}         {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}     {YELLOW}{BOLD}'_   -   _'{RESET}    Personal AI Assistant   {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}     {YELLOW}{BOLD}/ '-----' \\{RESET}                            {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}  {DIM}Provider:{RESET} {GREEN}{provider:<15}{RESET} {DIM}Mode:{RESET} {GREEN}{mode:<8}{RESET}  {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}  {DIM}Model:{RESET}    {GREEN}{model:<33}{RESET} {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}  {DIM}/help  /model  /provider  /clear  /exit{RESET}  {CYAN}{BOLD}│{RESET}"
    );
    println!("{CYAN}{BOLD}╰──────────────────────────────────────────────╯{RESET}");
    println!();
}

/// Scan the current directory for project markers and build a context summary.
fn scan_directory_context(cwd: &str) -> String {
    let p = Path::new(cwd);
    let mut markers = Vec::new();

    // Rust
    if p.join("Cargo.toml").exists() {
        markers.push("Rust (Cargo)");
    }
    // Node.js
    if p.join("package.json").exists() {
        markers.push("Node.js");
    }
    // Python
    if p.join("pyproject.toml").exists() || p.join("requirements.txt").exists() {
        markers.push("Python");
    }
    // Flutter/Dart
    if p.join("pubspec.yaml").exists() {
        markers.push("Flutter/Dart");
    }
    // Go
    if p.join("go.mod").exists() {
        markers.push("Go");
    }
    // Java/Kotlin
    if p.join("pom.xml").exists() || p.join("build.gradle").exists() {
        markers.push("Java/Kotlin");
    }
    // Docker
    if p.join("Dockerfile").exists() || p.join("docker-compose.yml").exists() {
        markers.push("Docker");
    }
    // Git
    if p.join(".git").exists() {
        markers.push("Git repo");
    }

    // List top-level files (up to 15) for context
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(p) {
        for entry in entries.flatten().take(30) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') {
                files.push(name);
            }
            if files.len() >= 15 {
                break;
            }
        }
    }

    if markers.is_empty() && files.is_empty() {
        return String::new();
    }

    let mut result = markers.join(", ");
    if !files.is_empty() {
        if !result.is_empty() {
            result.push_str(" | ");
        }
        result.push_str(&format!("Arquivos: {}", files.join(", ")));
    }
    result
}

/// Helper to resolve the API key checking env var, explicit config, and "main" config.
fn get_api_key(config: &AppConfig, provider_name: &str, env_var: &str) -> Option<String> {
    if !env_var.is_empty()
        && let Ok(key) = std::env::var(env_var)
        && !key.is_empty()
    {
        return Some(key);
    }
    if let Some(cfg) = config.llm.get(provider_name)
        && let Some(ref k) = cfg.api_key
        && !k.is_empty()
    {
        return Some(k.clone());
    }
    if let Some(cfg) = config.llm.get("main")
        && cfg.provider == provider_name
        && let Some(ref k) = cfg.api_key
        && !k.is_empty()
    {
        return Some(k.clone());
    }
    None
}

/// GAR-576 — Resolve the model name for a given provider kind.
///
/// Lookup order:
///   1. `model_override` (the CLI `--model` flag, absolute precedence).
///   2. `config.llm[provider_kind].model` (key-match).
///   3. The first `config.llm[*]` entry whose `provider` field equals
///      `provider_kind` and whose `model` is `Some(non-empty)`.
///
/// Returns `None` only when no source supplies a usable model name; the
/// caller is then responsible for picking a hardcoded fallback.
fn resolve_provider_model(
    config: &AppConfig,
    provider_kind: &str,
    model_override: Option<&str>,
) -> Option<String> {
    if let Some(m) = model_override
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    if let Some(cfg) = config.llm.get(provider_kind)
        && let Some(m) = cfg.model.as_deref()
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    for cfg in config.llm.values() {
        if cfg.provider == provider_kind
            && let Some(m) = cfg.model.as_deref()
            && !m.is_empty()
        {
            return Some(m.to_string());
        }
    }
    None
}

/// GAR-576 — Decision returned by [`decide_default_provider`].
///
/// `UseDefault` says "the operator configured `agent.default_provider`,
/// the matching `llm[<key>]` block is present, and a credential is
/// reachable — go build the provider". `FallThroughToChain` says
/// "either no default is configured, the lookup failed, or there is no
/// usable credential — fall back to the legacy autodetect heuristic".
#[derive(Debug, Clone, PartialEq, Eq)]
enum DefaultProviderDecision {
    UseDefault {
        config_key: String,
        provider_kind: String,
        model: String,
    },
    FallThroughToChain {
        reason: &'static str,
    },
}

/// GAR-576 — Decide whether to honor `config.agent.default_provider`
/// before the legacy autodetect chain.
///
/// Pure function: takes presence-bool flags for the relevant env vars
/// instead of reading `std::env` directly, so unit tests can assert
/// regression scenarios (e.g. `OPENAI_API_KEY` in `.env` no longer
/// hijacks the provider when `agent.default_provider = "openrouter"`)
/// without mutating process-global env state.
fn decide_default_provider(
    config: &AppConfig,
    env_has_openai_key: bool,
    env_has_openrouter_key: bool,
    env_has_anthropic_key: bool,
) -> DefaultProviderDecision {
    let Some(default_key) = config.agent.default_provider.as_deref() else {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "no agent.default_provider configured",
        };
    };
    let Some(cfg) = config.llm.get(default_key) else {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "agent.default_provider key not present in llm map",
        };
    };
    let provider_kind = cfg.provider.as_str();

    let cfg_has_key = cfg.api_key.as_deref().is_some_and(|k| !k.is_empty());
    let credential_ok = match provider_kind {
        // Local — health-checked by the caller.
        "ollama" => true,
        "anthropic" => env_has_anthropic_key || cfg_has_key,
        // OpenAI-compatible local backends (e.g. LM Studio) commonly omit
        // the api_key and rely on `base_url` reachability. Treat them as
        // credential-ok for the purposes of routing.
        "openai" => cfg.base_url.is_some() || env_has_openai_key || cfg_has_key,
        "openrouter" => env_has_openrouter_key || cfg_has_key,
        _ => {
            return DefaultProviderDecision::FallThroughToChain {
                reason: "unknown provider kind in agent.default_provider",
            };
        }
    };

    if !credential_ok {
        return DefaultProviderDecision::FallThroughToChain {
            reason: "no credential available for agent.default_provider",
        };
    }

    let model = resolve_provider_model(config, provider_kind, None)
        .unwrap_or_else(|| hardcoded_default_model(provider_kind));

    DefaultProviderDecision::UseDefault {
        config_key: default_key.to_string(),
        provider_kind: provider_kind.to_string(),
        model,
    }
}

/// GAR-576 — Last-resort fallback model name per provider kind, used
/// only when neither the CLI flag nor `config.llm` supplies one.
///
/// Single source of truth: `select_explicit_provider` and `detect_provider`
/// both route through here rather than repeating the literals inline, so the
/// two paths cannot disagree about what "the default" means.
pub(crate) fn hardcoded_default_model(provider_kind: &str) -> String {
    match provider_kind {
        // `qwen3.8:latest` == `qwen3.8:27b` (Q4_K_M, ~18 GB, 262 144-token
        // context, vision + tools). Kept byte-identical to
        // `garraia_agents::ollama::DEFAULT_MODEL`.
        "ollama" => "qwen3.8:latest",
        "anthropic" => "claude-sonnet-4-5-20250929",
        "openai" => "gpt-4o",
        "openrouter" => "openrouter/auto",
        "echo" => "echo-stub",
        _ => "auto",
    }
    .to_string()
}

/// GAR-576 — Construct an [`LlmProvider`] from a config-resolved default.
///
/// Returns `None` when construction is infeasible (e.g. Ollama daemon
/// unreachable, or required api_key absent at build time); the caller
/// then falls through to the legacy autodetect chain.
async fn try_build_default_provider(
    config: &AppConfig,
    provider_kind: &str,
    cfg: &garraia_config::LlmProviderConfig,
    model: &str,
) -> Option<Arc<dyn LlmProvider>> {
    // GAR-576: return ONLY the trait object — the display strings
    // (config_key, model) are formed at the call site from inputs that
    // never pass through this function. That keeps CodeQL's cleartext-
    // logging dataflow analysis from conservatively tainting the model
    // name through this scope, which also calls `get_api_key`.
    match provider_kind {
        "ollama" => {
            let ollama = OllamaProvider::new(Some(model.to_string()), cfg.base_url.clone());
            if !ollama.health_check().await.unwrap_or(false) {
                return None;
            }
            Some(Arc::new(ollama) as Arc<dyn LlmProvider>)
        }
        "anthropic" => {
            let key = get_api_key(config, "anthropic", "ANTHROPIC_API_KEY")?;
            let ap = AnthropicProvider::new(&key, Some(model.to_string()), None);
            Some(Arc::new(ap) as Arc<dyn LlmProvider>)
        }
        "openai" => {
            // OpenAI-compatible local backends (e.g. LM Studio) usually
            // omit the api_key; accept "not-needed" when `base_url` is set.
            let key = get_api_key(config, "openai", "OPENAI_API_KEY").or_else(|| {
                if cfg.base_url.is_some() {
                    Some("not-needed".to_string())
                } else {
                    None
                }
            })?;
            let op = OpenAiProvider::new(&key, Some(model.to_string()), cfg.base_url.clone());
            Some(Arc::new(op) as Arc<dyn LlmProvider>)
        }
        "openrouter" => {
            let key = get_api_key(config, "openrouter", "OPENROUTER_API_KEY")?;
            let base = cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
            // GAR-582: name the provider "openrouter" so AgentRuntime's
            // lookup-by-name resolves correctly. Without this, the runtime
            // emits `WARN Provider 'openrouter' not found, falling back to default`.
            let op = OpenAiProvider::new(&key, Some(model.to_string()), Some(base))
                .with_name("openrouter");
            Some(Arc::new(op) as Arc<dyn LlmProvider>)
        }
        _ => None,
    }
}

/// GAR-579 — Build a provider from an explicit `--provider <kind>` flag.
///
/// Returns the same `(display_name, model, Arc<dyn LlmProvider>)` triple
/// that `detect_provider` returns. Honors `model_override` first, then
/// `config.llm[*].model` via `resolve_provider_model`, then a hardcoded
/// per-kind fallback. Unknown `kind` is an error; missing api_key for a
/// cloud provider is an error.
///
/// Shared by `chat::run_chat` and `ask::run_ask` so the explicit-provider
/// path lives in exactly one place.
pub(crate) fn select_explicit_provider(
    config: &AppConfig,
    kind: &str,
    model_override: Option<&str>,
) -> Result<(String, String, Arc<dyn LlmProvider>)> {
    match kind {
        "ollama" => {
            let model = resolve_provider_model(config, "ollama", model_override)
                .unwrap_or_else(|| hardcoded_default_model("ollama"));
            let ollama = OllamaProvider::new(Some(model.clone()), None);
            Ok((
                "ollama".to_string(),
                model,
                Arc::new(ollama) as Arc<dyn LlmProvider>,
            ))
        }
        "anthropic" => {
            let key = get_api_key(config, "anthropic", "ANTHROPIC_API_KEY")
                .context("ANTHROPIC_API_KEY not set and not found in config")?;
            let model = resolve_provider_model(config, "anthropic", model_override)
                .unwrap_or_else(|| hardcoded_default_model("anthropic"));
            let ap = AnthropicProvider::new(&key, Some(model.clone()), None);
            Ok((
                "anthropic".to_string(),
                model,
                Arc::new(ap) as Arc<dyn LlmProvider>,
            ))
        }
        "openai" => {
            let key = get_api_key(config, "openai", "OPENAI_API_KEY")
                .context("OPENAI_API_KEY not set and not found in config")?;
            let model = resolve_provider_model(config, "openai", model_override)
                .unwrap_or_else(|| hardcoded_default_model("openai"));
            let op = OpenAiProvider::new(&key, Some(model.clone()), None);
            Ok((
                "openai".to_string(),
                model,
                Arc::new(op) as Arc<dyn LlmProvider>,
            ))
        }
        "openrouter" => {
            let key = get_api_key(config, "openrouter", "OPENROUTER_API_KEY")
                .context("OPENROUTER_API_KEY not set and not found in config")?;
            let model = resolve_provider_model(config, "openrouter", model_override)
                .unwrap_or_else(|| hardcoded_default_model("openrouter"));
            // GAR-582: name the provider "openrouter" so AgentRuntime's
            // lookup-by-name resolves correctly (avoids WARN at request time).
            let op = OpenAiProvider::new(
                &key,
                Some(model.clone()),
                Some("https://openrouter.ai/api/v1".to_string()),
            )
            .with_name("openrouter");
            Ok((
                "openrouter".to_string(),
                model,
                Arc::new(op) as Arc<dyn LlmProvider>,
            ))
        }
        // Dev/CI only: o EchoProvider keyless (feature `dev-echo-provider`)
        // fica acessível também por `ask`/`mcp-server`, não só pelo gateway —
        // é o que permite smoke-testar o pipeline `garra_ask` sem API key.
        #[cfg(feature = "dev-echo-provider")]
        "echo" => {
            let model = resolve_provider_model(config, "echo", model_override)
                .unwrap_or_else(|| hardcoded_default_model("echo"));
            let echo = garraia_agents::EchoProvider::new(Some(model.clone()));
            Ok((
                "echo".to_string(),
                model,
                Arc::new(echo) as Arc<dyn LlmProvider>,
            ))
        }
        other => anyhow::bail!(
            "Provider desconhecido: {other}. Use: ollama, anthropic, openai, openrouter"
        ),
    }
}

/// Base URL of the local Ollama daemon. Extracted so the autodetect chain
/// and [`try_local_ollama_model`] cannot drift apart.
fn ollama_base_url() -> String {
    std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string())
}

/// How long the "is this tag installed?" probe may take. Local HTTP against
/// `/api/tags`; 2s is generous and keeps `garra --model …` snappy when the
/// daemon is down.
const OLLAMA_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// True when `model` is the model of a configured *non-Ollama* provider.
///
/// Without this, `--model gpt-4o` on a box that also runs Ollama would be
/// probed against `/api/tags`, come back missing, and offer to *pull gpt-4o
/// from Ollama* — a download that cannot succeed and a question the user
/// should never be asked. When config already says `gpt-4o` belongs to
/// `openai`, the Ollama path is skipped entirely and the regular chain
/// routes it.
fn model_belongs_to_configured_cloud_provider(config: &AppConfig, model: &str) -> bool {
    config
        .llm
        .values()
        .any(|cfg| cfg.provider != "ollama" && cfg.model.as_deref() == Some(model))
}

/// Outcome of probing the local Ollama daemon for an explicit `--model` tag.
enum LocalOllamaProbe {
    /// The tag is installed; the provider is ready to use.
    Installed(Box<(String, String, Arc<dyn LlmProvider>)>),
    /// The daemon answered but does not have this tag. Carries the
    /// normalized tag so the caller can offer to pull it.
    Missing { tag: String },
    /// Not an Ollama-shaped reference, or the daemon is unreachable.
    NotApplicable,
}

/// Probe the local Ollama daemon for an explicit `--model <tag>`.
///
/// This is what makes `garraia --model qwen3.8` open the local model
/// directly. `qwen3.8` is normalized to `qwen3.8:latest` first, because
/// `/api/tags` only ever reports explicit tags.
///
/// Deliberately conservative: it can only ever win on an exact hit in
/// `/api/tags`, so `--model gpt-4o` is never hijacked to a local provider.
async fn try_local_ollama_model(config: &AppConfig, model_override: &str) -> LocalOllamaProbe {
    // `openrouter/auto` and friends belong to another provider — bail before
    // spending a round-trip.
    let Some(tag) = normalize_ollama_tag(model_override) else {
        return LocalOllamaProbe::NotApplicable;
    };
    // Config already claims this name for a cloud provider — not a tag to pull.
    if model_belongs_to_configured_cloud_provider(config, model_override) {
        return LocalOllamaProbe::NotApplicable;
    }

    let base = ollama_base_url();
    let Ok(probe_client) = reqwest::Client::builder()
        .timeout(OLLAMA_PROBE_TIMEOUT)
        .build()
    else {
        return LocalOllamaProbe::NotApplicable;
    };
    let probe = OllamaProvider::new(None, Some(base.clone())).with_client(probe_client);

    match probe.resolve_installed_model(&tag).await {
        Ok(Some(found)) => {
            // Rebuild with the default (untimed) client — the 2s probe budget
            // must not cap inference.
            let provider = OllamaProvider::new(Some(found.clone()), Some(base));
            LocalOllamaProbe::Installed(Box::new((
                "ollama".to_string(),
                found,
                Arc::new(provider) as Arc<dyn LlmProvider>,
            )))
        }
        Ok(None) => LocalOllamaProbe::Missing { tag },
        // Daemon down: stay quiet and let the regular chain report whatever
        // it finds. `--model` is still honored by every branch below.
        Err(_) => LocalOllamaProbe::NotApplicable,
    }
}

/// Offer to pull a missing Ollama tag, then return the ready provider.
///
/// * `assume_yes` (the `-y` flag) pulls without asking.
/// * An interactive terminal gets a confirmation prompt.
/// * Anything else — a pipe, CI, `ask --json` — never prompts and returns
///   `None` so the caller can fall through with a visible hint.
///
/// Progress goes to stderr so `ask --json` keeps a single clean JSON line on
/// stdout.
async fn offer_pull_ollama_model(
    tag: &str,
    assume_yes: bool,
) -> Option<(String, String, Arc<dyn LlmProvider>)> {
    use std::io::IsTerminal as _;

    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    if !assume_yes {
        if !interactive {
            eprintln!(
                "{YELLOW}Modelo '{tag}' nao esta baixado no Ollama.{RESET} \
                 Rode `ollama pull {tag}` (ou use -y para baixar agora)."
            );
            return None;
        }
        eprint!("{YELLOW}Modelo '{tag}' nao esta baixado. Baixar agora? [S/n] {RESET}");
        let _ = io::stderr().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() {
            return None;
        }
        let answer = answer.trim().to_lowercase();
        if !(answer.is_empty()
            || answer == "s"
            || answer == "sim"
            || answer == "y"
            || answer == "yes")
        {
            eprintln!("{DIM}  Download cancelado. Rode `ollama pull {tag}` quando quiser.{RESET}");
            return None;
        }
    }

    let base = ollama_base_url();
    // Default client: no timeout — a cold pull can run for many minutes.
    let puller = OllamaProvider::new(Some(tag.to_string()), Some(base.clone()));

    eprintln!("{DIM}Baixando {tag}…{RESET}");
    let mut last_status = String::new();
    let result = puller
        .pull_model(tag, |progress| {
            match progress.percent() {
                Some(pct) => eprint!("\r{DIM}  {} {pct:.0}%          {RESET}", progress.status),
                None if progress.status != last_status => {
                    eprint!("\r{DIM}  {}          {RESET}", progress.status);
                }
                None => return,
            }
            last_status = progress.status.clone();
            let _ = io::stderr().flush();
        })
        .await;
    eprintln!();

    match result {
        Ok(()) => {
            eprintln!("{GREEN}  {tag} pronto.{RESET}");
            let provider = OllamaProvider::new(Some(tag.to_string()), Some(base));
            Some((
                "ollama".to_string(),
                tag.to_string(),
                Arc::new(provider) as Arc<dyn LlmProvider>,
            ))
        }
        Err(e) => {
            eprintln!("{YELLOW}  Falha ao baixar {tag}: {e}{RESET}");
            None
        }
    }
}

/// Detect which provider to use based on config and availability.
///
/// `model_override` is the CLI `--model` flag. It has absolute precedence
/// over every configured model, and — when it names a tag the local Ollama
/// daemon has installed — it also selects the provider (see
/// [`try_local_ollama_model`]).
pub async fn detect_provider(
    config: &AppConfig,
    url_override: Option<&str>,
    model_override: Option<&str>,
    assume_yes: bool,
) -> (String, String, Arc<dyn LlmProvider>) {
    // 0. If a custom URL is provided, use OpenAI-compatible provider (LM Studio, vLLM, etc.)
    if let Some(url) = url_override {
        let base = url.trim_end_matches('/').to_string();
        // Try multiple env vars for the API key (LM Studio may require auth)
        let key = std::env::var("LLM_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .or_else(|_| std::env::var("GARRAIA_EMBEDDING_API_KEY"))
            .unwrap_or_else(|_| "not-needed".to_string());
        let provider = OpenAiProvider::new(
            &key,
            None, // model will be set from --model flag or default
            Some(base.clone()),
        )
        .with_name("lmstudio");

        // An explicit --model wins over whatever the endpoint advertises.
        let model = match model_override {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => match provider.available_models().await {
                Ok(models) if !models.is_empty() => models[0].clone(),
                _ => "default".to_string(),
            },
        };
        return (
            format!("lmstudio ({})", base),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 0.5 — an explicitly named, locally-installed Ollama tag beats
    // `agent.default_provider`: `garraia --model qwen3.8` must open qwen3.8
    // even on a box configured for OpenRouter. Placed after `--url`, which is
    // a more explicit routing instruction than a bare model name.
    if let Some(m) = model_override.filter(|m| !m.is_empty()) {
        match try_local_ollama_model(config, m).await {
            LocalOllamaProbe::Installed(hit) => return *hit,
            LocalOllamaProbe::Missing { tag } => {
                if let Some(hit) = offer_pull_ollama_model(&tag, assume_yes).await {
                    return hit;
                }
                // Declined / failed / non-interactive: fall through. Every
                // branch below still honors `--model`.
            }
            LocalOllamaProbe::NotApplicable => {}
        }
    }

    // GAR-576 — honor `config.agent.default_provider` BEFORE the env-based
    // autodetect chain below. This prevents a stale `OPENAI_API_KEY` loaded
    // from cwd `.env` (via `dotenvy::dotenv()` in main.rs) from hijacking the
    // provider when the operator explicitly configured a different default.
    let env_has = |name: &str| std::env::var(name).map(|v| !v.is_empty()).unwrap_or(false);
    let decision = decide_default_provider(
        config,
        env_has("OPENAI_API_KEY"),
        env_has("OPENROUTER_API_KEY"),
        env_has("ANTHROPIC_API_KEY"),
    );
    if let DefaultProviderDecision::UseDefault {
        config_key,
        provider_kind,
        model,
    } = decision
        // `--model` outranks the configured default. With `None` this
        // reproduces `decide_default_provider`'s own lookup exactly.
        && let model = resolve_provider_model(config, &provider_kind, model_override)
            .unwrap_or(model)
        && let Some(cfg) = config.llm.get(&config_key)
        && let Some(provider) =
            try_build_default_provider(config, &provider_kind, cfg, &model).await
    {
        // GAR-576: form the display tuple here from the (untainted)
        // strings returned by `decide_default_provider` — they never
        // pass through the function that calls `get_api_key`.
        return (config_key, model, provider);
        // If construction fails (e.g. Ollama health-check fails) the
        // outer `if-let` chain shorts out and we fall through to the
        // legacy autodetect chain below.
    }

    // Every branch below resolves its model through `resolve_provider_model`,
    // so `--model` is honored whichever provider wins — and the returned
    // provider object always carries the model it will actually be asked for.
    let ollama_url = ollama_base_url();

    // 1. Try Ollama first (local, offline)
    let model = resolve_provider_model(config, "ollama", model_override)
        .unwrap_or_else(|| hardcoded_default_model("ollama"));
    let ollama = OllamaProvider::new(Some(model.clone()), Some(ollama_url.clone()));
    if ollama.health_check().await.unwrap_or(false) {
        return (
            "ollama".to_string(),
            model,
            Arc::new(ollama) as Arc<dyn LlmProvider>,
        );
    }

    // 2. Try Anthropic (cloud)
    if let Some(key) = get_api_key(config, "anthropic", "ANTHROPIC_API_KEY") {
        let model = resolve_provider_model(config, "anthropic", model_override)
            .unwrap_or_else(|| hardcoded_default_model("anthropic"));
        let provider = AnthropicProvider::new(&key, Some(model.clone()), None);
        return (
            "anthropic".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 3. Try OpenAI (cloud)
    if let Some(key) = get_api_key(config, "openai", "OPENAI_API_KEY") {
        let model = resolve_provider_model(config, "openai", model_override)
            .unwrap_or_else(|| hardcoded_default_model("openai"));
        let provider = OpenAiProvider::new(&key, Some(model.clone()), None);
        return (
            "openai".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 4. Try OpenRouter (cloud fallback)
    if let Some(key) = get_api_key(config, "openrouter", "OPENROUTER_API_KEY") {
        let model = resolve_provider_model(config, "openrouter", model_override)
            .unwrap_or_else(|| hardcoded_default_model("openrouter"));
        // GAR-582: name the provider "openrouter" so AgentRuntime's
        // lookup-by-name resolves correctly (avoids WARN at request time).
        let provider = OpenAiProvider::new(
            &key,
            Some(model.clone()),
            Some("https://openrouter.ai/api/v1".to_string()),
        )
        .with_name("openrouter");
        return (
            "openrouter".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 5. Fallback: Ollama with no health check (user will see error on first message)
    let ollama = OllamaProvider::new(Some(model.clone()), Some(ollama_url));
    (
        "ollama (offline)".to_string(),
        model,
        Arc::new(ollama) as Arc<dyn LlmProvider>,
    )
}

/// Resultado de um turno, do ponto de vista do REPL.
///
/// `Cancelled` existe porque o `Ctrl+C` passou a abortar apenas o turno em
/// andamento em vez de matar o processo — ver `stream_turn`.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum TurnOutcome<T, E> {
    Done(std::result::Result<T, E>),
    TimedOut,
    Cancelled,
}

/// Await the streaming LLM call while concurrently draining `rx`, writing
/// each delta to `out` as it arrives. The runtime pushes deltas through a
/// bounded channel with `send().await` (runtime.rs), so the receiver MUST be
/// polled during the call — draining only after completion deadlocks the
/// producer once the buffer fills (the original `garra chat` hang).
///
/// Returns `TurnOutcome::TimedOut` on timeout. In every path the call future
/// is dropped before the final drain, which closes the sender side so the
/// drain terminates once buffered deltas are consumed.
///
/// # Indicador de atividade
///
/// `spinner` anima a janela entre o envio e o primeiro token. Ele é um
/// **braço a mais do mesmo `select!`** — nunca uma task separada nem um sleep
/// bloqueante — justamente para não quebrar a drenagem concorrente descrita
/// acima: `rx` continua sendo consumido enquanto a garra gira. `None`
/// desativa a animação por completo (stdout redirecionado, `NO_COLOR`, etc.)
/// e nesse caso nem um byte de spinner chega ao `out`.
///
/// `prefix` (o rótulo `garra >`) é escrito exatamente uma vez, imediatamente
/// antes do primeiro delta — ou na saída, se nenhum delta chegar. Ele não pode
/// ser impresso antes da chamada como era feito: o spinner ocupa a mesma linha
/// e o `\r\x1b[2K` da limpeza apagaria o rótulo junto.
///
/// `cancel` é acordado pelo vigia de SIGINT de `run_chat` quando o usuário
/// aperta Ctrl+C durante o turno.
async fn stream_turn<F, T, E>(
    call: F,
    mut rx: mpsc::Receiver<String>,
    timeout: std::time::Duration,
    out: &mut (impl io::Write + ?Sized),
    mut spinner: Option<crate::spinner::Spinner>,
    prefix: &str,
    cancel: &tokio::sync::Notify,
) -> TurnOutcome<T, E>
where
    F: Future<Output = std::result::Result<T, E>>,
{
    let mut call = Box::pin(tokio::time::timeout(timeout, call));
    let mut rx_open = true;
    let mut prefix_written = false;

    // O primeiro tick de `interval` dispara imediatamente, então a garra
    // aparece assim que o turno começa, sem esperar um período.
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(
        crate::spinner::FRAME_INTERVAL_MS,
    ));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // `write_delta` centraliza a ordem obrigatória: apagar o spinner, escrever
    // o prefixo uma única vez, só então o texto do modelo. Sem isso o rótulo
    // sairia no meio da resposta ou colidiria com um quadro da animação.
    macro_rules! write_delta {
        ($delta:expr) => {{
            if let Some(s) = spinner.as_mut() {
                s.clear(out);
            }
            if !prefix_written {
                let _ = write!(out, "{prefix}");
                prefix_written = true;
            }
            let _ = write!(out, "{}", $delta);
            let _ = out.flush();
        }};
    }

    let result = loop {
        tokio::select! {
            r = &mut call => break match r {
                Ok(inner) => TurnOutcome::Done(inner),
                Err(_elapsed) => TurnOutcome::TimedOut,
            },
            // Cancelamento do turno, sinalizado pelo vigia de SIGINT criado em
            // `run_chat`. Sem este braço o Ctrl+C matava o processo inteiro no
            // meio do stream, levando junto o histórico da sessão.
            //
            // O sinal NÃO é registrado aqui de propósito: `tokio::signal::ctrl_c`
            // instala um handler para o resto da vida do processo, e um handler
            // sem ninguém escutando engoliria o Ctrl+C no prompt `voce >` — o
            // usuário ficaria sem como sair. Um dono único resolve os dois casos.
            _ = cancel.notified() => break TurnOutcome::Cancelled,
            _ = ticker.tick(), if spinner.is_some() => {
                // Só anima antes do primeiro token: depois disso a linha
                // pertence à resposta e um quadro colidiria com ela.
                if !prefix_written
                    && let Some(s) = spinner.as_mut() {
                        s.render_frame(out);
                    }
            }
            maybe = rx.recv(), if rx_open => match maybe {
                Some(delta) => write_delta!(delta),
                None => rx_open = false,
            },
        }
    };

    // Box::pin so the future (and its sender) can be dropped here even after
    // a timeout — tokio::pin! would keep it alive and hang the drain below.
    drop(call);
    if rx_open {
        while let Some(delta) = rx.recv().await {
            write_delta!(delta);
        }
    }

    // Limpeza incondicional: vale para sucesso, erro do provedor, timeout,
    // Ctrl+C e cancelamento. `clear` é idempotente, então repetir é barato.
    if let Some(s) = spinner.as_mut() {
        s.clear(out);
    }
    if !prefix_written {
        let _ = write!(out, "{prefix}");
    }
    let _ = out.flush();

    result
}

/// Run the interactive chat REPL.
pub async fn run_chat(
    config: AppConfig,
    provider_override: Option<String>,
    model_override: Option<String>,
    url_override: Option<String>,
    timeout_secs: u64,
    assume_yes: bool,
) -> Result<()> {
    // An explicit `--provider` short-circuits detection entirely; otherwise
    // `detect_provider` owns both the provider *and* the model, so the two can
    // no longer disagree (previously `--model` without `--provider` swapped
    // only the displayed string, leaving the built provider stale).
    let (provider_name, mut model_name, provider) = if let Some(ref p) = provider_override {
        // GAR-579: shared with `garra ask` — the explicit-provider path
        // lives in `select_explicit_provider` so chat and ask agree
        // byte-for-byte on construction + model resolution + error msgs.
        select_explicit_provider(&config, p.as_str(), model_override.as_deref())?
    } else {
        detect_provider(
            &config,
            url_override.as_deref(),
            model_override.as_deref(),
            assume_yes,
        )
        .await
    };

    let mode = if provider_name.contains("ollama") {
        "local"
    } else {
        "cloud"
    };

    // Gather current directory context
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "(desconhecido)".to_string());

    // Scan directory for context
    let dir_context = scan_directory_context(&cwd);

    print_chat_banner(&provider_name, &model_name, mode);
    println!("{DIM}  Diretorio: {cwd}{RESET}");
    if !dir_context.is_empty() {
        println!("{DIM}  Projeto:   {dir_context}{RESET}");
    }
    println!();

    // Build runtime with filesystem tools
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);
    runtime.register_tool(Box::new(FileReadTool::new(None)));
    runtime.register_tool(Box::new(FileWriteTool::new(None)));
    runtime.register_tool(Box::new(BashTool::new_with_confirmation(Some(30))));
    runtime.register_tool(Box::new(GitDiffTool::new(None, None)));

    let system_prompt = format!(
        "Voce e o GarraIA, um assistente pessoal de IA criado em Rust. \
         Seja prestativo, conciso e amigavel. Responda no idioma do usuario.\n\n\
         ## Ferramentas disponiveis\n\
         Voce tem acesso a estas ferramentas que pode usar quando necessario:\n\
         - **file_read**: Le o conteudo de um arquivo. Use para ver codigo, configs, READMEs.\n\
         - **file_write**: Escreve/cria arquivos. Use para editar codigo ou criar novos arquivos.\n\
         - **bash**: Executa comandos no terminal (ls, dir, cargo, git, etc.).\n\
         - **git_diff**: Executa comandos git seguros (diff, status, log, branch).\n\n\
         IMPORTANTE: Quando o usuario perguntar sobre arquivos, SEMPRE use as ferramentas \
         para ler/listar em vez de apenas descrever. Use 'bash' com 'ls' ou 'dir' para \
         listar arquivos. Use 'file_read' para ler conteudo de arquivos.\n\n\
         ## Contexto do diretorio atual\n\
         O usuario esta trabalhando em: {cwd}\n\
         {}\
         \n\
         Quando o usuario perguntar sobre arquivos, codigo ou o projeto, \
         USE as ferramentas para investigar. Nao invente — leia os arquivos reais.",
        if dir_context.is_empty() {
            String::new()
        } else {
            format!("Tipo de projeto detectado: {dir_context}\n")
        }
    );
    runtime.set_system_prompt(system_prompt);
    runtime.set_max_tokens(4096);

    let session_id = format!("cli-{}", uuid::Uuid::new_v4());
    let mut history: Vec<ChatMessage> = Vec::new();
    // Semente do indicador de atividade: roda a mensagem de abertura a
    // cada turno, para dois envios seguidos não começarem com a mesma frase.
    let mut turn_index: usize = 0;

    // Dono único do SIGINT.
    //
    // `tokio::signal::ctrl_c()` instala um handler que substitui o
    // comportamento padrão do processo e permanece instalado até o fim da
    // execução. Registrá-lo dentro do turno (o lugar óbvio) teria um efeito
    // colateral silencioso e ruim: a partir do primeiro turno, o Ctrl+C no
    // prompt `voce >` deixaria de encerrar o `garra` e simplesmente sumiria,
    // porque o handler continua instalado sem ninguém escutando e o
    // `read_line` apenas reinicia no EINTR. O usuário ficaria preso.
    //
    // Com um dono único os dois casos ficam corretos:
    //   - durante o turno  -> cancela o turno e devolve o prompt;
    //   - ocioso no prompt -> encerra a sessão, como sempre encerrou.
    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
    let turn_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let cancel = std::sync::Arc::clone(&cancel);
        let turn_active = std::sync::Arc::clone(&turn_active);
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    // Sem handler de sinal disponível: mantém o padrão do SO.
                    return;
                }
                if turn_active.load(std::sync::atomic::Ordering::SeqCst) {
                    cancel.notify_waiters();
                } else {
                    // 130 = terminado por SIGINT, a convenção do shell.
                    println!("\n{DIM}Ate mais! 🦀{RESET}");
                    let _ = io::stdout().flush();
                    std::process::exit(130);
                }
            }
        });
    }
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        // Prompt
        print!("{GREEN}{BOLD}voce >{RESET} ");
        io::stdout().flush()?;

        let mut input = String::new();
        if reader.read_line(&mut input)? == 0 {
            // EOF (Ctrl+D)
            println!("\n{DIM}Ate mais! 🦀{RESET}");
            break;
        }

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Handle slash commands
        match input.as_str() {
            "/exit" | "/quit" | "/sair" => {
                println!("{DIM}Ate mais! 🦀{RESET}");
                break;
            }
            "/clear" | "/limpar" => {
                history.clear();
                println!("{DIM}Historico limpo.{RESET}");
                continue;
            }
            "/help" | "/ajuda" => {
                println!("{DIM}Comandos disponiveis:{RESET}");
                println!("  /model <nome>      Trocar modelo");
                println!("  /provider <nome>   Trocar provider (ollama, anthropic, openai)");
                println!("  /models            Listar modelos disponiveis");
                println!("  /clear             Limpar historico");
                println!("  /history           Mostrar historico");
                println!("  /exit              Sair");
                continue;
            }
            "/history" | "/historico" => {
                if history.is_empty() {
                    println!("{DIM}Historico vazio.{RESET}");
                } else {
                    for msg in &history {
                        let role = match msg.role {
                            ChatRole::User => format!("{GREEN}voce{RESET}"),
                            ChatRole::Assistant => format!("{CYAN}garra{RESET}"),
                            _ => "system".to_string(),
                        };
                        let text = match &msg.content {
                            MessagePart::Text(t) => t.as_str(),
                            MessagePart::Parts(_) => "(multi-part)",
                        };
                        let preview: String = text.chars().take(80).collect();
                        println!("  {role}: {preview}");
                    }
                }
                continue;
            }
            _ if input.starts_with("/model ") => {
                let new_model = input[7..].trim();
                if new_model.is_empty() {
                    println!("{DIM}Uso: /model <nome>{RESET}");
                    continue;
                }
                // On Ollama, `/model qwen3.8` means `qwen3.8:latest` — spell
                // it out so the banner and `/models` marker line up.
                let resolved = if provider_name.contains("ollama") {
                    normalize_ollama_tag(new_model).unwrap_or_else(|| new_model.to_string())
                } else {
                    new_model.to_string()
                };
                // Advisory only: an unknown name is not fatal (the provider
                // may serve models it does not list), but silently talking to
                // a nonexistent model is a bad surprise.
                if let Some(p) = runtime.default_provider()
                    && let Ok(models) = p.available_models().await
                    && !models.is_empty()
                    && !models.contains(&resolved)
                {
                    println!(
                        "{YELLOW}Aviso: '{resolved}' nao aparece em /models deste provider.{RESET}"
                    );
                }
                model_name = resolved;
                println!("{DIM}Modelo alterado para: {model_name}{RESET}");
                println!(
                    "{DIM}  (o provider continua {provider_name} — para trocar, reinicie com --provider ou --model){RESET}"
                );
                continue;
            }
            "/models" => {
                let provider_ref = runtime.default_provider();
                if let Some(p) = provider_ref {
                    match p.available_models().await {
                        Ok(models) => {
                            println!("{DIM}Modelos disponiveis ({provider_name}):{RESET}");
                            for m in models.iter().take(20) {
                                let marker = if m == &model_name { " *" } else { "" };
                                println!("  {m}{marker}");
                            }
                            if models.len() > 20 {
                                println!("  ... e mais {} modelos", models.len() - 20);
                            }
                        }
                        Err(e) => println!("{DIM}Erro listando modelos: {e}{RESET}"),
                    }
                }
                continue;
            }
            _ if input.starts_with("/provider ") => {
                let new_provider = input[10..].trim();
                println!(
                    "{DIM}Para trocar provider, reinicie com: garraia chat --provider {new_provider}{RESET}"
                );
                println!(
                    "{DIM}  Para um modelo local do Ollama basta: garraia --model <tag>{RESET}"
                );
                continue;
            }
            _ => {}
        }

        // Stream response.
        //
        // O rótulo NÃO é impresso aqui: ele vai como `prefix` para o
        // `stream_turn`, que o escreve junto do primeiro token. O indicador de
        // atividade ocupa esta linha enquanto o modelo pensa, e limpá-la
        // apagaria o rótulo se ele já estivesse na tela.
        let spinner = crate::spinner::detect(turn_index);
        turn_index = turn_index.wrapping_add(1);

        let (tx, rx) = mpsc::channel::<String>(100);

        // The runtime appends the user message on top of the given history
        // (runtime.rs), so `history` must NOT contain it yet — it is pushed
        // below only after a successful turn.
        let history_clone = history.clone();
        let session_clone = session_id.clone();
        let model_clone = model_name.clone();

        let call = runtime.process_message_streaming(
            &session_clone,
            &input,
            &history_clone,
            tx,
            Some(&model_clone),
        );
        let mut stdout = io::stdout();
        turn_active.store(true, std::sync::atomic::Ordering::SeqCst);
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(timeout_secs),
            &mut stdout,
            spinner,
            &format!("{CYAN}{BOLD}garra >{RESET} "),
            &cancel,
        )
        .await;
        turn_active.store(false, std::sync::atomic::Ordering::SeqCst);

        match outcome {
            TurnOutcome::TimedOut => {
                println!(
                    "\n{YELLOW}Tempo esgotado apos {timeout_secs}s. A resposta foi descartada; \
                     tente de novo ou aumente com --timeout-secs.{RESET}"
                );
            }
            TurnOutcome::Cancelled => {
                // Ctrl+C aborta o turno, não a sessão: o histórico segue vivo.
                println!("\n{DIM}Cancelado. Manda outra ou /exit para sair.{RESET}");
            }
            TurnOutcome::Done(Ok(full_response)) => {
                // Deltas were already printed live during streaming
                println!();

                history.push(ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text(input.clone()),
                });
                history.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Text(full_response),
                });
            }
            TurnOutcome::Done(Err(e)) => {
                println!("\n{YELLOW}Erro: {e}{RESET}");

                // Hint for common errors
                let err_str = format!("{e}");
                if err_str.contains("Connection refused") || err_str.contains("connect") {
                    println!("{DIM}Dica: Ollama nao esta rodando. Inicie com: ollama serve{RESET}");
                } else if err_str.contains("401") || err_str.contains("Unauthorized") {
                    println!(
                        "{DIM}Dica: API key invalida. Verifique ANTHROPIC_API_KEY ou OPENAI_API_KEY{RESET}"
                    );
                }
            }
        }

        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    //! GAR-576 — Pure tests for provider/model resolution. None of these
    //! touch `std::env` or the filesystem; env presence is passed in as
    //! bool flags so the `OPENAI_API_KEY` hijack regression can be
    //! asserted without mutating process-global state.

    use super::*;
    use garraia_config::{AgentConfig, AppConfig, LlmProviderConfig};
    use std::collections::HashMap;

    fn make_llm_cfg(
        provider: &str,
        model: Option<&str>,
        api_key: Option<&str>,
        base_url: Option<&str>,
    ) -> LlmProviderConfig {
        LlmProviderConfig {
            provider: provider.to_string(),
            model: model.map(String::from),
            api_key: api_key.map(String::from),
            base_url: base_url.map(String::from),
            extra: HashMap::new(),
        }
    }

    fn config_with(entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = AppConfig::default();
        for (k, v) in entries {
            cfg.llm.insert((*k).to_string(), v.clone());
        }
        cfg
    }

    fn config_with_default(default_key: &str, entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = config_with(entries);
        cfg.agent = AgentConfig {
            default_provider: Some(default_key.to_string()),
            ..AgentConfig::default()
        };
        cfg
    }

    // ─── select_explicit_provider: echo (dev-echo-provider) ────────────

    /// Com a feature, `--provider echo` seleciona o EchoProvider keyless.
    #[cfg(feature = "dev-echo-provider")]
    #[test]
    fn select_explicit_provider_echo_needs_no_key() {
        let cfg = AppConfig::default();
        let (name, model, _provider) = select_explicit_provider(&cfg, "echo", None)
            .expect("echo deve ser selecionável com a feature dev-echo-provider");
        assert_eq!(name, "echo");
        assert_eq!(model, "echo-stub");
    }

    /// Sem a feature, `echo` continua sendo provider desconhecido —
    /// builds de produção não ganham o caminho keyless.
    #[cfg(not(feature = "dev-echo-provider"))]
    #[test]
    fn select_explicit_provider_echo_rejected_without_feature() {
        let cfg = AppConfig::default();
        assert!(select_explicit_provider(&cfg, "echo", None).is_err());
    }

    // ─── resolve_provider_model ────────────────────────────────────────

    #[test]
    fn resolve_provider_model_override_wins() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", Some("openrouter/auto"));
        assert_eq!(got.as_deref(), Some("openrouter/auto"));
    }

    #[test]
    fn resolve_provider_model_key_match() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", None);
        assert_eq!(got.as_deref(), Some("openrouter/free"));
    }

    #[test]
    fn resolve_provider_model_provider_field_match() {
        // Key name is arbitrary (`my-router`), but the `provider` field
        // matches the requested kind — the helper must still find the model.
        let cfg = config_with(&[(
            "my-router",
            make_llm_cfg("openrouter", Some("openrouter/free"), Some("k"), None),
        )]);
        let got = resolve_provider_model(&cfg, "openrouter", None);
        assert_eq!(got.as_deref(), Some("openrouter/free"));
    }

    #[test]
    fn resolve_provider_model_no_match() {
        let cfg = AppConfig::default();
        assert!(resolve_provider_model(&cfg, "openrouter", None).is_none());
    }

    #[test]
    fn resolve_provider_model_empty_string_skipped() {
        let cfg = config_with(&[(
            "openrouter",
            make_llm_cfg("openrouter", Some(""), Some("k"), None),
        )]);
        // Empty string in config must not be returned as a valid model.
        assert!(resolve_provider_model(&cfg, "openrouter", None).is_none());
    }

    // ─── decide_default_provider ───────────────────────────────────────

    #[test]
    fn decide_default_provider_no_default_falls_through() {
        let cfg = AppConfig::default();
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_missing_llm_key_falls_through() {
        let cfg = config_with_default("missing", &[]);
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_openrouter_wins_over_openai_env() {
        // GAR-576 regression: this is the exact scenario from the bug
        // report — operator configured OpenRouter as the default, but
        // OPENAI_API_KEY is loaded from cwd `.env` and was hijacking
        // the autodetect chain. The new branch must pick OpenRouter.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg(
                    "openrouter",
                    Some("openrouter/free"),
                    Some("test-key"),
                    None,
                ),
            )],
        );
        let decision = decide_default_provider(
            &cfg, /* env_has_openai */ true, /* env_has_openrouter */ true,
            /* env_has_anthropic */ false,
        );
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "openrouter");
                assert_eq!(provider_kind, "openrouter");
                assert_eq!(model, "openrouter/free");
            }
            other => panic!("expected UseDefault(openrouter), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_falls_through_when_no_credential() {
        // default_provider points to a kind that needs a key, but neither
        // the env nor the config supplies one — fall through to the
        // legacy chain rather than building a doomed provider.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg("openrouter", Some("openrouter/free"), None, None),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        assert!(matches!(
            decision,
            DefaultProviderDecision::FallThroughToChain { .. }
        ));
    }

    #[test]
    fn decide_default_provider_ollama_no_credential_needed() {
        // Ollama has no api_key concept — the credential gate is the
        // async health-check inside try_build_default_provider, not
        // the decision function.
        let cfg = config_with_default(
            "ollama-local",
            &[(
                "ollama-local",
                make_llm_cfg(
                    "ollama",
                    Some("llama3.2"),
                    None,
                    Some("http://localhost:11434"),
                ),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "ollama-local");
                assert_eq!(provider_kind, "ollama");
                assert_eq!(model, "llama3.2");
            }
            other => panic!("expected UseDefault(ollama), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_openai_compat_with_base_url_accepts_no_key() {
        // LM Studio scenario: provider kind is `openai` but the
        // base_url points at a local server that does not enforce an
        // api_key. The helper must accept the config and route there.
        let cfg = config_with_default(
            "lm-studio",
            &[(
                "lm-studio",
                make_llm_cfg(
                    "openai",
                    Some("local-model"),
                    None,
                    Some("http://localhost:1234/v1"),
                ),
            )],
        );
        let decision = decide_default_provider(&cfg, false, false, false);
        match decision {
            DefaultProviderDecision::UseDefault {
                config_key,
                provider_kind,
                model,
            } => {
                assert_eq!(config_key, "lm-studio");
                assert_eq!(provider_kind, "openai");
                assert_eq!(model, "local-model");
            }
            other => panic!("expected UseDefault(openai-compat), got {other:?}"),
        }
    }

    #[test]
    fn decide_default_provider_uses_hardcoded_fallback_when_model_missing() {
        // Config declares the provider but no model. The helper must
        // fall back to hardcoded_default_model rather than refusing.
        let cfg = config_with_default(
            "openrouter",
            &[(
                "openrouter",
                make_llm_cfg("openrouter", None, Some("test-key"), None),
            )],
        );
        let decision = decide_default_provider(&cfg, false, true, false);
        match decision {
            DefaultProviderDecision::UseDefault { model, .. } => {
                assert_eq!(model, "openrouter/auto");
            }
            other => panic!("expected UseDefault with hardcoded model, got {other:?}"),
        }
    }

    /// Spec-lock for the one default-model table. Every fallback in this file
    /// routes through `hardcoded_default_model`, so this is the only place the
    /// defaults are written down — changing a row here means changing the docs
    /// and example configs in lockstep.
    #[test]
    fn hardcoded_default_model_table_is_locked() {
        assert_eq!(hardcoded_default_model("ollama"), "qwen3.8:latest");
        assert_eq!(
            hardcoded_default_model("anthropic"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(hardcoded_default_model("openai"), "gpt-4o");
        assert_eq!(hardcoded_default_model("openrouter"), "openrouter/auto");
        assert_eq!(hardcoded_default_model("echo"), "echo-stub");
        assert_eq!(hardcoded_default_model("something-else"), "auto");
    }

    /// The Ollama default must be byte-identical to the provider crate's own
    /// `DEFAULT_MODEL`, or `OllamaProvider::new(None, _)` and the CLI would
    /// disagree about which model "no model specified" means.
    #[test]
    fn ollama_default_matches_the_provider_crate() {
        let provider = garraia_agents::OllamaProvider::new(None, None);
        assert_eq!(
            provider.configured_model(),
            Some(hardcoded_default_model("ollama").as_str())
        );
    }

    /// Guard for the "offer to pull gpt-4o from Ollama" trap: when config
    /// already names the model under a cloud provider, the local-Ollama path
    /// must not claim it.
    #[test]
    fn configured_cloud_models_are_not_ollama_pull_candidates() {
        let cfg = config_with_default(
            "openai",
            &[
                (
                    "openai",
                    make_llm_cfg("openai", Some("gpt-4o"), Some("k"), None),
                ),
                (
                    "local",
                    make_llm_cfg("ollama", Some("qwen3.8:latest"), None, None),
                ),
            ],
        );
        assert!(model_belongs_to_configured_cloud_provider(&cfg, "gpt-4o"));
        // An Ollama-provider entry is never a reason to skip the probe.
        assert!(!model_belongs_to_configured_cloud_provider(
            &cfg,
            "qwen3.8:latest"
        ));
        // Unknown names stay eligible.
        assert!(!model_belongs_to_configured_cloud_provider(&cfg, "qwen3.8"));
    }

    /// `select_explicit_provider` must land on the same table. Ollama is the
    /// only kind constructible without an API key, so it is the one arm that
    /// can be exercised without touching env or config.
    #[test]
    fn select_explicit_provider_uses_the_default_table_for_ollama() {
        let cfg = AppConfig::default();
        let (name, model, _) =
            select_explicit_provider(&cfg, "ollama", None).expect("ollama needs no credential");
        assert_eq!(name, "ollama");
        assert_eq!(model, "qwen3.8:latest");
    }

    // ─── stream_turn (regression: bounded-channel deadlock, GAR chat hang) ─

    /// A producer that pushes far more deltas than the channel capacity used
    /// to deadlock forever: the old REPL only drained AFTER the call
    /// completed, so `send().await` wedged once the buffer filled. The
    /// 5s outer timeout turns a regression into a failure instead of a hang.
    #[tokio::test]
    async fn stream_turn_drains_concurrently_without_deadlock() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(2);
        let call = async move {
            for i in 0..300 {
                tx.send(format!("d{i} "))
                    .await
                    .map_err(|_| "receiver dropped")?;
            }
            Ok::<String, &'static str>("full".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        // Com o spinner LIGADO de propósito: o braço extra do `select!` não
        // pode roubar a vez da drenagem e re-introduzir o deadlock original.
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_secs(30),
                &mut out,
                test_spinner(),
                "",
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("stream_turn must not deadlock with >capacity deltas");

        assert_eq!(outcome, TurnOutcome::Done(Ok("full".to_string())));
        let printed = strip_spinner(&String::from_utf8(out).expect("utf8"));
        assert!(printed.starts_with("d0 "));
        assert!(printed.ends_with("d299 "));
        assert_eq!(printed.matches(' ').count(), 300);
    }

    /// On timeout the call future must be dropped (closing the sender) and
    /// already-buffered deltas must still be flushed before returning None.
    #[tokio::test]
    async fn stream_turn_times_out_and_flushes_buffered_deltas() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            tx.send("partial ".to_string())
                .await
                .map_err(|_| "receiver dropped")?;
            // Never completes: simulates a stalled provider/SSE stream.
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_millis(50),
                &mut out,
                None,
                "",
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("timeout path must not hang");

        assert_eq!(outcome, TurnOutcome::TimedOut);
        assert_eq!(String::from_utf8(out).expect("utf8"), "partial ");
    }

    // ---- Ajuda para os testes do indicador de atividade -------------------

    /// Spinner determinístico para teste: estilo ASCII (comparável byte a byte)
    /// e largura fixa, sem depender do terminal do runner de CI.
    fn test_spinner() -> Option<crate::spinner::Spinner> {
        Some(crate::spinner::Spinner::new(
            crate::spinner::SpinnerStyle::Ascii,
            80,
            0,
        ))
    }

    /// Remove as sequências ANSI de `raw`, preservando os `\r`.
    fn strip_ansi(raw: &str) -> String {
        let mut out = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Consome o CSI inteiro (ESC '[' ... letra final).
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for t in chars.by_ref() {
                        if t.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    /// Devolve só o que o modelo escreveu, descartando as linhas do spinner.
    ///
    /// Cada quadro é reescrito a partir de um `\r`, então basta quebrar por
    /// `\r` e jogar fora os segmentos que começam com um desenho da garra.
    /// Descartar *segmentos inteiros* (em vez de só o glifo) é o que torna a
    /// asserção estável: o texto da mensagem — "Afiando as garras..." — sai
    /// junto, e um tick que ganhe a corrida do `select!` contra o primeiro
    /// delta não vira teste intermitente.
    fn strip_spinner(raw: &str) -> String {
        strip_ansi(raw)
            .split('\r')
            .filter(|segment| !ASCII_FRAMES.iter().any(|f| segment.starts_with(f)))
            .collect::<Vec<_>>()
            .join("")
    }

    /// Os quadros do estilo ASCII, que é o usado por `test_spinner`.
    const ASCII_FRAMES: [&str; 6] = ["<   >", "</  >", "<// >", "<///>", "< //>", "<  />"];

    /// Qualquer quadro da animação presente no texto cru?
    fn contains_spinner_frame(raw: &str) -> bool {
        ASCII_FRAMES.iter().any(|f| raw.contains(f))
    }

    /// O indicador tem de aparecer ANTES do primeiro token, que é exatamente a
    /// janela em que o REPL parecia congelado no `garra >`.
    #[tokio::test]
    async fn spinner_runs_before_the_first_token() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            // Latência de provedor: vários quadros cabem aqui antes do 1o token.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            tx.send("resposta".to_string())
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("resposta".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            test_spinner(),
            "garra > ",
            &tokio::sync::Notify::new(),
        )
        .await;

        assert_eq!(outcome, TurnOutcome::Done(Ok("resposta".to_string())));
        let raw = String::from_utf8(out).expect("utf8");
        assert!(
            contains_spinner_frame(&raw),
            "nenhum quadro desenhado durante a espera: {raw:?}"
        );
        let first_frame = raw.find("<").expect("quadro presente");
        let first_token = raw.find("resposta").expect("token presente");
        assert!(
            first_frame < first_token,
            "o spinner tem de vir antes do primeiro token"
        );
    }

    /// A linha do spinner é apagada antes do texto, e nenhum quadro sobrevive
    /// depois que a resposta começa a sair.
    #[tokio::test]
    async fn spinner_is_cleared_before_streamed_output() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            tx.send("alpha ".to_string())
                .await
                .map_err(|_| "receiver dropped")?;
            tx.send("beta".to_string())
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("alpha beta".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            test_spinner(),
            "garra > ",
            &tokio::sync::Notify::new(),
        )
        .await;

        let raw = String::from_utf8(out).expect("utf8");
        let clear_at = raw.find("\r\x1b[2K").expect("sequência de limpeza emitida");
        let token_at = raw.find("alpha").expect("token presente");
        assert!(clear_at < token_at, "limpa a linha antes de escrever");

        // Depois do primeiro token não pode haver mais nenhum quadro: a linha
        // agora pertence à resposta e um quadro colidiria com ela.
        assert!(
            !contains_spinner_frame(&raw[token_at..]),
            "quadro desenhado depois do primeiro token: {:?}",
            &raw[token_at..]
        );

        // O texto do modelo sai íntegro e o rótulo aparece exatamente uma vez.
        let cleaned = strip_spinner(&raw);
        assert!(cleaned.contains("garra > alpha beta"), "saiu: {cleaned:?}");
        assert_eq!(cleaned.matches("garra >").count(), 1);
    }

    /// Erro do provedor precisa limpar a animação — nada de spinner órfão.
    #[tokio::test]
    async fn spinner_is_cleaned_up_on_provider_error() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            drop(tx);
            Err::<String, &'static str>("provider exploded")
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            test_spinner(),
            "garra > ",
            &tokio::sync::Notify::new(),
        )
        .await;

        assert_eq!(outcome, TurnOutcome::Done(Err("provider exploded")));
        let raw = String::from_utf8(out).expect("utf8");
        assert!(raw.contains("\r\x1b[2K"), "a linha tem de ser apagada");
        assert!(
            raw.ends_with("\r\x1b[2Kgarra > "),
            "limpa a linha e ainda assim emite o rótulo: {raw:?}"
        );
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
    }

    /// Timeout também limpa.
    #[tokio::test]
    async fn spinner_is_cleaned_up_on_timeout() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            let _keep = tx;
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_millis(300),
                &mut out,
                test_spinner(),
                "garra > ",
                &tokio::sync::Notify::new(),
            ),
        )
        .await
        .expect("timeout path must not hang");

        assert_eq!(outcome, TurnOutcome::TimedOut);
        let raw = String::from_utf8(out).expect("utf8");
        assert!(contains_spinner_frame(&raw), "girou durante a espera");
        assert!(raw.contains("\r\x1b[2K"), "limpou ao estourar o tempo");
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
    }

    /// Ctrl+C durante a espera: cancela o turno, limpa a animação e devolve o
    /// prompt — sem matar o processo e sem deixar o cursor escondido.
    #[tokio::test]
    async fn cancellation_stops_and_cleans_up_the_spinner() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            let _keep = tx;
            // Provedor que nunca responde: só o cancelamento tira a gente daqui.
            std::future::pending::<()>().await;
            Ok::<String, &'static str>("unreachable".to_string())
        };

        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = std::sync::Arc::clone(&cancel);
        tokio::spawn(async move {
            // Tempo para alguns quadros aparecerem antes do "Ctrl+C".
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            signal.notify_waiters();
        });

        let mut out: Vec<u8> = Vec::new();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream_turn(
                call,
                rx,
                std::time::Duration::from_secs(30),
                &mut out,
                test_spinner(),
                "garra > ",
                &cancel,
            ),
        )
        .await
        .expect("cancelamento não pode travar");

        assert_eq!(outcome, TurnOutcome::Cancelled);
        let raw = String::from_utf8(out).expect("utf8");
        assert!(contains_spinner_frame(&raw), "girou antes de cancelar");
        assert!(raw.contains("\r\x1b[2K"), "limpou a linha ao cancelar");
        assert!(!raw.contains("\x1b[?25l"), "nunca esconde o cursor");
        assert!(raw.ends_with("\r\x1b[2Kgarra > "), "termina limpo: {raw:?}");
    }

    /// Superfície não-TTY (stdout redirecionado / pipe): `detect` devolve
    /// `None` e a saída tem de ser byte a byte igual à de antes do spinner.
    #[tokio::test]
    async fn non_tty_output_contains_no_animation_frames() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            tx.send("resposta limpa".to_string())
                .await
                .map_err(|_| "receiver dropped")?;
            Ok::<String, &'static str>("resposta limpa".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            None, // exatamente o que `spinner::detect` devolve num pipe
            "garra > ",
            &tokio::sync::Notify::new(),
        )
        .await;

        let raw = String::from_utf8(out).expect("utf8");
        assert_eq!(raw, "garra > resposta limpa");
        assert!(!contains_spinner_frame(&raw));
        assert!(
            !raw.contains('\r'),
            "sem carriage return em saída redirecionada"
        );
        assert!(
            !raw.contains('\x1b'),
            "sem escape ANSI em saída redirecionada"
        );
    }

    /// O rótulo sai uma única vez mesmo com muitos deltas — regressão contra
    /// duplicação de linha quando o provedor entrega token a token.
    #[tokio::test]
    async fn prefix_is_written_exactly_once_across_many_deltas() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
        let call = async move {
            for i in 0..50 {
                tx.send(format!("t{i}"))
                    .await
                    .map_err(|_| "receiver dropped")?;
            }
            Ok::<String, &'static str>("done".to_string())
        };

        let mut out: Vec<u8> = Vec::new();
        let _ = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            test_spinner(),
            "garra > ",
            &tokio::sync::Notify::new(),
        )
        .await;

        let cleaned = strip_spinner(&String::from_utf8(out).expect("utf8"));
        assert_eq!(cleaned.matches("garra >").count(), 1);
        assert!(cleaned.contains("garra > t0t1t2"));
    }

    /// Errors from the call are passed through untouched.
    #[tokio::test]
    async fn stream_turn_propagates_call_error() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        let call = async move {
            drop(tx);
            Err::<String, &'static str>("provider exploded")
        };

        let mut out: Vec<u8> = Vec::new();
        let outcome = stream_turn(
            call,
            rx,
            std::time::Duration::from_secs(5),
            &mut out,
            None,
            "",
            &tokio::sync::Notify::new(),
        )
        .await;
        assert_eq!(outcome, TurnOutcome::Done(Err("provider exploded")));
        assert!(out.is_empty());
    }
}
