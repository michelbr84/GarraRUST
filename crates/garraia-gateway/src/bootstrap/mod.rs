use std::sync::Arc;

use garraia_agents::tools::Tool;
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, CohereEmbeddingProvider, FileReadTool,
    FileWriteTool, McpManager, OllamaEmbeddingProvider, OllamaProvider, OpenAiEmbeddingProvider,
    OpenAiProvider, WebFetchTool, WebSearchTool,
};
use garraia_config::AppConfig;
use garraia_db::MemoryStore;
use tracing::{info, warn};

mod channels;
mod config;
mod discord;
#[cfg(target_os = "macos")]
mod imessage;
mod slack;
mod telegram;
mod whatsapp;

// Slice 10.a (GAR-440): path resolvers and API-key precedence chain extracted
// to `bootstrap::config`. Re-exported at this level so external paths
// `crate::bootstrap::default_vault_path` and `crate::bootstrap::resolve_api_key`
// stay valid (consumed by `admin::handlers`, `router`, `state`).
pub(crate) use config::{default_vault_path, resolve_api_key};

// Slice 10.b (GAR-476): channel registry orchestrator extracted to `bootstrap::channels`.
pub use channels::build_channels;

// Slice 10.c (GAR-477): Discord wiring + command handler extracted to `bootstrap::discord`.
pub use discord::build_discord_channels;

// Slice 10.d (GAR-478): Slack wiring extracted to `bootstrap::slack`.
pub use slack::build_slack_channels;

// Slice 10.e (GAR-479): WhatsApp wiring extracted to `bootstrap::whatsapp`.
pub use whatsapp::build_whatsapp_channels;

// Slice 10.f (GAR-480): iMessage wiring extracted to `bootstrap::imessage` (macOS-only).
#[cfg(target_os = "macos")]
pub use imessage::build_imessage_channels;

// Slice 10.g (GAR-691): Telegram wiring + voice handler extracted to `bootstrap::telegram`.
pub use telegram::build_telegram_channels;

