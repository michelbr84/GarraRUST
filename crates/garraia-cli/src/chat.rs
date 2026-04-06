//! GarraIA interactive chat REPL.
//!
//! `garraia chat` or just `garra` opens a local-first AI assistant
//! that streams responses from Ollama (offline) or cloud providers (online).

use std::io::{self, BufRead, Write as _};
use std::sync::Arc;

use anyhow::{Context, Result};
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, ChatMessage, ChatRole, FileReadTool,
    FileWriteTool, LlmProvider, MessagePart, OllamaProvider, OpenAiProvider,
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
    println!("{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}");
    println!("{CYAN}{BOLD}│{RESET}      {YELLOW}{BOLD}_~^~^~_{RESET}                                {CYAN}{BOLD}│{RESET}");
    println!("{CYAN}{BOLD}│{RESET}   {YELLOW}{BOLD}\\) /  o o  \\ (/{RESET}   {GREEN}{BOLD}GarraIA v{version}{RESET}         {CYAN}{BOLD}│{RESET}");
    println!("{CYAN}{BOLD}│{RESET}     {YELLOW}{BOLD}'_   -   _'{RESET}    Personal AI Assistant   {CYAN}{BOLD}│{RESET}");
    println!("{CYAN}{BOLD}│{RESET}     {YELLOW}{BOLD}/ '-----' \\{RESET}                            {CYAN}{BOLD}│{RESET}");
    println!("{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}");
    println!(
        "{CYAN}{BOLD}│{RESET}  {DIM}Provider:{RESET} {GREEN}{provider:<15}{RESET} {DIM}Mode:{RESET} {GREEN}{mode:<8}{RESET}  {CYAN}{BOLD}│{RESET}"
    );
    println!(
        "{CYAN}{BOLD}│{RESET}  {DIM}Model:{RESET}    {GREEN}{model:<33}{RESET} {CYAN}{BOLD}│{RESET}"
    );
    println!("{CYAN}{BOLD}│{RESET}                                              {CYAN}{BOLD}│{RESET}");
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

/// Detect which provider to use based on config and availability.
pub async fn detect_provider(
    config: &AppConfig,
    url_override: Option<&str>,
) -> (String, String, Arc<dyn LlmProvider>) {
    // 0. If a custom URL is provided, use OpenAI-compatible provider (LM Studio, vLLM, etc.)
    if let Some(url) = url_override {
        let base = url.trim_end_matches('/').to_string();
        // Use "not-needed" as API key — local servers don't require auth
        let key = std::env::var("LLM_API_KEY").unwrap_or_else(|_| "not-needed".to_string());
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
            Arc::new(provider),
        );
    }

    // 1. Try Ollama first (local, offline)
    let ollama_url = std::env::var("OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let ollama = OllamaProvider::new(None, Some(ollama_url.clone()));
    if ollama.health_check().await.unwrap_or(false) {
        let model = ollama
            .configured_model()
            .unwrap_or("llama3.1")
            .to_string();
        return (
            "ollama".to_string(),
            model,
            Arc::new(ollama),
        );
    }

    // 2. Try Anthropic (cloud)
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            let model = config
                .llm
                .get("anthropic")
                .and_then(|c| c.model.as_deref())
                .unwrap_or("claude-sonnet-4-5-20250929")
                .to_string();
            let provider = AnthropicProvider::new(&key, Some(model.clone()), None);
            return ("anthropic".to_string(), model, Arc::new(provider));
        }
    }

    // 3. Try OpenAI (cloud)
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            let model = config
                .llm
                .get("openai")
                .and_then(|c| c.model.as_deref())
                .unwrap_or("gpt-4o")
                .to_string();
            let provider = OpenAiProvider::new(&key, Some(model.clone()), None);
            return ("openai".to_string(), model, Arc::new(provider));
        }
    }

    // 4. Try OpenRouter (cloud fallback)
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        if !key.is_empty() {
            let model = config
                .llm
                .get("openrouter")
                .and_then(|c| c.model.as_deref())
                .unwrap_or("anthropic/claude-sonnet-4-5")
                .to_string();
            let provider = OpenAiProvider::new(
                &key,
                Some(model.clone()),
                Some("https://openrouter.ai/api/v1".to_string()),
            );
            return ("openrouter".to_string(), model, Arc::new(provider));
        }
    }

    // 5. Fallback: Ollama with no health check (user will see error on first message)
    let ollama = OllamaProvider::new(None, Some(ollama_url));
    let model = ollama
        .configured_model()
        .unwrap_or("llama3.1")
        .to_string();
    (
        "ollama (offline)".to_string(),
        model,
        Arc::new(ollama),
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
        match p.as_str() {
            "ollama" => {
                let ollama = OllamaProvider::new(model_override.clone(), None);
                model_name = model_override.unwrap_or_else(|| "llama3.1".to_string());
                provider_name = "ollama".to_string();
                provider = Arc::new(ollama);
            }
            "anthropic" => {
                let key = std::env::var("ANTHROPIC_API_KEY")
                    .context("ANTHROPIC_API_KEY not set")?;
                let model = model_override.unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
                let ap = AnthropicProvider::new(&key, Some(model.clone()), None);
                model_name = model;
                provider_name = "anthropic".to_string();
                provider = Arc::new(ap);
            }
            "openai" => {
                let key = std::env::var("OPENAI_API_KEY")
                    .context("OPENAI_API_KEY not set")?;
                let model = model_override.unwrap_or_else(|| "gpt-4o".to_string());
                let op = OpenAiProvider::new(&key, Some(model.clone()), None);
                model_name = model;
                provider_name = "openai".to_string();
                provider = Arc::new(op);
            }
            other => {
                anyhow::bail!("Provider desconhecido: {other}. Use: ollama, anthropic, openai");
            }
        }
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
                println!("{DIM}Para trocar provider, reinicie com: garraia chat --provider {new_provider}{RESET}");
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
                    println!(
                        "{DIM}Dica: Ollama nao esta rodando. Inicie com: ollama serve{RESET}"
                    );
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
