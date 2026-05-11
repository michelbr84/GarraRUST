//! GarraIA interactive chat REPL.
//!
//! `garraia chat` or just `garra` opens a local-first AI assistant
//! that streams responses from Ollama (offline) or cloud providers (online).

use std::io::{self, BufRead, Write as _};
use std::sync::Arc;

use anyhow::{Context, Result};
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, ChatMessage, ChatRole, FileReadTool, FileWriteTool,
    LlmProvider, MessagePart, OllamaProvider, OpenAiProvider, tools::git_diff_tool::GitDiffTool,
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
fn hardcoded_default_model(provider_kind: &str) -> String {
    match provider_kind {
        "ollama" => "llama3.1",
        "anthropic" => "claude-sonnet-4-5-20250929",
        "openai" => "gpt-4o",
        "openrouter" => "openrouter/auto",
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
            let op = OpenAiProvider::new(&key, Some(model.to_string()), Some(base));
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
                .unwrap_or_else(|| "llama3.1".to_string());
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
                .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
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
                .unwrap_or_else(|| "gpt-4o".to_string());
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
                .unwrap_or_else(|| "openrouter/auto".to_string());
            let op = OpenAiProvider::new(
                &key,
                Some(model.clone()),
                Some("https://openrouter.ai/api/v1".to_string()),
            );
            Ok((
                "openrouter".to_string(),
                model,
                Arc::new(op) as Arc<dyn LlmProvider>,
            ))
        }
        other => anyhow::bail!(
            "Provider desconhecido: {other}. Use: ollama, anthropic, openai, openrouter"
        ),
    }
}

/// Detect which provider to use based on config and availability.
pub async fn detect_provider(
    config: &AppConfig,
    url_override: Option<&str>,
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

        // Try to detect available models
        let model = match provider.available_models().await {
            Ok(models) if !models.is_empty() => models[0].clone(),
            _ => "default".to_string(),
        };
        return (
            format!("lmstudio ({})", base),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
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

    // 1. Try Ollama first (local, offline)
    let ollama_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());

    let ollama = OllamaProvider::new(None, Some(ollama_url.clone()));
    if ollama.health_check().await.unwrap_or(false) {
        let model = ollama.configured_model().unwrap_or("llama3.1").to_string();
        return (
            "ollama".to_string(),
            model,
            Arc::new(ollama) as Arc<dyn LlmProvider>,
        );
    }

    // 2. Try Anthropic (cloud)
    if let Some(key) = get_api_key(config, "anthropic", "ANTHROPIC_API_KEY") {
        let model = config
            .llm
            .get("anthropic")
            .and_then(|c| c.model.as_deref())
            .unwrap_or("claude-sonnet-4-5-20250929")
            .to_string();
        let provider = AnthropicProvider::new(&key, Some(model.clone()), None);
        return (
            "anthropic".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 3. Try OpenAI (cloud)
    if let Some(key) = get_api_key(config, "openai", "OPENAI_API_KEY") {
        let model = config
            .llm
            .get("openai")
            .and_then(|c| c.model.as_deref())
            .unwrap_or("gpt-4o")
            .to_string();
        let provider = OpenAiProvider::new(&key, Some(model.clone()), None);
        return (
            "openai".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 4. Try OpenRouter (cloud fallback)
    if let Some(key) = get_api_key(config, "openrouter", "OPENROUTER_API_KEY") {
        let model = config
            .llm
            .get("openrouter")
            .and_then(|c| c.model.as_deref())
            .unwrap_or("openrouter/auto")
            .to_string();
        let provider = OpenAiProvider::new(
            &key,
            Some(model.clone()),
            Some("https://openrouter.ai/api/v1".to_string()),
        );
        return (
            "openrouter".to_string(),
            model,
            Arc::new(provider) as Arc<dyn LlmProvider>,
        );
    }

    // 5. Fallback: Ollama with no health check (user will see error on first message)
    let ollama = OllamaProvider::new(None, Some(ollama_url));
    let model = ollama.configured_model().unwrap_or("llama3.1").to_string();
    (
        "ollama (offline)".to_string(),
        model,
        Arc::new(ollama) as Arc<dyn LlmProvider>,
    )
}

/// Run the interactive chat REPL.
pub async fn run_chat(
    config: AppConfig,
    provider_override: Option<String>,
    model_override: Option<String>,
    url_override: Option<String>,
) -> Result<()> {
    // Detect or use specified provider
    let (mut provider_name, mut model_name, mut provider) =
        detect_provider(&config, url_override.as_deref()).await;

    // Apply overrides
    if let Some(ref p) = provider_override {
        // GAR-579: shared with `garra ask` — the explicit-provider path
        // now lives in `select_explicit_provider` so chat and ask agree
        // byte-for-byte on construction + model resolution + error msgs.
        let (name, model, prov) =
            select_explicit_provider(&config, p.as_str(), model_override.as_deref())?;
        provider_name = name;
        model_name = model;
        provider = prov;
    } else if let Some(ref m) = model_override {
        model_name = m.clone();
    }

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
                let new_model = input[7..].trim().to_string();
                if new_model.is_empty() {
                    println!("{DIM}Uso: /model <nome>{RESET}");
                } else {
                    model_name = new_model;
                    println!("{DIM}Modelo alterado para: {model_name}{RESET}");
                }
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
                continue;
            }
            _ => {}
        }

        // Add user message to history
        history.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(input.clone()),
        });

        // Stream response
        print!("{CYAN}{BOLD}garra >{RESET} ");
        io::stdout().flush()?;

        let (tx, mut rx) = mpsc::channel::<String>(100);

        let history_clone = history.clone();
        let session_clone = session_id.clone();
        let model_clone = model_name.clone();
        let runtime_ref = &runtime;

        // Spawn streaming task
        let result = tokio::select! {
            result = runtime_ref.process_message_streaming(
                &session_clone,
                &input,
                &history_clone,
                tx,
                Some(&model_clone),
            ) => result,
        };

        // Drain any remaining deltas from the channel
        while let Ok(delta) = rx.try_recv() {
            print!("{delta}");
            io::stdout().flush()?;
        }

        match result {
            Ok(full_response) => {
                // Print any remaining text not sent via streaming
                println!();

                // Add assistant response to history
                history.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Text(full_response),
                });
            }
            Err(e) => {
                println!("\n{YELLOW}Erro: {e}{RESET}");

                // Remove the failed user message
                history.pop();

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
}