/// Build a fully-configured `AgentRuntime` from the application config.
pub fn build_agent_runtime(config: &AppConfig) -> AgentRuntime {
    let mut runtime = AgentRuntime::new();
    let mut unreachable_local_providers: Vec<String> = Vec::new();

    let llm_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            config.timeouts.llm.default_secs,
        ))
        .build()
        .unwrap_or_default();

    // --- LLM Providers ---
    for (name, llm_config) in &config.llm {
        match llm_config.provider.as_str() {
            "anthropic" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "ANTHROPIC_API_KEY",
                    "ANTHROPIC_API_KEY",
                );

                if let Some(key) = api_key {
                    let provider = AnthropicProvider::new(
                        key,
                        llm_config.model.clone(),
                        llm_config.base_url.clone(),
                    )
                    .with_client(llm_client.clone());
                    runtime.register_provider(Arc::new(provider));
                    info!("configured anthropic provider: {name}");
                } else {
                    warn!(
                        "skipping anthropic provider {name}: no API key (set api_key in config or ANTHROPIC_API_KEY env var)"
                    );
                }
            }
            "openai" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "OPENAI_API_KEY",
                    "OPENAI_API_KEY",
                );

                if let Some(key) = api_key {
                    // Health check for local OpenAI-compatible providers (LM Studio, vLLM, etc.)
                    if let Some(ref base_url) = llm_config.base_url
                        && (base_url.contains("localhost") || base_url.contains("127.0.0.1"))
                    {
                        let addr = base_url
                            .trim_start_matches("http://")
                            .trim_start_matches("https://")
                            .split('/')
                            .next()
                            .unwrap_or("localhost:1234");
                        let sock_addr = if addr.contains(':') {
                            addr.to_string()
                        } else {
                            format!("{}:1234", addr)
                        };
                        match std::net::TcpStream::connect_timeout(
                            &sock_addr.parse().unwrap_or_else(|_| {
                                std::net::SocketAddr::from(([127, 0, 0, 1], 1234))
                            }),
                            std::time::Duration::from_secs(2),
                        ) {
                            Ok(_) => {
                                info!("Local OpenAI provider '{name}' reachable at {base_url}");
                            }
                            Err(e) => {
                                warn!(
                                    "Local OpenAI provider '{name}' not reachable at {base_url} \
                                     ({}). Provider registered but may fail. \
                                     Start the local server or switch default_provider in config.",
                                    e
                                );
                                unreachable_local_providers.push(name.clone());
                            }
                        }
                    }

                    let provider = OpenAiProvider::new(
                        key,
                        llm_config.model.clone(),
                        llm_config.base_url.clone(),
                    )
                    .with_client(llm_client.clone());
                    runtime.register_provider(Arc::new(provider));
                    info!("configured openai provider: {name}");
                } else {
                    warn!(
                        "skipping openai provider {name}: no API key (set api_key in config or OPENAI_API_KEY env var)"
                    );
                }
            }
            "ollama" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string());

                // Health check: quick TCP connectivity test to Ollama
                let addr = base_url
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_end_matches('/');
                let sock_addr = if addr.contains(':') {
                    addr.to_string()
                } else {
                    format!("{}:11434", addr)
                };
                match std::net::TcpStream::connect_timeout(
                    &sock_addr
                        .parse()
                        .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 11434))),
                    std::time::Duration::from_secs(2),
                ) {
                    Ok(_) => {
                        info!(
                            "✅ Ollama reachable at {} — registering provider: {name}",
                            base_url
                        );
                    }
                    Err(e) => {
                        warn!(
                            "⚠️  Ollama not reachable at {} — provider registered but will fail until Ollama starts. \
                             Error: {}. Run: ollama serve",
                            base_url, e
                        );
                    }
                }

                let provider =
                    OllamaProvider::new(llm_config.model.clone(), llm_config.base_url.clone())
                        .with_client(llm_client.clone());
                runtime.register_provider(Arc::new(provider));
                info!("configured ollama provider: {name}");
            }
            "sansa" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "SANSA_API_KEY",
                    "SANSA_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.sansaml.com".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("sansa-auto".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("sansa");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured sansa provider: {name}");
                } else {
                    warn!(
                        "skipping sansa provider {name}: no API key (set api_key in config or SANSA_API_KEY env var)"
                    );
                }
            }
            "deepseek" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "DEEPSEEK_API_KEY",
                    "DEEPSEEK_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.deepseek.com".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("deepseek-chat".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("deepseek");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured deepseek provider: {name}");
                } else {
                    warn!(
                        "skipping deepseek provider {name}: no API key (set api_key in config or DEEPSEEK_API_KEY env var)"
                    );
                }
            }
            "mistral" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "MISTRAL_API_KEY",
                    "MISTRAL_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.mistral.ai".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("mistral-large-latest".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("mistral");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured mistral provider: {name}");
                } else {
                    warn!(
                        "skipping mistral provider {name}: no API key (set api_key in config or MISTRAL_API_KEY env var)"
                    );
                }
            }
            "gemini" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "GEMINI_API_KEY",
                    "GEMINI_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config.base_url.clone().or_else(|| {
                        Some("https://generativelanguage.googleapis.com/v1beta/openai/".to_string())
                    });
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("gemini-2.5-flash".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("gemini");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured gemini provider: {name}");
                } else {
                    warn!(
                        "skipping gemini provider {name}: no API key (set api_key in config or GEMINI_API_KEY env var)"
                    );
                }
            }
            "falcon" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "FALCON_API_KEY",
                    "FALCON_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.ai71.ai/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("tiiuae/falcon-180b-chat".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("falcon");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured falcon provider: {name}");
                } else {
                    warn!(
                        "skipping falcon provider {name}: no API key (set api_key in config or FALCON_API_KEY env var)"
                    );
                }
            }
            "jais" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "JAIS_API_KEY",
                    "JAIS_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.core42.ai/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("jais-adapted-70b-chat".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("jais");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured jais provider: {name}");
                } else {
                    warn!(
                        "skipping jais provider {name}: no API key (set api_key in config or JAIS_API_KEY env var)"
                    );
                }
            }
            "qwen" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "QWEN_API_KEY",
                    "QWEN_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config.base_url.clone().or_else(|| {
                        Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string())
                    });
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("qwen-plus".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("qwen");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured qwen provider: {name}");
                } else {
                    warn!(
                        "skipping qwen provider {name}: no API key (set api_key in config or QWEN_API_KEY env var)"
                    );
                }
            }
            "yi" => {
                let api_key =
                    resolve_api_key(llm_config.api_key.as_deref(), "YI_API_KEY", "YI_API_KEY");

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.lingyiwanwu.com/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("yi-large".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("yi");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured yi provider: {name}");
                } else {
                    warn!(
                        "skipping yi provider {name}: no API key (set api_key in config or YI_API_KEY env var)"
                    );
                }
            }
            "cohere" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "COHERE_API_KEY",
                    "COHERE_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.cohere.com/compatibility/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("command-r-plus".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("cohere");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured cohere provider: {name}");
                } else {
                    warn!(
                        "skipping cohere provider {name}: no API key (set api_key in config or COHERE_API_KEY env var)"
                    );
                }
            }
            "minimax" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "MINIMAX_API_KEY",
                    "MINIMAX_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.minimaxi.chat/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("MiniMax-Text-01".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("minimax");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured minimax provider: {name}");
                } else {
                    warn!(
                        "skipping minimax provider {name}: no API key (set api_key in config or MINIMAX_API_KEY env var)"
                    );
                }
            }
            "moonshot" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "MOONSHOT_API_KEY",
                    "MOONSHOT_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://api.moonshot.cn/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("kimi-k2-0711-preview".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("moonshot");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured moonshot provider: {name}");
                } else {
                    warn!(
                        "skipping moonshot provider {name}: no API key (set api_key in config or MOONSHOT_API_KEY env var)"
                    );
                }
            }
            "openrouter" => {
                let api_key = resolve_api_key(
                    llm_config.api_key.as_deref(),
                    "OPENROUTER_API_KEY",
                    "OPENROUTER_API_KEY",
                );

                if let Some(key) = api_key {
                    let base_url = llm_config
                        .base_url
                        .clone()
                        .or_else(|| Some("https://openrouter.ai/api/v1".to_string()));
                    let model = llm_config
                        .model
                        .clone()
                        .or_else(|| Some("openai/gpt-4o".to_string()));
                    let provider = OpenAiProvider::new(key, model, base_url)
                        .with_client(llm_client.clone())
                        .with_name("openrouter");
                    runtime.register_provider(Arc::new(provider));
                    info!("configured openrouter provider: {name}");
                } else {
                    warn!(
                        "skipping openrouter provider {name}: no API key (set api_key in config or OPENROUTER_API_KEY env var)"
                    );
                }
            }
            // Plan 0051 (GAR-444): deterministic echo provider for dev + CI
            // smoke tests. Gated by `dev-echo-provider` feature on
            // `garraia-gateway` (forwarded to `garraia-agents`). Default OFF —
            // the arm is not compiled in production release builds.
            #[cfg(feature = "dev-echo-provider")]
            "echo" => {
                let provider = garraia_agents::EchoProvider::new(llm_config.model.clone());
                runtime.register_provider(Arc::new(provider));
                info!("configured echo provider: {name} (dev-echo-provider feature)");
            }
            other => {
                warn!("unknown LLM provider type: {other}, skipping {name}");
            }
        }
    }

    // --- Provider Status Summary ---
    {
        let providers = runtime.provider_ids();
        let configured_count = config.llm.len();
        let active_count = providers.len();
        let skipped_count = configured_count.saturating_sub(active_count);

        info!("╔══════════════════════════════════════╗");
        info!("║       Provider Status Summary        ║");
        info!("╠══════════════════════════════════════╣");
        for (name, llm_config) in &config.llm {
            let provider_type = &llm_config.provider;
            let is_active = providers.iter().any(|p| p == name || p == provider_type);
            if is_active {
                info!("║  ✅ {:<15} ({:<12}) ║", name, provider_type);
            } else {
                info!("║  ⚠️  {:<15} DISABLED       ║", name);
            }
        }
        info!("╠══════════════════════════════════════╣");
        info!(
            "║  Total: {} active / {} configured     ║",
            active_count, configured_count
        );
        if skipped_count > 0 {
            info!("║  ⚠️  {} provider(s) skipped          ║", skipped_count);
        }
        info!("╚══════════════════════════════════════╝");
    }

    // --- Auto-fallback: if default_provider is unreachable, try another ---
    if let Some(ref default_id) = config.agent.default_provider
        && unreachable_local_providers.iter().any(|p| p == default_id)
    {
        let providers = runtime.provider_ids();
        let fallback = providers
            .iter()
            .find(|p| !unreachable_local_providers.contains(p));
        if let Some(fallback_id) = fallback {
            warn!(
                "Default provider '{}' is unreachable — auto-switching to '{}'",
                default_id, fallback_id
            );
            runtime.set_default_provider_id(fallback_id);
        } else {
            warn!(
                "Default provider '{}' is unreachable and no fallback available. \
                 Start the local server or add a cloud provider to config.",
                default_id
            );
        }
    }

    // --- Tools ---
    // GAR-187: use confirmation-enabled BashTool when config.agent.tool_confirmation_enabled
    let bash_tool = if config.agent.tool_confirmation_enabled {
        BashTool::new_with_confirmation(None)
    } else {
        BashTool::new(None)
    };
    runtime.register_tool(Box::new(bash_tool));
    runtime.register_tool(Box::new(FileReadTool::new(None)));
    runtime.register_tool(Box::new(FileWriteTool::new(None)));
    runtime.register_tool(Box::new(WebFetchTool::new(None)));

    // Web search (Brave Search API) — only registered when an API key is available
    let brave_config_key = config.llm.get("brave").and_then(|c| c.api_key.clone());
    if let Some(key) = resolve_api_key(
        brave_config_key.as_deref(),
        "BRAVE_API_KEY",
        "BRAVE_API_KEY",
    ) {
        runtime.register_tool(Box::new(WebSearchTool::new(key)));
    }

    // --- Memory ---
    if config.memory.enabled {
        let data_dir = config
            .data_dir
            .clone()
            .unwrap_or_else(|| garraia_config::ConfigLoader::default_config_dir().join("data"));

        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            warn!("failed to create data directory: {e}");
        }

        let memory_db_path = data_dir.join("memory.db");
        match MemoryStore::open(&memory_db_path) {
            Ok(store) => {
                let store = Arc::new(store);
                runtime.set_memory_provider(store);
                info!("memory store opened at {}", memory_db_path.display());

                // Attach embedding provider if configured
                if let Some(embed_name) = &config.memory.embedding_provider
                    && let Some(embed_config) = config.embeddings.get(embed_name)
                {
                    let embed_client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(
                            config.timeouts.llm.default_secs,
                        )) // Reuse llm timeout for embeddings
                        .build()
                        .unwrap_or_default();

                    match embed_config.provider.as_str() {
                        // =====================================
                        // COHERE
                        // =====================================
                        "cohere" => {
                            let api_key = resolve_api_key(
                                embed_config.api_key.as_deref(),
                                "COHERE_API_KEY",
                                "COHERE_API_KEY",
                            );

                            if let Some(key) = api_key {
                                let provider = CohereEmbeddingProvider::new(
                                    key,
                                    embed_config.model.clone(),
                                    embed_config.base_url.clone(),
                                )
                                .with_client(embed_client.clone());

                                runtime.set_embedding_provider(Arc::new(provider));

                                info!("configured cohere embedding provider: {embed_name}");
                            } else {
                                warn!("skipping cohere embedding provider: no API key");
                            }
                        }

                        // =====================================
                        // OLLAMA (ADICIONE ESTE BLOCO)
                        // =====================================
                        "ollama" => {
                            let provider = OllamaEmbeddingProvider::new(
                                embed_config.model.clone(),
                                embed_config.base_url.clone(),
                            )
                            .with_client(embed_client.clone());

                            runtime.set_embedding_provider(Arc::new(provider));

                            info!("configured ollama embedding provider: {embed_name}");
                        }

                        // =====================================
                        // OPENAI-COMPATIBLE (LM Studio, OpenAI, etc.)
                        // =====================================
                        "openai" => {
                            let api_key = embed_config
                                .api_key
                                .clone()
                                .unwrap_or_else(|| "no-key".to_string());

                            let provider = OpenAiEmbeddingProvider::new(
                                api_key,
                                embed_config.model.clone(),
                                embed_config.base_url.clone(),
                            )
                            .with_client(embed_client.clone());

                            runtime.set_embedding_provider(Arc::new(provider));
                            info!("configured openai embedding provider: {embed_name}");
                        }

                        // =====================================
                        // UNKNOWN
                        // =====================================
                        other => {
                            warn!("unknown embedding provider type: {other}");
                        }
                    }
                }
            }
            Err(e) => {
                warn!("failed to open memory store: {e}");
            }
        }
    }

    // --- Agent Config ---
    if let Some(prompt) = &config.agent.system_prompt {
        runtime.set_system_prompt(prompt.clone());
    }
    if let Some(max_tokens) = config.agent.max_tokens {
        runtime.set_max_tokens(max_tokens);
    }
    if let Some(max_tool_calls) = config.agent.max_tool_calls {
        runtime.set_max_tool_calls(max_tool_calls);
    }
    // GAR-210: wire fallback provider list from config
    if !config.agent.fallback_providers.is_empty() {
        runtime.set_fallback_providers(config.agent.fallback_providers.clone());
        info!(
            "provider fallback order: {:?}",
            config.agent.fallback_providers
        );
    }
    // GAR-208: wire context window / summarization policy
    {
        use garraia_agents::context_policy::ContextPolicy;
        let policy = ContextPolicy::new(
            config.agent.max_history_messages,
            config.agent.summarize_threshold,
        );
        if policy.max_history_messages.is_some() || policy.summarize_threshold.is_some() {
            info!(
                window = ?policy.max_history_messages,
                threshold = ?policy.summarize_threshold,
                summarizer_model = ?config.agent.summarizer_model,
                "context policy configured"
            );
        }
        runtime.set_context_policy(policy);
    }

    // Wire tools_model: model override used when tools are present (e.g. avoids openrouter/free
    // which may not support function calling).
    if let Some(ref tm) = config.agent.tools_model {
        runtime.set_tools_model(Some(tm.clone()));
        info!(tools_model = %tm, "tools_model configured for tool-capable requests");
    }

    // --- Skills ---
    let skills_dir = garraia_config::ConfigLoader::default_config_dir().join("skills");
    let scanner = garraia_skills::SkillScanner::new(&skills_dir);
    match scanner.discover() {
        Ok(skills) if !skills.is_empty() => {
            let mut skill_block = String::from("\n\n# Active Skills\n");
            for skill in &skills {
                skill_block.push_str(&format!(
                    "\n## {}\n{}\n",
                    skill.frontmatter.name, skill.frontmatter.description
                ));
                if !skill.frontmatter.triggers.is_empty() {
                    skill_block.push_str(&format!(
                        "Triggers: {}\n",
                        skill.frontmatter.triggers.join(", ")
                    ));
                }
                skill_block.push('\n');
                skill_block.push_str(&skill.body);
                skill_block.push('\n');
            }

            let new_prompt = match runtime.system_prompt() {
                Some(existing) => format!("{existing}{skill_block}"),
                None => skill_block,
            };
            runtime.set_system_prompt(new_prompt);
            info!("injected {} skill(s) into system prompt", skills.len());
        }
        Ok(_) => {} // no skills found
        Err(e) => warn!("failed to scan skills directory: {e}"),
    }

    // --- Facts (User Information) ---
    let facts_path = garraia_config::ConfigLoader::default_config_dir()
        .join("memoria")
        .join("fatos.json");
    if facts_path.exists() {
        match std::fs::read_to_string(&facts_path) {
            Ok(content) => {
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(facts) => {
                        // Validate: must be a JSON object with at least one key
                        if !facts.is_object() {
                            warn!(
                                path = %facts_path.display(),
                                "facts.json is not a JSON object, skipping — expected {{ \"nome\": ..., \"sobre\": ... }}"
                            );
                        } else if facts.as_object().is_none_or(|o| o.is_empty()) {
                            warn!(
                                path = %facts_path.display(),
                                "facts.json is an empty object {{}}, skipping injection"
                            );
                        } else {
                            // Build facts context for system prompt
                            let mut facts_context = String::from("\n\n# Fatos do Usuário\n");

                            // Nome
                            if let Some(nome) = facts.get("nome").and_then(|v| v.as_str())
                                && !nome.is_empty()
                            {
                                facts_context.push_str(&format!("Nome: {}\n", nome));
                            }
                            // Apelido
                            if let Some(apelido) = facts.get("apelido").and_then(|v| v.as_str())
                                && !apelido.is_empty()
                            {
                                facts_context.push_str(&format!("Apelido: {}\n", apelido));
                            }
                            // Sobre
                            if let Some(sobre) = facts.get("sobre").and_then(|v| v.as_str())
                                && !sobre.is_empty()
                            {
                                facts_context.push_str(&format!("Sobre: {}\n", sobre));
                            }
                            // Empresa
                            if let Some(empresa) = facts.get("empresa").and_then(|v| v.as_str())
                                && !empresa.is_empty()
                            {
                                facts_context.push_str(&format!("Empresa: {}\n", empresa));
                            }
                            // Cargo
                            if let Some(cargo) = facts.get("cargo").and_then(|v| v.as_str())
                                && !cargo.is_empty()
                            {
                                facts_context.push_str(&format!("Cargo: {}\n", cargo));
                            }
                            // Localização
                            if let Some(local) =
                                facts.get("localizacao").and_then(|v| v.as_object())
                            {
                                let mut parts = Vec::new();
                                if let Some(v) = local.get("cidade").and_then(|v| v.as_str())
                                    && !v.is_empty()
                                {
                                    parts.push(v.to_string());
                                }
                                if let Some(v) = local.get("estado").and_then(|v| v.as_str())
                                    && !v.is_empty()
                                {
                                    parts.push(v.to_string());
                                }
                                if let Some(v) = local.get("pais").and_then(|v| v.as_str())
                                    && !v.is_empty()
                                {
                                    parts.push(v.to_string());
                                }
                                if !parts.is_empty() {
                                    facts_context
                                        .push_str(&format!("Localização: {}\n", parts.join(", ")));
                                }
                            }
                            // Idiomas
                            if let Some(idioma) =
                                facts.get("idioma_principal").and_then(|v| v.as_str())
                            {
                                facts_context.push_str(&format!("Idioma principal: {}\n", idioma));
                            }
                            if let Some(idiomas) =
                                facts.get("idiomas_secundarios").and_then(|v| v.as_array())
                            {
                                let langs: Vec<&str> =
                                    idiomas.iter().filter_map(|v| v.as_str()).collect();
                                if !langs.is_empty() {
                                    facts_context.push_str(&format!(
                                        "Idiomas secundarios: {}\n",
                                        langs.join(", ")
                                    ));
                                }
                            }
                            // Preferências
                            if let Some(prefs) =
                                facts.get("preferencias").and_then(|v| v.as_object())
                            {
                                facts_context.push_str("Preferências:\n");
                                if let Some(v) = prefs.get("idioma").and_then(|v| v.as_str()) {
                                    facts_context.push_str(&format!("  - Idioma: {}\n", v));
                                }
                                if let Some(v) = prefs.get("tom").and_then(|v| v.as_str()) {
                                    facts_context.push_str(&format!("  - Tom: {}\n", v));
                                }
                                if let Some(v) = prefs.get("nivel_detalhe").and_then(|v| v.as_str())
                                {
                                    facts_context
                                        .push_str(&format!("  - Nivel de detalhe: {}\n", v));
                                }
                                if let Some(v) =
                                    prefs.get("formato_resposta").and_then(|v| v.as_str())
                                {
                                    facts_context.push_str(&format!("  - Formato: {}\n", v));
                                }
                            }
                            // Ambiente
                            if let Some(amb) = facts.get("ambiente").and_then(|v| v.as_object()) {
                                facts_context.push_str("Ambiente:\n");
                                if let Some(v) =
                                    amb.get("sistema_operacional").and_then(|v| v.as_str())
                                {
                                    facts_context.push_str(&format!("  - SO: {}\n", v));
                                }
                                if let Some(v) = amb.get("usa_ollama").and_then(|v| v.as_bool()) {
                                    facts_context.push_str(&format!(
                                        "  - Usa Ollama: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                                if let Some(v) = amb.get("usa_openrouter").and_then(|v| v.as_bool())
                                {
                                    facts_context.push_str(&format!(
                                        "  - Usa OpenRouter: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                                if let Some(v) =
                                    amb.get("usa_modelos_locais").and_then(|v| v.as_bool())
                                {
                                    facts_context.push_str(&format!(
                                        "  - Modelos locais: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                            }
                            // Interesses
                            if let Some(interesses) =
                                facts.get("interesses").and_then(|v| v.as_array())
                            {
                                let interesses: Vec<&str> =
                                    interesses.iter().filter_map(|v| v.as_str()).collect();
                                if !interesses.is_empty() {
                                    facts_context.push_str(&format!(
                                        "Interesses: {}\n",
                                        interesses.join(", ")
                                    ));
                                }
                            }
                            // Projetos
                            if let Some(projetos) = facts.get("projetos").and_then(|v| v.as_array())
                            {
                                let projetos: Vec<&str> =
                                    projetos.iter().filter_map(|v| v.as_str()).collect();
                                if !projetos.is_empty() {
                                    facts_context
                                        .push_str(&format!("Projetos: {}\n", projetos.join(", ")));
                                }
                            }
                            // Restrições
                            if let Some(rest) = facts.get("restricoes").and_then(|v| v.as_object())
                            {
                                facts_context.push_str("Restricoes:\n");
                                if let Some(v) = rest.get("nao_alucinar").and_then(|v| v.as_bool())
                                {
                                    facts_context.push_str(&format!(
                                        "  - Nao alucinar: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                                if let Some(v) =
                                    rest.get("priorizar_precisao").and_then(|v| v.as_bool())
                                {
                                    facts_context.push_str(&format!(
                                        "  - Priorizar precisao: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                                if let Some(v) = rest
                                    .get("priorizar_respostas_tecnicas")
                                    .and_then(|v| v.as_bool())
                                {
                                    facts_context.push_str(&format!(
                                        "  - Respostas tecnicas: {}\n",
                                        if v { "Sim" } else { "Nao" }
                                    ));
                                }
                            }
                            // Fatos importantes
                            if let Some(fatos) =
                                facts.get("fatos_importantes").and_then(|v| v.as_array())
                                && !fatos.is_empty()
                            {
                                facts_context.push_str("Fatos importantes:\n");
                                for fato in fatos {
                                    if let Some(f) = fato.as_str() {
                                        facts_context.push_str(&format!("- {}\n", f));
                                    }
                                }
                            }

                            // Inject facts into system prompt
                            let new_prompt = match runtime.system_prompt() {
                                Some(existing) => format!("{}\n{}", existing, facts_context),
                                None => facts_context.clone(),
                            };
                            runtime.set_system_prompt(new_prompt);
                            info!(
                                "loaded user facts from {} ({} keys, context len: {})",
                                facts_path.display(),
                                facts.as_object().map_or(0, |o| o.len()),
                                facts_context.len()
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            path = %facts_path.display(),
                            error = %e,
                            "facts.json contains invalid JSON, skipping — boot continues normally"
                        );
                    }
                }
            }
            Err(e) => {
                warn!("failed to read facts file: {}", e);
            }
        }
    }

    runtime
}

/// Build MCP tools from merged config (config.yml + mcp.json).
///
/// Returns the manager and a flat list of bridged tools ready for registration.
pub async fn build_mcp_tools(config: &AppConfig) -> (McpManager, Vec<Box<dyn Tool>>) {
    let loader = match garraia_config::ConfigLoader::new() {
        Ok(l) => l,
        Err(e) => {
            warn!("failed to create config loader for MCP: {e}");
            return (McpManager::new(), Vec::new());
        }
    };

    let mcp_configs = loader.merged_mcp_config(config);
    if mcp_configs.is_empty() {
        return (McpManager::new(), Vec::new());
    }

    let manager = McpManager::new();
    let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();

    for (name, server_config) in &mcp_configs {
        let enabled = server_config.enabled.unwrap_or(true);
        if !enabled {
            info!("MCP server '{name}' is disabled, skipping");
            continue;
        }

        let timeout_secs = server_config
            .timeout
            .unwrap_or(config.timeouts.mcp.default_secs);
        // GAR-293: resource limit config (with defaults).
        let memory_limit_mb = server_config.memory_limit_mb;
        let max_restarts = server_config.max_restarts.unwrap_or(5);
        let restart_delay_secs = server_config.restart_delay_secs.unwrap_or(5);

        let connect_result = match server_config.transport.as_str() {
            "stdio" => {
                manager
                    .connect(
                        name,
                        &server_config.command,
                        &server_config.args,
                        &server_config.env,
                        timeout_secs,
                        server_config.allowed_tools.clone(),
                        memory_limit_mb,
                        max_restarts,
                        restart_delay_secs,
                    )
                    .await
            }
            #[cfg(feature = "mcp-http")]
            "http" => {
                let Some(url) = &server_config.url else {
                    warn!(
                        "MCP server '{name}' uses HTTP transport but no 'url' configured, skipping"
                    );
                    continue;
                };
                manager
                    .connect_http(
                        name,
                        url,
                        timeout_secs,
                        server_config.allowed_tools.clone(),
                        max_restarts,
                        restart_delay_secs,
                    )
                    .await
            }
            other => {
                warn!("MCP server '{name}' uses unsupported transport '{other}', skipping");
                continue;
            }
        };

        match connect_result {
            Ok(()) => {
                let tools = manager
                    .take_tools(name, std::time::Duration::from_secs(timeout_secs))
                    .await;
                info!("MCP server '{name}': registered {} tool(s)", tools.len());
                all_tools.extend(tools);
            }
            Err(e) => {
                warn!("failed to connect MCP server '{name}': {e}");
            }
        }
    }

    (manager, all_tools)
}

/// Build a voice handler for Telegram that processes voice messages.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_agent_runtime_empty_config_no_crash() {
        let config = AppConfig::default();
        let _runtime = build_agent_runtime(&config);
        // Should succeed with no providers or tools crashing.
        // We do not assert `_runtime.system_prompt().is_none()` because
        // local skills in ~/.garraia/skills could be injected automatically.
    }

    #[test]
    fn build_agent_runtime_unknown_provider_skips_gracefully() {
        let mut config = AppConfig::default();
        config.llm.insert(
            "bad".to_string(),
            garraia_config::LlmProviderConfig {
                provider: "nonexistent-provider".to_string(),
                model: None,
                api_key: None,
                base_url: None,
                extra: std::collections::HashMap::new(),
            },
        );
        // Should not panic — unknown providers are logged and skipped
        let _runtime = build_agent_runtime(&config);
    }
}
